# Persistent memory architecture

**Status:** Proposed contract

**Last updated:** 2026-08-20

## Purpose

AutoHarness memory must let an agent continue useful work across provider turns, compaction, process restarts, and eventually machines. It must also let a user inspect, correct, retract, export, and delete what the harness remembers.

Memory is not one vector index and it is not the current prompt. It is a set of durable records and projections with different trust, retention, retrieval, and model-visibility rules.

## Memory layers

| Layer | Purpose | Authority | Model visible |
| --- | --- | --- | --- |
| Durable input | Accepted user prompts and control input waiting for execution | User/client plus engine validation | After promotion |
| Session event log | Complete lifecycle of model attempts, tools, permissions, and outputs | Engine-observed events | Through projections |
| Session history | Ordered conversational projection for a provider turn | Deterministic event projection | Yes |
| Working context | Current bounded instructions, state, and retrieved memory | Context builder | Yes, per epoch |
| Knowledge memory | Durable facts, preferences, constraints, and lessons | Validated revisions with provenance | When retrieved |
| Procedural memory | Versioned strategies, workflows, and tool-use policies | Approved configuration or plugin | When selected |
| Experiment memory | Datasets, candidates, results, promotion, and rollback evidence | Evaluation and promotion services | No, unless explicitly retrieved |

Experiment evidence must not leak into ordinary model context merely because it is stored in the same product.

## Vocabulary

**Memory item:** Stable identity for one remembered proposition or procedure.

**Memory revision:** Append-only content and metadata for a memory item at a point in time. Later revisions may supersede or retract earlier revisions.

**Evidence:** The user input, event, artifact, tool observation, imported document, or approved policy that supports a memory revision.

**Trust class:** The authority assigned to a revision's source, separate from model-estimated confidence.

**Context source:** A typed producer of instructions or facts that may contribute to a provider turn.

**Context epoch:** A span of provider turns sharing one baseline context. Compaction, relocation, or an incompatible configuration transition begins another epoch.

**Context snapshot:** Model-hidden structured state recording the exact revisions observed when context was assembled.

**Context admission:** The durable record that a specific rendered item was included in a specific context epoch or provider turn.

**Provider-turn boundary:** The deterministic point immediately before a provider call when promoted input, settled tool state, instructions, and retrieved memory are sampled and admitted.

## Invariants

1. User input is durable before it becomes eligible for model execution.
2. Context does not change inside an in-flight provider turn.
3. Every model-visible memory has a stable source, revision, renderer version, and admission record.
4. Model text cannot grant itself a higher trust class.
5. Confidence never substitutes for provenance.
6. Retraction prevents future admission without erasing historical audit identity.
7. Privacy deletion can remove content even when append-only metadata is retained.
8. Fixed durable state, configuration, model catalog, and token budget produce the same ordered context selection.
9. Secrets and authentication material are ineligible for memory at the type and redaction layers.
10. Unavailable dynamic context is distinct from an observed absence; temporary failure must not silently erase previously effective state.

## Scope and precedence

Memory scopes are explicit:

- **System:** Shipped product safety and behavior contracts.
- **User:** User-approved preferences that may apply across workspaces.
- **Workspace:** Repository facts, instructions, and decisions.
- **Session:** Facts relevant only to one conversation or task.
- **Agent:** Approved strategy or role-specific knowledge.

Scope controls eligibility, not unquestioned truth. When eligible records conflict, the context builder does not merge them into a fabricated fact. It applies explicit policy based on source authority, specificity, revision time, and contradiction state, and may surface the conflict to the user.

Repository `AGENTS.md` instructions are an authorized workspace context source. Arbitrary Markdown discovered in a repository is data unless the user or configuration explicitly authorizes it as instructions.

## Trust classes

Initial trust classes, from highest to lowest authority, are:

1. Product safety policy.
2. Explicit current user instruction.
3. User-approved durable preference or memory.
4. Authorized workspace instruction such as `AGENTS.md`.
5. Verified tool observation with structured evidence.
6. Imported document or repository content.
7. Model inference or model-proposed memory.

