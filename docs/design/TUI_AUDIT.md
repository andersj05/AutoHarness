# Terminal interface audit

**Reviewed:** 2026-08-27

**Scope:** Every visible surface of `autoharness-tui` at the reviewed sizes 120x50, 120x40, 100x30, 80x24, 60x18, and 40x12.

**Purpose:** Establish the evidence baseline for the redesign.
This document records what is wrong today.
[TUI_DESIGN_SYSTEM.md](TUI_DESIGN_SYSTEM.md) defines the target contract and [TUI_REDESIGN_PLAN.md](TUI_REDESIGN_PLAN.md) defines the ordered work.

## Method

Rendered buffers were captured from the checked-in review harness `crates/autoharness-tui/tests/visual_review.rs` with `cargo test -p autoharness-tui --test visual_review -- --ignored --nocapture`, plus the four fixed-size goldens under `crates/autoharness-tui/tests/golden/`.
Source behavior was read directly from `crates/autoharness-tui/src/view.rs` (4088 lines), `update.rs` (4141 lines), and `model.rs` (2637 lines).
Every defect below cites either a captured buffer or a source location.

## Structural findings

### S1. The presentation layer has no abstraction between preferences and cells

`view.rs` is one 4088-line module with 130 free functions.
Every surface calls `visual_style(model, role)` (`view.rs:185`), which re-derives a complete `Style` through a nested match over five color modes and nine theme presets on every span of every frame.
There is no resolved theme value, no token table, no component layer, and no layout contract.
Consequently each page invents its own spacing, its own label widths, its own help-row vocabulary, and its own separator glyphs.
This is the root cause of nearly every visual defect in this document.

### S2. Only eleven visual roles exist for the whole application

`VisualRole` (`view.rs:33`) offers `Normal`, `Header`, `Muted`, `User`, `Assistant`, `Error`, `Tool`, `Selected`, `Border`, `Warning`, and `Field`.
There is no surface elevation, no focus ring, no divider, no scrollbar, no success state, no informational state, and no distinction between a subtle and a strong border.
Panels therefore reuse `Selected` to mean "focused", which paints a solid accent background on a border and reads as an error state.
Overlays reuse `Field` to mean "input", which paints a full-width inverted bar.

### S3. Background color is baked into every role, then stripped by a second function

Each role returns a style with an explicit `.bg(...)` (`view.rs:190`), so a second function `chat_visual_style` exists purely to call `.bg(Color::Reset)` afterwards (`view.rs:348`), and a third helper `transparent_chat_text` walks an already-built `Text` to reset every span background (`view.rs:3550`).
Three mechanisms compete to decide whether a cell is transparent.
Chat is transparent, Settings is opaque, and overlays are inconsistent, so the same theme looks like two different applications depending on the route.

### S4. All themes emit twenty-four-bit color unconditionally

Every palette entry is a `Color::Rgb` literal (`view.rs:80` through `view.rs:289`).
Nothing inspects terminal color capability.
On a 256-color or 16-color terminal these values are downsampled by the terminal itself with no control over the result, and the carefully chosen accent relationships collapse.

### S5. There is no contrast guarantee

No test asserts a minimum contrast ratio between any foreground and background token.
`Muted` in the System theme is `Rgb(100, 116, 139)` on `Rgb(8, 12, 24)`, which is legible, but the same role in the six extra themes is hardcoded to `Color::Gray` on a theme-specific background (`view.rs:166`) with no verification.
Six of the nine themes therefore share one grey muted color regardless of their background luminance.

### S6. The nine themes are nine unrelated hand-written tables

`extra_theme_style` hardcodes a ten-tuple of RGB values per preset (`view.rs:77`), `visual_style` hardcodes three more inline (`view.rs:190`, `view.rs:227`, `view.rs:253`), and `theme_gradient` hardcodes a separate two-stop pair per preset (`view.rs:352`).
`System` and `Dark` are byte-identical duplicates.
Adding a theme requires editing three unrelated tables, and there is no shared derivation, so tone relationships differ arbitrarily between presets.

### S7. Golden tests cannot detect a color regression

The snapshot helpers in `visual_review.rs:109` and the golden comparison read only `cell.symbol()`.
Style, foreground, background, and modifiers are discarded.
Every color, gradient, focus indicator, and contrast property in the application is therefore untested, and a redesign has no regression net for the exact thing it changes.

### S8. Layout thresholds are duplicated between rendering and hit testing

