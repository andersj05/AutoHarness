# Storage benchmark environment

The isolated Rust runner measures production SQLite append and synchronous projection throughput, transcript reads, and warm reopen with strict replay.
It does not measure GUI rendering or provider network latency.
Historical terminal instrumentation is retained as documentation only under [ADR-0020](../docs/adr/0020-retire-terminal-client.md).

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

## Historical terminal evidence

The terminal latency runner and its instrumentation were retired under [ADR-0020](../docs/adr/0020-retire-terminal-client.md).
Historical results and the instrumentation contract describe prior revisions only.
The current benchmark executable measures storage; GUI latency requires native desktop evidence.

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

Start the release application normally in one terminal and wait until the desktop baseline and catalog refresh have settled.
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
