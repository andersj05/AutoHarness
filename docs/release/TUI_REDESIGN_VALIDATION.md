# TUI redesign validation

**Reviewed:** 2026-08-28

**Phase:** 3.10 step 10

**Branch:** `feat/tui-conversation-flow`

**Implementation base:** `dev` at `6cf27da9df87347d9f697a2ffbe8c24775bbb310`

**Latest locally validated implementation commit:** `d125ca4`

**Status:** Local Windows validation, including the conversation-flow follow-up, is complete, but cross-platform promotion and human terminal review remain pending.

## Scope completed locally

The real-PTY journeys now assert the delivered redesign rather than retired labels and decoration.
The covered flows are credential-free first run, routed navigation, Settings persistence, provider recovery and Codex login handoff, offline restart, session lifecycle, invalid-call repair, permission deny and allow with replay, and forced-shutdown recovery.

Session rename mode now renders `Rename session`, `Enter save`, and `Esc cancel` in the action bar.
This closes a user-visible ambiguity found while replaying the lifecycle journey through the real binary.

Three new cross-platform PTY capability smokes exercise the real startup path.
Nerd Font mode must emit a BMP private-use glyph without a replacement character.
Unicode mode must render portable route glyphs without a private-use dependency.
Basic16 mode must report `16 colors` and emit neither truecolor nor indexed-color escape sequences.

The 2026-08-28 conversation-flow follow-up makes the composer the final item in the scrollable Chat content.
A blank conversation places the composer at the top, a short conversation places it after the final message, and a long conversation lets Page Up, Page Down, Alt-arrow input, or the mouse wheel scroll the composer out of view.
Typing, pasting, opening command input, focusing the composer, or pressing Ctrl+End restores tail following.
The follow-up also removes the empty-session onboarding hero, the redundant per-message vertical rule, and the `Session opened` notice.

## Local Windows evidence

All commands ran from the repository root on 2026-08-27.

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Pass |
| `cargo clippy --workspace --all-targets --all-features --locked --no-deps -- -D warnings` | Pass |
| `cargo test --workspace --all-targets --all-features --locked --no-fail-fast` | Pass |
| Warning-denied `cargo doc` and workspace doctests | Pass |
| `python scripts/check_docs_links.py` | Pass |
| Isolated benchmark workspace formatting, Clippy, and tests | Pass |
| Explicit real-PTY matrix across eight scenario binaries and twelve tests on the 2026-08-27 candidate | Pass |
| New long-session conversation-flow real-PTY test on the 2026-08-28 candidate | Pass |
| Release render-cost report | Pass |

The release render-cost report retained constant work across transcript lengths.
The 32-turn sample used 417 allocations and 70,624 total allocated bytes.
The 4,096-turn sample used 417 allocations and 70,680 total allocated bytes.

## Promotion gates still open

The existing CI workflow runs the ignored real-PTY suite on Windows, macOS, and Linux.
A pull request or pushed candidate must make that matrix green on one exact final commit before Phase 3.10 exits.

The automated glyph smokes verify emitted cells and replacement behavior, but they cannot verify the shape supplied by a human terminal font.
A reviewer must still inspect one actual Nerd Font terminal, one terminal without a Nerd Font, and one sixteen-color terminal.

The broader [terminal release checklist](TERMINAL_RELEASE_CHECKLIST.md) is not approved yet.
Its live-provider, platform-vault, reference-machine benchmark, database rollback, external reviewer, and release-approval items remain promotion gates shared with Phase 3.9.

[ADR-0016](../adr/0016-use-typed-tui-presentation-layer.md) therefore remains Proposed.
It may be marked Accepted only after the same final candidate passes the cross-platform PTY matrix, all three human terminal smokes, and the approved release checklist.

## Next actions

1. Push one final candidate and collect the Windows, macOS, and Linux CI run links for that exact commit.
2. Perform and record the three human terminal smokes against the same candidate.
3. Complete the remaining terminal release checklist evidence and obtain independent reviewer approval.
4. Merge the follow-up branch into `dev`, validate the resulting exact candidate, mark ADR-0016 Accepted after approval, and promote the validated `dev` branch into `main`.
