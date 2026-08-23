use std::env;
use std::error::Error;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use portable_pty::{Child, CommandBuilder, PtySize, native_pty_system};
use serde::{Deserialize, Serialize};

const TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_SAMPLES: usize = 5;
const MAX_SAMPLES: usize = 100;

fn main() -> ExitCode {
    match parse_arguments(env::args().skip(1)).and_then(|config| run(&config)) {
        Ok(report) => match publish(&report) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("terminal benchmark report failed: {error}");
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            eprintln!("terminal benchmark failed: {error}");
            ExitCode::FAILURE
        }
    }
}

#[derive(Debug)]
struct Config {
    executable: PathBuf,
    samples: usize,
}

fn parse_arguments(mut arguments: impl Iterator<Item = String>) -> Result<Config, Box<dyn Error>> {
    let mut executable = None;
    let mut samples = DEFAULT_SAMPLES;
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--executable" => {
                executable = Some(PathBuf::from(next_value(&mut arguments, "--executable")?));
            }
            "--samples" => {
                samples = next_value(&mut arguments, "--samples")?
                    .parse::<usize>()
                    .map_err(|_| invalid_input("--samples must be an integer"))?;
                if !(1..=MAX_SAMPLES).contains(&samples) {
                    return Err(invalid_input("--samples must be between 1 and 100"));
                }
            }
            "--help" | "-h" => {
                return Err(invalid_input(
                    "usage: terminal_latency --executable <instrumented-autoharness> [--samples N]",
                ));
            }
            _ => return Err(invalid_input(format!("unknown option: {argument}"))),
        }
    }
    let executable = executable.ok_or_else(|| invalid_input("--executable is required"))?;
    if !executable.is_file() {
        return Err(invalid_input("--executable must name a built binary"));
    }
    Ok(Config {
        executable,
        samples,
    })
}

fn next_value(
    arguments: &mut impl Iterator<Item = String>,
    option: &str,
) -> Result<String, Box<dyn Error>> {
    arguments
        .next()
        .ok_or_else(|| invalid_input(format!("{option} requires a value")))
}

fn invalid_input(message: impl Into<String>) -> Box<dyn Error> {
    Box::new(io::Error::new(io::ErrorKind::InvalidInput, message.into()))
}

#[derive(Clone, Copy, Debug, Serialize)]
struct Sample {
    process_start_to_first_draw_ns: u64,
    app_initialize_to_first_draw_ns: u64,
    input_to_provider_dispatch_ns: u64,
    provider_chunk_to_rendered_delta_ns: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
struct Distribution {
    minimum_ns: u64,
    median_ns: u64,
    p95_ns: u64,
    mean_ns: u64,
    maximum_ns: u64,
}

#[derive(Debug, Serialize)]
struct Report {
    schema_version: u16,
    generated_unix_ms: u128,
    sample_count: usize,
    marker_side_channel: &'static str,
    network_intervals: &'static str,
    process_start_to_first_draw: Distribution,
    app_initialize_to_first_draw: Distribution,
    input_to_provider_dispatch: Distribution,
    provider_chunk_to_rendered_delta: Distribution,
    samples: Vec<Sample>,
}

fn run(config: &Config) -> Result<Report, Box<dyn Error>> {
    let mut samples = Vec::with_capacity(config.samples);
    for number in 0..config.samples {
        samples.push(run_sample(&config.executable, number)?);
    }
    Ok(Report {
        schema_version: 1,
        generated_unix_ms: SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
        sample_count: samples.len(),
        marker_side_channel: "loopback UDP; process-start metric includes one datagram delivery",
        network_intervals: "not measured; loopback fixture time is excluded from harness metrics",
        process_start_to_first_draw: distribution(
            samples
                .iter()
                .map(|sample| sample.process_start_to_first_draw_ns),
        ),
        app_initialize_to_first_draw: distribution(
            samples
                .iter()
                .map(|sample| sample.app_initialize_to_first_draw_ns),
        ),
        input_to_provider_dispatch: distribution(
            samples
                .iter()
                .map(|sample| sample.input_to_provider_dispatch_ns),
        ),
        provider_chunk_to_rendered_delta: distribution(
            samples
                .iter()
                .map(|sample| sample.provider_chunk_to_rendered_delta_ns),
        ),
        samples,
    })
}

fn distribution(values: impl Iterator<Item = u64>) -> Distribution {
    let mut values = values.collect::<Vec<_>>();
    values.sort_unstable();
    let count = values.len();
    let median = values[(count - 1) / 2];
    let p95_index = ((count * 95).div_ceil(100)).saturating_sub(1);
    let sum = values.iter().map(|value| u128::from(*value)).sum::<u128>();
    Distribution {
        minimum_ns: values[0],
        median_ns: median,
        p95_ns: values[p95_index],
        mean_ns: u64::try_from(sum / count as u128).unwrap_or(u64::MAX),
        maximum_ns: values[count - 1],
    }
}

struct PtyMasterGuard {
    master: Option<Box<dyn portable_pty::MasterPty + Send>>,
}

impl PtyMasterGuard {
    fn new(master: Box<dyn portable_pty::MasterPty + Send>) -> Self {
        Self {
            master: Some(master),
        }
    }

