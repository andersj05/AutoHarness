# AutoHarness project plan

**Status:** Active

**Last updated:** 2026-08-20

**Planning horizon:** Foundation through distributed self-improvement

## Outcome

Build an open-source, provider-neutral agent harness that feels instantaneous in a terminal, preserves enough execution state to recover and replay work, and improves its behavior through evidence rather than unchecked self-modification.

The first end-to-end slice connects to Google AI Studio, discovers available models dynamically, lets the user select one, streams a response, and restores the session after restart. The next provider slice connects to a configurable model router.

## Product principles

1. **The engine is the product.** The terminal is the first client, not the owner of orchestration logic.
2. **Durability precedes autonomy.** Inputs, tool state, model attempts, and behavior-changing decisions must be recoverable.
3. **Protocols are normalized at the edge.** Provider-specific request and stream formats stop at provider adapters.
4. **Context is admitted, not concatenated ad hoc.** Every model-visible memory item has provenance and a deterministic admission boundary.
5. **Improvement is an experiment.** A candidate must be reproducible, evaluated, promotable, and reversible.
6. **Local first, remotely scalable.** Begin as a modular single binary; preserve contracts that allow a daemon and remote workers later.
7. **Security is capability based.** Secrets, tools, plugins, and stored memory receive the least authority required.
8. **Performance is measured.** Optimize startup, first draw, stream overhead, memory use, and recovery using benchmarks rather than intuition.

## Scope

### Initial scope

- Native terminal chat with a selectable model catalog.
- Google AI Studio authentication and model discovery.
- Google streaming responses with cancellation and usage reporting.
- Configurable model-router adapter, initially supporting an OpenAI-compatible dialect.
- Durable, replayable sessions in local SQLite.
- Typed provider, event, storage, and context contracts.
- Repository-native project memory and ADRs.
- Tests, benchmarks, tracing, and secret-redaction guarantees.

### Long-term scope

- Tool execution with permissions, budgets, and sandboxes.
- Multiple concurrent agents and resumable workflows.
- User-, workspace-, session-, and agent-scoped memory.
- Provider routing based on capability, quality, latency, availability, and cost.
- Evaluation datasets derived from failures and explicit feedback.
- Candidate generation for prompts, policies, tools, routing, memory, and source changes.
- Gated promotion, canaries, rollback, and distributed workers.
- Sandboxed extension ecosystem using versioned plugin capabilities.

### Non-goals for the first release

- Training or fine-tuning foundation-model weights.
- A web or desktop UI.
- Unrestricted shell or filesystem tools.
- Multi-tenant cloud hosting.
- Autonomous promotion of model-authored changes.
- A mandatory vector database.
- Microservices before local process boundaries are measured as insufficient.

## Delivery phases

### Phase 0: Repository foundation

**Status:** Complete

**Goal:** Make the intended product, architecture, decisions, and current work state durable before scaffolding implementation.

Deliverables:

- Root `AGENTS.md` with cross-tool working and memory instructions.
- Project plan, architecture overview, and persistent-memory specification.
- Repository memory with stable, active, and progress layers.
- ADR process and initial decisions.
- Reference-project research with commit-pinned sources.

Exit criteria:

- A new contributor or agent can identify the goal, current phase, next action, constraints, and decision history from repository files alone.
- No contradictory source of truth exists for project status or architecture.

### Phase 1: Fast terminal vertical slice

**Status:** Complete as of 2026-08-20

**Goal:** Prove the complete path from key discovery to selectable model to streamed, replayable conversation.

Deliverables:

- Rust workspace and continuous-integration baseline.
- Headless engine with typed commands and events.
- Ratatui client using a model/update/view loop.
- Gemini provider with API-key authentication, paginated model discovery, and streaming.
- SQLite migrations and repositories for sessions, durable input, and events.
- Model picker, transcript, multiline composer, cancellation, retry, and error presentation.
- Structured tracing with mandatory secret redaction.

Exit criteria:

- `GEMINI_API_KEY` is sufficient to start a session without writing the key to disk.
- Compatible models are discovered from the API rather than hardcoded.
- The user can choose a model, stream a response, cancel it, and retry safely.
- Restarting the app restores the selected model and transcript.
- Replaying stored events reconstructs the same visible session.
- Tests cover model pagination, arbitrary SSE fragmentation, cancellation, retry classification, terminal restoration, and redaction.

Completion is supported by fixture-backed provider tests and a composed integration test that selects a model, streams, cancels, retries, shuts down, reopens SQLite, and compares the recovered terminal projection.
The repository has not exercised a live Gemini network request, so that remains a separate pre-release validation item rather than claimed completion evidence.

### Phase 2: Provider and router platform

**Status:** Next

**Goal:** Prove that provider differences remain outside the engine.

Deliverables:

- Stable `Provider`, `Catalog`, and `ModelStream` contracts.
- Capability-aware `ModelDescriptor` and provider availability state.
- OpenAI-compatible router adapter with configurable URL, authentication header, and model discovery.
- Provider conformance suite driven by recorded HTTP fixtures.
- Timeout, retry, concurrency, and per-project rate-limit middleware.
- Model-catalog cache with refresh and stale-data policy.

Exit criteria:

- The same engine and TUI session path works against Gemini and the configured router.
- Adding a fixture-only provider requires no engine or TUI changes.
- Unsupported capabilities fail before a provider request when discoverable.
- Provider-specific payloads never enter core domain types.

### Phase 3: Safe agent execution

**Goal:** Add a resumable tool loop without giving models ambient authority.

Deliverables:

- Versioned tool schemas and typed tool-call events.
- Permission engine with deny, ask, and allow decisions scoped by tool and resource.
- Filesystem, process, and HTTP capability interfaces.
- Bounded output capture and artifact storage.
- Durable tool-call lifecycle and crash recovery.
- Per-run limits for turns, time, tokens, cost, output, and concurrency.

Exit criteria:

- Every external side effect is attributable to a durable tool call and permission decision.
- Interrupted calls settle deterministically as completed, failed, cancelled, or unknown with an explicit recovery policy.
- A model cannot acquire a capability merely by emitting a differently shaped tool call.

### Phase 4: Persistent context and memory

**Goal:** Turn durable history into useful, bounded, auditable model context.

Deliverables:

- Context-source registry and deterministic context builder.
- Context epochs, snapshots, admissions, and compaction.
- User-, workspace-, session-, and agent-scoped memory records.
- Memory proposal, validation, deduplication, supersession, retraction, and deletion flows.
- SQLite FTS retrieval with a replaceable ranking interface.
- Memory inspection UI showing source, confidence, scope, age, and admission history.

Exit criteria:

- Every injected memory can answer where it came from, why it was selected, and which provider turns saw it.
- Model-generated memory is never silently promoted to trusted memory.
- Context construction is deterministic for a fixed event log, catalog snapshot, configuration, and token budget.
- Compaction and restart do not change the effective durable facts.

### Phase 5: Evaluation and self-improvement

**Goal:** Improve harness behavior through controlled, reproducible experiments.

Deliverables:

- Versioned evaluation-case and dataset formats.
- Trace-to-evaluation failure-mining workflow.
- Candidate registry for prompts, policies, routing, tools, memory strategy, and source patches.
- Reproducible experiment runner with baseline/candidate comparison.
- Metrics for quality, task success, safety, latency, cost, and reliability.
- Promotion policy, approval gate, canary, rollback, and audit log.
- Isolation between candidate generation and candidate evaluation.

Exit criteria:

- A candidate cannot alter its evaluation data, judge configuration, promotion threshold, or audit record.
- Promotion requires configured metric improvement with no guardrail regression.
- Every promoted behavior is linked to its code/config version, dataset, evaluator, results, approval, and rollback target.

### Phase 6: Extension and distributed runtime

**Goal:** Scale execution without changing the core semantics proven locally.

