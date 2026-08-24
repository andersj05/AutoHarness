# AutoHarness project plan

**Status:** Active

**Last updated:** 2026-08-23

**Planning horizon:** Foundation through distributed self-improvement

## Outcome

Build an open-source, provider-neutral agent harness that feels instantaneous in a terminal, preserves enough execution state to recover and replay work, and improves its behavior through evidence rather than unchecked self-modification.

The first end-to-end slice connects to Google AI Studio, discovers available models dynamically, lets the user select one, streams a response, and restores the session after restart. The next provider slice connects to a configurable model router.

## Product principles

1. **The engine is the product.** The terminal is the first client, not the owner of orchestration logic.
2. **Durability precedes autonomy.** Inputs, tool state, model attempts, and behavior-changing decisions must be recoverable.
3. **Protocols are normalized at the edge.** Provider-specific request and stream formats stop at provider adapters.
4. **Context is admitted, not concatenated ad hoc.** Every model-visible memory item has provenance and a deterministic admission boundary.
5. **Improvement is an experiment.** A candidate must be reproducible, evaluated, promotable, and reversible.
6. **Local first, remotely scalable.** Begin as a modular single binary; preserve contracts that allow a daemon and remote workers later.
7. **Security is capability based.** Secrets, tools, plugins, and stored memory receive the least authority required.
8. **Performance is measured.** Optimize startup, first draw, stream overhead, memory use, and recovery using benchmarks rather than intuition.

## Scope

### Initial scope

- Native terminal chat with a selectable model catalog.
- Google AI Studio authentication and model discovery.
- Google streaming responses with cancellation and usage reporting.
- Configurable model-router adapter, initially supporting an OpenAI-compatible dialect.
- Durable, replayable sessions in local SQLite.
- In-terminal session, provider-profile, settings, and credential management sufficient for ordinary daily use without shell setup.
- Typed provider, event, storage, and context contracts.
- Repository-native project memory and ADRs.
- Tests, benchmarks, tracing, and secret-redaction guarantees.

### Long-term scope

- Tool execution with permissions, budgets, and sandboxes.
- Multiple concurrent agents and resumable workflows.
- User-, workspace-, session-, and agent-scoped memory.
- Provider routing based on capability, quality, latency, availability, and cost.
- Evaluation datasets derived from failures and explicit feedback.
- Candidate generation for prompts, policies, tools, routing, memory, and source changes.
- Gated promotion, canaries, rollback, and distributed workers.
- Sandboxed extension ecosystem using versioned plugin capabilities.

### Non-goals for the first release

- Training or fine-tuning foundation-model weights.
- A web or desktop UI.
- Unrestricted shell or filesystem tools.
- Multi-tenant cloud hosting.
- Autonomous promotion of model-authored changes.
- A mandatory vector database.
- Microservices before local process boundaries are measured as insufficient.

## Delivery phases

### Phase 0: Repository foundation

**Status:** Complete

**Goal:** Make the intended product, architecture, decisions, and current work state durable before scaffolding implementation.

Deliverables:

- Root `AGENTS.md` with cross-tool working and memory instructions.
- Project plan, architecture overview, and persistent-memory specification.
- Repository memory with stable, active, and progress layers.
- ADR process and initial decisions.
- Reference-project research with commit-pinned sources.

Exit criteria:

- A new contributor or agent can identify the goal, current phase, next action, constraints, and decision history from repository files alone.
- No contradictory source of truth exists for project status or architecture.

### Phase 1: Fast terminal vertical slice

**Status:** Complete as of 2026-08-20

**Goal:** Prove the complete path from key discovery to selectable model to streamed, replayable conversation.

Deliverables:

- Rust workspace and continuous-integration baseline.
- Headless engine with typed commands and events.
- Ratatui client using a model/update/view loop.
- Gemini provider with API-key authentication, paginated model discovery, and streaming.
- SQLite migrations and repositories for sessions, durable input, and events.
- Model picker, transcript, multiline composer, cancellation, retry, and error presentation.
- Masked, ephemeral in-app API-key entry with an optional environment override.
- Structured tracing with mandatory secret redaction.

Exit criteria:

- A pasted in-app key or `GEMINI_API_KEY` is sufficient to start a session without writing the key to disk.
- Compatible models are discovered from the API rather than hardcoded.
- The user can choose a model, stream a response, cancel it, and retry safely.
- Restarting the app restores the selected model and transcript.
- Replaying stored events reconstructs the same visible session.
- Tests cover model pagination, arbitrary SSE fragmentation, cancellation, retry classification, terminal restoration, and redaction.

Completion is supported by fixture-backed provider tests and a composed integration test that selects a model, streams, cancels, retries, shuts down, reopens SQLite, and compares the recovered terminal projection.
The repository has not exercised a live Gemini network request, so that remains a separate pre-release validation item rather than claimed completion evidence.