    fn get(&self) -> &dyn portable_pty::MasterPty {
        self.master.as_deref().expect("live PTY master")
    }

    fn close(&mut self) {
        self.master.take();
    }
}

impl Drop for PtyMasterGuard {
    fn drop(&mut self) {
        #[cfg(windows)]
        if let Some(master) = self.master.take() {
            // Failed nested ConPTY startup can make ClosePseudoConsole block
            // forever while its cloned output reader is live. The benchmark
            // process owns these handles and releases them when it exits.
            std::mem::forget(master);
        }
    }
}

struct ChildGuard {
    child: Box<dyn Child + Send + Sync>,
    exited: bool,
}

impl ChildGuard {
    fn new(child: Box<dyn Child + Send + Sync>) -> Self {
        Self {
            child,
            exited: false,
        }
    }

    fn get(&mut self) -> &mut Box<dyn Child + Send + Sync> {
        &mut self.child
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if !self.exited {
            let _ = self.child.kill();
        }
    }
}

fn run_sample(executable: &Path, number: usize) -> Result<Sample, Box<dyn Error>> {
    let marker_socket = UdpSocket::bind("127.0.0.1:0")?;
    marker_socket.set_read_timeout(Some(TIMEOUT))?;
    let marker_address = marker_socket.local_addr()?;
    let router = RouterFixture::start()?;
    let root = TempRoot::new(number)?;

    let pty = native_pty_system();
    let pair = pty.openpty(PtySize {
        rows: 30,
        cols: 100,
        pixel_width: 0,
        pixel_height: 0,
    })?;
    let mut master = PtyMasterGuard::new(pair.master);
    let reader = master.get().try_clone_reader()?;
    let mut writer = master.get().take_writer()?;
    let output = spawn_reader(reader);
    let command = instrumented_command(
        executable,
        root.path(),
        &router.base_url,
        &marker_address.to_string(),
    );
    let process_started = Instant::now();
    let mut child = ChildGuard::new(pair.slave.spawn_command(command)?);

    let first_draw = receive_marker(&marker_socket, "first_draw_completed")?;
    let process_start_to_first_draw_ns = nanos(process_started.elapsed());
    wait_for_screen(&output, child.get(), 30, 100, |screen| {
        screen.contents().contains("AutoHarness")
    })?;

    writer.write_all(&[0x10])?;
    writer.flush()?;
    wait_for_screen(&output, child.get(), 30, 100, |screen| {
        let text = screen.contents();
        text.contains("Models") && text.contains("PTY Router")
    })?;
    writer.write_all(b"\rterminal latency probe")?;
    writer.write_all(&[0x13])?;
    writer.flush()?;

    let markers = receive_correlated_markers(&marker_socket)?;
    writer.write_all(&[0x03])?;
    writer.flush()?;
    wait_for_exit(child.get())?;
    child.exited = true;
    drop(writer);
    master.close();
    let requests = router.finish()?;
    if requests
        != [
            "GET /v1/models?limit=1000".to_owned(),
            "POST /v1/chat/completions".to_owned(),
        ]
    {
        return Err(Box::new(io::Error::other(format!(
            "unexpected fixture requests: {requests:?}"
        ))));
    }

    Ok(Sample {
        process_start_to_first_draw_ns,
        app_initialize_to_first_draw_ns: first_draw.elapsed_ns,
        input_to_provider_dispatch_ns: markers
            .dispatch
            .elapsed_ns
            .checked_sub(markers.input.elapsed_ns)
            .ok_or_else(|| invalid_input("dispatch marker preceded input marker"))?,
        provider_chunk_to_rendered_delta_ns: markers
            .rendered
            .elapsed_ns
            .checked_sub(markers.chunk.elapsed_ns)
            .ok_or_else(|| invalid_input("render marker preceded chunk marker"))?,
    })
}

fn nanos(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

fn instrumented_command(
    executable: &Path,
    data_dir: &Path,
    router_base_url: &str,
    marker_address: &str,
) -> CommandBuilder {
    let mut command = CommandBuilder::new(executable);
    for (name, value) in [
        ("AUTOHARNESS_DATA_DIR", data_dir.as_os_str()),
        ("AUTOHARNESS_WORKSPACE", data_dir.as_os_str()),
    ] {
        command.env(name, value);
    }
    for (name, value) in [
        ("AUTOHARNESS_PROVIDER", "router"),
        ("AUTOHARNESS_ROUTER_BASE_URL", router_base_url),
        ("AUTOHARNESS_ROUTER_API_KEY", "terminal-benchmark-secret"),
        ("AUTOHARNESS_ROUTER_PROJECT", "terminal-benchmark"),
        ("AUTOHARNESS_PROVIDER_TIMEOUT_MS", "5000"),
        ("AUTOHARNESS_PROVIDER_IDLE_TIMEOUT_MS", "5000"),
        ("AUTOHARNESS_PROVIDER_RETRY_ATTEMPTS", "1"),
        ("AUTOHARNESS_BENCHMARK_MARKER_ADDR", marker_address),
    ] {
        command.env(name, value);
    }
    command.env_remove("GEMINI_API_KEY");
    command
}

fn spawn_reader(mut reader: Box<dyn Read + Send>) -> Receiver<Vec<u8>> {
    let (sender, receiver) = std::sync::mpsc::channel();
    let _ = std::thread::spawn(move || {
        let mut buffer = [0_u8; 16_384];
        while let Ok(read @ 1..) = reader.read(&mut buffer) {
            if sender.send(buffer[..read].to_vec()).is_err() {
                break;
            }
        }
    });
    receiver
}

fn wait_for_screen(
    output: &Receiver<Vec<u8>>,
    child: &mut Box<dyn Child + Send + Sync>,
    rows: u16,
    columns: u16,
    condition: impl Fn(&vt100::Screen) -> bool,
) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + TIMEOUT;
    let mut parser = vt100::Parser::new(rows, columns, 0);
    loop {
        match output.try_recv() {
            Ok(chunk) => parser.process(&chunk),
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                return Err(Box::new(io::Error::other("terminal output closed")));
            }
        }
        if condition(parser.screen()) {
            return Ok(());
        }
        if let Some(status) = child.try_wait()? {
            return Err(Box::new(io::Error::other(format!(
                "AutoHarness exited before screen condition: {}",
                status.exit_code()
            ))));
        }
        if Instant::now() >= deadline {
            return Err(Box::new(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("screen condition timed out: {}", parser.screen().contents()),
            )));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[derive(Clone, Debug, Deserialize)]
struct Marker {
    marker: String,
    elapsed_ns: u64,
    correlation: Option<u64>,
    sequence: Option<u64>,
}

fn receive_marker(socket: &UdpSocket, expected: &str) -> Result<Marker, Box<dyn Error>> {
    loop {
        let marker = receive(socket)?;
        if marker.marker == expected {
            return Ok(marker);
        }
    }
}

struct CorrelatedMarkers {
    input: Marker,
    dispatch: Marker,
    chunk: Marker,
    rendered: Marker,
}

fn receive_correlated_markers(socket: &UdpSocket) -> Result<CorrelatedMarkers, Box<dyn Error>> {
    let input = receive_marker(socket, "input_accepted")?;
    let correlation = input
        .correlation
        .ok_or_else(|| invalid_input("input marker lacks correlation"))?;
    let mut dispatch = None;
    let mut chunk = None;
    let mut rendered = None;
    while dispatch.is_none() || chunk.is_none() || rendered.is_none() {
        let marker = receive(socket)?;
        if marker.correlation != Some(correlation) {
            continue;
        }
        match marker.marker.as_str() {
            "provider_dispatch_started" => dispatch = Some(marker),
            "provider_chunk_received" => chunk = Some(marker),
            "rendered_delta"
                if chunk
                    .as_ref()
                    .and_then(|received: &Marker| received.sequence)
                    == marker.sequence =>
            {
                rendered = Some(marker);
            }
            _ => {}
        }
    }
    Ok(CorrelatedMarkers {
        input,
        dispatch: dispatch.expect("checked dispatch marker"),
        chunk: chunk.expect("checked chunk marker"),
        rendered: rendered.expect("checked rendered marker"),
    })
}

fn receive(socket: &UdpSocket) -> Result<Marker, Box<dyn Error>> {
    let mut bytes = [0_u8; 2048];
    let read = socket.recv(&mut bytes)?;
    Ok(serde_json::from_slice(&bytes[..read])?)
}

fn wait_for_exit(child: &mut Box<dyn Child + Send + Sync>) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        if let Some(status) = child.try_wait()? {
            if status.success() {
                return Ok(());
            }
            return Err(Box::new(io::Error::other(format!(
                "AutoHarness exited with code {}",
                status.exit_code()
            ))));
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            return Err(Box::new(io::Error::new(
                io::ErrorKind::TimedOut,
                "AutoHarness did not exit",
            )));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

struct RouterFixture {
    base_url: String,
    thread: std::thread::JoinHandle<Result<Vec<String>, io::Error>>,
}

impl RouterFixture {
    fn start() -> Result<Self, Box<dyn Error>> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let base_url = format!("http://{}/", listener.local_addr()?);
        let thread = std::thread::spawn(move || {
            let bodies = [
                (
                    "application/json",
                    r#"{"data":[{"id":"terminal-model","name":"PTY Router","capabilities":{"chat":true,"streaming":true}}],"has_more":false}"#,
                ),
                (
                    "text/event-stream",
                    concat!(
                        "data: {\"choices\":[{\"delta\":{\"content\":\"benchmark delta\"},\"finish_reason\":\"stop\"}]}\n\n",
                        "data: [DONE]\n\n"
                    ),
                ),
            ];
            let mut requests = Vec::new();
            for (content_type, body) in bodies {
                let (mut socket, _) = listener.accept()?;
                requests.push(read_http_request(&mut socket)?);
                write!(
                    socket,
                    "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )?;
                socket.flush()?;
            }
            Ok(requests)
        });
        Ok(Self { base_url, thread })
    }

    fn finish(self) -> Result<Vec<String>, Box<dyn Error>> {
        self.thread
            .join()
            .map_err(|_| io::Error::other("router fixture panicked"))?
            .map_err(Into::into)
    }
}

fn read_http_request(socket: &mut TcpStream) -> Result<String, io::Error> {
    let mut bytes = Vec::new();
    let header_end = loop {
        if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
        let mut chunk = [0_u8; 4096];
        let read = socket.read(&mut chunk)?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "request headers ended early",
            ));
        }
        bytes.extend_from_slice(&chunk[..read]);
    };
    let headers = std::str::from_utf8(&bytes[..header_end])
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "request headers"))?;
    let request_line = headers
        .lines()
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "request line"))?
        .trim_end_matches(" HTTP/1.1")
        .to_owned();
    let content_length = headers
        .lines()
        .find_map(|line| {
            line.strip_prefix("content-length:")
                .or_else(|| line.strip_prefix("Content-Length:"))
        })
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(0);
    while bytes.len() < header_end.saturating_add(content_length) {
        let mut chunk = [0_u8; 4096];
        let read = socket.read(&mut chunk)?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "request body ended early",
            ));
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
    Ok(request_line)
}

