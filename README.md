# AutoHarness

AutoHarness is an open-source agent runtime designed to improve the infrastructure around current language models.
Its long-term goal is to learn from durable execution traces and safely improve prompts, policies, routing, tools, memory, and code through reproducible evaluations and gated promotion.

The Phase 3 safe-execution substrate and Phase 3.2 through Phase 3.7 terminal product slices are implemented locally.
The Rust terminal application runs a durable, resumable tool loop through Google AI Studio Gemini or a configurable OpenAI-compatible model router, with a responsive route-based shell, offline sessions, named provider profiles, operating-system-vault credentials, explicit recovery states, and one deterministic modal owner.
Versioned filesystem, direct-process, and HTTP tools use scoped deny, ask, or allow decisions, bounded output, content-addressed artifacts, immutable run budgets, and explicit interruption recovery.
Shared provider policy applies timeouts, bounded pre-stream retries, concurrency, per-project rate limits, capability preflight, and a durable model-catalog cache with explicit refresh and stale fallback rules.
Gemini function arguments are aggregated across streamed deltas, malformed model calls are durably denied and returned for bounded repair, and failed prompts are not replayed into unrelated later turns.
Reviewed live Gemini plain-chat and function-calling probes passed on 2026-08-22; reviewed configured-router release-candidate evidence remains open.

## GUI preview

AutoHarness now includes an initial Tauri 2 and React desktop preview built on a renderer-neutral Rust client contract.
The preview is not the default client, a packaged release, or a claim of feature parity with the terminal application.
Install the pinned frontend workspace dependencies from the repository root:

```text
pnpm install
```

Launch the native development preview:

```text
pnpm gui:desktop
```

For browser-only interface development against deterministic fixture state, run:

```text
pnpm gui:dev
```

The browser fixture exercises presentation, interaction, recovery, and responsive states without connecting to the authoritative Rust runtime or writing real sessions and credentials.
Use the native preview when validating the Tauri carrier and engine integration.
The macOS preview requires macOS 11.3 or later so its system WebView provides the layout and protocol features used by the interface.
This preview intentionally permits one renderer connection per process while native frame delivery is outstanding.
If the development renderer reloads before acknowledging its last native frame, restart AutoHarness to establish a fresh bounded channel.

## Terminal application compatibility and reference

The repository pins Rust 1.97.1 through `rust-toolchain.toml`.
Run the binary from the repository root:

```text
cargo run --locked -p autoharness-app --bin autoharness
```

For a globally available short command, install the `ah` launcher once:

```text
cargo install --locked --path crates/autoharness-app --bin ah
ah
```

Cargo installs `ah.exe` into `%USERPROFILE%\.cargo\bin` on Windows.
Ensure that directory is on `PATH`.

Gemini remains the default provider.
When no credential is available, AutoHarness opens a masked terminal overlay.
Paste or type the selected provider's API key and press `Enter` to validate it and load the model catalog.
Press `Ctrl+K` to open the credential overlay again if the key is rejected, expires, or needs to be replaced.

The application transfers the key through redacted, zeroizing in-memory values and never writes it to configuration, durable session state, logs, transcripts, or model-visible content.
The pasted key is intentionally forgotten when the process exits.
`GEMINI_API_KEY` and `AUTOHARNESS_ROUTER_API_KEY` are optional startup overrides for automation or managed launches.
Do not put the key in a repository file or command-line argument.

### Provider profiles and the credential vault

A named provider profile stores validated non-secret connection fields, an optional default model, and an opaque credential reference.
The raw key lives only in the operating-system credential vault (Windows Credential Manager, macOS Keychain, or Linux Secret Service).
Press `Ctrl+G` to open Profiles and Providers, where you can create, edit, duplicate, activate, test, disconnect, and delete Gemini and router profiles.
Press `Alt+K` inside that surface to save or replace the selected profile's key, and press `Alt+M` to use the active session's selected model as that profile's default.
Saving a credential is explicit and opt in, and duplicated profiles never share credential linkage.
When an active profile has a stored credential, AutoHarness reconnects after restart without asking for the key again.
If the vault is unavailable, AutoHarness stays usable offline and falls back to environment or session-only entry; it never creates its own plaintext store.
Interrupted profile and credential mutations retain only bounded non-secret recovery records and are reconciled idempotently after restart.
Press `Ctrl+,` to open the read-only settings provenance overlay, which shows the effective provider and safe credential source (`environment`, `credential vault`, or `session only`).

