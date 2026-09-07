# Active memory

**Last reviewed:** 2026-09-07

**Phase:** GUI-only foundation

## Current objective

Improve the desktop GUI on the validated renderer-neutral foundation authorized by [ADR-0020](../adr/0020-retire-terminal-client.md).
Source integration is tracked by [PR #26](https://github.com/andersj05/AutoHarness/pull/26) and the dedicated `dev` to `main` promotion.
Keep Rust authoritative for durability, providers, credentials, permissions, tools, memory, and recovery.

## Current repository state

- Both executable names select the GUI; the TUI renderer, terminal entry mode, PTY infrastructure, and terminal latency instrumentation are removed.
- `autoharness-client::runtime` owns in-process messages, projections, and bounded channels; the crate root separately defines the schema-v4 wire protocol.
- The coordinator and carrier no longer depend on terminal-owned contracts.
- The desktop provides Chat, Sessions, Providers, Memory, Settings, Help, exact permission review, ephemeral credentials, and inert typed advanced surfaces.
- The `feat/desktop-ux-redesign` branch adds simplified navigation and copy, session search and sorting, safe Markdown responses, focused settings categories, and progressive disclosure; the [UX validation](../release/GUI_UX_REDESIGN_VALIDATION.md) records 130 passing GUI tests, the complete local baseline, browser review, and native-review limits.
- Legacy terminal settings remain readable for existing profile compatibility.
- The [foundation review](../release/GUI_FOUNDATION_REVIEW.md) records the passing local baseline, native Windows lifecycle, palette contrast fix, and alias diagnostics regression.

## Immediate next actions

1. Review and integrate `feat/desktop-ux-redesign` into `dev`, then collect native product and human visual review against the same source.
2. Keep collecting the remaining signed-distribution and cross-platform release evidence independently.

## Remaining release work

Signed same-candidate packages, all-platform native vault and accessibility journeys, live-provider release evidence, reference-machine reports, and human visual approval remain open in the [release checklist](../release/GUI_RELEASE_CHECKLIST.md).
Source cutover does not mark those distribution gates complete.
An unacknowledged renderer replacement still requires restarting the desktop process.
Native planning and evaluation producers remain future runtime work.
Historical terminal milestone evidence is retained in [progress memory](progress.md), with its original gaps unclaimed.
