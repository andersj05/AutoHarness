# Desktop UX redesign validation

**Reviewed:** 2026-09-07

**Branch:** `feat/desktop-ux-redesign`, created from the latest fetched `dev`.

## Outcome

The desktop now uses quieter surfaces, a consistent sidebar, concise labels, and progressive disclosure across Chat, Sessions, Providers, Memory, Settings, and Help.
The [design system](../design/GUI_DESIGN_SYSTEM.md) owns the resulting interaction contract.
[Notion's sidebar navigation](https://www.notion.com/help/navigate-with-the-sidebar) and [Spotify's desktop library and optional detail panes](https://newsroom.spotify.com/2023-06-20/spotify-desktop-experience-redesign-your-library-now-playing-views-customize/) informed hierarchy and navigation, without copying branding or unsupported features.

## User-facing changes

- Sidebar Search combines commands and unarchived sessions, ranks label matches first, and opens the first enabled result with Enter.
- Chat has a focused composer, optional session details, response and code-block copy, transcript actions, and safe Markdown formatting.
- Sessions adds sorting and keyboard opening, and keeps selected details consistent with the visible results.
- Providers leads with model defaults and daily actions, while credential maintenance and connection metadata use disclosures.
- Memory leads with readable content and keeps its full audit trail available through Details and history.
- Settings shows one category at a time, offers visual theme choices, and searches individual settings plus their option labels across categories.
- The desktop always uses compact spacing and no longer exposes the Comfortable/Compact setting.
- Secondary labels use stronger contrast, while status badges use quieter surfaces.
- Help presents shortcuts and expandable guidance, with fewer introductory titles and descriptions.

The renderer still sends typed commands to the Rust authority.
Permissions retain their exact tool, resource, trusted fields, one-call scope, and Deny-first focus.
Credential fields remain masked and clear before native transfer.
Model-authored HTML, image references, and links remain inert.

## Defects reproduced and resolved

| End-user reproduction | Verified result |
| --- | --- |
| Create a session from the 640 by 480 navigation drawer | The drawer closes and the new composer is available |
| Send and immediately stop at 640 by 480 | The header remains at the top, with outer workspace scrollTop equal to zero |
| Stop before the first response chunk | No empty completed agent bubble or empty Copy action appears |
| Resize a failed conversation from 1280 by 800 to 640 by 480 | Retry remains visible above the composer; the recovery panel ends at approximately 282 pixels and the composer begins at 332 pixels |
| Resize while reading older messages | Tail following stays disabled and Jump to latest remains available |
| Set interface zoom to 200 percent | Search reduces to a named icon alongside the other compact navigation controls |
| Search models and use Arrow keys or Enter | Available models can be selected without reaching unavailable entries |
| Search Sessions until no result remains | Unrelated details and their destructive actions disappear |
| Open a session rename or delete dialog and press a global shortcut | Background actions are blocked; permission review can preempt the dialog |
| View Sessions or Providers at 907 pixels wide with the full navigation rail | Library content reflows; clientWidth and scrollWidth match for both route and detail panels |
| Search for Audit context and press Enter in the search field | The matching session opens and the search dialog closes |
| Select Light and press ArrowRight in visual theme selection | Dark becomes selected through the authoritative settings command |
| Copy one of two code blocks | Only the selected block is copied, including its whitespace |

Connection and catalog recovery messages now appear beside the composer instead of above the entire transcript.
The compact callout action spans the row without widening its icon column.
Fixture responses explicitly identify simulated results and do not claim persistence.

## Automated evidence

`python scripts/validate_gui.py` passed all 15 local baseline gates on this branch during implementation.
Its local report is `target/gui-evidence/local-baseline/baseline.json` with status `passed`, based on commit `611193d` plus the then-current working tree.
The report is local evidence, not an immutable release-candidate attestation.

The baseline includes frozen frontend installation, type checking, frontend tests and production build, Python tooling tests, documentation links, Rust formatting, generated theme freshness, strict all-feature Clippy, the complete locked Rust workspace tests, rustdoc, doctests, and storage-benchmark gates.
After the compact-layout refinement, frontend type checking, all 133 tests in 19 files, the production build, and generated-theme freshness passed again.
The final frontend run stopped on the first failed gate and completed successfully.
No Rust source changed in this redesign.

Focused coverage includes [safe message formatting](../../apps/gui/src/components/MessageContent.test.tsx), [tail following and resize](../../apps/gui/src/components/Conversation.test.tsx), [model keyboard selection](../../apps/gui/src/components/ModelPicker.test.tsx), [menu focus](../../apps/gui/src/components/primitives/ActionMenu.test.tsx), and [application modal ownership](../../apps/gui/src/App.test.tsx).
Existing provider, memory, store, protocol, and appearance tests remain part of the passing suite.

## Browser review

Browser fixtures were reviewed interactively with screenshots and accessibility-tree inspection.

| Viewport or treatment | Review |
| --- | --- |
| 1280 by 720 and 1280 by 800 | Light and dark conversation, library, providers, memory, and settings |
| 900 by 640 and 907 by 763 | Compact navigation, full-rail library reflow, memory filters, and paging |
| 640 by 480 | Navigation drawer, session creation, memory actions, send and stop, permission review, failed response, and offline recovery |
| 1600 by 1000 | Wide conversation measure, provider detail layout, two-column shortcuts, and high-contrast navigation |
| 200 percent interface zoom | Settings reflow, visual theme selection, compact Search, conversation actions, and scrollable model selection |
| Reduced motion | Enabled through Settings during zoom and keyboard review |

Keyboard model search selected the requested model and closed its dialog.
The global search Enter reproduction opened Audit context manifests after the fix.
Visual theme controls remained usable at 200 percent zoom, including scrolling and selection.
Code-copy feedback appeared in the browser, and unit tests verified exact clipboard text and the failure fallback.
The compact permission dialog preserved its exact fields in a scrollable body and kept Deny and Allow once visible.
The failure and offline recovery controls remained visible above the compact composer after the fixes.

## Limits and remaining release evidence

This review establishes renderer behavior in browser fixtures.
The available UI automation did not expose native application surfaces, so this task does not claim a new native webview, vault, persistence, live-provider, installer, or assistive-technology journey.
The passing Rust baseline remains distinct from visual native-product evidence.
Signed distribution, same-candidate cross-platform review, and human visual approval remain tracked in the [GUI release checklist](GUI_RELEASE_CHECKLIST.md).
