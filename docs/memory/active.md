# Active memory

**Last reviewed:** 2026-08-23

**Phase:** 3.4 - TUI usability and discoverability

**Status:** Phase 3.4 implemented locally on `feat/phase-3-4-tui-usability`; all slices land behind tests with baseline gates green, promotion into `dev` remains open

## Current objective

Promote Phase 3.4 from `feat/phase-3-4-tui-usability` into `dev`, then proceed through Phase 3.5 terminal release hardening.

## Current repository state

- The repository contains an eleven-crate Rust 2024 workspace pinned to Rust 1.97.1 and a runnable `autoharness` terminal binary.
- `autoharness-domain` and `autoharness-engine` define schema-v1 commands and events, deterministic replay, durable attempt and tool lifecycles, immutable run limits, explicit permissions, cancellation state, usage, safe failures, and retry lineage.
- `autoharness-tool` provides the trusted versioned tool registry, deny, ask, and allow policy, restart-aware run budgets, content-addressed artifacts, and workspace filesystem, direct-process, and exact-origin HTTP capability ports.
- `autoharness-provider` exposes provider-neutral catalog, streaming, versioned tool definition, complete tool call, and tool-result ports.
- `autoharness-provider` also provides shared SSE framing, fixture conformance assertions, capability preflight, deadlines, bounded pre-stream retries, concurrency, per-project rate limits, and catalog freshness policy.
- `autoharness-provider-gemini` implements paginated Google model discovery, stable Interactions v1 streaming, a narrow pre-stream Generate Content fallback, cancellation, retry classification, limits, environment or in-app credential admission, and credential redaction.
- `autoharness-provider-openai` implements configurable OpenAI-compatible router discovery and streamed chat completions with a validated base URL, configurable sensitive authentication header, pagination, cumulative usage, cancellation, limits, and credential redaction.
- `autoharness-store` and `autoharness-store-sqlite` provide an event-authoritative store, transactional projections, WAL-mode local durability, idempotent append, migration verification, projection rebuilding, and an integrity-checked provider-neutral model-catalog cache.
- `autoharness-settings` resolves defaults, user file, workspace file, environment, and override layers in fixed precedence with per-key provenance, safe malformed-layer diagnostics, fail-closed future schema versions, and a workspace allowlist that cannot weaken credential or permission policy (ADR-0012).
- The application-owned profile document `autoharness.profiles.json` stores validated named profiles with non-secret connection fields and opaque credential references, written atomically with a `.bad` backup on corruption.
- `autoharness-app::vault` defines the credential-vault port with an operating-system implementation through the `keyring` crate (Windows Credential Manager, macOS Keychain, Linux Secret Service) and a fake implementation for tests; secrets are validated, bounded, zeroizing, and never rendered by debug output.
- Startup resolves one effective credential source in precedence order: environment, then the active profile's vault entry, then session-only entry; a missing or locked vault degrades to offline-usable session-only operation without creating any plaintext fallback store (ADR-0009, accepted).
- An active router profile supplies its own base URL, project identity, and authentication header to the adapter; without a profile the documented environment configuration still applies.
- The terminal receives a safe settings projection and renders a non-modal `Ctrl+,` overlay naming the effective provider and credential source (`environment`, `credential vault`, or `session only`).
- Sentinel tests seed a unique marker secret through save, rotate, disconnect, and delete flows and scan every durable file plus rendered debug output to prove no leakage.
- `autoharness-tui` provides a Ratatui model/update/view client with masked zeroizing API-key entry, a searchable model picker, Unicode multiline composer, streaming transcript, scoped tool permission overlay, cancellation, retry, usage, errors, scrolling, compact rendering, a searchable session browser, per-session drafts, and settings provenance display.
- Phase 3.4 usability surfaces are implemented: a `Ctrl+/` searchable command palette and generalized slash commands over one shared typed command table, an `F1` contextual help overlay whose section order follows the surface help was opened from, an enriched header status surface (profile, credential source, selected model, attempt state, token usage) that degrades at narrow widths, `Ctrl+Up`/`Ctrl+Down` composer history with draft stashing, `Ctrl+F` transcript search with match counting and jump-to-match wrapped-row scrolling, `Ctrl+Y` OSC 52 transcript copy plus `/export` Markdown export written beside the database from durable events, structured collapsible tool rows with `Ctrl+X` expand toggle, and confirm-gated archiving with one-shot `Ctrl+Z` undo in the session browser.
- Tool definitions are advertised only for positively identified model support, and the current single safe-agent interaction mode enables the exact built-in registry.
- Gemini Interactions function-call arguments are buffered across streamed deltas and emitted only after a complete bounded JSON object is available.
- Unknown names and invalid argument shapes become durable `InvalidToolCall` no-authority proposals, are force-denied even under permissive policy, and return a deterministic result for bounded model repair.
- Provider request history includes completed turns and the current input while excluding unrelated prior failed or cancelled prompts.
- `Ctrl+N` creates and activates a fresh durable session even when credentials or the catalog are unavailable, and `Ctrl+L` opens a searchable session browser over every durable session.
- The session lifecycle is event-sourced under [ADR-0011](../adr/0011-use-event-sourced-session-lifecycle.md): rename, archive, and unarchive are schema-v1 commands and events, archived sessions accept only unarchive, switching replays the target history before any projection swap and is refused while an attempt or permission prompt is active.
- Deleting a session exports the complete authoritative event stream to a documented provider-neutral JSON archive beside the database first ([format](../architecture/SESSION_EXPORT.md)); export failure aborts deletion, and version-mismatched deletes fail closed.
- Session titles use the validated `SessionTitle` value type (non-empty, bounded, no control characters) and store titles live in SQLite migration 3.
- Non-secret runtime configuration remains environment-driven for values not yet covered by profiles, and the in-app credential overlay remains available under [ADR-0005](../adr/0005-use-ephemeral-in-app-credentials.md) for session-only entry.
- The user-observed 2026-08-22 Gemini wire shape is represented by a recorded structural SSE fixture that survives one-byte fragmentation without emitting empty arguments.
- On 2026-08-22 both opt-in Gemini live probes passed against production Google AI Studio using current-generation models: plain chat with the complete registry streamed text to completion, and streamed function calling produced one complete bounded `http_request` call before a tool-calls completion.
- Stable failure codes, compact safe attempt references, retry actions, and the global fresh-session action are rendered in failed transcript rows and fixed-size golden buffers.
- Formatting, strict Clippy, full workspace tests, warning-denied rustdoc, doctests, fixed-size renders, and the actual credential-free `Ctrl+N` PTY flow pass locally after Phase 3.2 documentation reconciliation.
- A checked-in isolated benchmark environment measures durable append, projection reads, and warm SQLite recovery without provider requests, and includes an idle resident-memory sampler.
- Continuous integration defines formatting, Clippy, documentation, doctest, native Linux, Windows, and macOS gates, plus separate formatting, Clippy, and test gates for the isolated benchmark workspace.

