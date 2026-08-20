# ADR-0003: Use main, dev, and short-lived feature branches

**Status:** Accepted

**Date:** 2026-08-20

**Owners:** Project maintainers

## Context and problem statement

AutoHarness needs a predictable integration and release path before implementation begins.
The repository currently has a stable `main` branch and short-lived agent work, but it needs a long-lived integration branch so incomplete features do not accumulate directly on the release branch.

## Decision drivers

- Keep the release branch stable and reviewable.
- Give concurrent feature work one consistent integration target.
- Make branch ancestry and pull-request targets obvious to humans and agents.
- Support staged validation before release promotion.
- Keep the workflow simple enough for a small project while remaining scalable.

## Considered options

1. Trunk-based development directly on `main`.
2. `main` plus short-lived feature branches.
3. `main -> dev -> short-lived feature branches`.
4. A larger Git Flow model with permanent release and hotfix branch families.

## Decision outcome

Chosen option: **`main -> dev -> short-lived feature branches`**.

`main` is always release-ready.
`dev` is the long-lived integration branch and begins from `main`.
Regular feature, fix, documentation, and refactor branches begin from the latest `dev` and merge back into `dev` through pull requests.
Validated releases move from `dev` to `main` through a dedicated promotion pull request.

Branch names use a short namespace and kebab-case topic.
Examples include `feature/gemini-provider`, `fix/stream-cancellation`, and an agent-required namespace such as `codex/repository-foundation`.

Emergency production hotfixes may branch from `main`.
After merging a hotfix into `main`, the same change must be reconciled immediately into `dev` so the branches do not diverge.

## Consequences

### Positive

- `main` remains a clear release and rollback reference.
- `dev` provides a stable integration target for concurrent work.
- Feature branches stay short-lived and isolated.
- Pull-request intent is explicit: integration targets `dev`, and release promotion targets `main`.

### Negative

- `dev` can accumulate instability if validation is weak.
- Changes wait for an additional promotion step before reaching `main`.
- Hotfixes require deliberate reconciliation into `dev`.
- Repository branch protections must be configured to enforce the documented policy rather than relying only on convention.

### Follow-up

- Base all new regular work on `dev`.
- Configure GitHub branch protections for `main` and `dev` when the required checks and maintainer policy are defined.
- Require the future continuous-integration suite on feature-to-`dev` and `dev`-to-`main` pull requests.
- Delete feature branches after merge.

## Evidence

- [Remote main branch](https://github.com/andersj05/AutoHarness/tree/main)
- [Remote dev branch](https://github.com/andersj05/AutoHarness/tree/dev)

## Related decisions

- [ADR-0002](0002-use-repository-native-memory.md)
