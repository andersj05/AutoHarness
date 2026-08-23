# ADR-0013: Use durable recovery records for credential mutations

**Status:** Accepted

**Date:** 2026-08-23

**Owners:** Project maintainers

## Context and problem statement

A named provider profile is stored in the application-owned non-secret settings document, while its raw credential is stored in the operating-system credential vault under an opaque reference.
The settings file and platform vault cannot participate in one atomic transaction.
Phase 3.6 adds in-terminal save, replace, disconnect, and delete operations, so interruption or failure between the two stores must have deterministic and user-visible recovery.

## Decision drivers

- Raw credentials must never enter application-owned durable files, logs, events, transcripts, telemetry, crash output, or model-visible context.
- A committed profile must never silently reference a different profile's credential.
- Interrupted save, disconnect, and delete operations must be recoverable after process restart.
- Recovery must be idempotent because the operating-system vault cannot expose a portable transaction or listing API.
- Locked or unavailable vaults must not block offline profile and session management.
- Environment and session-only credential sources must remain available without becoming plaintext fallbacks.

## Considered options

1. Treat the two writes as best effort and show an error when the second write fails.
2. Store credentials in the settings document so one file rename can commit both values.
3. Add a second application-owned recovery file beside the profile document.
4. Record non-secret credential mutation recovery state inside the versioned profile document and reconcile it through an application-owned workflow.

## Decision outcome

Chosen option: **record non-secret recovery state in the versioned profile document**, because the document is already the authority for profile existence, credential linkage, and schema migration.

Each named profile uses one deterministic vault reference derived from its validated `ProfileId`.
The profile document stores only that opaque reference and bounded recovery records that contain an operation kind, profile identity, and opaque reference.
Recovery records never contain a credential value, length, fingerprint, provider response, or private endpoint.
The application serializes profile mutations through one `ProfileManager`; the TUI emits typed intents and never calls the profile store or vault directly.

A first save follows this order:

1. Atomically record an `uncommitted_save` recovery record in the profile document.
2. Save the credential under the deterministic vault reference.
3. Atomically link the reference to the exact profile and remove the recovery record.

On recovery, an `uncommitted_save` record is complete only when the exact profile links the exact reference.
Otherwise AutoHarness deletes the deterministic vault entry and then removes the recovery record.
A locked or unavailable vault leaves the record pending and presents a safe repair state.

Disconnect and profile deletion follow this order:

1. Atomically remove the credential link or profile and add a `delete` recovery record in the same profile-document write.
2. Delete the exact vault entry idempotently.
3. Atomically remove the recovery record.

A failed or interrupted deletion therefore leaves no live application link to the credential and retains enough non-secret state to retry cleanup after restart.
Deleting an already absent vault entry succeeds, which makes cleanup repeatable.

Replacing a credential operates on the exact linked vault reference and does not modify the profile document.
A replace failure never creates a new reference or silently falls back to save; the connection becomes safely unverified until an explicit test succeeds.

Connection tests receive a zeroizing credential directly from the entry flow or vault, construct the selected provider adapter outside the TUI, and retain only a bounded safe result state.
Environment credentials remain read-only overrides and are never copied into the vault unless the user explicitly enters a value for saving.

## Consequences

### Positive

- Every cross-system mutation has a deterministic restart path without storing secret bytes in application files.
- Profile deletion cannot leave a durable application link to a credential that the user asked to remove.
- Stable references and idempotent deletion make retries safe across Windows Credential Manager, macOS Keychain, and Linux Secret Service.
- One application workflow owns validation, mutation ordering, recovery, and safe user-facing status.

### Negative

- The profile settings schema must carry internal non-secret recovery records and migrate existing schema-v1 documents.
- Saving a credential requires two atomic profile-document writes around one vault write.
- A locked vault can leave cleanup pending until the user unlocks the platform service or explicitly retries.
- The operating-system vault still defines the atomicity of replacing one existing secret value.

### Follow-up

- Add fake-vault fault injection for every boundary before and after a profile-document or vault mutation.
- Add platform smoke coverage for idempotent save, load, replace, and delete behavior without printing secret values.
- Surface pending recovery in the Profiles and Providers read model without exposing opaque references.
- Preserve sentinel scans across profile files, SQLite, exports, logs, debug rendering, and provider-visible history.

## Evidence

- [Settings and credentials architecture](../architecture/SETTINGS.md)
- [Phase 3.6 plan](../PROJECT_PLAN.md#phase-36-local-profile-and-provider-connection-center)
- [`ProfileStore`](../../crates/autoharness-app/src/profiles.rs)
- [`VaultPort`](../../crates/autoharness-app/src/vault.rs)

## Related decisions

- [ADR-0005](0005-use-ephemeral-in-app-credentials.md)
- [ADR-0009](0009-use-os-backed-provider-credential-profiles.md)
- [ADR-0012](0012-use-typed-settings-resolver.md)
