use std::env;
use std::error::Error;
use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use autoharness_domain::{
    AttemptId, CommandEnvelope, CommandId, CommandPayload, CorrelationId, DeliveryMode, EventId,
    InputId, ModelId, ModelRef, PromptText, ProviderId, ResponseText, SessionId, TimestampMillis,
};
use autoharness_engine::{DurableEngine, EventMetadataSource, GeneratedEventMetadata};
use autoharness_store::SessionStore as _;
use autoharness_store_sqlite::SqliteStore;

const REPORT_SCHEMA_VERSION: u16 = 1;
const DEFAULT_TURNS: &str = "10,100,1000";
const DEFAULT_CHUNKS_PER_TURN: usize = 4;
const DEFAULT_PROMPT_BYTES: usize = 256;
const DEFAULT_CHUNK_BYTES: usize = 64;
const DEFAULT_SAMPLES: usize = 5;
const DEFAULT_WARMUPS: usize = 1;
const DEFAULT_PROJECTION_READS: usize = 20;
const MAX_TURNS: usize = 100_000;
const MAX_CHUNKS_PER_TURN: usize = 10_000;
const MAX_CONTENT_BYTES: usize = 1_048_576;
const MAX_SAMPLES: usize = 1_000;
const MAX_PROJECTION_READS: usize = 100_000;

type BenchEngine = DurableEngine<SqliteStore, BenchmarkMetadata>;
type BenchResult<T> = Result<T, Box<dyn Error>>;

fn main() -> ExitCode {
    match parse_arguments(env::args().skip(1)) {
        Ok(ParseOutcome::Help) => {
            println!("{}", usage());
            ExitCode::SUCCESS
        }
        Ok(ParseOutcome::Run(config)) => match run(&config) {
            Ok(report) => match publish_report(&report, config.output.as_deref()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("Phase 1 benchmark failed: {error}");
                    ExitCode::FAILURE
                }
            },
            Err(error) => {
                eprintln!("Phase 1 benchmark failed: {error}");
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            eprintln!("Phase 1 benchmark argument error: {error}");
            eprintln!("{}", usage());
            ExitCode::FAILURE
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Config {
    turns: Vec<usize>,
    chunks_per_turn: usize,
    prompt_bytes: usize,
    chunk_bytes: usize,
    samples: usize,
    warmups: usize,
    projection_reads: usize,
    output: Option<PathBuf>,
    keep_databases: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            turns: parse_turns(DEFAULT_TURNS).expect("static turn list is valid"),
            chunks_per_turn: DEFAULT_CHUNKS_PER_TURN,
            prompt_bytes: DEFAULT_PROMPT_BYTES,
            chunk_bytes: DEFAULT_CHUNK_BYTES,
            samples: DEFAULT_SAMPLES,
            warmups: DEFAULT_WARMUPS,
            projection_reads: DEFAULT_PROJECTION_READS,
            output: None,
            keep_databases: false,
        }
    }
}

enum ParseOutcome {
    Help,
    Run(Config),
}

fn parse_arguments(arguments: impl Iterator<Item = String>) -> BenchResult<ParseOutcome> {
    let mut config = Config::default();
    let mut arguments = arguments.peekable();

    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "-h" | "--help" => return Ok(ParseOutcome::Help),
            "--turns" => config.turns = parse_turns(&next_value(&mut arguments, "--turns")?)?,
            "--chunks-per-turn" => {
                config.chunks_per_turn = parse_bounded(
                    &next_value(&mut arguments, "--chunks-per-turn")?,
                    "chunks per turn",
                    1,
                    MAX_CHUNKS_PER_TURN,
                )?;
            }
            "--prompt-bytes" => {
                config.prompt_bytes = parse_bounded(
                    &next_value(&mut arguments, "--prompt-bytes")?,
                    "prompt bytes",
                    1,
                    MAX_CONTENT_BYTES,
                )?;
            }
            "--chunk-bytes" => {
                config.chunk_bytes = parse_bounded(
                    &next_value(&mut arguments, "--chunk-bytes")?,
                    "chunk bytes",
                    1,
                    MAX_CONTENT_BYTES,
                )?;
            }
            "--samples" => {
                config.samples = parse_bounded(
                    &next_value(&mut arguments, "--samples")?,
                    "samples",
                    1,
                    MAX_SAMPLES,
                )?;
            }
            "--warmups" => {
                config.warmups = parse_bounded(
                    &next_value(&mut arguments, "--warmups")?,
                    "warmups",
                    0,
                    MAX_SAMPLES,
                )?;
            }
            "--projection-reads" => {
                config.projection_reads = parse_bounded(
                    &next_value(&mut arguments, "--projection-reads")?,
                    "projection reads",
                    1,
                    MAX_PROJECTION_READS,
                )?;
            }
            "--output" => {
                config.output = Some(PathBuf::from(next_value(&mut arguments, "--output")?));
            }
            "--keep-databases" => config.keep_databases = true,
            _ => return Err(invalid_input(format!("unknown argument `{argument}`"))),
        }
    }

    Ok(ParseOutcome::Run(config))
}

