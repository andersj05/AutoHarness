# Phase 1 and Phase 3.5 benchmark environment

This directory contains the checked-in storage and terminal latency benchmark runners required by the Phase 1 performance gate and Phase 3.5 terminal release hardening.
The storage runner uses only AutoHarness path crates and Rust's standard library.
The terminal runner uses a real pseudo-terminal, a loopback structural router fixture, and the opt-in benchmark marker channel.
Neither runner reads a real provider credential or records prompt or response content.

## Coverage

| Required metric | Current automation | Network treatment |
| --- | --- | --- |
| Cold process start to first terminal draw | Automated by the `terminal_latency` runner against the instrumented release binary | No provider request starts before the first draw |
| Idle resident memory | Automated for an already idle process by [`scripts/sample-idle-memory.ps1`](scripts/sample-idle-memory.ps1) | The operator waits for catalog activity to settle before sampling |
| Input-to-request dispatch overhead | Automated from correlated `input_accepted` and `provider_dispatch_started` monotonic markers | Stops immediately before provider I/O |
| Provider-chunk receipt to rendered-delta latency | Automated from correlated `provider_chunk_received` and `rendered_delta` monotonic markers | Starts after a decoded loopback-provider chunk, so network time is excluded |
| Event append and transcript projection throughput | Automated by the storage runner against production SQLite settings | No network requests occur |
| Recovery time for representative session sizes | Automated by the Rust runner for 10, 100, and 1,000 completed turns by default | No network requests occur |
| LLM network latency | Explicitly reported as not measured by this environment | Must be reported as a separate metric |

The terminal metrics cannot be derived honestly from ordinary logs or from a piped terminal.
The opt-in application feature emits only numeric structural markers over loopback UDP according to [instrumentation-contract.md](instrumentation-contract.md).
The process-start metric includes one loopback datagram delivery because the launcher and application cannot serialize a shared `std::time::Instant`; the report names that side-channel interval explicitly.

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

## Terminal latency run

Build the release application with instrumentation, then launch the terminal runner against that exact executable:

```text
cargo build --release --locked -p autoharness-app --features benchmark-instrumentation
cargo run --release --locked --manifest-path benchmarks/Cargo.toml --bin terminal_latency -- --executable target/release/autoharness --samples 20
```

Use `target/release/autoharness.exe` on Windows.
Set `AUTOHARNESS_TERMINAL_BENCHMARK_OUTPUT` to a new result path when collecting evidence; the runner refuses to replace an existing report.
The runner starts a loopback OpenAI-compatible structural fixture, drives model selection and one prompt through a real PTY, and reports startup, input-to-dispatch, and decoded-chunk-to-render latency separately from network time.
The instrumented binary emits no content, credentials, provider payloads, paths, or durable identifiers through the marker channel.

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
