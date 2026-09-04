# GUI Stage 7 validation

**Date:** 2026-09-03

**Status:** Implemented and validated locally; cross-platform desktop release evidence remains open.

## Delivered behavior

The Memory workspace exposes literal search, scope and lifecycle filters, bounded cursor paging, explicit memory creation, confined document import, provenance, evidence availability, relations, validation findings, admission history, proposal approval and rejection, correction, retraction, standalone export, and confirmed content deletion.
Actions retain the terminal reference's distinction between active, conflicting, expired, proposed, and deleted records.
Revision-changing commands carry the exact sequence and proposal or revision identity required by the existing Rust authority.
Approval creates a distinct user-approved revision; imported and model-authored proposals cannot approve themselves.

The [schema-v4 memory contract](../../crates/autoharness-client/src/memory.rs) bounds pages to 100 records and 8 MiB of serialized JSON.
An unrepresentable memory page becomes a recoverable workspace failure without preventing the rest of the client from opening or settling commands.
Query generations distinguish the requested view from delayed pages, while mutation generations and exact sequence checks preserve stale-review rejection.

The [typed presentation slots](../../apps/gui/src/features/workspace/slots.tsx) cover plans, artifacts, files, comparisons, terminal output, and evaluation evidence.
Every surface renders text inertly, including hostile markup, paths, commands, terminal escape sequences, and URL-shaped strings.
The native inspector exposes only existing bounded tool evidence.
All six presentation forms have browser fixtures; this does not implement the future planning or evaluation runtimes, arbitrary file access, terminal execution, or plugin authority.

## Automated evidence

- [Client wire tests](../../crates/autoharness-client/tests/wire_contract.rs) pass with exact decimal identities, no trust-authoring fields, content-redacted diagnostics, bounded unique rows, aggregate serialization limits, and memory changes excluded from session-only deltas.
- [Native memory tests](../../crates/autoharness-app/src/gui/memory_tests.rs) pass through serialized command ingress, correlated ordered frames, the real coordinator, SQLite, and two shutdown/restart boundaries.
- The native journey remembers content, imports an untrusted document, approves a distinct revision, rejects stale approval, verifies replay-equivalent rows, corrects, exports, retracts, deletes, and verifies replay-equivalent deletion.
- Native tests reject traversal, absolute and URL-shaped import paths, excessive search input, and unsafe page representation before it can become an actionable view.
- [Workspace tests](../../apps/gui/src/features/memory/MemoryWorkspace.test.tsx) cover deliberate approval, correction comparisons, export, retraction, exact deletion confirmation, literal search, empty results, opaque paging, stale review, and permission preemption.
- [Rich-content tests](../../apps/gui/src/features/workspace/slots.test.tsx) verify all six forms produce no executable HTML, links, scripts, styles, images, or command controls.
- The complete GUI suite passes with 113 tests, and frontend type checking and production build pass.
- Rust workspace formatting, strict all-target/all-feature Clippy, and the complete locked workspace test suite pass.
- Focused client and native tests also pass after the final aggregate-limit and recoverable-failure changes.

## Visual and interaction evidence

Live browser review covers 900 by 640, 1280 by 800, 1600 by 1000, and 640 by 480 viewports.
The reviewed states include light and dark themes, proposal review, correction, inert comparison content, mobile navigation, high contrast, and 200 percent application zoom.
The review fixed inherited light-theme text colors, excessive selection fill, mobile focus scrolling of the application root, and dialog heights that previously ignored application zoom.
At 200 percent zoom in a 1280 by 800 viewport, the correction dialog occupies vertical coordinates 48 through 752 and its confirmation footer remains inside the viewport.
Browser warning and error logs are empty for the reviewed flow.

## Remaining release evidence

Browser fixtures establish rendering and interaction behavior, not native system-webview parity.
The native automated journey establishes the real GUI protocol and storage lifecycle, not a packaged desktop or live-provider release claim.
Windows, macOS, and Linux system-webview screenshot, assistive-technology, packaging, installer, update, and release approval work remains in the [GUI migration plan](../design/GUI_IMPLEMENTATION_PLAN.md).
The GUI remains a development preview, with the temporary TUI projection adapter still explicit and the terminal still available as the migration reference.
