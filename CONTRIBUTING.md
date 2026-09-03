# Contributing to AutoHarness

Thank you for considering a contribution.
AutoHarness accepts code, documentation, architecture decisions, and evidence-backed experiments through the workflow below.

## License

By contributing, you agree that your contributions are licensed under the [MIT License](LICENSE).

## Read first

1. [`AGENTS.md`](AGENTS.md): repository conventions, guardrails, engineering standards, and the memory protocol.
2. [`docs/README.md`](docs/README.md): routes each task to its smallest authoritative document.
3. [`README.md`](README.md): running the GUI preview or terminal reference client and configuring the runtime.

## Branching

- The permanent hierarchy is `main -> dev -> feat/<name>`.
- Create every regular change branch from the latest `dev`.
- Open pull requests against `dev`, never directly against `main`.
- Releases are promoted from `dev` into `main` with a dedicated pull request.
- See [ADR-0003](docs/adr/0003-use-main-dev-feature-branches.md) for the durable decision.

## Required validation

Run the Rust gates from the repository root before opening a pull request:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked --no-deps -- -D warnings
cargo test --workspace --all-targets --all-features --locked --no-fail-fast
```

These commands remain the comprehensive local baseline while the terminal reference is present.
The [CI workflow](.github/workflows/ci.yml) runs targeted renderer-neutral and desktop-host Rust lanes on Linux, Windows, and macOS without the frozen `autoharness-tui` tests or ignored PTY acceptance scenarios.
Pull requests must keep those targeted Rust jobs green.
Install the pinned frontend workspace dependencies before GUI development:

```text
pnpm install
```

Changes to the GUI, its wire contract, or the desktop bridge must also pass:

```text
pnpm gui:typecheck
pnpm gui:test
pnpm gui:build
```

## GUI preview development

Run `pnpm gui:dev` for browser-only development against deterministic fixture state.
The fixture is suitable for interface and recovery-state work, but it is not evidence of native integration, durable persistence, credential handling, packaging, or terminal parity.
Run `pnpm gui:desktop` to exercise the native Tauri development preview against the Rust host.
The terminal application remains the compatibility and behavioral reference until the GUI satisfies its documented release gate.

## Engineering expectations

- Add focused tests with implementation changes.
- Provider tests must cover pagination, arbitrarily fragmented streams, cancellation, retries, and secret redaction.
- Keep the headless engine independent of Ratatui, Tauri, React, concrete providers, SQLite, and plugin runtimes.
- Put cross-client commands, projections, frames, recovery semantics, and safe failures in the versioned renderer-neutral client contract.
- Keep durable runtime, provider, storage, and policy logic outside both terminal and GUI renderers.
- Normalize provider streams into typed internal events inside adapters only.
- Make cancellation, backpressure, retries, budgets, and permissions explicit.
- Never commit secrets, credentials, tokens, or raw secret-bearing tool output.
- Use conventional commit subjects such as `type(scope): summary`.

## Documentation and decisions

- Record decisions that are costly to reverse or cross component boundaries as numbered ADRs in `docs/adr/` following the index process there.
- Keep stable product facts in `docs/memory/project.md`, present-tense work state in `docs/memory/active.md`, and milestone status in `docs/memory/progress.md`.
- Link to an authoritative document instead of copying it.

## Reporting issues

Open a GitHub issue with the observed behavior, the expected behavior, and reproduction steps including platform and command output.
For security-sensitive findings, do not include secrets or live provider payloads; describe the boundary involved instead.
