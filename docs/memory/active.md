# Active memory

**Last reviewed:** 2026-09-02

**Phase:** Native GUI migration Stage 4 session and workspace parity

**Status:** The first renderer-neutral client, Tauri carrier, and React desktop slice are merged into `dev`; Stage 4 session and workspace parity is implemented on `feat/gui-session-workspace`, its complete local gates pass, and migration release gates remain open

## Current objective

Finish the staged native GUI migration without making it the default client or claiming terminal parity.
Keep Rust authoritative for durability, providers, credentials, permissions, tools, memory, and recovery.
Retain the TUI as the compatibility and behavioral reference until the GUI release gate is complete.

## Current repository state

- [ADR-0019](../adr/0019-use-tauri-web-rendered-desktop-client.md) selects Tauri 2, React, TypeScript, and Vite for the native desktop client.
- [`autoharness-client`](../../crates/autoharness-client/src/lib.rs) defines schema-v1 renderer-neutral commands, snapshots, bounded active-session deltas, notices, bounded permission projections, request correlation, monotonic transport revisions, secret ingress, and resynchronization.
- [`autoharness-app::gui`](../../crates/autoharness-app/src/gui.rs) embeds the authoritative runtime in the Tauri process and exposes only narrow client commands, ordered frames, acknowledgements, and one-way credential ingress.
- The bridge keeps one bounded frame in flight, coalesces projections, gives acknowledgements a dedicated mailbox, requires a process restart after an unacknowledged renderer replacement, and publishes shutdown lifecycle before terminal notices.
- The React workspace under [`apps/gui`](../../apps/gui/package.json) owns presentation state only and uses a React-free client store between components and the native or fixture transport.
- The initial shell provides responsive navigation, active-session chat, catalog and model selection, prompt composition, stream and cancellation state, retry, exact permission review, ephemeral credential entry, offline recovery, and deterministic fixture scenarios.
- [`autoharness-presentation`](../../crates/autoharness-presentation/src/lib.rs) is the renderer-neutral source for nine theme seeds, five color treatments, semantic color ramps, and contrast floors consumed by both GUI CSS generation and the TUI adapter.
- The GUI has semantic typography, spacing, elevation, radii, focus, motion, responsive, control-size, and stacking tokens plus shared transport-free primitives for buttons, fields, chips, menus, dialogs, command palette, split panes, virtual lists, callouts, tool cards, meters, and status surfaces.
- The live shell exposes all appearance combinations, native and explicit reduced motion, `Ctrl+K` command navigation, a keyboard-resizable context split, and virtualized session rows while preserving permission preemption.
- The Sessions workspace searches titles and identities, filters open and archived rows, switches, renames, archives, restores, exports, and deletes exact sessions through the Rust-owned lifecycle commands.
- Permanent deletion names the exact session, explains export-before-delete behavior, and stays disabled until the user types the complete session title.
- Chat mounts at most 36 transcript rows, searches messages and rich tool evidence, copies plain text, requests host-owned Markdown export, and expands a matching tool disclosure.
- Any selected open or archived session can be exported directly without changing the active conversation.
- Navigation and inspector panes are keyboard and pointer resizable and retain their values across route changes.
- Optimistic prompts are keyed by Rust-issued request identifiers and retire only after the matching durable user row is observed or the request is rejected.
- Active-session streaming crosses the native carrier as a bounded transcript splice, preserving unchanged client rows instead of serializing total transcript history for every update.
- Permission review uses one injective visible encoding for controls, directional formatting, default-ignorable characters, and literal backslashes.
- The permission wire contract losslessly covers the built-in tool planner's maximum argument count and worst-case safe-display expansion while retaining an aggregate byte bound.
- Session-only credential sentinels remain zeroized and participate in cross-delta output rejection without entering durable state.
- Prompt and retry commands settle as committed after their durable admission boundary even when provider startup subsequently fails.
- GUI catalog projection bounds provider-authored labels, details, and row count so malformed remote presentation data cannot deny client startup.
- Saved inactive profiles coexist with one synthetic active default connection when no named profile is active.
- The desktop icon and platform icon set derive from [`icon-source.png`](../../crates/autoharness-app/icons/icon-source.png).
- Browser fixture review covers ready, streaming, offline, credential, permission, failure, empty, compact, standard, wide, resilience, no-color, and high-contrast states.
- A real Windows Tauri development window launched against the Rust host, rendered the shared desktop shell and keyboard command palette in WebView2, and exited cleanly.
- Rust formatting, strict workspace Clippy, the complete locked Rust suite, the real session lifecycle shutdown and restart PTY journey, frozen frontend lock verification, generated-theme freshness, frontend type checking, 79 GUI tests, the production Vite build, documentation links, and focused visual review pass locally for Stage 4.
- The final independent client, bridge, coordinator, frontend, Tauri, package, and CI audit reports no remaining actionable P0 through P2 findings.

## Open migration work

- `autoharness-app` still maps temporary TUI-owned projections into the renderer-neutral contract.
- Stage 1 exits only after application orchestration no longer imports renderer-owned types and both clients consume the shared contract directly.
- The GUI does not yet provide complete Profiles, Settings, Help, Memory, import, Codex login, or router configuration parity.
- Renderer restart recovery currently requires restarting the desktop process when an earlier native frame remains unacknowledged.
- Packaging, signing, updates, installers, macOS and Linux system-webview screenshot matrices, and Windows, macOS, and Linux packaged-app tests remain open.
- Windows WebView2 received a live wide-shell and command-palette review, while the exact compact, standard, and wide viewport matrix is currently browser-fixture evidence only.
- The GUI is not the default application and `bundle.active` remains false.
- Existing Phase 3.9, Phase 3.10, and Phase 4 release evidence gaps remain open, including cross-platform vault smokes, live router evidence, approved reference-machine reports, human review, rollback, checklist, approval, and promotion.

## Immediate next actions

1. Collect exact compact, standard, and wide native screenshot evidence on Windows, macOS, and Linux for the Stage 3 and Stage 4 workspaces.
2. Implement Stage 5 provider, profile, model-default, reasoning, and credential parity without widening webview authority.
3. Continue Stage 1 and Stage 2 migration cleanup by removing the temporary TUI projection adapter and extending native runtime restart journeys.
