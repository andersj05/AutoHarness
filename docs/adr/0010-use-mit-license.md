# ADR-0010: Use the MIT License

**Status:** Accepted

**Date:** 2026-08-22

**Owners:** Project maintainers

## Context and problem statement

The repository declares in project memory and the README that AutoHarness will be open source, but no license existed, which legally blocks any third-party use, contribution, or redistribution until resolved.
The gap is tracked as an open question in repository memory and as a known gap in progress memory.
A decision was required before the first public release and before soliciting external contributions.

## Decision drivers

- The project wants broad adoption of the terminal runtime and its documentation system.
- The engine intends to support plugin boundaries later, so permissive terms keep downstream embedding simple.
- The maintainers do not want to require source disclosure for derived agent runtimes.
- The license must be simple to audit, understood by employers and ecosystems, and compatible with common dependency licenses already in use (MIT, Apache-2.0, BSD via the Rust ecosystem).

## Considered options

1. MIT: maximally short and permissive; no explicit patent grant.
2. Apache-2.0: permissive with an explicit patent grant and trademark terms; longer text.
3. Dual `MIT OR Apache-2.0`: the Rust ecosystem default; lets recipients choose; requires maintaining both texts and dual notices everywhere.
4. GPL-3.0-or-later: copyleft; strongest guarantee against closed forks; reduces employer and SaaS adoption.

## Decision outcome

Chosen option: **MIT**, because the primary goals are frictionless adoption, embedding, and contribution, the codebase forbids unsafe code and ships no patent-encumbered algorithms that would make an explicit grant material today, and the single short text minimizes compliance cost across ten crates and generated metadata.

## Consequences

### Positive

- Any individual or organization may use, embed, modify, and redistribute AutoHarness with minimal obligations.
- The license question in repository memory is resolved and the README can link a concrete license.
- Cargo manifests can declare `license = "MIT"` so registry and tooling audits resolve cleanly.

### Negative

- No express patent defense is granted to users, unlike Apache-2.0.
- Closed-source forks of the runtime are permitted.

### Follow-up

- Add the `LICENSE` file, a `CONTRIBUTING.md`, and the workspace-level `license` field in the same change.
- Revisit only if the project later accepts contributions carrying patent commitments, in which case supersede this record rather than editing it.

## Evidence

- Maintainer decision recorded on 2026-08-22 selecting MIT from the considered options.
- Open-source intention recorded in `docs/memory/project.md`.

## Related decisions

- [ADR-0001](0001-use-rust-modular-monolith.md) for the Rust technology basis whose ecosystem licenses informed compatibility analysis.
