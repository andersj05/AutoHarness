# Benchmark results

This directory holds reviewed performance evidence produced by the checked-in Phase 1 benchmark environment.
The benchmark runner refuses to replace an existing result file.

Use `phase1-<machine>-<date>.json` for durable-store and recovery reports.
Use `idle-memory-<machine>-<date>.json` for resident-memory reports.
Copy and complete [reference-machine-template.md](reference-machine-template.md) for every machine whose measurements are used to set or evaluate a release threshold.

Do not commit credentials, environment dumps, prompts, responses, personal paths, or raw local trial output.
