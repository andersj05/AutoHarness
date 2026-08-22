# AutoHarness project plan

**Status:** Active

**Last updated:** 2026-08-22

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

**Status:** Active

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
Ignored opt-in Gemini and router compatibility probes compile and retain only structural assertions, but the live plain-chat and HTTP-tool exit criteria remain open because no provider credential or router endpoint was available in the verification process.

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

**Status:** Planned

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

### Phase 3.4: TUI usability and discoverability

**Status:** Planned

**Goal:** Make the terminal interface understandable without memorizing shortcuts and efficient enough for sustained daily work.

Deliverables:

- A command palette and slash-command layer backed by the same typed application intents as keyboard and visible UI actions.
- A contextual help screen and footer that show available actions for the current focus and terminal size.
- Clear navigation among sessions, transcript, composer, models, profiles, settings, and pending permissions.
- A status surface for workspace, session, provider profile, model, interaction mode, context or usage, network state, and active work.
- Structured transcript rows for tools, permissions, results, warnings, failures, retries, and recovery actions, with collapsible detail where terminal space is limited.
- Transcript search, copy, and export plus composer history and preserved per-session drafts.
- User-configurable theme, no-color or high-contrast presentation, reduced motion, and keybinding help.
- Confirmations and undo where practical for archive, credential removal, settings reset, and deletion operations.

Exit criteria:

- A first-time user can connect a provider, select a model, create and resume sessions, change a setting, send a prompt, approve a tool, recover from failure, and find help using only in-app affordances.
- The complete flow is usable and visually reviewed at 80-by-24, 120-by-40, and a wide terminal without clipped controls or hidden critical state.
- Every important action is reachable without a mouse and has one authoritative application intent regardless of whether it starts from a key, command, or visible control.

### Phase 3.5: Terminal release hardening

**Status:** Planned

**Goal:** Prove the complete Phase 3.x terminal product as a stable base before persistent memory adds more state and UI.

Deliverables:

- End-to-end PTY scenarios for first run, returning profile, offline resume, multi-session switching, invalid tool repair, permission handling, settings persistence, and destructive confirmations.
- Opt-in live-provider smoke scenarios for plain chat and each supported tool-call dialect, with no credentials or content in checked-in evidence.
- Migration, backup, corruption, locked-vault, network-loss, terminal-resize, forced-shutdown, and restart-recovery tests.
- The deferred monotonic startup, dispatch, and rendered-delta markers plus an approved reference-machine benchmark report.
- A release checklist covering secret scanning, accessibility, terminal restoration, help and documentation accuracy, and database rollback preparation.

Exit criteria:

- All baseline Rust gates and Phase 3.x PTY scenarios pass on Windows, macOS, and Linux.
- The supported live-provider matrix passes plain chat and safe tool continuation on a release candidate.
- No P0 or P1 defect remains in basic chat, session lifecycle, settings, credential handling, permission handling, recovery, or terminal rendering.
- Phase 3.x benchmark and usability evidence is reviewed, and Phase 4 can consume stable session, settings, profile, and navigation boundaries rather than building around temporary UI state.

### Phase 4: Persistent context and memory

**Status:** Designed; implementation gated by Phase 3.1 through Phase 3.5

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
The observed live-provider failure and missing session and settings lifecycle show that the terminal is not yet a sufficient product surface for Phase 4.
Proceed in this order:

1. Complete the remaining Phase 3.1 live Gemini and configured-router plain-chat and approved HTTP-tool exit evidence using the checked-in structural probes.
2. Complete Phase 3.2 multi-session lifecycle and offline session navigation.
3. Complete Phase 3.3 typed settings, provider profiles, and opt-in operating-system credential storage.
4. Complete Phase 3.4 TUI usability and discoverability.
5. Complete Phase 3.5 cross-platform release hardening, benchmark markers, and reference-machine evidence.
6. Begin Phase 4 with deterministic context epochs and untrusted memory proposal contracts.

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

- Basic chat, session management, settings, credentials, and failure recovery are available inside the terminal without requiring database or shell manipulation.
- Network or credential failure does not block offline access to durable sessions and non-secret settings.
- Keyboard, command, and visible-control paths converge on the same typed application intents.
- Destructive actions expose their scope and require explicit confirmation.

## Major risks and responses

| Risk | Response |
| --- | --- |
| Provider APIs change rapidly | Contract tests, recorded fixtures, capability discovery, and isolated adapters |
| Fixture-backed protocol tests miss live behavior | Opt-in redacted live compatibility runs and release-candidate smoke gates for every supported provider dialect |
| A failed tool call traps later conversation | Durable rejection and bounded repair, explicit failed-turn context, actionable recovery, and an always-available fresh session |
| TUI feature debt makes durable capabilities inaccessible | Treat session, settings, profile, and navigation completeness as a Phase 4 entry gate |
| Credential convenience weakens secret handling | Store raw secrets only in an operating-system vault, keep opaque references in profiles, and retain session-only and environment fallbacks |
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