### Phase 2: Provider and router platform

**Status:** Complete

**Goal:** Prove that provider differences remain outside the engine.

Deliverables:

- Stable `Provider`, `Catalog`, and `ModelStream` contracts.
- Capability-aware `ModelDescriptor` and provider availability state.
- OpenAI-compatible router adapter with configurable URL, authentication header, and model discovery.
- Provider conformance suite driven by recorded HTTP fixtures.
- Timeout, retry, concurrency, and per-project rate-limit middleware.
- Model-catalog cache with refresh and stale-data policy.

Exit criteria:

- The same engine and TUI session path works against Gemini and the configured router.
- Adding a fixture-only provider requires no engine or TUI changes.
- Unsupported capabilities fail before a provider request when discoverable.
- Provider-specific payloads never enter core domain types.

Completion is supported by shared conformance assertions, independent Gemini and router HTTP fixtures, provider-policy tests, SQLite cache migration and integrity tests, and a composed application test that runs router discovery and streaming through the existing coordinator, engine, store, and terminal ports.
The Phase 2 completion evidence did not exercise a live Gemini or router request, so successful live service compatibility remains a separate pre-release gate.

### Phase 3: Safe agent execution

**Goal:** Add a resumable tool loop without giving models ambient authority.

Deliverables:

- Versioned tool schemas and typed tool-call events.
- Permission engine with deny, ask, and allow decisions scoped by tool and resource.
- Filesystem, process, and HTTP capability interfaces.
- Bounded output capture and artifact storage.
- Durable tool-call lifecycle and crash recovery.
- Per-run limits for turns, time, reported tokens, output, and concurrency.
- Explicit deferral of monetary limits until trusted durable pricing snapshots exist.

Exit criteria:

- Every external side effect is attributable to a durable tool call and permission decision.
- Interrupted calls settle deterministically as completed, failed, cancelled, or unknown with an explicit recovery policy.
- A model cannot acquire a capability merely by emitting a differently shaped tool call.

**Completion:** Implemented and locally verified on 2026-08-21.

Completion is supported by stable schema-shape tests, engine lifecycle and replay tests, filesystem, process, HTTP, permission, budget, and artifact tests, fragmented and aggregate-bounded Gemini and OpenAI-compatible function-call fixtures, scrollable exact-detail terminal permission tests, explicit interruption recovery tests, and composed SQLite-backed allow, execute, continue, shutdown, and reopen tests.
The security-remediated slice asks before every default local capability, conservatively marks ambiguous started effects unknown, prevents child tool authority from outliving its attempt during live execution and recovery, rejects Windows batch programs, and blocks reconstructable provider credentials within or across normalized events.
The Phase 3 completion evidence did not exercise a live Gemini or router function-calling request, so successful live service compatibility remains a separate pre-release gate.

### Phase 3.1: Live protocol reliability and recovery

**Status:** Implemented; live release-candidate evidence moves to Phase 3.9

**Goal:** Make ordinary conversation and the safe tool loop work reliably against live providers, and ensure one invalid model tool emission cannot trap a session in repeated terminal failures.

Deliverables:

- Reproduce the observed live Gemini invalid-tool-call failure through the real terminal path with secret-safe diagnostics.
- Validate the exact custom-function request and streamed-response dialect against live Gemini and one configured OpenAI-compatible router.
- Add an opt-in live compatibility harness that can produce redacted structural fixtures without retaining prompts, responses, credentials, private endpoints, or raw tool arguments.
- Advertise tools only when the selected provider and model can support the exact required dialect and the session's interaction mode enables tools.
- Add a durable, bounded rejection and repair path for unknown names, invalid arguments, and unsupported tool-call shapes instead of failing the entire attempt without recovery.
- Make provider context accurately represent failed and cancelled turns so a failed instruction is not silently replayed as an unanswered instruction on every later prompt.
- Add an always-available new-session action that works even while the catalog, credential, or current attempt is failed.
- Present stable failure codes, safe diagnostic correlation, and concrete recovery actions in the transcript.

Exit criteria:

- A live plain-chat prompt completes through Gemini with the Phase 3 tool registry present or intentionally disabled by mode.
- A live HTTP-tool prompt reaches the exact permission overlay, executes only after approval, returns a durable result, and continues the provider turn.
- Fixture tests prove that unknown and malformed tool calls settle durably, receive at most the configured repair allowance, and do not poison the next user turn.
- After any terminal attempt failure, the user can continue safely, retry when valid, or create a fresh session without restarting or editing application data.
- Captured diagnostics and every durable file remain free of provider credentials and raw secret-bearing payloads.

**Local implementation evidence:** Implemented and fixture-verified on 2026-08-22.

