# Documentation map

This index routes humans and agents to the smallest authoritative document for a task.
Do not load every document by default.
The desktop GUI is the source default and only product interface under [ADR-0020](adr/0020-retire-terminal-client.md).
Signed distribution remains gated; terminal documentation records historical behavior and evidence.

## Planning

- [Project plan](PROJECT_PLAN.md): phases, deliverables, exit criteria, risks, and initial implementation order.

## Design

- [GUI design system](design/GUI_DESIGN_SYSTEM.md): desktop visual language, layout, components, interaction, accessibility, and validation rules.
- [GUI implementation plan](design/GUI_IMPLEMENTATION_PLAN.md): ordered migration from the terminal client to the desktop GUI.
- [Terminal design system](design/TUI_DESIGN_SYSTEM.md): terminal visual tokens, gradients, icon triples, components, and responsive rules.
- [Terminal interface audit](design/TUI_AUDIT.md): the evidence-backed defect baseline the redesign closes.
- [Terminal interface redesign plan](design/TUI_REDESIGN_PLAN.md): the ordered Phase 3.10 steps and exit criteria.
- [Persistent context and memory plan](design/PERSISTENT_CONTEXT_MEMORY_PLAN.md): the ordered Phase 4 runtime, storage, provider, and terminal slices.

## Architecture

- [GUI architecture](architecture/GUI.md): desktop client ownership, protocol, carrier, security, recovery, and testing contracts.
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
- [DeepSeek Harness GUI patterns](research/deepseek-harness-gui-patterns.md): commit-pinned review of the client layering, protocol, recovery, and packaging patterns adopted or rejected for the desktop migration.

## Release

- [GUI foundation review](release/GUI_FOUNDATION_REVIEW.md): desktop-only source ownership, retirement validation, and remaining distribution evidence.

- [Terminal release checklist](release/TERMINAL_RELEASE_CHECKLIST.md): Phase 3.x cross-platform, security, usability, recovery, benchmark, and rollback promotion gate.
- [TUI redesign validation](release/TUI_REDESIGN_VALIDATION.md): Phase 3.10 step 10 candidate evidence, local results, and outstanding promotion gates.

- [GUI Stage 7 validation](release/GUI_STAGE7_VALIDATION.md): Memory lifecycle, native replay, inert workspace surfaces, responsive review, and remaining desktop release evidence.
- [GUI release checklist](release/GUI_RELEASE_CHECKLIST.md): immutable candidate evidence, signing, approval, default cutover, and retirement gates.
- [GUI update policy](release/GUI_UPDATE_POLICY.md): candidate installers, signing prerequisites, deliberate updates, and rollback.
- [GUI Stage 8 validation](release/GUI_STAGE8_VALIDATION.md): package and native lifecycle evidence, scope limits, and remaining release blockers.

## Validation

- [GUI validation workflow](release/GUI_VALIDATION_WORKFLOW.md): change-scoped pull-request checks, the complete local baseline, and deliberate native candidate runs.

- [`scripts/check_docs_links.py`](../scripts/check_docs_links.py): verifies that every relative link in every Markdown file resolves and that every ADR is indexed; runs in CI and locally from the repository root.

Install frontend dependencies once from the repository root with `pnpm install`.
Use `pnpm gui:dev` for browser-only fixture development and `pnpm gui:desktop` for the native Tauri development preview.
The verified GUI gates are `pnpm gui:typecheck`, `pnpm gui:test`, and `pnpm gui:build`.
The fixture validates renderer behavior only and does not establish native integration, persistence, credential safety, packaging, or signed distribution.

## Source-of-truth rule

Each fact should have one authoritative home:

| Information | Authority |
| --- | --- |
| Product purpose and durable constraints | `docs/memory/project.md` |
| Current objective and blockers | `docs/memory/active.md` |
| Milestone status | `docs/memory/progress.md` |
| Delivery sequence and exit criteria | `docs/PROJECT_PLAN.md` |
| Current system contracts | `docs/architecture/` |
| GUI client ownership, protocol, carrier, security, and recovery | `docs/architecture/GUI.md` |
| GUI visual contract | `docs/design/GUI_DESIGN_SYSTEM.md` |
| Terminal visual contract | `docs/design/TUI_DESIGN_SYSTEM.md` |
| Why a significant choice was made | `docs/adr/` |
| Historical code changes | Git history |

Link to the authority instead of maintaining parallel copies.
