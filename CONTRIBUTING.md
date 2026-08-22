# Contributing to AutoHarness

Thank you for considering a contribution.
AutoHarness accepts code, documentation, architecture decisions, and evidence-backed experiments through the workflow below.

## License

By contributing, you agree that your contributions are licensed under the [MIT License](LICENSE).

## Read first

1. [`AGENTS.md`](AGENTS.md): repository conventions, guardrails, engineering standards, and the memory protocol.
2. [`docs/README.md`](docs/README.md): routes each task to its smallest authoritative document.
3. [`README.md`](README.md): running the terminal application and configuration.

## Branching

- The permanent hierarchy is `main -> dev -> feat/<name>`.
- Create every regular change branch from the latest `dev`.
- Open pull requests against `dev`, never directly against `main`.
- Releases are promoted from `dev` into `main` with a dedicated pull request.
- See [ADR-0003](docs/adr/0003-use-main-dev-feature-branches.md) for the durable decision.

## Required validation

Run these gates from the repository root before opening a pull request:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked --no-deps -- -D warnings
cargo test --workspace --all-targets --all-features --locked --no-fail-fast
```

Pull requests must keep all three green on Linux, Windows, and macOS in CI.

## Engineering expectations

- Add focused tests with implementation changes.
- Provider tests must cover pagination, arbitrarily fragmented streams, cancellation, retries, and secret redaction.
- Keep the headless engine independent of Ratatui, concrete providers, SQLite, and plugin runtimes.
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
