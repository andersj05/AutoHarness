# Terminal design system

**Reviewed:** 2026-08-27

**Status:** Proposed target contract for the redesign; not yet implemented.

**Authority:** This document is the single source of truth for terminal visual tokens, glyphs, components, and responsive rules.
Route, overlay, focus, and intent semantics stay in `docs/architecture/OVERVIEW.md`.
Preference keys and layer precedence stay in `docs/architecture/SETTINGS.md`.
The defects this contract exists to fix are recorded in [TUI_AUDIT.md](TUI_AUDIT.md), and the delivery order is in [TUI_REDESIGN_PLAN.md](TUI_REDESIGN_PLAN.md).

## Principles

The interface is a terminal-native product, not a web layout transplanted into cells.
Every visual decision must survive an 80x24 terminal, a monochrome terminal, and a terminal without a Nerd Font.

1. Resolve appearance once per frame into an immutable value; never re-derive style inside a render function.
2. Name every color by intent, never by hue, and never inline a literal color outside the palette module.
3. Prefer alignment and whitespace over borders; a box is the last resort, not the default container.
4. Show state with a full-width fill plus a glyph, never with foreground color alone, so no-color and colorblind users get the same information.
5. Make every glyph optional; each icon must have a Nerd Font, Unicode, and ASCII form that occupy the same cell budget.
6. Degrade by dropping the lowest-priority content, never by overflowing or clipping mid-word.
7. Gradients are accents on structure (rules, meters, focused borders, brand text), never decoration on body text.
8. Anything the user can act on must render a visible affordance and export its own hit region.

## Layer architecture

The presentation layer replaces the current single `view.rs` module with an explicit pipeline.

```
EffectiveLocalPreferences + terminal capability
        v
resolve()  ->  Theme  (immutable, one per frame)
        v
Layout::compute(area, route, Theme)  ->  Frame tree of named Rects + HitRegions
        v
Components (Panel, ListView, SettingRow, Meter, Modal, ...)  ->  Ratatui widgets
```

Module layout inside `crates/autoharness-tui/src/ui/`:

| Module | Responsibility |
| --- | --- |
| `ui/color.rs` | `Rgb`, Oklab conversion, mixing, relative luminance, contrast ratio, quantization to 256 and 16 color |
| `ui/palette.rs` | Theme seeds and the derivation that expands a seed into a full ramp |
| `ui/tokens.rs` | The semantic token table and the `ColorMode` treatment functions |
| `ui/theme.rs` | `Theme`, `resolve(preferences, capability)`, and the public `style(token)` accessor |
| `ui/gradient.rs` | Multi-stop gradient sampling and the derived span builders |
| `ui/icon.rs` | The `Icon` enum, the glyph triple table, and `IconSet` resolution |
| `ui/metrics.rs` | Breakpoints, spacing scale, and named insets |
| `ui/motion.rs` | Frame tables, tick math, and the reduced-motion gate |
| `ui/layout.rs` | The per-route layout pass and the hit-region map |
| `ui/component/*.rs` | One module per component listed below |
| `ui/page/*.rs` | One module per route, composing components only |

`view.rs` becomes a thin entry point that resolves the theme, computes the layout, and dispatches to a page module.
No page module may construct a `Style` or a `Color` directly.

## Color model

### Representation

All internal color arithmetic uses a linear-light `Rgb` newtype.
Mixing, gradient interpolation, and lightness adjustment convert to Oklab first, because interpolating cyan to violet in sRGB passes through a desaturated grey that is clearly visible on a 40-cell rule.
Only the final conversion to `ratatui::style::Color` leaves the module.

### Terminal capability

`ColorDepth` is detected once at startup and carried on the `Theme`.

| Depth | Detection | Emission |
| --- | --- | --- |
| `TrueColor` | `COLORTERM` contains `truecolor` or `24bit` | `Color::Rgb` |
| `Indexed256` | `TERM` contains `256color` | nearest xterm-256 index, computed from the same Oklab distance |
| `Basic16` | otherwise | nearest of the sixteen ANSI colors, with bold used to reach the bright half |

Quantization happens once during theme resolution, so no per-frame cost is added.
The detected depth is displayed in the Settings `About` category so a user can see why colors look flat, and an override preference is out of scope for this redesign.

### Theme seeds

A theme is defined by four values, not by a ten-tuple.