The Gemini Interactions adapter now buffers streamed argument deltas until a complete bounded function call can be emitted, and a one-byte-fragmented recorded fixture covers the observed wire shape.
Unknown names and invalid arguments enter a durable force-denied no-authority lifecycle, receive a bounded repair result, and cannot reach an external capability.
Provider requests exclude prior failed and cancelled prompts unless the same input is explicitly retried, tool definitions require positive model capability evidence, and the terminal exposes a durable global `Ctrl+N` recovery action with stable failure codes and safe attempt references.
Composed SQLite-backed tests cover repair continuation and replay, and an actual credential-free PTY run created a new session from the credential overlay and restored the terminal.
Ignored opt-in Gemini and router compatibility probes retain only structural assertions; Gemini plain chat and HTTP-tool probes passed live on 2026-08-22, while reviewed configured-router live evidence remains open for the final Phase 3.9 release candidate.

### Phase 3.2: Complete session lifecycle

**Status:** Implemented

**Goal:** Turn the existing replay store into a usable multi-session product rather than an application that silently opens one active session.

Deliverables:

- An application-owned session manager that can create, list, select, resume, rename, archive, unarchive, and explicitly delete sessions without putting storage access in the TUI.
- A searchable recent-session browser with deterministic titles, timestamps, provider/model metadata, workspace scope, status, and transcript previews.
- `Ctrl+N` for a new session, a discoverable session-browser action, and equivalent command-palette or slash-command paths.
- Offline startup into session history and settings without requiring a credential or successful model-catalog refresh.
- Per-session model, interaction mode, draft, scroll, and pending-permission state with safe switching rules for active attempts.
- Versioned migration and replay coverage for session metadata, lifecycle events, deletion semantics, and artifact ownership.
- Export of one session to a documented, provider-neutral format before destructive deletion.

Exit criteria:

- A user can create two sessions, switch between them, restart AutoHarness, and resume either with a replay-equivalent transcript and selected model.
- Session search, rename, archive, unarchive, export, and confirmed deletion work without network access.
- Switching cannot orphan an active provider attempt or tool permission, and deleting a session cannot leave unowned durable artifacts.
- Existing Phase 1 through Phase 3 databases migrate without losing their current session.

### Phase 3.3: User profiles, settings, and secure credentials

**Status:** Implemented

**Goal:** Let users configure AutoHarness inside the terminal and reconnect after restart without storing plaintext API keys in ordinary configuration or session history.

Deliverables:

- A versioned typed settings resolver with documented default, user, workspace, environment, and command-line precedence and a visible explanation of each effective value's source.
- An allowlist of workspace-overridable settings that prevents model-writable project files from weakening credential, permission, sandbox, retention, or telemetry policy.
- Named provider profiles containing non-secret connection settings, default model and mode, and an opaque credential reference.
- An operating-system credential-vault port backed by Windows Credential Manager, macOS Keychain, and Linux Secret Service where available.
- Opt-in save, replace, test, disconnect, and delete flows for provider credentials, with session-only and environment-variable fallbacks when secure persistence is unavailable or unwanted.
- A searchable settings screen for provider profile, default model, interaction mode, approval policy, retention, theme, accessibility, logging, and terminal behavior.
- Atomic non-secret settings updates, schema migration, validation, backup, and recovery from malformed user configuration.
- A decision that supersedes or extends [ADR-0005](adr/0005-use-ephemeral-in-app-credentials.md) before persistent credential references are implemented.

Exit criteria:

- An opted-in provider profile reconnects after restart without asking for the key again, while the raw key is absent from settings files, SQLite, events, logs, transcripts, telemetry, crash output, and model-visible context.
- A user can inspect which profile and setting source is active, replace or remove its credential, and choose session-only mode.
- AutoHarness remains usable for offline session management when the credential vault is locked or unavailable.
- Cross-platform tests use fake vaults, and platform smoke tests verify save, retrieve, replace, and delete behavior without printing secret values.

**Local implementation evidence:** Implemented and fixture-verified on 2026-08-22.

ADR-0009 is accepted and [ADR-0012](adr/0012-use-typed-settings-resolver.md) records the layered resolver contract.
`autoharness-settings` resolves defaults, user file, workspace file, environment, and overrides in fixed order with per-key provenance; malformed layers degrade to safe diagnostics, future schema versions fail closed, and workspace documents cannot override provider, profile, or policy keys.
The `autoharness.profiles.json` document stores validated profiles atomically with a `.bad` backup on corruption, and credential linkage writes only opaque references to the operating-system vault through the `keyring` crate (Windows Credential Manager, macOS Keychain, Linux Secret Service) behind an application-owned port with a fake implementation for tests.
Startup resolves one effective source in precedence order: environment, then the active profile's vault entry, then session-only entry; a missing or locked vault degrades to offline-usable session-only operation.
The terminal publishes safe provenance labels and a non-modal `Ctrl+,` settings overlay showing which source is active.
Sentinel tests scan every durable file plus debug output across save, rotate, disconnect, and delete flows and find no credential bytes.
Remaining for full exit evidence: in-terminal flows to create, replace, test, and disconnect credentials from the overlay itself, and platform vault smoke coverage beyond the fake.

