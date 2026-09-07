# ADR-0020: Retire the terminal client and make desktop the source default

**Status:** Accepted

**Date:** 2026-09-07

## Context

The maintainer explicitly selected the GUI as the only product interface and requested retirement of the legacy TUI before further GUI improvement.
The coordinator still imports application messages and projections from the terminal crate, so deleting the renderer first would break the authoritative runtime.

## Decision

Move the in-process application messages, projections, and bounded channels into `autoharness-client`, separately from its versioned wire protocol.
Remove the Ratatui renderer, terminal runner, PTY acceptance infrastructure, and terminal-only performance tooling.
Keep durable runtime, provider, credential, permission, memory, storage, and replay tests.
Make both application executable names launch the desktop by default.
Preserve settings-file compatibility with legacy terminal preferences without retaining a terminal renderer.

This decision replaces ADR-0019's requirement to retain the TUI until signed release approval and a rollback window have completed.
The maintainer's source cutover authorization does not establish signed distribution, cross-platform accessibility approval, or completion of uncollected release evidence.
Historical terminal evidence remains in the documentation and Git history.
Rollback uses a prior Git revision and its compatible application data backup.

## Consequences

The runtime no longer depends on a presentation adapter.
Future work targets one desktop interface and the renderer-neutral contract.
Source integration proceeds through feature-to-dev and dev-to-main pull requests with applicable validation.
The GUI release checklist continues to govern distribution evidence independently of this source retirement.

## Related decisions

- [ADR-0019](0019-use-tauri-web-rendered-desktop-client.md)
- [Branch workflow](0003-use-main-dev-feature-branches.md)
