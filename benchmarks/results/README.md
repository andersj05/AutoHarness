# Benchmark results

This directory holds reviewed performance evidence produced by the checked-in Phase 1 benchmark environment.
The benchmark runner refuses to replace an existing result file.

Use `phase1-<machine>-<date>.json` for durable-store and recovery reports.
Use `idle-memory-<machine>-<date>.json` for resident-memory reports.
Use `tui-render-phase-<phase>-<platform>-<date>.md` for reviewed local frame-time and allocation comparisons, such as the [Phase 3.10 Step 9 evidence](tui-render-phase-3-10-windows-2026-08-27.md).
Copy and complete [reference-machine-template.md](reference-machine-template.md) for every machine whose measurements are used to set or evaluate a release threshold.

Do not commit credentials, environment dumps, prompts, responses, personal paths, or raw local trial output.
