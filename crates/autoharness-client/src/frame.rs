use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{
    ActiveSessionDelta, CLIENT_SCHEMA_VERSION, ClientNotice, ClientSnapshot, TransportRevision,
};

/// Why a complete authoritative snapshot was emitted.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotReason {
    Initial,
    Projection,
    Resynchronization,
}

/// Ordered carrier payload.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    deny_unknown_fields,
    rename_all = "snake_case",
    tag = "kind",
    content = "payload"
)]
pub enum FramePayload {
    Snapshot {
        reason: SnapshotReason,
        snapshot: Box<ClientSnapshot>,
    },
    ActiveSessionDelta(Box<ActiveSessionDelta>),
    Notice(ClientNotice),
}

/// One versioned frame in the ordered host-to-client stream.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ServerFrame {
    pub schema_version: u16,
    pub revision: TransportRevision,
    pub payload: FramePayload,
}

impl ServerFrame {
    /// Constructs a complete startup, projection, or resynchronization snapshot frame.
    #[must_use]
    pub fn snapshot(
        revision: TransportRevision,
        reason: SnapshotReason,
        snapshot: ClientSnapshot,
    ) -> Self {
        Self {
            schema_version: CLIENT_SCHEMA_VERSION,
            revision,
            payload: FramePayload::Snapshot {
                reason,
                snapshot: Box::new(snapshot),
            },
        }
    }

    /// Constructs a correlated or process-lifecycle notice frame.
    #[must_use]
    pub const fn notice(revision: TransportRevision, notice: ClientNotice) -> Self {
        Self {
            schema_version: CLIENT_SCHEMA_VERSION,
            revision,
            payload: FramePayload::Notice(notice),
        }
    }

    /// Constructs an incremental active-session frame.
    #[must_use]
    pub fn active_session_delta(revision: TransportRevision, delta: ActiveSessionDelta) -> Self {
        Self {
            schema_version: CLIENT_SCHEMA_VERSION,
            revision,
            payload: FramePayload::ActiveSessionDelta(Box::new(delta)),
        }
    }

    /// Classifies this frame relative to the last authoritative applied revision.
    ///
    /// Initial and resynchronization snapshots establish a baseline.
    /// A newer resynchronization snapshot repairs any gap instead of depending on it.
    #[must_use]
    pub fn classify_after(&self, last_applied: Option<TransportRevision>) -> FrameDisposition {
        let baseline = matches!(
            self.payload,
            FramePayload::Snapshot {
                reason: SnapshotReason::Initial | SnapshotReason::Resynchronization,
                ..
            }
        );
        match last_applied {
            None if baseline => FrameDisposition::Baseline,
            None => FrameDisposition::InvalidBaseline {
                received: self.revision,
            },
            Some(last) if self.revision <= last => FrameDisposition::Stale {
                last_applied: last,
                received: self.revision,
            },
            Some(_)
                if matches!(
                    self.payload,
                    FramePayload::Snapshot {
                        reason: SnapshotReason::Resynchronization,
                        ..
                    }
                ) =>
            {
                FrameDisposition::Baseline
            }
            Some(last) => match last.next() {
                Ok(expected) if self.revision == expected => FrameDisposition::Next,
                Ok(expected) => FrameDisposition::Gap {
                    expected,
                    received: self.revision,
                },
                Err(_) => FrameDisposition::SequenceExhausted { last_applied: last },
            },
        }
    }
}

impl<'de> Deserialize<'de> for ServerFrame {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireFrame {
            schema_version: u16,
            revision: TransportRevision,
            payload: FramePayload,
        }
        let wire = WireFrame::deserialize(deserializer)?;
        if wire.schema_version != CLIENT_SCHEMA_VERSION {
            return Err(D::Error::custom("unsupported server frame schema version"));
        }
        Ok(Self {
            schema_version: CLIENT_SCHEMA_VERSION,
            revision: wire.revision,
            payload: wire.payload,
        })
    }
}

/// Client-side decision for one received frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameDisposition {
    /// Replace all client state from this complete authoritative baseline.
    Baseline,
    /// Apply the frame after the current state.
    Next,
    /// Ignore a duplicate or older frame.
    Stale {
        last_applied: TransportRevision,
        received: TransportRevision,
    },
    /// Stop applying dependent frames and request a complete resynchronization snapshot.
    Gap {
        expected: TransportRevision,
        received: TransportRevision,
    },
    /// The first observed frame cannot initialize client state.
    InvalidBaseline { received: TransportRevision },
    /// No further dependent frame can follow the maximum transport revision.
    SequenceExhausted { last_applied: TransportRevision },
}
