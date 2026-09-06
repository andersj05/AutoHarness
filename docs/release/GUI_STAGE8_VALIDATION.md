# GUI Stage 8 validation

**Reviewed:** 2026-09-06

**Status:** Candidate packaging and release gates implemented; Stage 8 release, default cutover, and retirement remain incomplete.

## Delivered scope

The [candidate workflow](../../.github/workflows/gui-packages.yml) builds unsigned release-profile NSIS, DMG, and Debian artifacts on Windows, macOS, and Linux.
The [packaging script](../../scripts/package_gui.py) requires a clean immutable commit, records toolchain and executable hashes, hashes copied artifacts, and rejects a checkout changed during packaging.
Signed mode requires provisioned platform identities and verifies Authenticode with timestamp, Apple notarization, or Linux detached GPG signatures before marking evidence signed.
No signing identities are currently provisioned, so the signed paths are implemented but not validated release evidence.
The [update policy](GUI_UPDATE_POLICY.md) defines deliberate installer updates, cold backups, and rollback without adding an automatic updater.

The [release validator](../../scripts/check_gui_release.py) rejects incomplete gates, mixed commits, missing or altered evidence, unsigned or debug packages, open blockers, absent approvals, and early TUI retirement.
It validates evidence completeness and byte identity; it cannot independently prove human attestations.
The [checklist](GUI_RELEASE_CHECKLIST.md) requires release, rollback, default-interface, and later separate retirement approvals.

The opt-in installer opens the native GUI without arguments while retaining `ah` and explicit terminal launch for compatibility.
Ordinary source builds remain terminal-default and the preview bundle stays inactive.
Native Quit AutoHarness uses the existing shutdown authority, and the returning Tauri event loop permits durable runtime joins before process exit.
Session actions remain reachable in compact windows, and a pending export cannot race with destructive confirmation state.

## Automated and native evidence

Frontend type checking, the 116-test GUI suite, production frontend build, and the 12-test Python tooling suite pass locally.
Workspace formatting, strict all-target and all-feature Clippy, and the complete locked all-target and all-feature Rust workspace suite pass locally.
The deliberate Windows first-run PTY journey also passes with all features, using the packaged-mode console companion to verify terminal rendering, Settings navigation, and clean restoration on exit.
The tooling tests cover tampered evidence, candidate mismatch, unsigned packages, signing prerequisites, retirement boundaries, transient driver readiness, and database replay digest behavior.

The Windows installed debug candidate at `bedef639f3bfde2be419d46150605114c0195f6d` passed installation, same-version reinstallation, native launch, rename, archive, restore, export, exact-title deletion, two restart boundaries, clean shutdown, and uninstallation.
Its native lifecycle report identifies executable SHA-256 `9eb75149ca9c47305341c02982e2fbb471b24d97a9b64b7f0c99f8faadb77823`.
Local reports and screenshots are under `target/gui-evidence/windows-final`; these ignored artifacts are workstation evidence, not durable release approval.
The native screenshot journey captures onboarding, Sessions, Providers, Memory, and Settings at exact 900 by 640, 1280 by 800, and 1600 by 1000 CSS viewports.
Assistant review of the Windows native captures found the compact session-action defect and verified the corrected reachable details and dialog.
Screenshots deliberately retain pending human review status.
The optimized Windows candidate at `2d7612d5e15ada17c951978d61949a8741d2ade3` passes the same installed-app journey, with reports under `target/gui-evidence/windows-release`.
Its installer SHA-256 is `574ee5c757a2769e4ef40ca392cc3b1f764dd1b5d38c60d99845a89d19682442` and installed executable SHA-256 is `1da5f5d19579c2cf580fe3d146bd278ee01c7bbb0e2fb86f71313dde5a8d7619`.
The build-output executable can differ from the installed executable because Tauri patches bundle metadata, so installed evidence identifies its own executable bytes.
The Windows installed process smoke also verifies two real OS-window close requests, complete runtime shutdown, and equivalent idle replay without WebDriver or test IPC.
The Windows installer produced by the cross-platform CI run passes the full installed journey when downloaded and exercised locally; hosted WebDriver startup evidence is tracked separately in the pull request.

The [macOS candidate run](https://github.com/andersj05/AutoHarness/actions/runs/34053878881/job/101542138991) mounted the actual DMG, installed its application, validated the property list, acknowledged two native renderer baselines, completed two graceful shutdowns, and preserved the idle durable database digest.
This establishes native startup and idle replay only, not the complete macOS interaction matrix.
The [pull request](https://github.com/andersj05/AutoHarness/pull/24) records subsequent candidate workflow and baseline results.
The [Linux installed journey](https://github.com/andersj05/AutoHarness/actions/runs/34055422907/job/101546349030) passes native session mutations, restart, graceful shutdown, and uninstall through WebKitGTK's native WebDriver endpoint.
The proxy transport had dropped responses; direct WebDriver removes that test-only failure boundary.
Review of its captures exposed stale compositor pixels under Xvfb, so subsequent virtual-desktop captures explicitly use software rendering without compositing, wait for paint, and assert visible dialog bounds.
This CI configuration is isolated to the test launcher and is recorded in the lifecycle report; hardware-composited X11 and Wayland review remains a release prerequisite.

## Remaining release blockers

- Provision authorized Windows code-signing, Apple Developer ID and notarization, and Linux signing identities; execute and verify all signed paths.
- Finish Stage 1 renderer independence and Help parity.
- Complete all-platform native provider, GUI vault, permission, memory, crash recovery, migration, and cold rollback journeys on one committed candidate.
- Complete system-webview visual review, keyboard and screen-reader review, security review, and approved reference-machine performance evidence.
- Preserve signed artifacts and immutable evidence beyond the CI retention window, obtain the required approvals, and close the agreed rollback window before separately approving retirement.

No public release, default-interface promotion, Ratatui removal, PTY infrastructure removal, or retirement approval is claimed by this work.