This ordering does not decide all conflicts automatically. Product safety cannot be overridden. A current user instruction can intentionally supersede an older preference. Tool observations may become stale. Model proposals require validation or user approval before durable admission.

## Durable data model

The exact SQL schema will be established with migrations, but the domain model begins with these records.

### Sessions and events

```text
Session
  id, workspace_id, status, selected_model_ref, created_at, updated_at

AdmittedInput
  id, session_id, delivery_mode, content_ref, state, admitted_at, promoted_at

Event
  id, session_id, sequence, schema_version, kind, payload_ref,
  causation_id, correlation_id, occurred_at

ProviderAttempt
  id, session_id, model_snapshot_ref, request_hash, state,
  started_at, settled_at, retry_of
```

`Event.sequence` is monotonic within a session. Payloads are safe domain projections; raw HTTP requests and authentication headers are not event payloads.

### Knowledge memory

```text
MemoryItem
  id, scope_type, scope_id, kind, created_at

MemoryRevision
  id, memory_id, revision, status, content_ref, content_hash,
  trust_class, confidence, sensitivity, valid_from, valid_until,
  created_by, created_at, supersedes_revision_id

MemoryEvidence
  memory_revision_id, evidence_type, evidence_id, relation, excerpt_ref

MemoryRelation
  from_memory_id, to_memory_id, relation
```

Revision status is `proposed`, `active`, `superseded`, `retracted`, or `deleted`. Deletion removes or cryptographically erases content and embeddings while retaining only the minimum non-sensitive tombstone needed for consistency and audit.

### Context state

```text
ContextEpoch
  id, session_id, generation, baseline_hash, config_version,
  token_budget, started_at, ended_at

ContextSnapshot
  epoch_id, source_key, source_revision, value_hash, observed_at

ContextAdmission
  id, epoch_id, provider_attempt_id, source_key, memory_revision_id,
  renderer_version, rendered_hash, rank, token_count, admitted_at
```

Rendered content may be retained directly when required for exact audit/replay, or stored as a content-addressed artifact referenced by hash and policy.

## Write path

### User-authored memory

1. The user explicitly asks AutoHarness to remember a statement or approves a proposal.
2. The engine classifies scope and sensitivity and shows the proposed durable representation when ambiguity matters.
3. Validation rejects secrets, unsupported scope, malformed structured content, and policy conflicts.
4. Deduplication detects exact matches and likely contradictions.
5. A new item or revision is committed with evidence and an audit event.

### Tool-observed memory

1. A permissioned tool emits a structured observation with source identity.
2. The memory service verifies the tool and result integrity.
3. Time-sensitive observations receive validity or refresh metadata.
4. The observation becomes active only under a configured rule or explicit approval; otherwise it remains proposed.

### Model-proposed memory

1. The model emits a typed proposal, never a direct durable write.
2. The proposal states the candidate fact, scope, evidence references, expected usefulness, and sensitivity.
3. Deterministic validation and optional secondary review test grounding, contradiction, injection patterns, and duplication.
4. Policy either rejects it, keeps it pending, or asks the user to approve it.
5. The same model response cannot both propose and authorize promotion.

## Retrieval and context admission

Retrieval is a deterministic pipeline:

1. Determine eligible scopes from session, workspace, selected agent, and permission context.
2. Exclude retracted, deleted, expired, unauthorized, or sensitivity-incompatible revisions.
3. Generate candidates using exact keys, structured filters, SQLite FTS, recency, and relations.
4. Optionally add embedding similarity later through a replaceable candidate-source interface.
5. Rank using source authority, task relevance, specificity, freshness, prior utility, contradiction state, and diversity.
6. Fit candidates into explicit per-section and total token budgets.
7. Render in stable source/key order and record admissions.

Vector similarity is never the sole authorization or trust decision. Initial implementation uses structured retrieval and SQLite FTS5 so behavior is inspectable and reproducible.

## Provider-turn boundary