`shell_layout` decides the wide shell at `width >= 100 && height >= 16` (`view.rs:997`), and `hit_test` re-derives the same thresholds independently (`view.rs:561`).
Column bands for buttons are magic literals: `profile_action_at_column` maps `0..=10`, `12..=19`, and `21..=29` to three actions (`view.rs:944`), while the labels those bands are supposed to cover are rendered from a separate string literal `"[ API key ] [ Test ] [ Model ]"` (`view.rs:2447`).
Nothing keeps the two in sync.

### S9. Dead interaction paths exist because geometry and rendering disagree

`handle_mouse` implements `ChatSend`, `ChatModels`, `ChatNewSession`, `ChatSessions`, `ChatCredential`, and `ChatHelp` (`update.rs:136`), but `hit_test` never produces any of them.
`profile_local_hit_row` always returns `None` (`view.rs:728`), making its caller unreachable.
General Settings rows, Models rows, the transcript, and the composer have no mouse handling at all, so the application is inconsistently clickable.

## Visual findings by surface

### Chat

**C1. The prompt separator is drawn below the input instead of above it.**
`render_prompt_bar` places the gradient rule at `surface.bottom() - 1` (`view.rs:3373`), which is the last row of the prompt region.
In `tests/golden/main-120x40.txt` row 40 is a full-width rule underneath the composer, and in `main-80x24.txt` row 23 is the same.
A separator under the input reads as a stray underline; the boundary that actually needs marking is between the transcript and the prompt.

**C2. The workspace segment renders as a bare period when the workspace is unknown.**
`workspace_display_path` returns `"."` for an empty input (`view.rs:3083`).
Captured 80x24 output ends with `Gemini 2.5 Pro │ auto ○○○○○○ │ ctx <0.1% │ .`.
The segment should be omitted, not degraded to a single punctuation mark.

**C3. The thinking meter shows six empty circles for the common `auto` state.**
`thinking_level` maps any unrecognized value, including the default empty string, to `("auto", 0)` (`view.rs:3070`), so the meter renders `auto ○○○○○○`.
Six empty slots communicate "zero of six" rather than "not applicable".

**C4. Status segments drop at hardcoded pixel widths rather than by measured priority.**
The status line tests `width >= 20`, `>= 32`, `>= 56`, `>= 76`, and `>= 98` against the region width (`view.rs:3296` through `view.rs:3343`), never against the width the earlier segments actually consumed.
A long model name and a long branch name can therefore overflow at a width where the thresholds all pass, and short values leave the line sparse at widths where a threshold fails.

**C5. Conversation roles are shouted rather than structured.**
Each turn emits a bare uppercase heading `YOU`, `AUTOHARNESS`, or `TOOL` followed by unindented body text (`view.rs:3672`, `view.rs:3700`, `view.rs:3676`), with a blank line between turns as the only grouping.
There is no gutter, no icon, no rule, and no visual containment, so at 80x24 the transcript reads as a flat log.
Token usage is appended as its own body line `18 input tokens · 41 output tokens` (`view.rs:3777`) rather than as turn metadata.

**C6. Failure rows are plain text with a pipe-delimited action string.**
A failed attempt emits `rate_limited | Ctrl+R retry | Ctrl+N new | ref attempt-2` (`view.rs:3766`) in the muted role.
The most important recovery affordance in the application is rendered less prominently than the error message above it and is not visually separated from ordinary transcript text.

**C7. The generation indicator is an ASCII progress bar in every glyph mode.**
`generation_animation` returns frames such as `[==>-----]` (`view.rs:4186`) with no Unicode or Nerd Font variant, and the sixteen-frame table is unrelated to the theme gradient.

**C8. Tool rows collapse structure into one concatenated string.**
`TOOL · fs_read · completed · 27 bytes read` is built by appending separators to a single heading (`view.rs:3676`), and the expanded form appends the resource inside square brackets to the same line (`view.rs:3687`).
There is no status icon, no aligned columns, no duration, and no expand affordance.

### Command palette and slash commands

**P1. The inline palette has no container and bleeds into the transcript.**
`render_inline_palette` clears its rows and renders a bare `List` with no block, border, or title (`view.rs:1296`).
Captured 80x24 output shows palette rows beginning immediately under the transcript line `partial` with identical indentation and weight, so the two are indistinguishable.

**P2. Palette rows truncate mid-word with no ellipsis.**
Rows are formatted as one unbounded string (`view.rs:1340`) and clipped by the widget.
Captured output shows `Choose the model and thinking mode for new sessio`, `profile summary  [Al`, and `Choose the model used by the current sessi`.

