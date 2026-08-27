# Phase 3.10 TUI render-cost evidence

**Date:** 2026-08-27

**Status:** Local conformance evidence, not Phase 3.9 reference-machine evidence.

## Scope

This report compares the same 80x24 `TestBackend` Chat render workload at the pre-redesign commit `0057a1a` and the Phase 3.10 Step 9 implementation.
Each case uses a warmed terminal, 500 release-mode frame samples, and one isolated allocation sample after an allocation-counting warmup.
The 32-turn case defines the recorded pre-redesign envelope because it is representative of a normal visible session.
The 4,096-turn case checks that frame work and live allocation do not grow with durable transcript length while tail-follow is active.
The run used the local Windows x86_64 development environment with an uncontrolled power mode, so the values are suitable for regression comparison but not release approval.

## Results

| Revision | Turns | Median frame | p95 frame | Allocations | Allocated bytes | Peak live allocations | Peak live bytes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Pre-redesign `0057a1a` | 32 | 0.6904 ms | 0.9462 ms | 483 | 82,701 | 200 | 32,086 |
| Pre-redesign `0057a1a` | 4,096 | 47.6065 ms | 108.2226 ms | 49,258 | 5,147,401 | 24,584 | 3,249,738 |
| Step 9 bounded tail | 32 | 0.1076 ms | 0.1518 ms | 333 | 67,444 | 12 | 8,300 |
| Step 9 bounded tail | 4,096 | 0.1141 ms | 0.1740 ms | 333 | 67,486 | 12 | 8,302 |

## Gate

The automated allocation gate permits at most 500 allocations, 90,000 allocated bytes, 220 peak live allocations, and 36,000 peak live bytes per warmed frame.
It also permits no more than eight additional allocations or 2,048 additional allocated or live bytes between 32 and 4,096 turns.
The explicit release-mode report gate permits at most 0.95 ms p95 frame time, matching the rounded pre-redesign 32-turn result.
Both Step 9 cases pass with substantial headroom, and the 4,096-turn case no longer scales with transcript length.

## Reproduction

Run the allocation gate from the repository root:

```text
cargo test --locked -p autoharness-tui --test render_cost tail_render_allocations_do_not_scale_with_transcript_length -- --exact
```

Run the timing report in release mode:

```text
cargo test --release --locked -p autoharness-tui --test render_cost report_render_cost_envelope -- --ignored --exact --nocapture
```