fn next_value(arguments: &mut impl Iterator<Item = String>, option: &str) -> BenchResult<String> {
    arguments
        .next()
        .ok_or_else(|| invalid_input(format!("{option} requires a value")))
}

fn parse_turns(value: &str) -> BenchResult<Vec<usize>> {
    let mut turns = Vec::new();
    for item in value.split(',') {
        let parsed = parse_bounded(item, "turn count", 1, MAX_TURNS)?;
        if turns.contains(&parsed) {
            return Err(invalid_input(format!(
                "turn count `{parsed}` is duplicated"
            )));
        }
        turns.push(parsed);
    }
    if turns.is_empty() {
        return Err(invalid_input("at least one turn count is required"));
    }
    Ok(turns)
}

fn parse_bounded(value: &str, name: &str, minimum: usize, maximum: usize) -> BenchResult<usize> {
    let parsed = value
        .parse::<usize>()
        .map_err(|_| invalid_input(format!("{name} must be an integer")))?;
    if !(minimum..=maximum).contains(&parsed) {
        return Err(invalid_input(format!(
            "{name} must be between {minimum} and {maximum}"
        )));
    }
    Ok(parsed)
}

fn invalid_input(message: impl Into<String>) -> Box<dyn Error> {
    Box::new(io::Error::new(io::ErrorKind::InvalidInput, message.into()))
}

fn usage() -> String {
    format!(
        "AutoHarness Phase 1 benchmark runner\n\
         \n\
         Usage:\n\
           cargo run --release --locked --manifest-path benchmarks/Cargo.toml -- [options]\n\
         \n\
         Options:\n\
           --turns <list>             Comma-separated session turn counts [{DEFAULT_TURNS}]\n\
           --chunks-per-turn <count>  Durable response chunks per turn [{DEFAULT_CHUNKS_PER_TURN}]\n\
           --prompt-bytes <count>      ASCII prompt bytes per turn [{DEFAULT_PROMPT_BYTES}]\n\
           --chunk-bytes <count>       ASCII response bytes per chunk [{DEFAULT_CHUNK_BYTES}]\n\
           --samples <count>           Recorded fresh-database samples [{DEFAULT_SAMPLES}]\n\
           --warmups <count>           Unreported warmup samples [{DEFAULT_WARMUPS}]\n\
           --projection-reads <count>  Transcript reads within each sample [{DEFAULT_PROJECTION_READS}]\n\
           --output <path>             Create a JSON result file instead of printing to stdout\n\
           --keep-databases            Retain generated SQLite databases for inspection\n\
           -h, --help                  Print this help"
    )
}

#[derive(Clone, Debug)]
struct BenchmarkMetadata {
    next_event: u64,
}

