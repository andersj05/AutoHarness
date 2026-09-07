# Active memory

**Last reviewed:** 2026-09-07

**Phase:** GUI-only foundation review and source integration

## Current objective

Complete the desktop-only source foundation authorized by [ADR-0020](../adr/0020-retire-terminal-client.md), validate it, and integrate through feature-to-dev and dev-to-main pull requests.
Keep Rust authoritative for durability, providers, credentials, permissions, tools, memory, and recovery.

## Current repository state

- Both executable names select the GUI; the TUI renderer, terminal entry mode, PTY infrastructure, and terminal latency instrumentation are removed.
- `autoharness-client::runtime` owns in-process messages, projections, and bounded channels; the crate root separately defines the schema-v4 wire protocol.
- The coordinator and carrier no longer depend on terminal-owned contracts.
- The desktop provides Chat, Sessions, Providers, Memory, Settings, Help, exact permission review, ephemeral credentials, and inert typed advanced surfaces.
- Legacy terminal settings remain readable for existing profile compatibility.
- The [foundation review](../release/GUI_FOUNDATION_REVIEW.md) records current validation status.

## Immediate next actions

1. Finish full local baseline, native lifecycle, and focused visual review.
2. Commit verified results, merge the feature PR into `dev`, and promote validated `dev` into `main`.
3. Continue GUI improvements against the single desktop and renderer-neutral runtime foundation.

## Remaining release work

Signed same-candidate packages, all-platform native vault and accessibility journeys, live-provider release evidence, reference-machine reports, and human visual approval remain open in the [release checklist](../release/GUI_RELEASE_CHECKLIST.md).
Source cutover does not mark those distribution gates complete.
An unacknowledged renderer replacement still requires restarting the desktop process.
Native planning and evaluation producers remain future runtime work.
Historical terminal milestone evidence is retained in [progress memory](progress.md), with its original gaps unclaimed.
