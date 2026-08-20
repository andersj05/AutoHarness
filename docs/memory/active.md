# Active memory

**Last reviewed:** 2026-08-20

**Phase:** 1 - Fast terminal vertical slice

**Status:** Ready to scaffold

## Current objective

Scaffold the Rust workspace and prove the smallest headless command/event/replay path before adding networking or terminal complexity.

## Current repository state

- The repository contains planning, architecture, ADR, research, and repository-memory documents.
- No Rust workspace or executable implementation exists yet.
- The default branch is `main`.
- The repository memory system uses root `AGENTS.md`, three core memory files, progressive documentation routing, ADRs, and exceptional detailed handoffs.
- `main` is the stable branch, `dev` is the published integration branch, and the current work remains on the short-lived `feat/repository-foundation` branch.
- All current local Markdown links resolve.

## Recently completed

- Evaluated OpenCode, OpenHands, Cline, Roo Code, the AGENTS.md convention, and ADR practice.
- Established the product roadmap from Google AI Studio chat through controlled self-improvement and remote scale.
- Defined the proposed system boundaries and persistent runtime-memory contracts.
- Recorded Rust/modular-monolith and repository-memory decisions.
- Validated the documentation tree and completed Phase 0.
- Adopted repository-wide writing, commit, generated-file, technical-decision, end-to-end testing, UI-quality, and validation guidelines in `AGENTS.md`.
- Established and published the `main -> dev -> feat/<name>` hierarchy and recorded its workflow in `AGENTS.md` and ADR-0003.

## Immediate next actions

1. Scaffold the Rust workspace and pin the supported toolchain.
2. Define the initial domain command/event/error contracts.
3. Add deterministic in-memory replay tests before networking or terminal integration.
4. Implement the smallest Ratatui shell consuming engine events.
5. Decide the license before the first public release and the Gemini default transport before implementing that adapter.

## Open questions

- What protocol, base URL, authentication scheme, and model-discovery endpoint does the user's model router expose?
- Should the first Gemini path default to Interactions or Generate Content while supporting the other as compatibility mode?
- Which open-source license should govern the repository?
- What reference machine should define startup and stream-overhead benchmarks?

## Blockers

None for Phase 1 scaffolding.
Router details are required before implementing that adapter but do not block the Gemini vertical slice.

## Handoff note

The next implementation task should start with [ADR-0001](../adr/0001-use-rust-modular-monolith.md), [the architecture overview](../architecture/OVERVIEW.md), and Phase 1 of [the project plan](../PROJECT_PLAN.md).
Do not create all target crates empty; introduce boundaries with their first consumer.
