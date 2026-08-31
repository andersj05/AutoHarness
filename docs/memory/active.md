# Active memory

**Last reviewed:** 2026-08-30

**Phase:** Native GUI migration preview on `dev`

**Status:** The first renderer-neutral client, Tauri carrier, and React desktop slice are merged into `dev`, complete local workspace validation passes, and the final independent audit has no actionable P0 through P2 findings; migration release gates remain open

## Current objective

Finish the first bounded native GUI slice without making it the default client or claiming terminal parity.
Keep Rust authoritative for durability, providers, credentials, permissions, tools, memory, and recovery.
Retain the TUI as the compatibility and behavioral reference until the GUI release gate is complete.

## Current repository state

- [ADR-0019](../adr/0019-use-tauri-web-rendered-desktop-client.md) selects Tauri 2, React, TypeScript, and Vite for the native desktop client.
- [`autoharness-client`](../../crates/autoharness-client/src/lib.rs) defines schema-v1 renderer-neutral commands, snapshots, notices, bounded permission projections, request correlation, monotonic transport revisions, secret ingress, and resynchronization.
- [`autoharness-app::gui`](../../crates/autoharness-app/src/gui.rs) embeds the authoritative runtime in the Tauri process and exposes only narrow client commands, ordered frames, acknowledgements, and one-way credential ingress.
- The bridge keeps one bounded frame in flight, coalesces projections, gives acknowledgements a dedicated mailbox, requires a process restart after an unacknowledged renderer replacement, and publishes shutdown lifecycle before terminal notices.
- The React workspace under [`apps/gui`](../../apps/gui/package.json) owns presentation state only and uses a React-free client store between components and the native or fixture transport.
- The initial shell provides responsive navigation, active-session chat, catalog and model selection, prompt composition, stream and cancellation state, retry, exact permission review, ephemeral credential entry, offline recovery, and deterministic fixture scenarios.
- Permission review uses one injective visible encoding for controls, directional formatting, default-ignorable characters, and literal backslashes.
- The permission wire contract losslessly covers the built-in tool planner's maximum argument count and worst-case safe-display expansion while retaining an aggregate byte bound.
- Session-only credential sentinels remain zeroized and participate in cross-delta output rejection without entering durable state.
- Prompt and retry commands settle as committed after their durable admission boundary even when provider startup subsequently fails.
- GUI catalog projection bounds provider-authored labels, details, and row count so malformed remote presentation data cannot deny client startup.
- Saved inactive profiles coexist with one synthetic active default connection when no named profile is active.
- The desktop icon and platform icon set derive from [`icon-source.png`](../../crates/autoharness-app/icons/icon-source.png).
- Browser fixture review covered ready, streaming, offline, credential, permission, failure, empty, compact, standard, and wide states.
- A real Windows Tauri development window launched against the Rust host, rendered the terminal-inspired three-pane workspace, and exited cleanly.
- Rust formatting, strict workspace Clippy, the complete locked workspace suite, frontend lock verification, type checking, 49 component, store, and transport tests, the production Vite build, documentation links, and diff checks pass locally.
- The final independent client, bridge, coordinator, frontend, Tauri, package, and CI audit reports no remaining actionable P0 through P2 findings.

## Open migration work

- `autoharness-app` still maps temporary TUI-owned projections into the renderer-neutral contract.
- Stage 1 exits only after application orchestration no longer imports renderer-owned types and both clients consume the shared contract directly.
- The initial GUI does not yet provide complete Sessions, Profiles, Settings, Help, Memory, import, export, archive, deletion, Codex login, or router configuration parity.
- Whole-session snapshot streaming remains transitional and must become keyed bounded deltas before long-session performance parity.
- Renderer restart recovery currently requires restarting the desktop process when an earlier native frame remains unacknowledged.
- Packaging, signing, updates, installers, accessibility journeys, long-session virtualization, system-webview screenshot matrices, and Windows, macOS, and Linux packaged-app tests remain open.
- The GUI is not the default application and `bundle.active` remains false.
- Existing Phase 3.9, Phase 3.10, and Phase 4 release evidence gaps remain open, including cross-platform vault smokes, live router evidence, approved reference-machine reports, human review, rollback, checklist, approval, and promotion.

## Immediate next actions

1. Continue Stage 1 by moving application projections and client ports out of the temporary TUI compatibility adapter.
2. Implement Stage 2 native startup-to-restart journeys with real prompt, stream, cancellation, permission, credential, and crash-interruption coverage.
