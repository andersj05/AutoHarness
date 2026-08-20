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

## When an ADR is not needed

Do not create an ADR for a reversible local refactor, routine dependency update, bug fix that restores documented behavior, or status update. Put current contracts in architecture docs and current work state in repository memory.