**P3. Selection is indicated by foreground color alone.**
`inline_palette_item` styles the selected row with `chat_visual_style(Assistant)` and the rest with `Normal` (`view.rs:1349`), so the only difference is text color plus a one-character marker.
The non-inline palette does use a filled `Selected` style (`view.rs:1447`), so the two palettes disagree about what selection looks like.

**P4. Rows are unstructured and ungrouped.**
Identifier, human label, description, and key hint are concatenated into a single line with two-space runs (`view.rs:1340`), so nothing aligns across rows and there is no category grouping even though the command table has clear groups.
Relevance ranking already computes match positions in `ranked_command_entries` (`model.rs:2766`), but no match highlighting is rendered.

### Sessions

**S-1. Timestamps are not implemented.**
`session_timestamp_label` returns the literal string `"updated"` for the default `Relative` style and `format!("updated {updated_at_ms}")` for `Absolute` (`view.rs:2149`).
Captured output shows `> Destructive accessibility review  [updated]`.
The root cause is that the model holds only a monotonic `UiInstant` (`model.rs:2166`), so no wall-clock reference exists to compute a relative age from `updated_at_ms`.
The redesign must plumb wall-clock time before any session list can show a real age.

**S-2. Rows carry no structure and no columns.**
`browser_item` appends `[active]`, `[archived]`, and `[updated]` to the title as bracketed text (`view.rs:2962`).
There is no aligned metadata column, no model name, no message count, no grouping by recency, and no detail pane at any width.

**S-3. The filter row is a full-width inverted bar labeled `Filter:`.**
`render_browser` renders the query with `VisualRole::Field` across the entire inner width (`view.rs:2904`), which under `NoColor` becomes a fully reversed row and under a color theme becomes a solid slab.
It is styled more strongly than the list it filters.

**S-4. Confirmation is announced twice with different key vocabulary.**
The modal body says `Y confirm  N or Esc cancel` (`view.rs:1216`) while the page action bar simultaneously shows `[ Y Confirm ]  [ N Cancel ]` (`view.rs:2948`).
Captured 80x24 confirmation output contains both.

### Settings

This surface has the most defects and the user-reported confusion is fully reproduced.

**T1. The page has three different names for itself.**
The frame title is `Settings & Provenance` (`view.rs:1535`), the first navigation tab is labeled `Settings` (`view.rs:471`), and the body header for that same tab is `General` (`view.rs:1580`).
Captured 120x50 output shows all three within the first four rows.

**T2. Values, provenance, and descriptions share one unaligned text run.**
Rows are formatted as `{marker} {label:<18} {value}  Source: {source}  {explanation}` (`view.rs:1936`).
Only the label is padded, so the value column, the `Source:` column, and the explanation column start at a different screen position on every row.
Captured 120x50 output shows `not set  Source: default`, `gemini (default)  Source: runtime`, and `Ctrl+S / Ctrl+Enter  Source: default` all beginning at different columns.
This single formatting decision is the largest readability defect in the application.

**T3. Nine of the nineteen rows are not settings.**
`Provider`, `Profile`, `Credential`, `Source`, `Model`, `Mode`, `Approvals`, `Retention`, and `Logging` are read-only runtime or policy facts with hardcoded source strings (`view.rs:1754` through `view.rs:1826`) and no backing preference.
They are keyboard-selectable, they consume selection indices, and `Left`, `Right`, `R`, and `D` silently do nothing on them (`update.rs:874`), with no rendered indication that the row is inert.

**T4. Section headers are fabricated around single inert rows.**
`APPROVALS`, `RETENTION`, and `LOGGING` are each a header followed by exactly one read-only row (`view.rs:1609`, `view.rs:1611`, `view.rs:1623`).
Blank-line separation is applied inconsistently: `PROFILE DEFAULTS` and `APPEARANCE` get one, `PROMPT BAR`, `ACCESSIBILITY`, `LOGGING`, and `TERMINAL BEHAVIOR` do not.

**T5. A full keyboard reference is dumped inside the preference list.**
`SHORTCUT REFERENCE` appends one row per command with a key hint (`view.rs:1632`) to the same scrolling paragraph that holds the editable preferences.
The reference is unreachable without scrolling past every setting, and it competes with the settings for the same selection model.

**T6. The help row mixes two navigation vocabularies and two glyph modes.**
`render_settings` builds the footer as `format!("{} {controls}", navigation_keys(model))` (`view.rs:1659`), where `controls` already contains its own arrow glyphs.
Captured ASCII-mode 120x50 output reads `Up/Down ←/→ page  Down open  Esc Chat`, which prints the ASCII arrow name and the Unicode arrows side by side and describes two different things with one label.
The same hardcoded Unicode arrows appear in the Providers help (`view.rs:2218`), the Models help (`view.rs:2697`), the Profile help (`view.rs:1744`), and the profile editor hint (`view.rs:2815`), so ASCII mode is broken on every Settings page.

