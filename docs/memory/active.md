# Active memory

**Last reviewed:** 2026-08-25

**Phase:** 3.9 terminal product validation

**Status:** Command and Settings integration is implemented on `feat/tui-command-settings`; 134 focused TUI tests, strict TUI Clippy, formatting, visual review, and a clean built-binary Windows launch smoke passed, while full Phase 3.9 release evidence remains pending

## Current objective

Run and record the release-candidate baseline, PTY, migration, benchmark, live-provider, vault, and rollback gates without reopening stabilized terminal contracts.

## Current repository state

- The repository contains an eleven-crate Rust 2024 workspace pinned to Rust 1.97.1 and a runnable `autoharness` terminal binary.
- `autoharness-domain` and `autoharness-engine` define schema-v1 commands and events, deterministic replay, durable attempt and tool lifecycles, immutable run limits, explicit permissions, cancellation state, usage, safe failures, and retry lineage.
- `autoharness-tool` provides the trusted versioned tool registry, deny, ask, and allow policy, restart-aware run budgets, content-addressed artifacts, and workspace filesystem, direct-process, and exact-origin HTTP capability ports.
- `autoharness-provider` exposes provider-neutral catalog, streaming, versioned tool definition, complete tool call, and tool-result ports.
- `autoharness-provider` also provides shared SSE framing, fixture conformance assertions, capability preflight, deadlines, bounded pre-stream retries, concurrency, per-project rate limits, and catalog freshness policy.
- `autoharness-provider-gemini` implements paginated Google model discovery, stable Interactions v1 streaming, a narrow pre-stream Generate Content fallback, cancellation, retry classification, limits, environment or in-app credential admission, and credential redaction.
- `autoharness-provider-openai` implements configurable OpenAI-compatible router discovery and streamed chat completions with a validated base URL, configurable sensitive authentication header, pagination, cumulative usage, cancellation, limits, and credential redaction.
- `autoharness-store` and `autoharness-store-sqlite` provide an event-authoritative store, transactional projections, WAL-mode local durability, idempotent append, migration verification, projection rebuilding, and an integrity-checked provider-neutral model-catalog cache.
- `autoharness-settings` resolves defaults, user file, workspace file, environment, and override layers in fixed precedence with per-key provenance; schema 2 adds optional profile default models and non-secret recovery records while schema-v1 documents migrate on mutation (ADR-0012).
- The atomic application-owned `autoharness.profiles.json` document stores validated named profiles, non-secret connection fields, optional default models, opaque credential references, and bounded credential-recovery records.
- `autoharness-app::vault` defines the credential-vault port with an operating-system implementation through the `keyring` crate and a fake implementation for tests; Windows Credential Manager save, load, replace, and delete passed the opt-in platform smoke.
- Startup and runtime switching resolve one provider-matched credential source in precedence order: environment, then the active profile's vault entry, then session-only; locked or unavailable vaults preserve offline use without plaintext fallback.
- Active Gemini and router profiles rebuild their exact provider adapters and connection fields at runtime; a compatible selected model can be saved as the profile default and reapplied after catalog refresh.
- The terminal consumes live safe settings and profile projections through one typed shell with Chat, Sessions, Profiles, Settings, and Help routes.
- `Route` is the single primary-page authority and `OverlayKind` is the mutually exclusive modal authority for model selection, credential entry, command and transcript search, permission, and confirmation.
- Wide terminals show a persistent rail with local profile and runtime status; narrower terminals show compact route tabs and one prioritized status line.
- Chat has explicit conversation roles, composer boundaries, recovery actions, and offline, loading, connection-error, no-model, and new-conversation states.
- Sessions, Profiles, Settings, and Help are primary routes with contextual action bars; settings shows safe runtime provenance and recovery without becoming the Phase 3.8 editor.
- Route changes preserve drafts and stable selections while clearing hidden confirmations and secret editors, and permission prompts preempt lower-authority overlays before taking focus.
- Phase 3.7 layouts are reviewed at 120x50, 120x40, 80x24, 60x18, and 40x12 across all routes and an exact destructive confirmation.
- Phase 3.4 usability surfaces are implemented: a `Ctrl+/` searchable command palette and generalized slash commands over one shared typed command table, an `F1` contextual help overlay whose section order follows the surface help was opened from, an enriched header status surface (profile, credential source, selected model, attempt state, token usage) that degrades at narrow widths, `Ctrl+Up`/`Ctrl+Down` composer history with draft stashing, `Ctrl+F` transcript search with match counting and jump-to-match wrapped-row scrolling, `Ctrl+Y` OSC 52 transcript copy plus `/export` Markdown export written beside the database from durable events, structured collapsible tool rows with `Ctrl+X` expand toggle, and confirm-gated archiving with one-shot `Ctrl+Z` undo in the session browser.
- Phase 3.5 real-PTY scenarios cover credential-free first run and restoration, returning-profile offline replay, settings provenance, resize and restart, multi-session switching and destructive confirmations, invalid-call repair, permission deny and allow with replay, and forced-shutdown recovery.
- Eight real-PTY scenarios now include the routed shell journey alongside first run, returning-profile offline replay, settings provenance, resize and restart, multi-session switching and confirmations, profile management, invalid-call repair, permission outcomes, and forced-shutdown recovery.
- The instrumented release binary and three-sample real-PTY loopback runner produce valid correlated first-draw, input-to-dispatch, and decoded-chunk-to-render intervals outside network time; the result is local smoke evidence, not authoritative reference-machine evidence.
- Environment credentials are paired only with the effective provider, active-profile identity remains visible when environment credentials override vault storage, and locked or unavailable vault access degrades to session-only mode.
- Corrupt catalog caches are discarded before live replacement, the configured router now has both plain-chat and function-calling live probes, and the terminal release checklist covers security, accessibility, restoration, documentation, benchmark provenance, and database rollback.
- The cross-platform PTY gate exposed and now covers Windows cursor-position report handling, inactive-session mutation isolation, reverse-causation event deletion, visible durable tool rows, and notice clearing when destructive confirmation is cancelled.
- Tool definitions are advertised only for positively identified model support, and the current single safe-agent interaction mode enables the exact built-in registry.
- Gemini Interactions function-call arguments are buffered across streamed deltas and emitted only after a complete bounded JSON object is available.
- Unknown names and invalid argument shapes become durable `InvalidToolCall` no-authority proposals, are force-denied even under permissive policy, and return a deterministic result for bounded model repair.
- Provider request history includes completed turns and the current input while excluding unrelated prior failed or cancelled prompts.
- `Ctrl+N` creates and activates a fresh durable session even when credentials or the catalog are unavailable, and `Ctrl+L` opens a searchable session browser over every durable session.
- The session lifecycle is event-sourced under [ADR-0011](../adr/0011-use-event-sourced-session-lifecycle.md): rename, archive, and unarchive are schema-v1 commands and events, archived sessions accept only unarchive, switching replays the target history before any projection swap and is refused while an attempt or permission prompt is active.
- Deleting a session exports the complete authoritative event stream to a documented provider-neutral JSON archive beside the database first ([format](../architecture/SESSION_EXPORT.md)); export failure aborts deletion, and version-mismatched deletes fail closed.
- Session titles use the validated `SessionTitle` value type (non-empty, bounded, no control characters) and store titles live in SQLite migration 3.
- Non-secret runtime configuration remains environment-driven for values not yet covered by profiles, and the in-app credential overlay remains available under [ADR-0005](../adr/0005-use-ephemeral-in-app-credentials.md) for session-only entry.
- The user-observed 2026-08-22 Gemini wire shape is represented by a recorded structural SSE fixture that survives one-byte fragmentation without emitting empty arguments.
- On 2026-08-22 both opt-in Gemini live probes passed against production Google AI Studio using current-generation models: plain chat with the complete registry streamed text to completion, and streamed function calling produced one complete bounded `http_request` call before a tool-calls completion.
- Stable failure codes, compact safe attempt references, retry actions, and the global fresh-session action are rendered in failed transcript rows and fixed-size golden buffers.
- Formatting, strict Clippy, 325 workspace tests across 46 suites, fixed-size goldens, documentation links, focused navigation tests, and the complete serial Windows PTY matrix passed locally before Phase 3.7 promotion.
- A real Windows terminal smoke selected a loopback router model, submitted a prompt, rendered the completed response, emitted one correctly correlated marker chain, exited with code 0, and restored the terminal.
- Continuous integration defines formatting, Clippy, documentation, doctest, native Linux, Windows, and macOS gates, a serial cross-platform PTY scenario gate, and separate formatting, Clippy, and test gates for the isolated benchmark workspace.

