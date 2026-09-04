# GUI architecture

**Status:** Accepted migration target

**Last updated:** 2026-09-03

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

The protocol is currently at schema version 4.
Schema version 2 added typed provider profiles, non-secret connection configuration, profile scope, credential state, model defaults, reasoning effort, and native Codex authentication commands without adding secret material to serializable frames.
Schema version 3 added the authoritative renderer-relevant settings projection and typed user-layer preference changes.
Schema version 4 adds bounded Memory inspection, exact revision-scoped lifecycle commands, and recoverable memory-page failures.
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
- Change or clear one renderer-relevant user preference through the Rust settings authority.
- Query, remember, import, correct, approve, reject, retract, export, or delete memory through the existing coordinator and ledger authority.

### Frames

The first carrier sends:

- A complete startup snapshot.
- Bounded active-session deltas for changes confined to the current session.
- Coalesced projection snapshots when state outside the active session changes.
- Correlated commit, rejection, and authentication notices.
- An explicit resynchronization snapshot when the client detects a gap.
- Bounded provider projections that distinguish named profiles from a temporary session default and expose only safe connection, credential-source, default, health, and recovery metadata.
- Effective GUI settings with values, provenance, and explicit user-override presence.
- Query-correlated Memory pages with durable mutation generation, exact revision guards, evidence availability, provenance, relations, validation findings, and bounded admissions.

Each frame carries a monotonic transport revision.
Durable session revisions remain visible inside session data and are not replaced by the transport revision.

An active-session delta carries the exact session identity, updated session summary and revision, selected model, permission requests, and one transcript splice.
The splice retains the unchanged prefix and suffix and serializes only inserted or replaced transcript rows.
The client rejects a delta whose identity or splice range does not match its authoritative baseline.
Ordinary streaming therefore scales with the changed transcript item, while a complete snapshot remains the recovery and cross-session baseline.

### Settings authority

The renderer consumes effective theme, color mode, zoom, font size, reduced motion, density, timestamp, and composer-submission values from the snapshot.
It may apply those values to presentation immediately, but it does not read or write settings files.
Each Settings control sends one typed preference change to Rust, and clearing a value removes only the user-file override.
Rust persists and resolves the change before a replacement projection becomes authoritative.
Source and user-override fields let the UI explain defaults, inherited workspace or environment values, explicit overrides, and overrides hidden by higher-precedence layers.

### Memory authority

Memory pages contain at most 100 records and 8 MiB of serialized JSON, with individual content and nested collection bounds.
The host converts an unrepresentable page into a safe workspace failure so users can narrow the query without blocking unrelated client commands.
A query carries a view generation independent of the ledger mutation generation, and the renderer only enables actions against the requested current page.
The coordinator applies literal search, scope, lifecycle, and opaque cursor filtering before paging.
All integer sequence and generation values cross the carrier as decimal strings.

Correction, proposal review, retraction, and deletion preserve exact optimistic ledger sequences.
Approval and rejection also name the proposed revision, and retraction names the current revision.
Imported and model-authored proposals remain visibly untrusted until a distinct user-approved revision commits.
Deletion erases retained content and evidence while preserving audit metadata and existing user-owned exports.
Standalone exports remain Rust-owned JSON files beside the database.

The GUI uses the existing confined workspace-relative UTF-8 import command and never receives a generic file reader.
The provenance timeline describes the projected source and current revision; retained historical revisions remain in the ledger and standalone export.
Admission history shows its bounded displayed count against the host's recorded count.

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

The implemented route and inspector slots use closed typed presentation interfaces.
The inspector accepts plan, artifact, file, diff, terminal-output, and evaluation data rendered by trusted components as inert text.
The native producer exposes existing tool evidence only; fixture producers demonstrate the other surface contracts until their authoritative runtimes exist.
No surface supplies HTML, script, styles, executable command callbacks, arbitrary links, or a filesystem capability.
Two-column comparisons preserve complete before and after content without interpreting patch instructions.

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

The frozen TUI and its PTY tests remain available as local migration references until the GUI release checklist explicitly retires them.
They do not gate ordinary GUI migration pull requests.