### Phase 3.4: TUI usability and discoverability

**Status:** Implemented

**Goal:** Make the terminal interface understandable without memorizing shortcuts and efficient enough for sustained daily work.

Delivered:

- A command palette (`Ctrl+/`) and slash-command layer backed by the same typed application intents as keyboard and visible UI actions.
- A contextual help overlay (`F1`) whose section order follows the current focus, plus footer affordances for the new surfaces at every supported width.
- An enriched header status surface showing provider profile, credential source, selected model, attempt settlement, aggregate token usage, and catalog state with graceful narrow-width degradation.
- Composer history recall (`Ctrl+Up` / `Ctrl+Down`) alongside preserved per-session drafts.
- Transcript search (`Ctrl+F`) with match counting and jump-to-match wrapped-row scrolling.
- Transcript copy through OSC 52 from the runner and Markdown export beside the database satisfied from durable events.
- Structured collapsible tool rows rendered from the authoritative aggregate.
- Confirm-gated archiving and one-shot `Ctrl+Z` undo in the session browser.

Deferred to later phases:

- User-configurable theme, no-color or high-contrast presentation, and reduced motion (waits on settings keys; recorded as a plan non-goal for this phase).
- Mouse support remains a non-goal.

Exit criteria evidence:

- Fixed-size goldens updated for the new header and footer and visually reviewed at 40x12, 60x18, 80x24, 120x40, and 120x50 through a checked-in ignored review harness.
- Every important action is reachable without a mouse through key, palette, and slash paths over one authoritative application intent table.
- Full baseline gates pass: formatting, strict Clippy, full workspace tests, warning-free rustdoc, and doctests.

### Phase 3.5: Terminal release hardening

**Status:** Implemented; pull-request and cross-platform evidence pending

**Goal:** Prove the current runtime and terminal mechanics before product-level profile management and interface expansion.

Deliverables:

- End-to-end PTY scenarios for first run, returning profile, offline resume, multi-session switching, invalid tool repair, permission handling, settings persistence, and destructive confirmations.
- Opt-in live-provider smoke scenarios for plain chat and each supported tool-call dialect, with no credentials or content in checked-in evidence.
- Migration, backup, corruption, locked-vault, network-loss, terminal-resize, forced-shutdown, and restart-recovery tests.
- The deferred monotonic startup, dispatch, and rendered-delta markers plus an approved reference-machine benchmark report.
- A release checklist covering secret scanning, accessibility, terminal restoration, help and documentation accuracy, and database rollback preparation.

Local implementation evidence:

- Real-PTY integration scenarios cover first run, returning-profile offline replay, resize and restart, multi-session lifecycle and destructive confirmations, invalid-call repair, deny and allow permission outcomes, and forced-shutdown recovery.
- The platform test matrix runs the ignored PTY scenario group serially on Windows, macOS, and Linux while the ordinary workspace suite remains deterministic under non-terminal test hosts.
- The robustness suite covers schema-v1 migration, future-schema rejection, migration and event corruption, catalog-cache replacement, malformed-profile backup, locked-vault degradation, interrupted-attempt recovery, network interruption, and terminal restoration.
- Opt-in live probes now cover Gemini and the configured router for both plain chat and the supported function-calling dialect.
- The `benchmark-instrumentation` feature and `terminal_latency` runner correlate first draw, input acceptance, provider dispatch, decoded chunks, and rendered revisions without content-bearing telemetry.
- The [terminal release checklist](release/TERMINAL_RELEASE_CHECKLIST.md) gates security, accessibility, restoration, documentation, benchmark provenance, and database rollback preparation.

Remaining exit evidence requires green baseline and PTY matrix runs for the Phase 3.5 pull-request commit.
The final configured live-provider matrix, approved reference-machine report, complete usability review, and release approval move to Phase 3.9 because Phases 3.6 through 3.8 materially change the shipped terminal experience.

Exit criteria:

- All baseline Rust gates and the current Phase 3.x PTY scenarios pass on Windows, macOS, and Linux.
- Migration, corruption, locked-vault, network-loss, resize, forced-shutdown, and terminal-restoration coverage passes on the pull-request commit.
- Benchmark instrumentation and the isolated terminal runner produce valid content-free reports, while final thresholds and reference evidence remain a Phase 3.9 gate.
- The implementation is promoted to `dev` before Phase 3.6 begins.

