# AutoHarness

AutoHarness is an open-source agent runtime designed to improve the infrastructure around current language models.
Its long-term goal is to learn from durable execution traces and safely improve prompts, policies, routing, tools, memory, and code through reproducible evaluations and gated promotion.

The Phase 3 safe-execution substrate is complete, and Phase 3.1 local protocol reliability and recovery are fixture-verified while live-provider exit evidence remains open.
The Rust terminal application runs a durable, resumable tool loop through Google AI Studio Gemini or a configurable OpenAI-compatible model router, but session browsing, persistent profiles, and in-app settings remain planned work.
Versioned filesystem, direct-process, and HTTP tools use scoped deny, ask, or allow decisions, bounded output, content-addressed artifacts, immutable run budgets, and explicit interruption recovery.
Shared provider policy applies timeouts, bounded pre-stream retries, concurrency, per-project rate limits, capability preflight, and a durable model-catalog cache with explicit refresh and stale fallback rules.
Gemini function arguments are aggregated across streamed deltas, malformed model calls are durably denied and returned for bounded repair, and failed prompts are not replayed into unrelated later turns.
The current reviewed provider-protocol evidence still does not claim a successful live Gemini or router verification.

## Run the terminal application

The repository pins Rust 1.97.1 through `rust-toolchain.toml`.
Run the binary from the repository root:

```text
cargo run --locked -p autoharness-app --bin autoharness
```

Gemini remains the default provider.
When no credential is available, AutoHarness opens a masked terminal overlay.
Paste or type the selected provider's API key and press `Enter` to validate it and load the model catalog.
Press `Ctrl+K` to open the credential overlay again if the key is rejected, expires, or needs to be replaced.

The application transfers the key through redacted, zeroizing in-memory values and never writes it to configuration, durable session state, logs, transcripts, or model-visible content.
The pasted key is intentionally forgotten when the process exits.
`GEMINI_API_KEY` and `AUTOHARNESS_ROUTER_API_KEY` are optional startup overrides for automation or managed launches.
Do not put the key in a repository file or command-line argument.

To use an OpenAI-compatible router, set `AUTOHARNESS_PROVIDER=router`, a base URL ending in `/`, and the router credential.
Router credentials require HTTPS except for loopback HTTP endpoints such as `http://127.0.0.1:PORT/`.
The default relative endpoints are `v1/models` and `v1/chat/completions`.
Routers mounted below a path such as `https://router.example/api/` retain that path when relative endpoints are resolved.

## Controls

| Action | Key |
| --- | --- |
| Send the composed prompt | `Ctrl+S` or `Ctrl+Enter` |
| Insert a newline | `Enter` |
| Open the model picker | `Ctrl+P` |
| Create a fresh durable session | `Ctrl+N` |
| Open the session browser | `Ctrl+L` |
| Search sessions | Type while the browser is open |
| Rename, archive, or unarchive the highlighted session | `Ctrl+R`, `Ctrl+A`, `Ctrl+U` while the browser is open |
| Delete the highlighted session | `Ctrl+D` then `Y` to confirm while the browser is open |
| Slash commands | `/sessions`, `/open <n>`, `/rename <title>`, `/archive`, `/unarchive`, `/delete` in the composer |
| Open or replace the API key | `Ctrl+K` |
| Filter models | Type while the picker is open |
| Choose a model | `Up` or `Down`, then `Enter` |
| Close the model picker | `Esc` |
| Refresh a failed catalog | `Ctrl+R` while the picker is open |
| Cancel the active response | `Esc` or `Ctrl+C` |
| Retry the latest failed or cancelled attempt | `Ctrl+R` |
| Allow the displayed exact tool request once | `Y` |
| Deny the displayed tool request | `N` or `Esc` |
| Inspect a long tool request | `Up` or `Down` while the permission overlay is open |
| Scroll the transcript | `Alt+Up` or `Alt+Down`, or `Ctrl+PageUp` or `Ctrl+PageDown` |
| Resume following the transcript tail | `Ctrl+End` |
| Quit | `Ctrl+C` when no attempt is active |

Prompts are saved before provider dispatch.
Cancellation requests and response segments also cross the durable command boundary before the terminal treats them as committed.
Tool calls, scoped permission decisions, human answers, effect-start boundaries, and results cross the same boundary.
Workspace reads, writes, direct process execution, and HTTP requests all require an explicit terminal answer under the default local policy.
The permission overlay shows scrollable operation-specific fields, including every process argument and the HTTP method and full URL.

## Configuration and local data

