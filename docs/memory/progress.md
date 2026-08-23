# Progress memory

**Last reviewed:** 2026-08-23

**Evidence rule:** Mark capabilities complete only when verified by repository contents, automated checks, or observable behavior.

## Milestones

| Phase | Status | Verified outcome |
| --- | --- | --- |
| 0. Repository foundation | Complete | Plan, architecture, memory protocol, research, initial ADRs, and validated local links are present |
| 1. Terminal vertical slice | Complete | The fixture-verified terminal path discovers models, streams typed Gemini events, cancels and retries attempts, commits SQLite events and projections, and restores the same visible session after restart |
| 2. Provider/router platform | Complete | Gemini and the configurable OpenAI-compatible router pass fixture conformance and the same composed session path through shared provider policy and durable catalog caching |
| 3. Safe agent execution | Complete | Security-audited versioned tools run through explicit durable permission, bounded provider admission, capability, budget, artifact, continuation, parent-child lifetime, and conservative recovery boundaries |
| 3.1. Live protocol reliability and recovery | Implemented | Gemini argument aggregation, durable invalid-call repair, failed-turn isolation, capability gating, stable diagnostics, and durable `Ctrl+N` recovery pass local fixture, integration, render, replay, and PTY tests; live Gemini plain-chat and function-call probes passed on 2026-08-22, while final configured-router evidence moves to Phase 3.9 |
| 3.2. Complete session lifecycle | Complete | Event-sourced rename, archive, unarchive, guarded switching, atomic version-checked deletion with pre-deletion export, schema-v3 titles, a searchable `Ctrl+L` browser with slash commands and per-session drafts, and the composed two-session restart replay-equivalence path pass domain, engine, store, TUI, app, and render tests |
| 3.3. User profiles, settings, and secure credentials | Implemented | Layered typed settings with provenance, validated profiles in an atomic schema-versioned document, the OS credential-vault port over Windows Credential Manager, macOS Keychain, and Linux Secret Service, startup reconnect from environment or vault with session-only degradation, a `Ctrl+,` settings overlay naming each source, and sentinel tests proving no credential bytes reach durable files pass resolver, vault, profile-store, startup-reconnect, TUI, and sentinel tests |
| 3.4. TUI usability and discoverability | Implemented | A `Ctrl+/` searchable command palette and generalized slash commands over one shared typed command table, an `F1` contextual help overlay, an enriched header status surface with graceful narrow-width degradation, composer history recall, `Ctrl+F` transcript search with jump-to-match scrolling, OSC 52 transcript copy, durable-event Markdown export beside the database, structured collapsible tool rows, and confirm-gated archiving with `Ctrl+Z` undo pass palette, help, search, tool-row, archive-undo, export, render, and full-workspace tests |
| 3.5. Terminal release hardening | Implemented; pull-request evidence pending | Six real-PTY release scenarios, serial cross-platform CI routing, Windows cursor reporting, inactive-session isolation, causation-safe deletion, durable tool rows, locked-vault and corrupt-cache recovery, provider-matched environment credentials, content-free terminal markers, a PTY latency runner, and the terminal release checklist pass local baseline, benchmark, real Windows router smoke, and six-scenario Windows PTY validation; green cross-platform pull-request evidence remains open |
| 3.6. Local profile and provider connection center | Implemented locally | ADR-0013 recovery records, settings schema 2, application-owned profile workflows, distinct provider-scoped vault entries, runtime Gemini and router switching, content-free tests, default models, live safe projections, the responsive `Ctrl+G` center, fault injection, sentinel coverage, a composed restart lifecycle, a real PTY journey, and Windows Credential Manager smoke pass locally; cross-platform CI and macOS and Linux vault evidence remain open |
| 3.7. Unified TUI shell and navigation | Planned | Replace overlay-led navigation with a responsive route-based shell, one focus and overlay model, redesigned chat hierarchy, visible profile and provider status, and explicit empty and recovery states |
| 3.8. Personalization and accessibility | Planned | Add an in-terminal settings workspace, provenance and reset controls, typed local-profile preferences, themes, no-color and high-contrast modes, reduced motion, ASCII mode, density, and single-column presentation |
| 3.9. Terminal product validation | Planned | Validate complete cross-platform onboarding, multiple providers, vault workflows, navigation, accessibility, live-provider compatibility, visual quality, performance, migration, rollback, and zero-shell ordinary use on one release candidate |
| 4. Persistent context and memory | Designed and gated | Architecture is documented; runtime is not implemented and waits for Phase 3.9 validation |
| 5. Evaluation and self-improvement | Planned | Roadmap and guardrails are documented; runtime is not implemented |
| 6. Extension and distributed runtime | Planned | Target boundaries are documented; runtime is not implemented |

