# ADR-0011: Use event-sourced session lifecycle with archive guards and export-before-delete

**Status:** Accepted

**Date:** 2026-08-22

**Owners:** Project maintainers

## Context and problem statement

Phase 3.1 delivered durable multi-session persistence, but a session could
only be created and selected. Users could not rename sessions to keep the
browser readable, retire finished work without losing it, or remove unwanted
sessions at all.
The terminal had no way to browse every durable session while working inside
one of them.
Deleting durable history is destructive and irreversible, so deletion needed
an explicit contract before any UI exposed it.

## Decision drivers

- Every observable state change must remain an authoritative event so strict replay reconstructs identical aggregates.
- Archived history stays queryable and openable; only mutation stops.
- An active provider attempt must never be silently abandoned by a session switch.
- Deletion must be atomic and must leave a readable archive behind.
- The browser must stay usable on narrow terminals and keyboard-only flows.

## Considered options

1. Soft-delete flag on the projection row only: cheap, but the authoritative event stream would diverge from visible state and replay could resurrect deleted sessions.
2. Hard delete without export: simplest storage story, but destroys the only copy of user history with no recourse.
3. Event-sourced lifecycle (rename, archive, unarchive events) plus atomic export-then-delete: keeps one source of truth, makes archive state replayable, and pairs every destructive action with an archival artifact.

## Decision outcome

Chosen option: **Event-sourced lifecycle with archive guards and export-before-delete**, because it preserves strict replay as the single source of truth, gives archived sessions precise semantics (openable and readable, but no ordinary commands), blocks switching away from unsettled provider work instead of guessing, and satisfies the pre-deletion export exit criterion without inventing a second persistence layer.

### Semantics fixed by this decision

- `SessionTitle` is a validated value type: non-empty, bounded length, no control characters, so titles render safely in one-line terminal rows.
- Rename, archive, and unarchive are ordinary commands and events. Duplicate transitions (archiving an archived session, unarchiving an active session) are conflicts, not silent successes.
- An archived session accepts only `UnarchiveSession`; every other command is rejected with a typed conflict naming the session.
- Opening another session replays that session's authoritative history into the coordinator before any projection swap, and switching is refused while an attempt or permission prompt is active.
- Deletion requires the caller to pass the expected last sequence; a mismatch fails closed with a version conflict.
- Export serializes the full schema-v1 event stream plus the projection summary to JSON beside the database, atomically, before any row is deleted; export failure aborts deletion.

## Consequences

### Positive

- Replay equivalence holds for the whole lifecycle: restarts restore titles and archive state exactly.
- Users can clean up sessions without fear because every delete leaves a complete JSON archive.
- The browser overlay can mark active and archived rows deterministically from projection data alone.
- Guards surface as typed errors that the terminal renders as actionable notices.

### Negative

- Deletion now depends on filesystem writability beside the database, which can block deletion on read-only volumes; users must free space or move the database.
- Archive state adds one guard branch to every future command decision.
- The export format is a compatibility surface documented in [SESSION_EXPORT](../architecture/SESSION_EXPORT.md) and pinned by tests.

## Compliance

- Strict replay remains the only state derivation path; see ADR-0002 for repository-native memory and ADR-0004 for the interaction schema these events extend.
- Branching and promotion follow ADR-0003.
