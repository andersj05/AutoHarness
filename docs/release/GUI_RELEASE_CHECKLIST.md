# GUI release checklist

**Status:** Required before default-interface promotion; no release approval recorded.

**Last updated:** 2026-09-06

Use this checklist for Stage 8 of the [GUI implementation plan](../design/GUI_IMPLEMENTATION_PLAN.md).
Every required gate must pass for one committed candidate and its exact signed package hashes.
Browser fixtures, native protocol tests, unsigned installers, debug builds, and historical terminal evidence cannot substitute for packaged release evidence.

## Candidate and artifacts

- [ ] Record the full candidate commit, version, target architecture, Rust toolchain, Node version, pnpm version, and CI run.
- [ ] Build from a clean candidate based on `dev`; keep the promotion PR from `dev` into `main` separate from feature work.
- [ ] Build and verify Windows Authenticode with SHA-256 and timestamp, macOS Developer ID with notarization and stapling, and Linux detached package signatures.
- [ ] Preserve every installer, package manifest, signature, screenshot, and review record outside expiring CI storage.
- [ ] Verify artifact hashes again after download and before installation.
- [ ] Confirm the installer launches the GUI without command-line arguments and retains the `ah` terminal rollback executable.
- [ ] Confirm release builds use local production assets, no development server, disabled devtools, strict capabilities, and no credential-bearing source maps or diagnostics.

The [packaging script](../../scripts/package_gui.py) requires signing configuration by default and verifies platform signatures before recording a successful manifest.
Unsigned and debug modes explicitly cannot satisfy [the release validator](../../scripts/check_gui_release.py).
The [candidate workflow](../../.github/workflows/gui-packages.yml) creates unsigned CI artifacts only and has no publishing or signing authority.

## Prerequisites and baseline

- [ ] Stage 1 exits: orchestration has no renderer-owned type imports and both clients consume the renderer-neutral contract.
- [ ] Complete Help parity is available inside the GUI, including keyboard shortcuts and recovery guidance.
- [ ] `cargo fmt --all -- --check` passes.
- [ ] `cargo clippy --workspace --all-targets --all-features --locked --no-deps -- -D warnings` passes.
- [ ] `cargo test --workspace --all-targets --all-features --locked --no-fail-fast` passes.
- [ ] The ordinary non-package launch mode and packaged launch mode both pass their focused tests.
- [ ] Warning-denied rustdoc, doctests, generated themes, documentation links, and storage benchmark validation pass.
- [ ] `pnpm gui:typecheck`, `pnpm gui:test`, and `pnpm gui:build` pass after `pnpm install --frozen-lockfile`.
- [ ] `python -m unittest discover -s scripts -p "test_*.py"` passes.

## Packaged journeys on every platform

Run each row on Windows WebView2, macOS WKWebView, and Linux WebKitGTK with the same candidate.
Record exact OS, architecture, webview version, package hash, outcome, and evidence reference.
The Windows and Linux [native driver journey](../../scripts/gui_webdriver.py) automates the offline session subset against an installed binary without test IPC or fixture transport.
Its native screenshots still require visual review.
The macOS process smoke establishes native baseline acknowledgement, graceful shutdown, and idle replay; the full interactive WKWebView journeys remain a required review on a macOS host.

