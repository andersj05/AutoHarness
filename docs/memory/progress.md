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
| 4. Persistent context and memory | Designed | Architecture is documented; runtime is not implemented |
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
- The executable composes the terminal client, bounded async coordinator, runtime Gemini provider replacement, dedicated blocking SQLite writer, startup recovery, explicit cancellation, application data paths, one-writer locking, and content-free structured tracing.
- Recovery settles never-dispatched attempts as retryable failures and marks ambiguously dispatched attempts unknown without inventing a provider outcome.
- Tool recovery preserves unanswered permission requests only for parents already awaiting tools, settles every live child before marking an interrupted parent unknown, marks started effects unknown without replay, and resumes a paused provider turn only after every call is settled.
- A composed integration test proves a workspace write cannot happen before the exact human allow event commits, then verifies execution, provider continuation, durable completion, shutdown, SQLite reopen, and replay-equivalent tool state.
- A composed integration test selects a model, persists a prompt, streams partial output, cancels, retries to completion, shuts down, reopens SQLite, and verifies a replay-equivalent visible session.
- Compatibility tests pin every command and event serialization shape and verify prompt preservation, API-key masking and non-persistence, debug redaction, identifier validation, replay integrity, failure atomicity, and terminal restoration.
- Local validation passes formatting, strict Clippy, warning-denied rustdoc, doctests, and the full workspace test suite.
- A PTY smoke run without a Gemini credential rendered the complete 80-by-24 terminal interface, confined application files to an isolated data directory, exited successfully through `Ctrl+C`, and restored the terminal.
- A credential-overlay PTY smoke run masked a bracketed-paste sentinel, cleared it when dismissed, reopened an empty editor through `Ctrl+K`, excluded the sentinel from application files, and restored the terminal.
- The isolated benchmark environment measures durable append with synchronous projections, transcript-read throughput, and warm SQLite reopen with strict replay for representative session sizes while explicitly excluding network latency.
- A PowerShell idle resident-memory sampler is available, and the benchmark documentation defines provenance requirements and exact monotonic markers for deferred latency metrics.
- Continuous integration defines formatting, lint, documentation, doctest, native Linux, Windows, and macOS test gates, plus separate formatting, lint, and test gates for the isolated benchmark workspace.

## Known gaps

- No license or contribution guide.
- No live Gemini network verification has been performed; provider and function-calling protocol evidence is fixture-backed.
- No live router network verification has been performed; router and function-calling dialect evidence is fixture-backed.
- No reviewed reference-machine benchmark report exists, and cold-start, input-to-dispatch, and provider-chunk-to-render latency still lack runtime markers.
- No automated documentation-link or memory-consistency check.

## Next milestone exit target

Phase 4 must make every injected memory attributable to a durable source and admission decision, treat model-authored memory as an untrusted proposal, and construct the same context for the same event log, configuration, catalog snapshot, and token budget.
