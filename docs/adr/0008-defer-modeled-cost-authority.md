# ADR-0008: Defer modeled cost authority until trusted pricing exists

**Status:** Accepted

**Date:** 2026-08-22

**Owners:** Project maintainers

## Context and problem statement

ADR-0007 reserved a nonzero modeled-cost ceiling while the runtime charged every provider turn as zero.
That field appeared to constrain monetary authority but could never stop a run.
Recovery also had no durable cost input to reconstruct.
Token, turn, time, output, and concurrency bounds remain enforceable from trusted local state, but provider pricing does not.

## Decision drivers

- Do not expose a security or spending control that production code cannot enforce.
- Keep every run-budget dimension recoverable from authoritative durable inputs.
- Avoid embedding mutable provider prices or guessed rates in the headless domain.
- Preserve a clear path for future price-aware routing and cost governance.

## Considered options

1. Retain the cost field and continue charging zero until pricing arrives.
2. Estimate cost from a hard-coded model-price table.
3. Remove modeled cost from the Phase 3 run authority and add it only with a trusted durable pricing snapshot.

## Decision outcome

Chosen option: **remove modeled cost from Phase 3 run authority until a trusted durable pricing snapshot exists**.

`RunLimits` and `RunBudget` bound only provider turns, elapsed time, cumulative reported tokens, provider and tool output bytes, and concurrent tool effects.
The runtime must not claim a monetary ceiling until each charged usage dimension has a trusted price source, version, effective interval, model identity, and durable provenance.
Future cost enforcement must record cumulative modeled cost durably and reconstruct it exactly after restart.

## Consequences

### Positive

- Every advertised Phase 3 limit is enforceable in production and reconstructable after restart.
- Operators are not given false assurance that a monetary ceiling will stop a run.
- Future pricing can be introduced as an explicit versioned contract instead of hidden process configuration.

### Negative

- Phase 3 does not provide a direct monetary run limit.
- Token limits remain the closest provider-consumption guard until trusted pricing exists.

### Follow-up

- Define a provider-neutral, versioned pricing snapshot with provenance and effective dates.
- Decide how cached input, reasoning tokens, tool-use tokens, and provider-specific billing dimensions contribute to modeled cost.
- Add durable cross-restart cost-limit tests before exposing a monetary ceiling.

## Evidence

- The Phase 3 security audit found that `RunBudget::record_cost` had no production caller and recovery restored cost as zero.
- [Architecture tool execution flow](../architecture/OVERVIEW.md#tool-execution)
- [Phase 3 plan](../PROJECT_PLAN.md#phase-3-safe-agent-execution)

## Related decisions

- [ADR-0007](0007-use-durable-capability-tool-runtime.md)