| Gate | Required journey |
| --- | --- |
| Installer lifecycle | Fresh install, launch from OS shortcut, same-version repair, upgrade from previous supported release, uninstall, reinstall, and user-data retention |
| Session lifecycle | Offline first run, create, switch, rename, archive, restore, export, exact confirmed deletion, clean close, and replay after restart |
| Providers | Gemini and router plain chat and tool continuation, Codex browser sign-in and cancellation, saved model/reasoning defaults, stream cancellation and retry |
| GUI vault | GUI save, rotate, reload after restart, disconnect, delete, environment precedence, locked/missing vault, and interrupted credential mutation recovery |
| Permissions | Exact allow and deny, dialog preemption, no effect after denial, conservative recovery across permission-pending and effect-started interruption |
| Memory | Search, paging, import, untrusted proposal review, distinct approval revision, stale review rejection, correction, retraction, export, deletion, and restart equivalence |
| Recovery | Native close while idle, streaming, and running a tool; forced process termination at prepared, dispatched, permission-pending, and effect-started boundaries; writer-lock exclusion and renderer-gap recovery |
| Accessibility | Keyboard-only primary routes and security dialogs, focus restoration, announcements, screen-reader order, 200 percent zoom, reduced motion, no-color, and high contrast |
| Native visual review | Pixel review at exact 900x640, 1280x800, and 1600x1000 CSS viewports, plus minimum-window resilience, onboarding, chat, sessions, providers, settings, memory, and security dialogs |

Do not retain credentials, provider payloads, personal workspaces, or raw driver logs in evidence.
Use synthetic sessions for screenshot and lifecycle automation.
Live-provider evidence contains only public provider/model identifiers, candidate identity, date, and outcome.

## Migration, rollback, security, and performance

- [ ] Stop all clients before making a cold backup of the complete application data directory, including SQLite sidecars, settings, profiles, and artifacts.
- [ ] Replay a copy of the previous release's data with the candidate and verify transcript, session, memory, profile, and settings equivalence.
- [ ] Future schema versions, corruption, and invalid migration history fail closed without mutation.
- [ ] Rehearse rollback using the previous signed package and the untouched cold backup; never point an older binary at a migrated live database.
- [ ] Verify user-owned exports and operating-system vault entries are preserved on uninstall and rollback; use isolated test profiles for credential rehearsal.
- [ ] Record startup, stream-to-paint latency, cancellation, visible-row update cost, memory use, and long-session responsiveness on an approved reference machine.
- [ ] Verify long transcripts remain bounded by visible rows plus changed data and meet the approved reference baseline.
- [ ] Complete a security review of installer privileges, signing, carrier capabilities, CSP, secret ingress, inactive credentials, recovery, and inert rich content.
- [ ] Run secret sentinels through GUI entry, vault mutations, native frames, frontend output, logs, diagnostics, browser storage, and durable files.
- [ ] No P0 or P1 defect remains in onboarding, chat, sessions, profiles, credentials, settings, permissions, memory, recovery, accessibility, or rendering.

## Evidence and approval

Generate an empty record with `python scripts/check_gui_release.py --template` and store the completed record beside its evidence files.
Each required gate names the exact candidate commit, a passed status, and a nonempty evidence file with SHA-256.
Each platform references the packaging script's manifest and its exact artifact files.
All paths are confined to the evidence directory.
The validator checks completeness and byte identity, not the truth of a human attestation.
Only the release maintainers can approve review evidence.

- [ ] Record release approval, rollback approval, and default-interface approval separately.
- [ ] Record the previous signed candidate, rollback rehearsal, and an approved rollback-window closing date.
- [ ] Run `python scripts/check_gui_release.py <record.json> --commit <full-candidate-commit>` successfully.
- [ ] Promote the reviewed GUI default only through the approved `dev` to `main` release PR.
- [ ] Apply the [update and rollback policy](GUI_UPDATE_POLICY.md) to the exact approved artifacts.

## TUI retirement

Default cutover and terminal retirement are separate actions.
The packaged `ah` executable, source TUI, terminal renderer adapter, and deliberate PTY checks remain available during the rollback window.

- [ ] The approved rollback window has closed without unresolved GUI release regressions.
- [ ] Record a distinct TUI-retirement approval for the candidate.
- [ ] Run the release validator with `--retire-tui` successfully.
- [ ] Remove Ratatui, Crossterm, TUI adapters, terminal binary, and PTY-specific infrastructure in a separately reviewed change after renderer independence is proven.
- [ ] Re-run the remaining migrated baseline and packaged-app matrix before promoting the retirement change.
