//! Opt-in monotonic terminal latency markers over a loopback UDP side channel.
//!
//! Markers carry only process-local numeric correlation, sequence, byte-count,
//! revision, and elapsed-time fields. No prompt, response, credential, provider
//! payload, durable identity, or filesystem path enters this channel.

use std::collections::{BTreeMap, HashMap};
use std::net::UdpSocket;
use std::sync::{LazyLock, Mutex, OnceLock};
use std::time::Instant;

use crate::RequestId;

const MARKER_ADDRESS_ENV: &str = "AUTOHARNESS_BENCHMARK_MARKER_ADDR";

static STARTED: OnceLock<Instant> = OnceLock::new();
static SOCKET: LazyLock<Option<UdpSocket>> = LazyLock::new(|| {
    let address = std::env::var(MARKER_ADDRESS_ENV).ok()?;
    let socket = UdpSocket::bind("127.0.0.1:0").ok()?;
    socket.connect(address).ok()?;
    Some(socket)
});
static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| Mutex::new(State::default()));

#[derive(Default)]
struct State {
    next_correlation: u64,
    request_correlations: HashMap<u64, u64>,
    attempt_correlations: HashMap<String, u64>,
    chunk_sequences: HashMap<String, u64>,
    pending_renders: BTreeMap<(String, u64), Vec<(u64, u64)>>,
}

/// Initializes the process monotonic origin and optional marker socket.
pub fn initialize() {
    let _ = STARTED.set(Instant::now());
    let _ = &*SOCKET;
    let _ = &*STATE;
}

/// Emits the boundary immediately after the first successful terminal flush.
pub fn first_draw_completed() {
    emit("first_draw_completed", None, None, None, None);
}

/// Creates a benchmark-only correlation when prompt submission is accepted.
pub fn input_accepted(request_id: RequestId) {
    if marker_socket().is_none() {
        return;
    }
    let correlation = {
        let mut state = state().lock().expect("benchmark marker state");
        state.next_correlation = state.next_correlation.saturating_add(1).max(1);
        let correlation = state.next_correlation;
        state
            .request_correlations
            .insert(request_id.get(), correlation);
        correlation
    };
    emit("input_accepted", Some(correlation), None, None, None);
}

/// Binds the UI correlation to an attempt immediately before provider dispatch.
pub fn provider_dispatch_started(request_id: RequestId, attempt_id: &str) {
    if marker_socket().is_none() {
        return;
    }
    let correlation = {
        let mut state = state().lock().expect("benchmark marker state");
        let Some(correlation) = state.request_correlations.remove(&request_id.get()) else {
            return;
        };
        state
            .attempt_correlations
            .insert(attempt_id.to_owned(), correlation);
        correlation
    };
    emit(
        "provider_dispatch_started",
        Some(correlation),
        None,
        None,
        None,
    );
}

/// Emits a decoded text-chunk boundary and returns its local sequence.
#[must_use]
pub fn provider_chunk_received(attempt_id: &str, bytes: usize) -> Option<u64> {
    marker_socket()?;
    let (correlation, sequence) = {
        let mut state = state().lock().expect("benchmark marker state");
        let correlation = *state.attempt_correlations.get(attempt_id)?;
        let sequence = state
            .chunk_sequences
            .entry(attempt_id.to_owned())
            .or_default();
        *sequence = sequence.saturating_add(1);
        (correlation, *sequence)
    };
    emit(
        "provider_chunk_received",
        Some(correlation),
        Some(sequence),
        Some(bytes),
        None,
    );
    Some(sequence)
}

/// Associates one committed projection revision with its decoded provider chunk.
pub fn projection_committed(
    session_id: &str,
    revision: u64,
    attempt_id: &str,
    chunk_sequence: u64,
) {
    if marker_socket().is_none() {
        return;
    }
    let mut state = state().lock().expect("benchmark marker state");
    let Some(&correlation) = state.attempt_correlations.get(attempt_id) else {
        return;
    };
    state
        .pending_renders
        .entry((session_id.to_owned(), revision))
        .or_default()
        .push((correlation, chunk_sequence));
}

/// Emits every pending chunk included in a successfully flushed projection.
pub fn rendered_projection(session_id: &str, revision: u64) {
    if marker_socket().is_none() {
        return;
    }
    let rendered = {
        let mut state = state().lock().expect("benchmark marker state");
        let keys = state
            .pending_renders
            .keys()
            .filter(|(candidate, candidate_revision)| {
                candidate == session_id && *candidate_revision <= revision
            })
            .cloned()
            .collect::<Vec<_>>();
        keys.into_iter()
            .flat_map(|key| state.pending_renders.remove(&key).unwrap_or_default())
            .collect::<Vec<_>>()
    };
    for (correlation, sequence) in rendered {
        emit(
            "rendered_delta",
            Some(correlation),
            Some(sequence),
            None,
            Some(revision),
        );
    }
}

fn state() -> &'static Mutex<State> {
    &STATE
}

fn marker_socket() -> Option<&'static UdpSocket> {
    SOCKET.as_ref()
}

fn emit(
    marker: &str,
    correlation: Option<u64>,
    sequence: Option<u64>,
    bytes: Option<usize>,
    revision: Option<u64>,
) {
    let Some(socket) = marker_socket() else {
        return;
    };
    let elapsed_ns = STARTED.get_or_init(Instant::now).elapsed().as_nanos();
    let correlation = correlation.map_or_else(|| "null".to_owned(), |value| value.to_string());
    let sequence = sequence.map_or_else(|| "null".to_owned(), |value| value.to_string());
    let bytes = bytes.map_or_else(|| "null".to_owned(), |value| value.to_string());
    let revision = revision.map_or_else(|| "null".to_owned(), |value| value.to_string());
    let message = format!(
        "{{\"schema_version\":1,\"marker\":\"{marker}\",\"elapsed_ns\":{elapsed_ns},\"correlation\":{correlation},\"sequence\":{sequence},\"bytes\":{bytes},\"revision\":{revision}}}"
    );
    let _ = socket.send(message.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_correlates_render_only_after_projection_commit() {
        let mut state = State::default();
        state.request_correlations.insert(7, 3);
        state.attempt_correlations.insert("attempt".to_owned(), 3);
        state
            .pending_renders
            .entry(("session".to_owned(), 9))
            .or_default()
            .push((3, 1));

        assert_eq!(state.request_correlations.remove(&7), Some(3));
        assert_eq!(state.attempt_correlations.get("attempt"), Some(&3));
        assert_eq!(
            state
                .pending_renders
                .remove(&("session".to_owned(), 9))
                .expect("pending render"),
            vec![(3, 1)]
        );
    }
}