```
struct Seed {
    base: Rgb,        // surface background anchor
    accent_a: Rgb,    // primary accent and gradient start
    accent_b: Rgb,    // secondary accent and gradient end
    light: bool,      // inverts the lightness direction of derived steps
}
```

The nine existing preset names are preserved so no settings schema change is required.
Seeds reuse the current anchors so the redesign is recognizably the same product.

| Preset | base | accent_a | accent_b | light |
| --- | --- | --- | --- | --- |
| System | `08,0C,18` | `22,D3,EE` | `A7,8B,FA` | false |
| Dark | `05,07,0E` | `22,D3,EE` | `A7,8B,FA` | false |
| Light | `FA,FA,FB` | `25,63,EB` | `DB,27,77` | true |
| Aurora | `04,0F,1E` | `2D,D4,BF` | `81,8C,F8` | false |
| Ember | `1A,0A,0A` | `FB,92,3C` | `F4,3F,5E` | false |
| Midnight | `03,07,12` | `60,A5,FA` | `63,66,F1` | false |
| Ocean | `02,14,20` | `22,D3,EE` | `0E,A5,E9` | false |
| Forest | `07,14,0D` | `4A,DE,80` | `FA,CC,15` | false |
| Rose | `1D,08,14` | `F4,72,B6` | `C0,84,FC` | false |

`System` and `Dark` are no longer identical: `Dark` uses a deeper base so the two presets are distinguishable, which resolves audit finding S6.

### Derivation

Every token is derived from the seed by fixed ratios, so all nine themes share tone relationships.

| Derived step | Rule |
| --- | --- |
| `surface_base` | `base` |
| `surface_sunken` | `base` shifted 4 percent away from the text direction |
| `surface_raised` | `base` shifted 6 percent toward the text direction |
| `surface_overlay` | `base` shifted 10 percent toward the text direction |
| `text_primary` | contrast-maximizing near-neutral tinted 4 percent toward `accent_a` |
| `text_secondary` | `text_primary` mixed 30 percent into `surface_base` |
| `text_muted` | `text_primary` mixed 55 percent into `surface_base`, then lifted until it reaches the muted contrast floor |
| `accent` | `accent_a` |
| `accent_alt` | `accent_b` |
| `accent_soft` | `accent_a` mixed 70 percent into `surface_base` |
| `border_subtle` | `text_muted` mixed 55 percent into `surface_base` |
| `border_strong` | `text_secondary` mixed 25 percent into `surface_base` |
| `border_focus` | `accent_a` |

Semantic colors are fixed hues adjusted for the seed lightness rather than derived from the accents, because a red must stay red in the Forest theme.

| Token | Dark base | Light base |
| --- | --- | --- |
| `success` | `4A,DE,80` | `15,80,3D` |
| `warning` | `FB,BF,24` | `A1,62,07` |
| `danger` | `FB,71,85` | `BE,12,3C` |
| `info` | `60,A5,FA` | `1D,4E,D8` |

### Token table

Tokens are the only vocabulary a page module may use.
This replaces the eleven-variant `VisualRole`.

**Surfaces**

`surface_base`, `surface_sunken`, `surface_raised`, `surface_overlay`, `surface_scrim`, `surface_selected`, `surface_selected_muted`, `surface_danger`, `surface_warning`, `surface_success`.

**Text**

`text_primary`, `text_secondary`, `text_muted`, `text_disabled`, `text_on_accent`, `text_on_danger`, `text_link`.

**Accents**

`accent`, `accent_alt`, `accent_soft`, `accent_on_surface`.

**Semantic**

`success`, `warning`, `danger`, `info`, and their `_soft` surface pairs.

**Chrome**

`border_subtle`, `border_strong`, `border_focus`, `divider`, `scrollbar_track`, `scrollbar_thumb`, `focus_ring`.

**Conversation roles**

`role_user`, `role_assistant`, `role_tool`, `role_system`.

Each token resolves to a `Style` carrying an explicit foreground and an explicit background intent, where the background intent is one of `Inherit`, `Surface(token)`, or `Transparent`.
`Inherit` is the default, which removes the need for `chat_visual_style` and `transparent_chat_text` and resolves audit finding S3.

### Color mode treatments

`ColorMode` is applied during resolution, not at each call site.

