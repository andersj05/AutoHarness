use std::cmp::Reverse;

use autoharness_domain::{
    AgentId, ConfidenceBasisPoints, MemoryContent, MemoryId, MemoryKind, MemoryRevisionId,
    MemoryRevisionStatus, MemoryScope, MemoryValidity, Sensitivity, TimestampMillis, TrustClass,
    UserId, WorkspaceId,
};

/// Fixed-point contribution explaining one deterministic rank decision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RankFactor {
    /// Stable machine-readable factor key.
    pub key: &'static str,
    /// Signed score contribution.
    pub contribution: i64,
    /// Stable reason code rendered by inspection clients.
    pub reason: RankReason,
}

/// Stable explanation for one ranking factor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RankReason {
    /// Revision authority contributed to ranking.
    SourceAuthority,
    /// Exact lexical match contributed to ranking.
    ExactMatch,
    /// Bounded lexical overlap contributed to ranking.
    LexicalOverlap,
    /// Scope specificity contributed to ranking.
    ScopeSpecificity,
    /// Freshness policy contributed to ranking.
    Freshness,
    /// Confidence contributed without changing trust.
    Confidence,
}

/// One immutable retrieval candidate produced by structured filters or FTS.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryCandidate {
    /// Stable memory identity.
    pub memory_id: MemoryId,
    /// Exact immutable revision identity.
    pub revision_id: MemoryRevisionId,
    /// Current immutable revision status.
    pub status: MemoryRevisionStatus,
    /// Exact authorization scope.
    pub scope: MemoryScope,
    /// Semantic memory kind.
    pub kind: MemoryKind,
    /// Trust assigned by trusted policy.
    pub trust: TrustClass,
    /// Confidence in basis points.
    pub confidence: ConfidenceBasisPoints,
    /// Sensitivity handling class.
    pub sensitivity: Sensitivity,
    /// Explicit validity interval.
    pub validity: MemoryValidity,
    /// Exact bounded content.
    pub content: MemoryContent,
    /// Canonical digest of exact normalized content.
    pub content_hash: autoharness_domain::Sha256Digest,
    /// Revision creation time used only through a deterministic freshness bucket.
    pub created_at: TimestampMillis,
    /// Whether an exact structured or lexical key matched.
    pub exact_match: bool,
    /// Quantized lexical relevance in basis points.
    pub lexical_basis_points: u16,
    /// Whether a contradiction remains unresolved.
    pub conflicted: bool,
}

/// Exact eligible identities for one retrieval boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetrievalScope {
    /// Current local user identity.
    pub user_id: UserId,
    /// Current workspace identity.
    pub workspace_id: WorkspaceId,
    /// Current session identity.
    pub session_id: autoharness_domain::SessionId,
    /// Selected agent identity, when an explicit agent is active.
    pub agent_id: Option<AgentId>,
    /// Explicit deterministic observation time.
    pub as_of: TimestampMillis,
    /// Highest sensitivity the target context permits.
    pub sensitivity_ceiling: Sensitivity,
}

/// Candidate plus deterministic score and explanation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RankedMemory {
    /// Original immutable candidate.
    pub candidate: MemoryCandidate,
    /// Final fixed-point score.
    pub score: i64,
    /// Ordered factor breakdown persisted with admission.
    pub factors: Vec<RankFactor>,
}

/// Replaceable deterministic memory ranking boundary.
pub trait MemoryRanker {
    /// Stable algorithm version persisted with each context turn.
    fn version(&self) -> &'static str;

    /// Filters and ranks candidates with a stable total order.
    fn rank(&self, scope: &RetrievalScope, candidates: Vec<MemoryCandidate>) -> Vec<RankedMemory>;
}

/// Version 1 integer ranker with stable identity ties.
#[derive(Clone, Copy, Debug, Default)]
pub struct DeterministicRankerV1;

impl MemoryRanker for DeterministicRankerV1 {
    fn version(&self) -> &'static str {
        "deterministic_rank_v1"
    }

    fn rank(&self, scope: &RetrievalScope, candidates: Vec<MemoryCandidate>) -> Vec<RankedMemory> {
        let mut ranked: Vec<_> = candidates
            .into_iter()
            .filter(|candidate| eligible(scope, candidate))
            .map(|candidate| rank_one(scope, candidate))
            .collect();
        ranked.sort_by_key(|item| {
            (
                Reverse(item.score),
                item.candidate.revision_id.clone(),
                item.candidate.memory_id.clone(),
            )
        });
        ranked
    }
}