**T7. Entering a page behaves differently depending on which key is used.**
`Enter` on a navigation tab calls `activate_settings_nav`, which initializes tab-specific state, while `Down` only clears `nav_focus` with no initialization (`update.rs:657` and `update.rs:672`).
The Providers pane therefore has a stale focus target when entered with `Down` and a correct one when entered with `Enter`.

**T8. `Tab` is an explicit no-op on the pages where a user will reach for it first.**
`handle_settings_input` matches `Tab` and returns nothing (`update.rs:656`), as does the Models handler (`update.rs:2276`) and the profile editor (`update.rs:2445`).
The only way to change page is arrow keys on the tab row, which is undiscoverable from the rendered chrome.

**T9. Two unlabeled single-letter chords perform similar but distinct destructive resets.**
`R` clears the user override so the value falls back to an inherited layer, and `D` writes the built-in default at the user layer (`update.rs:964` and `update.rs:997`).
The footer names them only as `R inherit  D reset`, which does not state the resulting value and inverts the natural reading of both words.

**T10. Two more single-letter chords silently leave the page.**
`K` opens the session-only credential overlay and `P` navigates away to the Profiles route (`update.rs:744` and `update.rs:753`), neither of which is announced by the rendered footer.
`K` is only discoverable from the static body line `API KEY  /connect or press K` (`view.rs:1593`).

**T11. The choice carousel hides the option set and mixes brackets with arrows.**
`wheel_value` renders `‹previous  [current]  next›` (`view.rs:1953`).
For the nine-value theme preset this shows three of nine options with no position indicator, and the reader must guess whether the brackets or the arrows mark the current value.

**T12. Theme selection has no preview.**
Choosing among nine color presets is done by reading nine lowercase words.
No swatch, gradient sample, or live preview is rendered, so the only way to evaluate a theme is to select it and look at the whole application.

**T13. `Esc` leaves the entire route from any depth.**
`handle_settings_input` maps `Esc` to `navigate_to_route(Chat)` before any delegation (`update.rs:617`), so pressing `Esc` inside the Providers pane exits Settings rather than returning to the tab row.
The embedded Providers handler has its own conflicting `Esc` behavior that returns to the nav row (`update.rs:1870`), and the outer handler wins.

**T14. The Profile page is a single action with an almost empty body.**
`render_settings_profile` renders a two-row header, a four-row card, and a help row (`view.rs:1718`), and `Enter` opens an overlay to edit one string.
At 120x50 the page is more than eighty percent empty.

**T15. Scroll position is computed by searching rendered text for a label substring.**
`settings_scroll` locates the selected row by scanning built `Line` values for the preference's label string (`view.rs:2064`).
Any label that is a substring of another label, or any future row that repeats a label, silently scrolls to the wrong place.

### Providers

**V1. Panel titles contradict their contents.**
The left pane is titled `Add provider` but is rendered by a function named `render_connected_profiles` (`view.rs:2258`), while the right pane is titled `Connected providers` and is rendered by `render_profile_detail` (`view.rs:2324`).
The right pane mixes a list of saved profiles and the selected profile's detail fields in one unbroken paragraph (`view.rs:2352` through `view.rs:2455`).

**V2. Buttons are text literals with separately hardcoded hit bands.**
`[ API key ] [ Test ] [ Model ]` and `[ Disconnect ] [ Remove ]` are string literals (`view.rs:2445` and `view.rs:2452`) whose clickable regions are the column ranges in `profile_action_at_column` and `profile_secondary_action_at_column` (`view.rs:944`).
The literals and the ranges are maintained independently, and the Codex variant `[ Sign in ]` has a different width than `[ API key ]` while sharing the same band table.

**V3. Detail fields are a flat list of fourteen label-value lines with no grouping.**
`detail_line` pads labels to fourteen characters (`view.rs:2747`) and every field is rendered at the same weight, so identity, status, credential location, and defaults are indistinguishable.

**V4. Provider availability is communicated by the word `Unavailable`.**
`provider_choice_status` returns `"Unavailable"` for Cursor and Claude Code (`view.rs:2538`) with no icon, no dimming beyond the shared row style, and no explanation.

### Models

**M1. The page is a two-step wizard for what is a single decision.**
`render_model_defaults` renders step chips `1  MODEL` and `2  THINKING` (`view.rs:2579`) and requires `Enter` to advance before the thinking level can be seen.
Both values are saved together (`update.rs:2410`), so the sequencing is imposed by the renderer rather than by the data.