| Mode | Treatment |
| --- | --- |
| `Color` | Tokens as derived |
| `Soft` | Chroma of every accent and semantic token reduced 35 percent in Oklab; contrast floors still enforced; no `DIM` modifier is used because `DIM` is unevenly supported |
| `Vivid` | Chroma increased 25 percent, accents lightened 8 percent, `BOLD` applied to accent and semantic text tokens only |
| `NoColor` | All colors dropped; distinction carried by `BOLD`, `REVERSED`, `UNDERLINED`, and by the icon set; selection becomes a reversed full-width fill |
| `HighContrast` | Two-value surface set (pure base and pure inverse), `border_strong` everywhere, `BOLD` on all accent and semantic text, selection reversed and underlined |

### Contrast floors

Enforced by a test over the full matrix of nine presets and five color modes.

| Pair | Minimum ratio |
| --- | --- |
| `text_primary` on any surface token | 7.0 |
| `text_secondary` on any surface token | 4.5 |
| `text_muted` on `surface_base` | 3.5 |
| `text_on_accent` on `surface_selected` | 4.5 |
| any semantic text on its `_soft` surface | 4.5 |
| `border_focus` on `surface_base` | 3.0 |

Derivation clamps toward the floor rather than failing, and the test asserts the clamp held.

## Gradients

### Model

```
struct Gradient { stops: [(f32, Rgb); 3] }
```

Three stops per theme: `accent_a` at 0.0, a midpoint at 0.5, and `accent_b` at 1.0.
The midpoint is `mix_oklab(accent_a, accent_b, 0.5)` lifted 6 percent in lightness, which prevents the muddy centre that a two-stop sRGB blend produces today.

`Gradient::sample(t: f32) -> Rgb` interpolates in Oklab between the bracketing stops.
Callers always pass a normalized position, never an index and a count, so a gradient is reproducible across regions of different widths.

### Sanctioned uses

| Use | Direction |
| --- | --- |
| Sidebar divider | vertical, full height |
| Transcript-to-composer rule | horizontal, full width |
| Focused panel border | perimeter, clockwise from top-left |
| Active navigation indicator | horizontal, tab width |
| Thinking meter fill | horizontal, filled segments only |
| Context utilization gauge | horizontal, filled segments only, overridden by `warning` above 70 percent and `danger` above 90 percent |
| Brand and page titles | per character across the string |
| Streaming activity wave | horizontal, animated phase offset |

Body text, list rows, help text, and setting values never use a gradient.

### Degradation

`NoColor` replaces every gradient with `border_subtle` and, for meters, with filled and empty ASCII segments.
`HighContrast` replaces every gradient with a solid `accent`.
`Basic16` samples the gradient at three points only, because a per-cell ramp quantized to sixteen colors produces visible banding worse than a flat rule.

## Icons

### Contract

Every icon is a triple.
`IconSet` is resolved once from `GlyphMode` and stored on the `Theme`.
No page module may write a glyph literal.

Cell budget rule: Unicode and ASCII forms must measure exactly one cell under `unicode-width`.
Nerd Font forms occupy a reserved two-cell slot rendered as the glyph plus one space, because several terminals report Nerd Font private-use codepoints as ambiguous width and would otherwise shift the following column.
A test asserts the measured width of every entry in all three modes.

### Table

