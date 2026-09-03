//! Bounded, inert memory inspection and exact revision-scoped user intent.

use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::bounds::{validate_count, validate_text};
use crate::{DecimalU64, MemoryId, SafeFailure, UnixMillis, ValidationError};

/// Text data only: never interpreted as markup, a URL, or an executable action.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct MemoryText(String);

impl MemoryText {
    pub fn new(value: impl Into<String>) -> Result<Self, ValidationError> {
        let value = value.into();
        validate_text("memory_text", &value, 65_536)?;
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for MemoryText {
    type Error = ValidationError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<MemoryText> for String {
    fn from(value: MemoryText) -> Self {
        value.0
    }
}

impl fmt::Debug for MemoryText {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("MemoryText([REDACTED])")
    }
}

macro_rules! memory_enum {
    ($name:ident { $($variant:ident),+ }) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name { $($variant),+ }
    };
}

memory_enum!(MemoryStatus {
    Active,
    Proposed,
    Conflicting,
    Superseded,
    Rejected,
    Retracted,
    Expired,
    Deleted
});
memory_enum!(MemoryScope {
    User,
    Workspace,
    Session,
    Agent
});
memory_enum!(MemoryTrust {
    UserApproved,
    VerifiedObservation,
    Imported,
    UntrustedProposal
});
memory_enum!(MemoryOrigin {
    ExplicitUser,
    VerifiedTool,
    ImportedDocument,
    ModelProposal,
    Compaction
});
memory_enum!(MemorySensitivity {
    Public,
    Internal,
    Sensitive,
    Secret
});
memory_enum!(MemoryEvidenceAvailability {
    Retained,
    Absent,
    Erased
});
memory_enum!(MemoryRelationKind {
    DuplicateOf,
    Contradicts,
    Refines,
    Supersedes,
    Related,
    DerivedFrom
});
memory_enum!(MemoryFindingKind {
    Duplicate,
    Contradiction,
    SecretDetected,
    UnsupportedScope,
    MalformedContent,
    PolicyConflict,
    InjectionPattern,
    UngroundedEvidence
});
memory_enum!(MemoryStatusFilter {
    Eligible,
    All,
    Active,
    Proposed,
    Inactive
});
memory_enum!(MemoryScopeFilter {
    All,
    User,
    Workspace,
    Session,
    Agent
});
memory_enum!(MemoryPageDirection {
    First,
    Next,
    Previous
});

/// The coordinator validates query bounds and opaque cursors before storage access.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryQuery {
    pub view_generation: DecimalU64,
    pub literal: MemoryText,
    pub status: MemoryStatusFilter,
    pub scope: MemoryScopeFilter,
    pub direction: MemoryPageDirection,
    pub before: Option<MemoryText>,
}