## Verified repository capabilities

- Human-facing README routes to authoritative project documentation.
- Cross-tool root `AGENTS.md` defines read order, architecture guardrails, and memory maintenance.
- Stable, active, and progress memory are separated.
- Architecture decisions have a numbered template, lifecycle, and index.
- Research sources are commit-pinned where possible.
- Runtime persistent-memory layers, invariants, data model, admission, retrieval, compaction, security, and tests are specified.
- Root agent guidance includes the project's general engineering and quality standards.
- Root instructions and ADR-0003 define the permanent `main -> dev -> feat/<name>` workflow.
- Rust 1.97.1 and Cargo resolver 3 are pinned for the Rust 2024 workspace, with a workspace dependency lockfile.
- Provider-neutral session, input, attempt, cancellation, response, usage, settlement, and retry commands produce schema-v1 events with stable identity, sequence, time, causation, correlation, and safe payloads.
- Provider-neutral run-budget, provider-turn, tool-call, permission, effect-start, settlement, pause, and resume commands produce stable schema-v1 event shapes.
- The headless engine rejects command-ID reuse and invalid attempt transitions, applies event batches atomically, and reconstructs the same selected model, transcript, usage, and retry lineage from serialized history without using timestamps for order.
- The headless engine freezes model arguments with a trusted derived capability, requires a matching durable policy and optional human answer before `ToolCallStarted`, and reconstructs tool and paused-attempt state from the authoritative event stream.
- The durable engine appends events before publishing projected state and recovers every stored session through the same strict replay path.
- SQLite runs in WAL mode with verified durability settings, transactional event and projection updates, optimistic sequence checks, byte-identical idempotent append, migration-history validation, corruption detection, and projection rebuilding.
- Provider-neutral catalog and chat ports isolate provider-native protocols from the engine and terminal.
- Provider-neutral versioned tool definitions, complete tool calls, and result history isolate Gemini Interactions and OpenAI-compatible function-calling payloads from the engine and terminal.
- Stable catalog requests distinguish cache-preferred startup from explicit refresh, and catalog results identify live, fresh-cache, or stale-fallback provenance.
- Shared provider conformance assertions pin catalog identity, normalized lifecycle, cumulative usage, non-retryable failures, and credential redaction across adapters.
- The configurable OpenAI-compatible router adapter validates one base origin, resolves relative discovery and streamed-chat paths, supports a configurable sensitive authentication header and project identity, disables redirects, follows bounded pagination, and normalizes chat-completions SSE without leaking router payloads into core types.
- The shared managed-provider layer applies catalog and dispatch deadlines, stream idle deadlines, bounded retries only before semantic streaming, concurrency limits, per-project request windows, and preflight rejection of known unsupported chat or streaming capabilities.
- Schema-v1 provider-neutral catalog snapshots are stored in SQLite with content hashes and migration-history integrity, provide fresh-cache startup, and permit stale fallback only for bounded transient refresh failures.
- A composed integration test runs the actual router adapter through model discovery, selection, prompt admission, streaming, durable completion, and terminal projection using the unchanged engine session path.
- A real PTY router smoke run rendered discovery, selection, prompt admission, and streamed completion, then restarted with the fixture offline and restored the selected model and transcript from durable replay plus a fresh catalog cache without retaining credential bytes.
- The Gemini adapter accepts a zeroizing in-app handoff or reads `GEMINI_API_KEY`, authenticates by a sensitive header, discovers compatible models through opaque-token pagination, streams stable Interactions v1 events, and permits only a narrow pre-stream Generate Content fallback.
- The Gemini decoder normalizes lifecycle, text, completion, and cumulative usage events across arbitrary byte and SSE fragmentation while filtering provider thought steps.
- Gemini and OpenAI-compatible decoders normalize arbitrarily fragmented native function calls into one bounded complete internal call before the application can admit it.
- Gemini Interactions function calls wait for every streamed argument fragment, including across one-byte SSE fragmentation, instead of emitting the placeholder start arguments.
- Tool definitions require positive model capability evidence before advertisement, and older catalog snapshots decode missing tool capability as unknown and therefore disabled.
- Unknown names and invalid argument shapes enter the durable tool lifecycle with an `InvalidToolCall` no-authority capability, are force-denied, cannot be authorized by policy or replay evidence, and return a deterministic bounded repair result to the provider.
- A provider that repeats invalid calls is stopped after the immutable eight-turn allowance with the stable `tool_turn_limit` failure code, and all rejected calls replay as denied.
- Provider context includes completed turns and the current attempt while excluding unrelated failed and cancelled prompts, so a later greeting cannot replay an earlier failed instruction.
- The trusted tool registry strictly parses schema-v1 filesystem read, filesystem write, direct process, and HTTP calls, rejects unknown fields, traversal, shell programs, unsupported methods, redirects, and recovery drift, and derives capability authority without a model-selected permission field.
- The local policy denies unmatched calls and asks before workspace-confined reads, writes, direct process execution, or exact-origin HTTP requests.
- Filesystem, process, and HTTP capability ports enforce workspace or origin confinement, cooperative cancellation, time and byte bounds, direct argument-vector execution without an inherited environment, and HTTP without ambient proxies or redirects.
- Immutable run budgets bound turns, elapsed time, reported tokens, output bytes, and concurrent tool effects, and recovery reconstructs durable counters plus elapsed wall time.
- Monetary limits are explicitly deferred until a trusted durable pricing snapshot can make them enforceable and recoverable.
- Provider adapters enforce per-turn tool-call counts and aggregate structured-argument bounds before durable admission, reject structured values from which configured credentials could be reconstructed, and retain a zeroized credential-length suffix to reject reconstruction across ordered normalized events.
- Started effects settle as unknown on unproven runtime errors, and attempts cannot settle while owned tool authority remains live.
- Permission projections include scrollable operation-specific process and HTTP details without exposing those details through debug output.
- Bounded tool results retain oversized full output in atomically published content-addressed artifacts while only bounded inline content enters the next provider turn.
- The Ratatui client includes a masked zeroizing credential overlay, a searchable model picker, Unicode multiline composer, streamed transcript, cancellation and retry states, safe errors, usage, tail following, manual scrolling, compact layouts, and bounded application mailboxes.
- Failed transcript rows expose stable codes, compact safe attempt references, retry actions, and a fresh-session recovery action.
- A global `Ctrl+N` action creates and activates a fresh durable session from credential, catalog, or settled-attempt failure states, and session identity prevents a new revision-1 projection from being mistaken for stale state.
- A searchable `Ctrl+L` session browser lists every durable session with deterministic titles, active and archived badges, case-insensitive search, Ctrl-chord rename, archive, unarchive, confirm-gated delete, and slash-command equivalents, while per-session composer drafts survive switching.
- Rename, archive, and unarchive are schema-v1 events guarded by the aggregate: archived sessions accept only unarchive, duplicate transitions conflict, and every command decision stays strictly replayable.
- Opening another session replays its authoritative history into the coordinator before any projection swap, and switching is refused while an attempt or permission prompt is active.
- Deleting a settled session exports its complete event history to a documented provider-neutral JSON archive beside the database before any row is removed; export failure aborts deletion and sequence-mismatched deletes fail closed.
- SQLite migration 3 stores session titles, projection rebuilds preserve them, and enriched summaries feed both the browser and the export.
- The executable composes the terminal client, bounded async coordinator, runtime Gemini provider replacement, dedicated blocking SQLite writer, startup recovery, explicit cancellation, application data paths, one-writer locking, and content-free structured tracing.
- Recovery settles never-dispatched attempts as retryable failures and marks ambiguously dispatched attempts unknown without inventing a provider outcome.
- Tool recovery preserves unanswered permission requests only for parents already awaiting tools, settles every live child before marking an interrupted parent unknown, marks started effects unknown without replay, and resumes a paused provider turn only after every call is settled.
- A composed integration test proves a workspace write cannot happen before the exact human allow event commits, then verifies execution, provider continuation, durable completion, shutdown, SQLite reopen, and replay-equivalent tool state.
- A composed integration test selects a model, persists a prompt, streams partial output, cancels, retries to completion, shuts down, reopens SQLite, and verifies a replay-equivalent visible session.
- Compatibility tests pin every command and event serialization shape and verify prompt preservation, API-key masking and non-persistence, debug redaction, identifier validation, replay integrity, failure atomicity, and terminal restoration.
- Local validation passes formatting, strict Clippy, warning-denied rustdoc, doctests, and the full workspace test suite.
- A PTY smoke run without a Gemini credential rendered the complete 80-by-24 terminal interface, confined application files to an isolated data directory, exited successfully through `Ctrl+C`, and restored the terminal.
- A credential-overlay PTY smoke run masked a bracketed-paste sentinel, cleared it when dismissed, reopened an empty editor through `Ctrl+K`, excluded the sentinel from application files, and restored the terminal.
- A Phase 3.1 PTY smoke run created a durable session through `Ctrl+N` while the credential overlay was open, rendered the confirmation, exited cleanly, restored the terminal, and removed its isolated data.
- Ignored opt-in live compatibility tests cover Gemini plain chat with the complete registry, Gemini streamed HTTP function calling, and configured-router streamed HTTP function calling using structural assertions only.
- The isolated benchmark environment measures durable append with synchronous projections, transcript-read throughput, and warm SQLite reopen with strict replay for representative session sizes while explicitly excluding network latency.
- A PowerShell idle resident-memory sampler is available, and the benchmark documentation defines provenance requirements and exact monotonic markers for deferred latency metrics.
- Continuous integration defines formatting, lint, documentation, doctest, native Linux, Windows, and macOS test gates, plus separate formatting, lint, and test gates for the isolated benchmark workspace.
- An automated stdlib-only check validates every relative Markdown link, heading anchor, and ADR index entry locally and in a dedicated CI job.
- The opt-in live Gemini probes passed on 2026-08-22 against production Google AI Studio on current-generation models after the decoder learned the live `arguments_delta` dialect and stopped treating the empty start placeholder as complete arguments.
- The repository carries the accepted MIT license decision in ADR-0010 with a root `LICENSE`, a contributor guide, and workspace-level Cargo license metadata.
- ADR-0009 is accepted: opted-in provider credentials live in the operating-system credential vault behind an application-owned port, profiles retain only opaque references, and missing or locked vaults degrade to environment or session-only operation without any plaintext fallback store.
- ADR-0012 is accepted: settings resolve through defaults, user file, workspace file, environment, and overrides in fixed order with per-key provenance, malformed-layer recovery, fail-closed future schema versions, and a workspace allowlist that cannot weaken credential or permission policy.
- The `autoharness.profiles.json` document stores validated named profiles atomically with schema version 1 and a `.bad` backup on corruption; credential linkage writes only opaque references such as `autoharness/profile/<name>`.
- Startup resolves one effective credential source in precedence order (environment, then active-profile vault entry, then session-only) before provider construction, and publishes safe provenance labels to the terminal's non-modal `Ctrl+,` settings overlay.
- Sentinel tests scan every durable file plus debug output across save, rotate, disconnect, and delete flows with a unique marker secret and find no leakage.
- A searchable `Ctrl+/` command palette and bare composer slash commands execute one shared typed command table so keyboard, palette, and slash paths converge on identical application intents, with unknown commands rejected without clearing the draft.
- The `F1` help overlay orders its sections by the surface help was opened from and the footer advertises `F1` at wide widths while preserving state-dependent retry affordances.
- The header renders provider profile, credential source, selected model, attempt settlement, aggregate token usage, and catalog state, degrading through width bands down to a two-item 40-column line verified by updated fixed-size goldens.
- `Ctrl+Up` and `Ctrl+Down` recall recently submitted prompts in run order, stashing the live draft on walk start and restoring it on return, while per-session drafts stay independent.
- `Ctrl+F` opens a transcript search bar with live match counting, Enter and Shift+Tab stepping through matches, wrapped-row jump-to-match scrolling consistent with the renderer, and Esc closing without losing scroll position rules.
- `Ctrl+Y` copies the visible transcript through OSC 52 emitted from the runner without new dependencies, and `/export` dispatches a durable export intent the engine actor satisfies by writing human-readable Markdown beside the database from authoritative events only, leaving history untouched.
- Durable tool calls render as structured collapsed rows (name, status, bounded summary) that expand to include the canonical resource under a global `Ctrl+X` toggle, and retry lineage stays visible alongside them.
- Archiving arms for explicit Y confirmation like deletion, unarchiving runs immediately as the safe direction, and the most recent committed archive or unarchive stays reversible with exactly one `Ctrl+Z` until superseded.
- Six ignored real-PTY scenarios drive the actual binary for first run and restoration, returning-profile offline replay with resize and restart, multi-session switching and destructive confirmations, invalid-call repair, permission deny and allow with durable replay, and forced-shutdown recovery.
- The Windows, macOS, and Linux test matrix runs the PTY group serially after the ordinary workspace suite, while non-terminal test hosts keep the baseline deterministic.
- Locked or unavailable vault access now has an explicit safe error and degrades to session-only operation, and corrupt catalog-cache reads are discarded before live replacement.
- Environment credentials are selected only for the effective provider, so simultaneous Gemini and router variables cannot cross-configure an adapter and an environment override no longer hides the active profile.
- The configured-router live matrix includes plain chat beside the existing function-call probe, matching the Gemini plain-chat and function-call coverage with structural assertions only.
- The `benchmark-instrumentation` feature emits monotonic first-draw, input, dispatch, decoded-chunk, and rendered-revision markers over content-free loopback UDP with process-local correlation.
- The isolated `terminal_latency` runner launches the instrumented binary in a real PTY, drives a loopback structural router, validates marker correlation, separates harness from network intervals, and produces distribution reports covered by unit and Clippy gates.
- The terminal release checklist gates secret scanning, accessibility, restoration, documentation accuracy, benchmark provenance, database backup, rollback preparation, defect severity, and promotion approval.
- A real Windows terminal smoke selected the loopback router, durably completed one prompt, emitted one complete correlated marker chain, exited successfully, and restored the terminal.
- PTY hardening now answers Windows cursor-position reports, prevents inactive rename and archive commands from replacing the active projection, deletes event-causation DAGs in reverse sequence, clears cancelled confirmation notices, and projects durable tool calls into structured transcript rows.
- ADR-0013 is accepted: non-secret `uncommitted_save` and `delete` records make cross-system profile document and vault mutations deterministic and restart-safe.
- Settings schema 2 adds optional profile default models and credential recovery state; schema-v1 documents remain readable and migrate on mutation.
- `ProfileManager` serializes create, edit, duplicate, activate, save, replace, disconnect, delete, default-model, and recovery operations while the TUI sees only typed intents and safe read models.
- Distinct deterministic vault references scope credentials to exact profile identities, duplicated profiles start disconnected, and deletion of one profile leaves every other profile and key unchanged.
- Runtime profile switching rebuilds the correct managed Gemini or router adapter, applies provider-matched environment precedence, refreshes the catalog, and reapplies a compatible saved default model.
- The full-screen `Ctrl+G` Profiles and Providers center shows the local workspace and defaults, searchable profiles, source and connection state, responsive details, masked credential entry, safe test results, recovery state, and confirmed destructive actions.
- Profile-center render and interaction tests cover 120x50, 120x40, 80x24, 60x18, and 40x12 layouts, guided Gemini and router forms, masking, filtering, typed actions, and focus-safe confirmation.
- A composed coordinator test manages two providers and distinct credentials through switching, live structural catalog tests, default assignment, scoped deletion, shutdown, reopen, and secret-free durable files.
- The real profile-center PTY journey creates Gemini and router profiles, duplicates without credential linkage, activates, cancels and confirms deletion, exits successfully, and restores the terminal.
- The opt-in operating-system vault test passed real save, load, replace, and delete behavior against Windows Credential Manager without printing secret values.

