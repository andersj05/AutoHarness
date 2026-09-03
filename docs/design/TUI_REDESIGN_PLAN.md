# Terminal interface redesign plan

**Reviewed:** 2026-08-27

**Phase:** 3.10, sequenced after the Phase 3.9 release-candidate evidence matrix.

**Goal:** A beautiful, responsive, unmistakably AutoHarness terminal interface with one consistent visual language across every route, a settings workspace whose controls explain themselves, and no surface that is hard to read.

**Inputs:** [TUI_AUDIT.md](TUI_AUDIT.md) records the defects this plan closes.
[TUI_DESIGN_SYSTEM.md](TUI_DESIGN_SYSTEM.md) defines the contract every step implements against.
[ADR-0016](../adr/0016-use-typed-tui-presentation-layer.md) records the decision to introduce the presentation layer.

## Strategy

The audit's conclusion drives the sequencing: the layout skeleton is right and the missing piece is the layer between preferences and cells.
Polishing surfaces before that layer exists would reproduce the same inconsistency at higher effort, so the plan builds the foundation in steps 1 through 4 and only then rebuilds surfaces in steps 5 through 8.

Three rules govern every step.

1. No step may change route semantics, overlay ownership, intent dispatch, credential handling, or persistence contracts.
   This is a presentation change; the engine boundary and the settings schema stay fixed.
2. Every step ends with the workspace green on formatting, strict Clippy, and the full locked test suite, and with the route render matrix passing.
   No step may be merged with a red gate inherited from an earlier step.
3. Every step that changes a rendered surface updates the style-aware snapshots in the same commit, and a reviewer reads the rendered output rather than the diff.

Branches follow `feat/<kebab-case-name>` off the latest `dev`, one branch per step, each opening a pull request into `dev`.

## Prerequisites

These must land before step 1 because later steps depend on them and neither is a presentation concern.

**Wall-clock time in the projection.**
Audit finding S-1 shows session timestamps are unimplementable today because the model holds only a monotonic `UiInstant`.
Add a wall-clock millisecond field alongside it, published from the same place `Message::Tick` originates, and keep the monotonic value for animation and deadlines.
Relative formatting then becomes a pure function of two integers and stays deterministic in tests.

**Style-aware snapshot helper.**
Audit finding S7 shows the current snapshots discard style, so the redesign would have no regression net for exactly what it changes.
Add a shared test helper that serializes symbol, foreground, background, and modifiers per cell into a stable, diff-friendly text format, and convert the existing four goldens to it.
This helper is the single most important piece of leverage in the plan and must exist before any color code changes.

## Step 1: Presentation foundation

**Branch:** `feat/tui-theme-foundation`

**Closes:** S1, S2, S3, S4, S5, S6.

Create `crates/autoharness-tui/src/ui/` with `color.rs`, `palette.rs`, `tokens.rs`, `theme.rs`, and `metrics.rs`.
Implement the linear `Rgb` newtype, Oklab conversion, mixing, relative luminance, contrast ratio, and quantization to indexed-256 and basic-16.
Implement the nine theme seeds and the derivation table from the design system, replacing the three hand-written palette tables.
Implement the full token set with an explicit background intent of `Inherit`, `Surface(token)`, or `Transparent`, which removes `chat_visual_style` and `transparent_chat_text`.
Implement `ColorDepth` detection from `COLORTERM` and `TERM`, quantizing once during resolution.
Implement the five `ColorMode` treatments during resolution rather than at call sites.
Move the breakpoint and spacing constants into `metrics.rs`.

Mechanically map every existing `VisualRole` call site to a token so this step is a refactor with a deliberate visual delta limited to the new derivation, and record that delta in the snapshots.

**Exit criteria**

- `Theme` is resolved exactly once per frame and no render function constructs a `Style` or a `Color`.
- The contrast matrix test passes for all nine presets across all five color modes at the documented floors.
- A source check asserts color literals appear only in `palette.rs` and `color.rs`.
- Indexed-256 and basic-16 quantization have unit tests with fixed expected indices.
- `System` and `Dark` render measurably different backgrounds.
- Style-aware snapshots for all five sizes are reviewed and committed.

## Step 2: Gradient and icon engines

**Branch:** `feat/tui-gradient-icons`

**Closes:** W3, and the gradient half of C7.

