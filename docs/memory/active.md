# Active memory

**Last reviewed:** 2026-08-23

**Phase:** 3.6 local profile and provider connection center

**Status:** Phase 3.6 is implemented locally on `feat/phase-3-6-profile-center`; focused tests, strict baseline gates, a real profile-management PTY journey, responsive visual review, and a Windows Credential Manager smoke pass

## Current objective

Promote Phase 3.6 through cross-platform CI and platform-vault evidence, then begin Phase 3.7 from the stable profile read models, typed intents, focus rules, and provider-switching workflow.

## Current repository state

- The repository contains an eleven-crate Rust 2024 workspace pinned to Rust 1.97.1 and a runnable `autoharness` terminal binary.
- `autoharness-domain` and `autoharness-engine` define schema-v1 commands and events, deterministic replay, durable attempt and tool lifecycles, immutable run limits, explicit permissions, cancellation state, usage, safe failures, and retry lineage.
- `autoharness-tool` provides the trusted versioned tool registry, deny, ask, and allow policy, restart-aware run budgets, content-addressed artifacts, and workspace filesystem, direct-process, and exact-origin HTTP capability ports.
- `autoharness-provider` exposes provider-neutral catalog, streaming, versioned tool definition, complete tool call, and tool-result ports.
- `autoharness-provider` also provides shared SSE framing, fixture conformance assertions, capability preflight, deadlines, bounded pre-stream retries, concurrency, per-project rate limits, and catalog freshness policy.
- `autoharness-provider-gemini` implements paginated Google model discovery, stable Interactions v1 streaming, a narrow pre-stream Generate Content fallback, cancellation, retry classification, limits, environment or in-app credential admission, and credential redaction.
- `autoharness-provider-openai` implements configurable OpenAI-compatible router discovery and streamed chat completions with a validated base URL, configurable sensitive authentication header, pagination, cumulative usage, cancellation, limits, and credential redaction.
- `autoharness-store` and `autoharness-store-sqlite` provide an event-authoritative store, transactional projections, WAL-mode local durability, idempotent append, migration verification, projection rebuilding, and an integrity-checked provider-neutral model-catalog cache.
- `autoharness-settings` resolves defaults, user file, workspace file, environment, and override layers in fixed precedence with per-key provenance; schema 2 adds optional profile default models and non-secret recovery records while schema-v1 documents migrate on mutation (ADR-0012).
- The atomic application-owned `autoharness.profiles.json` document stores validated named profiles, non-secret connection fields, optional default models, opaque credential references, and bounded credential-recovery records.
- `autoharness-app::vault` defines the credential-vault port with an operating-system implementation through the `keyring` crate and a fake implementation for tests; Windows Credential Manager save, load, replace, and delete passed the opt-in platform smoke.
- Startup and runtime switching resolve one provider-matched credential source in precedence order: environment, then the active profile's vault entry, then session-only; locked or unavailable vaults preserve offline use without plaintext fallback.
- Active Gemini and router profiles rebuild their exact provider adapters and connection fields at runtime; a compatible selected model can be saved as the profile default and reapplied after catalog refresh.
- The terminal receives live safe settings and profile projections; `Ctrl+,` shows provenance and `Ctrl+G` opens the full-screen searchable Profiles and Providers center.
- Sentinel tests seed unique marker secrets through save, replace, disconnect, delete, failed recovery, and debug rendering and find no leakage into application-owned durable files.
- `autoharness-tui` adds masked profile-scoped credential entry, guided Gemini and router forms, duplicate-without-credential behavior, activate, test, default-model, disconnect, delete, source, connection, and recovery states over typed intents.
- Phase 3.6 responsive profile layouts are verified at 120x50, 120x40, 80x24, 60x18, and 40x12, and the actual PTY journey creates, switches, duplicates, cancels and confirms deletion, and exits cleanly without shell setup.
- Phase 3.4 usability surfaces are implemented: a `Ctrl+/` searchable command palette and generalized slash commands over one shared typed command table, an `F1` contextual help overlay whose section order follows the surface help was opened from, an enriched header status surface (profile, credential source, selected model, attempt state, token usage) that degrades at narrow widths, `Ctrl+Up`/`Ctrl+Down` composer history with draft stashing, `Ctrl+F` transcript search with match counting and jump-to-match wrapped-row scrolling, `Ctrl+Y` OSC 52 transcript copy plus `/export` Markdown export written beside the database from durable events, structured collapsible tool rows with `Ctrl+X` expand toggle, and confirm-gated archiving with one-shot `Ctrl+Z` undo in the session browser.
- Phase 3.5 real-PTY scenarios cover credential-free first run and restoration, returning-profile offline replay, settings provenance, resize and restart, multi-session switching and destructive confirmations, invalid-call repair, permission deny and allow with replay, and forced-shutdown recovery.
- The PTY scenarios are ignored in ordinary non-terminal test hosts and run serially in a dedicated Windows, macOS, and Linux CI matrix step.
- The opt-in `benchmark-instrumentation` feature emits content-free monotonic first-draw, input, dispatch, decoded-chunk, and rendered-revision markers over loopback UDP, and the isolated `terminal_latency` runner reports their correlated harness intervals separately from network time.
- Environment credentials are paired only with the effective provider, active-profile identity remains visible when environment credentials override vault storage, and locked or unavailable vault access degrades to session-only mode.
- Corrupt catalog caches are discarded before live replacement, the configured router now has both plain-chat and function-calling live probes, and the terminal release checklist covers security, accessibility, restoration, documentation, benchmark provenance, and database rollback.
- The cross-platform PTY gate exposed and now covers Windows cursor-position report handling, inactive-session mutation isolation, reverse-causation event deletion, visible durable tool rows, and notice clearing when destructive confirmation is cancelled.
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
- Formatting, strict Clippy, 307 workspace tests across 41 suites, warning-denied rustdoc, doctests, benchmark gates, and documentation links pass locally after Phase 3.5 implementation.
- A real Windows terminal smoke selected a loopback router model, submitted a prompt, rendered the completed response, emitted one correctly correlated marker chain, exited with code 0, and restored the terminal.
- Continuous integration defines formatting, Clippy, documentation, doctest, native Linux, Windows, and macOS gates, a serial cross-platform PTY scenario gate, and separate formatting, Clippy, and test gates for the isolated benchmark workspace.

