# Settings, profiles, and credential handling

**Status:** Current settings, credential, and local-preference contract through Phase 3.8.

This document describes how AutoHarness resolves settings, stores provider profiles and local preferences, and handles credentials.
The durable decisions are [ADR-0009](../adr/0009-use-os-backed-provider-credential-profiles.md) for the credential vault, [ADR-0012](../adr/0012-use-typed-settings-resolver.md) for layered settings resolution, and [ADR-0013](../adr/0013-use-durable-credential-mutation-recovery.md) for cross-system mutation recovery.

## Layered settings resolution

`autoharness-settings` resolves five layers in fixed precedence:

1. Built-in defaults.
2. User settings document (`autoharness.profiles.json` in the data directory).
3. Workspace settings file.
4. Environment variables.
5. Command-line and in-app overrides.

Every effective value records which layer supplied it through a provenance map.
A malformed layer is skipped with a safe diagnostic instead of failing startup.
A future schema version fails closed with an explicit error.
Workspace files may override only explicitly permitted appearance, accessibility, and terminal-presentation preferences.
Workspace files may not override local display identity, provider, profiles, active profile, internal credential recovery state, approvals, retention, telemetry, sandbox policy, or credentials.

The profile document carries schema version 3 and is written atomically through a temporary file plus rename.
Schema-v1 and schema-v2 documents migrate on their next mutation.
Schema 3 adds typed non-secret local preferences while retaining optional profile default models and credential recovery records.
An unparseable or invalid existing document is renamed to `autoharness.profiles.json.bad` and replaced with defaults so AutoHarness remains usable.
Future schema documents remain intact and fail closed.

## Local profile and preferences

The application-owned profile document is also the one durable store for a local display label and terminal preferences.
There is no second preferences file or profile store.
Every local preference is optional in a layer, so an absent user value inherits a permitted lower-layer value or the built-in default.
The Settings route offers reset to inherited by clearing the user-layer value and reset to default by writing the built-in default at the user layer.

The typed persisted preferences are:

- Theme preset.
- Color mode: color, no color, or high contrast.
- Glyph mode: Unicode or ASCII chrome.
- Reduced motion.
- Density: comfortable or compact.
- Layout: responsive or single column.
- Terminal timestamp style: relative, absolute, or hidden.
- Composer submission behavior: Control-S or Enter.

The terminal receives a safe effective projection with each value, source, and explanation.
The TUI emits typed preference changes only.
The application validates display labels, atomically updates the document, resolves the new projection, and republishes it before the TUI acknowledges the action.

## Provider profiles

A profile is a named record containing:

- The provider kind (`gemini`, `router`, or `codex_cli`).
- Non-secret connection fields such as the router base URL, project identity, authentication header name, and relative endpoint paths.
- An optional default model identifier.
- An optional opaque credential reference for API-key providers.

Profile names are validated, bounded values (`ProfileId`), as are references (`CredentialReference`).
References never contain credential material; they only name a vault entry such as `autoharness/profile/<profile>`.

## Credential sources

At launch the application resolves exactly one effective credential source:

1. **Environment:** `GEMINI_API_KEY` or `AUTOHARNESS_ROUTER_API_KEY` wins outright for managed API-key launches.
2. **Credential vault:** the active API-key profile's reference is resolved through the operating-system vault.
3. **Session-only:** nothing persisted applies; the user may paste a key per session under [ADR-0005](../adr/0005-use-ephemeral-in-app-credentials.md).

The `codex_cli` provider is different.
It uses only the user's authenticated official Codex CLI session and never reads, stores, or accepts Codex subscription tokens.
Run `codex login` outside AutoHarness, then test or activate the profile.

A missing or locked vault entry degrades to session-only operation rather than blocking offline use.
AutoHarness never creates its own encrypted fallback store.
The effective source is displayed in safe terms in both the `Ctrl+,` provenance overlay and the Connected Accounts workspace.

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
The Providers workspace is available through `Ctrl+G`, `/provider`, or the Settings tab.
It lists Gemini, Google AI Studio API, Cursor, Codex, Claude Code, and OpenAI-compatible API choices.
Gemini and Google AI Studio API share the same Gemini API-key adapter and open the API-key setup form.
Codex opens the subscription authentication page, retains no AutoHarness credential, and invokes the official Codex CLI only after its user-owned `codex login` through the documented read-only, ephemeral JSONL boundary under [ADR-0014](../adr/0014-use-codex-cli-subscription-boundary.md).
Cursor and Claude Code choices name their official CLI login commands but remain unavailable until equivalent repository-owned process adapters exist, rather than claiming a saved or invokable account.
The Agents workspace selects a connected provider, then a compatible model, then provider-default thinking when the catalog positively advertises thinking support.
Providers that do not advertise portable thinking levels never receive an invented effort setting.
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
