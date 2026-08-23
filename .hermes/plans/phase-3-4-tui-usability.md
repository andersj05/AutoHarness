# Phase 3.4: TUI usability and discoverability

Branch: `feat/phase-3-4-tui-usability` cut from `dev` at `246a18a`.

## Goal

Make the terminal interface understandable without memorizing shortcuts and efficient
enough for sustained daily work, per [PROJECT_PLAN](../../PROJECT_PLAN.md) Phase 3.4.

## Vertical slices

Each slice lands behind failing tests first and is committed independently.

1. **Command palette and slash commands.** A searchable palette overlay
   (`Ctrl+/`) whose entries execute the same handler functions as keyboard
   shortcuts, so keyboard, palette, and slash paths converge on identical typed
   application intents. Composer slash commands generalize beyond `/sessions`
   to the full command table with explicit unknown-command validation.
2. **Contextual help overlay.** `F1` or palette entry opens grouped keybindings;
   the visible section reflects current focus. Footer gains affordances for the
   new surfaces at every supported width.
3. **Status surface.** Header becomes a real status line: provider profile and
   credential source from the settings projection, selected model, session
   identity, aggregate token usage, catalog freshness or failure, and active
   work state, degrading gracefully at narrow widths.
4. **Composer history.** In-run recall of recently submitted prompts with
   `Ctrl+Up` / `Ctrl+Down`, preserved alongside per-session drafts.
5. **Transcript search.** `Ctrl+F` opens a search bar; Enter advances through
   matches; the transcript scrolls the matching row into view using wrapped
   row accounting consistent with the renderer.
6. **Copy and export.** Copy emits the transcript through OSC 52 from the
   runner (no new dependencies); export dispatches a new `ExportSession`
   intent the coordinator satisfies from durable events as Markdown beside
   the database.
7. **Structured tool rows.** Durable tool calls join the transcript as their
   own row kind from the authoritative aggregate, collapsed to one line with
   a global expand toggle, including permission outcomes and result state.
8. **Confirmations and undo.** Archive arms for confirmation like delete; a
   recent archive or unarchive stays reversible with `Ctrl+Z` until superseded.

## Exit evidence

- Fixed-size goldens regenerated and visually reviewed at 80x24, 120x40,
  120x50 (wide), 60x18, and 40x12.
- Every important action reachable from palette, slash, and key paths.
- Full baseline gates green; documentation links verified.

## Non-goals

- Theme configuration files (deferred until settings keys exist).
- Mouse support.
- Credential management flows (tracked under Phase 3.3 follow-up).