| Icon | Nerd Font | Unicode | ASCII |
| --- | --- | --- | --- |
| `Brand` | `nf-fa-cube` | `◆` | `#` |
| `RouteChat` | `nf-fa-comment` | `▣` | `c` |
| `RouteSessions` | `nf-fa-history` | `☰` | `s` |
| `RouteProviders` | `nf-fa-cloud` | `⌘` | `p` |
| `RouteSettings` | `nf-fa-cog` | `⚙` | `*` |
| `RouteModels` | `nf-fa-cubes` | `◈` | `m` |
| `RouteHelp` | `nf-fa-question_circle` | `?` | `?` |
| `User` | `nf-fa-user` | `☺` | `u` |
| `Assistant` | `nf-fa-magic` | `◆` | `a` |
| `Tool` | `nf-fa-wrench` | `⚒` | `&` |
| `Workspace` | `nf-oct-file_directory` | `▸` | `/` |
| `GitBranch` | `nf-dev-git_branch` | `⑂` | `*` |
| `Model` | `nf-fa-circle` | `●` | `o` |
| `Thinking` | `nf-fa-lightbulb_o` | `◐` | `@` |
| `Context` | `nf-fa-bars` | `▰` | `=` |
| `Tokens` | `nf-fa-database` | `Σ` | `T` |
| `Success` | `nf-fa-check_circle` | `✔` | `+` |
| `Warning` | `nf-fa-warning` | `⚠` | `!` |
| `Danger` | `nf-fa-times_circle` | `✖` | `x` |
| `Info` | `nf-fa-info_circle` | `ⓘ` | `i` |
| `Pending` | `nf-fa-circle_o_notch` | animated braille table | animated ASCII table |
| `Connected` | `nf-fa-link` | `●` | `*` |
| `Disconnected` | `nf-fa-chain_broken` | `○` | `-` |
| `Locked` | `nf-fa-lock` | `⚿` | `K` |
| `Search` | `nf-fa-search` | `⌕` | `/` |
| `Collapsed` | `nf-fa-chevron_right` | `▸` | `>` |
| `Expanded` | `nf-fa-chevron_down` | `▾` | `v` |
| `SelectionCaret` | `nf-fa-chevron_right` | `❯` | `>` |
| `PromptCaret` | `nf-fa-chevron_right` | `❯` | `>` |
| `Archived` | `nf-fa-archive` | `▪` | `~` |
| `Default` | `nf-fa-star` | `★` | `!` |
| `Reset` | `nf-fa-undo` | `↺` | `^` |
| `Inherited` | `nf-fa-arrow_down` | `↓` | `v` |

Icon names reference the Nerd Font class names rather than raw codepoints so the table stays reviewable; the implementation resolves them to constants in `ui/icon.rs`.
Nerd Font forms use only BMP private-use codepoints because supplementary-plane Material Design symbols render as replacement diamonds in otherwise compatible Windows terminal setups.

### Glyph verification row

Settings gains a read-only `Glyph check` row in the `Appearance` category that renders one line containing every icon in the currently selected mode.
Because Nerd Font availability cannot be detected reliably from inside a terminal, this lets a user confirm visually before committing to Nerd Font mode.

## Metrics

### Width breakpoints

Named once in `ui/metrics.rs` and used by both rendering and hit testing, replacing the duplicated literals in audit finding S8.

| Band | Columns | Shell |
| --- | --- | --- |
| `Xs` | under 48 | single column, no sidebar, icon-only navigation |
| `Sm` | 48 to 71 | single column, no sidebar, compact labels |
| `Md` | 72 to 99 | single column with two-pane pages where the page needs it |
| `Lg` | 100 to 139 | sidebar plus content |
| `Xl` | 140 and above | sidebar plus content plus an optional detail pane |

### Height breakpoints

| Band | Rows |
| --- | --- |
| `Short` | under 20 |
| `Medium` | 20 to 35 |
| `Tall` | 36 and above |

`Short` suppresses page subtitles and reduces the composer to its minimum.

### Spacing scale

Only these values may be used: `0`, `1`, `2`, and `4` cells.
Page gutters are `2` at `Md` and above and `1` below.
Panel padding is `1` horizontally and `0` vertically unless the panel has a title, in which case `1` vertically.
The sidebar is `26` columns at `Lg` and `32` at `Xl`, replacing today's fixed `28`.

## Components

Each component is a module under `ui/component/`, owns its own measurement, and returns the hit regions it created.
Every component is unit-tested by rendering into a `TestBackend` at `40`, `60`, `80`, and `120` columns and asserting both symbols and styles.

