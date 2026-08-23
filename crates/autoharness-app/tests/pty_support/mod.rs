//! Shared pseudo-terminal harness for end-to-end release-hardening scenarios.
//!
//! Each scenario launches the real `autoharness` binary inside a ConPTY or
//! Unix PTY, drives it with raw key bytes, and asserts on parsed screen state
//! so the tests exercise the actual terminal backend, input pipeline, and
//! alternate-screen restoration exactly as an end user experiences them.
//!
//! Every scenario isolates application state through `AUTOHARNESS_DATA_DIR`
//! and clears provider environment variables so runs are credential-free and
//! deterministic without network access.
//!
//! Output arrives on a dedicated reader thread because a pseudo-terminal read
//! blocks while the application is idle; scenarios must never block on read.

use std::collections::HashMap;
use std::ffi::OsString;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use autoharness_domain::{
    AttemptId, CommandEnvelope, CommandId, CommandPayload, CorrelationId, DeliveryMode, EventId,
    InputId, ModelId, ModelRef, PromptText, ProviderId, ResponseText, SessionId, SessionTitle,
    TimestampMillis,
};
use autoharness_engine::{DurableEngine, EventMetadataSource, GeneratedEventMetadata};
use autoharness_store_sqlite::SqliteStore;

use portable_pty::native_pty_system;
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize};

/// Upper bound for one awaited screen condition before failing.
pub const SETTLE_TIMEOUT: Duration = Duration::from_secs(30);

/// A running AutoHarness process attached to a real pseudo-terminal.
///
/// Shared harness methods gain consumers across the scenario files added
/// throughout Phase 3.5, so intentionally-unused members are tolerated here.
#[allow(dead_code)]
pub struct PtySession {
    master: Option<Box<dyn MasterPty + Send>>,
    writer: Option<Box<dyn Write + Send>>,
    child: Box<dyn Child + Send>,
    parser: vt100::Parser,
    output: OutputFeed,
    data_dir: PathBuf,
    raw_output: Vec<u8>,
    cursor_queries_answered: usize,
}

/// Non-blocking view of pseudo-terminal output parsed into screen state.
struct OutputFeed {
    receiver: Receiver<Vec<u8>>,
    reader_thread: Option<JoinHandle<()>>,
    eof: bool,
}

struct SpawnedPty {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send>,
    output: OutputFeed,
}

/// Environment for one isolated scenario run.
pub struct ScenarioEnvironment {
    variables: HashMap<String, OsString>,
    data_dir: PathBuf,
    _directory: tempfile::TempDir,
}

#[allow(dead_code)]
pub struct RouterFixture {
    base_url: String,
    thread: Option<JoinHandle<Vec<String>>>,
}

#[allow(dead_code)]
impl RouterFixture {
    /// Starts a loopback OpenAI-compatible router with ordered stream bodies.
    pub fn start(stream_bodies: Vec<String>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind router fixture");
        let address = listener.local_addr().expect("router fixture address");
        let base_url = format!("http://{address}/");
        let thread = std::thread::spawn(move || {
            let model_body = r#"{"data":[{"id":"pty-router-model","name":"PTY Router","capabilities":{"chat":true,"streaming":true,"function_calling":true}}],"has_more":false}"#;
            let mut requests = Vec::with_capacity(stream_bodies.len() + 1);
            for (content_type, body) in std::iter::once(("application/json", model_body.to_owned()))
                .chain(
                    stream_bodies
                        .into_iter()
                        .map(|body| ("text/event-stream", body)),
                )
            {
                let (mut socket, _) = listener.accept().expect("router fixture request");
                requests.push(read_http_request(&mut socket));
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket
                    .write_all(response.as_bytes())
                    .expect("router fixture response");
            }
            requests
        });
        Self {
            base_url,
            thread: Some(thread),
        }
    }

