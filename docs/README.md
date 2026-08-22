# Documentation map

This index routes humans and agents to the smallest authoritative document for a task. Do not load every document by default.

## Planning

- [Project plan](PROJECT_PLAN.md): phases, deliverables, exit criteria, risks, and initial implementation order.

## Architecture

- [System overview](architecture/OVERVIEW.md): component boundaries, dependency rules, runtime flow, and proposed workspace.
- [Persistent memory](architecture/PERSISTENT_MEMORY.md): durable session, context, knowledge, and experiment memory contracts.
- [Session export format](architecture/SESSION_EXPORT.md): the provider-neutral JSON export written before destructive deletion.
- [Settings and credentials](architecture/SETTINGS.md): layered settings resolution, provider profiles, and the credential-vault contract.

## Durable decisions

- [ADR index and process](adr/README.md)
- [ADR template](adr/0000-template.md)

## Repository memory

- [Memory protocol](memory/README.md)
- [Project memory](memory/project.md): stable purpose and constraints.
- [Active memory](memory/active.md): current focus and immediate handoff.
- [Progress memory](memory/progress.md): verified milestone state.
- [Detailed handoffs](memory/handoffs/README.md): exceptional, task-specific continuation notes.

## Research

- [Agent memory patterns](research/agent-memory-patterns.md): source review and the conventions adopted for AutoHarness.

## Validation

- [`scripts/check_docs_links.py`](../scripts/check_docs_links.py): verifies that every relative link in every Markdown file resolves and that every ADR is indexed; runs in CI and locally from the repository root.

## Source-of-truth rule

Each fact should have one authoritative home:

| Information | Authority |
| --- | --- |
| Product purpose and durable constraints | `docs/memory/project.md` |
| Current objective and blockers | `docs/memory/active.md` |
| Milestone status | `docs/memory/progress.md` |
| Delivery sequence and exit criteria | `docs/PROJECT_PLAN.md` |
| Current system contracts | `docs/architecture/` |
| Why a significant choice was made | `docs/adr/` |
| Historical code changes | Git history |

Link to the authority instead of maintaining parallel copies.