Implement `ui/gradient.rs` with the three-stop model, Oklab sampling by normalized position, and the degradation rules for `NoColor`, `HighContrast`, and `Basic16`.
Replace the index-and-count gradient helpers so a gradient is reproducible across regions of different widths.
Implement `ui/icon.rs` with the full glyph triple table and `IconSet` resolution, plus the reserved two-cell Nerd Font slot.
Implement `ui/motion.rs` with the frame tables, the reduced-motion gate, the 100 ms repaint floor, and the idle suspension rule.
Add the `Glyph check` data needed by the Settings row that step 6 renders.

**Exit criteria**

- Every icon measures one cell in Unicode and ASCII and two cells in the Nerd Font slot, asserted by test.
- A source check asserts box-drawing and symbol codepoints appear only in `icon.rs` and `ui/component/`.
- Gradient sampling has unit tests pinning endpoint and midpoint values per theme.
- `NoColor` and `HighContrast` gradient degradation is snapshot-verified.
- No animation frame table exists outside `motion.rs`.

## Step 3: Component library

**Branch:** `feat/tui-component-library`

**Closes:** the rendering half of T2, T11, H1, V3, O2, O3, O4, P1, P3, S-3.

Implement every component in the design system catalog under `ui/component/`.
Each component measures itself, renders only from tokens and icons, and returns the hit regions it created.
`SettingRow` carries its variant as data so the editable-versus-inert question has one authority.
`ButtonRow` owns its geometry, which retires the magic column bands.
`Scrim` and the single `Modal` sizing function replace the five ad hoc popup rect helpers.

No page is converted in this step; the components land with tests and an internal gallery.

**Exit criteria**

- Every component has a rendering test at 40, 60, 80, and 120 columns asserting both symbols and styles.
- A gallery review harness, ignored by default like the existing visual review, renders every component in every variant at every reviewed size.
- `ButtonRow` hit regions are asserted to match the rendered label positions exactly.
- `KeyValueTable` label alignment is asserted for a mixed-length label set.
- `StatusBar` priority dropping is asserted at each breakpoint with deliberately overlong values.

## Step 4: Layout and hit-region unification

**Branch:** `feat/tui-layout-contract`

**Closes:** S8, S9.

Implement `Layout::compute` returning named rects plus an ordered hit-region list.
Convert `view` to compute the layout once and pass rects down, and convert `hit_test` into a reverse scan of the hit list.
Remove every threshold literal outside `metrics.rs`.
Resolve the dead interaction paths: wire the composer, transcript, and Settings rows where clicking is meaningful, and delete the mouse actions that no surface will ever produce.

**Exit criteria**

- No width or height literal outside `metrics.rs`, asserted by a source check.
- Every `MouseAction` variant is either produced by a layout case or removed, asserted by a coverage test.
- `profile_local_hit_row` and the other unreachable branches are gone.
- Existing mouse interaction tests pass unchanged against the new hit source.

## Step 5: Chat workspace

**Branch:** `feat/tui-chat-workspace`

**Closes:** C1, C2, C3, C4, C5, C6, C7, C8, W1, W2.

Rebuild the transcript from `MessageBlock`, `ToolCard`, and `Callout`.
A turn gains a two-cell role gutter with an icon and a rule, a role name, right-aligned turn metadata carrying model, duration, and token totals, and a wrapped body with a hanging indent.
Failures become a `danger` callout with an icon, a message, a code chip, and a real `ButtonRow` for retry and fresh session.
Tool calls become collapsible cards with a status icon, target, duration, and an expand caret.
Move the gradient rule from below the composer to between the transcript and the prompt.
Replace the status line with `StatusBar` priority segments, omit the workspace segment when the workspace is unknown, and render an `auto` thinking level as an explicit label rather than six empty slots.
Replace the streaming bar with the gradient wave, keeping the ASCII bar for ASCII mode and a static bar under reduced motion.
Replace the onboarding and empty-conversation states with `Hero`.
Rebuild the sidebar to carry route navigation with icons, a grouped recent-session list, and the workspace, sized 26 columns at `Lg` and 32 at `Xl`.
The selected route uses one full-width themed surface with a caret and icon, while inactive rows and the surrounding rail preserve the terminal background.
Do not duplicate a primary route in a bottom action row.
Remove the two-item compact footer from routes where it is meaningless.

**Exit criteria**

