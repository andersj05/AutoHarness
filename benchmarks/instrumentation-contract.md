# Deferred latency instrumentation contract

Three Phase 1 latency metrics require boundaries that the current executable does not expose.
This contract defines the smallest safe instrumentation needed before those numbers can be automated.
It does not authorize adding content, credentials, provider payloads, or raw identifiers to telemetry.

## Clock and correlation rules

- Boundary timestamps must use one monotonic clock and an integer elapsed-nanosecond representation.
- A benchmark-only opaque correlation value may connect an input, attempt, provider chunk sequence, and rendered revision.
- Correlation values must be newly generated, non-secret, and useless outside the running benchmark.
- Markers must contain structural counts only, such as response byte length and a local chunk sequence.
- Benchmark output must keep harness and provider-network intervals in separate fields.

## Required boundaries

| Marker | Exact boundary |
| --- | --- |
| `first_draw_completed` | Immediately after the terminal backend successfully flushes the first frame |
| `input_accepted` | After the terminal update accepts a submit action and immediately before it queues the application intent |
| `provider_dispatch_started` | Immediately before the provider adapter begins request I/O, after durable preparation and start have committed |
| `provider_request_started` | At the provider transport request boundary used only for network metrics |
| `provider_chunk_received` | Immediately after one provider chunk is decoded and before it is queued for storage or projection |
| `rendered_delta` | Immediately after a successful draw that contains the correlated chunk's projected revision |
| `provider_stream_completed` | When provider transport completion is observed, before durable settlement |

Cold startup additionally needs an external launcher that records a monotonic timestamp immediately before process creation and receives `first_draw_completed` through a benchmark side channel.
A normal tracing file is not a suitable side channel because log buffering and file polling would contaminate the interval.
A real pseudo-terminal is required so the measurement exercises the actual terminal backend.

## Phase 3.5 implementation status

The `benchmark-instrumentation` feature implements `first_draw_completed`, `input_accepted`, `provider_dispatch_started`, `provider_chunk_received`, and `rendered_delta`.
The markers use one process monotonic origin and a loopback UDP side channel carrying only numeric structural fields.
The `terminal_latency` runner launches the real binary in a PTY, drives a loopback structural provider, and correlates the dispatch and render pairs.
Its external process-start sample ends when the first-draw datagram is received and therefore includes one loopback delivery interval, which every report labels.
`provider_request_started` and `provider_stream_completed` remain intentionally unavailable because the runner does not report loopback fixture latency as provider-network evidence.

## Formulas

| Metric | Formula | Classification |
| --- | --- | --- |
| Cold process start to first terminal draw | `first_draw_completed - launcher_process_start` | Harness startup |
| Input-to-request dispatch overhead | `provider_dispatch_started - input_accepted` | Harness overhead |
| Provider-chunk receipt to rendered-delta latency | `rendered_delta - provider_chunk_received` for the same local chunk sequence | Harness overhead |
| Provider time to first decoded chunk | `first provider_chunk_received - provider_request_started` | Network and provider latency |
| Provider stream duration | `provider_stream_completed - provider_request_started` | Network and provider latency |

The report must never subtract independently sampled wall-clock timestamps or infer a missing boundary from a neighboring storage event.
If correlation is lost or a marker is missing, that sample is invalid rather than zero.