**M2. The default marker is bare uppercase text appended to the model name.**
`  DEFAULT` is concatenated into the row (`view.rs:2641`) instead of rendered as a badge, and the row also concatenates the capability detail string with two spaces.

**M3. Model capabilities are an opaque provider string.**
`summary.detail` is rendered verbatim, producing rows such as `reasoning | text`.
Context window size is available on `ModelSummary` and used by the status bar, but is never shown in the catalog where a user chooses a model.

### Help

**H1. Key and description columns are ragged.**
Rows are built as `Span("  {key}")` followed by `Span("  {description}")` with no padding (`view.rs:1498`).
Captured 80x24 output shows descriptions starting at four different columns within the first five rows.

**H2. Section headings have no visual weight beyond a color change and no separation.**
Only the first section is styled `Selected`; the rest use `Normal` (`view.rs:1491`), and no blank line or rule separates sections.

**H3. Help documents only half the route chords.**
The reference lists `Alt+1..5` (`model.rs:1509`) while `direct_route` also accepts `Ctrl+1..5` (`update.rs:439`).

### Overlays

**O1. Modals do not obscure their background, leaving orphan characters beside the frame.**
Every modal clears only its own rectangle.
In captured 60x18 permission output the onboarding lines behind the modal remain visible one column to its left as the isolated characters `G`, `1`, `2`, and `3`.
There is no scrim, dim, or shadow.

**O2. Modal chrome is inconsistent across overlays.**
Border roles differ per modal without a rule: `Selected` for startup (`view.rs:534`), `Error` for confirmation (`view.rs:1205`), `Warning` for permission (`view.rs:2999`) and profile credential (`view.rs:2840`), and `Border` for the palette, help, picker, user profile, Codex sign-in, and profile editor.
Sizing helpers are five separate functions with five different clamp tables (`view.rs:4042`, `view.rs:4081`, `view.rs:4095`, `view.rs:4109`, `view.rs:4123`).

**O3. Action rows are strings, not buttons.**
`[ Allow ] Y    [ Deny ] N/Esc deny` (`view.rs:3047`), `[ Connect ] Enter    [ Cancel ] Esc` (`view.rs:3975`), and `[ Save ] Enter    [ Cancel ] Esc` (`view.rs:2867`) each invent their own spacing and their own key annotation format.
The permission string also contains the duplicated word `deny`.

**O4. The user profile dialog renders buttons it cannot style consistently.**
`[ Save ]` uses `Selected` and `[ Cancel ]` uses `Field` (`view.rs:1266`), so cancel appears as an inverted input field rather than a secondary button.

### Shell chrome

**W1. The compact footer offers two destinations that duplicate the Settings tabs.**
`render_shell_footer` renders only ` Profile  |  Settings ` (`view.rs:1037`) and is drawn under every route, including Help and Sessions, where it is meaningless.
Captured output shows it beneath the Help frame and beneath the Sessions frame.

**W2. The wide sidebar has one gradient element and no navigation.**
`render_navigation_rail` renders the brand, a session list, a `PROJECTS` heading, one workspace name, and the same two-item footer (`view.rs:1065`).
There is no route navigation in the rail even though five routes exist, and the sidebar is a fixed 28 columns at every width above the breakpoint.

**W3. Nerd Font mode changes exactly three glyphs.**
The entire Nerd Font code path consists of the sidebar brand prefix (`view.rs:1085`), the workspace path marker (`view.rs:3123`), and the Git branch marker (`view.rs:3131`).
No route, status, provider, model, tool, session, setting, or action has an icon in any glyph mode.

## Defect summary

| Area | Structural | Visual | Interaction |
| --- | --- | --- | --- |
| Foundation | S1 to S9 | | |
| Chat | | C1 to C8 | |
| Palette | | P1 to P4 | |
| Sessions | | S-1 to S-4 | |
| Settings | | T1 to T6, T11, T12, T14, T15 | T7 to T10, T13 |
| Providers | | V1 to V4 | |
| Models | | M1 to M3 | |
| Help | | H1 to H3 | |
| Overlays | | O1 to O4 | |
| Shell | | W1 to W3 | |

## Conclusion

The layout skeleton is sound: a bottom composer under a tail-following transcript, a sidebar at wide widths, and five typed routes with one modal owner are the right structure.
The defects are almost entirely in the layer that does not exist: there is no theme value, no token vocabulary, no icon set, no component library, and no shared layout contract.
Fixing the surfaces one at a time without that layer would reproduce the same inconsistency at a higher polish level, which is why the redesign plan builds the foundation first.
