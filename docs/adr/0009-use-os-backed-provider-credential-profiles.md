# ADR-0009: Use operating-system-backed provider credential profiles

**Status:** Proposed

**Date:** 2026-08-22

**Owners:** Project maintainers

## Context and problem statement

[ADR-0005](0005-use-ephemeral-in-app-credentials.md) deliberately limited interactive credentials to one process lifetime and deferred persistent credential storage.
That boundary keeps secrets out of AutoHarness storage, but it forces an interactive user to paste the same key after every restart and prevents a provider profile from reconnecting as a normal application setting.
Phase 3.3 requires convenient reconnect behavior without moving raw API keys into SQLite, a settings file, session events, transcripts, logs, telemetry, or model-visible context.

## Decision drivers

- Let a user opt in to reconnecting a named provider profile after restart.
- Keep raw credentials out of provider-neutral domain state and ordinary application-owned durable storage.
- Support replace, disconnect, and delete flows with understandable ownership.
- Preserve environment-variable and session-only credential paths for automation, managed launches, and systems without an available credential vault.
- Keep network failure or a locked vault from blocking offline session and settings access.
- Prevent model-writable workspace configuration from selecting or weakening secret-handling policy.

## Considered options

1. Continue requiring environment variables or ephemeral paste on every launch.
2. Store plaintext keys in a user settings file or SQLite.
3. Encrypt keys in application storage with an AutoHarness-managed master password or local encryption key.
4. Store keys in the operating system's credential facility and retain only an opaque reference in the provider profile.

## Proposed decision outcome

Use an application-owned credential-vault port with operating-system implementations for Windows Credential Manager, macOS Keychain, and Linux Secret Service where available.
A named provider profile contains non-secret provider configuration and an opaque credential reference, never the raw credential.
Saving is explicit and opt in.
Session-only entry remains available, and environment variables remain available for non-interactive or managed launches.
The effective credential source is visible in safe terms such as `environment`, `credential vault`, or `session only`, without exposing secret metadata that could aid reconstruction.

Vault reads transfer the secret directly into the provider adapter's redacting and zeroizing credential type.
The raw value never enters domain commands, durable events, SQLite, settings serialization, transcripts, ordinary debug values, logs, telemetry, or model-visible content.
Replace and delete operations use exact provider-profile identity, update the vault and profile through an explicit recoverable workflow, and never silently fall back to plaintext persistence.

If the platform vault is missing, locked, or unavailable, AutoHarness remains usable offline and offers environment or session-only authentication.
It does not create an insecure application-owned fallback store.
Workspace configuration may select only an allowlisted non-secret profile name or default, and it cannot supply credentials or weaken permission and secret policy.

This proposal extends ADR-0005 rather than invalidating its ephemeral overlay.
If accepted and implemented, ADR-0005 remains the contract for session-only entry while this ADR becomes the contract for opt-in persistence.

## Consequences

### Positive

- Interactive users can reconnect after restart without repeatedly pasting a key.
- AutoHarness continues to keep plaintext credentials out of its replay, settings, and content stores.
- Provider profiles can be managed independently from conversation sessions.
- Platforms without a usable vault fail safely to offline, environment, or session-only operation.

### Negative

- Vault APIs, availability, unlock behavior, and packaging differ across supported operating systems.
- Cross-platform automated tests require a fake vault plus platform-specific smoke coverage.
- Updating a vault entry and its non-secret profile reference cannot be one cross-system transaction, so replace and delete flows require explicit recovery semantics.
- Headless systems may not provide Linux Secret Service and will continue to need environment or session-only credentials.

## Follow-up

- Accept, revise, or reject this proposal before implementing Phase 3.3 credential persistence.
- Define the versioned provider-profile schema and settings precedence separately from raw secret storage.
- Specify recovery for partial save, replace, and delete operations across the vault and non-secret profile store.
- Add sentinel tests for settings, SQLite, logs, telemetry, crash output, exported sessions, and model-visible provider history.
- Verify restrictive access controls for every non-secret profile and settings file created by AutoHarness.

## Related decisions

- [ADR-0001](0001-use-rust-modular-monolith.md)
- [ADR-0005](0005-use-ephemeral-in-app-credentials.md)
- [ADR-0007](0007-use-durable-capability-tool-runtime.md)
