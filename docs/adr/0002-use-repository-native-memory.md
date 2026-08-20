# ADR-0002: Use repository-native layered memory and ADRs

**Status:** Accepted

**Date:** 2026-08-20

**Owners:** Project maintainers

## Context and problem statement

AutoHarness will be developed across many human and agent sessions. Chat history is not a reliable project authority, and a single ever-growing memory file becomes stale, contradictory, and expensive to load.

The repository needs a tool-neutral memory system that preserves stable intent, current work, verified progress, architecture, and rationale without duplicating Git history.

## Decision drivers

- Compatibility across coding agents and editors.
- Small always-loaded context.
- Clear separation between durable facts and volatile work state.
- Discoverable architectural rationale.
- Version control, review, and evidence links.
- A maintenance protocol that future agents can follow.

## Considered options

1. Rely on chat/thread history.
2. Use one tool-specific rules or memory directory.
3. Require every agent to load one complete Cline-style memory bank on every task.
4. Use a concise root `AGENTS.md`, routed Markdown memory, and numbered ADRs.

## Decision outcome

Chosen option: **a concise root `AGENTS.md` plus layered repository memory and numbered Markdown ADRs**.

The three core memory files are:

- `project.md` for stable purpose and constraints.
- `active.md` for current work state and immediate next actions.
- `progress.md` for verified milestone status.

`docs/README.md` routes task-specific architecture, plan, research, and decision documents. Detailed handoffs are exceptional and live under `docs/memory/handoffs/`.

## Consequences

### Positive

- `AGENTS.md` is recognized across a broad tool ecosystem.
- Future sessions can resume without relying on private conversation state.
- Stable, active, and historical information have distinct authorities.
- Progressive disclosure limits context cost as documentation grows.
- ADRs preserve alternatives and rationale without polluting active memory.

### Negative

- The system relies on contributors and agents reconciling memory after material work.
- Markdown does not enforce freshness or consistency by itself.
- Some tools may still require a thin compatibility file or explicit configuration.
- Incorrectly copied facts can persist until reviewed.

### Follow-up

- Add lightweight documentation validation after the Rust workspace establishes the project's normal tooling.
- Add nested `AGENTS.md` only for future crates with genuinely local rules.
- Periodically archive or remove stale handoff documents after their durable information is promoted.

## Evidence

- [AGENTS.md open format](https://agents.md/)
- [Reference-project review](../research/agent-memory-patterns.md)
- [Markdown Architectural Decision Records](https://adr.github.io/madr/)

## Related decisions

- [ADR-0001](0001-use-rust-modular-monolith.md)