impl BenchmarkMetadata {
    const fn new() -> Self {
        Self { next_event: 1 }
    }
}

impl EventMetadataSource for BenchmarkMetadata {
    fn next_event_metadata(&mut self) -> GeneratedEventMetadata {
        let number = self.next_event;
        self.next_event = self
            .next_event
            .checked_add(1)
            .expect("event count is bounded");
        GeneratedEventMetadata::new(
            EventId::new(format!("benchmark-event-{number}")).expect("generated event ID is valid"),
            TimestampMillis::new(i64::try_from(number).expect("event count fits timestamp")),
        )
    }
}

#[derive(Debug)]
struct CommandFactory {
    next_command: u64,
}

impl CommandFactory {
    const fn new() -> Self {
        Self { next_command: 1 }
    }

    fn envelope(&mut self, payload: CommandPayload) -> CommandEnvelope {
        let number = self.next_command;
        self.next_command = self
            .next_command
            .checked_add(1)
            .expect("command count is bounded");
        CommandEnvelope::new(
            CommandId::new(format!("benchmark-command-{number}"))
                .expect("generated command ID is valid"),
            CorrelationId::new(format!("benchmark-correlation-{number}"))
                .expect("generated correlation ID is valid"),
            payload,
        )
    }
}

#[derive(Debug)]
struct TempRoot {
    path: PathBuf,
    keep: bool,
}