fn eligible(scope: &RetrievalScope, candidate: &MemoryCandidate) -> bool {
    candidate.status == MemoryRevisionStatus::Active
        && candidate.trust != TrustClass::UntrustedProposal
        && !candidate.conflicted
        && candidate.created_at <= scope.as_of
        && sensitivity_score(candidate.sensitivity) <= sensitivity_score(scope.sensitivity_ceiling)
        && scope_matches(scope, &candidate.scope)
        && validity_contains(candidate.validity, scope.as_of)
}

fn scope_matches(retrieval: &RetrievalScope, memory: &MemoryScope) -> bool {
    match memory {
        MemoryScope::User(id) => id == &retrieval.user_id,
        MemoryScope::Workspace(id) => id == &retrieval.workspace_id,
        MemoryScope::Session(id) => id == &retrieval.session_id,
        MemoryScope::Agent(id) => retrieval.agent_id.as_ref() == Some(id),
    }
}

fn validity_contains(validity: MemoryValidity, as_of: TimestampMillis) -> bool {
    match validity {
        MemoryValidity::Indefinite => true,
        MemoryValidity::From { valid_from } => as_of.get() >= valid_from.get(),
        MemoryValidity::Until { valid_until } => as_of.get() < valid_until.get(),
        MemoryValidity::Window(window) => {
            as_of.get() >= window.valid_from().get() && as_of.get() < window.valid_until().get()
        }
    }
}

fn rank_one(scope: &RetrievalScope, candidate: MemoryCandidate) -> RankedMemory {
    let authority = authority_score(candidate.trust);
    let specificity = scope_score(&candidate.scope);
    let exact = if candidate.exact_match { 2_000 } else { 0 };
    let lexical = i64::from(candidate.lexical_basis_points.min(10_000)) / 5;
    let confidence = i64::from(candidate.confidence.get()) / 20;
    let freshness = freshness_score(scope.as_of, candidate.created_at);
    let factors = vec![
        RankFactor {
            key: "authority",
            contribution: authority,
            reason: RankReason::SourceAuthority,
        },
        RankFactor {
            key: "exact_match",
            contribution: exact,
            reason: RankReason::ExactMatch,
        },
        RankFactor {
            key: "lexical_overlap",
            contribution: lexical,
            reason: RankReason::LexicalOverlap,
        },
        RankFactor {
            key: "scope_specificity",
            contribution: specificity,
            reason: RankReason::ScopeSpecificity,
        },
        RankFactor {
            key: "freshness",
            contribution: freshness,
            reason: RankReason::Freshness,
        },
        RankFactor {
            key: "confidence",
            contribution: confidence,
            reason: RankReason::Confidence,
        },
    ];
    let score = factors.iter().map(|factor| factor.contribution).sum();
    RankedMemory {
        candidate,
        score,
        factors,
    }
}

const fn authority_score(trust: TrustClass) -> i64 {
    match trust {
        TrustClass::UserApproved => 4_000,
        TrustClass::VerifiedObservation => 3_000,
        TrustClass::Imported => 2_000,
        TrustClass::UntrustedProposal => 0,
    }
}

const fn scope_score(scope: &MemoryScope) -> i64 {
    match scope {
        MemoryScope::Session(_) => 800,
        MemoryScope::Workspace(_) => 600,
        MemoryScope::User(_) => 400,
        MemoryScope::Agent(_) => 200,
    }
}

const fn sensitivity_score(sensitivity: Sensitivity) -> u8 {
    match sensitivity {
        Sensitivity::Public => 0,
        Sensitivity::Internal => 1,
        Sensitivity::Sensitive => 2,
        Sensitivity::Secret => 3,
    }
}

