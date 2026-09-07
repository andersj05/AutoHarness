# GUI validation workflow

**Updated:** 2026-09-07

## Automatic pull-request checks

The [CI workflow](../../.github/workflows/ci.yml) tests pull requests into `dev` and `main`.
It does not repeat the same matrix on the subsequent branch push.
Direct pushes to either integration branch must therefore be avoided; use the documented feature and promotion pull requests.
Manual CI dispatch runs every baseline job.

Every pull request checks documentation links and the Python release-tooling and CI-routing tests.
The [scope selector](../../scripts/ci_scope.py) compares the tested merge tree with its exact base, including removed files and both sides of renames.
Documentation and tooling changes do not compile the desktop.
Frontend changes run type checking, the complete GUI unit and integration suite, and a production build on Linux once.
Rust changes run formatting, generated-theme validation, strict Clippy, rustdoc, doctests, storage-benchmark validation, and the renderer-neutral and desktop-host tests on Linux, Windows, and macOS.
Shared client and presentation changes also run the frontend checks.
Unknown infrastructure and workflow changes conservatively select all checks.
The terminal renderer and PTY journeys are retired under [ADR-0020](../adr/0020-retire-terminal-client.md).

## Local baseline

From the repository root, run:

```sh
python scripts/validate_gui.py
```

The [runner](../../scripts/validate_gui.py) installs locked frontend dependencies, validates types, tests and builds the GUI, runs Python tooling and documentation links, then runs complete all-feature Rust formatting, Clippy, workspace tests, warning-denied documentation, doctests, generated themes, and isolated storage-benchmark checks.
It stops at the first failed check and writes its log and a structured report under `target/gui-evidence/local-baseline`.
The report records the starting commit and whether the checkout is dirty.
Logs remain local and are not uploaded by default.
This baseline covers the complete current workspace but does not opt into live-provider or vault journeys.
It is not signed release evidence or a replacement for native accessibility review.

## Deliberate native candidates

The [candidate workflow](../../.github/workflows/gui-packages.yml) runs only through manual dispatch.
Select one platform when diagnosing that platform, or `all` when deliberately collecting the full candidate matrix.
The default is Linux only.
No schedule, pull-request trigger, or branch-push trigger builds installers.
Existing concurrency cancellation still stops superseded runs for the same ref and platform selection.
Artifact retention is 14 days; archive release evidence separately before it expires.

The same packaging and lifecycle scripts remain available locally.
The [Stage 8 record](GUI_STAGE8_VALIDATION.md) distinguishes installed native evidence from browser fixtures and records known platform limitations.
Moving expensive checks out of automatic CI does not waive any requirement in the [GUI release checklist](GUI_RELEASE_CHECKLIST.md).
GUI-only source integration into `main` and signed distribution remain distinct events.