### Phase 3.6: Local profile and provider connection center

**Status:** Implemented locally; cross-platform pull-request evidence pending

**Goal:** Give users one safe in-terminal place to understand their local profile, manage every supported provider connection, and save distinct API keys without shell setup.

Deliverables:

- A full-screen Profiles and Providers surface reachable from the global `Ctrl+G` shortcut, command palette, and settings provenance surface.
- A local-only user summary showing the active workspace, default provider profile, default model, and current safe-agent mode; display-label and appearance editing remain in Phase 3.8.
- The local user profile is not a hosted account or authentication identity, and it never owns provider secrets.
- A searchable provider-profile list showing provider kind, active and default state, credential source, connection health, default model, and last safe test result.
- Guided create, edit, duplicate, activate, and delete flows for multiple Gemini and OpenAI-compatible router profiles.
- Secure save, replace, test, disconnect, delete, and session-only credential actions using one operating-system vault entry per named provider profile.
- Read-only explanation when an environment credential overrides a saved vault entry, including the exact non-secret source layer and the action needed to use the saved entry.
- Typed application-owned profile and credential-management commands, read models, validation failures, and recovery states so the TUI never calls the vault, settings store, provider, or filesystem directly.
- Explicit recovery for partial operations across the profile document and operating-system vault, including orphaned references, failed deletion, locked vaults, and interrupted replacement.
- A focused ADR accepted before implementation defines ordering, rollback, and user-visible repair for cross-system profile document and vault mutations.
- Migration and sentinel coverage proving that profile edits and every credential lifecycle operation keep raw keys out of application-owned durable state and rendered diagnostics.

Exit criteria:

- From a fresh launch, a user can create one Gemini profile and one router profile, save a different credential for each, test both, choose defaults, switch between them, and reconnect after restart without a shell command.
- Replacing, disconnecting, and deleting a credential affects only the selected profile and never silently changes another profile or falls back to plaintext storage.
- Environment, vault, and session-only precedence is visible and deterministic, including when the vault is locked or unavailable.
- Every management action has keyboard, palette, and visible-control paths that converge on the same typed application intent.
- Fake-vault integration tests cover complete and interrupted workflows, and platform vault smoke tests exercise save, retrieve, replace, and delete without printing secret values.

**Local implementation evidence:** Implemented and verified on Windows on 2026-08-23.

[ADR-0013](adr/0013-use-durable-credential-mutation-recovery.md) defines deterministic recovery records and operation ordering across the atomic profile document and operating-system vault.
Settings schema 2 adds non-secret recovery state and optional profile default models while schema-v1 documents migrate on their next mutation.
The application-owned `ProfileManager` serializes create, edit, duplicate, activate, save, replace, disconnect, delete, and restart reconciliation without exposing the vault or profile file to the TUI.
The `Ctrl+G` full-screen surface shows local defaults, searchable Gemini and router profiles, active and credential-source state, content-free connection health, responsive detail panes, masked credential entry, destructive confirmations, and keyboard help.
Typed TUI intents drive the same coordinator operations as command-palette and visible actions, and runtime profile switches rebuild the correct provider adapter without crossing provider credentials.
Focused fault-injection and sentinel tests cover interrupted saves, failed cleanup, idempotent recovery, scoped replacement and deletion, duplication without credential linkage, default-model persistence, and raw-secret exclusion.
A composed coordinator test creates distinct Gemini and router profiles and keys, switches and tests both, assigns a default model, deletes only the router, restarts, and proves the Gemini profile and credential remain.
An actual PTY journey creates, switches, duplicates, cancels and confirms deletion, exits cleanly, and resolves the surviving profiles without shell setup.
The opt-in operating-system vault smoke passed save, load, replace, and delete against Windows Credential Manager; macOS Keychain and Linux Secret Service evidence remains part of the cross-platform pull-request and Phase 3.9 release matrix.

### Phase 3.7: Unified TUI shell and navigation

**Status:** Implemented locally; cross-platform pull-request evidence pending

**Goal:** Replace the growing collection of overlays with a coherent application shell whose hierarchy, focus, and next action are obvious at every supported terminal size.

Deliverables:

- A route-based shell for Chat, Sessions, Profiles and Providers, Settings, and Help, with a navigation rail at wide widths and a compact route switcher at narrow widths.
- A visible local profile and active-provider summary in the shell, with connection, model, attempt, and offline state presented once rather than repeated inconsistently.
- A redesigned chat workspace with clearer transcript grouping, tool and failure hierarchy, composer boundaries, streaming state, and contextual actions.
- One focus model and one overlay stack with deterministic opening, dismissal, focus restoration, and conflict rules for permission prompts, destructive confirmations, credential entry, search, and the command palette.
- Reusable terminal design tokens and components for spacing, borders, typography emphasis, selection, focus, disabled controls, success, warning, and failure.
- Responsive layout classes with explicit information priority for 40x12, 60x18, 80x24, 120x40, 120x50, and wider terminals.
- Deliberate empty, loading, offline, locked-vault, no-model, no-session, and recoverable-error states with one visible primary action.
- Contextual action bars and command-palette routing that keep every important operation discoverable without requiring shortcut memorization.
- A thin TUI boundary that continues to consume application read models and emit typed intents without network, storage, provider, or model logic in the render loop.

