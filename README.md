# AutoHarness

AutoHarness is an open-source agent runtime with a native Tauri and React desktop interface.
Rust owns providers, durable sessions, tool execution, permissions, credentials, memory, and recovery.
The long-term system improves prompts, routing, policies, tools, memory, and code through measured experiments and gated promotion.

The desktop is now the only product interface under [ADR-0020](docs/adr/0020-retire-terminal-client.md).
The legacy terminal renderer and PTY tooling have been removed.
Signed public distribution still requires the [GUI release checklist](docs/release/GUI_RELEASE_CHECKLIST.md); source integration does not establish completion of those gates.

## Run the desktop

Install the pinned Rust toolchain from `rust-toolchain.toml`, Node version from `.node-version`, and pnpm version from `package.json`.
Native Tauri development also requires the platform webview and build prerequisites.
Run from the repository root:

```text
pnpm install
pnpm gui:desktop
```

For a source launch using embedded production assets:

```text
pnpm gui:build
cargo run -p autoharness-app --locked
```

Both `autoharness` and `ah` open the desktop without arguments.
The optional `--gui` argument remains accepted for existing desktop launchers; `--tui` is rejected.
The library remains usable without desktop features through Cargo's `--no-default-features` option.

For browser-only presentation development:

```text
pnpm gui:dev
```

The browser uses deterministic fixtures and does not connect to the Rust host or persist real sessions and credentials.
Use the native application to validate persistence, credential safety, and recovery.
Unsigned candidate installer tooling and signing prerequisites are documented in the [update policy](docs/release/GUI_UPDATE_POLICY.md).

## Use AutoHarness

Open Providers to create and activate a Gemini or OpenAI-compatible router connection, or sign in with a Codex subscription.
Choose a compatible model in Chat, write a prompt, and select Send.
Stop requests cancellation; retry remains subject to the authoritative attempt state.

Session-only credentials remain temporary.
Saved credentials use Windows Credential Manager, macOS Keychain, or Linux Secret Service.
Environment credentials override saved credentials, and the interface displays the effective source.
Never paste credentials into a conversation.
The [settings and credentials contract](docs/architecture/SETTINGS.md) documents configuration, precedence, and interrupted mutation recovery.

Sessions supports search, creation, switching, rename, archive, restore, export, and exact confirmed deletion.
Memory supports search, filters, provenance, evidence, proposal review, correction, retraction, export, and deletion.
Imported and model-authored proposals remain untrusted until a distinct approval revision commits.
Tool permission requests preempt ordinary interaction and name the exact requested operation.

Settings exposes themes, contrast, zoom, font size, density, motion, timestamps, and submission behavior with provenance and reset.
Help provides searchable workflows, shortcuts, and recovery guidance.
Use `Ctrl/Cmd+K` for the command palette, `Ctrl/Cmd+N` for a session, `Ctrl/Cmd+F` for transcript search, and `F1` for Help.
Use Quit AutoHarness in the command palette for orderly shutdown.

Durable state replays after restarting the process.
If the renderer loses its runtime connection, restart before resending work with an uncertain outcome.
An unacknowledged renderer replacement currently requires a process restart.

## Validate and contribute

Run the complete local baseline:

```text
python scripts/validate_gui.py
```

It covers frontend types, tests and build, Python tooling, documentation links, Rust formatting, strict Clippy, workspace tests, rustdoc, doctests, generated themes, and isolated storage benchmarks.
Native packaged journeys and cross-platform release evidence are separate, deliberate checks.
See the [validation workflow](docs/release/GUI_VALIDATION_WORKFLOW.md).

Changes start from `dev` on `feat/<name>` branches and merge through a pull request into `dev`.
Promote `dev` to `main` with a dedicated pull request.
Read [CONTRIBUTING.md](CONTRIBUTING.md) and [AGENTS.md](AGENTS.md) before implementation.

## Architecture and roadmap

The headless engine is independent of React, Tauri, providers, storage adapters, and plugin runtimes.
The [client contract](crates/autoharness-client/src/lib.rs) separates the versioned wire protocol from renderer-neutral in-process messages and bounded channels.
The [GUI architecture](docs/architecture/GUI.md) defines carrier ordering, recovery, secret ingress, and presentation authority.

The runtime includes replayable sessions, scoped tools, immutable run budgets, provider retries and catalog caching, and persistent memory.
Evaluation-driven self-improvement and extension runtimes remain future work.
See the [project plan](docs/PROJECT_PLAN.md), [documentation map](docs/README.md), and [progress memory](docs/memory/progress.md) for verified capabilities and remaining evidence gaps.
Historical terminal results remain in documentation and Git history.

AutoHarness is licensed under the [MIT License](LICENSE).