impl TempRoot {
    fn create(keep: bool) -> BenchResult<Self> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = env::temp_dir().join(format!(
            "autoharness-phase1-bench-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&path)?;
        Ok(Self { path, keep })
    }

    fn database(&self, turns: usize, kind: &str, index: usize) -> PathBuf {
        self.path
            .join(format!("turns-{turns}-{kind}-{index}.sqlite3"))
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        if self.keep {
            eprintln!("Benchmark databases retained at {}", self.path.display());
            return;
        }
        let is_owned_temp = self
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("autoharness-phase1-bench-"));
        if is_owned_temp {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Sample {
    append_ms: f64,
    append_events_per_second: f64,
    append_transactions_per_second: f64,
    projection_read_ms: f64,
    projection_entries_per_second: f64,
    projection_bytes_per_second: f64,
    recovery_ms: f64,
    recovery_events_per_second: f64,
}

#[derive(Clone, Copy, Debug)]
struct Distribution {
    count: usize,
    minimum: f64,
    median: f64,
    p95: f64,
    mean: f64,
    maximum: f64,
}

impl Distribution {
    fn from_values(values: impl Iterator<Item = f64>) -> Self {
        let mut values: Vec<f64> = values.collect();
        assert!(!values.is_empty(), "sample count is validated");
        values.sort_by(f64::total_cmp);
        let count = values.len();
        let median = if count.is_multiple_of(2) {
            (values[count / 2 - 1] + values[count / 2]) / 2.0
        } else {
            values[count / 2]
        };
        let p95_index = (count * 95).div_ceil(100).saturating_sub(1);
        let mean = values.iter().sum::<f64>() / count as f64;
        Self {
            count,
            minimum: values[0],
            median,
            p95: values[p95_index],
            mean,
            maximum: values[count - 1],
        }
    }
}

#[derive(Debug)]
struct ScenarioReport {
    turns: usize,
    events_per_sample: usize,
    timed_append_events_per_sample: usize,
    timed_append_transactions_per_sample: usize,
    transcript_entries_per_read: usize,
    transcript_bytes_per_read: usize,
    append_ms: Distribution,
    append_events_per_second: Distribution,
    append_transactions_per_second: Distribution,
    projection_read_ms: Distribution,
    projection_entries_per_second: Distribution,
    projection_bytes_per_second: Distribution,
    recovery_ms: Distribution,
    recovery_events_per_second: Distribution,
}

#[derive(Debug)]
struct Report {
    generated_unix_ms: u128,
    config: Config,
    scenarios: Vec<ScenarioReport>,
}

fn run(config: &Config) -> BenchResult<Report> {
    let root = TempRoot::create(config.keep_databases)?;
    let mut scenarios = Vec::with_capacity(config.turns.len());

    for &turns in &config.turns {
        for warmup in 0..config.warmups {
            let database = root.database(turns, "warmup", warmup);
            let _ = run_sample(config, turns, &database)?;
        }

        let mut samples = Vec::with_capacity(config.samples);
        for sample in 0..config.samples {
            let database = root.database(turns, "sample", sample);
            samples.push(run_sample(config, turns, &database)?);
        }
        scenarios.push(summarize_scenario(config, turns, &samples)?);
    }

    Ok(Report {
        generated_unix_ms: SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
        config: config.clone(),
        scenarios,
    })
}

fn run_sample(config: &Config, turns: usize, database: &Path) -> BenchResult<Sample> {
    let store = SqliteStore::open(database)?;
    let mut engine = DurableEngine::new(store, BenchmarkMetadata::new());
    let mut commands = CommandFactory::new();
    let session_id = SessionId::new("benchmark-session")?;
    let model = ModelRef::new(
        ProviderId::new("gemini")?,
        ModelId::new("models/benchmark-model")?,
    );

    execute(
        &mut engine,
        &mut commands,
        CommandPayload::CreateSession {
            session_id: session_id.clone(),
        },
    )?;
    execute(
        &mut engine,
        &mut commands,
        CommandPayload::SelectModel {
            session_id: session_id.clone(),
            model,
        },
    )?;

    let prompt = PromptText::new("p".repeat(config.prompt_bytes))?;
    let chunk = ResponseText::new("r".repeat(config.chunk_bytes))?;
    let mut timed_events = 0_usize;
    let mut timed_transactions = 0_usize;
    let append_started = Instant::now();

    for turn in 0..turns {
        let input_id = InputId::new(format!("benchmark-input-{turn}"))?;
        let attempt_id = AttemptId::new(format!("benchmark-attempt-{turn}"))?;
        timed_events += execute(
            &mut engine,
            &mut commands,
            CommandPayload::AdmitPromptAndPrepareAttempt {
                session_id: session_id.clone(),
                input_id,
                prompt: prompt.clone(),
                delivery_mode: DeliveryMode::NextTurn,
                attempt_id: attempt_id.clone(),
            },
        )?;
        timed_transactions += 1;
        timed_events += execute(
            &mut engine,
            &mut commands,
            CommandPayload::StartAttempt {
                session_id: session_id.clone(),
                attempt_id: attempt_id.clone(),
            },
        )?;
        timed_transactions += 1;
        for _ in 0..config.chunks_per_turn {
            timed_events += execute(
                &mut engine,
                &mut commands,
                CommandPayload::AppendAttemptText {
                    session_id: session_id.clone(),
                    attempt_id: attempt_id.clone(),
                    text: chunk.clone(),
                },
            )?;
            timed_transactions += 1;
        }
        timed_events += execute(
            &mut engine,
            &mut commands,
            CommandPayload::CompleteAttempt {
                session_id: session_id.clone(),
                attempt_id,
            },
        )?;
        timed_transactions += 1;
    }

    let append_seconds = nonzero_seconds(append_started.elapsed().as_secs_f64());
    let append_ms = append_seconds * 1_000.0;
    let append_events_per_second = timed_events as f64 / append_seconds;
    let append_transactions_per_second = timed_transactions as f64 / append_seconds;

    let projection_started = Instant::now();
    let mut projected_entries = 0_usize;
    let mut projected_bytes = 0_usize;
    for _ in 0..config.projection_reads {
        let transcript = engine.store().load_transcript(&session_id)?;
        projected_entries = projected_entries
            .checked_add(transcript.len())
            .ok_or_else(|| invalid_input("projected entry count overflowed"))?;
        let bytes = transcript.iter().try_fold(0_usize, |total, entry| {
            total
                .checked_add(entry.content().as_str().len())
                .ok_or_else(|| invalid_input("projected byte count overflowed"))
        })?;
        projected_bytes = projected_bytes
            .checked_add(bytes)
            .ok_or_else(|| invalid_input("projected byte count overflowed"))?;
    }
    let projection_seconds = nonzero_seconds(projection_started.elapsed().as_secs_f64());

    let total_events = engine.events().len();
    let (store, _) = engine.into_parts();
    drop(store);

    let recovery_started = Instant::now();
    let recovered = DurableEngine::recover(SqliteStore::open(database)?, BenchmarkMetadata::new())?;
    let recovery_seconds = nonzero_seconds(recovery_started.elapsed().as_secs_f64());
    if recovered.events().len() != total_events || recovered.session(&session_id).is_none() {
        return Err(invalid_input(
            "recovery did not reconstruct the generated fixture",
        ));
    }

    Ok(Sample {
        append_ms,
        append_events_per_second,
        append_transactions_per_second,
        projection_read_ms: projection_seconds * 1_000.0,
        projection_entries_per_second: projected_entries as f64 / projection_seconds,
        projection_bytes_per_second: projected_bytes as f64 / projection_seconds,
        recovery_ms: recovery_seconds * 1_000.0,
        recovery_events_per_second: total_events as f64 / recovery_seconds,
    })
}

fn execute(
    engine: &mut BenchEngine,
    commands: &mut CommandFactory,
    payload: CommandPayload,
) -> BenchResult<usize> {
    let command = commands.envelope(payload);
    Ok(engine.execute(&command)?.len())
}

fn nonzero_seconds(seconds: f64) -> f64 {
    seconds.max(f64::EPSILON)
}

fn summarize_scenario(
    config: &Config,
    turns: usize,
    samples: &[Sample],
) -> BenchResult<ScenarioReport> {
    let events_per_turn = config
        .chunks_per_turn
        .checked_add(4)
        .ok_or_else(|| invalid_input("event count overflowed"))?;
    let transactions_per_turn = config
        .chunks_per_turn
        .checked_add(3)
        .ok_or_else(|| invalid_input("transaction count overflowed"))?;
    let timed_append_events_per_sample = turns
        .checked_mul(events_per_turn)
        .ok_or_else(|| invalid_input("event count overflowed"))?;
    let timed_append_transactions_per_sample = turns
        .checked_mul(transactions_per_turn)
        .ok_or_else(|| invalid_input("transaction count overflowed"))?;
    let events_per_sample = timed_append_events_per_sample
        .checked_add(2)
        .ok_or_else(|| invalid_input("event count overflowed"))?;
    let transcript_entries_per_read = turns
        .checked_mul(2)
        .ok_or_else(|| invalid_input("transcript entry count overflowed"))?;
    let response_bytes = config
        .chunks_per_turn
        .checked_mul(config.chunk_bytes)
        .ok_or_else(|| invalid_input("response byte count overflowed"))?;
    let transcript_bytes_per_read = turns
        .checked_mul(
            config
                .prompt_bytes
                .checked_add(response_bytes)
                .ok_or_else(|| invalid_input("transcript byte count overflowed"))?,
        )
        .ok_or_else(|| invalid_input("transcript byte count overflowed"))?;

    Ok(ScenarioReport {
        turns,
        events_per_sample,
        timed_append_events_per_sample,
        timed_append_transactions_per_sample,
        transcript_entries_per_read,
        transcript_bytes_per_read,
        append_ms: Distribution::from_values(samples.iter().map(|sample| sample.append_ms)),
        append_events_per_second: Distribution::from_values(
            samples.iter().map(|sample| sample.append_events_per_second),
        ),
        append_transactions_per_second: Distribution::from_values(
            samples
                .iter()
                .map(|sample| sample.append_transactions_per_second),
        ),
        projection_read_ms: Distribution::from_values(
            samples.iter().map(|sample| sample.projection_read_ms),
        ),
        projection_entries_per_second: Distribution::from_values(
            samples
                .iter()
                .map(|sample| sample.projection_entries_per_second),
        ),
        projection_bytes_per_second: Distribution::from_values(
            samples
                .iter()
                .map(|sample| sample.projection_bytes_per_second),
        ),
        recovery_ms: Distribution::from_values(samples.iter().map(|sample| sample.recovery_ms)),
        recovery_events_per_second: Distribution::from_values(
            samples
                .iter()
                .map(|sample| sample.recovery_events_per_second),
        ),
    })
}

impl Report {
    fn to_json(&self) -> String {
        let mut output = String::new();
        writeln!(output, "{{").expect("string write");
        writeln!(output, "  \"schema_version\": {REPORT_SCHEMA_VERSION},").expect("string write");
        writeln!(
            output,
            "  \"generated_unix_ms\": {},",
            self.generated_unix_ms
        )
        .expect("string write");
        writeln!(output, "  \"benchmark\": \"phase1_harness_overhead\",").expect("string write");
        writeln!(output, "  \"platform\": {{").expect("string write");
        writeln!(output, "    \"os\": \"{}\",", env::consts::OS).expect("string write");
        writeln!(output, "    \"arch\": \"{}\",", env::consts::ARCH).expect("string write");
        writeln!(
            output,
            "    \"build_profile\": \"{}\"",
            if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            }
        )
        .expect("string write");
        writeln!(output, "  }},").expect("string write");
        writeln!(output, "  \"configuration\": {{").expect("string write");
        write!(output, "    \"turns\": [").expect("string write");
        for (index, turns) in self.config.turns.iter().enumerate() {
            if index > 0 {
                output.push_str(", ");
            }
            write!(output, "{turns}").expect("string write");
        }
        writeln!(output, "],").expect("string write");
        writeln!(
            output,
            "    \"chunks_per_turn\": {},",
            self.config.chunks_per_turn
        )
        .expect("string write");
        writeln!(
            output,
            "    \"prompt_bytes\": {},",
            self.config.prompt_bytes
        )
        .expect("string write");
        writeln!(output, "    \"chunk_bytes\": {},", self.config.chunk_bytes)
            .expect("string write");
        writeln!(output, "    \"samples\": {},", self.config.samples).expect("string write");
        writeln!(output, "    \"warmups\": {},", self.config.warmups).expect("string write");
        writeln!(
            output,
            "    \"projection_reads_per_sample\": {}",
            self.config.projection_reads
        )
        .expect("string write");
        writeln!(output, "  }},").expect("string write");
        writeln!(output, "  \"methodology\": {{").expect("string write");
        writeln!(output, "    \"clock\": \"monotonic_instant\",").expect("string write");
        writeln!(output, "    \"percentile\": \"nearest_rank\",").expect("string write");
        writeln!(output, "    \"fresh_database_per_sample\": true,").expect("string write");
        writeln!(
            output,
            "    \"sqlite_configuration\": \"production_default\","
        )
        .expect("string write");
        writeln!(
            output,
            "    \"recovery_cache_state\": \"warm_or_os_managed\","
        )
        .expect("string write");
        writeln!(output, "    \"network_requests\": 0,").expect("string write");
        writeln!(output, "    \"network_latency_included\": false").expect("string write");
        writeln!(output, "  }},").expect("string write");
        writeln!(output, "  \"scenarios\": [").expect("string write");
        for (index, scenario) in self.scenarios.iter().enumerate() {
            scenario.write_json(&mut output, index + 1 == self.scenarios.len());
        }
        writeln!(output, "  ],").expect("string write");
        writeln!(output, "  \"separate_metrics\": {{").expect("string write");
        writeln!(output, "    \"idle_resident_memory\": {{").expect("string write");
        writeln!(output, "      \"status\": \"external_sampler_available\",")
            .expect("string write");
        writeln!(
            output,
            "      \"runner\": \"benchmarks/scripts/sample-idle-memory.ps1\""
        )
        .expect("string write");
        writeln!(output, "    }},").expect("string write");
        writeln!(output, "    \"llm_network_latency\": {{").expect("string write");
        writeln!(output, "      \"status\": \"not_measured\",").expect("string write");
        writeln!(output, "      \"included_in_harness_metrics\": false").expect("string write");
        writeln!(output, "    }}").expect("string write");
        writeln!(output, "  }},").expect("string write");
        writeln!(output, "  \"deferred_metrics\": [").expect("string write");
        write_deferred_metric(
            &mut output,
            "cold_process_start_to_first_terminal_draw",
            "requires a first-draw marker observable by an external process launcher",
            false,
        );
        write_deferred_metric(
            &mut output,
            "input_to_request_dispatch_overhead",
            "requires correlated input-accepted and provider-dispatch monotonic markers",
            false,
        );
        write_deferred_metric(
            &mut output,
            "provider_chunk_receipt_to_rendered_delta_latency",
            "requires correlated provider-receipt and completed-draw monotonic markers",
            true,
        );
        writeln!(output, "  ]").expect("string write");
        writeln!(output, "}}").expect("string write");
        output
    }
}

impl ScenarioReport {
    fn write_json(&self, output: &mut String, last: bool) {
        writeln!(output, "    {{").expect("string write");
        writeln!(output, "      \"turns\": {},", self.turns).expect("string write");
        writeln!(
            output,
            "      \"events_per_sample\": {},",
            self.events_per_sample
        )
        .expect("string write");
        writeln!(output, "      \"durable_append_and_projection\": {{").expect("string write");
        writeln!(output, "        \"status\": \"measured\",").expect("string write");
        writeln!(output, "        \"definition\": \"headless validation plus production SQLite transaction and synchronous read-model maintenance\",").expect("string write");
        writeln!(
            output,
            "        \"events_per_sample\": {},",
            self.timed_append_events_per_sample
        )
        .expect("string write");
        writeln!(
            output,
            "        \"transactions_per_sample\": {},",
            self.timed_append_transactions_per_sample
        )
        .expect("string write");
        write_distribution(output, "duration_ms", self.append_ms, 8, true);
        write_distribution(
            output,
            "events_per_second",
            self.append_events_per_second,
            8,
            true,
        );
        write_distribution(
            output,
            "transactions_per_second",
            self.append_transactions_per_second,
            8,
            false,
        );
        writeln!(output, "      }},").expect("string write");
        writeln!(output, "      \"transcript_projection_read\": {{").expect("string write");
        writeln!(output, "        \"status\": \"measured\",").expect("string write");
        writeln!(
            output,
            "        \"entries_per_read\": {},",
            self.transcript_entries_per_read
        )
        .expect("string write");
        writeln!(
            output,
            "        \"content_bytes_per_read\": {},",
            self.transcript_bytes_per_read
        )
        .expect("string write");
        write_distribution(output, "duration_ms", self.projection_read_ms, 8, true);
        write_distribution(
            output,
            "entries_per_second",
            self.projection_entries_per_second,
            8,
            true,
        );
        write_distribution(
            output,
            "content_bytes_per_second",
            self.projection_bytes_per_second,
            8,
            false,
        );
        writeln!(output, "      }},").expect("string write");
        writeln!(output, "      \"warm_reopen_and_event_replay\": {{").expect("string write");
        writeln!(output, "        \"status\": \"measured\",").expect("string write");
        writeln!(output, "        \"definition\": \"SQLite open plus paginated authoritative event load plus strict headless replay\",").expect("string write");
        write_distribution(output, "duration_ms", self.recovery_ms, 8, true);
        write_distribution(
            output,
            "events_per_second",
            self.recovery_events_per_second,
            8,
            false,
        );
        writeln!(output, "      }}").expect("string write");
        writeln!(output, "    }}{}", if last { "" } else { "," }).expect("string write");
    }
}

fn write_distribution(
    output: &mut String,
    name: &str,
    distribution: Distribution,
    spaces: usize,
    trailing_comma: bool,
) {
    let indent = " ".repeat(spaces);
    writeln!(output, "{indent}\"{name}\": {{").expect("string write");
    writeln!(output, "{indent}  \"count\": {},", distribution.count).expect("string write");
    writeln!(output, "{indent}  \"min\": {:.6},", distribution.minimum).expect("string write");
    writeln!(output, "{indent}  \"median\": {:.6},", distribution.median).expect("string write");
    writeln!(output, "{indent}  \"p95\": {:.6},", distribution.p95).expect("string write");
    writeln!(output, "{indent}  \"mean\": {:.6},", distribution.mean).expect("string write");
    writeln!(output, "{indent}  \"max\": {:.6}", distribution.maximum).expect("string write");
    writeln!(
        output,
        "{indent}}}{}",
        if trailing_comma { "," } else { "" }
    )
    .expect("string write");
}

fn write_deferred_metric(output: &mut String, name: &str, reason: &str, last: bool) {
    writeln!(output, "    {{").expect("string write");
    writeln!(output, "      \"name\": \"{name}\",").expect("string write");
    writeln!(output, "      \"status\": \"not_instrumented\",").expect("string write");
    writeln!(output, "      \"reason\": \"{reason}\",").expect("string write");
    writeln!(output, "      \"network_latency_included\": false").expect("string write");
    writeln!(output, "    }}{}", if last { "" } else { "," }).expect("string write");
}

fn publish_report(report: &Report, output: Option<&Path>) -> BenchResult<()> {
    let json = report.to_json();
    if let Some(path) = output {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
        file.write_all(json.as_bytes())?;
        file.flush()?;
        println!("Wrote benchmark report to {}", path.display());
    } else {
        print!("{json}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_rank_p95_and_even_median_are_stable() {
        let distribution = Distribution::from_values([4.0, 1.0, 3.0, 2.0].into_iter());

        assert_eq!(distribution.count, 4);
        assert_eq!(distribution.minimum, 1.0);
        assert_eq!(distribution.median, 2.5);
        assert_eq!(distribution.p95, 4.0);
        assert_eq!(distribution.mean, 2.5);
        assert_eq!(distribution.maximum, 4.0);
    }

    #[test]
    fn duplicate_or_out_of_range_turns_are_rejected() {
        assert!(parse_turns("1,10,100").is_ok());
        assert!(parse_turns("10,10").is_err());
        assert!(parse_turns("0").is_err());
        assert!(parse_turns("not-a-number").is_err());
    }

    #[test]
    fn report_is_valid_json_by_construction_contract() {
        let scenario = ScenarioReport {
            turns: 1,
            events_per_sample: 8,
            timed_append_events_per_sample: 6,
            timed_append_transactions_per_sample: 5,
            transcript_entries_per_read: 2,
            transcript_bytes_per_read: 512,
            append_ms: distribution(),
            append_events_per_second: distribution(),
            append_transactions_per_second: distribution(),
            projection_read_ms: distribution(),
            projection_entries_per_second: distribution(),
            projection_bytes_per_second: distribution(),
            recovery_ms: distribution(),
            recovery_events_per_second: distribution(),
        };
        let report = Report {
            generated_unix_ms: 1,
            config: Config {
                turns: vec![1],
                samples: 1,
                warmups: 0,
                ..Config::default()
            },
            scenarios: vec![scenario],
        };
        let json = report.to_json();

        assert!(json.starts_with("{\n"));
        assert!(json.ends_with("}\n"));
        assert!(json.contains("\"network_requests\": 0"));
        assert!(json.contains("\"network_latency_included\": false"));
    }

    fn distribution() -> Distribution {
        Distribution::from_values([1.0].into_iter())
    }
}
