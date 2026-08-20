# Active memory

**Last reviewed:** 2026-08-20

**Phase:** 2 - Provider and router platform

**Status:** Phase 2 ready to start - Phase 1 locally verified

## Current objective

Extract reusable provider-conformance coverage from the Gemini adapter, then define and implement the configurable model-router path without changing the engine or TUI session flow.

## Current repository state

- The repository contains an eight-crate Rust 2024 workspace pinned to Rust 1.97.1 and a runnable `autoharness` terminal binary.
- `autoharness-domain` and `autoharness-engine` define schema-v1 commands and events, deterministic replay, durable attempt lifecycles, cancellation state, usage, safe failures, and retry lineage.
- `autoharness-provider` exposes provider-neutral catalog and streaming ports.
- `autoharness-provider-gemini` implements paginated Google model discovery, stable Interactions v1 streaming, a narrow pre-stream Generate Content fallback, cancellation, retry classification, limits, and credential redaction.
- `autoharness-store` and `autoharness-store-sqlite` provide an event-authoritative store, transactional projections, WAL-mode local durability, idempotent append, migration verification, and projection rebuilding.
- `autoharness-tui` provides a Ratatui model/update/view client with a searchable model picker, Unicode multiline composer, streaming transcript, cancellation, retry, usage, errors, scrolling, and compact rendering.
- `autoharness-app` composes the terminal, bounded coordinator, provider, dedicated SQLite writer, startup recovery, process-level cancellation, structured tracing, data-directory discovery, and an exclusive writer lease.
- Formatting, strict Clippy, warning-denied rustdoc, doctests, and the full workspace test suite pass locally across the complete Phase 1 slice.
- A PTY smoke run without a Gemini credential rendered the complete 80-by-24 terminal interface, confined all application files to an isolated data directory, exited successfully, and restored the terminal.
- The tests use local HTTP fixtures and fake providers; no live Gemini network request has been exercised.
- A checked-in isolated benchmark environment measures durable append, projection reads, and warm SQLite recovery without provider requests, and includes an idle resident-memory sampler.
- Continuous integration defines formatting, Clippy, documentation, doctest, native Linux, Windows, and macOS gates, plus separate formatting, Clippy, and test gates for the isolated benchmark workspace.

## Recently completed

- Completed the Phase 1 headless, provider, SQLite, Ratatui, and application-composition path.
- Recorded stable Gemini Interactions v1 with stateless local history and a constrained compatibility fallback in [ADR-0004](../adr/0004-use-gemini-interactions-v1.md).
- Verified the composed select, submit, stream, cancel, retry, shutdown, reopen, and replay path against a fake provider and real SQLite store.
- Verified paginated catalog requests, arbitrary byte and SSE fragmentation, stream cancellation, retry classification, fallback boundaries, and secret redaction against local HTTP fixtures.
- Added terminal lifecycle tests, fixed-size golden render tests, tiny-terminal coverage, and content-control sanitization.
- Added a checked-in benchmark runner, idle-memory sampler, result-provenance template, and exact marker contract for deferred latency metrics.
- Documented application startup, configuration, controls, data files, logging, and recovery in the root [README](../../README.md).

## Immediate next actions

1. Extract a provider-conformance suite from the Gemini fixture tests and pin the provider-neutral behavioral contract.
2. Define the router's base URL, authentication-header, model-discovery, and OpenAI-compatible streaming configuration.
3. Implement the router adapter behind the existing provider ports without changing engine or TUI types.
4. Add timeout, retry, concurrency, rate-limit, and catalog-cache policy at the provider boundary.
5. Add the safe monotonic markers required to measure startup, dispatch, and rendered-delta latency, then record results on an approved reference machine.
6. Decide the license before the first public release.

## Open questions

- What protocol, base URL, authentication scheme, and model-discovery endpoint does the user's model router expose?
- Which open-source license should govern the repository?
- What reference machine should define startup and stream-overhead benchmarks?

## Blockers

None for extracting provider-conformance tests or measuring the existing slice.
The router's external contract is required before its production adapter can be completed.

## Handoff note

The next implementation task should start from the provider ports in [`autoharness-provider`](../../crates/autoharness-provider/src/lib.rs) and the fixture-backed behavior in [`autoharness-provider-gemini`](../../crates/autoharness-provider-gemini/src/lib.rs).
Follow Phase 2 of [the project plan](../PROJECT_PLAN.md), preserve the existing engine and TUI contracts, and keep provider-native payloads inside adapters.
