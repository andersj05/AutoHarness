# GUI implementation plan

**Status:** Active

**Last updated:** 2026-09-02

**Decision:** [ADR-0019](../adr/0019-use-tauri-web-rendered-desktop-client.md)

**Architecture:** [GUI architecture](../architecture/GUI.md)

## Goal

Replace the terminal as AutoHarness's primary product interface with a beautiful, accessible, expandable desktop GUI while preserving every durable runtime, provider, credential, memory, permission, and recovery invariant.

The migration ships in complete vertical slices.
The TUI remains a frozen parity reference until the GUI passes its own release gate.

## Reference patterns

The implementation adopts these patterns from DeepSeek Harness:

- Host authority followed by a typed transport boundary.
- React-free client models between transport and components.
- Separate durable session history from transient control state.
- Complete baselines followed by ordered increments and explicit gap repair.
- Request-correlated optimistic echoes retired only by authoritative observation.
- Feature composition through typed presentation slots.
- Pure model and layout tests plus real-browser and long-session tests.

AutoHarness does not copy DeepSeek Harness's Node host, localhost HTTP carrier, Cordis plugin graph, or arbitrary browser plugin loading.

## Stage 0: Decision and migration boundary

Deliverables:

- Accept the Tauri, React, TypeScript, and Vite desktop decision.
- Mark future TUI presentation work superseded without rewriting its historical evidence.
- Define the GUI architecture, security boundary, design system, implementation stages, and release gate.
- Freeze net-new Ratatui product features except fixes required to preserve the migration reference.

Exit criteria:

- The ADR and documentation map agree on the new primary interface.
- Existing terminal release gaps remain recorded as historical terminal evidence rather than being claimed complete.

## Stage 1: Renderer-neutral client contract

Deliverables:

- Introduce `autoharness-client` with versioned commands, projections, notices, frames, bounds, and request identifiers.
- Separate serializable public data from zeroizing secret ingress.
- Move application-owned client ports and projection contracts out of the TUI dependency direction.
- Add complete snapshot, monotonic transport revision, gap detection, and resynchronization semantics.
- Coalesce high-frequency updates at the carrier boundary.

Exit criteria:

- `autoharness-app` orchestration no longer imports renderer or Ratatui types.
- Both the TUI and GUI consume the same renderer-neutral contract.
- Schema, redaction, request-correlation, overflow, gap, and resync tests pass.

## Stage 2: Desktop shell and real chat vertical slice

Deliverables:

- Add a Tauri 2 desktop shell with local assets and a strict capability policy.
- Add a React, TypeScript, and Vite client with a React-free store and a fixture transport for browser-only development.
- Render the active durable session, offline state, selected model, and catalog.
- Support ephemeral credential entry, model refresh and selection, prompt submission, committed streaming text, cancellation, retry, and exact permission allow or deny.
- Preserve clean shutdown and restart replay through the real runtime.

Exit criteria:

- One packaged or development desktop run completes the full startup-to-restart journey.
- A permission request always preempts ordinary interaction.
- Credential sentinels are absent from durable files, logs, frontend output, browser storage, and diagnostics.
- Window close during provider or tool work follows existing conservative recovery semantics.

## Stage 3: Desktop design system

Deliverables:

- Generate CSS custom properties from the renderer-neutral theme seeds and semantic tokens.
- Preserve all nine themes and the intent of soft, vivid, no-color, and high-contrast treatments.
- Add semantic typography, spacing, elevation, radii, focus, motion, and responsive tokens.
- Build shared primitives for buttons, fields, chips, menus, dialogs, command palette, split panes, virtual lists, callouts, tool cards, meters, and status surfaces.
- Add reduced-motion and keyboard-first behavior from the first component.

Exit criteria:

- Contrast, focus visibility, reduced motion, and semantic-state redundancy pass automated checks.
- Compact, standard, and wide shell screenshots receive pixel-level review on every system webview.

## Stage 4: Session and workspace parity

Deliverables:

- Port session search, creation, switching, rename, archive, unarchive, export, and confirmed deletion.
- Add virtualized long transcripts, resizable navigation, an optional inspector, and retained pane state.
- Add command palette, transcript search, copy, export, and rich tool disclosure.
- Preserve per-session drafts and request-correlated optimistic messages.

Exit criteria:

- The complete session lifecycle passes shutdown and restart tests.
- Long-session update and paint cost remains bounded by the visible window plus the changed item.
- Destructive actions expose exact scope and require explicit confirmation.

## Stage 5: Providers, profiles, models, and credentials

Deliverables:

- Port active profile, provider connection, model default, and reasoning controls.
- Port Gemini, router, and Codex subscription connection flows.
- Use operating-system dialogs only through narrow Rust-owned commands.
- Add connection diagnostics that remain content free and secret safe.

Exit criteria:

- Ordinary connection and rotation workflows require no shell or database access.
- Environment, vault, and session-only precedence remains identical to the Rust authority.
- Windows, macOS, and Linux credential-vault smokes pass through the GUI.

