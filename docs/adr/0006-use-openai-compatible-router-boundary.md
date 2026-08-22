# ADR-0006: Use an OpenAI-compatible router boundary

**Status:** Accepted

**Date:** 2026-08-20

**Owners:** Project maintainers

**2026-08-22 clarification:** Tool definitions require positive model capability evidence, while missing or older cached capability metadata disables tool advertisement and preserves plain streamed chat.

## Context and problem statement

Phase 2 must prove that a configurable model router can use the same catalog, session, streaming, cancellation, retry, and recovery path as Gemini without introducing router payloads into the engine or terminal.
Router deployments vary in origin, authentication header, project identity, endpoint paths, model metadata, and operational limits.
The shared boundary also needs a durable catalog cache and explicit timeout, retry, concurrency, and rate-limit behavior.

## Decision drivers

- Preserve the existing provider-neutral engine command and event contracts.
- Support common OpenAI-compatible routers without coupling AutoHarness to one router vendor.
- Keep credentials out of URLs, configuration projections, logs, events, cache records, and model-visible content.
- Reject capabilities reported as unsupported before dispatch.
- Retry only before a streaming request can have an ambiguous external outcome.
- Apply shared operational policy consistently across current and future adapters.
- Keep cached model metadata provider-neutral, bounded, integrity-checked, and scoped to one provider project.

## Considered options

1. Add a configurable OpenAI-compatible adapter behind the provider ports and wrap adapters with shared provider policy and catalog-cache behavior.
2. Implement router behavior directly in application orchestration.
3. Add router-specific commands, events, and terminal state to the engine-facing contracts.
4. Require one fixed router vendor and hard-code its origin, paths, and authentication scheme.

## Decision outcome

Chosen option: **add a configurable OpenAI-compatible adapter and a provider-neutral management wrapper**.

The router adapter uses `GET` model discovery and `POST` streamed chat-completions endpoints resolved under one validated base URL.
The base URL, provider-project identity, authentication header name, authentication scheme, model path, and chat path are non-secret configuration.
The credential remains a zeroizing in-memory value supplied through an environment reference or the existing masked terminal handoff.
Redirects are disabled, endpoints must retain the configured origin, and credentials are sent only in the configured sensitive header.
Credential-bearing non-loopback router endpoints require HTTPS, while loopback HTTP remains available for local routers and fixtures.

The adapter sends complete locally reconstructed chat history with `stream: true` and requests streamed usage when supported.
It normalizes OpenAI-compatible server-sent events into the existing lifecycle, text, cumulative usage, cancellation, and completion events.
Provider extensions in model metadata may explicitly advertise capabilities, but absent metadata remains unknown rather than being invented.
The application advertises the built-in custom-function registry only when the selected descriptor positively reports support for the adapter's exact tool-calling dialect.

Application composition wraps both Gemini and router adapters with the same bounded policy layer.
That layer applies dispatch and idle deadlines, bounded pre-stream retries, concurrency limits, a per-project request window, and capability preflight from the most recent discovered catalog.
It never automatically retries after a provider event stream has started.

Catalog requests distinguish ordinary cached startup reads from explicit refreshes.
Successful live results replace a schema-versioned SQLite catalog snapshot keyed by provider-project identity.
A fresh snapshot may satisfy startup without a network request.
A transient refresh failure may return a bounded stale snapshot, while authentication, authorization, invalid-request, corruption, and expired-stale failures fail closed.

## Consequences

### Positive

- Gemini, the router, and future fixture providers share one engine and terminal path.
- Router deployment details remain in adapter and application configuration.
- Shared policy behavior can be conformance-tested independently from provider wire protocols.
- Explicit capability metadata prevents known unsupported dispatches.
- Catalog startup and temporary provider outages have deterministic freshness behavior.
- Cache records contain only provider-neutral model descriptors and refresh metadata.

### Negative

- OpenAI-compatible dialect differences still require adapter compatibility work when a router departs from the selected contract.
- Capability discovery is conservative when a router returns only the standard minimal model object.
- SQLite gains a provider-catalog projection that must follow the same migration integrity rules as session storage.
- Automatic stream retries remain intentionally limited because duplicate generation cannot be ruled out after ambiguous dispatch.

### Follow-up

- Run the shared conformance assertions against every production adapter using local HTTP fixtures.
- Add adapter-specific compatibility only when recorded evidence demonstrates a real router dialect difference.
- Revisit remote cache coordination when provider execution moves out of process.

## Evidence

- [Phase 2 plan](../PROJECT_PLAN.md#phase-2-provider-and-router-platform)
- [Architecture provider boundary](../architecture/OVERVIEW.md#providers)
- [Gemini transport decision](0004-use-gemini-interactions-v1.md)
- Fixture-backed adapter and policy tests in the Phase 2 implementation.

## Related decisions

- [ADR-0001](0001-use-rust-modular-monolith.md)
- [ADR-0004](0004-use-gemini-interactions-v1.md)
- [ADR-0005](0005-use-ephemeral-in-app-credentials.md)
