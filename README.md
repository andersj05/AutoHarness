# AutoHarness

AutoHarness is an open-source agent runtime designed to improve the infrastructure around current language models.
Its long-term goal is to learn from durable execution traces and safely improve prompts, policies, routing, tools, memory, and code through reproducible evaluations and gated promotion.

Phase 1 is complete.
The repository now contains a Rust terminal application that discovers compatible Google AI Studio models, streams Gemini responses, supports cancellation and retry, and restores its selected model and transcript from SQLite after restart.
The current provider-protocol evidence uses local HTTP fixtures and fake providers, so it does not claim a live Gemini service verification.

## Run the terminal application

The repository pins Rust 1.97.1 through `rust-toolchain.toml`.
Run the binary from the repository root:

```text
cargo run --locked -p autoharness-app --bin autoharness
```

When no credential is available, AutoHarness opens a masked terminal overlay.
Paste or type the Google AI Studio API key and press `Enter` to validate it and load the model catalog.
Press `Ctrl+K` to open the credential overlay again if the key is rejected, expires, or needs to be replaced.

The application transfers the key through redacted, zeroizing in-memory values and never writes it to configuration, durable session state, logs, transcripts, or model-visible content.
The pasted key is intentionally forgotten when the process exits.
`GEMINI_API_KEY` remains an optional startup override for automation or managed launches.
Do not put the key in a repository file or command-line argument.

## Controls

| Action | Key |
| --- | --- |
| Send the composed prompt | `Ctrl+S` or `Ctrl+Enter` |
| Insert a newline | `Enter` |
| Open the model picker | `Ctrl+P` |
| Open or replace the API key | `Ctrl+K` |
| Filter models | Type while the picker is open |
| Choose a model | `Up` or `Down`, then `Enter` |
| Close the model picker | `Esc` |
| Refresh a failed catalog | `Ctrl+R` while the picker is open |
| Cancel the active response | `Esc` or `Ctrl+C` |
| Retry the latest failed or cancelled attempt | `Ctrl+R` |
| Scroll the transcript | `Alt+Up` or `Alt+Down`, or `Ctrl+PageUp` or `Ctrl+PageDown` |
| Resume following the transcript tail | `Ctrl+End` |
| Quit | `Ctrl+C` when no attempt is active |

Prompts are saved before provider dispatch.
Cancellation requests and response segments also cross the durable command boundary before the terminal treats them as committed.

## Configuration and local data

| Environment variable | Purpose |
| --- | --- |
| `GEMINI_API_KEY` | Optional Google AI Studio credential used only by the Gemini adapter; otherwise paste the key in the app |
| `AUTOHARNESS_DATA_DIR` | Optional absolute override for the application data directory |
| `AUTOHARNESS_LOG` | Log level: `off`, `error`, `warn`, `info`, `debug`, or `trace`; defaults to `info` |

Without an override, AutoHarness uses the platform application-data location:

- Windows: `%LOCALAPPDATA%\AutoHarness`
- macOS: `$HOME/Library/Application Support/AutoHarness`
- Linux and other Unix systems: `$XDG_DATA_HOME/autoharness`, or `$HOME/.local/share/autoharness` when `XDG_DATA_HOME` is unset

The directory contains:

| File | Purpose |
| --- | --- |
| `autoharness.sqlite3` | Durable schema-v1 session events and rebuilt projections |
| `autoharness.log` | Content-free structured lifecycle and operational trace events |
| `autoharness.writer.lock` | Exclusive writer lease that prevents two processes from mutating the same store |

On startup, AutoHarness replays the active session from authoritative events.
An attempt interrupted before provider dispatch becomes a retryable failure, while an attempt interrupted after dispatch becomes an explicit unknown outcome instead of a fabricated success.

## Development

Run the verified baseline gates from the repository root:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked --no-deps -- -D warnings
cargo test --workspace --all-targets --all-features --locked --no-fail-fast
```

The local Phase 1 validation passed formatting, strict Clippy, warning-denied rustdoc, doctests, and the full workspace test suite.
The suite covers in-app credential entry, the composed cancel, retry, and restart path, SQLite replay and projection rebuilding, model pagination, arbitrary SSE fragmentation, provider cancellation, retry classification, terminal restoration, fixed-size rendering, and credential redaction.
A PTY smoke run without a Gemini credential rendered the complete 80-by-24 terminal interface, confined its SQLite, log, and lock files to an isolated absolute data directory, exited successfully through `Ctrl+C`, and restored the terminal.
A credential-overlay PTY smoke run pasted a sentinel through bracketed paste, displayed only the fixed mask, cleared it on dismissal, reopened an empty editor through `Ctrl+K`, found no sentinel bytes in the application files, and restored the terminal on exit.

## Performance evidence

The checked-in [Phase 1 benchmark environment](benchmarks/README.md) measures durable event append with synchronous projections, transcript-read throughput, and warm SQLite reopen with strict replay for representative session sizes.
It also includes a PowerShell idle resident-memory sampler and a reference-machine record template.

```text
cargo run --release --locked --manifest-path benchmarks/Cargo.toml -- --output benchmarks/results/phase1-<machine>-<date>.json
```

The benchmark report excludes network requests and records LLM latency separately as not measured.
Cold start to first draw, input-to-dispatch overhead, and provider-chunk-to-render latency remain unmeasured until the application exposes the exact monotonic markers defined by the [instrumentation contract](benchmarks/instrumentation-contract.md).

## Project documentation

- [Project plan](docs/PROJECT_PLAN.md)
- [Architecture overview](docs/architecture/OVERVIEW.md)
- [Persistent memory architecture](docs/architecture/PERSISTENT_MEMORY.md)
- [Repository memory](docs/memory/README.md)
- [Architecture decision records](docs/adr/README.md)
- [Reference-project research](docs/research/agent-memory-patterns.md)

## Guiding principles

- Keep the engine independent from the terminal interface and model providers.
- Treat every provider response as a typed, replayable event stream.
- Preserve provenance for every memory, decision, experiment, and promoted change.
- Evaluate proposed improvements before promotion and retain rollback paths.
- Keep secrets out of source control, logs, transcripts, and model-visible memory.
- Prefer native performance, bounded concurrency, deterministic recovery, and explicit permissions.

Licensing and contributor policies will be finalized before the first public release.
