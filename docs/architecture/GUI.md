# GUI architecture

**Status:** Accepted migration target

**Last updated:** 2026-09-02

## Purpose

The AutoHarness GUI is a thin, web-rendered desktop client over the existing Rust application runtime.
It expands the presentation ceiling without moving durable state, model execution, credentials, permissions, storage, or tools into the webview.

## System shape

```text
React feature UI
       |
       v
React-free client store and typed actions
       |
       v
Tauri carrier: commands, ordered frames, snapshot resync
       |
       v
Rust application coordinator and projection builders
       |
       +--> headless engine
       +--> providers
       +--> tool runtime
       +--> memory policy
       +--> SQLite and artifacts
       +--> settings and operating-system vault
```

The Tauri carrier is a local adapter, not the business boundary.
The renderer-neutral client contract is the business boundary.

## Ownership rules

### Rust host

Rust owns:

- Durable commands, events, projections, and replay.
- Session, attempt, tool, memory, and permission state.
- Provider discovery, requests, cancellation, and retry policy.
- SQLite, artifacts, settings, profiles, and credential-vault access.
- Workspace and external-effect capability enforcement.
- Request validation, ordering, backpressure, and shutdown.
- Sanitized public failures and secret-redaction boundaries.

### Client model

The renderer-free client model owns:

- The latest validated host snapshot.
- Monotonic frame application and gap detection.
- Stable object identities useful to rendering.
- Optimistic command echoes correlated by Rust-issued request identifiers.
- Replacement from an authoritative snapshot after a gap or reconnect.
- View-neutral selectors and command methods.

It does not decide whether a mutation is allowed or durable.

### React presentation

React owns:

- Routes, panes, drawers, menus, dialogs, and disclosure state.
- Composer drafts before submission.
- Selection, hover, focus, and animation state.
- Layout preferences that are explicitly presentation-only.
- Semantic HTML rendering of trusted projections.

React never imports a Rust runtime implementation and never receives a provider, vault, store, filesystem, process, HTTP, or tool handle.

## Client protocol

The protocol is currently at schema version 2.
Schema version 2 adds typed provider profiles, non-secret connection configuration, profile scope, credential state, model defaults, reasoning effort, and native Codex authentication commands without adding secret material to serializable frames.
All numeric identities that can exceed JavaScript's safe integer range cross the boundary as strings.
Secret inputs use dedicated one-way commands and never appear in snapshots, frames, notices, frontend storage, or diagnostics.

### Commands

Commands express requested intent and receive a Rust-issued request identifier.
A successful command response means the request entered the bounded application mailbox, not that the durable mutation succeeded.
Commit or rejection arrives separately through a correlated notice or durable projection.

The current command set covers:

- Create, open, rename, archive, unarchive, export, or delete an exact session.
- Refresh the model catalog.
- Select a model.
- Submit a prompt.
- Cancel or retry an attempt.
- Answer an exact tool permission request.
- Submit an ephemeral credential through a dedicated secret ingress.
- Create or edit, duplicate, activate, test, disconnect, or delete an exact named provider profile.
- Save model and reasoning defaults together for the active provider profile.
- Start or cancel one request-correlated native Codex subscription authentication flow.

### Frames

The first carrier sends:

- A complete startup snapshot.
- Bounded active-session deltas for changes confined to the current session.
- Coalesced projection snapshots when state outside the active session changes.
- Correlated commit, rejection, and authentication notices.
- An explicit resynchronization snapshot when the client detects a gap.
- Bounded provider projections that distinguish named profiles from a temporary session default and expose only safe connection, credential-source, default, health, and recovery metadata.

Each frame carries a monotonic transport revision.
Durable session revisions remain visible inside session data and are not replaced by the transport revision.

An active-session delta carries the exact session identity, updated session summary and revision, selected model, permission requests, and one transcript splice.
The splice retains the unchanged prefix and suffix and serializes only inserted or replaced transcript rows.
The client rejects a delta whose identity or splice range does not match its authoritative baseline.
Ordinary streaming therefore scales with the changed transcript item, while a complete snapshot remains the recovery and cross-session baseline.

### Ordering and recovery

Notices and watch projections can become observable in either order.
The client correlates requests by identifier and treats the durable projection as authoritative.
It does not assume that an acknowledgement arrives before the visible state change.

If a transport revision is missing, the client stops applying dependent increments and requests a complete snapshot.
Reconnect replaces client state from a fresh baseline before later frames apply.

## Tauri carrier

Tauri is configured with local assets, a strict content security policy, no broad plugins, and one main-window capability.
Development tools are not enabled in release builds.
The webview cannot call generic filesystem, process, HTTP, SQL, or shell commands.

Ordered streaming uses Tauri Channel semantics or an equivalent ordered adapter with bounded host queues.
High-frequency provider output is coalesced to a display cadence so token fragments cannot starve input or serialize the full transcript repeatedly.

The logical protocol does not mention Tauri types.
A future WebSocket, local daemon, or remote carrier must implement the same command, frame, correlation, and resynchronization semantics.

## Credential ingress

Credential fields use ordinary semantic password controls with browser persistence, spellcheck, and autocomplete disabled.
Submitting a credential immediately transfers the owned string to a Rust zeroizing type and clears the DOM value and client state.
The host never echoes credential content or credential length.
The dedicated ingress distinguishes session-only, saved-vault, and replacement operations.
Rust validates the exact target and permits vault writes only for named Gemini or router profiles, while session-only credentials apply only to the active connection.
Environment credentials remain read-only overrides, and a linked vault credential is shown only as a fallback while that override is effective.

Codex subscription credentials never enter the renderer.
The renderer asks Rust to start one native PKCE browser flow and can cancel only the exact Rust-issued request identifier.
Rust owns browser launch, loopback callback handling, token storage, refresh, and safe terminal notices.

Remote assets, analytics scripts, form persistence, local storage, and crash payloads are prohibited on credential surfaces.
Secret-sentinel tests scan application data, Rust logs, packaged frontend output, browser storage, and captured diagnostics.

## Permission preemption

An unresolved durable tool permission request preempts ordinary interaction.
The GUI renders the exact trusted operation details supplied by Rust and offers allow or deny for that frozen call only.
The permission view cannot be hidden behind another modal, route, pane, or animation.

Window close requests cancellation but does not fabricate settlement.
The existing durable recovery rules decide the result after restart.

## Feature composition

The frontend begins as one workspace with explicit feature folders and typed slots.
Features may publish components, actions, and selectors through stable presentation interfaces.
They do not runtime-import another feature's implementation or receive a global host context.

The initial slots are:

- Application rail.
- Conversation header.
- Transcript item renderer.
- Composer accessory row.
- Inspector pane.
- Route workspace.
- Modal layer.

Third-party extensions initially contribute declarative data and actions rendered by trusted components.
Arbitrary JavaScript plugins are out of scope.

## Testing contract

The GUI adds these gates without weakening the Rust gates:

- Protocol schema, request correlation, gap, resync, bounds, and redaction tests.
- Offline startup and replay through the real application runtime.
- Browser-mode component, reducer, keyboard, and accessibility tests.
- Screenshot and geometry review at compact, standard, and wide desktop sizes.
- Long-transcript virtualization and stream-to-paint performance tests.
- Complete session lifecycle tests across shutdown and restart.
- Tauri packaged-app tests on Windows, macOS, and Linux system webviews.
- Credential sentinel, permission preemption, window-close recovery, and crash-interruption tests.
- Keyboard-only and screen-reader smoke reviews.

The TUI and its PTY tests remain until the GUI release checklist explicitly retires them.