- The transcript reads as a threaded conversation at 80x24 with no shouted headings and no orphan token lines.
- The separator sits above the metadata line at every size, verified in snapshots.
- The status line never overflows and never leaves a gap, verified with deliberately overlong model, path, and branch values at every breakpoint.
- Streaming, cancelling, failed, offline, loading, no-model, empty-catalog, and new-conversation states each have a reviewed snapshot.
- Reduced motion produces a byte-identical frame across two consecutive ticks.

## Step 6: Settings workspace

**Branch:** `feat/tui-settings-workspace`

**Closes:** T1 through T15.

This is the largest step and the one the user asked for most directly, so it is specified in detail.

**Structure.** Replace the four-tab strip with a two-pane workspace: a left category rail with icons, and a right pane of rows.
Categories are `Appearance`, `Chat & Composer`, `Accessibility`, `Providers`, `Models & Thinking`, `Profile`, `Sessions & Data`, `Shortcuts`, and `About`.
The frame carries one name, `Settings`, and the selected category is the page title, which retires the three-name confusion.

**Rows become data.** Every row is a `SettingRow` variant, so the renderer and the input layer agree on what is editable.
The nine read-only runtime and policy facts move out of the preference list into an `Info` group at the top of their relevant category, visually distinct from editable rows, and they are no longer selectable as if they were settings.
The fabricated `APPROVALS`, `RETENTION`, and `LOGGING` single-row sections are absorbed into `About` and `Sessions & Data`.
The shortcut dump moves to the `Shortcuts` category rendered as a grouped `KeyValueTable`.

**Alignment.** Rows render through `KeyValueTable` geometry: a label column sized to the widest label in the category, a control column, a right-aligned provenance chip, and a description on a second line only when the row is focused.
This retires the single formatting decision that caused the worst readability defect in the audit.

**Provenance.** `Source: default` inline text becomes a right-aligned chip reading `default`, `user`, `workspace`, or `env`, colored by layer, with the full explanation shown in the focused-row footer.

**Controls.** Each variant is self-describing.

| Variant | Rendering | Keys |
| --- | --- | --- |
| `Toggle` | Segmented `on` and `off` with the active side filled | `Space` or `Enter` toggles; `Left` and `Right` also work |
| `Choice` | `SegmentedControl`: all options as chips at `Md` and above, `‹ current ›` with an `n/m` indicator below | `Left` and `Right` move; `Enter` opens a picker when the option count exceeds five |
| `Text` | Inline editor with a visible cursor and a bounded length indicator | `Enter` saves, `Esc` cancels |
| `Action` | A `ButtonRow` button such as `[ Connect API key ]` | `Enter` activates |
| `Info` | Value plus provenance chip, dimmed, skipped by selection | none |

**Theme preview.** The `Appearance` theme row renders each option as a chip followed by an eight-cell gradient sample and three surface swatches, so a theme is chosen by looking rather than by reading a lowercase word.
The `Glyph check` row renders the full icon set in the selected mode.

**Reset.** The unlabeled `R` and `D` chords are replaced by one explicit affordance on the focused row.
The footer names both outcomes and shows the resulting value, for example `Backspace inherit -> system  ·  Shift+Backspace default -> system`.
When a row has no user override, the inherit affordance is hidden rather than silently inert.

**Navigation.** `Tab` and `Shift+Tab` move between the rail and the rows, which is the first key a user reaches for and is currently a documented no-op.
`Up` and `Down` move within the focused pane and stay clamped, not cyclic, so the pane boundary is discoverable.
`Left` and `Right` change the focused row's value only when the rows pane has focus.
`Enter` enters the rail's category or activates the focused row, and `Down` from the rail does the same initialization as `Enter`, which retires the two-path asymmetry.
`Esc` steps back exactly one level: rows to rail, rail to Chat.
The surprise `K` and `P` chords are removed; connecting a credential and opening the profile become `Action` rows.

**Search.** `Ctrl+F` filters rows across all categories, showing the owning category as a group header on each match.
This is nearly free once rows are data and is the fastest way to find a setting in a nine-category workspace.

**Footer.** A persistent two-row footer shows the focused row's name, its resolved value, its source, and exactly the keys valid at that moment, so nothing has to be guessed.
Every help string is built from the icon set and the key table, which retires the mixed ASCII and Unicode arrow defect on every Settings page.

**Scroll.** Selection-aware scrolling is computed from row indices, not by searching rendered text for a label substring.

**Exit criteria**

