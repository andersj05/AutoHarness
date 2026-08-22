# Active memory

**Last reviewed:** 2026-08-22

**Phase:** 4 - Persistent context and memory

**Status:** Phase 3 complete and security remediated - Phase 4 ready for design and implementation

## Current objective

Add the safe monotonic benchmark markers and choose a reference machine, then begin deterministic context epochs and untrusted memory proposal contracts without weakening the durable tool boundary.

## Current repository state

- The repository contains a ten-crate Rust 2024 workspace pinned to Rust 1.97.1 and a runnable `autoharness` terminal binary.
- `autoharness-domain` and `autoharness-engine` define schema-v1 commands and events, deterministic replay, durable attempt and tool lifecycles, immutable run limits, explicit permissions, cancellation state, usage, safe failures, and retry lineage.
- `autoharness-tool` provides the trusted versioned tool registry, deny, ask, and allow policy, restart-aware run budgets, content-addressed artifacts, and workspace filesystem, direct-process, and exact-origin HTTP capability ports.
- `autoharness-provider` exposes provider-neutral catalog, streaming, versioned tool definition, complete tool call, and tool-result ports.
- `autoharness-provider` also provides shared SSE framing, fixture conformance assertions, capability preflight, deadlines, bounded pre-stream retries, concurrency, per-project rate limits, and catalog freshness policy.
- `autoharness-provider-gemini` implements paginated Google model discovery, stable Interactions v1 streaming, a narrow pre-stream Generate Content fallback, cancellation, retry classification, limits, environment or in-app credential admission, and credential redaction.
- `autoharness-provider-openai` implements configurable OpenAI-compatible router discovery and streamed chat completions with a validated base URL, configurable sensitive authentication header, pagination, cumulative usage, cancellation, limits, and credential redaction.
- `autoharness-store` and `autoharness-store-sqlite` provide an event-authoritative store, transactional projections, WAL-mode local durability, idempotent append, migration verification, projection rebuilding, and an integrity-checked provider-neutral model-catalog cache.
- `autoharness-tui` provides a Ratatui model/update/view client with masked zeroizing API-key entry, a searchable model picker, Unicode multiline composer, streaming transcript, scoped tool permission overlay, cancellation, retry, usage, errors, scrolling, and compact rendering.
- `autoharness-app` selects Gemini or the configured router, composes both through the same managed provider and coordinator path, runs durable provider and tool turns, accepts runtime credentials, owns dedicated SQLite work, performs conservative startup recovery, propagates process cancellation, emits structured tracing, discovers the data and workspace directories, and holds an exclusive writer lease.
- Formatting, strict Clippy, warning-denied rustdoc, doctests, and the full workspace test suite pass locally across the complete Phase 3 slice.
- A PTY smoke run without a Gemini credential rendered the complete 80-by-24 terminal interface, confined all application files to an isolated data directory, exited successfully, and restored the terminal.
- A credential-overlay PTY smoke run masked a bracketed-paste sentinel, cleared it on dismissal, reopened an empty editor, found no sentinel bytes in application files, and restored the terminal.
- A Phase 2 PTY smoke run discovered and selected a model through a local router fixture, streamed a completed response, restarted with the fixture offline, restored the selected model and transcript through replay plus the fresh catalog cache, found no credential bytes in application files, and restored the terminal on both exits.
- The tests use local HTTP fixtures and fake providers; no live Gemini network request has been exercised.
- A checked-in isolated benchmark environment measures durable append, projection reads, and warm SQLite recovery without provider requests, and includes an idle resident-memory sampler.
- Continuous integration defines formatting, Clippy, documentation, doctest, native Linux, Windows, and macOS gates, plus separate formatting, Clippy, and test gates for the isolated benchmark workspace.

## Recently completed

