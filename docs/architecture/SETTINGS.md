# Settings, profiles, and credential handling

**Status:** Current settings, credential, and local-preference contract through Phase 3.8.

This document describes how AutoHarness resolves settings, stores provider profiles and local preferences, and handles credentials.
The durable decisions are [ADR-0009](../adr/0009-use-os-backed-provider-credential-profiles.md) for the credential vault, [ADR-0012](../adr/0012-use-typed-settings-resolver.md) for layered settings resolution, [ADR-0013](../adr/0013-use-durable-credential-mutation-recovery.md) for cross-system mutation recovery, and [ADR-0015](../adr/0015-use-native-codex-subscription-adapter.md) for native Codex subscription authentication.

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

The profile document carries schema version 4 and is written atomically through a temporary file plus rename.
Schema-v1, schema-v2, and schema-v3 documents migrate on their next mutation.
Schema 3 added typed non-secret local preferences, and schema 4 adds an optional validated default reasoning effort beside each profile default model.
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
- An optional provider-native default reasoning effort.
- An optional opaque credential reference for providers with persisted credentials.

Profile names are validated, bounded values (`ProfileId`), as are references (`CredentialReference`).
References never contain credential material; they only name a vault entry such as `autoharness/profile/<profile>`.

## Credential sources

At launch the application resolves exactly one effective credential source:

1. **Environment:** `GEMINI_API_KEY` or `AUTOHARNESS_ROUTER_API_KEY` wins outright for managed API-key launches.
2. **Credential vault:** the active profile's reference is resolved through the operating-system vault.
3. **Session-only:** nothing persisted applies; the user may paste a key per session under [ADR-0005](../adr/0005-use-ephemeral-in-app-credentials.md).

The `codex_cli` identifier is retained for settings compatibility, but the provider no longer requires an installed Codex CLI.
The Providers wizard starts a native PKCE browser flow, receives the bounded loopback callback, stores one opaque OAuth payload in the operating-system vault, and activates the resulting Codex profile automatically.
The adapter refreshes expiring credentials into the same vault entry and keeps token material in zeroizing process memory.

A missing or locked vault entry degrades to session-only operation rather than blocking offline use.
AutoHarness never creates its own encrypted fallback store.
The effective source is displayed in safe terms in both the `Ctrl+,` provenance overlay and the Connected Providers workspace.

## Credential-vault port

`autoharness_app::vault` defines the port with two implementations:

- `KeyringVault`: Windows Credential Manager, macOS Keychain, and Linux Secret Service through the `keyring` crate, namespaced under the service name `AutoHarness`.
- `FakeVault`: an in-process implementation for tests.

Secrets are validated (non-empty, bounded at 32768 bytes, visible ASCII) before storage and returned in zeroizing strings.
The operating-system adapter stores oversized opaque payloads as generation-scoped bounded chunks behind one manifest entry so Windows Credential Manager limits do not leak into the provider contract.
Vault errors never include secret material.

## Profile management boundary

The application owns one serialized profile-management workflow.
The TUI consumes safe profile and connection read models and emits typed intents; it never calls the settings file, operating-system vault, or provider adapters directly.
Secret-bearing save and replace intents remain ephemeral, non-serializable, zeroizing, and redacted in debug output.
The Providers workspace is available through `Ctrl+G`, `/provider`, or the Settings tab.
It lists Gemini, Google AI Studio API, Cursor, Codex, Claude Code, and OpenAI-compatible API choices.
Gemini opens the named API-key setup form.
Google AI Studio API creates its non-secret Gemini profile and then opens the existing masked credential dialog, storing the pasted key only through the operating-system vault rather than a plaintext `.env` file.
Codex opens a dedicated browser-login wizard with one sign-in action.
Pressing Enter opens the default browser directly without requiring a `codex` executable.
Successful authentication stores the opaque token payload in the operating-system vault, writes only the profile's opaque reference to settings, activates the profile, and loads the native Codex catalog under [ADR-0015](../adr/0015-use-native-codex-subscription-adapter.md).
The login can be cancelled from the TUI, stale callback results are ignored, and safe failure copy never claims that a browser opened when launch failed.
Cursor and Claude Code choices name their official CLI login commands but remain unavailable until equivalent repository-owned process adapters exist, rather than claiming a saved or invokable account.
The Models tab in Settings lists compatible models for the active provider profile, clearly marks the saved default, and preselects the saved model and reasoning effort.
The provider is changed separately in the Providers tab, so model selection never races profile activation or catalog replacement.
When the selected model advertises thinking support, the Models tab offers a validated provider-native reasoning effort before saving.
The selected model and effort are persisted together, and a newly created session durably selects that model before its first session projection is published.
Catalog refresh applies the profile default only when a fresh session has no selected model, and never overwrites an intentional session-specific choice.
Provider-default effort remains available when the user does not want an override.
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