To use an OpenAI-compatible router, set `AUTOHARNESS_PROVIDER=router`, a base URL ending in `/`, and the router credential.
Router credentials require HTTPS except for loopback HTTP endpoints such as `http://127.0.0.1:PORT/`.
The default relative endpoints are `v1/models` and `v1/chat/completions`.
Routers mounted below a path such as `https://router.example/api/` retain that path when relative endpoints are resolved.

### Unified terminal shell

AutoHarness always has one primary route: Chat, Sessions, Profiles, Settings, or Help.
Wide terminals show a persistent navigation rail with the local profile, active provider, credential source, model, attempt state, usage, and catalog health.
Narrow terminals show compact route tabs and the same prioritized status without duplicating it inside every page.
Use `Alt+1` through `Alt+5` to switch routes directly.
The legacy `Ctrl+L`, `Ctrl+G`, `Ctrl+,`, and `F1` shortcuts still open Sessions, Profiles, Settings, and Help.
Model selection, credential entry, command search, transcript search, permission decisions, and destructive confirmations share one modal slot and restore the exact prior route and focus when dismissed.

## Controls

| Action | Key |
| --- | --- |
| Switch Chat, Sessions, Profiles, Settings, or Help | `Alt+1` through `Alt+5` |
| Open the command palette over the current route | `Ctrl+/` |
| Send the composed prompt | `Ctrl+S` or `Ctrl+Enter` |
| Insert a newline | `Enter` |
| Open the model picker | `Ctrl+P` |
| Create a fresh durable session | `Ctrl+N` |
| Open Sessions | `Alt+2` or `Ctrl+L` |
| Open Profiles and Providers | `Alt+3` or `Ctrl+G` |
| Create or edit a provider profile | `Alt+N` or `Alt+E` inside Profiles and Providers |
| Duplicate profile configuration without its key | `Alt+D` inside Profiles and Providers |
| Save or replace the selected profile key | `Alt+K` inside Profiles and Providers |
| Test the selected provider connection | `Alt+T` inside Profiles and Providers |
| Use the selected model as the active profile default | `Alt+M` inside Profiles and Providers |
| Disconnect or delete the selected profile | `Alt+X`, or `Delete`, then `Y` to confirm |
| Search sessions | Type while Sessions is active |
| Rename, archive, or unarchive the highlighted session | `Ctrl+R`, `Ctrl+A`, `Ctrl+U` in Sessions |
| Delete the highlighted session | `Ctrl+D` then `Y` to confirm in Sessions |
| Slash commands | `/sessions`, `/open <n>`, `/rename <title>`, `/archive`, `/unarchive`, `/delete` in the composer |
| Open or replace the API key | `Ctrl+K` |
| Open Settings and provenance | `Alt+4` or `Ctrl+,` |
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

