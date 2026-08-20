# Architecture overview

**Status:** Proposed baseline

**Last updated:** 2026-08-20

## System shape

AutoHarness begins as a modular monolith distributed as one native executable. The terminal and headless commands compose the same engine in-process. Stable internal contracts allow the engine to move behind a local or remote daemon later without changing domain semantics.

```text
┌──────────────┐    commands     ┌──────────────────────┐
│ TUI / CLI    │ ──────────────> │ Application engine   │
│ clients      │ <────────────── │ sessions + scheduler │
└──────────────┘     events      └───────┬──────┬───────┘
                                         │      │
                              ports      │      │ ports
                                         v      v
                                  ┌──────────┐ ┌──────────────┐
                                  │ Provider │ │ Durable store │
                                  │ adapters │ │ + artifacts   │
                                  └────┬─────┘ └──────────────┘
                                       │
                              Gemini / router / future providers

Durable events ──> context + memory ──> evaluations ──> candidates
      ^                                                    │
      └──────────── promotion audit + rollback <───────────┘
```

## Dependency rules

1. Domain and protocol types do not depend on TUI, HTTP, SQLite, Wasmtime, or a concrete provider.
2. The engine depends on ports expressed in domain terms.
3. Provider, storage, plugin, and telemetry crates implement ports and depend inward.
4. The application crate owns composition, configuration, and process lifecycle.
5. The TUI consumes commands, read models, and events; it does not call provider or storage adapters directly.
6. Provider-native payloads stay inside the adapter. Persisted protocol-independent events are the integration boundary.
7. No crate cycle is allowed. Shared types move inward only when they are stable domain concepts, not merely to break a compiler error.

## Proposed workspace

```text
crates/
  autoharness-domain/              # IDs, values, commands, events, errors
  autoharness-engine/              # session state machines and scheduling
  autoharness-protocol/            # versioned external/daemon contracts
  autoharness-provider/            # provider ports and conformance suite
  autoharness-provider-gemini/     # Google AI Studio adapter
  autoharness-provider-openai/     # OpenAI-compatible router adapter
  autoharness-store/               # durable-store ports and migrations API
  autoharness-store-sqlite/        # local SQLite implementation
  autoharness-memory/              # context sources, retrieval, admission
  autoharness-evals/               # datasets, experiments, promotion evidence
  autoharness-plugin-host/         # Wasmtime/WIT capability host
  autoharness-tui/                 # Ratatui model/update/view client
  autoharness-app/                 # binary, config, composition, lifecycle
```

This is a target map, not a requirement to create empty crates. Introduce each crate with its first real consumer. Closely coupled, small modules may begin together and split only when their boundary is proven.

## Core runtime contracts

### Commands

Commands express requested intent, not assumed success. Representative commands include:

- Create or resume a session.
- Refresh a provider catalog.
- Select a provider/model/variant.
- Admit a user prompt.
- Cancel or retry an attempt.
- Answer a permission request.
- Inspect, accept, retract, or delete memory.

Each accepted command returns or emits a durable identifier so callers can correlate later events.

### Events

The normalized event stream must represent lifecycle rather than a provider's wire format. Initial event families are:

- Session created, updated, archived, or selected.
- Input admitted and promoted.
- Provider attempt started, delta received, usage reported, completed, failed, or cancelled.
- Text, reasoning, tool-call, artifact, warning, and metadata deltas.
- Catalog refreshed and model availability changed.
- Memory proposed, admitted, superseded, retracted, or deleted.
- Evaluation and promotion lifecycle events.

Events carry a schema version, stable ID, aggregate/session ID, monotonic sequence, timestamp, causation ID, correlation ID, and safe payload. Authentication material is not a payload field.

### Providers

Provider adapters implement four separable concerns:

1. Availability and authentication discovery.
2. Model discovery and capability mapping.
3. Request preparation from provider-neutral input plus explicitly namespaced options.
4. Native stream decoding into normalized engine events.

The first adapters are Gemini and an OpenAI-compatible model router. A provider conformance suite validates discovery pagination, cancellation, error classification, retry hints, stream fragmentation, usage, and redaction.

