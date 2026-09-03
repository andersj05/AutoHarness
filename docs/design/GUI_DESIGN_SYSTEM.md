# GUI design system

**Status:** Active migration contract

**Last updated:** 2026-09-03

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
- `ProvidersWorkspace`.
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

## Implemented Stage 3 contract

The renderer-neutral `autoharness-presentation` crate is the source of truth for the nine theme seeds, five color treatments, semantic color ramps, and contrast floors.
Its checked generator produces the complete GUI custom-property matrix, while the TUI consumes the same resolved ramps through its renderer adapter.

The GUI token layer adds semantic typography, spacing, elevation, radii, focus, motion, responsive dimensions, control sizes, and stacking levels.
The shared primitive catalog includes `Button`, `Field`, `Chip`, `Menu`, `Dialog`, `CommandPalette`, `SplitPane`, `VirtualList`, `Callout`, `ToolCard`, `Meter`, and `StatusSurface`.
These primitives remain transport-free and expose native roles, accessible names, focus behavior, keyboard interaction, and text or shape redundancy for semantic states.

The live shell consumes the shared appearance matrix, command palette, split pane, virtual session list, status surfaces, meters, tool cards, fields, chips, and buttons.
Permission review remains the highest-authority dialog and preempts the command palette and ordinary shortcuts.
Reduced-motion preference is accepted from the operating system and can also be enabled through presentation settings.

## Implemented Stage 5 provider workspace

The Providers workspace uses a master-detail layout with a bounded profile list, one selected detail surface, and compact status, scope, credential-source, and active-state labels.
Named profiles expose grouped connection and configuration actions, while the temporary session-default row is visibly distinct and omits durable edit, test, default, and deletion controls.
Environment overrides use a prominent explanatory callout and describe saved vault material only as a fallback.
Credential controls keep the masked field next to the immediate-transfer boundary, stack at narrow widths, and clear before native transfer.
Model and reasoning defaults form one atomic action against the active authoritative catalog.
Codex authentication presents one native-browser action and a correlated cancellation state without rendering tokens.
Permanent profile deletion moves into a separate danger zone and remains disabled until the exact profile identity is typed.

## Implemented Stage 6 personalization and accessibility

The Settings workspace groups all eight renderer-relevant preferences into Appearance, Accessibility, and Conversation sections with searchable labels and descriptions.
Every row presents the effective value, its authoritative source, an explanation, and a Reset action when a user-file override exists.
An override hidden by a higher-precedence layer remains visible as a warning so reset never appears ineffective or ambiguous.

System theme and motion preferences follow operating-system media queries until the user selects an explicit value.
Zoom from 75 through 200 percent, four font sizes, comfortable and compact density, timestamp visibility, color treatment, and composer submission all update the live shell from the host projection.
At high zoom the rail compacts, route workspaces reflow, and the inspector becomes an overlay while every primary and security-critical action remains reachable.

The application exposes named navigation and main landmarks, a skip link, route headings, icon labels, polite status announcements, and deterministic document order.
Alt+1 through Alt+5 opens each primary route and restores focus to its main landmark.
Permission and credential dialogs retain labelled descriptions, logical screen-reader order, focus containment, and focus restoration.

## Visual validation

Review at these initial viewport classes:

- Compact desktop: 900 by 640.
- Standard desktop: 1280 by 800.
- Wide desktop: 1600 by 1000.
- Resilience minimum: 640 by 480.

Each critical state receives semantic DOM assertions and screenshots.
The matrix includes dark and light bases, high contrast, reduced motion, 200 percent zoom, long content, permission, failure, offline, empty catalog, and active streaming.

Automated checks enforce all 45 theme and treatment combinations, generated-file freshness, documented contrast floors, two-layer focus visibility, reduced-motion overrides, and semantic-state redundancy.
Local browser review covers the compact, standard, wide, and resilience viewport classes.
Native system-webview review is recorded per operating system and remains a release-gate requirement where the target host is unavailable locally.
