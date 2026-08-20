# Progress memory

**Last reviewed:** 2026-08-20

**Evidence rule:** Mark capabilities complete only when verified by repository contents, automated checks, or observable behavior.

## Milestones

| Phase | Status | Verified outcome |
| --- | --- | --- |
| 0. Repository foundation | Complete | Plan, architecture, memory protocol, research, initial ADRs, and validated local links are present |
| 1. Terminal vertical slice | In progress | Pinned Rust workspace, typed domain contracts, deterministic in-memory command execution, strict replay, tests, and CI exist; the terminal/provider/storage path remains |
| 2. Provider/router platform | Not started | No provider contracts or adapters exist |
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
- Local and remote `main` and `dev` branches contain the repository foundation, and the repository documents the `main -> dev -> feat/<name>` workflow.
- Rust 1.97.1 and Cargo resolver 3 are pinned for the Rust 2024 workspace, with a workspace dependency lockfile.
- Provider-neutral create/select/admit commands produce schema-v1 events with stable identity, sequence, time, causation, correlation, and safe payloads.
- The headless engine rejects command-ID reuse, applies event batches atomically, and reconstructs the same selected model and admitted prompt from serialized history without using timestamps for order.
- Compatibility tests pin every initial command/event serialization shape and verify prompt preservation, debug redaction, identifier validation, replay integrity, and failure atomicity.
- Continuous integration defines formatting, lint, documentation, doctest, and native Linux, Windows, and macOS test gates.

## Known gaps

- No license or contribution guide.
- No executable application, Ratatui client, or terminal restoration tests.
- No provider or storage ports, Gemini or router adapters, SQLite migrations, streaming parser, cancellation, or retry implementation.
- No benchmarks or checked-in benchmark environment.
- No automated documentation-link or memory-consistency check.

## Next milestone exit target

Phase 1 must deliver a selectable, cancellable, streamed Gemini conversation with durable session replay and the verification suite defined in [the project plan](../PROJECT_PLAN.md).