## Recently completed

- On 2026-08-25, Providers became a provider-choice list for Gemini, Google AI Studio API, Cursor, Codex, Claude Code, and compatible APIs; Codex opens the official CLI subscription-authentication page and is the only non-API account bridge implemented, while Cursor and Claude Code are explicitly unavailable until equivalent adapters exist; 412 workspace tests, strict Clippy, formatting, documentation links, and the real Windows Codex authentication-page PTY journey passed.
- Implemented Phase 3.8 locally: schema-3 typed non-secret preferences, fixed layer precedence, safe workspace restrictions, atomic local profile persistence, Settings editing and reset controls, projection-driven themes and accessibility modes, generated shortcut reference, responsive security-overlay matrix, and an ASCII persistence PTY journey.
- On 2026-08-25, `feat/tui-professional-overhaul` corrected compact-shell mouse geometry from the rendered content rectangle, added exact profile click coverage, added a bounded reduced-motion-safe startup boot surface, exposed `/profile` beside `/settings`, refreshed the dark preset to a dark neon cockpit palette, and passed 66 focused TUI tests, 350 workspace tests, strict Clippy, formatting, and three ignored Windows PTY journeys.
- On 2026-08-25, the follow-up TUI cutover made the initial boot surface visible on the first draw, stopped automatic credential dialogs, routed `/settings`, `/models`, `/sessions`, `/connect-api-key`, `/retry`, `/cancel`, `/search`, and `/toggle-tools` through the shared command table, removed the prompt footer, tightened prompt/transcript borders, applied the neon palette to the default System theme, and passed 385 workspace tests plus first-run and routed-shell Windows PTY journeys.
- On 2026-08-25, slash input now opens a live filtered command browser, `/provider` opens provider setup, `/models` supports `D` to save the selected model as the active provider default, and Aurora and Ember theme presets were added with distinct color anchors; 389 workspace tests pass with strict Clippy and formatting.
- On 2026-08-25, the command palette became an eight-row inline Chat overlay with live filtering, and the prompt became a borderless metadata bar showing safe thinking mode, workspace basename, Git branch when available, model, and attempt state; 390 workspace tests and the routed-shell PTY journey pass.
- On 2026-08-25, the inline command browser moved to the bottom of Chat immediately above the prompt bar, while the active command query is rendered inside the prompt editor line; the routed-shell PTY journey and 390 workspace tests remain green.
- On 2026-08-25, the prompt bar was refined into a single Claude-like bottom composer surface, every visible Settings option became keyboard-selectable with safe no-op rows for policy/runtime values, and Backspace on an empty slash query now closes command mode; 391 workspace tests pass.
- On 2026-08-25, `feat/tui-redesign` removed the opaque Chat background, applied rounded borders through the shared panel helper, reduced the wide rail to previous sessions and projects with a bottom Settings action, added Settings, Providers, Profile, and Agents navigation placeholders, and passed 128 focused TUI tests, strict TUI Clippy, formatting, ignored visual review, and a clean Windows TUI launch smoke.
- On 2026-08-25, `feat/tui-audit` made the route bar persistent above both wide and compact shells, removed duplicate compact Chat headers, added cyclic arrow-key Settings page navigation with body focus transitions, ellipsized named sidebar sessions to one line, removed session IDs from untitled fallback labels, fixed compact tab widths at 48 columns, and passed 132 focused TUI tests plus a built-binary Windows launch smoke.
- On 2026-08-25, `feat/tui-chat-polish` removed visible route and provider status chrome, moved Profile and Settings to the bottom action bar, removed the sidebar session heading, replaced chat mode metadata with model capability-aware thinking and slash-style workspace paths, and added a transparent rounded composer with a `❯` prompt line.
- On 2026-08-25, `feat/tui-visual-refinement` moved Profile and Settings into the wide sidebar footer, kept the compact fallback footer only when no sidebar exists, routed provider access through Settings tabs, reset border backgrounds, dimmed inline slash commands, and removed the adjacent transcript/composer shared border.
- On 2026-08-25, `feat/tui-command-settings` made slash rows unique and identifier-first, preserved a visible cursor during command filtering, routed `/profile`, `/provider`, `/agents`, `/settings`, and the legacy `/profiles` alias into Settings tabs, and rendered provider/profile content inside the Settings page.
- Merged Phase 3.7 into `dev` through [pull request #17](https://github.com/andersj05/AutoHarness/pull/17) after green formatting, Clippy, documentation, benchmark, workspace, and serial PTY CI jobs on Windows, macOS, and Linux.
- Implemented Phase 3.7 end to end: typed routes, one modal owner, wide navigation rail, compact route tabs, unified status, routed Sessions, Profiles, Settings, and Help, redesigned Chat hierarchy and recovery states, exact confirmation dialogs, focus restoration, permission preemption, route visual matrix, and real PTY coverage.
- Fixed fresh-session list publication so a new durable session appears in Sessions immediately after commit.
- Implemented Phase 3.6 end to end: ADR-0013 recovery semantics, settings schema 2, `ProfileManager`, distinct per-profile vault entries, runtime adapter switching, safe connection tests, profile defaults, the responsive `Ctrl+G` center, composed lifecycle coverage, PTY coverage, and Windows vault smoke evidence.
- Implemented the Phase 3.5 PTY scenario harness and six release scenarios covering all planned terminal paths without live credentials.
- Expanded the roadmap with Phase 3.6 profile and provider management, Phase 3.7 unified TUI shell and navigation, Phase 3.8 personalization and accessibility, and Phase 3.9 integrated terminal product validation.
- Added monotonic benchmark markers, the real-PTY terminal latency runner, report validation, reference-machine fields, and content-free loopback correlation.
- Added the configured-router plain-chat live probe, locked-vault degradation, corrupt-cache live replacement coverage, and provider-matched environment credential resolution.
- Added and routed the terminal release checklist, updated benchmark and root documentation, and kept the baseline and isolated benchmark gates green.
- Implemented Phase 3.4 across eight test-first slices: command palette with generalized slash commands, contextual help overlay and footer affordances, enriched header status surface, composer history recall, transcript search, OSC 52 copy plus Markdown export intent satisfied from durable events in the coordinator and engine actor, structured collapsible tool rows, and archive confirmation with `Ctrl+Z` undo.
- Verified Phase 3.4 surfaces visually at 120x50, 120x40, 80x24, 60x18, and 40x12 through a checked-in ignored review harness, and updated the fixed-size goldens for the new header and footer.
- Implemented Phase 3.3: layered typed settings resolution with provenance, validated provider profiles with atomic durable storage, the operating-system credential-vault port, startup credential resolution with safe degradation, TUI settings provenance display, and secret sentinel coverage.
- Accepted ADR-0009 (operating-system-backed credential profiles) and recorded ADR-0012 (typed settings resolver with layered precedence).
- Added the `autoharness-settings` crate with fixed-order layer merging, workspace allowlist enforcement, schema-version handling, and unknown-active-profile validation.
- Added the app library target exposing vault, profiles, and credential-resolution modules to integration tests.
- Wired launch resolution into `main.rs`: profile document plus live environment feed the resolver, the active profile selects the adapter and connection fields, and resolved provenance publishes to the terminal before the first draw.
- Verified a returning profile reconnects from the fake vault across store reopen without prompting and without writing secret bytes anywhere on disk.
- Implemented Phase 3.2: schema-v3 session titles, enriched summaries, event-sourced rename/archive/unarchive with aggregate guards, atomic version-checked deletion, pre-deletion export, and a searchable `Ctrl+L` browser with slash commands and per-session composer drafts.
- Verified the composed two-session create, rename, archive, unarchive, switch, restart, and replay-equivalence path against real SQLite.
- Recorded the lifecycle contract in [ADR-0011](../adr/0011-use-event-sourced-session-lifecycle.md) and the export format in [SESSION_EXPORT](../architecture/SESSION_EXPORT.md).
- Buffered Gemini Interactions function calls until every streamed `partial_arguments` fragment forms the complete bounded JSON object.
- Added capability-aware tool advertisement with backward-compatible catalog decoding and positive support required before exposing functions.
- Added durable force-denied invalid-call proposals, deterministic provider repair results, no-authority authorization checks, and content-free rejection telemetry.
- Excluded prior failed and cancelled prompts from unrelated future provider requests while preserving explicit retry and completed history behavior.
- Added a global typed `Ctrl+N` intent, durable session creation, session-identity-aware TUI projection replacement, fixed-size footer affordances, and a successful credential-overlay PTY smoke run.
- Added stable UI failure codes, compact safe attempt references, and concrete retry or fresh-session recovery actions.
- Added opt-in ignored Gemini plain-chat, Gemini HTTP-function, and configured-router HTTP-function compatibility probes that retain only structural assertions.
- Completed the Phase 3 safe agent execution path without adding provider-native payloads or concrete capabilities to the engine.
- Recorded the durable capability boundary in [ADR-0007](../adr/0007-use-durable-capability-tool-runtime.md).
- On this branch, focused TUI tests (108 passed), the ignored visual review (2 passed), Windows PTY onboarding, routed shell, profile center, offline resume, session lifecycle, forced shutdown, and permission/tool journeys passed.
- Workspace formatting, strict Clippy, full workspace tests (336 passed across 46 suites), and documentation link checks passed.
- Added a local user-profile dialog with typed display-label persistence, command-palette and `Alt+U` access, visible Save/Cancel controls, and active provider/model summary.
- Added semantic mouse actions, responsive hit testing for route tabs, chat/session/profile controls, confirmations, picker rows, palette rows, credential dialogs, permission controls, and the user-profile dialog.
- Terminal lifecycle now enables mouse capture and restores it precisely on normal exit and partial setup failure; selected Windows PTY journeys pass, while panic-hook-specific and cross-platform restoration evidence remains open.
- Final local verification now reports 118 focused TUI tests, 347 workspace tests across 46 suites, strict Clippy, formatting, onboarding PTY, routed-shell PTY, and profile-center PTY success.

## Immediate next actions

1. Run warning-denied rustdoc/doctests and isolated benchmark formatting, Clippy, and test gates on this release-candidate branch.
2. Run the same PTY matrix on macOS and Linux and refresh Windows evidence for the release-candidate commit.
3. Execute migration and rollback rehearsal against the last Phase 3.5 database and settings formats.
4. Preserve configured-router live, macOS Keychain and Linux Secret Service vault smoke, approved reference-machine, and final release-checklist evidence for Phase 3.9.
5. Record and triage any cross-platform terminal rendering differences before release-candidate promotion.

## Open questions

- What reference machine should define the final Phase 3.9 startup and stream-overhead budgets?

## Blockers

The local TUI implementation slice is complete, but the Phase 3.9 release evidence has not been executed on this branch.
Configured router access, macOS and Linux platform vault environments, an approved reference machine, and operator-owned rollback inputs remain prerequisites.

## Handoff note

The branch adds release-candidate TUI hardening and focused tests; do not promote it until the full cross-platform evidence matrix and release checklist are recorded.