## Recently completed

- Implemented Phase 3.6 end to end: ADR-0013 recovery semantics, settings schema 2, `ProfileManager`, distinct per-profile vault entries, runtime adapter switching, safe connection tests, profile defaults, the responsive `Ctrl+G` center, composed lifecycle coverage, PTY coverage, and Windows vault smoke evidence.
- Implemented the Phase 3.5 PTY scenario harness and six release scenarios covering all planned terminal paths without live credentials.
- Expanded the roadmap with Phase 3.6 profile and provider management, Phase 3.7 unified TUI shell and navigation, Phase 3.8 personalization and accessibility, and Phase 3.9 integrated terminal product validation.
- Added monotonic benchmark markers, the real-PTY terminal latency runner, report validation, reference-machine fields, and content-free loopback correlation.
- Added the configured-router plain-chat live probe, locked-vault degradation, corrupt-cache live replacement coverage, and provider-matched environment credential resolution.
- Added and routed the terminal release checklist, updated benchmark and root documentation, and kept the baseline and isolated benchmark gates green.
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

1. Open the Phase 3.6 pull request from `feat/phase-3-6-profile-center` into `dev` and require green formatting, Clippy, documentation, full workspace, and serial PTY jobs.
2. Run the opt-in platform-vault smoke with `AUTOHARNESS_RUN_PLATFORM_VAULT_SMOKE=1` on macOS Keychain and Linux Secret Service environments and retain content-free pass or fail evidence.
3. Review the cross-platform profile-center PTY result, especially Alt-key decoding, focus restoration, narrow layouts, and terminal restoration.
4. Begin Phase 3.7 only after Phase 3.6 promotion, preserving the profile projections and typed intents while replacing overlay-led navigation with the route-based shell.
5. Preserve configured-router live, approved reference-machine, complete accessibility, migration, rollback, and final release-checklist evidence for Phase 3.9.

## Open questions

- What reference machine should define the final Phase 3.9 startup and stream-overhead budgets?

## Blockers

Local Windows implementation evidence is complete.
Promotion still requires refreshed baseline and serial PTY CI on Windows, macOS, and Linux plus platform-vault smoke evidence on macOS and Linux.
A configured router endpoint and approved reference machine remain Phase 3.9 evidence prerequisites rather than Phase 3.6 implementation blockers.

## Handoff note

Phase 3.6 is complete locally and committed frequently on `feat/phase-3-6-profile-center`.
Merge only after cross-platform PTY and ordinary baseline jobs pass and macOS and Linux vault results are reviewed.
The authoritative implementation contract is [ADR-0013](../adr/0013-use-durable-credential-mutation-recovery.md), and the current settings boundary is [SETTINGS](../architecture/SETTINGS.md).
