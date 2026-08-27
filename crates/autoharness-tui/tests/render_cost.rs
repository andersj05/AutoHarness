use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

use allocation_counter::{AllocationInfo, measure};
use autoharness_domain::{ModelId, ModelRef, ProviderId};
use autoharness_tui::{
    AttemptKey, AttemptStatus, CatalogProjection, Model, ModelSummary, SessionProjection,
    SessionsProjection, TranscriptItem, UsageView, view,
};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

const WIDTH: u16 = 80;
const HEIGHT: u16 = 24;
const PRE_REDESIGN_P95_NS: u128 = 950_000;
const PRE_REDESIGN_ALLOCATIONS: u64 = 500;
const PRE_REDESIGN_ALLOCATED_BYTES: u64 = 90_000;
const PRE_REDESIGN_PEAK_ALLOCATIONS: u64 = 220;
const PRE_REDESIGN_PEAK_BYTES: u64 = 36_000;

fn model_ref() -> ModelRef {
    ModelRef::new(
        ProviderId::new("google-ai-studio").expect("provider"),
        ModelId::new("models/render-cost").expect("model"),
    )
}

fn render_model(turns: usize) -> Model {
    let transcript = (0..turns)
        .map(|index| TranscriptItem::Assistant {
            attempt_id: AttemptKey::new(format!("render-{index}")).expect("attempt"),
            text: format!("Bounded render row {index} with enough text to exercise wrapping."),
            status: AttemptStatus::Completed,
            usage: Some(UsageView {
                input_tokens: 32,
                output_tokens: 16,
            }),
            retry_of: None,
        })
        .collect();
    Model::new(
        Arc::new(SessionProjection {
            session_id: "render-cost".to_owned(),
            revision: u64::try_from(turns).unwrap_or(u64::MAX),
            selected_model: Some(model_ref()),
            transcript,
            permission_requests: Vec::new(),
        }),
        Arc::new(SessionsProjection::default()),
        Arc::new(CatalogProjection::Ready {
            models: vec![ModelSummary {
                model: model_ref(),
                display_name: "Render Cost".to_owned(),
                detail: "text".to_owned(),
                context_window_tokens: Some(1_000_000),
                selectable: true,
            }],
            stale: false,
        }),
    )
}

fn terminal() -> Terminal<TestBackend> {
    Terminal::new(TestBackend::new(WIDTH, HEIGHT)).expect("terminal")
}

fn draw(terminal: &mut Terminal<TestBackend>, model: &Model) {
    black_box(terminal.draw(|frame| view(frame, model)).expect("draw"));
}

fn allocation_sample(turns: usize) -> AllocationInfo {
    let model = render_model(turns);
    let mut terminal = terminal();
    draw(&mut terminal, &model);
    let _ = measure(|| draw(&mut terminal, &model));
    measure(|| draw(&mut terminal, &model))
}

#[test]
fn tail_render_allocations_do_not_scale_with_transcript_length() {
    let short = allocation_sample(32);
    let long = allocation_sample(4_096);
    assert!(
        long.count_total <= short.count_total.saturating_add(8),
        "allocation count grew with transcript length: short={short:?}, long={long:?}"
    );
    assert!(
        long.bytes_total <= short.bytes_total.saturating_add(2_048),
        "allocated bytes grew with transcript length: short={short:?}, long={long:?}"
    );
    assert!(
        long.bytes_max <= short.bytes_max.saturating_add(2_048),
        "peak live bytes grew with transcript length: short={short:?}, long={long:?}"
    );
    for (name, sample) in [("short", short), ("long", long)] {
        assert!(
            sample.count_total <= PRE_REDESIGN_ALLOCATIONS,
            "{name} render exceeded the pre-redesign allocation envelope: {sample:?}"
        );
        assert!(
            sample.bytes_total <= PRE_REDESIGN_ALLOCATED_BYTES,
            "{name} render exceeded the pre-redesign byte envelope: {sample:?}"
        );
        assert!(
            sample.count_max <= PRE_REDESIGN_PEAK_ALLOCATIONS,
            "{name} render exceeded the pre-redesign live-allocation envelope: {sample:?}"
        );
        assert!(
            sample.bytes_max <= PRE_REDESIGN_PEAK_BYTES,
            "{name} render exceeded the pre-redesign peak-byte envelope: {sample:?}"
        );
    }
}

#[test]
#[ignore = "release-mode render envelope report; run explicitly with --ignored --nocapture"]
fn report_render_cost_envelope() {
    for turns in [32, 4_096] {
        let model = render_model(turns);
        let mut terminal = terminal();
        for _ in 0..20 {
            draw(&mut terminal, &model);
        }
        let mut samples = Vec::with_capacity(500);
        for _ in 0..500 {
            let started = Instant::now();
            draw(&mut terminal, &model);
            samples.push(started.elapsed().as_nanos());
        }
        samples.sort_unstable();
        let median = samples[samples.len() / 2];
        let p95 = samples[(samples.len() * 95).div_ceil(100) - 1];
        let allocations = allocation_sample(turns);
        assert!(
            p95 <= PRE_REDESIGN_P95_NS,
            "{turns} turns exceeded the pre-redesign p95 envelope: {p95} ns"
        );
        println!(
            "turns={turns} samples={} median_ns={median} p95_ns={p95} allocations={} bytes_total={} peak_allocations={} peak_bytes={}",
            samples.len(),
            allocations.count_total,
            allocations.bytes_total,
            allocations.count_max,
            allocations.bytes_max,
        );
    }
}