Exit criteria:

- A keyboard-only user can move among the five primary routes, return to the prior focus after every overlay, and complete ordinary chat, session, provider, and settings tasks without consulting external documentation.
- No input can open incompatible overlays, dispatch an action against a hidden or stale selection, or lose a composer draft.
- Fixed-size goldens and critical visual review cover every responsive layout class and all primary loading, empty, error, permission, and destructive-confirmation states.
- Instrumented first draw, input dispatch, and decoded-chunk-to-render intervals remain within the reviewed Phase 3.x budgets.

**Local implementation evidence:** Implemented and verified on Windows on 2026-08-23.

The TUI now has one typed primary `Route` for Chat, Sessions, Profiles, Settings, and Help, reachable through portable `Alt+1` through `Alt+5` chords plus the existing legacy shortcuts and shared command table.
Terminals at least 100 columns wide render a persistent navigation rail; narrower terminals render prioritized route tabs and one compact status line.
The shell presents local profile, workspace, provider, credential source, model, attempt, usage, and catalog health once, while every route owns its content and contextual action bar.
Chat renders a Conversation workspace with explicit `YOU`, `AUTOHARNESS`, and `TOOL` hierarchy, bounded failure recovery, composer separation, and deliberate offline, loading, connection-error, empty-catalog, no-model, and new-conversation states.
Sessions, Profiles, Settings, and Help are primary pages rather than modal overlays, and Settings provides safe effective runtime, provenance, recovery, and profile-management routing.
One `OverlayKind` slot now owns model selection, session-only and profile credential entry, command and transcript search, permission decisions, and exact destructive confirmations.
Every overlay captures and restores its prior route and focus; permission preempts lower-authority overlays, global route changes clear hidden confirmations, and secret editors are dropped before focus moves.
Navigation tests cover direct and legacy routes, overlay restoration from non-chat routes, permission preemption, modal replacement, confirmation clearing, draft preservation, explicit recovery states, and every route at 40x12, 60x18, 80x24, 120x40, and 120x50.
Reviewed fixed-size goldens and the ignored visual matrix cover all five routes and the confirmation surface at every responsive layout class.
The actual PTY journey switches all routes with Alt chords, restores Settings after model-picker dismissal, preserves a draft, creates and lists another durable session, cancels a deletion confirmation, resizes to 40x12, exits cleanly, and restores the terminal.
An instrumented release build and three-sample real-PTY loopback smoke produced valid correlated first-draw, input-to-dispatch, and decoded-chunk-to-render reports with network time excluded; authoritative thresholds and reference-machine evidence remain Phase 3.9 gates.
The Phase 3.7 validation path also fixed fresh-session list publication so a newly committed session becomes immediately visible in Sessions.

### Phase 3.8: Personalization and accessibility

**Status:** Implemented locally

**Goal:** Let users adapt the terminal to their environment and accessibility needs without editing configuration files.

Deliverables:

- A categorized settings workspace for profile defaults, providers, model and mode, approvals, retention, appearance, accessibility, logging, and terminal behavior.
- Provenance beside every effective setting, plus reset-to-inherited and reset-to-default actions that preserve layered resolver semantics.
- Persisted theme presets, no-color and high-contrast modes, reduced-motion behavior, Unicode and ASCII glyph modes, compact and comfortable density, and configurable terminal time presentation.
- An editor for the local user profile display label and defaults, backed by typed non-secret settings rather than a second profile store.
- Strong visible focus, deterministic tab order, text alternatives for color-only state, stable status wording, and a single-column presentation suitable for narrow terminals and assistive terminal workflows.
- Configurable composer submission behavior and a safe shortcut reference generated from the authoritative command table.
- Schema migration, malformed-setting recovery, workspace-override restrictions, and restart coverage for every new setting.

Exit criteria:

- Every shipped presentation and terminal-behavior setting can be inspected, changed, explained, and reset inside the TUI.
- No-color, high-contrast, ASCII, reduced-motion, compact, and single-column combinations preserve all status and action information without clipping security-critical prompts.
- User preferences survive restart, respect fixed precedence, and cannot weaken credential, permission, retention, telemetry, or sandbox policy from a workspace file.
- Visual review covers representative theme and accessibility combinations at every supported responsive layout class.