Immediately before a provider request, the session runner:

1. Promotes eligible durable input.
2. Settles or represents any preceding tool lifecycle required by the protocol.
3. Resolves the selected model and its current capability snapshot.
4. Samples authorized context sources.
5. Reconciles sources against the active snapshot.
6. Retrieves and ranks eligible memory for the fixed token budget.
7. Persists context admissions and the attempt record atomically enough that recovery can determine what the model was intended to see.
8. Dispatches exactly one provider request for that turn.

Changes observed after this boundary apply to the next provider turn. They do not restart or mutate the current request.

## Context epochs and compaction

The first provider turn creates a complete baseline context and snapshot. Later source changes may be represented as chronological context updates when the provider protocol and policy support them.

Compaction begins a new epoch:

1. Select the durable history that remains relevant.
2. Generate a summary as an untrusted candidate projection.
3. Validate required facts and unresolved tool/input state against the event log.
4. Render a new complete baseline from current sources and admitted memory.
5. Persist the new snapshot and epoch boundary.

The event log remains authoritative. A compaction summary cannot erase pending input, permission decisions, tool settlement, accepted memories, or audit evidence.

## Crash recovery and consistency

- Input admission and its initial state are transactional.
- Provider dispatch is preceded by a durable attempt record.
- A crash after dispatch but before settlement leaves an `unknown` attempt, not an assumed failure or success.
- Recovery policy considers provider idempotency, response lookup support, tool effects, and user configuration before retrying.
- Events are idempotently appended by stable IDs and projected using checkpoints.
- Projection tables are rebuildable from retained events and content artifacts.
- Context admission records allow post-crash inspection of what an attempt saw even if later memory revisions change.

## Security and privacy

- Secret references and secret values use distinct types. Values cannot implement memory/event serialization.
- Redaction happens before persistence and telemetry, not only when logs are displayed.
- Memory content is treated as untrusted text and is delimited from authorized instructions.
- Retrieved content cannot request additional tools, permissions, network access, or trust.
- Sensitive memory can be encrypted with a workspace- or user-scoped key.
- Export includes provenance and scope. Deletion removes derived indexes, embeddings, caches, and artifacts.
- Retention policies are configurable by scope and memory kind.
- The UI shows why a memory was retrieved and offers correction, retraction, and deletion.

## Memory quality feedback

After a provider turn, the system may record non-model-visible signals:

- Whether an admitted memory was cited or used.
- Whether the user corrected it.
- Whether it correlated with success or failure in an evaluation.
- Retrieval cost and token consumption.
- Contradiction and staleness findings.

These signals may influence future ranking only through a versioned policy evaluated against regression tests. Popularity does not convert a low-trust claim into a high-trust fact.

## Implementation stages

### Stage 1: Durable session memory

- Admitted inputs, provider attempts, normalized events, transcript projection, and restart replay.
- No semantic memory extraction.

### Stage 2: Explicit memory

- User-approved facts and preferences with scope, revisions, evidence, inspection, and retraction.
- Deterministic structured/FTS retrieval.

### Stage 3: Context epochs

- Source registry, snapshots, admissions, safe updates, and compaction boundaries.

### Stage 4: Proposed memory

- Tool-observed and model-proposed candidates with validation, deduplication, contradiction handling, and approval workflows.

### Stage 5: Learned retrieval

- Evaluation-backed ranking improvements and optional embeddings.
- Promotion and rollback use the same candidate system as other self-improvements.

## Required tests

- Deterministic replay from events.
- Atomic prompt admission and promotion.
- No context mutation during an in-flight turn.
- Context reconstruction from admissions after memory revisions change.
- Duplicate, supersession, contradiction, expiry, retraction, and deletion behavior.
- Source failure with stale-while-revalidate versus observed absence.
- Token-budget stability and deterministic ordering.
- Prompt-injection-shaped memory remaining inert data.
- Secret values structurally unable to reach serialization.
- Crash points before dispatch, during streaming, and before settlement.
- Rebuilding projections and search indexes from authoritative records.