/// No model-origin or trust fields are accepted from the renderer.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    deny_unknown_fields,
    rename_all = "snake_case",
    tag = "kind",
    content = "payload"
)]
pub enum MemoryCommand {
    Query(MemoryQuery),
    Remember {
        content: MemoryText,
    },
    Import {
        path: MemoryText,
    },
    Revise {
        memory_id: MemoryId,
        expected_last_sequence: DecimalU64,
        content: MemoryText,
    },
    Approve {
        memory_id: MemoryId,
        expected_last_sequence: DecimalU64,
        proposal_revision_id: MemoryId,
    },
    Reject {
        memory_id: MemoryId,
        expected_last_sequence: DecimalU64,
        proposal_revision_id: MemoryId,
    },
    Retract {
        memory_id: MemoryId,
        expected_last_sequence: DecimalU64,
        revision_id: MemoryId,
    },
    Delete {
        memory_id: MemoryId,
        expected_last_sequence: DecimalU64,
    },
    Export {
        memory_id: MemoryId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryEvidence {
    pub label: MemoryText,
    pub source: MemoryText,
    pub excerpt: Option<MemoryText>,
    pub availability: MemoryEvidenceAvailability,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryRelation {
    pub kind: MemoryRelationKind,
    pub memory_id: MemoryId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryFinding {
    pub kind: MemoryFindingKind,
    pub related_memory_id: MemoryText,
    pub summary: MemoryText,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryRevisionContext {
    pub expected_last_sequence: DecimalU64,
    pub revision_id: MemoryId,
    pub proposal_revision_id: Option<MemoryId>,
    pub scope_identity: MemoryText,
    pub origin: MemoryOrigin,
    pub sensitivity: MemorySensitivity,
    pub evidence: Vec<MemoryEvidence>,
    pub relations: Vec<MemoryRelation>,
    pub findings: Vec<MemoryFinding>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryAdmissionContext {
    pub provider_attempt: MemoryText,
    pub run_turn: u32,
    pub epoch: MemoryText,
    pub token_count: u32,
    pub source_revision: MemoryText,
    pub renderer_version: MemoryText,
    pub reason_factors: Vec<MemoryText>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryAdmission {
    pub session: MemoryText,
    pub model: MemoryText,
    pub reason: MemoryText,
    pub admitted_at_ms: UnixMillis,
    pub rank: u32,
    pub context: Option<MemoryAdmissionContext>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryDetail {
    pub revision: u32,
    pub content: Option<MemoryText>,
    pub source: MemoryText,
    pub trust: MemoryTrust,
    pub created_at_ms: UnixMillis,
    pub valid_until_ms: Option<UnixMillis>,
    pub admissions: Vec<MemoryAdmission>,
    pub revision_context: Option<MemoryRevisionContext>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryRow {
    pub memory_id: MemoryId,
    pub preview: MemoryText,
    pub status: MemoryStatus,
    pub scope: MemoryScope,
    pub updated_at_ms: UnixMillis,
    pub confidence_bps: Option<u16>,
    pub admission_count: u32,
    pub detail: Option<MemoryDetail>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    deny_unknown_fields,
    rename_all = "snake_case",
    tag = "kind",
    content = "payload"
)]
pub enum MemoryLoadState {
    Ready,
    Loading,
    Failed { failure: SafeFailure },
}

/// Bounded page. `total` is the host's page count, never an invented global count.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "MemoryPage")]
pub struct MemoryProjection {
    pub view_generation: DecimalU64,
    pub generation: DecimalU64,
    pub state: MemoryLoadState,
    pub rows: Vec<MemoryRow>,
    pub total: u32,
    pub stale: bool,
    pub next_cursor: Option<MemoryText>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MemoryPage {
    view_generation: DecimalU64,
    generation: DecimalU64,
    state: MemoryLoadState,
    rows: Vec<MemoryRow>,
    total: u32,
    stale: bool,
    next_cursor: Option<MemoryText>,
}

impl TryFrom<MemoryPage> for MemoryProjection {
    type Error = ValidationError;
    fn try_from(page: MemoryPage) -> Result<Self, Self::Error> {
        let value = Self {
            view_generation: page.view_generation,
            generation: page.generation,
            state: page.state,
            rows: page.rows,
            total: page.total,
            stale: page.stale,
            next_cursor: page.next_cursor,
        };
        value.validate()?;
        Ok(value)
    }
}

impl Default for MemoryProjection {
    fn default() -> Self {
        Self {
            view_generation: 0.into(),
            generation: 0.into(),
            state: MemoryLoadState::Ready,
            rows: Vec::new(),
            total: 0,
            stale: false,
            next_cursor: None,
        }
    }
}

impl MemoryProjection {
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_count("memory_rows", self.rows.len(), 100)?;
        let ids: BTreeSet<_> = self.rows.iter().map(|row| &row.memory_id).collect();
        if ids.len() != self.rows.len() || (self.total as usize) < self.rows.len() {
            return Err(ValidationError::Inconsistent {
                field: "memory_rows",
            });
        }
        for row in &self.rows {
            if let Some(detail) = &row.detail {
                validate_count("memory_admissions", detail.admissions.len(), 64)?;
                for admission in &detail.admissions {
                    if let Some(context) = &admission.context {
                        validate_count("memory_reason_factors", context.reason_factors.len(), 16)?;
                    }
                }
                if let Some(context) = &detail.revision_context {
                    validate_count("memory_evidence", context.evidence.len(), 64)?;
                    validate_count("memory_relations", context.relations.len(), 64)?;
                    validate_count("memory_findings", context.findings.len(), 256)?;
                }
            }
        }
        Ok(())
    }
}