### Storage

The local store uses SQLite with write-ahead logging. The storage boundary exposes domain transactions rather than raw SQL across the engine. Append-only events remain authoritative; read-optimized projections serve the TUI and queries.

Initial durable records include:

- Sessions and selected provider/model snapshots.
- Admitted inputs.
- Provider attempts and normalized events.
- Transcript projections.
- Catalog snapshots and refresh metadata.
- Context epochs and admissions.
- Memory identities, revisions, evidence, and lifecycle.
- Artifacts by content hash.

Large content moves to an artifact store while the event log retains identity, media type, size, content hash, and policy metadata.

## Runtime flow

### First model request

1. The client submits an `AdmitPrompt` command.
2. The engine validates the target session and durably admits the prompt.
3. At the next provider-turn boundary, the engine promotes eligible input, resolves the selected model, and builds a deterministic context snapshot.
4. The provider adapter prepares the native request without exposing credentials to the engine event payload.
5. The engine durably records the attempt before dispatch.
6. Provider chunks are decoded to normalized events, persisted in bounded batches, and published to clients through bounded channels.
7. Completion, failure, or cancellation settles the attempt and updates projections.
8. Restart reconstructs the session from durable inputs, attempt state, and events rather than assuming the prior process finished cleanly.

### Terminal rendering

The TUI follows model/update/view:

- **Model:** local read state needed to draw the current screen.
- **Update:** pure or narrowly effectful handling of input and engine events.
- **View:** terminal rendering from model state only.

Network and storage tasks run outside the render loop. Bounded queues and coalesced delta rendering prevent high-frequency streams from starving keyboard input or terminal restoration.

### Improvement lifecycle

1. Traces and explicit feedback produce evaluation candidates.
2. A curator admits reproducible cases into a versioned dataset.
3. A generator proposes one versioned behavioral or source candidate.
4. An isolated runner evaluates baseline and candidate under the same manifest.
5. An independent policy checks primary metrics and non-regression guardrails.
6. Approved candidates enter a canary before promotion.
7. The registry retains the prior version and complete rollback evidence.

Generation, evaluation, and promotion must not share mutable authority.

## Concurrency and recovery

- One session has a single logical writer for state transitions, implemented initially as an engine task/actor.
- Different sessions may progress concurrently within configured global and provider limits.
- Channels are bounded; overload has an explicit backpressure, coalescing, or rejection policy.
- Cancellation is a first-class signal propagated from client to engine to provider/tool operation.
- In-memory run/drain objects coordinate work but are not treated as durable business entities.
- Crash recovery derives pending work from durable inputs and settled/unknown attempts.
- Retrying an external effect requires an explicit idempotency or reconciliation strategy.

## Configuration and secrets

- Human-editable non-secret configuration uses a versioned file format and environment overrides.
- Secret configuration is represented as references such as `env:GEMINI_API_KEY` or an OS-keyring entry.
- Debug views render a redacted configuration projection.
- Base URLs are validated; redirect behavior must not forward credentials to an untrusted origin.
- Provider and plugin network access is governed by an allow policy in security-sensitive modes.

## Extension model

The eventual primary plugin boundary is the WebAssembly Component Model with WIT-defined capabilities. A plugin receives only declared host functions and bounded resources. Compiled components may be cached to preserve startup performance.

A supervised subprocess bridge supports ecosystems that cannot yet target components. It uses a versioned protocol, handshake, deadlines, output limits, process-tree cancellation, and the same capability policy as component plugins.

No plugin may obtain ambient filesystem, network, process, secret, memory, or promotion access.

## Evolution to remote scale

Move a boundary out of process only after its local semantics and failure states are covered by conformance tests. The expected sequence is:

1. Run the same engine behind a local daemon transport.
2. Make the TUI a thin client using the versioned protocol.
3. Add remote worker leases for provider/tool/evaluation jobs.
4. Replace SQLite with PostgreSQL for shared coordination while preserving the store port.
5. Move large artifacts to object storage.

The event, permission, context, and promotion semantics must remain the same in local and remote modes.
