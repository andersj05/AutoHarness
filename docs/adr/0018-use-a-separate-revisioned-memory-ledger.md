# ADR-0018: Use a separate revisioned memory ledger

**Status:** Proposed

**Date:** 2026-08-29

**Owners:** Project maintainers

## Context and problem statement

The current durable event envelope is routed through one session and one session-local sequence.
Phase 4 adds user-, workspace-, session-, and agent-scoped memory that can outlive or apply across sessions.
Attaching a workspace or user memory mutation to the session that happened to initiate it would make ownership, replay, deletion, and concurrency depend on an incidental conversation.

Model, tool, import, and compaction output also need a proposal path that cannot upgrade its own authority.
Privacy deletion must remove content and derived model-visible copies while preserving enough non-content identity to keep audit references consistent.

## Decision drivers

- Cross-scope memory needs one authoritative identity and revision history independent of sessions.
- Model-authored content must never grant itself active or trusted status.
- Optimistic concurrency and idempotent retry must match the existing session engine quality.
- FTS and projections must be rebuildable from authoritative operations.
- Retraction and deletion have different audit and privacy consequences.
- Session deletion must not cascade into unrelated user or workspace memory.
- Raw workspace paths, display labels, and provider profile names are not scope authority.

## Considered options

1. Store every memory mutation as an event in the initiating session.
2. Use mutable memory rows plus an ordinary updated timestamp and no authoritative ledger.
3. Create one hidden global session that owns every memory event.
4. Use a separate event-sourced memory ledger with per-item sequences, opaque scope identities, erasable content sidecars, and a global eligibility generation.

## Decision outcome

Chosen option: **use a separate event-sourced memory ledger**, because memory ownership and authority are independent of the conversation that observed or approved it.

Each memory item has one stable `MemoryId`, one optimistic sequence, and contiguous immutable revisions.
The ledger stores bounded non-content operation envelopes and provenance identities.
Exact content and evidence excerpts live in separately hash-verified blobs that deletion can remove.
A global memory generation increments whenever eligibility can change and protects retrieval-to-context commits from mixed snapshots.

Memory scopes use typed `UserId`, `WorkspaceId`, `SessionId`, and `AgentId` values.
The application resolves canonical workspace locations to opaque identities and treats relocation or explicit reassociation as a deliberate operation.

Explicit user memory can become active only through deterministic validation and any required confirmation.
Tool observations, imports, model output, and compaction summaries create proposals by default.
The immutable proposal origin and trust class cannot be mutated upward.
Approval creates a new active user-approved revision through a distinct authority and supersedes the proposal.

Correction requires the expected current revision.
Retraction preserves content and historical admissions but prevents future retrieval.
Deletion appends a tombstone, removes content blobs, FTS rows, evidence excerpts, embeddings, caches, and retained rendered admission copies, and prevents revival.
Source session events and external backups remain separate authorities and are not silently rewritten.

SQLite FTS5 produces bounded candidates only.
The adapter maintains FTS explicitly inside the same transaction as ledger and projection changes because `trusted_schema=OFF` disables a trigger-based design.
A versioned Rust ranker uses fixed-point features and stable ties before context admission.

Session deletion explicitly exports and removes context rows and applies the configured session-scoped memory tombstone policy.
Cross-scope evidence retains a non-content unavailable-source tombstone instead of blocking deletion or cascading into global memory.

## Consequences

### Positive

- User and workspace memory has one coherent history across sessions.
- Session replay remains focused on session facts and provider-turn context.
- Proposer authority and promotion authority are structurally separate.
- Retraction, deletion, FTS rebuild, and context generation conflicts have explicit semantics.
- The same single storage thread can serialize both ledgers without adding a process or database boundary.

### Negative

- The storage runtime owns two event-sourced aggregate families and coordinated context commits.
- Migration, corruption, rebuild, export, and deletion tests become broader.
- Content sidecars and FTS maintenance add transaction work to memory mutations.
- Plaintext SQLite cannot promise forensic erasure from WAL files, backups, or storage media.

### Follow-up

- Pin the memory command, operation, revision, validation, evidence, and tombstone formats.
- Add failure injection around ledger, projection, content, FTS, and commit boundaries.
- Add explicit v3 migration and rollback evidence.
- Extend the session export format before deleting sessions that own context or session-scoped memory.
- Consider encryption and key erasure in a separate decision before claiming forensic deletion.
- Accept this ADR only after explicit memory, independent approval, rebuild, and deletion paths are locally verified.

## Evidence

- [Persistent memory architecture](../architecture/PERSISTENT_MEMORY.md)
- [Phase 4 implementation plan](../design/PERSISTENT_CONTEXT_MEMORY_PLAN.md)
- [Event-sourced session lifecycle](0011-use-event-sourced-session-lifecycle.md)
- Existing `SessionStore` and SQLite projection architecture in `crates/autoharness-store` and `crates/autoharness-store-sqlite`

## Related decisions

- [ADR-0001](0001-use-rust-modular-monolith.md)
- [ADR-0007](0007-use-durable-capability-tool-runtime.md)
- [ADR-0011](0011-use-event-sourced-session-lifecycle.md)
- [ADR-0012](0012-use-typed-settings-resolver.md)
