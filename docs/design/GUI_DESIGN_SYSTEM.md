# GUI design system

**Status:** Active migration contract

**Last updated:** 2026-08-30

## Direction

AutoHarness keeps the atmosphere of its terminal interface without keeping terminal layout constraints.
The GUI should feel like a precise instrument: deep surfaces, luminous cyan-to-violet accents, restrained motion, monospace technical detail, and generous spatial hierarchy.

The interface is not a terminal emulator.
It uses native semantic controls, fluid layout, readable proportional text where appropriate, and rich desktop interactions.

## Principles

1. Keep the conversation calm and the controls quiet until they are needed.
2. Use whitespace, hierarchy, and translucency before borders.
3. Use the cyan-to-violet gradient only for focus, progress, identity, and major structure.
4. Render code, commands, paths, identifiers, and metrics in monospace, while ordinary prose uses a highly legible system sans-serif stack.
5. Express state with text, shape, icon, and contrast rather than color alone.
6. Keep one obvious primary action on every empty, offline, error, permission, and destructive surface.
7. Preserve user context when panes collapse, routes change, or the window resizes.
8. Prefer direct manipulation and discoverable controls while retaining fast keyboard paths.

## Shell

The wide shell has three regions:

- A 248-pixel navigation rail for identity, routes, recent sessions, and the active workspace.
- A flexible primary workspace with a readable conversation measure.
- An optional 320-pixel inspector for context, activity, permissions, and details.

The rail and inspector are independently collapsible.
Collapsed panes retain selection and scroll state.
The center workspace never shifts while text is being selected or a response is streaming.

At narrower widths, the inspector becomes a drawer and the rail becomes a compact icon bar.
At phone-like widths used only for resilience testing, navigation becomes a modal sheet and the composer remains fully usable.

## Surfaces

The default dark theme derives from the existing System seed:

- Base: `#080c18`.
- Cyan accent: `#22d3ee`.
- Violet accent: `#a78bfa`.

Surfaces use subtle Oklab lightness steps instead of unrelated hex tables.
Raised panels may use translucency only when the underlying layer remains predictable and contrast floors still hold.
Body content should not be surrounded by decorative boxes.

The nine existing theme identities remain recognizable.
CSS custom properties are generated from the same renderer-neutral seed and semantic-token source used by the legacy client.

## Typography

The default prose stack uses the operating-system UI font.
The technical stack uses `ui-monospace`, `SFMono-Regular`, `Cascadia Code`, `Consolas`, and sensible fallbacks.

Conversation prose targets 15 to 16 pixels with a relaxed line height.
Labels and metadata use 11 to 13 pixels with careful contrast rather than extreme letter spacing.
Code blocks, tool details, paths, and token metrics use the technical stack.

## Conversation

Messages form one open vertical flow rather than a stack of opaque chat bubbles.
User turns use a quiet raised surface and align to the content column.
Agent turns remain open on the base surface with a compact identity line, optional timing, and rich Markdown-safe content.
Tool activity appears as structured cards that can disclose trusted details without overwhelming the transcript.

The composer is a rounded command surface anchored to the visible conversation tail.
It grows within a bounded height, preserves a single scrollport, and exposes model, reasoning, attachment, command, and submission controls without turning into a toolbar wall.

Streaming uses a small gradient activity trace and incremental content.
Reduced-motion mode replaces movement with a stable progress state.

## Interaction states

Every interactive element has visible default, hover, focus-visible, active, disabled, loading, success, warning, and danger treatment where meaningful.
Keyboard focus uses a two-layer ring that remains visible on every theme.
Hover never carries information that is unavailable through focus or touch.

Permission requests use a top-authority modal with an explicit capability, resource, exact trusted fields, and clearly separated deny and allow actions.
Destructive confirmations name the exact target and consequence.

## Motion

Motion explains relationship or state change.
It does not decorate idle surfaces.

- Micro-interactions use 120 to 180 milliseconds.
- Pane and route transitions use 180 to 240 milliseconds.
- Streaming indicators repaint no faster than 100 milliseconds.
- Reduced-motion mode removes translation, scaling, parallax, and looping animation.

## Accessibility

Semantic HTML is mandatory.
Routes use landmarks and headings, dialogs trap and restore focus, status changes use restrained live regions, and every icon-only control has an accessible name.

Minimum contrast targets are:

- 7.0 for primary text on ordinary surfaces.
- 4.5 for secondary text and control labels.
- 3.0 for focus indicators and large non-text state shapes.

No-color and high-contrast modes preserve status with labels, icons, outlines, and patterns.
Zoom to 200 percent must preserve every primary action and security-critical detail.

## Initial component catalog

- `AppRail`.
- `RouteButton`.
- `SessionList`.
- `ConversationView`.
- `MessageTurn`.
- `ToolCard`.
- `Composer`.
- `ModelPicker`.
- `ContextInspector`.
- `StatusChip`.
- `Callout`.
- `Button`.
- `Field`.
- `Dialog`.
- `PermissionDialog`.
- `CommandPalette`.
- `SplitPane`.
- `VirtualList`.

Components receive typed props and callbacks.
They do not receive the transport, Rust application handle, or global coordinator context.

## Visual validation

Review at these initial viewport classes:

- Compact desktop: 900 by 640.
- Standard desktop: 1280 by 800.
- Wide desktop: 1600 by 1000.
- Resilience minimum: 640 by 480.

Each critical state receives semantic DOM assertions and screenshots.
The matrix includes dark and light bases, high contrast, reduced motion, 200 percent zoom, long content, permission, failure, offline, empty catalog, and active streaming.

