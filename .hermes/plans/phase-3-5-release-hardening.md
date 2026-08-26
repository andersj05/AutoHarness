# Phase 3.5: Terminal release hardening

Branch: `feat/phase-3-5-release-hardening` cut from `dev` at `f25da8c`.

## Goal

Prove the complete Phase 3.x terminal product as a stable base before persistent
memory adds more state and UI, per the Phase 3.5 section of `docs/PROJECT_PLAN.md`.

## Vertical slices

Each slice lands behind failing tests first and is committed independently.

1. **PTY scenario harness and first run.** A checked-in integration harness drives
   the real `autoharness` binary through a real pseudo-terminal using
   `portable-pty` and asserts on parsed screen state via `vt100`. The first
   scenario covers a credential-free launch: interface renders, files stay inside
   the isolated data directory, quit restores the terminal, exit code is clean.
2. **Returning profile and offline resume PTY scenarios.** A session with durable
   history restarts with no provider reachable and the transcript, selected model,
   and session list are restored from replay; the composer accepts input offline.
3. **Multi-session, permission, and destructive-confirmation PTY scenarios.**
   `Ctrl+N`, `Ctrl+L`, rename, confirm-gated archive, and delete-with-export are
   exercised end to end; the tool permission overlay appears only for a real
   pending call and both outcomes settle durably.
4. **Settings persistence and terminal resize scenarios.** The settings overlay
   reflects provenance across restart; resize redraws without artifacts at
   multiple dimensions mid-run.
5. **Robustness suite.** Migration forward-compatibility (v1 to current), catalog
   cache corruption recovery, locked-vault degradation to session-only, forced
   shutdown (SIGKILL-equivalent) leaving a recoverable store, and network-loss
   attempt settlement as unknown with retry lineage preserved.
6. **Latency markers per the instrumentation contract.** Monotonic markers for
   `first_draw_completed`, correlated `input_accepted` /
   `provider_dispatch_started`, and correlated `provider_chunk_received` /
   `rendered_delta` land behind an opt-in benchmark feature; the benchmark runner
   gains a startup scenario that launches the binary in a PTY and reports the
   three deferred metrics with harness and network intervals separated.
7. **Live-provider smoke matrix.** Opt-in ignored structural probes extended so
   each supported provider covers plain chat plus every supported tool-call
   dialect (Gemini function calling; router plain chat added beside its existing
   HTTP-function probe). No credentials or content in checked-in evidence.
8. **Release checklist.** A documented checklist covering secret scanning,
   accessibility, terminal restoration, help and documentation accuracy, and
   database rollback preparation.

## Exit evidence

- All baseline gates pass on this branch; PTY scenarios are gated by
  `#[ignore]` only where they need a built binary or platform terminal.
- Benchmark report structure validated by unit tests; reference-machine numbers
  require operator hardware and stay out of CI.
- Live matrix remains opt-in and records only structural assertions.

## Non-goals

- Platform keyring smoke coverage beyond the fake vault (tracked Phase 3.3
  follow-up; requires interactive OS sessions).
- Recording authoritative benchmark numbers on this development machine; the
  reference-machine template stays unfilled until the operator designates one.