- No inline `Source:` text remains and every row's value column starts at the same screen position within its category.
- Every row's editability is derived from its variant, verified by a test that no `Info` row can be reached by selection and no editable row is inert.
- `Tab`, `Shift+Tab`, `Up`, `Down`, `Left`, `Right`, `Enter`, and `Esc` behave identically in every category, verified by a focused navigation test per category.
- Theme and glyph previews render correctly in all nine presets and all three glyph modes.
- Settings search finds every preference key by label and by value.
- ASCII mode contains no Unicode glyph on any Settings page, asserted by a snapshot scan.
- The reset footer names the resulting value for a row with an override and hides the inherit affordance for a row without one.
- The `Profile` category is a populated page rather than a single action behind an overlay.

## Step 7: Sessions, Models, Providers, and Help

**Branch:** `feat/tui-content-pages`

**Closes:** S-1, S-2, S-3, S-4, V1, V2, V3, V4, M1, M2, M3, H1, H2, H3.

**Sessions.** Two panes at `Md` and above: a grouped `ListView` and a detail pane showing title, model, message count, last activity, and archived state.
Rows carry a right-aligned metadata column and `Chip` badges for `active`, `archived`, and `default model`.
Real relative ages replace the literal string `updated`, using the wall-clock field added in the prerequisites, with group headers for `Today`, `Yesterday`, `This week`, and `Older`.
The inverted `Filter:` slab becomes a `SearchField` with an icon and a match count.
Destructive confirmation is announced once, in the modal, with a real `ButtonRow`; the duplicate page action bar is removed while confirmation is armed.

**Models.** Replace the two-step wizard with one page: a `ListView` of model cards showing display name, a `Default` badge, capability chips parsed from the provider detail string, and the context window size, beside a thinking-level `SegmentedControl`.
Both values still persist together, so the sequencing disappears without changing the persistence contract.

**Providers.** Rename the panes to match their contents: a provider catalog and saved connections.
Split the detail pane into grouped `KeyValueTable` sections for identity, connection, credential, and defaults.
Replace the button string literals with `ButtonRow`, which retires the mismatched column bands.
Render unavailable providers with a `muted` chip and a one-line reason rather than the bare word `Unavailable`.

**Help.** Rebuild from grouped `KeyValueTable` with aligned key and description columns, section rules, and icons.
Generate the content from the shared command and key tables so the reference cannot drift from the implementation, which fixes the missing `Ctrl+1..5` documentation permanently.

**Exit criteria**

- Session ages are real, deterministic in tests, and correct across day and week boundaries.
- Every session row's metadata column aligns and every badge is a chip.
- Model capability chips and context window sizes render for both a Gemini and a router catalog fixture.
- Provider button hit regions match rendered labels exactly, including the Codex `[ Sign in ]` variant width.
- Help is generated, and a test asserts every command with a key hint appears exactly once.
- Confirmation is announced exactly once per armed action, asserted by a snapshot scan for duplicate confirm affordances.

## Step 8: Overlays and command palette

**Branch:** `feat/tui-overlays`

**Closes:** O1, O2, O3, O4, P1, P2, P3, P4.

Apply `Scrim` beneath every modal so no background character survives beside the frame.
Convert every overlay to the single `Modal` component with one sizing function, one border rule keyed to intent, and a `ButtonRow` footer.
Fix the copy defects, including the duplicated word in the permission action row.

Rebuild the command palette as a real anchored panel: a bordered container with a title and a `SearchField`, rows in three aligned columns of identifier, label, and right-aligned key hint, ellipsis truncation that never cuts mid-word, category group headers, matched-substring highlighting from the ranges the existing relevance ranker already computes, and a full-width selection fill so selection is never carried by foreground color alone.
The inline and centered palettes share one renderer so they can no longer disagree about what selection looks like.

**Exit criteria**

- A snapshot scan asserts no non-space background character appears outside a modal frame while a modal is open.
- Every overlay uses the same sizing function and the same border rule table.
- Palette rows never truncate mid-word at any width, asserted with deliberately long command descriptions.
- Match highlighting is asserted for exact, prefix, substring, and fuzzy matches.
- Inline and centered palettes produce identical row rendering for the same state.

## Step 9: Responsive and accessibility conformance

**Branch:** `feat/tui-conformance`

**Closes:** the remaining matrix coverage for every finding.

