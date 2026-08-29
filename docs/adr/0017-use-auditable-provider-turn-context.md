# ADR-0017: Use auditable provider-turn context manifests

**Status:** Proposed

**Date:** 2026-08-29

**Owners:** Project maintainers

## Context and problem statement

The application currently rebuilds provider history in the coordinator immediately before each request.
The result has no durable context identity, source snapshot, budget record, renderer version, admission reason, or request hash.
The first provider call and tool continuation also use different command ordering.
One provider attempt can contain several run turns around tools, so an admission tied only to an attempt cannot identify which request observed it.

Phase 4 requires deterministic context that survives restart, correction, retraction, compaction, and audit.
A decision is required for the exact durability boundary before provider dispatch.

## Decision drivers

- Context must not mutate inside an in-flight provider turn.
- Every model-visible item must identify its source revision, renderer, reason, budget count, attempt, and run turn.
- Fixed durable state, configuration, catalog snapshot, and budget must produce byte-identical context.
- A crash before dispatch must not fabricate model visibility.
- A crash after dispatch must retain the current conservative unknown-outcome semantics.
- Provider-specific instruction fields must remain adapter concerns.

## Considered options

1. Keep rebuilding context in the coordinator and log a summary after dispatch.
2. Store one mutable context snapshot per session and let every provider call read it.
3. Build and commit one immutable provider-turn manifest, then bind its hash through the session event stream before dispatch.
4. Persist only rendered prompt text without typed sources, admissions, or policy versions.

## Decision outcome

Chosen option: **build and commit one immutable provider-turn manifest before dispatch**, because it makes the model-visible boundary deterministic, inspectable, and recoverable without coupling provider payloads to the engine.

Each top-level attempt starts a context epoch.
Tool continuations remain inside that epoch but receive distinct manifests identified by `(attempt_id, turn)`.
Compaction, relocation, and incompatible builder, source-registry, ranker, renderer, sizer, configuration, catalog, model-capability, or tool-registry versions begin a new epoch.

The manifest records fixed scope identities, source observation states, source revisions, memory generation, configuration and capability hashes, budgets, ordered admissions, typed rank reasons, rendered hashes, sizing counts, and the canonical provider-neutral request hash.
Source observation distinguishes available, retained stale, observed absent, and unavailable states.

Context preparation reads one immutable store snapshot, builds through a pure deterministic service, and commits through an optimistic transaction.
The transaction rejects changes to the memory generation, session sequence, epoch, attempt turn, admitted revision eligibility, validity, sensitivity, hashes, or budgets.
A rejected draft is rebuilt from a fresh snapshot.

The session event stream binds the committed manifest hash before `RunTurnStarted` can make provider dispatch possible.
The same rule applies to the first call and every tool continuation.

Provider-neutral requests carry an explicitly classified context prelude.
Gemini maps it to `system_instruction`, OpenAI-compatible adapters map it to a leading system message, and Codex maps it into its existing developer boundary.
Memory and imported content render as inert length-delimited data, not as authorized instruction text.

## Consequences

### Positive

- Every provider call has an exact durable explanation of what it was intended to see.
- Multi-turn tool attempts no longer collapse several context boundaries into one attempt record.
- Memory changes cannot race into a partially built request.
- Provider adapters retain native protocol excellence without leaking payloads into domain state.
- Compaction and restart can be compared using canonical manifests and durable-facts hashes.

### Negative

- Provider dispatch gains one deterministic build and storage commit before network work begins.
- Context schema and session lifecycle tests must cover two coordinated durable records.
- Version changes intentionally begin new epochs and require migration-compatible readers.
- Conservative UTF-8 byte sizing may underuse model context until a versioned provider tokenizer is added.

### Follow-up

- Pin manifest, snapshot, admission, and binding-event serialization shapes.
- Add crash tests before context commit, after binding, and after run start.
- Add body-shape tests for Gemini, OpenAI-compatible routers, and Codex.
- Add request-hash and shuffled-insertion determinism tests.
- Accept this ADR only after the complete first-call and tool-continuation paths are locally verified.

## Evidence

- [Persistent memory architecture](../architecture/PERSISTENT_MEMORY.md)
- [Phase 4 implementation plan](../design/PERSISTENT_CONTEXT_MEMORY_PLAN.md)
- [Runtime flow](../architecture/OVERVIEW.md#runtime-flow)
- Current request construction in `crates/autoharness-app/src/coordinator.rs`

## Related decisions

- [ADR-0001](0001-use-rust-modular-monolith.md)
- [ADR-0007](0007-use-durable-capability-tool-runtime.md)
- [ADR-0011](0011-use-event-sourced-session-lifecycle.md)
- [ADR-0016](0016-use-typed-tui-presentation-layer.md)