- Completed the Phase 3 safe agent execution path without adding provider-native payloads or concrete capabilities to the engine.
- Added strict schema-v1 filesystem read, filesystem write, direct process, and HTTP tools whose trusted registry derives authority from bounded model arguments.
- Added durable call, permission, human-answer, effect-start, result, denial, cancellation, unknown, pause, resume, turn, and run-budget events.
- Added deny-by-default scoped policy, one-call human approval, workspace and exact-origin confinement, no-redirect HTTP, shell rejection, cooperative cancellation, output bounds, and atomic content-addressed artifact publication.
- Added immutable bounds for turns, elapsed time, reported tokens, output bytes, and concurrent tool effects, including elapsed-time reconstruction after restart.
- Removed the unenforceable modeled-cost ceiling until trusted durable pricing snapshots exist, as recorded in [ADR-0008](../adr/0008-defer-modeled-cost-authority.md).
- Completed Phase 3 security diff audits and remediated every reportable finding by requiring permission for reads, exposing exact critical permission details, bounding aggregate provider tool state, conservatively settling ambiguous effects, enforcing parent-child authority lifetimes during live execution and recovery, rejecting Windows batch programs, and preventing credential reconstruction within or across normalized provider events.
- Normalized fragmented Gemini Interactions function calls and OpenAI-compatible streamed tool calls into the same provider-neutral complete call and reconstructed native result history for continuation.
- Added a terminal permission overlay and verified that a write does not happen before the exact allow answer commits.
- Verified the composed allow, execute, provider continuation, durable completion, shutdown, SQLite reopen, and replay path.
- Verified recovery preserves unanswered permissions, marks started effects unknown without replay, and resumes only settled paused turns.
- Verified a denied workspace-secret read reaches neither durable files nor provider-visible history, a provider error settles pending children before the parent, and a post-commit tool error remains durably unknown.
- Recorded the durable capability boundary in [ADR-0007](../adr/0007-use-durable-capability-tool-runtime.md).
- Completed the Phase 2 configurable OpenAI-compatible router path without changing engine command or event types.
- Extracted shared provider conformance assertions and incremental SSE framing from the Gemini-only implementation.
- Added shared dispatch and idle timeouts, safe bounded retries, concurrency, per-project rate limits, and known-capability rejection before dispatch.
- Added schema-v1 durable catalog snapshots with SQLite migration integrity, fresh-cache startup, explicit refresh, bounded stale fallback, and fail-closed authentication policy.
- Verified the real router adapter through the same application coordinator, engine, store, and terminal ports used by Gemini.
- Verified the actual router terminal flow and an offline restart against an isolated data directory without retaining the router credential.
- Recorded the router and shared provider-policy contract in [ADR-0006](../adr/0006-use-openai-compatible-router-boundary.md).
- Added a startup API-key overlay and `Ctrl+K` replacement flow so interactive users can paste a Gemini key without first configuring their shell.
- Recorded the ephemeral credential lifetime, zeroization, masking, transfer, and non-persistence contract in [ADR-0005](../adr/0005-use-ephemeral-in-app-credentials.md).
- Verified that credential values are redacted from TUI and intent debug output, absent from rendered buffers, and absent from SQLite and related durable files.
- Verified the composed credential handoff and catalog load with a fake provider, plus the actual terminal overlay and bracketed-paste path without a live network request.
- Completed the Phase 1 headless, provider, SQLite, Ratatui, and application-composition path.
- Recorded stable Gemini Interactions v1 with stateless local history and a constrained compatibility fallback in [ADR-0004](../adr/0004-use-gemini-interactions-v1.md).
- Verified the composed select, submit, stream, cancel, retry, shutdown, reopen, and replay path against a fake provider and real SQLite store.
- Verified paginated catalog requests, arbitrary byte and SSE fragmentation, stream cancellation, retry classification, fallback boundaries, and secret redaction against local HTTP fixtures.
- Added terminal lifecycle tests, fixed-size golden render tests, tiny-terminal coverage, and content-control sanitization.
- Added a checked-in benchmark runner, idle-memory sampler, result-provenance template, and exact marker contract for deferred latency metrics.
- Documented application startup, configuration, controls, data files, logging, and recovery in the root [README](../../README.md).

## Immediate next actions

1. Add the safe monotonic markers required to measure startup, dispatch, and rendered-delta latency.
2. Record the checked-in benchmark suite on an approved reference machine.
3. Decide the license and contribution policy before the first public release.
4. Define Phase 4 context epochs, deterministic context construction, and untrusted memory proposal contracts.

## Open questions

- Which open-source license should govern the repository?
- What reference machine should define startup and stream-overhead benchmarks?

## Blockers

None for beginning Phase 4 design or adding the benchmark markers.
An approved reference machine is required before recording authoritative latency results.

## Handoff note

The next implementation task should follow Phase 4 of [the project plan](../PROJECT_PLAN.md) and start with deterministic context epoch and memory proposal contracts inward of retrieval and storage adapters.
Preserve the Phase 3 rule that model-authored data is untrusted and cannot grant authority.