    /// Applies the fixture's non-secret router settings to a scenario.
    pub fn configure(&self, environment: &mut ScenarioEnvironment) {
        environment.insert("AUTOHARNESS_PROVIDER", "router");
        environment.insert("AUTOHARNESS_ROUTER_BASE_URL", self.base_url.as_str());
        environment.insert("AUTOHARNESS_ROUTER_API_KEY", "pty-router-secret");
        environment.insert("AUTOHARNESS_ROUTER_PROJECT", "pty-fixture");
        environment.insert("AUTOHARNESS_PROVIDER_TIMEOUT_MS", "5000");
        environment.insert("AUTOHARNESS_PROVIDER_IDLE_TIMEOUT_MS", "5000");
        environment.insert("AUTOHARNESS_PROVIDER_RETRY_ATTEMPTS", "1");
    }

    /// Joins the fixture and returns the observed request lines.
    pub fn finish(mut self) -> Vec<String> {
        self.thread
            .take()
            .expect("router fixture thread")
            .join()
            .expect("router fixture join")
    }
}

fn read_http_request(socket: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    let header_end = loop {
        if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
        let mut chunk = [0_u8; 4096];
        let read = socket.read(&mut chunk).expect("router fixture read");
        assert_ne!(read, 0, "router request ended before its headers");
        bytes.extend_from_slice(&chunk[..read]);
    };
    let headers = std::str::from_utf8(&bytes[..header_end]).expect("router request headers");
    let request_line = headers
        .lines()
        .next()
        .expect("router request line")
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
        let read = socket.read(&mut chunk).expect("router fixture body");
        assert_ne!(read, 0, "router request body ended early");
        bytes.extend_from_slice(&chunk[..read]);
    }
    request_line
}

#[derive(Clone, Debug)]
struct ScenarioMetadata {
    next_event: u64,
}

impl EventMetadataSource for ScenarioMetadata {
    fn next_event_metadata(&mut self) -> GeneratedEventMetadata {
        let number = self.next_event;
        self.next_event = self.next_event.checked_add(1).expect("bounded event count");
        GeneratedEventMetadata::new(
            EventId::new(format!("pty-event-{number}")).expect("event ID"),
            TimestampMillis::new(i64::try_from(number).expect("timestamp")),
        )
    }
}

#[derive(Debug)]
struct ScenarioCommands {
    next_command: u64,
}

impl ScenarioCommands {
    fn execute(
        &mut self,
        engine: &mut DurableEngine<SqliteStore, ScenarioMetadata>,
        payload: CommandPayload,
    ) {
        let number = self.next_command;
        self.next_command = self
            .next_command
            .checked_add(1)
            .expect("bounded command count");
        let command = CommandEnvelope::new(
            CommandId::new(format!("pty-command-{number}")).expect("command ID"),
            CorrelationId::new(format!("pty-correlation-{number}")).expect("correlation ID"),
            payload,
        );
        engine.execute(&command).expect("seed durable command");
    }
}

