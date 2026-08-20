# Detailed handoffs

This directory is for exceptional unfinished-task notes that another session needs to continue safely. It is not a diary and not the primary project backlog.

## Create a handoff when

- Work is incomplete and depends on non-obvious reproduction steps, partial results, or external state.
- The details are too large for `../active.md`.
- The information does not belong in architecture, an ADR, or a tracked issue.

## Do not create a handoff for

- Completed work already visible in the repository and Git history.
- Stable project facts.
- Architectural decisions.
- General future ideas or wishlist items.
- Secrets, private endpoints, raw logs, or personal data.

## Naming and lifecycle

Use `YYYY-MM-DD-short-topic.md`. Include status, objective, exact current state, verification performed, remaining work, blockers, and relevant file links.

When the work completes, promote any durable knowledge to its authoritative document and delete the handoff. Git history preserves it if later forensic context is needed.

There are currently no active detailed handoffs.