## Stage 6: Settings, accessibility, and personalization

Deliverables:

- Port all renderer-relevant settings with provenance and reset controls.
- Separate terminal-only preferences from cross-client preferences through a versioned settings migration.
- Add system theme integration, zoom, font size, density, motion, contrast, timestamp, and submission behavior.
- Add complete keyboard navigation, focus restoration, landmarks, labels, announcements, and screen-reader ordering.

Exit criteria:

- Every GUI setting can be inspected, changed, explained, and reset inside the application.
- Keyboard-only and screen-reader journeys cover the primary routes and every security-critical dialog.

## Stage 7: Memory and advanced workspaces

Deliverables:

- Port Memory search, filters, paging, provenance, evidence, admissions, proposal review, correction, retraction, export, and deletion.
- Add rich provenance timelines, relation views, and safe diff rendering without changing memory authority.
- Add plan, artifact, file, diff, terminal-output, and evaluation surfaces only through typed feature slots.

Exit criteria:

- The existing memory lifecycle remains replay equivalent through the GUI.
- Model-authored and imported proposals remain visibly untrusted until a distinct approval revision commits.
- Rich content renders inertly and cannot inject HTML, scripts, styles, or commands.

## Stage 8: Release, default cutover, and TUI retirement

Deliverables:

- Add signed installers, update policy, packaged-app lifecycle tests, and a GUI release checklist.
- Run system-webview screenshots and end-to-end journeys on Windows, macOS, and Linux.
- Complete provider, vault, migration, rollback, recovery, security, performance, accessibility, and human visual review gates.
- Make the GUI the default application after approval.
- Remove Ratatui and PTY-specific infrastructure only after the rollback window closes.

Exit criteria:

- No P0 or P1 defect remains in onboarding, chat, sessions, profiles, credentials, settings, permissions, memory, recovery, accessibility, or rendering.
- One committed release candidate passes every migrated baseline and GUI-specific gate.
- The release checklist, rollback evidence, and default-interface promotion are approved.

## First implementation slice

The migration began with Stages 0 through 2 in a deliberately bounded scope:

1. Establish the versioned GUI client protocol and secret ingress split.
2. Add the Tauri and React workspace with fixture-mode browser development.
3. Connect the desktop shell to the existing bounded coordinator ports through a temporary compatibility adapter.
4. Prove active-session, catalog, model selection, prompt, stream, cancel, retry, permission, and restart behavior.
5. Continue extracting the complete shared client contract until the coordinator no longer imports terminal-owned types.

The compatibility adapter is migration scaffolding, not the final dependency direction.
It must remain explicit in code and memory until Stage 1 exits.

## Current slice evidence

The current implementation completes the Stage 3 design-system slice, the Stage 4 session and workspace slice, the Stage 5 provider-management slice, and the Stage 6 settings and accessibility slice locally.
The schema-v3 client contract, settings schema-v5 migration, Tauri carrier, React-free store, responsive desktop shell, deterministic fixtures, permission preemption, ordered recovery, complete locked Rust workspace gates, complete GUI test suite, and the Windows operating-system vault smoke are verified locally.
The Providers workspace creates and edits Gemini and router profiles, duplicates non-secret configuration, activates and tests exact connections, manages environment, vault, and session-only credential states, saves model and reasoning defaults atomically, runs request-correlated native Codex sign-in, and requires exact confirmation for deletion.
Secret entry clears before transport, only a dedicated zeroizing native ingress accepts it, temporary session-default rows cannot write to the vault, environment overrides remain visibly authoritative, and diagnostics stay content free.
Focused browser review covers the provider workspace at compact, mobile-resilience, and wide layouts, including navigation, action wrapping, credential actions, fallback messaging, and destructive controls.
The Settings workspace exposes every renderer-relevant preference with effective provenance, hidden-override explanation, and reset, while the Rust host remains the only persistence authority.
Keyboard integration covers all five primary routes with focus restoration, and semantic review covers the permission and credential dialogs in screen-reader order.
Focused live browser review covers standard and mobile layouts, system appearance, high contrast, 200 percent zoom, responsive inspector behavior, and a clean browser console.
Default pull-request CI now gates renderer-neutral Rust, desktop-host, frontend, documentation, and storage-benchmark coverage without running the frozen TUI package or ignored PTY acceptance matrix.
The terminal tests remain available for deliberate local migration-reference checks until final retirement.

The current implementation does not complete Stage 1 because application orchestration still maps TUI-owned projections through a temporary adapter.
It does not complete Stage 2 because the full real-provider startup-to-restart journey, packaged application lifecycle, cross-platform system-webview evidence, and crash-interruption matrix remain open.
Stage 5's cross-platform exit evidence remains open because macOS and Linux GUI-host credential-vault smokes have not run on this Windows branch.
Stage 6 is complete locally, while cross-platform system-webview accessibility review remains part of the release evidence.
Stages 7 and 8 remain planned migration work.
