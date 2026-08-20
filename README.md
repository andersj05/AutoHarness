# AutoHarness

AutoHarness is an open-source agent runtime designed to improve the infrastructure around current language models.
Its long-term goal is to learn from durable execution traces and safely improve prompts, policies, routing, tools, memory, and code through reproducible evaluations and gated promotion.

The architecture and repository-memory foundation is complete, and Phase 1 implementation is in progress.
The first executable milestone is a fast terminal application that discovers selectable Google AI Studio models, streams responses, and records replayable sessions.
A configurable model-router adapter follows immediately afterward.

## Project documentation

- [Project plan](docs/PROJECT_PLAN.md)
- [Architecture overview](docs/architecture/OVERVIEW.md)
- [Persistent memory architecture](docs/architecture/PERSISTENT_MEMORY.md)
- [Repository memory](docs/memory/README.md)
- [Architecture decision records](docs/adr/README.md)
- [Reference-project research](docs/research/agent-memory-patterns.md)

## Current status

The pinned Rust 2024 workspace now contains provider-neutral command and event contracts plus a synchronous headless engine with deterministic replay tests.
No terminal executable, provider adapter, or durable database exists yet.
See [active memory](docs/memory/active.md) for the current objective and [progress](docs/memory/progress.md) for milestone status.

## Development

Run the verified baseline gates from the repository root:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked --no-deps -- -D warnings
cargo test --workspace --all-targets --all-features --locked --no-fail-fast
```

## Guiding principles

- Keep the engine independent from the terminal interface and model providers.
- Treat every provider response as a typed, replayable event stream.
- Preserve provenance for every memory, decision, experiment, and promoted change.
- Evaluate proposed improvements before promotion and retain rollback paths.
- Keep secrets out of source control, logs, transcripts, and model-visible memory.
- Prefer native performance, bounded concurrency, deterministic recovery, and explicit permissions.

Licensing and contributor policies will be finalized before the first public release.
