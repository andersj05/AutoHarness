# ADR-0004: Use Gemini Interactions v1 for the default stream

**Status:** Accepted

**Date:** 2026-08-20

**Owners:** Project maintainers

## Context and problem statement

Phase 1 requires a concrete Google AI Studio transport for cancellable streamed conversation.
Google now recommends the Interactions API for new Gemini applications, and its core contract is generally available in the stable `v1` API.
AutoHarness also requires local event history to remain authoritative for replay and recovery.

## Decision drivers

- Use Google's recommended stable API for new applications.
- Preserve deterministic local replay without relying on provider-retained conversation state.
- Decode a typed lifecycle stream rather than provider-specific response snapshots in the engine.
- Keep compatibility with models that still expose only Generate Content streaming.
- Prevent credentials from entering URLs, events, logs, storage, or terminal state.

## Considered options

1. Use stable Interactions `v1` as the default and keep Generate Content as an explicit compatibility fallback.
2. Use `streamGenerateContent` `v1beta` as the only Phase 1 transport.
3. Use provider-stored Interactions state through `previous_interaction_id`.

## Decision outcome

Chosen option: **use `POST /v1/interactions?alt=sse` with `stream: true` and `store: false` as the default Gemini conversation transport**.

AutoHarness sends the complete locally reconstructed provider-neutral conversation for each turn.
The adapter authenticates with the `x-goog-api-key` header, disables cross-origin redirects, and keeps the key inside secret-bearing adapter configuration.
The adapter may fall back to `POST /v1beta/models/{model}:streamGenerateContent?alt=sse` only when the default request is rejected before any semantic stream event with a known unsupported or model-not-found classification.
It never changes transport after a request may have produced an external effect.

Model discovery continues to use paginated `GET /v1beta/models` because that is the documented Models API route.
Compatibility requires an exact `generateContent` entry in `supportedGenerationMethods`.
The catalog does not claim Interactions support because the Models API does not currently advertise that capability.

## Consequences

### Positive

- Phase 1 follows Google's stable recommended path for new Gemini applications.
- Local durable events remain the single recovery and transcript authority.
- `store: false` avoids depending on provider retention or interaction deletion for session correctness.
- The fallback policy is narrow, testable, and cannot duplicate an ambiguously dispatched request.

### Negative

- The adapter must maintain two request and stream decoders during the compatibility period.
- Local history must include every complete turn needed by the provider request.
- Model discovery cannot prove Interactions compatibility before dispatch, so that capability remains unknown.
- Stateless requests cannot use `previous_interaction_id` or provider-side background execution.

### Follow-up

- Cover pagination, arbitrary UTF-8 and SSE fragmentation, lifecycle completion, cancellation, fallback boundaries, retry classification, and secret redaction with fixture tests.
- Revisit the Generate Content fallback after the catalog exposes reliable Interactions capability metadata or incompatible models disappear from supported use.
- Supersede this ADR if Google changes the stable transport or retention contract.

## Evidence

- [Interactions API overview](https://ai.google.dev/gemini-api/docs/interactions-overview)
- [Stable Interactions API reference](https://ai.google.dev/api/interactions-api-v1)
- [Gemini API version guarantees](https://ai.google.dev/gemini-api/docs/api-versions)
- [Models API reference](https://ai.google.dev/api/models)
- [Gemini API key guidance](https://ai.google.dev/gemini-api/docs/api-key)

## Related decisions

- [ADR-0001](0001-use-rust-modular-monolith.md)