## Recently completed

- Implemented Phase 3.4 across eight test-first slices: command palette with generalized slash commands, contextual help overlay and footer affordances, enriched header status surface, composer history recall, transcript search, OSC 52 copy plus Markdown export intent satisfied from durable events in the coordinator and engine actor, structured collapsible tool rows, and archive confirmation with `Ctrl+Z` undo.
- Verified Phase 3.4 surfaces visually at 120x50, 120x40, 80x24, 60x18, and 40x12 through a checked-in ignored review harness, and updated the fixed-size goldens for the new header and footer.
- Implemented Phase 3.3: layered typed settings resolution with provenance, validated provider profiles with atomic durable storage, the operating-system credential-vault port, startup credential resolution with safe degradation, TUI settings provenance display, and secret sentinel coverage.
- Accepted ADR-0009 (operating-system-backed credential profiles) and recorded ADR-0012 (typed settings resolver with layered precedence).
- Added the `autoharness-settings` crate with fixed-order layer merging, workspace allowlist enforcement, schema-version handling, and unknown-active-profile validation.
- Added the app library target exposing vault, profiles, and credential-resolution modules to integration tests.
- Wired launch resolution into `main.rs`: profile document plus live environment feed the resolver, the active profile selects the adapter and connection fields, and resolved provenance publishes to the terminal before the first draw.
- Verified a returning profile reconnects from the fake vault across store reopen without prompting and without writing secret bytes anywhere on disk.
- Implemented Phase 3.2: schema-v3 session titles, enriched summaries, event-sourced rename/archive/unarchive with aggregate guards, atomic version-checked deletion, pre-deletion export, and a searchable `Ctrl+L` browser with slash commands and per-session composer drafts.
- Verified the composed two-session create, rename, archive, unarchive, switch, restart, and replay-equivalence path against real SQLite.
- Recorded the lifecycle contract in [ADR-0011](../adr/0011-use-event-sourced-session-lifecycle.md) and the export format in [SESSION_EXPORT](../architecture/SESSION_EXPORT.md).
- Buffered Gemini Interactions function calls until every streamed `partial_arguments` fragment forms the complete bounded JSON object.
- Added capability-aware tool advertisement with backward-compatible catalog decoding and positive support required before exposing functions.
- Added durable force-denied invalid-call proposals, deterministic provider repair results, no-authority authorization checks, and content-free rejection telemetry.
- Excluded prior failed and cancelled prompts from unrelated future provider requests while preserving explicit retry and completed history behavior.
- Added a global typed `Ctrl+N` intent, durable session creation, session-identity-aware TUI projection replacement, fixed-size footer affordances, and a successful credential-overlay PTY smoke run.
- Added stable UI failure codes, compact safe attempt references, and concrete retry or fresh-session recovery actions.
- Added opt-in ignored Gemini plain-chat, Gemini HTTP-function, and configured-router HTTP-function compatibility probes that retain only structural assertions.
- Completed the Phase 3 safe agent execution path without adding provider-native payloads or concrete capabilities to the engine.
- Recorded the durable capability boundary in [ADR-0007](../adr/0007-use-durable-capability-tool-runtime.md).

## Immediate next actions

1. Open the Phase 3.4 pull request from `feat/phase-3-4-tui-usability` into `dev` and wait for green CI.
2. Add in-terminal flows to create, replace, test, and disconnect provider credentials from the settings overlay so the full Phase 3.3 exit criteria close without shell access.
3. Run the opt-in configured-router plain-chat and HTTP-function compatibility probes against the intended router project and record only the pass or fail matrix and version provenance.
4. Proceed through Phase 3.5 release hardening in [the revised project plan](../PROJECT_PLAN.md).

## Open questions

- What reference machine should define startup and stream-overhead benchmarks?
- Should the settings overlay grow direct profile editing before the remaining credential flows land?

## Blockers

None for the completed local Phase 3.4 implementation.
A configured router endpoint plus credential is required for the remaining Phase 3.1 live-provider exit evidence; the Gemini half closed on 2026-08-22.
An approved reference machine is required before recording authoritative latency results in Phase 3.5.

## Handoff note

Phase 3.4 is complete locally on `feat/phase-3-4-tui-usability` with all baseline gates passing; merge through a pull request into `dev` per [ADR-0003](../adr/0003-use-main-dev-feature-branches.md).
The Phase 3.1 configured-router live matrix stays open until runtime credentials are available.
Use the ignored structural compatibility probes first, then verify the same plain-chat and approved HTTP-tool paths through the real terminal without retaining provider content or credentials.
