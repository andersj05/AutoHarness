# Session and memory export formats

**Status:** Phase 4 implementation contract

**Last updated:** 2026-08-29

AutoHarness writes a provider-neutral JSON archive before destructive session deletion.
Schema v2 extends the complete session event stream with the exact durable context audit records and current session-scoped memory inspection records that still exist at export time.
AutoHarness can also export one explicitly authorized memory item through a separate schema-v1 artifact.

## File names and atomicity

Session exports use this shape:

```text
autoharness-session-{session_id}.export.v2-{unique_suffix}.json
```

Standalone memory exports use this shape:

```text
autoharness-memory-{memory_id}.export.v1-{unique_suffix}.json
```

The exporter replaces characters outside ASCII letters, digits, `-`, and `_` in the identity portion and limits that portion to 96 characters.
The unique suffix prevents one export from overwriting another export.
The exporter writes a temporary sibling file and renames it into place.
A failed export removes the temporary file and never leaves a truncated destination.

The unique file name is intentionally nondeterministic.
The JSON bytes are deterministic for a fixed durable database state.

## Session export schema v2

The top-level document has this shape:

```json
{
  "schema_version": 2,
  "session": {
    "session_id": "session-...",
    "status": "active",
    "title": null,
    "selected_provider_id": "google-ai-studio",
    "selected_model_id": "models/gemini-2.5-pro",
    "created_at_ms": 1724000000000,
    "updated_at_ms": 1724000100000
  },
  "event_count": 8,
  "events": [],
  "context_audit": {
    "epoch_count": 1,
    "turn_count": 1,
    "epochs": [],
    "turns": []
  },
  "session_memories": []
}
```

### Session and events

`session` is the durable session projection at export time.
`title` is `null` when the session has never been renamed.
The selected provider and model are `null` until a model has been selected.

`events` is the complete ordered schema-v1 event stream for the session.
`event_count` equals `events.len()`.
Event envelopes use the serialized shape pinned by `crates/autoharness-domain/tests/serialization.rs`.
Strict replay of `events` reconstructs the session aggregate without a provider or the original SQLite database.

### Context audit

`context_audit.epochs` contains each distinct context epoch referenced by an exported `ContextTurnBound` event.
Epochs are ordered by their typed epoch identity so a fixed database state produces stable bytes.
`epoch_count` equals `epochs.len()`.

`context_audit.turns` follows the authoritative `ContextTurnBound` event order.
`turn_count` equals `turns.len()`.
Each turn has this shape:

```json
{
  "manifest": {
    "context_turn_id": "context-turn-...",
    "epoch_id": "epoch-...",
    "attempt_id": "attempt-...",
    "run_turn": 1,
    "manifest_hash": "...",
    "sources": [],
    "admissions": []
  },
  "rendered_prelude_state": "retained",
  "rendered_prelude": "AutoHarness context v1...",
  "admissions": [
    {
      "manifest": {
        "admission_id": "admission-...",
        "memory_revision_id": "revision-...",
        "rank": 1,
        "rank_score": 500,
        "reasons": []
      },
      "rendered_content_state": "retained",
      "rendered_content": "<autoharness-memory-data-v1>..."
    }
  ]
}
```

The manifest objects are the exact domain records, including frozen eligibility, budgets, versions, hashes, source observations, admission ranks, and admission reason factors.
The optional rendered fields are erasable sidecars rather than authorities.
`retained` means the exact hash-verified sidecar was available to the exporter.
`unavailable` means no sidecar was available.
For an admission that was originally committed with bytes, `unavailable` records a later privacy erasure.
For a turn prelude, `unavailable` can mean either that the turn had no prelude or that a later privacy action erased it.
The current read port does not distinguish those two prelude cases.

Deleting a memory revision erases its retained admission rendering and every complete turn prelude that incorporated the memory.
The admission and turn manifests remain as contentless audit records.

The store does not retain an independent content sidecar for a source snapshot merely because the source was observed.
An admitted source has an exact rendered admission sidecar while it remains retained.
A source that was observed but not admitted has only its typed source key, observation state, revision hash, value hash, and observation time.

### Session-scoped memories

`session_memories` contains current inspection records for every memory item whose exact scope is the exported session.
The list includes active, proposed, rejected, retracted, and deleted lifecycles when those states exist.
Each item includes its stable identity, scope, kind, current lifecycle, item sequence and timestamps, immutable revision metadata, retained revision content, and provider-context admission history.

One revision has this shape:

```json
{
  "metadata": {
    "status": "active",
    "revision_id": "revision-...",
    "revision": 1,
    "content_hash": "...",
    "origin": "explicit_user",
    "trust_class": "user_approved",
    "sensitivity": "internal",
    "evidence": [],
    "relations": [],
    "created_at": 1724000000000
  },
  "content_state": "retained",
  "content": "Prefer concise status updates."
}
```

`content_state` has these values:

| Value | Meaning |
| --- | --- |
| `retained` | Exact hash-verified revision content is present in `content`. |
| `unavailable` | Revision content has been logically erased and `content` is `null`. |
| `redacted_by_policy` | Metadata labels the revision as secret and the exporter forces `content` to `null`. |

New writes reject secret sensitivity before persistence, so `redacted_by_policy` is a defense-in-depth export state rather than an ordinary Phase 4 lifecycle.
Retraction preserves revision content for audit while preventing future admission.
Deletion preserves contentless metadata while removing revision content, evidence excerpt sidecars, search rows, and retained provider renderings.

Memory admission rows identify the exact revision, context turn, epoch, session, attempt, run turn, model, timestamp, rank, score, token count, renderer version, and ordered reason factors.
Their `rendered_content_state` and `rendered_content` fields use the same retained versus unavailable convention as context audit admissions.

Session export intentionally excludes user-, workspace-, and agent-scoped memories even when their evidence references the session.
Those records require a separate export under their own exact scope authority.
The session memory section is an inspection snapshot and not a second replayable memory ledger because it does not duplicate memory operations.

## Standalone memory export schema v1

`export_memory` requires the caller to supply an authorized scope equal to the scope in the memory's creation operation.
An unknown memory or a scope mismatch fails before any output file is written.

The document has this shape:

```json
{
  "schema_version": 1,
  "memory_id": "memory-...",
  "scope": {"kind": "workspace", "id": "workspace-..."},
  "operation_count": 2,
  "operations": [],
  "revisions": [],
  "admissions": []
}
```

`operations` is the complete ordered schema-v1 ledger for the memory item.
`operation_count` equals `operations.len()`.
`revisions` and `admissions` use the same shapes and privacy states as the session export.
The complete operation ledger plus retained sidecars supports lifecycle inspection without the original SQLite database.

## Evidence excerpt limitation

Revision metadata exports typed evidence identities, source references, relations, and expected excerpt hashes.
Exact evidence excerpt sidecars are not exported because the current `MemoryStore` read port exposes revision content but has no authorized, hash-verifying evidence-content read.
The archive therefore cannot distinguish a retained evidence excerpt from an excerpt erased by session deletion.
It also cannot reproduce the exact excerpt bytes from the hash.

The smallest completing API is a bounded `load_memory_evidence_content(revision_id)` read that returns exact evidence identities and hash-verified excerpt sidecars after the caller has authorized the owning memory scope.
An export can then pair each evidence metadata record with `retained` or `unavailable` state without reading SQLite tables directly.
Until that API exists, AutoHarness must not claim that standalone memory export contains exact evidence excerpts or proves their erasure state.

Deleting a session erases session-owned memory content and evidence excerpts on memories in other scopes when their typed evidence source names the deleted session.
The cross-scope memory content and its contentless evidence source reference remain.
A post-deletion standalone export can therefore show the surviving source reference but not the erased excerpt state.

## Security and privacy boundaries

- Export contains user prompts, assistant output, internal or sensitive memory content, exact retained provider context, and durable identifiers.
- Export files must be handled as sensitive local artifacts and are not encrypted by this format.
- Provider credentials and authentication material must be rejected or redacted before durable persistence.
- The exporter does not reinterpret arbitrary session event text as a credential and does not replace the ingress secret gate.
- Exact evidence excerpts are omitted under the current read contract.
- Secret-labeled revision content is never serialized by the exporter.
- Memory scope authorization is checked before a standalone memory file is written.
- Session export includes only exact session-scoped memories and never widens memory scope authority.

An export is an independent copy.
Deleting a session or memory after export does not modify or remove an already-written archive.
Backups, copied exports, SQLite pages, WAL files, and already dispatched provider requests remain separate retention authorities.
Phase 4 deletion is application-level logical deletion and does not claim forensic erasure.

## Boundedness and failure behavior

The exporter reads events, memory operations, memory inspection rows, and admission history through bounded pages.
The complete archive can still grow with the complete durable history of the selected session or memory item.
There is no silent truncation.

If a `ContextTurnBound` event names a missing turn or epoch, export fails closed.
If a retained content sidecar fails its store-level hash checks, export fails closed.
Session deletion proceeds only after the pre-deletion session export succeeds.
