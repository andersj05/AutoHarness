# GUI-only foundation review

**Date:** 2026-09-07

**Status:** Implementation complete; full validation in progress.

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
- Frontend production build and 22 Python tooling tests pass.
- Complete Rust, frontend, documentation, and storage baseline results are pending.
- Native lifecycle and visual review results are pending.

## Remaining distribution evidence

This review does not establish signed installers, all-platform GUI vault journeys, live-provider release evidence, approved reference-machine performance, or human screen-reader and visual approval.
Those requirements remain in the [GUI release checklist](GUI_RELEASE_CHECKLIST.md).
An unacknowledged renderer replacement still requires a desktop process restart.
Advanced plan and evaluation surfaces remain inert typed presentation contracts until authoritative runtime producers exist.
