# GUI-only foundation review

**Date:** 2026-09-07

**Status:** GUI-only implementation and local validation complete; distribution evidence remains open.

## Scope and decision

[ADR-0020](../adr/0020-retire-terminal-client.md) authorizes source cutover and terminal retirement before signed distribution approval.
Application messages, projections, and bounded channels now live in `autoharness-client::runtime`, separately from the versioned serializable wire contract.
The coordinator and desktop carrier no longer import a renderer-owned type.
Ratatui, Crossterm, the terminal editor and runner, PTY tests, and terminal latency instrumentation are removed.
Provider, storage, engine, credential, permission, memory, and coordinator regression coverage remains.
Both `autoharness` and `ah` select the desktop by default and reject `--tui`.
Legacy terminal settings remain readable so existing profiles retain compatibility.

## Validation

- The 27 existing wire-contract tests and two in-process channel and secret-ingress tests pass.
- The complete baseline on `af93bf6` passes 656 Rust tests, 118 GUI tests, 22 Python tooling tests, formatting, strict Clippy, warning-denied rustdoc, doctests, theme generation, documentation links, and three isolated storage benchmark tests.
- Native review found low-contrast menu descriptions and shortcuts, including white-on-white high-contrast selection; the shared menu fix passes a 45-theme/treatment contrast check and the complete 119-test GUI suite.
- Native alias review found that the default tracing filter omitted `ah` lifecycle events; both application namespaces now emit safe markers while dependency logging remains excluded.
- After those fixes, strict workspace Clippy and all 327 application tests pass, including the new target-filter regression; the combined current Rust suite contains 657 passing tests and six opt-in ignored probes.
- The headless application library compiles with `--no-default-features`, and actionlint accepts the CI workflow.
- Windows WebView2 native session creation, switching, rename, archive, restore, export, confirmed deletion, clean shutdown, and durable replay pass across two restart boundaries.
- The `ah` alias also passes the native journey, with content-free readiness and clean shutdown markers verified on all launches.
- Native compact, standard, and wide screenshot evidence covers onboarding, Sessions, Providers, Memory, Settings, Help, and the normal and high-contrast command palettes.
- Focused assistant pixel review covers the compact rename dialog, route layouts, and selected palette rows; the native palette measures 7.01:1 text contrast in color mode and 21:1 in high-contrast mode.
- The documented `pnpm gui:desktop` command launches the Rust host with the hot-reload frontend and acknowledges its native baseline.
- A live alias walkthrough verifies F1 Help navigation, command-palette keyboard access, named native accessibility landmarks, and window-manager close.

The baseline logs and report remain local under `target/gui-evidence/local-baseline`.
Native reports and screenshots remain local under `target/gui-evidence/foundation-native`, `target/gui-evidence/foundation-alias-native`, and `target/gui-evidence/foundation-primary-final`.
These runs use isolated synthetic data and unsigned debug binaries with embedded production assets, not installed signed release candidates.
Source integration is tracked by [PR #26](https://github.com/andersj05/AutoHarness/pull/26) and the dedicated `dev` to `main` promotion.
Full cross-platform CI is required before each merge.

## Remaining distribution evidence

This review does not establish signed installers, all-platform GUI vault journeys, live-provider release evidence, approved reference-machine performance, or human screen-reader and visual approval.
Those requirements remain in the [GUI release checklist](GUI_RELEASE_CHECKLIST.md).
An unacknowledged renderer replacement still requires a desktop process restart.
Advanced plan and evaluation surfaces remain inert typed presentation contracts until authoritative runtime producers exist.
