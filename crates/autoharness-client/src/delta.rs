use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{
    ClientSnapshot, MAX_TRANSCRIPT_ITEMS, ModelRef, PermissionRequest, SessionId,
    SessionProjection, SessionRevision, SessionSummary, TranscriptItem, ValidationError,
};

/// One bounded replacement inside the active session transcript.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TranscriptSplice {
    pub start: u32,
    pub delete_count: u32,
    pub items: Vec<TranscriptItem>,
}

impl TranscriptSplice {
    fn new(
        start: u32,
        delete_count: u32,
        items: Vec<TranscriptItem>,
    ) -> Result<Self, ValidationError> {
        if items.len() > MAX_TRANSCRIPT_ITEMS {
            return Err(ValidationError::TooMany {
                field: "transcript_delta_items",
                max_items: MAX_TRANSCRIPT_ITEMS,
                actual_items: items.len(),
            });
        }
        Ok(Self {
            start,
            delete_count,
            items,
        })
    }

    fn between(previous: &[TranscriptItem], next: &[TranscriptItem]) -> Self {
        let prefix = previous
            .iter()
            .zip(next)
            .take_while(|(left, right)| left == right)
            .count();
        let suffix_limit = previous.len().min(next.len()).saturating_sub(prefix);
        let suffix = previous
            .iter()
            .rev()
            .zip(next.iter().rev())
            .take(suffix_limit)
            .take_while(|(left, right)| left == right)
            .count();
        let previous_end = previous.len().saturating_sub(suffix);
        let next_end = next.len().saturating_sub(suffix);
        Self {
            start: u32::try_from(prefix).expect("bounded transcript index fits u32"),
            delete_count: u32::try_from(previous_end.saturating_sub(prefix))
                .expect("bounded transcript count fits u32"),
            items: next[prefix..next_end].to_vec(),
        }
    }
}

impl<'de> Deserialize<'de> for TranscriptSplice {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireSplice {
            start: u32,
            delete_count: u32,
            items: Vec<TranscriptItem>,
        }
        let wire = WireSplice::deserialize(deserializer)?;
        Self::new(wire.start, wire.delete_count, wire.items).map_err(D::Error::custom)
    }
}

/// Incremental update for the already-active session.
///
/// Changes outside this exact session continue to use a complete snapshot baseline.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ActiveSessionDelta {
    pub session_id: SessionId,
    pub revision: SessionRevision,
    pub summary: SessionSummary,
    pub selected_model: Option<ModelRef>,
    pub transcript: TranscriptSplice,
    pub permission_requests: Vec<PermissionRequest>,
}

impl ActiveSessionDelta {
    #[allow(clippy::too_many_arguments)]
    fn new(
        session_id: SessionId,
        revision: SessionRevision,
        summary: SessionSummary,
        selected_model: Option<ModelRef>,
        transcript: TranscriptSplice,
        permission_requests: Vec<PermissionRequest>,
    ) -> Result<Self, ValidationError> {
        if summary.session_id != session_id {
            return Err(ValidationError::Inconsistent {
                field: "session_delta_summary",
            });
        }
        SessionProjection::new(
            session_id.clone(),
            revision,
            selected_model.clone(),
            transcript.items.clone(),
            permission_requests.clone(),
        )?;
        Ok(Self {
            session_id,
            revision,
            summary,
            selected_model,
            transcript,
            permission_requests,
        })
    }

    /// Builds a delta only when all changes are confined to the active session.
    #[must_use]
    pub fn between(previous: &ClientSnapshot, next: &ClientSnapshot) -> Option<Self> {
        if previous.schema_version != next.schema_version
            || previous.lifecycle != next.lifecycle
            || previous.active_session_id != next.active_session_id
            || previous.catalog != next.catalog
            || previous.providers != next.providers
            || previous.settings != next.settings
            || previous.sessions.len() != next.sessions.len()
        {
            return None;
        }
        let (Some(previous_active), Some(next_active)) =
            (&previous.active_session, &next.active_session)
        else {
            return None;
        };
        if previous_active.session_id != next_active.session_id {
            return None;
        }
        let active_id = &next_active.session_id;
        if previous
            .sessions
            .iter()
            .zip(&next.sessions)
            .any(|(left, right)| {
                left.session_id != right.session_id
                    || (left.session_id != *active_id && left != right)
            })
        {
            return None;
        }
        let summary = next
            .sessions
            .iter()
            .find(|summary| summary.session_id == *active_id)?
            .clone();
        Some(Self {
            session_id: active_id.clone(),
            revision: next_active.revision,
            summary,
            selected_model: next_active.selected_model.clone(),
            transcript: TranscriptSplice::between(
                &previous_active.transcript,
                &next_active.transcript,
            ),
            permission_requests: next_active.permission_requests.clone(),
        })
    }

    /// Applies this update to an authoritative baseline and revalidates the result.
    pub fn apply_to(&self, snapshot: &ClientSnapshot) -> Result<ClientSnapshot, ValidationError> {
        if snapshot.active_session_id.as_ref() != Some(&self.session_id) {
            return Err(ValidationError::Inconsistent {
                field: "session_delta_identity",
            });
        }
        let Some(active) = &snapshot.active_session else {
            return Err(ValidationError::Inconsistent {
                field: "session_delta_identity",
            });
        };
        if active.session_id != self.session_id {
            return Err(ValidationError::Inconsistent {
                field: "session_delta_identity",
            });
        }
        let start = self.transcript.start as usize;
        let delete_count = self.transcript.delete_count as usize;
        let end = start
            .checked_add(delete_count)
            .filter(|end| *end <= active.transcript.len())
            .ok_or(ValidationError::Inconsistent {
                field: "transcript_delta_range",
            })?;
        let final_len = active
            .transcript
            .len()
            .saturating_sub(delete_count)
            .saturating_add(self.transcript.items.len());
        if final_len > MAX_TRANSCRIPT_ITEMS {
            return Err(ValidationError::TooMany {
                field: "transcript",
                max_items: MAX_TRANSCRIPT_ITEMS,
                actual_items: final_len,
            });
        }

        let mut transcript = active.transcript.clone();
        transcript.splice(start..end, self.transcript.items.clone());
        let active_session = SessionProjection::new(
            self.session_id.clone(),
            self.revision,
            self.selected_model.clone(),
            transcript,
            self.permission_requests.clone(),
        )?;
        let mut sessions = snapshot.sessions.clone();
        let Some(summary) = sessions
            .iter_mut()
            .find(|summary| summary.session_id == self.session_id)
        else {
            return Err(ValidationError::Inconsistent {
                field: "session_delta_summary",
            });
        };
        *summary = self.summary.clone();
        ClientSnapshot::new(
            snapshot.lifecycle.clone(),
            snapshot.active_session_id.clone(),
            sessions,
            Some(active_session),
            snapshot.catalog.clone(),
            snapshot.providers.clone(),
            snapshot.settings.clone(),
            snapshot.provider_recovery_pending.get(),
        )
    }
}

impl<'de> Deserialize<'de> for ActiveSessionDelta {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireDelta {
            session_id: SessionId,
            revision: SessionRevision,
            summary: SessionSummary,
            selected_model: Option<ModelRef>,
            transcript: TranscriptSplice,
            permission_requests: Vec<PermissionRequest>,
        }
        let wire = WireDelta::deserialize(deserializer)?;
        Self::new(
            wire.session_id,
            wire.revision,
            wire.summary,
            wire.selected_model,
            wire.transcript,
            wire.permission_requests,
        )
        .map_err(D::Error::custom)
    }
}