Run the verified Rust baseline gates from the repository root:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked --no-deps -- -D warnings
cargo test --workspace --all-targets --all-features --locked --no-fail-fast
```

Run the verified frontend gates when changing the GUI, its transport contract, or the desktop bridge:

```text
pnpm gui:typecheck
pnpm gui:test
pnpm gui:build
```

The terminal application remains the compatibility and behavioral reference while the GUI preview advances through its documented parity and release gates.

The local Phase 3.7 validation passes formatting, strict Clippy, the full workspace suite, and the serial Windows PTY matrix.
The TUI suite covers typed routes, single-overlay ownership, permission preemption, exact focus restoration, hidden-confirmation clearing, draft and selection preservation, responsive rail and tab layouts, every primary route, explicit empty and recovery states, and fixed-size goldens.
The real routed-shell PTY journey switches every route with portable Alt chords, restores Settings after a model-picker overlay, preserves a composer draft, creates and lists a second durable session, cancels a scoped deletion confirmation, resizes to 40x12, exits cleanly, and restores the terminal.
The suite continues to cover both production adapters, fragmented native function calls, durable tool transitions, capability confinement, profile and credential mutation recovery, SQLite replay, terminal restoration, and credential redaction.
The opt-in platform-vault smoke verifies save, load, replace, and delete without printing secret values; set `AUTOHARNESS_RUN_PLATFORM_VAULT_SMOKE=1` and run its ignored test on the target operating system.
An instrumented release smoke ran the routed shell through the real PTY loopback latency runner and produced valid correlated startup, dispatch, and rendered-delta intervals without measuring network time.
Opt-in ignored live probes are available in each provider crate and retain only structural event assertions.
With the corresponding runtime credentials configured, run `cargo test --locked -p autoharness-provider-gemini --test live_compat -- --ignored` for Gemini and `cargo test --locked -p autoharness-provider-openai --test live_compat -- --ignored` for the configured router.
A PTY smoke run without a Gemini credential rendered the complete 80-by-24 terminal interface, confined its SQLite, log, and lock files to an isolated absolute data directory, exited successfully through `Ctrl+C`, and restored the terminal.
A credential-overlay PTY smoke run pasted a sentinel through bracketed paste, displayed only the fixed mask, cleared it on dismissal, reopened an empty editor through `Ctrl+K`, found no sentinel bytes in the application files, and restored the terminal on exit.
A Phase 2 PTY smoke run used a local OpenAI-compatible fixture to discover and select a router model, durably admit a prompt, render a completed streamed response, restart with the fixture offline, and restore the selected model and transcript from replay plus the fresh catalog cache.
The isolated SQLite, log, and lock files contained no router credential bytes, and both runs restored the terminal.

## Performance evidence

The checked-in [benchmark environment](benchmarks/README.md) measures durable event append with synchronous projections, transcript-read throughput, warm SQLite reopen with strict replay, and the three terminal harness latency boundaries required before Phase 4.
It also includes a PowerShell idle resident-memory sampler, an opt-in instrumented PTY runner, and a reference-machine record template.

```text
cargo run --release --locked --manifest-path benchmarks/Cargo.toml -- --output benchmarks/results/phase1-<machine>-<date>.json
```

The terminal runner correlates first draw, input acceptance, provider dispatch, decoded provider chunks, and rendered revisions over a content-free loopback side channel defined by the [instrumentation contract](benchmarks/instrumentation-contract.md).
Harness overhead and provider-network time remain separate, and unavailable network metrics are never represented as zero.

## Project documentation

- [Project plan](docs/PROJECT_PLAN.md)
- [Architecture overview](docs/architecture/OVERVIEW.md)
- [GUI architecture](docs/architecture/GUI.md)
- [GUI implementation plan](docs/design/GUI_IMPLEMENTATION_PLAN.md)
- [GUI design system](docs/design/GUI_DESIGN_SYSTEM.md)
- [Persistent memory architecture](docs/architecture/PERSISTENT_MEMORY.md)
- [Repository memory](docs/memory/README.md)
- [Architecture decision records](docs/adr/README.md)
- [Terminal release checklist](docs/release/TERMINAL_RELEASE_CHECKLIST.md)
- [Reference-project research](docs/research/agent-memory-patterns.md)

## Guiding principles

- Keep the engine independent from terminal and GUI renderers, desktop carriers, and model providers.
- Evolve shared client behavior through the versioned renderer-neutral contract instead of renderer-owned runtime logic.
- Treat every provider response as a typed, replayable event stream.
- Preserve provenance for every memory, decision, experiment, and promoted change.
- Evaluate proposed improvements before promotion and retain rollback paths.
- Keep secrets out of source control, logs, transcripts, and model-visible memory.
- Prefer native performance, bounded concurrency, deterministic recovery, and explicit permissions.

## License and contributions

AutoHarness is released under the [MIT License](LICENSE).
See [CONTRIBUTING.md](CONTRIBUTING.md) for the branching model, required validation gates, and documentation expectations before opening a pull request.