## Known gaps

- Reviewed live Gemini compatibility verification passed on 2026-08-22; the checked-in evidence now includes a fixture recorded from the same live dialect.
- No successful reviewed live router compatibility verification has been performed; checked-in router and function-calling dialect evidence is fixture-backed.
- The current TUI still uses independent overlays rather than the Phase 3.7 route-based shell, and it has no full settings editor, theme selection, high-contrast or no-color mode, ASCII mode, or single-column accessibility presentation.
- The opt-in platform-vault smoke passed Windows Credential Manager; macOS Keychain and Linux Secret Service evidence remains open.
- Session deletion archives events but does not yet garbage-collect content-addressed artifacts owned exclusively by the deleted session; Markdown export files are written beside the database but never garbage-collected after session deletion either.
- No reviewed reference-machine benchmark report exists; runtime markers and report automation are implemented, but authoritative storage and terminal latency numbers require an approved machine and release-candidate run.
- Seven dedicated PTY scenarios, including profile management, pass locally on Windows; Linux, macOS, and refreshed Windows evidence awaits the updated pull-request CI run.
- The checked-in release checklist has not been executed against a committed release candidate.

## Next milestone exit target

Phase 3.6 must pass green baseline and dedicated PTY pull-request gates on Windows, macOS, and Linux, collect macOS and Linux platform-vault smoke evidence, and merge to `dev`.
Phase 3.7 then exits only when the responsive route-based shell replaces overlay-led primary navigation without regressing the stable session, profile, credential, provider, and settings intents.
