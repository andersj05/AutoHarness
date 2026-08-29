# Persistent context and memory implementation plan

**Status:** Implemented and locally validated; release remains gated by the Phase 3.9 and Phase 3.10 evidence.

**Planned:** 2026-08-29

**Implemented:** 2026-08-29

**Authority:** This document defines the ordered Phase 4 implementation slices and their local exit criteria.
The durable runtime contract remains in [PERSISTENT_MEMORY.md](../architecture/PERSISTENT_MEMORY.md).
The delivery outcome remains in [PROJECT_PLAN.md](../PROJECT_PLAN.md#phase-4-persistent-context-and-memory).

## Outcome

Phase 4 turns replayable history into bounded provider context and durable memory that a user can inspect, correct, retract, export, and delete.
The implementation must preserve the existing provider-neutral engine, event-authoritative session store, single storage writer, and typed terminal presentation layer.
No provider call may receive context that cannot be reconstructed from a durable provider-turn manifest.
No model-authored statement may become active memory through the same authority that proposed it.

Phase 4 implementation may proceed on its feature branch while the external Phase 3.9 and Phase 3.10 evidence is collected.
Phase 4 must not be described as released until those earlier promotion gates and the Phase 4 local gates are satisfied on the intended candidate.

## Binding design corrections

The original architecture sketch attached context admissions only to a provider attempt.
One attempt can perform several provider calls around durable tool execution, so every turn manifest and admission must identify both `attempt_id` and the monotonically increasing run `turn`.

User-, workspace-, and agent-scoped memory does not belong to whichever session happened to create it.
Memory therefore uses a separate event-sourced ledger with optimistic per-item sequences and a global eligibility generation.
Session events remain authoritative for context epochs, provider-turn manifests, source snapshots, and admissions.

Scope identity is typed and opaque.
Display labels, provider profile names, and raw workspace paths are never durable scope identifiers.
The local user and built-in agent receive stable application identities.
A canonical workspace locator resolves to an opaque `WorkspaceId`, and relocation or explicit reassociation begins a new context epoch.

SQLite FTS5 is a deterministic candidate source, not the final ranking authority.
FTS maintenance happens explicitly inside Rust-owned transactions because the database runs with `trusted_schema=OFF`.
Final ordering uses versioned integer features and a stable identity tie break in Rust.

Privacy deletion removes memory content, evidence excerpts, FTS rows, retained rendered admission copies, embeddings, and derived caches.
The ledger retains only the minimum non-content tombstone required for consistency and audit identity.
Plaintext SQLite, WAL files, backups, exports, and source session events prevent an honest claim of forensic erasure, so the UI and documentation describe deletion as logical application deletion unless a later encryption decision adds key erasure.

## Runtime ownership

The application keeps one blocking storage thread.
That thread owns the SQLite connection, session engine, memory ledger, context commit checks, FTS queries, projection rebuilds, and deletion transactions.
The TUI emits typed intents and consumes bounded projections only.
The coordinator never issues SQL and never mutates memory or context through an ad hoc connection.

A real `autoharness-memory` crate owns pure policy and algorithms with their first consumers:

- Context-source registration and deterministic observation ordering.
- Bounded source snapshots with available, retained-stale, observed-absent, and unavailable states.
- Memory validation, exact deduplication, structured contradiction candidates, and eligibility filtering.
- Replaceable candidate ranking with integer scores and typed reason factors.
- Stable greedy budget fitting over complete context items.
- Versioned conservative sizing and canonical length-delimited rendering.
- Context manifest hashing and compaction verification.

The crate does not own SQLite, provider payloads, Ratatui, credentials, or process lifecycle.

## Context policy version 1

A new top-level attempt starts a context epoch.
An explicit retry starts another attempt and another epoch.
Tool continuations remain inside the attempt's epoch, while their dynamic tool and history sources receive distinct per-turn snapshots.
The baseline source set and eligible memory revisions are frozen for the epoch, so a model proposal cannot feed itself back during the same run.

Compaction, relocation, or an incompatible builder, registry, ranker, renderer, sizer, configuration, catalog, model-capability, or tool-registry version starts a new epoch.
Changes observed after a provider-turn boundary apply only to a later eligible epoch or turn.
An in-flight request is never rebuilt asynchronously.

The context builder performs these steps in a fixed order:

1. Resolve the exact user, workspace, session, and selected-agent scope identities.
2. Observe registered sources in `ContextSourceKey` order and distinguish absence from temporary unavailability.
3. Filter out proposed, superseded, conflicted, expired, retracted, deleted, unauthorized, and sensitivity-incompatible memory revisions.
4. Ask structured filters and literalized FTS5 for a bounded immutable candidate batch and its global memory generation.
5. Rank candidates in Rust with a versioned integer tuple and a stable final `MemoryRevisionId` tie break.
6. Reserve budgets for product safety, current input, and tool protocol before workspace instructions, complete history groups, and retrieved memory.
7. Admit or skip each complete memory item without partial proposition truncation.
8. Render authorized instructions separately from inert untrusted data using a canonical length-prefixed format.
9. Persist the complete manifest, source snapshots, admissions, reason factors, sizing counts, rendered hashes, and request hash before provider dispatch.

The initial conservative sizer counts UTF-8 bytes plus fixed framing overhead.
This intentionally over-reserves common text while providing a reproducible upper bound without a provider tokenizer.
A provider tokenizer can replace it only under a new recorded sizer version and epoch compatibility key.

## Memory lifecycle version 1

Each memory item has one stable identity and an append-only sequence of revisions and lifecycle operations.
Content lives in a separately erasable, hash-verified blob rather than inside the immutable ledger envelope.
Confidence uses fixed-point basis points and never substitutes for trust or provenance.

Origins and authority remain separate:

- An explicit user memory may become active after deterministic validation and any required scope or sensitivity confirmation.
- A verified tool observation creates a proposal unless a separately configured policy authorizes that exact observation class.
- Imported data creates a proposal unless the user explicitly approves the import policy.
- A model or compaction summary can create only a proposal.
- Approval creates a new user-approved active revision linked to the immutable proposal.
- The proposer cannot supply its trust class, validator result, approval authority, or final status.

Exact duplicate detection uses canonical line endings, scope, kind, subject key, and a content hash.
Different content under the same explicit subject key becomes a contradiction candidate for review.
Phase 4 does not auto-merge semantic claims or use a model judgment as the final contradiction authority.

Correction requires the expected current revision and creates the next contiguous revision.
Supersession prevents the older revision from future admission while preserving its prior audit history.
Retraction prevents future admission without removing retained content or admissions.
Deletion erases content and every derived model-visible copy while preserving only a minimal tombstone.
A deleted item cannot be revived.

## Provider-turn durability

Context preparation uses an optimistic read-build-commit sequence.
The storage thread reads one immutable candidate snapshot with a global memory generation.
The pure builder constructs the manifest outside SQL.
The context commit then verifies the same memory generation, session sequence, epoch generation, attempt turn, active revision set, validity window, scope, sensitivity, hashes, and budgets inside one `BEGIN IMMEDIATE` transaction.
A conflict discards the draft and rebuilds from a fresh snapshot.

The session event stream binds the committed manifest hash to the exact attempt and run turn before `RunTurnStarted` can make dispatch possible.
Provider dispatch is forbidden until the manifest and its binding event are durable.
A crash before binding cannot dispatch.
A crash after the durable run start retains the existing conservative unknown-attempt recovery semantics.

Provider-neutral `ChatRequest` gains one explicitly classified context prelude instead of a fabricated user-history message.
Gemini maps the prelude to `system_instruction`.
OpenAI-compatible routers map it to a leading system instruction message.
The native Codex adapter places it in its existing developer boundary while retaining the transcript-as-untrusted-data instruction.
Every adapter redacts configured credentials before serialization and pins the native request shape in tests.

## Memory workspace experience

Memory is a sixth primary route reached through `Alt+6`, `/memory`, the command palette, and a Sessions & Data cross-link in Settings.
Existing `Alt+1` through `Alt+5` mappings remain unchanged.
Create, revise, proposal review, action selection, retraction, and deletion use the single overlay owner and restore the exact prior route and focus.

The route reuses the implemented terminal design system:

- The terminal background remains transparent.
- Gradients appear only on the page title and structural dividers.
- Full-width selection uses the existing selected surface, caret, and route icon.
- State, scope, trust, freshness, and provenance use semantic chips with matching glyph or modifier alternatives.
- Search, lists, key-value detail, confidence meters, callouts, buttons, modals, and scrims use existing components.
- One new `RouteMemory` icon triple is added to the centralized icon table.
- No page-level style, color literal, or glyph literal is introduced.

Responsive behavior is explicit:

- `Xl` terminals show searchable list, detail, and admission-history panes.
- `Md` and `Lg` terminals show list and detail, with admission history as a detail tab.
- `Xs`, `Sm`, and forced single-column layouts use list-to-detail drill down with `Esc` stepping back.
- Short terminals suppress subtitles and secondary counts before hiding any action.
- Narrow action sets collapse into a visible Actions button and a vertical action modal.

The default list groups Needs review, Active, and Inactive records.
Deleted tombstones appear only when the user includes them in the state filter.
The detail surface shows inert exact content, revision, scope identity, origin, trust, confidence, sensitivity, validity, evidence, relations, and admission totals.
Admission history shows the exact session, provider attempt, run turn, model, epoch, time, rank, token count, source revision, renderer version, and typed selection reasons.

Approval never occurs from ordinary row activation.
The review modal shows exact proposed content, scope, evidence, sensitivity, duplicate and contradiction findings, and a deliberate approval action.
Retraction and deletion explain their different consequences and state that an already dispatched provider turn cannot be recalled.

## Ordered implementation slices

### Slice 4.0: Contracts and decisions

- Correct the architecture contract for per-turn admissions, typed scope identity, versioned manifests, and logical deletion.
- Add proposed ADR-0017 for auditable provider-turn context manifests.
- Add proposed ADR-0018 for the separate revisioned memory ledger and independent promotion authority.
- Pin domain serialization shapes before storage or UI code depends on them.

Exit criteria:

- Documentation has one non-contradictory contract for session context and cross-scope memory.
- Every irreversible choice is visible in an indexed ADR.
- The Phase 3 release gate remains explicit.

### Slice 4.1: Pure context and memory core

- Add typed IDs, bounded redacted values, scopes, origins, trust, sensitivity, evidence, lifecycle operations, source observations, manifests, and rank reasons.
- Introduce `autoharness-memory` with the source registry, validator, deterministic ranker, budget fitter, renderer, sizer, and manifest hash.
- Add shuffled-input, Unicode, budget-edge, injection-shape, duplicate, contradiction, expiry, and stable-tie tests.

Exit criteria:

- Fixed inputs produce byte-identical manifests regardless of insertion or physical row order.
- Proposed or conflicted memory is structurally ineligible.
- Memory text cannot escape the inert rendered data boundary.

### Slice 4.2: Durable ledger and retrieval

- Add checksummed SQLite migrations for opaque scope binding, the memory ledger, erasable content, validation, evidence, relations, and FTS5.
- Add separate synchronous `MemoryStore` and `ContextStore` ports beside `SessionStore`.
- Maintain FTS explicitly in the same transaction as each memory mutation.
- Rebuild memory projections and FTS from the authoritative ledger.

Exit criteria:

- Proposal, approval, correction, supersession, retraction, and deletion are optimistic, idempotent, and restart-safe.
- FTS queries are literalized, bounded, scope filtered, sensitivity filtered, and deterministically ordered after Rust ranking.
- Failure injection proves that ledger, projections, content, and FTS cannot partially commit.

### Slice 4.3: Durable provider-turn context

- Add context epoch, source snapshot, turn manifest, admission, reason, and compaction tables and session events.
- Bind every first provider call and tool continuation to one committed context manifest.
- Replace both ad hoc coordinator request-build paths with the pure context builder and storage-thread commit.
- Add provider-native instruction framing and body-shape tests for Gemini, OpenAI-compatible routers, and Codex.

Exit criteria:

- Dispatch cannot occur before the exact manifest is durable.
- Each admitted item identifies the exact attempt and run turn that saw it.
- Memory mutation during a build causes a generation conflict and deterministic rebuild instead of mixed context.

### Slice 4.4: Explicit useful memory

- Add typed application operations for explicit user memory, search, inspection, correction, retraction, deletion, and export.
- Retrieve active eligible memory into new context epochs and persist every admission reason.
- Preserve prior admissions after later correction or retraction, subject to privacy deletion.

Exit criteria:

- A remembered fact survives restart, is retrieved under the same fixed query and budget, and appears in the provider instruction prelude.
- Retraction prevents the next eligible epoch from admitting the fact.
- Deletion removes the content from the ledger sidecar, FTS, projections, retained admission copies, and export.

### Slice 4.5: Memory terminal workspace

- Add the Memory route, search and filters, responsive list-detail-history layout, exact proposal review, and lifecycle actions.
- Add a bounded coalesced `MemoryProjection` with generation checks and content-redacted debug behavior.
- Add command, help, keyboard, mouse, accessibility, and Settings cross-link paths over the same typed intents.

Exit criteria:

- A keyboard-only user can create, find, inspect, correct, retract, delete, and audit memory without shell or database access.
- Every action has measured hit geometry and a visible narrow-terminal path.
- All states remain distinct in NoColor, ASCII, reduced-motion, compact, and single-column modes.

### Slice 4.6: Untrusted proposals and compaction

- Add bounded model/tool proposal admission through a no-authority proposal sink.
- Persist deterministic validation outcomes, duplicate and contradiction candidates, and independent approval decisions.
- Add compaction as a new epoch with an untrusted summary proposal and a verified durable-facts hash.

Exit criteria:

- The same model response cannot propose and approve memory.
- Proposed memory is never retrieved before a distinct approval authority creates an active revision.
- Compaction and restart preserve the same active durable facts, pending input, permission, and tool state.

### Slice 4.7: Integrated release evidence

- Extend session export and deletion for context audit rows, session-scoped memory, and cross-scope evidence tombstones.
- Add migration and rollback rehearsal from schema 3, FTS rebuild and corruption tests, crash matrices, secret sentinels, benchmarks, conformance, and a real PTY journey.
- Reconcile architecture, active memory, progress memory, and the project plan only after the verified repository state changes.

Exit criteria:

- Every Phase 4 exit criterion in the project plan has repository evidence.
- Formatting, strict Clippy, full locked workspace tests, documentation, visual conformance, render-cost, migration, restart, and PTY gates pass.
- Phase 4 remains release-gated until the outstanding Phase 3 candidate evidence is approved.

## Verified implementation state

Slice 4.0 is complete in the feature branch.
The serialized context and memory contracts, logical-deletion boundary, proposed [ADR-0017](../adr/0017-use-auditable-provider-turn-context.md), and proposed [ADR-0018](../adr/0018-use-a-separate-revisioned-memory-ledger.md) are checked in with domain round-trip and rejection tests.

Slice 4.1 is complete in the feature branch.
The `autoharness-memory` crate implements deterministic source observation, validation, integer ranking, bounded fitting, canonical inert rendering, manifest hashing, and compaction fact verification with shuffled-input, Unicode, injection-shape, budget-edge, eligibility, and stable-tie coverage.

Slice 4.2 is complete in the feature branch.
SQLite migrations 4 through 6 add opaque scope bindings, the separate memory ledger, erasable sidecars, validation, evidence, relations, context records, and explicitly maintained FTS5 indexes.
The store tests cover optimistic and idempotent lifecycle batches, atomic rollback, literalized and scope-filtered retrieval, status and sensitivity filtering before page limits, deterministic physical-order rebuilds, complete projection replay, missing-sidecar failure, and logical deletion.

Slice 4.3 is complete in the feature branch.
Both first provider calls and tool continuations bind an exact manifest and request hash before dispatch through the single storage actor.
Gemini, OpenAI-compatible, and native Codex adapters map the provider-neutral prelude to their native instruction boundary with request-shape coverage.
Each run turn retains audit-only hashes for its exact provider history and tool definitions plus settled tool messages, while frozen prelude-eligible sources remain unchanged and audit-only sources can never back an admission.
Frozen continuation tests prove restart reuse of the exact retained instruction and memory bytes while later mutations wait for a new epoch, and erased retained bytes fail closed before dispatch.
Recovery waits for an exact live catalog match, reconstructs and verifies the already bound request hash, restores its run budget, and dispatches that turn once without creating a replacement manifest.

Slice 4.4 is complete in the feature branch.
Explicit workspace memory follows the create, validate, activate, retrieve, admit, inspect, correct, retract, export, delete, and restart path through typed application operations.
Durable admissions retain exact attempt, run-turn, model, epoch, rank, reason, source-revision, renderer, and retained-content coordinates, while later retraction or correction preserves prior audit identity.

Slice 4.5 is complete in the feature branch.
The sixth themed primary route provides debounced literal search, status and scope filters, authoritative bounded paging, loading and no-match states, responsive list-detail-history layouts, proposal review, all lifecycle actions, command and Settings entry points, keyboard navigation, and measured mouse targets.
Ten Memory surfaces participate in the complete five-size, theme, color-treatment, glyph, reduced-motion, density, single-column, Indexed256, and Basic16 conformance matrix.
The focused render-cost gate covers both an eight-record view and the one-hundred-record page limit.

Slice 4.6 is complete in the feature branch.
The no-authority proposal sink verifies inline or artifact evidence bytes, persists deterministic validation, duplicate and contradiction candidate identities, and provenance, settles proposal tool output without content, reconciles exact retries idempotently after restart, and never activates its own proposal.
Production compaction selects complete settled history groups, commits a verified durable-facts boundary and replacement epoch atomically, creates only an untrusted session-scoped summary proposal, preserves the replacement baseline across later tool turns, and excludes compacted raw history after restart.
Explicit import accepts one normalized workspace-relative path, verifies canonical containment, reads at most 16 KiB of safe UTF-8 text, hashes the exact source bytes, derives opaque path-free source identity, and creates only a workspace-scoped imported proposal with typed document evidence.
The terminal presents import through Alt+I, `/memory-import`, the command palette, and measured mouse controls, explains the size and review boundary at wide and narrow sizes, and requires a separate explicit-user revision before imported content can become active.

Slice 4.7 is complete and locally validated in the feature branch.
The checked-in evidence covers schema-3 migration and rollback-copy rehearsal, FTS and projection corruption recovery, context and memory transaction failure injection, exact raw configured-credential rejection, session and standalone exports, logical deletion, restart, the 7,685-case visual conformance manifest, bounded Memory rendering, and a real PTY remember-import-review-approve-to-delete journey.
The all-profile exact raw credential boundary fails closed when configured sentinels cannot be recovered, redacts exact values from submitted prompts before durable admission, and rejects or cancels matching context construction, reconstructed provider requests, streamed text, normalized call identities, structured argument keys and values, local tool output, memory writes, compaction, and recovered bound requests.
This credential boundary does not claim encoded or component-derived data-loss prevention or artifact-at-rest secret scanning, and fragmented active or session-only values depend on provider adapters preserving ordered stream fragments because the application does not own those raw sentinels.
Formatting, strict all-target and all-feature workspace Clippy, the complete locked workspace suite, documentation links, visual conformance, focused render cost, migration, restart, visual review, and the exact ignored Memory PTY journey pass locally.
The inherited Phase 3.9 and Phase 3.10 cross-platform CI, three human terminal smokes, live-provider and platform-vault checks, approved reference-machine reports, rollback evidence, release checklist, independent approval, promotion, and ADR acceptance remain release blockers.

## Required validation matrix

### Determinism and replay

- Shuffled source and candidate insertion produces the same ordered manifest and hash.
- Equal scores use stable identity tie breaks.
- Exact token-budget edges admit or skip complete items consistently.
- Context reconstruction remains exact after memory supersession or retraction.
- A deleted admission reconstructs as a content-unavailable tombstone without leaking erased text.
- Restart and projection rebuild preserve the same effective durable facts.

### Concurrency and crash boundaries

- Memory generation changes between retrieval and commit force a rebuild.
- Context cannot mutate during an in-flight provider turn.
- First dispatch and tool continuation each bind a distinct turn manifest.
- Crash before binding cannot dispatch.
- Crash after durable run start remains an explicit unknown outcome.
- Failure injection around ledger, projection, FTS, context, and commit boundaries leaves no partial durable state.

### Security and privacy

- Prompt-injection-shaped memory remains inert delimited data.
- Proposed memory cannot authorize tools, permissions, network, trust, or its own approval.
- Exact raw configured-credential sentinels never appear in ledger operations, blobs, FTS, snapshots, admissions, projections, model-visible requests, debug output, logs, or exports.
- Phase 4 does not claim encoded or component-derived data-loss prevention or scanning of arbitrary artifacts at rest.
- Retraction and deletion immediately affect future eligibility.
- Logical deletion semantics and source-history limitations are stated accurately.

### Search and ranking

- Quotes, FTS operators, control text, and Unicode are literalized safely.
- Scope, status, validity, contradiction, sensitivity, and authorization filters fail closed.
- Ranking uses fixed-point values and persists typed reason factors.
- Rebuilding FTS in another physical row order does not alter final ranking or context fit.

### Terminal quality

- Loading, empty, no-match, stale, failed, proposed, conflicting, active, superseded, retracted, expired, and deleted states render deliberately.
- Memory route and overlays render at 40x12, 60x18, 80x24, 120x40, and 120x50.
- Every existing theme and color treatment, all glyph modes, reduced motion, compact density, single-column layout, Indexed256, and Basic16 remain covered.
- Rendering and queries are bounded by visible pages and configured candidate limits rather than the total memory count.
- A real PTY journey covers remember, restart, search, inspect, retract, verify non-admission, delete, resize, and terminal restoration.

## Commit sequence

Each slice lands through small conventional commits that leave the workspace compiling or clearly limit an intermediate commit to documentation and isolated tests.
The expected sequence is documentation and ADRs, domain contracts, pure memory core, store ports, SQLite ledger, FTS retrieval, context manifests, provider framing, application integration, Memory route model, Memory page rendering, lifecycle UI, composed tests, PTY evidence, and repository-memory reconciliation.
Generated files and unrelated user work are never rewritten to make a slice convenient.
