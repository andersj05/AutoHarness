# Settings, profiles, and credential handling

**Status:** Current credential contract through Phase 3.6.

This document describes how AutoHarness resolves settings, stores provider profiles, and handles credentials.
The durable decisions are [ADR-0009](../adr/0009-use-os-backed-provider-credential-profiles.md) for the credential vault, [ADR-0012](../adr/0012-use-typed-settings-resolver.md) for layered settings resolution, and [ADR-0013](../adr/0013-use-durable-credential-mutation-recovery.md) for cross-system mutation recovery.

## Layered settings resolution

`autoharness-settings` resolves five layers in fixed precedence:

1. Built-in defaults.
2. User settings document (`autoharness.profiles.json` in the data directory).
3. Workspace settings file.
4. Environment variables.
5. Command-line and in-app overrides.

Every effective value records which layer supplied it through a provenance map.
A malformed layer is skipped with a safe diagnostic instead of failing startup; a future schema version fails closed with an explicit error.
Workspace files may not override `provider`, `profiles`, `active_profile`, or internal `credential_recovery` state, and can never supply credentials or weaken permission, retention, telemetry, or sandbox policy.

The profile document carries schema version 2 and is written atomically through a temporary file plus rename.
Schema-v1 documents migrate on their next mutation; schema 2 adds optional profile default models and non-secret credential recovery records.
An unparseable existing document is renamed to `autoharness.profiles.json.bad` and replaced with defaults so AutoHarness remains usable.

## Provider profiles

A profile is a named record containing:

- The provider kind (`gemini` or `router`).
- Non-secret connection fields such as the router base URL, project identity, authentication header name, and relative endpoint paths.
- An optional default model identifier and the current safe-agent interaction mode.
- An optional opaque credential reference.

Profile names are validated, bounded values (`ProfileId`), as are references (`CredentialReference`).
References never contain credential material; they only name a vault entry such as `autoharness/profile/<profile>`.

## Credential sources

At launch the application resolves exactly one effective credential source:

1. **Environment:** `GEMINI_API_KEY` or `AUTOHARNESS_ROUTER_API_KEY` wins outright for managed launches.
2. **Credential vault:** the active profile's reference is resolved through the operating-system vault.
3. **Session-only:** nothing persisted applies; the user may paste a key per session under [ADR-0005](../adr/0005-use-ephemeral-in-app-credentials.md).

A missing or locked vault entry degrades to session-only operation rather than blocking offline use.
AutoHarness never creates its own encrypted fallback store.
The effective source is displayed in safe terms in both the `Ctrl+,` provenance overlay and the `Ctrl+G` Profiles and Providers surface: `environment`, `credential vault`, or `session only`.

## Credential-vault port

`autoharness_app::vault` defines the port with two implementations:

- `KeyringVault`: Windows Credential Manager, macOS Keychain, and Linux Secret Service through the `keyring` crate, namespaced under the service name `AutoHarness`.
- `FakeVault`: an in-process implementation for tests.

Secrets are validated (non-empty, bounded at 4096 bytes, visible ASCII) before storage and returned in zeroizing strings.
Vault errors never include secret material.

## Profile management boundary

The application owns one serialized profile-management workflow.
The TUI consumes safe profile and connection read models and emits typed intents; it never calls the settings file, operating-system vault, or provider adapters directly.
Secret-bearing save and replace intents remain ephemeral, non-serializable, zeroizing, and redacted in debug output.
The full-screen `Ctrl+G` surface supports profile create, edit, duplicate, activate, test, disconnect, and confirmed delete actions.
It also supports explicit vault save or replace, selection of the current compatible model as the active profile default, and content-free connection health results.
Keyboard shortcuts, command-palette routing, and visible controls converge on the same typed intents.

Each profile uses one deterministic vault reference derived from its validated profile identity.
The versioned profile document records bounded non-secret recovery operations around save, disconnect, and delete mutations.
Recovery either completes an exact linked save or idempotently deletes an unlinked vault entry.
Locked or unavailable vaults leave cleanup visibly pending without blocking offline profiles, settings, or sessions.
Environment credentials are visible read-only overrides and are never copied into the vault implicitly.

## Secret-handling guarantees

Raw credentials are structurally excluded from:

- Settings documents and SQLite rows, including session events and projections.
- Logs, tracing output, and telemetry counters.
- Session exports and transcripts.
- Debug formatting of resolver, projection, and vault types.

Sentinel tests seed a unique marker secret through save, rotate, disconnect, and delete flows and scan every durable file plus rendered debug output to prove no leakage.
