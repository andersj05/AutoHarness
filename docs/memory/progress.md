# Progress memory

**Last reviewed:** 2026-08-20

**Evidence rule:** Mark capabilities complete only when verified by repository contents, automated checks, or observable behavior.

## Milestones

| Phase | Status | Verified outcome |
| --- | --- | --- |
| 0. Repository foundation | Complete | Plan, architecture, memory protocol, research, initial ADRs, and validated local links are present |
| 1. Terminal vertical slice | Complete | The fixture-verified terminal path discovers models, streams typed Gemini events, cancels and retries attempts, commits SQLite events and projections, and restores the same visible session after restart |
| 2. Provider/router platform | Not started | Phase 1 established provider-neutral ports and a Gemini adapter; shared conformance coverage and the configurable router remain |
| 3. Safe agent execution | Not started | No tool or permission runtime exists |
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
- The headless engine rejects command-ID reuse and invalid attempt transitions, applies event batches atomically, and reconstructs the same selected model, transcript, usage, and retry lineage from serialized history without using timestamps for order.
- The durable engine appends events before publishing projected state and recovers every stored session through the same strict replay path.
- SQLite runs in WAL mode with verified durability settings, transactional event and projection updates, optimistic sequence checks, byte-identical idempotent append, migration-history validation, corruption detection, and projection rebuilding.
- Provider-neutral catalog and chat ports isolate provider-native protocols from the engine and terminal.
- The Gemini adapter reads `GEMINI_API_KEY` only from the environment, authenticates by a sensitive header, discovers compatible models through opaque-token pagination, streams stable Interactions v1 events, and permits only a narrow pre-stream Generate Content fallback.
- The Gemini decoder normalizes lifecycle, text, completion, and cumulative usage events across arbitrary byte and SSE fragmentation while filtering provider thought steps.
- The Ratatui client includes a searchable model picker, Unicode multiline composer, streamed transcript, cancellation and retry states, safe errors, usage, tail following, manual scrolling, compact layouts, and bounded application mailboxes.
- The executable composes the terminal client, bounded async coordinator, Gemini provider, dedicated blocking SQLite writer, startup recovery, explicit cancellation, application data paths, one-writer locking, and content-free structured tracing.
- Recovery settles never-dispatched attempts as retryable failures and marks ambiguously dispatched attempts unknown without inventing a provider outcome.
- A composed integration test selects a model, persists a prompt, streams partial output, cancels, retries to completion, shuts down, reopens SQLite, and verifies a replay-equivalent visible session.
- Compatibility tests pin every command and event serialization shape and verify prompt preservation, debug redaction, identifier validation, replay integrity, failure atomicity, and terminal restoration.
- Local validation passes formatting, strict Clippy, warning-denied rustdoc, doctests, and the full workspace test suite.
- A PTY smoke run without a Gemini credential rendered the complete 80-by-24 terminal interface, confined application files to an isolated data directory, exited successfully through `Ctrl+C`, and restored the terminal.
- The isolated benchmark environment measures durable append with synchronous projections, transcript-read throughput, and warm SQLite reopen with strict replay for representative session sizes while explicitly excluding network latency.
- A PowerShell idle resident-memory sampler is available, and the benchmark documentation defines provenance requirements and exact monotonic markers for deferred latency metrics.
- Continuous integration defines formatting, lint, documentation, doctest, native Linux, Windows, and macOS test gates, plus separate formatting, lint, and test gates for the isolated benchmark workspace.

## Known gaps

- No license or contribution guide.
- No configurable model-router adapter, shared provider-conformance suite, cross-provider middleware, or durable catalog cache.
- No live Gemini network verification has been performed; provider protocol evidence is fixture-backed.
- No reviewed reference-machine benchmark report exists, and cold-start, input-to-dispatch, and provider-chunk-to-render latency still lack runtime markers.
- No automated documentation-link or memory-consistency check.

## Next milestone exit target

Phase 2 must run the same engine and terminal session path through Gemini and a configurable router, prove adapter interchangeability with a shared conformance suite, reject known unsupported capabilities before dispatch, and keep provider-native payloads outside core types.