| Environment variable | Purpose |
| --- | --- |
| `AUTOHARNESS_PROVIDER` | `gemini` or `router`; defaults to `gemini` |
| `GEMINI_API_KEY` | Optional Google AI Studio credential used only by the Gemini adapter; otherwise paste the key in the app |
| `AUTOHARNESS_ROUTER_API_KEY` | Optional router credential; otherwise paste the key in the app |
| `AUTOHARNESS_ROUTER_BASE_URL` | Required router base URL ending in `/` when the router is selected |
| `AUTOHARNESS_ROUTER_PROJECT` | Optional stable cache and rate-limit identity; otherwise derived from the base URL |
| `AUTOHARNESS_ROUTER_AUTH_HEADER` | Authentication header name; defaults to `Authorization` |
| `AUTOHARNESS_ROUTER_AUTH_SCHEME` | Authentication value prefix; defaults to `Bearer`; an empty value sends only the credential |
| `AUTOHARNESS_ROUTER_MODELS_PATH` | Relative model-discovery path; defaults to `v1/models` |
| `AUTOHARNESS_ROUTER_CHAT_PATH` | Relative streamed-chat path; defaults to `v1/chat/completions` |
| `AUTOHARNESS_PROVIDER_TIMEOUT_MS` | Optional positive catalog and pre-stream dispatch timeout |
| `AUTOHARNESS_PROVIDER_IDLE_TIMEOUT_MS` | Optional positive maximum silence between normalized stream events |
| `AUTOHARNESS_PROVIDER_RETRY_ATTEMPTS` | Optional positive catalog and pre-stream attempt bound |
| `AUTOHARNESS_PROVIDER_CONCURRENCY` | Optional positive concurrent-request limit for one provider project |
| `AUTOHARNESS_PROVIDER_RATE_REQUESTS` | Optional positive request count for one provider-project rate window |
| `AUTOHARNESS_PROVIDER_RATE_WINDOW_MS` | Optional positive provider-project rate-window duration |
| `AUTOHARNESS_CATALOG_REFRESH_MS` | Optional positive fresh-cache interval |
| `AUTOHARNESS_CATALOG_MAX_STALE_MS` | Optional maximum stale fallback age; must not be shorter than the refresh interval |
| `AUTOHARNESS_DATA_DIR` | Optional absolute override for the application data directory |
| `AUTOHARNESS_WORKSPACE` | Optional absolute filesystem and process workspace root; defaults to the canonical current directory |
| `AUTOHARNESS_LOG` | Log level: `off`, `error`, `warn`, `info`, `debug`, or `trace`; defaults to `info` |

Without an override, AutoHarness uses the platform application-data location:

- Windows: `%LOCALAPPDATA%\AutoHarness`
- macOS: `$HOME/Library/Application Support/AutoHarness`
- Linux and other Unix systems: `$XDG_DATA_HOME/autoharness`, or `$HOME/.local/share/autoharness` when `XDG_DATA_HOME` is unset

The directory contains:

| File | Purpose |
| --- | --- |
| `autoharness.sqlite3` | Durable schema-v1 session events, rebuilt projections, and integrity-checked provider-neutral catalog snapshots |
| `autoharness.log` | Content-free structured lifecycle and operational trace events |
| `autoharness.writer.lock` | Exclusive writer lease that prevents two processes from mutating the same store |
| `artifacts/` | Content-addressed full tool output retained when the model-visible inline result is truncated |

On startup, AutoHarness replays the active session from authoritative events.
An attempt interrupted before provider dispatch becomes a retryable failure, while an attempt interrupted after dispatch becomes an explicit unknown outcome instead of a fabricated success.
An unanswered tool permission remains pending after restart.
A tool interrupted after its durable start boundary becomes unknown and is not executed again automatically.

## Development

Run the verified baseline gates from the repository root:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked --no-deps -- -D warnings
cargo test --workspace --all-targets --all-features --locked --no-fail-fast
```

The local Phase 3 validation passes formatting, strict Clippy, warning-denied rustdoc, doctests, and the full workspace test suite.
The suite covers both production adapters, fragmented native function calls, durable permission and tool transitions, capability confinement, every run-budget dimension, bounded artifacts, permission UI behavior, interruption recovery, a composed allow-execute-continue-reopen path, SQLite replay, terminal restoration, and credential redaction.
A one-byte-fragmented Gemini Interactions fixture covers streamed function arguments, and a composed SQLite-backed test proves an unknown tool name is force-denied, returned to the provider, repaired in the same bounded attempt, and replayed after restart.
A Phase 3.1 PTY smoke run pressed `Ctrl+N` from the credential overlay, observed the durable new-session confirmation, exited through `Ctrl+C`, restored the terminal, and removed the isolated test data afterward.
Opt-in ignored live probes are available in each provider crate and retain only structural event assertions.
With the corresponding runtime credentials configured, run `cargo test --locked -p autoharness-provider-gemini --test live_compat -- --ignored` for Gemini and `cargo test --locked -p autoharness-provider-openai --test live_compat -- --ignored` for the configured router.
A PTY smoke run without a Gemini credential rendered the complete 80-by-24 terminal interface, confined its SQLite, log, and lock files to an isolated absolute data directory, exited successfully through `Ctrl+C`, and restored the terminal.
A credential-overlay PTY smoke run pasted a sentinel through bracketed paste, displayed only the fixed mask, cleared it on dismissal, reopened an empty editor through `Ctrl+K`, found no sentinel bytes in the application files, and restored the terminal on exit.
A Phase 2 PTY smoke run used a local OpenAI-compatible fixture to discover and select a router model, durably admit a prompt, render a completed streamed response, restart with the fixture offline, and restore the selected model and transcript from replay plus the fresh catalog cache.
The isolated SQLite, log, and lock files contained no router credential bytes, and both runs restored the terminal.

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

## License and contributions

AutoHarness is released under the [MIT License](LICENSE).
See [CONTRIBUTING.md](CONTRIBUTING.md) for the branching model, required validation gates, and documentation expectations before opening a pull request.