Run and commit the full matrix: five sizes across nine themes across five color modes, plus ASCII, Nerd Font, Unicode, reduced motion, compact density, and single-column layout, for every route and every overlay.
Fix every defect the matrix reveals, including ones outside the original findings, per the repository quality standard.
Re-check the contrast, icon width, literal-color, literal-glyph, and hit-coverage gates.
Measure render cost, because per-cell gradient spans and per-frame theme resolution are the two plausible regressions; compare against the existing benchmark markers and keep the render loop free of allocation growth proportional to transcript length.

**Exit criteria**

- Every route and overlay has a reviewed snapshot in every mode of the matrix.
- No clipped content, no mid-word truncation, and no overlapping text at any matrix cell.
- Reduced motion produces identical consecutive frames everywhere.
- `NoColor` conveys every state that `Color` conveys, verified by asserting each state has a distinguishing symbol or modifier and not only a color.
- Frame render time and allocation counts are within the recorded pre-redesign envelope.

## Step 10: Validation and promotion

**Branch:** `feat/tui-redesign-validation`

Run the real-PTY journeys on Windows, macOS, and Linux for the routed shell, first run, settings persistence, provider login, session lifecycle, permission outcomes, and forced-shutdown recovery.
Perform a real terminal smoke in a Nerd Font terminal and in a terminal without one, and in a terminal reporting only sixteen colors.
Reconcile `docs/memory/active.md` and `docs/memory/progress.md` with the delivered state, update `docs/architecture/OVERVIEW.md` if any component boundary moved, and mark ADR-0016 accepted.
Execute the terminal release checklist.

**Exit criteria**

- The cross-platform PTY matrix is green on one candidate commit.
- Three real terminal smokes pass: Nerd Font, no Nerd Font, and sixteen-color.
- Documentation, memory, and the ADR reflect the delivered system.
- The release checklist is executed and approved.

## Sequencing summary

| Step | Branch | Depends on | Primary outcome |
| --- | --- | --- | --- |
| 0 | `feat/tui-redesign-prerequisites` | none | Wall-clock time and style-aware snapshots |
| 1 | `feat/tui-theme-foundation` | 0 | Theme, tokens, seeds, color depth, contrast gate |
| 2 | `feat/tui-gradient-icons` | 1 | Gradient engine, icon set, motion tables |
| 3 | `feat/tui-component-library` | 2 | Component catalog with tests and a gallery |
| 4 | `feat/tui-layout-contract` | 3 | One layout pass and one hit-region source |
| 5 | `feat/tui-chat-workspace` | 4 | Conversation, status bar, sidebar, hero states |
| 6 | `feat/tui-settings-workspace` | 4 | Two-pane settings with explicit controls |
| 7 | `feat/tui-content-pages` | 4 | Sessions, Models, Providers, Help |
| 8 | `feat/tui-overlays` | 4 | Scrim, unified modals, palette |
| 9 | `feat/tui-conformance` | 5, 6, 7, 8 | Full matrix and performance envelope |
| 10 | `feat/tui-redesign-validation` | 9 | Cross-platform evidence and promotion |

Steps 5 through 8 depend only on step 4, so they can proceed in parallel once the foundation lands, provided each rebases onto `dev` before opening its pull request.

## Risks and responses

| Risk | Response |
| --- | --- |
| Per-cell gradient spans increase render cost | Sample gradients only on rules, borders, meters, and short titles; never on body text. Measure in step 9 against the recorded envelope. |
| Nerd Font glyphs render as boxes or shift columns | Reserve a two-cell slot, assert measured width, keep Unicode the default, and add the `Glyph check` row so a user verifies before committing. |
| Terminals without truecolor flatten the palette | Quantize during resolution with Oklab nearest-match, surface the detected depth in `About`, and snapshot the sixteen-color rendering. |
| Snapshot churn hides real regressions | Land the style-aware helper in step 0, keep one step per pull request, and require a rendered-output review rather than a diff review. |
| The refactor drifts into engine or settings changes | Rule 1 forbids it; the settings schema stays at version 4 and no new preference key is introduced by this plan. |
| Contrast clamping changes a theme's intended character | Clamp toward the floor rather than failing, and review each theme's rendered output in step 1 before committing the derivation ratios. |
| Steps 5 through 8 diverge in vocabulary while parallel | The component catalog is frozen at the end of step 3; any new component requires an amendment to the design system document first. |

## Non-goals

This plan does not add a preference key, change the settings schema version, alter the provider or credential boundary, introduce a custom theme file format, add terminal image or hyperlink protocols, or change which routes exist.
It also does not add mouse hover states, because Crossterm hover reporting is inconsistent across the supported terminals and would need its own decision record.
