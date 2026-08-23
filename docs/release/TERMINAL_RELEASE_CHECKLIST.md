# Terminal release checklist

Use this checklist for every Phase 3.x terminal release candidate before promotion from `dev` to `main`.
Record pass or fail evidence without credentials, prompt text, model output, personal paths, or private provider endpoints.
A failed required item blocks promotion.

## Candidate identity

- [ ] Record the release-candidate Git commit.
- [ ] Record the Rust toolchain from `rust-toolchain.toml`.
- [ ] Record the Windows, macOS, and Linux CI run links for the same commit.
- [ ] Confirm the candidate was built from `dev` and contains no unrelated working-tree changes.

## Baseline quality gates

- [ ] `cargo fmt --all -- --check` passes.
- [ ] `cargo clippy --workspace --all-targets --all-features --locked --no-deps -- -D warnings` passes.
- [ ] `cargo test --workspace --all-targets --all-features --locked --no-fail-fast` passes.
- [ ] Warning-denied rustdoc and doctests pass in CI.
- [ ] The isolated benchmark workspace formatting, Clippy, and test gates pass.
- [ ] The documentation link and ADR index check passes.

## Cross-platform terminal scenarios

- [ ] First run renders the complete terminal without a credential and exits cleanly.
- [ ] A returning profile reports the expected provider and credential provenance after restart.
- [ ] A fresh user creates Gemini and router profiles, saves distinct keys, tests both, chooses a default model, switches providers, and reconnects after restart without shell setup.
- [ ] Duplicate profile configuration starts disconnected and cannot inherit or share the source profile's vault entry.
- [ ] Environment overrides remain read-only and visibly take precedence without copying their values into the vault.
- [ ] Disconnect and profile deletion name their exact scope, require confirmation, and never affect another profile's credential.
- [ ] Offline resume restores the selected model, transcript, and session list from durable state.
- [ ] Multi-session create, switch, rename, archive, undo, and export-before-delete behavior passes.
- [ ] Invalid tool calls are force-denied and repaired without human or capability authority.
- [ ] Permission deny performs no effect, permission allow performs only the exact effect, and both outcomes replay after restart.
- [ ] Settings provenance survives restart and remains readable at supported terminal sizes.
- [ ] Mid-run resize redraws correctly at 40x12, 60x18, 80x24, 120x40, and 120x50.
- [ ] Forced termination leaves a database that opens and replays on the next launch.
- [ ] Every scenario above passes on Windows, macOS, and Linux for the same candidate.

## Recovery and storage

- [ ] Migration from schema version 1 to the current schema passes without transcript or session loss.
- [ ] A database with a future schema version fails closed without mutation.
- [ ] Migration-history tampering and event corruption fail closed with safe diagnostics.
- [ ] A corrupt catalog cache is discarded and replaced by live discovery without damaging session history.
- [ ] A malformed profile document is preserved as `.bad` and replaced atomically.
- [ ] A missing or locked credential vault degrades to session-only offline operation.
- [ ] Interrupted credential save, disconnect, and profile deletion operations expose bounded non-secret recovery state and reconcile idempotently after restart.
- [ ] Schema-v1 profile documents migrate to schema 2 without losing profile configuration, active selection, or credential linkage.
- [ ] Network loss settles the attempt and preserves an explicit retry path without replaying unrelated failed input.
- [ ] Interrupted prepared, dispatched, permission-pending, and effect-started attempts recover according to their durable ambiguity rules.

## Live-provider matrix

- [ ] Gemini plain chat passes against the release candidate.
- [ ] Gemini streamed function calling reaches one complete safe HTTP tool call before tool completion.
- [ ] The configured OpenAI-compatible router plain chat path passes.
- [ ] The configured router function-calling path reaches one complete safe HTTP tool call before tool completion.
- [ ] The matrix records only provider name, adapter version, model identifier, date, and pass or fail.
- [ ] No live prompt, response, credential, request header, private endpoint, or raw provider payload is retained as evidence.

## Security and privacy

- [ ] Review every changed file for embedded credentials, tokens, private endpoints, personal data, and copied provider payloads.
- [ ] Scan the candidate source tree, test fixtures, benchmark reports, archives, logs, and generated application data for the release sentinel values.
- [ ] Confirm application logs contain structural fields only and no prompt, response, tool content, credential, or opaque provider payload.
- [ ] Confirm profile and settings documents contain credential references only.
- [ ] Confirm credential recovery records contain only operation kind, profile identity, and opaque reference.
- [ ] Run the unique sentinel through profile save, replace, test, disconnect, delete, restart recovery, rendered UI, and debug output.
- [ ] Confirm permission prompts display the exact bounded capability and resource without exposing secret content.
- [ ] Confirm redirects, ambient proxies, inherited process environments, shell execution, and workspace traversal remain denied by the capability adapters.

## Accessibility and usability

- [ ] Every important action is reachable without a mouse through the documented key, palette, or slash path.
- [ ] `F1` help matches the actual keys and follows the current focus.
- [ ] Focus, selection, pending state, failure, retry, and destructive confirmation remain distinguishable without relying on animation.
- [ ] Text remains readable and no required action disappears at every supported terminal size.
- [ ] Screen-reader-facing terminal text uses meaningful labels for provider, model, attempt, permission, and error states.
- [ ] Keyboard-only first run, model selection, prompt submission, permission response, session recovery, and quit are reviewed end to end.

## Terminal restoration

- [ ] Normal quit disables bracketed paste, shows the cursor, leaves the alternate screen, and restores raw mode.
- [ ] Provider failure and storage failure still restore the terminal.
- [ ] Panic restoration is exercised without leaving the shell unusable.
- [ ] Forced process termination is documented as unable to run in-process cleanup, and the invoking terminal recovers its own session state.

## Help and documentation accuracy

- [ ] Root README setup, environment variables, key bindings, provider behavior, and recovery guidance match the candidate.
- [ ] Architecture documents describe the current settings, credential, session, provider, and tool boundaries.
- [ ] The project plan, active memory, and progress memory agree with verified repository state.
- [ ] Every documented command has been executed successfully on the candidate or is explicitly marked as an operator-only live command.

## Benchmark evidence

- [ ] The storage and recovery benchmark report is from an approved reference machine and the same release candidate.
- [ ] The terminal latency report contains process-start-to-first-draw, application-initialize-to-first-draw, input-to-dispatch, and decoded-chunk-to-render metrics.
- [ ] Harness and provider-network intervals are separate, and unavailable network metrics are not represented as zero.
- [ ] The reference-machine record contains hardware, operating system, terminal, toolchain, power mode, background workload, and exact commands.
- [ ] Phase 3.4 fixed-size usability evidence is linked and reviewed with the candidate.

## Database rollback preparation

- [ ] Stop AutoHarness before taking the rollback backup.
- [ ] Back up the database, `-wal`, and `-shm` files together with the profile document and content-addressed artifact directory.
- [ ] Record the current SQLite schema version and application commit with the backup.
- [ ] Verify the backup opens and replays with the release candidate before promotion.
- [ ] Never open a database migrated by a newer candidate with an older binary unless that rollback path was explicitly tested.
- [ ] If rollback is required, restore the complete stopped-process backup instead of editing migration rows or event history.

## Promotion decision

- [ ] No open P0 or P1 defect remains in chat, sessions, settings, credentials, permissions, recovery, or rendering.
- [ ] Every exception has an owner, severity, user impact, and explicit release approval.
- [ ] One reviewer other than the implementer signs off on the evidence.
- [ ] Promote `dev` to `main` only after every required item passes.