fn freshness_score(as_of: TimestampMillis, created_at: TimestampMillis) -> i64 {
    let age_ms = as_of.get().saturating_sub(created_at.get());
    const DAY_MS: i64 = 86_400_000;
    match age_ms {
        age if age <= DAY_MS => 300,
        age if age <= DAY_MS * 7 => 200,
        age if age <= DAY_MS * 30 => 100,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use autoharness_domain::{MemoryRevisionStatus, SessionId, Sha256Digest, UserId, WorkspaceId};

    use super::*;

    fn scope() -> RetrievalScope {
        RetrievalScope {
            user_id: UserId::new("user-1").expect("user ID"),
            workspace_id: WorkspaceId::new("workspace-1").expect("workspace ID"),
            session_id: SessionId::new("session-1").expect("session ID"),
            agent_id: None,
            as_of: TimestampMillis::new(10 * 86_400_000),
            sensitivity_ceiling: Sensitivity::Internal,
        }
    }

    fn candidate(id: &str) -> MemoryCandidate {
        MemoryCandidate {
            memory_id: MemoryId::new(format!("memory-{id}")).expect("memory ID"),
            revision_id: MemoryRevisionId::new(format!("revision-{id}")).expect("revision ID"),
            status: MemoryRevisionStatus::Active,
            scope: MemoryScope::Workspace(WorkspaceId::new("workspace-1").expect("workspace ID")),
            kind: MemoryKind::Fact,
            trust: TrustClass::UserApproved,
            confidence: ConfidenceBasisPoints::new(8_000).expect("confidence"),
            sensitivity: Sensitivity::Internal,
            validity: MemoryValidity::Indefinite,
            content: MemoryContent::new(format!("content {id} 雪")).expect("content"),
            content_hash: Sha256Digest::new("a".repeat(64)).expect("content hash"),
            created_at: TimestampMillis::new(9 * 86_400_000),
            exact_match: false,
            lexical_basis_points: 5_000,
            conflicted: false,
        }
    }

    #[test]
    fn equal_scores_use_revision_identity_as_the_stable_tie_break() {
        let ranker = DeterministicRankerV1;
        let first = ranker.rank(&scope(), vec![candidate("z"), candidate("a")]);
        let second = ranker.rank(&scope(), vec![candidate("a"), candidate("z")]);

        let identities = |ranked: &[RankedMemory]| {
            ranked
                .iter()
                .map(|item| item.candidate.revision_id.as_str().to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            identities(&first),
            vec!["revision-a".to_owned(), "revision-z".to_owned()]
        );
        assert_eq!(identities(&first), identities(&second));
    }

    #[test]
    fn lifecycle_scope_trust_conflict_time_and_sensitivity_fail_closed() {
        let mut candidates = vec![candidate("eligible")];
        let mut proposed = candidate("proposed");
        proposed.status = MemoryRevisionStatus::Proposed;
        candidates.push(proposed);
        let mut untrusted = candidate("untrusted");
        untrusted.trust = TrustClass::UntrustedProposal;
        candidates.push(untrusted);
        let mut conflicted = candidate("conflicted");
        conflicted.conflicted = true;
        candidates.push(conflicted);
        let mut wrong_scope = candidate("wrong-scope");
        wrong_scope.scope =
            MemoryScope::Workspace(WorkspaceId::new("workspace-2").expect("workspace ID"));
        candidates.push(wrong_scope);
        let mut expired = candidate("expired");
        expired.validity = MemoryValidity::Until {
            valid_until: scope().as_of,
        };
        candidates.push(expired);
        let mut future = candidate("future");
        future.created_at = TimestampMillis::new(scope().as_of.get() + 1);
        candidates.push(future);
        let mut sensitive = candidate("sensitive");
        sensitive.sensitivity = Sensitivity::Sensitive;
        candidates.push(sensitive);

        let ranked = DeterministicRankerV1.rank(&scope(), candidates);

        assert_eq!(ranked.len(), 1);
        assert_eq!(
            ranked[0].candidate.revision_id.as_str(),
            "revision-eligible"
        );
    }

    #[test]
    fn score_breakdown_is_integer_only_and_sums_to_the_total() {
        let mut exact = candidate("exact");
        exact.exact_match = true;
        exact.lexical_basis_points = 10_000;
        let ranked = DeterministicRankerV1.rank(&scope(), vec![exact]);

        assert_eq!(ranked[0].factors.len(), 6);
        assert_eq!(
            ranked[0].score,
            ranked[0]
                .factors
                .iter()
                .map(|factor| factor.contribution)
                .sum::<i64>()
        );
        assert!(ranked[0].score > 0);
    }
}