Deliverables:

- Versioned daemon protocol and thin remote TUI client.
- Remote worker leases, heartbeats, cancellation, idempotency, and recovery.
- PostgreSQL durable store and object storage for large artifacts.
- Wasmtime component host with WIT-defined provider, tool, memory, and evaluator capabilities.
- Signed extension packages, declared permissions, resource limits, and compatibility checks.
- Multi-workspace scheduling, quotas, and observability.

Exit criteria:

- Local and remote execution pass the same conformance and replay suites.
- Losing a worker cannot lose already-admitted user input or fabricate successful work.
- Plugins have no filesystem, network, process, secret, or memory access unless explicitly granted.

## Next implementation order

Phase 1 established the complete local terminal, Gemini, storage, replay, and tracing path.
Proceed through Phase 2 in this order:

1. Extract provider conformance tests from the Gemini implementation.
2. Stabilize provider availability, capability, catalog, and streaming contracts against that suite.
3. Define the configurable router URL, authentication-header, model-discovery, and OpenAI-compatible streaming contract.
4. Add the router adapter behind the existing provider ports.
5. Add shared timeout, retry, concurrency, and per-project rate-limit middleware.
6. Add a durable model-catalog cache with explicit refresh and stale-data policy.
7. Add safe monotonic markers for deferred startup, dispatch, and rendered-delta latency, then record the checked-in benchmark suite on an approved reference machine.

Each step must leave a runnable or testable vertical slice; avoid creating unused framework layers far ahead of their first consumer.

## Quality gates

### Correctness

- Domain transitions are testable without a terminal, network, or real database.
- Durable writes and externally visible events have explicit transaction boundaries.
- Retries use stable request identifiers where the provider supports them.
- Parsing handles arbitrary byte and event fragmentation.

### Performance

Establish a checked-in benchmark environment before setting release thresholds. Measure at minimum:

- Cold process start to first terminal draw.
- Idle resident memory.
- Input-to-request dispatch overhead.
- Provider-chunk receipt to rendered-delta latency.
- Event append and transcript projection throughput.
- Recovery time for representative session sizes.

LLM network latency must be reported separately from harness overhead.

### Security and privacy

- Secrets come from environment variables or OS credential storage and are represented by opaque references.
- Authentication material is structurally excluded from events, errors, telemetry, and memory.
- Model text is untrusted data, including when it resembles instructions or memory metadata.
- Tool and plugin capabilities default to denied.
- Users can inspect, export, retract, and delete stored memory.

### Compatibility

- Persistent rows and serialized events carry schema versions.
- Public protocols and plugin interfaces use explicit compatibility rules.
- Provider adapters declare supported features and degrade only through explicit policy.

## Major risks and responses

| Risk | Response |
| --- | --- |
| Provider APIs change rapidly | Contract tests, recorded fixtures, capability discovery, and isolated adapters |
| The abstraction collapses to the least common denominator | Keep normalized lifecycle events while allowing namespaced provider options at the edge |
| Memory becomes prompt-injection persistence | Provenance, trust classes, proposal validation, inspection, and retraction |
| Self-improvement rewards the evaluator instead of users | Hidden holdouts, multiple metrics, independent judges, guardrails, and canaries |
| Event storage grows without bound | Projections, retention policies, content-addressed artifacts, and compaction without losing audit identity |
| Rust plugin authoring is too restrictive | WIT components plus a supervised JSON-RPC subprocess bridge |
| Early distributed design slows the local product | Modular monolith first; remote protocols follow demonstrated semantics |
| Documentation memory goes stale | End-of-task reconciliation rules in `AGENTS.md` and a single authority for each fact |

## Open decisions

- The router's exact protocol, model-discovery endpoint, and authentication scheme.
- The public repository license and contribution policy.
- Benchmark hardware and release thresholds.
- The first non-Rust plugin authoring path to support.

Resolve an open decision with an ADR when it becomes implementation-blocking.