#[allow(dead_code)]
impl ScenarioEnvironment {
    /// Prepares an isolated data directory and a sanitized environment.
    pub fn prepare() -> Self {
        let directory = tempfile::tempdir().expect("scenario temporary directory");
        let data_dir = directory.path().join("data");
        std::fs::create_dir_all(&data_dir).expect("scenario data directory");
        let mut variables = HashMap::new();
        variables.insert(
            "AUTOHARNESS_DATA_DIR".to_owned(),
            OsString::from(data_dir.as_os_str()),
        );
        variables.insert(
            "AUTOHARNESS_WORKSPACE".to_owned(),
            OsString::from(data_dir.as_os_str()),
        );
        // Credential-free and offline by construction: empty values mean the
        // variable is removed entirely from the child environment.
        for name in [
            "GEMINI_API_KEY",
            "AUTOHARNESS_PROVIDER",
            "AUTOHARNESS_ROUTER_BASE_URL",
            "AUTOHARNESS_ROUTER_API_KEY",
            "AUTOHARNESS_ROUTER_PROJECT",
            "AUTOHARNESS_ROUTER_AUTH_HEADER",
            "AUTOHARNESS_ROUTER_AUTH_SCHEME",
            "AUTOHARNESS_ROUTER_MODELS_PATH",
            "AUTOHARNESS_ROUTER_CHAT_PATH",
            "AUTOHARNESS_PROVIDER_TIMEOUT_MS",
            "AUTOHARNESS_PROVIDER_IDLE_TIMEOUT_MS",
            "AUTOHARNESS_PROVIDER_RETRY_ATTEMPTS",
            "AUTOHARNESS_PROVIDER_CONCURRENCY",
            "AUTOHARNESS_PROVIDER_RATE_REQUESTS",
            "AUTOHARNESS_PROVIDER_RATE_WINDOW_MS",
            "AUTOHARNESS_CATALOG_REFRESH_MS",
            "AUTOHARNESS_CATALOG_MAX_STALE_MS",
        ] {
            variables.insert(name.to_owned(), OsString::new());
        }
        Self {
            variables,
            data_dir,
            _directory: directory,
        }
    }

    /// Removes one variable from the launched process.
    pub fn remove(&mut self, name: &str) {
        self.variables.insert(name.to_owned(), OsString::new());
    }

    /// Overrides one variable for the launched process.
    pub fn insert(&mut self, name: &str, value: impl Into<OsString>) {
        self.variables.insert(name.to_owned(), value.into());
    }

    /// The isolated durable-state directory for this run.
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// Path of the SQLite database inside the isolated data directory.
    pub fn database(&self) -> PathBuf {
        self.data_dir.join("autoharness.sqlite3")
    }

    /// Path of the provider-profile document inside the isolated directory.
    pub fn profiles_document(&self) -> PathBuf {
        self.data_dir.join("autoharness.profiles.json")
    }

    /// Path of the structured application log inside the isolated directory.
    pub fn log(&self) -> PathBuf {
        self.data_dir.join("autoharness.log")
    }

    /// Seeds one completed durable turn for restart and offline scenarios.
    pub fn seed_completed_session(&self, prompt: &str, response: &str) -> ModelRef {
        let store = SqliteStore::open(self.database()).expect("open scenario store");
        let mut engine = DurableEngine::new(store, ScenarioMetadata { next_event: 1 });
        let mut commands = ScenarioCommands { next_command: 1 };
        let session_id = SessionId::new("pty-offline-session").expect("session ID");
        let model = ModelRef::new(
            ProviderId::new("router:pty-fixture").expect("provider ID"),
            ModelId::new("router-offline-model").expect("model ID"),
        );
        let attempt_id = AttemptId::new("pty-offline-attempt").expect("attempt ID");

        for payload in [
            CommandPayload::CreateSession {
                session_id: session_id.clone(),
            },
            CommandPayload::SelectModel {
                session_id: session_id.clone(),
                model: model.clone(),
            },
            CommandPayload::AdmitPromptAndPrepareAttempt {
                session_id: session_id.clone(),
                input_id: InputId::new("pty-offline-input").expect("input ID"),
                prompt: PromptText::new(prompt).expect("prompt"),
                delivery_mode: DeliveryMode::NextTurn,
                attempt_id: attempt_id.clone(),
            },
            CommandPayload::StartAttempt {
                session_id: session_id.clone(),
                attempt_id: attempt_id.clone(),
            },
            CommandPayload::AppendAttemptText {
                session_id: session_id.clone(),
                attempt_id: attempt_id.clone(),
                text: ResponseText::new(response).expect("response"),
            },
            CommandPayload::CompleteAttempt {
                session_id: session_id.clone(),
                attempt_id,
            },
            CommandPayload::RenameSession {
                session_id,
                title: SessionTitle::new("Offline seed").expect("session title"),
            },
        ] {
            commands.execute(&mut engine, payload);
        }
        drop(engine);
        model
    }

