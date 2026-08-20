# Project memory

**Last reviewed:** 2026-08-20

**Stability:** Durable; change only when product direction or accepted constraints change.

## Identity

- **Name:** AutoHarness
- **Type:** Open-source agent runtime and experimentation platform
- **Primary interface:** Native terminal application, followed by headless and remote clients

## Vision

Create better infrastructure around current language models than provider-default agent loops offer. AutoHarness should learn from durable evidence and safely improve its prompts, policies, routing, tools, memory, and code while remaining observable, reproducible, customizable, and reversible.

## First user outcome

A user supplies a Google AI Studio credential through `GEMINI_API_KEY`, sees a dynamically discovered list of compatible models, selects one, sends a prompt, receives cancellable streamed output, and can resume the replayable session after restarting AutoHarness.

The next provider outcome is the same experience through the user's configurable model router.

## Durable constraints

- Development time is not the primary optimization. Prefer the strongest long-term architecture when tradeoffs are justified.
- Runtime performance, scalability, customization, safety, and observability are primary qualities.
- Rust 2024 is the core implementation language; see [ADR-0001](../adr/0001-use-rust-modular-monolith.md).
- Begin as a modular monolith and preserve a headless engine boundary.
- The TUI, provider protocols, storage implementation, and plugin runtime are adapters around the engine.
- Normalize provider streams into typed lifecycle events.
- Persist replayable inputs and events before adding autonomous tools.
- Treat model-generated memory and behavior changes as untrusted proposals.
- Improvements require versioned evaluations, guardrails, promotion evidence, and rollback.
- Secrets never enter source control, logs, transcripts, telemetry, or model-visible memory.
- The permanent branch hierarchy is `main -> dev -> short-lived feat/<name> branches`; see [ADR-0003](../adr/0003-use-main-dev-feature-branches.md).
- The project will be open source; the exact license remains an open decision.

## Product principles

- Evidence over intuition.
- Explicit state over hidden process memory.
- Capability-based authority over ambient access.
- Deterministic recovery over best-effort continuation.
- Progressive context disclosure over unbounded prompts.
- Provider-specific excellence at adapters without provider coupling in core.
- Local usability first without closing the path to remote scale.

## Initial technology direction

- Rust 2024, Tokio, Ratatui, and Crossterm.
- SQLite in WAL mode for local durable state.
- Serde-based domain serialization with explicit schema versions.
- `tracing` and OpenTelemetry-compatible observability.
- Wasmtime Component Model with WIT for the eventual primary plugin boundary.
- Supervised, versioned JSON-RPC subprocesses as an extension escape hatch.

Technology details remain subject to focused ADRs and measurements.

## Success definition

AutoHarness succeeds when it can demonstrate that a promoted behavior performs better than its prior version on reproducible user-relevant evaluations, without regressing safety, reliability, cost, or latency guardrails, and can explain and reverse that promotion.

## Non-goals

- Claiming that session memory is foundation-model weight training.
- Allowing a running agent to rewrite and promote itself without independent evaluation.
- Building every UI or provider before the engine contracts are proven.
- Using a vector database as a substitute for provenance, trust, or context policy.
- Prematurely distributing a runtime whose local failure semantics are not yet understood.

## Authoritative documents

- [Project plan](../PROJECT_PLAN.md)
- [Architecture overview](../architecture/OVERVIEW.md)
- [Persistent memory architecture](../architecture/PERSISTENT_MEMORY.md)
- [ADR index](../adr/README.md)
