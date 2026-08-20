# ADR-0005: Use ephemeral in-app provider credentials

**Status:** Accepted

**Date:** 2026-08-20

**Owners:** Project maintainers

## Context and problem statement

The Phase 1 terminal required users to populate `GEMINI_API_KEY` before launching AutoHarness.
That boundary was safe for automation, but it made the normal interactive path depend on a shell command and gave the application no guided recovery when a credential was missing or invalid.
Accepting a secret inside the terminal crosses the TUI, application-composition, and provider boundaries, so its lifetime and ownership must be explicit.

## Decision drivers

- Let an interactive user paste a Google AI Studio API key after AutoHarness starts.
- Keep credentials out of domain commands, durable events, SQLite, logs, transcripts, telemetry fields, debug output, and model-visible content.
- Preserve `GEMINI_API_KEY` as an optional non-interactive startup mechanism.
- Avoid retaining a plaintext credential in the ordinary prompt editor or a general configuration model.
- Provide safe retry after an invalid credential without echoing the rejected value.

## Considered options

1. Require environment configuration before every launch.
2. Accept the credential in a dedicated ephemeral terminal overlay and transfer it directly to the provider adapter.
3. Persist the credential through a configuration file or the operating-system credential store.
4. Reuse the multiline prompt editor for credential input.

## Decision outcome

Chosen option: **accept the credential in a dedicated ephemeral terminal overlay**, because it provides the requested interactive experience while preserving a narrow, auditable secret lifetime.

When `GEMINI_API_KEY` is absent, AutoHarness opens a masked API-key overlay at startup.
`Ctrl+K` opens the same overlay later so the user can replace a rejected or expired key.
The editor accepts a bounded visible-ASCII value, uses zeroizing storage, renders a fixed-length mask, and clears itself when submitted or dismissed.
Bracketed paste ownership is wrapped in zeroizing memory before the update finishes.
The TUI emits a secret-specific, non-serializable intent whose debug representation is redacted.
Application composition moves the value into `GeminiApiKey`, constructs a new provider, and validates it by loading the model catalog.
A validation or catalog failure reopens an empty overlay and presents only the provider's safe classified error.
Successful or failed values are never persisted, so an interactive credential must be entered again on the next process launch unless `GEMINI_API_KEY` is available.

## Consequences

### Positive

- The default interactive launch no longer requires a preceding credential command.
- Environment-based configuration remains available for automation and managed launches.
- Secret-bearing UI state is isolated from prompts, replayable state, and provider-neutral domain types.
- Invalid credentials can be replaced without restarting the process.

### Negative

- Interactive credentials are intentionally not remembered across process restarts.
- The terminal event path necessarily handles secret bytes briefly before transferring them into the provider's redacting value type.
- Persisting credentials in an operating-system keyring remains a separate future decision.

### Follow-up

- Apply the same ephemeral secret contract to future provider and router credential fields.
- Decide separately whether opt-in operating-system credential storage is desirable.
- Keep redaction, terminal rendering, debug output, and durable-file sentinel tests in the release gate.

## Evidence

- [`autoharness-tui` credential model and update tests](../../crates/autoharness-tui/tests/ui.rs)
- [`autoharness-app` composed credential test](../../crates/autoharness-app/src/coordinator.rs)
- [`GeminiApiKey` redaction and zeroization boundary](../../crates/autoharness-provider-gemini/src/auth.rs)

## Related decisions

- [ADR-0001](0001-use-rust-modular-monolith.md)
- [ADR-0004](0004-use-gemini-interactions-v1.md)