| Component | Contract |
| --- | --- |
| `Panel` | Optional icon, title, subtitle, and footer hint row. Border style is `border_subtle` when unfocused and a gradient perimeter when focused. Returns the inner rect. |
| `Scrim` | Dims a whole region by replacing every cell style with `surface_scrim` before a modal draws. Fixes audit finding O1. |
| `Modal` | Scrim plus `Panel` plus body plus a right-aligned `ButtonRow`. One sizing function with a single clamp table replaces the five existing helpers. |
| `ButtonRow` | Buttons with primary, secondary, and danger variants; renders `label` plus key annotation; measures itself; exports `Vec<(Rect, MouseAction)>`. Fixes audit findings V2 and O3. |
| `KeyValueTable` | Computes the label column from the widest label, right-aligns an optional trailing chip, and wraps values in a hanging indent. Fixes audit findings T2, H1, and V3. |
| `ListView` | Full-width selection fill plus caret icon, optional sticky group headers, optional right-aligned metadata column, gradient scrollbar thumb when content overflows, typed empty state, and a row-to-`MouseAction` hit map. |
| `Chip` | One-cell-padded label in a semantic surface pair. Variants: `neutral`, `accent`, `success`, `warning`, `danger`, `muted`. Used for `active`, `archived`, `default`, `connected`, provenance, and failure codes. |
| `Meter` | Segmented gradient fill with a label, a numeric value, and a threshold override. Serves the thinking level and the context gauge. |
| `SegmentedControl` | Renders all options as adjacent chips with the current one filled at `Md` and above; falls back to `‹ current ›` with an `n/m` position indicator below `Md`. Fixes audit finding T11. |
| `SettingRow` | Renders one of `Toggle`, `Choice`, `Text`, `Action`, or `Info` from data, with label, control, provenance chip, and description. The variant is the single authority for whether the row is editable, which removes the render-versus-update disagreement in audit finding T3. |
| `StatusBar` | Priority-ordered segments; measures each and drops the lowest priority until the line fits the actual remaining width. Fixes audit finding C4. |
| `MessageBlock` | A conversation turn: two-cell role gutter with icon and rule, role name, right-aligned turn metadata, wrapped body, and optional footer callout. Fixes audit findings C5 and C6. |
| `ToolCard` | Status icon, tool name, target, duration, expand caret, and an indented detail body when expanded. Fixes audit finding C8. |
| `Callout` | Bordered semantic block with icon, title, message, and an optional `ButtonRow`. Used for failures, offline state, and empty catalogs. |
| `Hero` | Centered brand gradient, headline, numbered step chips, and a highlighted next action. Used by the onboarding and empty-conversation states. |
| `SearchField` | Icon, inline query with a visible cursor, and a right-aligned match count. Replaces the inverted `Filter:` slabs in audit findings S-3 and P1. |

## Layout and hit regions

`Layout::compute(area, model, theme)` runs once per frame before any drawing and returns:

```
struct Frame {
    regions: NamedRects,               // sidebar, content, footer, page-specific rects
    hits: Vec<(Rect, MouseAction)>,    // in paint order, last match wins
}
```

Rendering reads `regions`; `hit_test` becomes a reverse scan of `hits`.
No threshold literal may appear outside `ui/metrics.rs`.
This deletes the duplicated geometry in audit finding S8 and makes the unreachable actions in audit finding S9 either wired or removed.

## Motion

`Motion` carries the tick source and the reduced-motion flag.
Frame tables live in `ui/motion.rs` and are selected by glyph mode.

| Animation | Nerd Font and Unicode | ASCII | Reduced motion |
| --- | --- | --- | --- |
| `Pending` spinner | eight braille frames | four ASCII frames | static `Icon::Pending` first frame |
| Streaming wave | six-cell gradient phase sweep | `[==>----]` bar | static filled bar |
| Startup indicator | spinner plus gradient rule | spinner plus ASCII rule | static text |

No animation may exceed one repaint per 100 ms, and no animation may run when the terminal has not been resized or focused for more than 30 seconds.

## Testing contract

| Gate | Assertion |
| --- | --- |
| Contrast matrix | Every token pair meets its floor across nine presets and five color modes |
| Icon width | Every icon measures one cell in Unicode and ASCII, and two cells in the Nerd Font slot |
| Style-aware snapshots | Snapshots capture symbol, foreground, background, and modifiers so color regressions fail |
| Component matrix | Every component renders correctly at 40, 60, 80, and 120 columns |
| Route matrix | Every route renders at 40x12, 60x18, 80x24, 120x40, and 120x50 in `Color`, `NoColor`, `HighContrast`, ASCII, compact, and single-column modes |
| No literal colors | A source check asserts `Color::Rgb`, `Color::Indexed`, and named `Color::` constants appear only inside `ui/palette.rs` and `ui/color.rs` |
| No literal glyphs | A source check asserts box-drawing and symbol codepoints appear only inside `ui/icon.rs` and `ui/component/` |
| Hit-region coverage | Every `MouseAction` variant is produced by at least one layout case |

## Out of scope

This contract deliberately excludes image protocols, mouse hover states, terminal hyperlinks, per-user custom theme files, and a font-family preference.
Each would need its own decision record.
