# ADR-0007: Use a durable capability-based tool runtime

**Status:** Accepted

**Date:** 2026-08-21

**Owners:** Project maintainers

**2026-08-22 clarification:** ADR-0008 supersedes the modeled-cost portions of this decision until a trusted durable pricing snapshot exists.

**2026-08-22 clarification:** A provider call that fails strict registered-schema parsing is frozen as a bounded `InvalidToolCall` no-authority proposal, force-denied by policy, and returned as a tool result for bounded model repair.
This remains an execution rejection and cannot be authorized by a permissive rule or forged replay evidence.

## Context and problem statement

Phase 3 must let a model request useful local actions without treating model-authored JSON as authority.
The tool loop must survive restart, preserve the exact permission scope, bound resource use, and avoid replaying an external effect whose outcome is ambiguous.
Gemini and OpenAI-compatible providers expose different function-calling wire formats, but those differences must not enter the engine state machine.

## Decision drivers

- Derive authority from trusted versioned tool definitions instead of model-selected fields.
- Commit the call, capability, and permission result before an external effect can start.
- Preserve deterministic recovery without automatically repeating ambiguous effects.
- Keep filesystem, process, HTTP, and artifact implementations outside the headless engine.
- Bound turns, elapsed time, tokens, modeled cost, output bytes, and concurrent effects.
- Return provider-native call results while retaining one provider-neutral durable history.

## Considered options

1. Add a provider-neutral durable tool lifecycle and execute only typed capabilities authorized for an exact frozen call.
2. Let provider adapters execute function calls directly.
3. Give a model a general shell tool guarded only by prompt instructions.
4. Keep the tool loop in memory and restart the whole attempt after interruption.

## Decision outcome

Chosen option: **add a provider-neutral durable tool lifecycle and execute only typed capabilities authorized for an exact frozen call**.

The registered schema version, tool name, provider call identity, bounded arguments, and trusted derived capability become one immutable `ToolCallSpec`.
Strict trusted parsers reject unknown fields, path traversal, unsupported HTTP methods and origins, shell programs, and any schema or argument drift during recovery.
A rejected provider call still crosses the durable proposal and denial lifecycle with a no-authority capability so the model can receive a deterministic correction without terminating the session.
A model cannot name a capability directly.

The permission policy evaluates the exact tool, capability class, and canonical resource and returns deny, ask, or allow.
Unmatched policy entries deny by default.
The local policy allows workspace-confined reads and asks before workspace writes, direct process execution, or HTTP requests.
A human allow answer grants one execution of only the frozen call.

The engine persists proposed, permission-recorded, permission-answered, started, completed, failed, denied, cancelled, and unknown states.
Execution receives an unforgeable process-local authorization value only after replayed durable state proves the matching policy and optional human answer.
The `started` event commits before any adapter call.
Recovery marks a previously started effect unknown, preserves unanswered asks, settles safe pre-effect states without execution, and resumes a paused provider turn only after every call is settled.

Filesystem paths are confined to one configured workspace root.
Processes receive a direct executable and argument vector without an implicit shell or inherited parent environment.
HTTP requests use an exact admitted origin, ignore ambient proxy configuration, and do not follow redirects.
All capability adapters enforce cancellation and hard byte limits.
Large output is truncated for the next model turn and retained by verified content hash in the application artifact directory.

One immutable run budget is committed before dispatch.
It bounds provider turns, elapsed time, cumulative reported tokens, modeled cost, provider and tool output, and concurrent tool effects.
Recovery reconstructs durable counters and elapsed wall time instead of resetting the budget.

## Consequences

### Positive

- Every external effect has a durable call, derived capability, and permission decision.
- Provider-specific function-calling payloads remain inside adapters.
- Permission UI can identify the exact tool and canonical resource before execution.
- Restart never invents success or silently replays an ambiguous effect.
- Capability adapters can later move into stronger operating-system or Wasmtime isolation without changing engine semantics.

### Negative

- Tool execution adds provider turns and durable events to a single logical attempt.
- Started non-idempotent effects may require human reconciliation because recovery records unknown instead of retrying.
- The initial local capability confinement is application-enforced rather than a separate process or Wasmtime boundary.
- Modeled cost remains zero until a trusted pricing source is introduced.

### Follow-up

- Add provider pricing snapshots before nonzero modeled cost can be charged against a run.
- Add operating-system or Wasmtime isolation when untrusted third-party tools are introduced.
- Add per-tool configuration only through versioned trusted registry entries and scoped policy rules.

## Evidence

- [Phase 3 plan](../PROJECT_PLAN.md#phase-3-safe-agent-execution)
- [Architecture tool execution flow](../architecture/OVERVIEW.md#tool-execution)
- Domain serialization, engine lifecycle, runtime capability, provider fragmentation, permission UI, composed execution, SQLite reopen, and recovery tests in the Phase 3 implementation.

## Related decisions

- [ADR-0001](0001-use-rust-modular-monolith.md)
- [ADR-0004](0004-use-gemini-interactions-v1.md)
- [ADR-0006](0006-use-openai-compatible-router-boundary.md)