    fn command(&self, executable: &Path) -> CommandBuilder {
        let mut command = CommandBuilder::new(executable);
        // Preserve the platform environment required by ConPTY, Unix PTYs,
        // locale handling, and OS credential services. Every application
        // configuration key is explicitly staged above.
        for (name, value) in &self.variables {
            if value.is_empty() {
                command.env_remove(name);
            } else {
                command.env(name, value);
            }
        }
        command
    }
}

fn spawn_pty(environment: &ScenarioEnvironment, rows: u16, columns: u16) -> SpawnedPty {
    let pty = native_pty_system();
    let pair = pty
        .openpty(PtySize {
            rows,
            cols: columns,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("open pseudo-terminal");
    let reader = pair.master.try_clone_reader().expect("clone PTY reader");
    let writer = pair.master.take_writer().expect("take PTY writer");
    // Drain output before process creation. Synchronous ConPTY startup can
    // otherwise deadlock while the pseudo-console establishes its pipes.
    let output = OutputFeed::spawn(reader);
    let executable = env!("CARGO_BIN_EXE_autoharness");
    let child = pair
        .slave
        .spawn_command(environment.command(Path::new(executable)))
        .expect("spawn autoharness under a pseudo-terminal");
    SpawnedPty {
        master: pair.master,
        writer,
        child,
        output,
    }
}

#[allow(dead_code)]
impl PtySession {
    /// Starts the real binary at the given dimensions.
    pub fn start(environment: &ScenarioEnvironment, rows: u16, columns: u16) -> Self {
        let SpawnedPty {
            master,
            writer,
            child,
            output,
        } = spawn_pty(environment, rows, columns);
        Self {
            master: Some(master),
            writer: Some(writer),
            child,
            parser: vt100::Parser::new(rows, columns, 0),
            output,
            data_dir: environment.data_dir().to_path_buf(),
            raw_output: Vec::new(),
            cursor_queries_answered: 0,
        }
    }

    /// Sends raw bytes as if typed on the keyboard.
    pub fn send_bytes(&mut self, bytes: &[u8]) {
        let writer = self.writer.as_mut().expect("live PTY writer");
        writer.write_all(bytes).expect("write to pseudo-terminal");
        writer.flush().expect("flush pseudo-terminal writer");
    }

    /// Types text without submitting it.
    pub fn type_text(&mut self, text: &str) {
        self.send_bytes(text.as_bytes());
    }

    /// Types text followed by Enter.
    pub fn submit_line(&mut self, text: &str) {
        let mut line = String::with_capacity(text.len() + 1);
        line.push_str(text);
        line.push('\r');
        self.send_bytes(line.as_bytes());
    }

    /// Drains everything the application has produced into the screen parser.
    pub fn pump(&mut self) {
        loop {
            match self.output.receiver.try_recv() {
                Ok(chunk) => {
                    self.raw_output.extend_from_slice(&chunk);
                    self.parser.process(&chunk);
                    let cursor_queries = self
                        .raw_output
                        .windows(b"\x1b[6n".len())
                        .filter(|window| *window == b"\x1b[6n")
                        .count();
                    while self.cursor_queries_answered < cursor_queries {
                        self.send_bytes(b"\x1b[1;1R");
                        self.cursor_queries_answered += 1;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.output.eof = true;
                    break;
                }
            }
        }
    }

    /// Waits until `condition` holds on the parsed screen or times out.
    pub fn wait_for(&mut self, condition: impl Fn(&vt100::Screen) -> bool, message: &'static str) {
        let deadline = Instant::now() + SETTLE_TIMEOUT;
        loop {
            self.pump();
            if condition(self.parser.screen()) {
                return;
            }
            if let Some(status) = self.child.try_wait().expect("poll child status") {
                panic!(
                    "{message}; autoharness exited early with code {}; raw output:\n{}",
                    status.exit_code(),
                    String::from_utf8_lossy(&self.raw_output)
                );
            }
            if Instant::now() >= deadline {
                let durable_entries = std::fs::read_dir(&self.data_dir)
                    .map(|entries| {
                        entries
                            .filter_map(Result::ok)
                            .map(|entry| entry.file_name())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let log = std::fs::read_to_string(self.data_dir.join("autoharness.log"))
                    .unwrap_or_else(|error| format!("<unavailable: {error}>"));
                panic!(
                    "{message}; child PID {:?}; data {:?}; files {:?}; last screen:\n{}; raw output:\n{}; log:\n{}",
                    self.child.process_id(),
                    self.data_dir,
                    durable_entries,
                    self.parser.screen().contents(),
                    String::from_utf8_lossy(&self.raw_output),
                    log
                );
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Waits until the raw terminal stream contains one exact byte sequence.
    pub fn wait_for_raw(&mut self, needle: &[u8], message: &'static str) {
        let deadline = Instant::now() + SETTLE_TIMEOUT;
        loop {
            self.pump();
            if self
                .raw_output
                .windows(needle.len())
                .any(|window| window == needle)
            {
                return;
            }
            if Instant::now() >= deadline {
                panic!("{message}");
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Current visible screen contents.
    pub fn screen_text(&mut self) -> String {
        self.pump();
        self.parser.screen().contents()
    }

    /// Resizes the terminal mid-run.
    pub fn resize(&mut self, rows: u16, columns: u16) {
        self.master
            .as_ref()
            .expect("live PTY master")
            .resize(PtySize {
                rows,
                cols: columns,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("resize pseudo-terminal");
        self.parser.set_size(rows, columns);
    }

    /// Waits for process exit and returns its exit code.
    pub fn wait_for_exit(&mut self) -> u32 {
        let deadline = Instant::now() + SETTLE_TIMEOUT;
        loop {
            self.pump();
            if let Some(status) = self.child.try_wait().expect("poll child status") {
                return status.exit_code();
            }
            if Instant::now() >= deadline {
                panic!(
                    "autoharness did not exit; last screen:\n{}",
                    self.parser.screen().contents()
                );
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Forces the process to stop regardless of its state.
    pub fn kill(&mut self) {
        let _ = self.child.kill();
    }

    /// The isolated data directory backing this session.
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        // Never let a failed scenario strand its child. ConPTY closes
        // synchronously and can deadlock while a cloned output reader exists,
        // so Windows leaves those handles to process teardown after killing
        // the child. Each integration-test binary then releases them at exit.
        let _ = self.child.kill();
        self.writer.take();
        #[cfg(windows)]
        {
            if let Some(master) = self.master.take() {
                std::mem::forget(master);
            }
            self.output.receiver = std::sync::mpsc::channel().1;
            self.output.reader_thread.take();
        }
        #[cfg(not(windows))]
        {
            let _ = self.child.wait();
            self.master.take();
            self.output.receiver = std::sync::mpsc::channel().1;
            if let Some(handle) = self.output.reader_thread.take() {
                let _ = handle.join();
            }
        }
    }
}

impl OutputFeed {
    fn spawn(mut reader: Box<dyn Read + Send>) -> Self {
        let (sender, receiver) = std::sync::mpsc::channel::<Vec<u8>>();
        let reader_thread = std::thread::spawn(move || {
            let mut buffer = [0_u8; 16_384];
            while let Ok(read @ 1..) = reader.read(&mut buffer) {
                if sender.send(buffer[..read].to_vec()).is_err() {
                    break;
                }
            }
        });
        Self {
            receiver,
            reader_thread: Some(reader_thread),
            eof: false,
        }
    }
}

/// Ctrl+C as the terminal delivers it.
pub fn ctrl_c() -> [u8; 1] {
    [0x03]
}
