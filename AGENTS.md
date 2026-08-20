# AutoHarness agent instructions

## Purpose

AutoHarness is an open-source, self-improving agent harness.
The first product slice is a fast Rust terminal application that discovers Google AI Studio models, streams model responses, and persists replayable sessions.
The long-term system improves prompts, policies, routing, tools, memory, and code only through measured experiments and gated promotion.

## Read before working

Read these small memory files at the start of every task:

1. `docs/memory/project.md` for durable product constraints.
2. `docs/memory/active.md` for the current objective and next actions.
3. `docs/memory/progress.md` for milestone state and known gaps.

Then read only the task-relevant documents routed from `docs/README.md`.
Review `docs/adr/README.md` before changing an architectural decision.

## Memory protocol

- Keep stable product facts in `docs/memory/project.md`.
- Keep only present-tense work state, blockers, and immediate next actions in `docs/memory/active.md`.
- Update `docs/memory/progress.md` when milestone status or verified capability changes.
- Record significant decisions as one numbered ADR in `docs/adr/`; do not silently rewrite an accepted decision.
- Put detailed unfinished-task handoffs in `docs/memory/handoffs/` only when another session needs information that does not belong in active memory.
- At the end of a material task, reconcile active memory and progress with the actual repository state.
- Use ISO dates (`YYYY-MM-DD`) and link to evidence.
- Do not duplicate Git history, speculative ideas, or transient conversation.
- Never store API keys, tokens, credentials, private endpoints, personal data, or raw secret-bearing tool output in repository memory.

## Architecture guardrails

- Use Rust 2024 and a modular-monolith workspace until a measured need justifies process separation.
- Keep the headless engine independent of Ratatui, concrete providers, SQLite, and plugin runtimes.
- Provider adapters translate native protocols into one typed internal event stream.
- Persist durable inputs and events before relying on in-memory coordination.
- Make cancellation, backpressure, retries, budgets, and permissions explicit.
- Treat model-authored memory as an untrusted proposal until it passes validation and provenance checks.
- Do not allow a production agent to directly promote its own behavioral or source changes.
- Prefer one source of truth.
- Link to an existing contract instead of copying it into another document or module.

## General engineering guidelines

- Never use an em dash.
- Use a plain hyphen-minus (`-`) instead.
- Never add an agent name or a `Co-authored-by` line to commit messages.
- Never manually modify `CHANGELOG.md` files or any file marked as generated or auto-generated.
- When writing or substantially editing a long Markdown file, put each complete sentence on its own physical line.
- Preserve normal Markdown structure, but do not place multiple sentences on one physical line.
- Do not give development cost much weight in technical decisions.
- Prefer quality, simplicity, robustness, scalability, and long-term maintainability.
- For a bug fix, first reproduce the bug end to end as closely as possible to the way an end user experiences it.
- Use that reproduction to verify that the fix addresses the actual user-visible problem.
- During end-to-end product testing, inspect the UI critically and require pixel-level quality.
- Fix clearly visible UI defects found in the affected flow, even when they are not the original focus of the task.
- Apply the same standard to engineering quality.
- Fix lint failures, test failures, and flaky tests discovered in the affected validation path, even when the current change did not cause them.

## Branching workflow

- Use `main -> dev -> feature branch` as the permanent branch hierarchy.
- Keep `main` release-ready and accept regular changes into it only through a promotion from `dev`.
- Keep `dev` as the long-lived integration branch and the base for normal development.
- Create every regular feature, fix, documentation, or refactor branch from the latest `dev`.
- Open regular pull requests from the short-lived branch into `dev`.
- Promote validated releases from `dev` into `main` with a dedicated pull request.
- Do not open a regular feature pull request directly against `main`.
- Use a short namespace and kebab-case topic for branch names, such as `feature/gemini-provider` or `codex/gemini-provider`.
- Delete short-lived branches after merge.
- For an emergency production hotfix, branch from `main`, merge the fix into `main`, and immediately reconcile the same fix back into `dev`.
- See [ADR-0003](docs/adr/0003-use-main-dev-feature-branches.md) for the durable decision.

## Development workflow

- No build, lint, or test commands exist until the Rust workspace is scaffolded.
- Do not invent or document commands that have not been verified.
- Add focused tests with implementation changes.
- Provider tests must cover pagination, arbitrarily fragmented streams, cancellation, retries, and secret redaction.
- Keep the terminal render loop free of network, storage, and model logic.
- Preserve unrelated user changes and keep patches scoped.
- Use conventional commit subjects when commits are requested: `type(scope): summary`.

## Documentation workflow

- Update the project plan only when scope, sequencing, or exit criteria change.
- Update architecture documents when contracts or invariants change.
- Add an ADR for decisions that are costly to reverse, cross component boundaries, or constrain future implementations.
- Keep this file concise and cross-tool.
- Add nested `AGENTS.md` files only when a future crate needs genuinely local instructions.
