# ADR-0016: Use a typed terminal presentation layer

**Status:** Proposed

**Date:** 2026-08-27

**Owners:** Project maintainers

## Context and problem statement

The terminal client renders every surface from one 4088-line `view.rs` module that re-derives a complete Ratatui `Style` for every span of every frame through a nested match over five color modes and nine theme presets.
There is no resolved theme value, no semantic token vocabulary beyond eleven visual roles, no icon abstraction, no component library, and no shared layout contract.

The consequences are recorded in [TUI_AUDIT.md](../design/TUI_AUDIT.md) and are structural rather than cosmetic.
Nine themes are nine unrelated hand-written tables with one duplicated pair.
Three competing mechanisms decide whether a cell background is transparent.
All palettes emit twenty-four-bit color with no terminal capability detection.
No test asserts any contrast ratio, and snapshot tests read only cell symbols, so no color, focus, or contrast property in the application is covered by any gate.
Layout thresholds are duplicated between rendering and hit testing, which has already produced six unreachable mouse actions and one hit-test function that always returns nothing.
Nerd Font support exists as a preference but changes exactly three glyphs.

A visual overhaul is required now, and the current shape means each surface would be polished with its own private spacing, alignment, and glyph decisions, reproducing the present inconsistency at higher effort.
A decision is needed because the fix introduces a new internal module boundary and a set of gates that constrain all future terminal work.

## Decision drivers

- The audit's defects are concentrated in a layer that does not exist, so surface-by-surface polish cannot fix them durably.
- Appearance must be verifiable, and today no automated gate can detect a color, contrast, or focus regression.
- The product must remain fully usable without color, without a Nerd Font, and on terminals that do not report truecolor.
- The repository prefers quality, simplicity, robustness, and long-term maintainability over development cost.
- Route, overlay, focus, intent, and settings contracts are stable and must not be disturbed by a presentation change.
- Adding a theme, an icon, or a page should be a small localized change rather than an edit across three unrelated tables.

## Considered options

1. **Incremental surface polish.** Keep `view.rs` and improve each route in place, adding colors and glyphs where needed.
2. **Typed presentation layer inside the TUI crate.** Introduce a `ui/` module tree containing an immutable per-frame `Theme`, a semantic token table, gradient and icon engines, a component library, and one layout pass that also produces hit regions.
3. **Separate presentation crate.** Extract the same layer into a new workspace crate with a published boundary.
4. **Adopt a third-party terminal component framework.** Replace hand-written widgets with an external Ratatui component library.

## Decision outcome

Chosen option: **a typed presentation layer inside the TUI crate**, because it removes the root cause without adding a process or crate boundary that no measurement justifies yet.

The layer is specified by [TUI_DESIGN_SYSTEM.md](../design/TUI_DESIGN_SYSTEM.md) and delivered by [TUI_REDESIGN_PLAN.md](../design/TUI_REDESIGN_PLAN.md).
Its binding rules are:

- Appearance resolves exactly once per frame into an immutable `Theme`; no render function constructs a `Style` or a `Color`.
- Color literals exist only in the palette and color modules, and glyph literals only in the icon and component modules, both asserted by source checks.
- A theme is defined by a four-value seed and a shared derivation, not by a hand-written table.
- Terminal color depth is detected once and the palette is quantized during resolution.
- Every token pair used for text has an enforced contrast floor asserted across all presets and color modes.
- Every icon is a Nerd Font, Unicode, and ASCII triple with an asserted cell width.
- State is conveyed by fill plus glyph, never by foreground color alone.
- One layout pass produces both the named rectangles used for rendering and the ordered hit regions used for mouse dispatch.
- Snapshots capture style as well as symbols.

Option 1 was rejected because it leaves every structural finding in place.
Option 3 was rejected because the layer has exactly one consumer and the modular-monolith guardrail requires a measured need before a new boundary.
Option 4 was rejected because the product's visual identity, accessibility degradation, and glyph strategy are differentiators rather than commodity chrome, and an external dependency would own them.

## Consequences

### Positive

- Appearance becomes testable, so color, contrast, focus, glyph width, and hit coverage all gain gates that do not exist today.
- Adding a theme is a four-value change and adding an icon is a three-value change.
- The transparent-background, duplicated-threshold, and unreachable-mouse-action classes of defect become impossible to reintroduce.
- Terminals without truecolor, without color, and without a Nerd Font get deliberate rather than accidental output.
- Parallel work on separate routes stays visually consistent because the component catalog is the shared vocabulary.

### Negative

- A large mechanical refactor of the whole terminal crate, with heavy snapshot churn during the transition.
- Nine themes will shift appearance when hand-written tables are replaced by a shared derivation, and each must be reviewed.
- Oklab conversion, contrast clamping, and quantization add resolution cost, though once per frame rather than once per span.
- Gradient sampling on borders and meters adds spans that must be measured against the existing benchmark envelope.

### Follow-up

- Land the wall-clock projection field and the style-aware snapshot helper before any color code changes.
- Measure render cost and allocation behavior in the conformance step and compare against the recorded pre-redesign envelope.
- Mark this record Accepted only after the redesign passes the cross-platform PTY matrix and the three real terminal smokes.
- Revisit extraction into a separate crate only if a second consumer, such as a remote or headless client, appears.

## Evidence

- [Terminal interface audit](../design/TUI_AUDIT.md), with buffer captures and source citations for every finding.
- [Terminal design system](../design/TUI_DESIGN_SYSTEM.md), the target contract.
- [Terminal interface redesign plan](../design/TUI_REDESIGN_PLAN.md), the ordered delivery steps and exit criteria.
- [Step 10 validation](../release/TUI_REDESIGN_VALIDATION.md), with the completed local gates and the cross-platform and human-review gates that still block acceptance.
- Rendered evidence from `crates/autoharness-tui/tests/visual_review.rs` and the four goldens under `crates/autoharness-tui/tests/golden/`.

## Related decisions

- [ADR-0001](0001-use-rust-modular-monolith.md) constrains this layer to stay inside the modular monolith until a measured need justifies a crate boundary.
- [ADR-0012](0012-use-typed-settings-resolver.md) owns the preference keys this layer reads; no new key is introduced.