**Local implementation evidence:** Implemented and verified on Windows on 2026-08-24.

Settings is now a categorized route-local workspace with deterministic selection and inline local-label editing.
Every shipped terminal preference shows its effective value, source, explanation, inherited reset, and user-default reset.
Schema 3 stores non-secret local display, theme, color, glyph, motion, density, layout, timestamp, and composer-submission preferences in the atomic profile document.
The resolver migrates schema 1 and 2 documents, fixes layer precedence independent of builder insertion order, and permits only safe workspace presentation overrides.
Renderer tokens apply theme, no-color, high-contrast, ASCII, reduced-motion, compact-density, and single-column behavior across routes and security overlays.
The Settings shortcut reference derives from the shared command table.
Focused resolver, profile-store, render matrix, complete workspace, and real Windows PTY route journey evidence pass.

### Phase 3.9: Terminal product validation

**Status:** Planned

**Goal:** Validate the complete redesigned terminal as the release-quality product boundary that Phase 4 can extend without reopening basic navigation, profile, credential, or accessibility work.

Deliverables:

- Real-PTY user journeys for first-run onboarding, local-profile setup, adding multiple providers, saving and rotating credentials, switching profiles, ordinary chat, offline resume, session lifecycle, settings changes, permissions, and recovery.
- Windows, macOS, and Linux vault smoke evidence for save, retrieve, replace, test, disconnect, and delete behavior with content-free output.
- Gemini and configured-router release-candidate live probes for plain chat and the supported tool-call dialect.
- Reviewed visual evidence for every responsive layout class, primary route, theme mode, accessibility mode, destructive confirmation, and critical failure state.
- Approved reference-machine reports for startup, input dispatch, rendered stream overhead, memory, storage, and replay, with network latency reported separately.
- An expanded terminal release checklist covering secrets, accessibility, navigation, terminal restoration, migration, rollback, help accuracy, and zero-shell ordinary use.
- A migration and rollback rehearsal from the last Phase 3.5 database and settings formats to the final Phase 3.9 release candidate.

Exit criteria:

- A fresh user can configure a supported provider, securely save a key, select a model, complete a chat, find the session after restart, and change the active provider without shell or database manipulation.
- All baseline gates, cross-platform PTY journeys, platform vault smokes, live-provider probes, documentation checks, and approved reference-machine budgets pass on one release-candidate commit.
- No P0 or P1 defect remains in onboarding, chat, sessions, profiles, credentials, settings, permissions, recovery, accessibility, or terminal rendering.
- The release checklist is approved, rollback evidence is complete, and Phase 4 can consume stable routes, read models, intents, profile settings, and credential workflows.

### Phase 4: Persistent context and memory

**Status:** Designed; implementation gated by Phase 3.1 through Phase 3.9

**Goal:** Turn durable history into useful, bounded, auditable model context.

Deliverables:

- Context-source registry and deterministic context builder.
- Context epochs, snapshots, admissions, and compaction.
- User-, workspace-, session-, and agent-scoped memory records.
- Memory proposal, validation, deduplication, supersession, retraction, and deletion flows.
- SQLite FTS retrieval with a replaceable ranking interface.
- Memory inspection UI showing source, confidence, scope, age, and admission history.

Exit criteria:

- Every injected memory can answer where it came from, why it was selected, and which provider turns saw it.
- Model-generated memory is never silently promoted to trusted memory.
- Context construction is deterministic for a fixed event log, catalog snapshot, configuration, and token budget.
- Compaction and restart do not change the effective durable facts.

### Phase 5: Evaluation and self-improvement

**Goal:** Improve harness behavior through controlled, reproducible experiments.

Deliverables:

- Versioned evaluation-case and dataset formats.
- Trace-to-evaluation failure-mining workflow.
- Candidate registry for prompts, policies, routing, tools, memory strategy, and source patches.
- Reproducible experiment runner with baseline/candidate comparison.
- Metrics for quality, task success, safety, latency, cost, and reliability.
- Promotion policy, approval gate, canary, rollback, and audit log.
- Isolation between candidate generation and candidate evaluation.

Exit criteria:

- A candidate cannot alter its evaluation data, judge configuration, promotion threshold, or audit record.
- Promotion requires configured metric improvement with no guardrail regression.
- Every promoted behavior is linked to its code/config version, dataset, evaluator, results, approval, and rollback target.

### Phase 6: Extension and distributed runtime

**Goal:** Scale execution without changing the core semantics proven locally.

Deliverables:

- Versioned daemon protocol and thin remote TUI client.
- Remote worker leases, heartbeats, cancellation, idempotency, and recovery.
- PostgreSQL durable store and object storage for large artifacts.
- Wasmtime component host with WIT-defined provider, tool, memory, and evaluator capabilities.
- Signed extension packages, declared permissions, resource limits, and compatibility checks.
- Multi-workspace scheduling, quotas, and observability.

