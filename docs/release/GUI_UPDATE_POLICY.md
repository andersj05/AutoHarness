# GUI update and rollback policy

**Status:** Candidate distribution policy; public release remains approval-gated; GUI-only source cutover is authorized.

**Last updated:** 2026-09-07

## Distribution

Stage 8 candidate formats are per-user NSIS installers for Windows x86_64, DMG packages for macOS arm64, and Debian packages for Linux x86_64.
The current Linux build baseline is Ubuntu 24.04; do not claim older-distribution support without compatibility evidence.
The macOS configured minimum version is not evidence that every supported version has been tested.
Windows installers bootstrap the system WebView2 runtime when needed and therefore require network access on machines without it.
Offline-runtime installation is not currently supported by this package configuration.

Only approved signed artifacts may become a public release.
The [candidate workflow](../../.github/workflows/gui-packages.yml) uses read-only repository permissions and uploads unsigned test artifacts without publishing them.
Workflow artifacts expire after 14 days; approved evidence and rollback packages must be preserved separately before expiration.

## Signing setup

The [packaging script](../../scripts/package_gui.py) has a verified unsigned development path using `python scripts/package_gui.py --unsigned --debug` on Windows.
Omit `--debug` for release-profile candidates.
Omit `--unsigned` to require the platform signature and verification steps.
Run from a clean committed checkout with installed frontend dependencies and native system prerequisites.
Signed builds require the following pre-provisioned signing environment; private material never belongs in the repository or chat.

| Platform | Required setup |
| --- | --- |
| Windows | A valid code-signing certificate with its private key available to the signing account, `AUTOHARNESS_WINDOWS_CERTIFICATE_THUMBPRINT`, and `AUTOHARNESS_WINDOWS_TIMESTAMP_URL` pointing to the certificate provider's HTTPS RFC 3161 service |
| macOS | An available Developer ID Application identity, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD`, and `APPLE_TEAM_ID`; Tauri signs, notarizes, and staples before validation |
| Linux | GPG with the release private key available and `AUTOHARNESS_LINUX_SIGNING_KEY`; the script emits and verifies a detached armored signature |

Signing identities must be provisioned and authorized by the project maintainer.
Use a protected signing machine or CI environment with no untrusted pull-request code and short-lived secret access.
Code-signing and notarization accounts are external prerequisites, not capabilities an agent can manufacture.
The signed path has not passed until real signatures for all three platforms are recorded against a candidate.

## Update behavior

Updates are deliberate installer replacements initiated by the user after release approval.
The application does not poll an update endpoint, download background executable code, or silently replace itself.
No updater plugin, renderer networking capability, update public key, or update endpoint is installed.
`createUpdaterArtifacts` remains false.
An automatic updater requires its own architecture and security review covering key rotation, trust, interruption, downgrade prevention, and rollback before introduction.

Before an update, finish or cancel active work and close every AutoHarness process.
Preserve a cold backup of the complete application data directory and the prior signed installer.
Install the approved replacement, verify its identity, and perform the restart and replay checks in the [release checklist](GUI_RELEASE_CHECKLIST.md).
An installer must not erase user data, exports, or vault credentials during upgrade or uninstall.

## Rollback

Keep the previous approved package and its compatible cold data backup for the maintainer-approved rollback window.
Both `autoharness` and `ah` now open the desktop under [ADR-0020](../adr/0020-retire-terminal-client.md).
Record the window's closing date in distribution evidence.
If the candidate fails, stop all clients, preserve its data directory for diagnosis, restore the untouched cold backup to a separate location, and launch the previous signed version against that backup.
Do not attempt a database downgrade or overwrite the only copy of migrated data.
Vault state is outside the data-directory backup, so credential mutation rollback must follow the existing [credential recovery contract](../architecture/SETTINGS.md).
Signed distribution still requires release, rollback, and default-interface approval for the exact package candidate.
The [release validator](../../scripts/check_gui_release.py) checks distribution evidence without performing publication.
Source retirement is separately authorized by ADR-0020 and does not satisfy missing distribution evidence.

## Platform references

The packaging configuration follows Tauri's [Windows installer](https://v2.tauri.app/distribute/windows-installer/), [Windows signing](https://v2.tauri.app/distribute/sign/windows/), and [macOS signing](https://v2.tauri.app/distribute/sign/macos/) guidance.
Native Windows and Linux testing uses the documented [external WebDriver setup](https://v2.tauri.app/develop/tests/webdriver/manual-setup/).
External `tauri-driver` does not support WKWebView on macOS.
The macOS process smoke verifies native baseline acknowledgement, graceful shutdown, and idle database replay from the mounted DMG's installed application; it does not replace native interaction, screenshot, or accessibility review.
