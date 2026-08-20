# Repository and runtime memory patterns

**Reviewed:** 2026-08-20

**Purpose:** Preserve the evidence behind AutoHarness's repository-memory and runtime-memory conventions.

## Sources reviewed

The review focused on successful open-source agent projects and cross-tool conventions:

- [OpenCode `AGENTS.md`](https://github.com/anomalyco/opencode/blob/cd2327e888119bb084895b615f5abc8f81afcc3f/AGENTS.md)
- [OpenCode session-context design](https://github.com/anomalyco/opencode/blob/5e5955d344a151afcaa43361e45cf76fd370fc52/CONTEXT.md)
- [OpenHands `AGENTS.md`](https://github.com/OpenHands/OpenHands/blob/13e724cc79a4efe71ba93180fa1c54aa752ae7a9/AGENTS.md)
- [Cline Memory Bank](https://github.com/cline/prompts/blob/bb2de2ee6ba6ff93bca517693d54fa645b849060/.clinerules/memory-bank.md)
- [Roo Code custom-instruction hierarchy](https://github.com/RooCodeInc/Roo-Code/blob/5a44a5f98f76b989af93bfca7a947b492b9b5a12/apps/docs/docs/features/custom-instructions.md)
- [AGENTS.md open format](https://agents.md/)
- [Markdown Architectural Decision Records](https://adr.github.io/madr/)

These links are commit-pinned where possible so later changes do not erase the basis for this design.

## Findings

### OpenCode

OpenCode separates repository instructions from its runtime context model. Its `AGENTS.md` contains actionable repository rules. Its context design distinguishes durable session history, typed context sources, context snapshots, context epochs, and safe provider-turn boundaries.

The most important runtime lesson is that context changes should not mutate an in-flight provider request. Sources are observed and admitted deterministically at a boundary, and the admitted representation is durable enough to audit and replay.

### OpenHands

OpenHands uses `AGENTS.md` for always-relevant repository knowledge and repository microagents for narrower expertise. Triggered knowledge avoids placing every specialist instruction into every model turn.

The lesson is progressive disclosure: keep routing information always available and load specialized knowledge only when the task needs it.

### Cline

Cline's Memory Bank separates a stable project brief, product context, system patterns, technical context, active context, and progress. This is a useful distinction between durable knowledge and current work state.

Its default instruction to read every memory file on every task becomes expensive as a repository grows. AutoHarness keeps the stable/active/progress separation but adds a small routing index and task-relevant loading.

### Roo Code and the AGENTS.md ecosystem

Roo supports layered global, workspace, and mode-specific rule files while also recognizing root `AGENTS.md`. The broader AGENTS.md convention provides a cross-tool root entry point and supports nested files for monorepos.

The lesson is to use `AGENTS.md` as the single cross-tool authority. AutoHarness will not create parallel `.clinerules`, `.roo/rules`, or similar copies unless a future integration requires a thin reference.

### ADR practice

Architecture Decision Records preserve context, alternatives, decisions, and consequences as small version-controlled Markdown files. They are a better home for durable rationale than active-status notes or an ever-growing instruction file.

## AutoHarness conventions

AutoHarness adopts:

- One concise root `AGENTS.md` as the always-loaded entry point.
- Three small core memory files: stable project memory, active state, and progress.
- Progressive disclosure through `docs/README.md`.
- Append-only numbered ADRs for costly or cross-cutting decisions.
- Detailed handoff notes only for unfinished work that cannot fit concisely in active memory.
- A runtime design based on durable events, context epochs, explicit admission boundaries, provenance, and deterministic replay.

AutoHarness intentionally avoids:

- Duplicating the same rules in tool-specific directories.
- Treating chat transcripts as authoritative project memory.
- Loading an unbounded documentation tree into every task.
- Recording Git history again in status documents.
- Allowing model-generated statements to become trusted runtime memory without validation.
- Mutating active model context asynchronously during a provider turn.
