# Progress memory

**Last reviewed:** 2026-08-20

**Evidence rule:** Mark capabilities complete only when verified by repository contents, automated checks, or observable behavior.

## Milestones

| Phase | Status | Verified outcome |
| --- | --- | --- |
| 0. Repository foundation | Complete | Plan, architecture, memory protocol, research, initial ADRs, and validated local links are present |
| 1. Terminal vertical slice | Not started | No Rust workspace or executable exists |
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
- Local and remote `dev` integration branches exist, and the repository documents the `main -> dev -> feat/<name>` workflow.

## Known gaps

- No license or contribution guide.
- No Rust toolchain, workspace, dependency policy, or continuous integration.
- No code, tests, benchmarks, migrations, or schemas.
- No Gemini or model-router connection.
- No automated documentation-link or memory-consistency check.

## Next milestone exit target

Phase 1 must deliver a selectable, cancellable, streamed Gemini conversation with durable session replay and the verification suite defined in [the project plan](../PROJECT_PLAN.md).
