# ADR-0001: Use Rust and a modular monolith

**Status:** Accepted

**Date:** 2026-08-20

**Owners:** Project maintainers

## Context and problem statement

AutoHarness needs a foundation for a fast terminal application, streaming network workloads, durable local state, future isolated plugins, and eventually distributed agent execution. Development time is not the primary constraint; runtime performance, reliability, scalability, and customization are.

The architecture must avoid binding the engine to a terminal framework or premature service topology.

## Decision drivers

- Native startup and predictable resource use.
- Memory and concurrency safety in a long-lived autonomous runtime.
- Strong async networking and cancellation support.
- Cross-platform single-binary distribution.
- A mature terminal UI and WebAssembly embedding ecosystem.
- Testable process boundaries without mandatory microservices.

## Considered options

1. Rust with Tokio and Ratatui.
2. TypeScript with Bun and OpenTUI, following OpenCode closely.
3. Go with Bubble Tea and subprocess or WebAssembly extensions.

## Decision outcome

Chosen option: **Rust 2024 with Tokio and Ratatui, organized initially as a modular-monolith Cargo workspace**.

The headless engine exposes commands, events, and ports. The application composes terminal, provider, storage, plugin, and telemetry adapters in one process. A versioned daemon protocol and remote workers are later stages, not first-release requirements.

## Consequences

### Positive

- Native performance, strong type safety, explicit ownership, and bounded concurrency.
- The Wasmtime ecosystem aligns with the desired capability-based plugin model.
- Engine semantics can be tested without terminal or network dependencies.
- One executable provides a simple local installation and debugging experience.

### Negative

- Provider SDK availability is weaker than TypeScript, so some APIs require direct REST/SSE adapters.
- Rust has a steeper extension-authoring and contribution curve.
- Ratatui provides rendering primitives rather than a complete large-application architecture.
- Compile times and Wasmtime dependency size require active management.

### Follow-up

- Establish the Rust toolchain and workspace in Phase 1.
- Define performance benchmarks before setting release thresholds.
- Evaluate WIT component authoring and a supervised JSON-RPC bridge before finalizing the plugin SDK.
- Supersede this ADR if measured constraints justify another runtime or process architecture.

## Evidence

- [OpenCode's Bun/TypeScript and OpenTUI dependency baseline](https://github.com/anomalyco/opencode/blob/0f11d0c3966af3af6f7ca188b79f46a1e241f12d/package.json)
- [Ratatui architecture and rendering model](https://ratatui.rs/concepts/rendering/)
- [Tokio asynchronous runtime](https://tokio.rs/)
- [Wasmtime component embedding](https://docs.wasmtime.dev/api/wasmtime/component/)

## Related decisions

- [ADR-0002](0002-use-repository-native-memory.md)