struct TempRoot {
    path: PathBuf,
}

impl TempRoot {
    fn new(number: usize) -> Result<Self, io::Error> {
        let path = env::temp_dir().join(format!(
            "autoharness-terminal-benchmark-{}-{number}",
            std::process::id()
        ));
        if path.exists() {
            fs::remove_dir_all(&path)?;
        }
        fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn publish(report: &Report) -> Result<(), Box<dyn Error>> {
    let output = serde_json::to_string_pretty(report)?;
    if let Some(path) = env::var_os("AUTOHARNESS_TERMINAL_BENCHMARK_OUTPUT") {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(PathBuf::from(path))?;
        file.write_all(output.as_bytes())?;
        file.write_all(b"\n")?;
    } else {
        println!("{output}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distribution_uses_nearest_rank_p95_and_lower_median() {
        let values = [1, 2, 3, 4, 100];
        assert_eq!(
            distribution(values.into_iter()),
            Distribution {
                minimum_ns: 1,
                median_ns: 3,
                p95_ns: 100,
                mean_ns: 22,
                maximum_ns: 100,
            }
        );
    }

    #[test]
    fn report_keeps_network_measurement_out_of_harness_fields() {
        let sample = Sample {
            process_start_to_first_draw_ns: 10,
            app_initialize_to_first_draw_ns: 8,
            input_to_provider_dispatch_ns: 4,
            provider_chunk_to_rendered_delta_ns: 2,
        };
        let json = serde_json::to_value(sample).expect("sample JSON");
        assert!(json.get("network_latency_ns").is_none());
        assert_eq!(json["provider_chunk_to_rendered_delta_ns"], 2);
    }
}