Exit criteria:

- Local and remote execution pass the same conformance and replay suites.
- Losing a worker cannot lose already-admitted user input or fabricate successful work.
- Plugins have no filesystem, network, process, secret, or memory access unless explicitly granted.

## Next implementation order

Phases 1 through 3 established the engine, provider, storage, replay, and safe tool-execution substrates.
Phases 3.2 through 3.7 now provide durable sessions, multiple secure provider profiles, and one stable responsive terminal shell with typed route, focus, overlay, and recovery boundaries.
Proceed in this order:

1. Promote the Phase 3.7 implementation through green baseline and cross-platform serial PTY pull-request gates.
2. Implement Phase 3.8 settings, personalization, and accessibility on top of the stable route-based shell.
3. Execute Phase 3.9 against one release-candidate commit, including the deferred live-provider, cross-platform vault, visual, benchmark, migration, and rollback evidence.
4. Begin Phase 4 with deterministic context epochs and untrusted memory proposal contracts.

Each step must leave a runnable or testable vertical slice; avoid creating unused framework layers far ahead of their first consumer.

## Quality gates

### Correctness

- Domain transitions are testable without a terminal, network, or real database.
- Durable writes and externally visible events have explicit transaction boundaries.
- Retries use stable request identifiers where the provider supports them.
- Parsing handles arbitrary byte and event fragmentation.

### Performance

Establish a checked-in benchmark environment before setting release thresholds. Measure at minimum:

- Cold process start to first terminal draw.
- Idle resident memory.
- Input-to-request dispatch overhead.
- Provider-chunk receipt to rendered-delta latency.
- Event append and transcript projection throughput.
- Recovery time for representative session sizes.

LLM network latency must be reported separately from harness overhead.

### Security and privacy

- Secrets come from environment variables or OS credential storage and are represented by opaque references.
- Authentication material is structurally excluded from events, errors, telemetry, and memory.
- Model text is untrusted data, including when it resembles instructions or memory metadata.
- Tool and plugin capabilities default to denied.
- Users can inspect, export, retract, and delete stored memory.

### Compatibility

- Persistent rows and serialized events carry schema versions.
- Public protocols and plugin interfaces use explicit compatibility rules.
- Provider adapters declare supported features and degrade only through explicit policy.

### Product usability

- Basic chat, session management, profile management, settings, credentials, and failure recovery are available inside the terminal without requiring database or shell manipulation.
- A local user profile summary and the active provider connection are visible without exposing secret metadata or implying a hosted identity.
- Network or credential failure does not block offline access to durable sessions and non-secret settings.
- Keyboard, command, and visible-control paths converge on the same typed application intents.
- Focus order, status meaning, and primary actions remain understandable in every responsive layout and supported accessibility mode.
- Destructive actions expose their scope and require explicit confirmation.

## Major risks and responses

| Risk | Response |
| --- | --- |
| Provider APIs change rapidly | Contract tests, recorded fixtures, capability discovery, and isolated adapters |
| Fixture-backed protocol tests miss live behavior | Opt-in redacted live compatibility runs and release-candidate smoke gates for every supported provider dialect |
| A failed tool call traps later conversation | Durable rejection and bounded repair, explicit failed-turn context, actionable recovery, and an always-available fresh session |
| TUI feature debt makes durable capabilities inaccessible | Complete the profile center, route-based shell, personalization, accessibility, and integrated Phase 3.9 validation before Phase 4 |
| Credential convenience weakens secret handling | Store raw secrets only in one operating-system vault entry per named provider profile, keep opaque references in profiles, and retain session-only and environment fallbacks |
| The abstraction collapses to the least common denominator | Keep normalized lifecycle events while allowing namespaced provider options at the edge |
| Memory becomes prompt-injection persistence | Provenance, trust classes, proposal validation, inspection, and retraction |
| Self-improvement rewards the evaluator instead of users | Hidden holdouts, multiple metrics, independent judges, guardrails, and canaries |
| Event storage grows without bound | Projections, retention policies, content-addressed artifacts, and compaction without losing audit identity |
| Rust plugin authoring is too restrictive | WIT components plus a supervised JSON-RPC subprocess bridge |
| Early distributed design slows the local product | Modular monolith first; remote protocols follow demonstrated semantics |
| Documentation memory goes stale | End-of-task reconciliation rules in `AGENTS.md` and a single authority for each fact |

## Open decisions

- Benchmark hardware and release thresholds.
- Credential-vault behavior on Linux systems without a Secret Service implementation.
- Session hard-deletion, artifact cleanup, retention, and export semantics.
- The first non-Rust plugin authoring path to support.

Resolve an open decision with an ADR when it becomes implementation-blocking.
