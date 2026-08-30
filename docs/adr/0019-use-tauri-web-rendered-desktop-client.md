# ADR-0019: Use a Tauri web-rendered desktop client

**Status:** Accepted

**Date:** 2026-08-30

**Owners:** Project maintainers

## Context and problem statement

AutoHarness has proven its engine, provider, storage, tool, credential, and memory contracts through a sophisticated Ratatui client.
The terminal grid now limits the product's ability to add rich document rendering, resizable workspaces, visual inspection, drag and drop, accessible semantic controls, and extensible feature surfaces.
The terminal presentation layer also requires application orchestration to import client contracts from `autoharness-tui`, which makes a second client harder to add cleanly.

DeepSeek Harness demonstrates the useful architecture for this transition even though it currently ships a browser Web UI rather than an official desktop application.
Its host remains authoritative, its React-free client models sit behind typed communication, and React feature packages compose through explicit presentation seams.
AutoHarness needs the same separation while preserving its Rust modular monolith, native local distribution, and operating-system credential boundary.

## Decision drivers

- Complex interaction and information design must not be constrained by terminal cells.
- The existing Rust engine, provider, store, tool, memory, and settings crates must remain authoritative and renderer independent.
- The interface needs semantic accessibility, mature browser layout, rich text and code rendering, virtualization, and strong automated testing.
- Distribution must remain local, native, cross-platform, and free of a required Node sidecar or localhost service.
- Credentials, permissions, database handles, provider clients, filesystem authority, and tool execution must never move into frontend code.
- A future daemon or remote client must be able to reuse the logical client contract over another carrier.
- The migration must preserve a working TUI until the GUI proves parity, recovery, accessibility, and release readiness.

## Considered options

1. Continue expanding Ratatui.
2. Build a pure Rust GUI with egui, iced, or Slint.
3. Build a browser UI served by a local HTTP server.
4. Build an Electron application around the Rust runtime.
5. Build a Tauri 2 desktop application with React, TypeScript, and Vite.

## Decision outcome

Chosen option: **Tauri 2 with React, TypeScript, and Vite, embedding the existing Rust application runtime in the same desktop process**.

The GUI follows a strict dependency path:

```text
Rust domain and durable runtime
        -> renderer-neutral client contract
        -> transport adapter and React-free client store
        -> presentation adapters and feature slots
        -> React components
```

User actions travel in the opposite direction through typed commands.
Components do not receive provider clients, storage ports, credential vaults, tool runtimes, or the application coordinator.

Tauri IPC is the first physical carrier.
The logical contract uses versioned commands, complete snapshots, monotonic revisions, bounded ordered frames, request correlation, and explicit resynchronization.
The contract must remain transport neutral so a later WebSocket or daemon carrier can implement it without changing business semantics.

Rust remains the only authority for persistence, provider activity, permissions, secrets, capability enforcement, and lifecycle recovery.
The frontend owns only presentation state such as the active route, pane sizes, local drafts, disclosure state, and transient animation.

The first GUI does not load remote content or arbitrary third-party JavaScript.
Extension surfaces begin as schema-validated data, actions, and slots rendered by trusted components.
Any future arbitrary UI plugin requires a separate sandbox and threat model.

The current TUI is frozen as a compatibility and parity reference.
It remains available until the GUI passes the migrated release gates, after which the GUI becomes the default and Ratatui-specific code can be removed deliberately.

## Consequences

### Positive

- AutoHarness gains the layout, editing, rendering, accessibility, and extension ceiling of the web platform.
- Rust keeps the existing safety, durability, provider, memory, and capability boundaries.
- Tauri uses the operating-system webview and avoids a required Chromium or Node runtime bundle.
- React feature packages can evolve independently behind typed slots and client services.
- Browser-mode component tests and desktop end-to-end tests can complement the existing Rust replay and integration suites.
- The logical client protocol prepares the project for a later local daemon or remote client without requiring process separation now.

### Negative

- Windows WebView2, macOS WKWebView, and Linux WebKitGTK require a real cross-platform compatibility and screenshot matrix.
- The project gains a controlled TypeScript and package-manager supply chain.
- HTML credential entry temporarily places secret text in webview memory and therefore needs stricter ingress, clearing, logging, storage, and crash-diagnostic tests.
- Packaging, signing, installers, updates, and desktop lifecycle behavior add new release work.
- The migration temporarily carries two presentation adapters.

### Follow-up

- Introduce the renderer-neutral client protocol and remove application orchestration's dependency on terminal presentation types.
- Expose one reusable application runtime lifecycle to both clients.
- Prove a real startup, session, model, prompt, stream, cancellation, permission, shutdown, and restart vertical slice before broad page parity work.
- Generate GUI theme tokens from one renderer-neutral source while retaining terminal color-depth and glyph logic only in the legacy adapter.
- Add GUI security, accessibility, long-session performance, packaged-app, visual, and cross-platform release gates.
- Make the GUI the default only after the parity and release checklist is approved.

## Evidence

- [DeepSeek Harness GUI reference review](../research/deepseek-harness-gui-patterns.md)
- [DeepSeek Harness GUI layering and RPC protocol at the reviewed commit](https://github.com/deepseek-ai/deepseek-harness/blob/0a53fb55bea101816fa226bb964ae2bed71c343b/.agents/notes/archived/architecture/2026-07-19-gui-layering-and-rpc-protocol.md)
- [DeepSeek Harness Web Client architecture at the reviewed commit](https://github.com/deepseek-ai/deepseek-harness/blob/0a53fb55bea101816fa226bb964ae2bed71c343b/docs/subsystems/web-client.md)
- [DeepSeek Harness Web application manifest at the reviewed commit](https://github.com/deepseek-ai/deepseek-harness/blob/0a53fb55bea101816fa226bb964ae2bed71c343b/apps/web/package.json)
- [Tauri 2 architecture](https://v2.tauri.app/concept/architecture/)
- [Tauri command and Channel documentation](https://v2.tauri.app/develop/calling-rust/)
- [Tauri security capabilities](https://v2.tauri.app/security/capabilities/)
- [Tauri webview version matrix](https://v2.tauri.app/reference/webview-versions/)
- [GUI architecture](../architecture/GUI.md)
- [GUI implementation plan](../design/GUI_IMPLEMENTATION_PLAN.md)

## Related decisions

- [ADR-0001](0001-use-rust-modular-monolith.md) remains authoritative for Rust and the modular-monolith boundary, while this record replaces its Ratatui presentation choice.
- [ADR-0005](0005-use-ephemeral-in-app-credentials.md) remains authoritative for ephemeral non-persistence and zeroization, while this record replaces its terminal-only editor mechanism.
- [ADR-0016](0016-use-typed-tui-presentation-layer.md) is superseded for future product presentation work, while its tested theme and accessibility principles remain migration inputs.
