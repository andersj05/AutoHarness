# Active memory

**Last reviewed:** 2026-08-20

**Phase:** 1 - Fast terminal vertical slice

**Status:** In progress - headless replay slice verified

## Current objective

Build the smallest Ratatui client and application composition on top of the verified headless command/event/replay path.

## Current repository state

- The repository contains a Rust 2024 workspace pinned to Rust 1.97.1.
- `autoharness-domain` defines validated identifiers, create/select/admit commands, schema-v1 session events, causation, correlation, delivery mode, and safe error classification.
- `autoharness-engine` provides a synchronous session reducer, atomic in-memory command execution, strict replay validation, and reconstructed session projections.
- Workspace tests verify deterministic command/event/replay behavior, exact serialized contracts, prompt preservation and debug redaction, sequence and causation integrity, command-ID reuse rejection, and failed-batch atomicity.
- Continuous integration checks formatting, Clippy, documentation, doctests, and native tests on Linux, Windows, and macOS.
- No executable, Ratatui client, provider adapter, or durable store exists yet.
- The repository memory system uses root `AGENTS.md`, three core memory files, progressive documentation routing, ADRs, and exceptional detailed handoffs.
- `main` contains the repository foundation, and `dev` is synchronized with that release state as the base for Phase 1 feature branches.
- All current local Markdown links resolve.

## Recently completed

- Evaluated OpenCode, OpenHands, Cline, Roo Code, the AGENTS.md convention, and ADR practice.
- Established the product roadmap from Google AI Studio chat through controlled self-improvement and remote scale.
- Defined the proposed system boundaries and persistent runtime-memory contracts.
- Recorded Rust/modular-monolith and repository-memory decisions.
- Validated the documentation tree and completed Phase 0.
- Adopted repository-wide writing, commit, generated-file, technical-decision, end-to-end testing, UI-quality, and validation guidelines in `AGENTS.md`.
- Established and published the `main -> dev -> feat/<name>` hierarchy and recorded its workflow in `AGENTS.md` and ADR-0003.
- Promoted the repository foundation from `dev` into `main` through [PR #1](https://github.com/andersj05/AutoHarness/pull/1).
- Scaffolded the Rust workspace, pinned toolchain, lockfile, and cross-platform continuous-integration baseline.
- Implemented the provider-neutral domain contracts and deterministic in-memory replay slice with focused validation.

## Immediate next actions

1. Implement the smallest Ratatui shell consuming only commands, read state, and engine events.
2. Add `autoharness-app` composition only when it can drive that shell through the real headless path.
3. Define the storage port and SQLite migrations for sessions, durable input, attempts, and events.
4. Decide the Gemini default transport, then implement paginated model discovery and adversarial streaming tests.
5. Decide the license before the first public release.

## Open questions

- What protocol, base URL, authentication scheme, and model-discovery endpoint does the user's model router expose?
- Should the first Gemini path default to Interactions or Generate Content while supporting the other as compatibility mode?
- Which open-source license should govern the repository?
- What reference machine should define startup and stream-overhead benchmarks?

## Blockers

None for the current Phase 1 work.
Router details are required before implementing that adapter but do not block the Gemini vertical slice.

## Handoff note

The next implementation task should start from the event-only boundary in [`autoharness-domain`](../../crates/autoharness-domain/src/lib.rs) and the replayed session projection in [`autoharness-engine`](../../crates/autoharness-engine/src/lib.rs).
Follow Phase 1 of [the project plan](../PROJECT_PLAN.md) and keep Ratatui, network, and storage logic outside the headless engine.
Do not create all target crates empty; introduce boundaries with their first consumer.
