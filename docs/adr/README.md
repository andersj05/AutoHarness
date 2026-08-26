# Architecture decision records

Architecture Decision Records preserve decisions that are expensive to reverse, affect multiple components, or constrain future work. They explain why the repository has its current shape; architecture documents explain the current shape itself.

AutoHarness uses a compact Markdown ADR format based on MADR conventions.

## Status values

- **Proposed:** Under consideration; implementation must not assume acceptance.
- **Accepted:** Current decision and default for new work.
- **Deprecated:** Still present but should not be extended.
- **Superseded by ADR-NNNN:** Replaced; retained for history.
- **Rejected:** Considered and explicitly not selected.

## Process

1. Copy `0000-template.md` to the next four-digit number with a short kebab-case title.
2. Describe one decision, its drivers, options, outcome, and consequences.
3. Link evidence and related ADRs or architecture documents.
4. Mark the record Proposed until the decision is accepted.
5. Once Accepted, do not rewrite its historical outcome. Add clarifications with dates or supersede it with another ADR.
6. Update this index in the same change.

## Index

| ADR | Status | Decision |
| --- | --- | --- |
| [0001](0001-use-rust-modular-monolith.md) | Accepted | Use Rust and a modular-monolith architecture |
| [0002](0002-use-repository-native-memory.md) | Accepted | Use repository-native layered memory and ADRs |
| [0003](0003-use-main-dev-feature-branches.md) | Accepted | Use main, dev, and short-lived feature branches |
| [0004](0004-use-gemini-interactions-v1.md) | Accepted | Use Gemini Interactions v1 for the default stream |
| [0005](0005-use-ephemeral-in-app-credentials.md) | Accepted | Accept provider credentials through an ephemeral in-app overlay |
| [0006](0006-use-openai-compatible-router-boundary.md) | Accepted | Use an OpenAI-compatible router boundary with shared provider policy and caching |
| [0007](0007-use-durable-capability-tool-runtime.md) | Accepted | Use a durable capability-based tool runtime |
| [0008](0008-defer-modeled-cost-authority.md) | Accepted | Defer modeled cost authority until trusted pricing exists |
| [0009](0009-use-os-backed-provider-credential-profiles.md) | Accepted | Store opted-in provider credentials in the operating-system credential vault and retain only opaque profile references |
| [0010](0010-use-mit-license.md) | Accepted | Use the MIT License |
| [0011](0011-use-event-sourced-session-lifecycle.md) | Accepted | Use event-sourced session lifecycle with archive guards and export-before-delete |
| [0012](0012-use-typed-settings-resolver.md) | Accepted | Use a versioned typed settings resolver with layered precedence |
| [0013](0013-use-durable-credential-mutation-recovery.md) | Accepted | Use durable non-secret recovery records for cross-system profile and credential mutations |
| [0014](0014-use-codex-cli-subscription-boundary.md) | Accepted | Use the official Codex CLI subscription boundary |

## When an ADR is not needed

Do not create an ADR for a reversible local refactor, routine dependency update, bug fix that restores documented behavior, or status update. Put current contracts in architecture docs and current work state in repository memory.
