# Phase 1 benchmark environment

This directory is the checked-in benchmark environment required by the Phase 1 performance gate.
The core runner uses only AutoHarness path crates and Rust's standard library.
It performs no provider requests and does not read `GEMINI_API_KEY`.

## Coverage

| Required metric | Current automation | Network treatment |
| --- | --- | --- |
| Cold process start to first terminal draw | Deferred until the runtime exposes an externally observable first-draw marker | No network work belongs in the interval |
| Idle resident memory | Automated for an already idle process by [`scripts/sample-idle-memory.ps1`](scripts/sample-idle-memory.ps1) | The operator waits for catalog activity to settle before sampling |
| Input-to-request dispatch overhead | Deferred until input acceptance and provider dispatch have correlated monotonic markers | Stops immediately before provider I/O |
| Provider-chunk receipt to rendered-delta latency | Deferred until provider receipt and completed draw have correlated monotonic markers | Starts after network receipt, so network time is excluded |
| Event append and transcript projection throughput | Automated by the Rust runner against production SQLite settings | No network requests occur |
| Recovery time for representative session sizes | Automated by the Rust runner for 10, 100, and 1,000 completed turns by default | No network requests occur |
| LLM network latency | Explicitly reported as not measured by this environment | Must be reported as a separate metric |

The deferred metrics cannot be derived honestly from current logs or from a piped terminal.
Estimating them from `attempt_started` and `response_segment_committed` would mix storage and network boundaries, so this environment refuses to do that.
The required future marker contract is in [instrumentation-contract.md](instrumentation-contract.md).

## Representative benchmark run

Run this command from the repository root after the normal Rust quality gates pass:

```text
cargo run --release --locked --manifest-path benchmarks/Cargo.toml -- --output benchmarks/results/phase1-<machine>-<date>.json
```

Replace the placeholders with a non-sensitive machine label and an ISO date.
The output path must not already exist, which prevents accidental result replacement.
The default suite uses 10, 100, and 1,000 turns, four response chunks per turn, one warmup, five recorded fresh-database samples, and 20 transcript reads per sample.

Each timed append sample includes headless command validation, SQLite serialization, the production durable transaction, and synchronous read-model projection maintenance.
Session creation and model selection initialize the fixture but are excluded from append throughput.
Each recovery sample includes SQLite open, paginated authoritative event loading, and strict headless replay.
Recovery is a warm reopen with operating-system-managed caches because portable cache eviction would require privileged and platform-specific operations.

The JSON report records minimum, median, nearest-rank p95, mean, and maximum values.
It also records the workload shape and labels every unavailable metric instead of emitting a placeholder number.

## Fast validation

Use this smaller workload when validating benchmark code rather than collecting performance evidence:

```text
cargo fmt --manifest-path benchmarks/Cargo.toml -- --check
cargo test --locked --manifest-path benchmarks/Cargo.toml
cargo clippy --locked --manifest-path benchmarks/Cargo.toml --all-targets -- -D warnings
cargo run --release --locked --manifest-path benchmarks/Cargo.toml -- --turns 2,4 --chunks-per-turn 2 --samples 2 --warmups 0 --projection-reads 2
```

Debug-build numbers are not performance evidence.
Record release-build results only when the machine is otherwise idle and its power mode is documented.

## Idle resident memory

Start the release application normally in one terminal and wait until the first draw and catalog refresh have settled.
Obtain the AutoHarness process ID without recording the API key or process environment.
Then run this command from another PowerShell session:

```text
& benchmarks/scripts/sample-idle-memory.ps1 -TargetProcessId <pid> -Samples 20 -IntervalMilliseconds 250 -Output benchmarks/results/idle-memory-<machine>-<date>.json
```

The sampler uses `Get-Process.WorkingSet64`, which is available in Windows PowerShell and cross-platform PowerShell.
Its result is a resident working-set sample, not an allocation count or peak-memory measurement.
The operator is responsible for declaring when the process is idle.

## Result provenance

Copy [results/reference-machine-template.md](results/reference-machine-template.md) next to any result proposed as release evidence and complete every applicable field.
Never place credentials, environment dumps, prompts, model responses, personal directory paths, or private hostnames in benchmark results.
Keep raw local trial output uncommitted unless it is intentional project evidence.
