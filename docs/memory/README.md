# Repository memory protocol

Repository memory is the durable handoff between human and agent sessions. It is intentionally small, plain Markdown, and version controlled.

This is development memory for the AutoHarness repository. The product's runtime memory design is documented separately in [persistent memory architecture](../architecture/PERSISTENT_MEMORY.md).

## Core memory

Read these files at the start of every task:

1. [Project](project.md): stable purpose, constraints, and accepted direction.
2. [Active](active.md): current objective, recent material changes, blockers, and immediate next actions.
3. [Progress](progress.md): verified milestone and capability status.

These files must remain small enough to read together. Use [the documentation map](../README.md) to load deeper material only when relevant.

## What belongs where

| Information | File |
| --- | --- |
| Stable vision, product boundaries, and durable constraints | `project.md` |
| Present work, blockers, open questions, next actions | `active.md` |
| Verified milestone/capability state and known gaps | `progress.md` |
| Delivery sequencing and exit criteria | `../PROJECT_PLAN.md` |
| Current technical contract | `../architecture/` |
| Decision rationale and alternatives | `../adr/` |
| Detailed unfinished-task continuation | `handoffs/` |
| Historical patches and authorship | Git history |

## Start-of-task protocol

1. Read the three core files.
2. Compare active memory with the actual repository before assuming it is current.
3. Read only the architecture, plan, ADR, or handoff documents relevant to the request.
4. Treat Proposed ADRs and speculative notes as proposals, not implemented facts.
5. If memory contradicts the repository, trust verified code/tests for implementation state and repair the memory in the same task.

## End-of-task protocol

For a material change:

1. Update `active.md` with the new current objective, blockers, and next actions.
2. Update `progress.md` only for capability or milestone changes verified by files, tests, or observable behavior.
3. Promote newly stable product facts to `project.md`.
4. Update architecture documents when contracts or invariants changed.
5. Add or supersede an ADR when a significant decision changed.
6. Create a detailed handoff only if work remains incomplete and the core files cannot capture the necessary continuation concisely.
7. Remove resolved details from active memory instead of accumulating a chronological diary.

Documentation-only edits that do not change project state do not require mechanical timestamp churn.

## Writing rules

- Use present tense for current state and past tense only for concise, relevant outcomes.
- Use ISO dates.
- Link to files, tests, issues, ADRs, or external evidence.
- Distinguish `Proposed`, `Accepted`, `In progress`, `Blocked`, and `Verified`.
- Record outcomes, not hidden reasoning or long conversation summaries.
- Do not duplicate the same fact in multiple memory files.
- Never record secrets, credential values, private endpoints, personal information, or raw sensitive output.

## Staleness handling

`active.md` and `progress.md` include a last-reviewed date. A date is a review signal, not proof. Any agent changing implementation must reconcile these files with the diff and verification results before finishing.

If a handoff is stale:

- Promote still-valid decisions to an ADR or architecture document.
- Move current next steps to `active.md`.
- Delete the obsolete handoff in the same reviewed change; Git retains its history.
