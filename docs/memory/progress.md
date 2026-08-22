# Progress memory

**Last reviewed:** 2026-08-22

**Evidence rule:** Mark capabilities complete only when verified by repository contents, automated checks, or observable behavior.

## Milestones

| Phase | Status | Verified outcome |
| --- | --- | --- |
| 0. Repository foundation | Complete | Plan, architecture, memory protocol, research, initial ADRs, and validated local links are present |
| 1. Terminal vertical slice | Complete | The fixture-verified terminal path discovers models, streams typed Gemini events, cancels and retries attempts, commits SQLite events and projections, and restores the same visible session after restart |
| 2. Provider/router platform | Complete | Gemini and the configurable OpenAI-compatible router pass fixture conformance and the same composed session path through shared provider policy and durable catalog caching |
| 3. Safe agent execution | Complete | Security-audited versioned tools run through explicit durable permission, bounded provider admission, capability, budget, artifact, continuation, parent-child lifetime, and conservative recovery boundaries |
| 3.1. Live protocol reliability and recovery | Active | Gemini argument aggregation, durable invalid-call repair, failed-turn isolation, capability gating, stable diagnostics, and durable `Ctrl+N` recovery pass local fixture, integration, render, replay, and PTY tests; live provider exit evidence remains open |
| 3.2. Complete session lifecycle | Planned | The store can list sessions, but the application and TUI expose only one startup-selected session |
| 3.3. User profiles, settings, and secure credentials | Planned | Environment configuration and session-only credential entry exist; persistent profiles and settings do not |
| 3.4. TUI usability and discoverability | Planned | The focused chat controls exist; full navigation, command discovery, help, and settings surfaces do not |
| 3.5. Terminal release hardening | Planned | Existing fixture, PTY, and security gates remain; the complete terminal product gate is not implemented |
| 4. Persistent context and memory | Designed and gated | Architecture is documented; runtime is not implemented and waits for Phase 3.x |
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
- The repository carries the accepted MIT license decision in ADR-0010 with a root `LICENSE`, a contributor guide, and workspace-level Cargo license metadata.

## Known gaps

- No successful reviewed live Gemini compatibility verification has been performed; checked-in provider and function-calling protocol evidence is fixture-backed.
- No successful reviewed live router compatibility verification has been performed; checked-in router and function-calling dialect evidence is fixture-backed.
- The terminal can create a fresh session but cannot browse, switch, rename, archive, export, or delete sessions even though durable session summaries can be listed by the store.
- The terminal has no user settings or named provider profiles, and pasted credentials are intentionally forgotten at process exit.
- Offline session browsing is unavailable because application composition exposes only one startup-selected session.
- No reviewed reference-machine benchmark report exists, and cold-start, input-to-dispatch, and provider-chunk-to-render latency still lack runtime markers.
- No automated documentation-link or memory-consistency check.

## Next milestone exit target

Phase 3.1 must run the compiled opt-in probes and real terminal plain-chat and approved HTTP-tool continuation paths against live Gemini and the configured router, then record secret-free pass or fail evidence.
