use autoharness_domain::{MemoryRevisionStatus, Sha256Digest};

use crate::{
    CanonicalEncoder, MemoryCandidate, MemoryError, RetrievalScope, normalized_content_hash,
};

/// Stable canonical contract for effective durable facts across compaction epochs.
pub const COMPACTION_FACTS_VERSION: u16 = 1;

/// Unsettled session state that compaction must never summarize away.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum PendingSessionFactKind {
    /// An admitted input has not reached a final disposition.
    Input,
    /// A tool permission decision remains relevant to execution.
    Permission,
    /// A tool call has not reached an authoritative settled state.
    Tool,
}

/// Contentless hashes of one authoritative unsettled session fact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingSessionFact {
    kind: PendingSessionFactKind,
    identity_hash: Sha256Digest,
    state_hash: Sha256Digest,
}

impl PendingSessionFact {
    /// Constructs one contentless fact from independently computed authoritative hashes.
    #[must_use]
    pub const fn new(
        kind: PendingSessionFactKind,
        identity_hash: Sha256Digest,
        state_hash: Sha256Digest,
    ) -> Self {
        Self {
            kind,
            identity_hash,
            state_hash,
        }
    }

    /// Returns the stable session-fact class.
    #[must_use]
    pub const fn kind(&self) -> PendingSessionFactKind {
        self.kind
    }

    /// Returns the hash of the fact's authoritative stable identity.
    #[must_use]
    pub const fn identity_hash(&self) -> &Sha256Digest {
        &self.identity_hash
    }

    /// Returns the hash of the complete authoritative state that must survive compaction.
    #[must_use]
    pub const fn state_hash(&self) -> &Sha256Digest {
        &self.state_hash
    }
}

/// Hashes every effective memory fact and unsettled session fact in stable logical order.
///
/// Retrieval-only signals such as lexical score and exact-query match are intentionally excluded.
/// The supplied retrieval scope determines eligibility at the exact compaction boundary.
pub fn effective_durable_facts_hash(
    scope: &RetrievalScope,
    candidates: &[MemoryCandidate],
    pending_session_facts: &[PendingSessionFact],
) -> Result<Sha256Digest, MemoryError> {
    let mut memories = candidates
        .iter()
        .filter(|candidate| crate::rank::eligible(scope, candidate))
        .collect::<Vec<_>>();
    memories.sort_by(|left, right| {
        (&left.memory_id, &left.revision_id).cmp(&(&right.memory_id, &right.revision_id))
    });
    if let Some(pair) = memories
        .windows(2)
        .find(|pair| pair[0].revision_id == pair[1].revision_id)
    {
        return Err(MemoryError::DuplicateMemoryRevision(
            pair[0].revision_id.clone(),
        ));
    }

    let mut pending = pending_session_facts.iter().collect::<Vec<_>>();
    pending.sort_by(|left, right| {
        (left.kind, &left.identity_hash, &left.state_hash).cmp(&(
            right.kind,
            &right.identity_hash,
            &right.state_hash,
        ))
    });
    if pending
        .windows(2)
        .any(|pair| pair[0].kind == pair[1].kind && pair[0].identity_hash == pair[1].identity_hash)
    {
        return Err(MemoryError::DuplicateCompactionFact);
    }

    let mut encoder = CanonicalEncoder::new();
    encoder.integer(
        "compaction_facts_version",
        u64::from(COMPACTION_FACTS_VERSION),
    )?;
    encoder.integer(
        "memory_fact_count",
        u64::try_from(memories.len()).map_err(|_| MemoryError::NumericOverflow)?,
    )?;
    for memory in memories {
        if memory.status != MemoryRevisionStatus::Active
            || normalized_content_hash(memory.content.as_str())? != memory.content_hash
        {
            return Err(MemoryError::InvalidCompactionFact);
        }
        encoder.field("memory_id", memory.memory_id.as_str().as_bytes())?;
        encoder.field("revision_id", memory.revision_id.as_str().as_bytes())?;
        encoder.field(
            "scope",
            &serde_json::to_vec(&memory.scope).map_err(|_| MemoryError::InvalidDomainValue)?,
        )?;
        encoder.field(
            "kind",
            &serde_json::to_vec(&memory.kind).map_err(|_| MemoryError::InvalidDomainValue)?,
        )?;
        encoder.field(
            "trust",
            &serde_json::to_vec(&memory.trust).map_err(|_| MemoryError::InvalidDomainValue)?,
        )?;
        encoder.integer("confidence", u64::from(memory.confidence.get()))?;
        encoder.field(
            "sensitivity",
            &serde_json::to_vec(&memory.sensitivity)
                .map_err(|_| MemoryError::InvalidDomainValue)?,
        )?;
        encoder.field(
            "validity",
            &serde_json::to_vec(&memory.validity).map_err(|_| MemoryError::InvalidDomainValue)?,
        )?;
        encoder.field("content_hash", memory.content_hash.as_str().as_bytes())?;
        encoder.field("created_at", &memory.created_at.get().to_be_bytes())?;
    }

    encoder.integer(
        "pending_session_fact_count",
        u64::try_from(pending.len()).map_err(|_| MemoryError::NumericOverflow)?,
    )?;
    for fact in pending {
        encoder.integer("pending_kind", pending_kind_code(fact.kind))?;
        encoder.field(
            "pending_identity_hash",
            fact.identity_hash.as_str().as_bytes(),
        )?;
        encoder.field("pending_state_hash", fact.state_hash.as_str().as_bytes())?;
    }
    encoder.finish()
}

/// Verifies a persisted compaction fingerprint against authoritative current facts.
pub fn verify_effective_durable_facts_hash(
    scope: &RetrievalScope,
    candidates: &[MemoryCandidate],
    pending_session_facts: &[PendingSessionFact],
    expected: &Sha256Digest,
) -> Result<bool, MemoryError> {
    Ok(effective_durable_facts_hash(scope, candidates, pending_session_facts)? == *expected)
}

const fn pending_kind_code(kind: PendingSessionFactKind) -> u64 {
    match kind {
        PendingSessionFactKind::Input => 1,
        PendingSessionFactKind::Permission => 2,
        PendingSessionFactKind::Tool => 3,
    }
}

#[cfg(test)]
mod tests {
    use autoharness_domain::{
        ConfidenceBasisPoints, MemoryContent, MemoryId, MemoryKind, MemoryRevisionId, MemoryScope,
        MemoryValidity, Sensitivity, SessionId, TimestampMillis, TrustClass, UserId, WorkspaceId,
    };

    use super::*;

    fn digest(character: char) -> Sha256Digest {
        Sha256Digest::new(character.to_string().repeat(64)).expect("digest")
    }

    fn scope() -> RetrievalScope {
        RetrievalScope {
            user_id: UserId::new("user-1").expect("user ID"),
            workspace_id: WorkspaceId::new("workspace-1").expect("workspace ID"),
            session_id: SessionId::new("session-1").expect("session ID"),
            agent_id: None,
            as_of: TimestampMillis::new(20),
            sensitivity_ceiling: Sensitivity::Internal,
        }
    }

    fn candidate(id: &str, content: &str) -> MemoryCandidate {
        MemoryCandidate {
            memory_id: MemoryId::new(format!("memory-{id}")).expect("memory ID"),
            revision_id: MemoryRevisionId::new(format!("revision-{id}")).expect("revision ID"),
            status: MemoryRevisionStatus::Active,
            scope: MemoryScope::Workspace(WorkspaceId::new("workspace-1").expect("workspace ID")),
            kind: MemoryKind::Fact,
            trust: TrustClass::UserApproved,
            confidence: ConfidenceBasisPoints::new(9_000).expect("confidence"),
            sensitivity: Sensitivity::Internal,
            validity: MemoryValidity::Indefinite,
            content: MemoryContent::new(content).expect("content"),
            content_hash: normalized_content_hash(content).expect("content hash"),
            created_at: TimestampMillis::new(10),
            exact_match: false,
            lexical_basis_points: 0,
            conflicted: false,
        }
    }

    #[test]
    fn shuffled_physical_order_has_one_effective_facts_hash() {
        let left = candidate("left", "left durable fact");
        let right = candidate("right", "right durable fact");
        let pending_input =
            PendingSessionFact::new(PendingSessionFactKind::Input, digest('a'), digest('b'));
        let pending_tool =
            PendingSessionFact::new(PendingSessionFactKind::Tool, digest('c'), digest('d'));

        let first = effective_durable_facts_hash(
            &scope(),
            &[right.clone(), left.clone()],
            &[pending_tool.clone(), pending_input.clone()],
        )
        .expect("first hash");
        let second =
            effective_durable_facts_hash(&scope(), &[left, right], &[pending_input, pending_tool])
                .expect("second hash");

        assert_eq!(first, second);
    }

    #[test]
    fn retrieval_scores_do_not_change_the_durable_fact_set() {
        let original = candidate("stable", "stable durable fact");
        let mut reranked = original.clone();
        reranked.exact_match = true;
        reranked.lexical_basis_points = 10_000;

        assert_eq!(
            effective_durable_facts_hash(&scope(), &[original], &[]).expect("original"),
            effective_durable_facts_hash(&scope(), &[reranked], &[]).expect("reranked")
        );
    }

    #[test]
    fn changed_memory_or_pending_state_changes_the_hash() {
        let original = candidate("stable", "first durable fact");
        let changed = candidate("stable", "changed durable fact");
        let pending =
            PendingSessionFact::new(PendingSessionFactKind::Permission, digest('e'), digest('f'));
        let changed_pending =
            PendingSessionFact::new(PendingSessionFactKind::Permission, digest('e'), digest('0'));

        let baseline = effective_durable_facts_hash(
            &scope(),
            std::slice::from_ref(&original),
            std::slice::from_ref(&pending),
        )
        .expect("baseline");
        assert_ne!(
            baseline,
            effective_durable_facts_hash(&scope(), &[changed], &[pending]).expect("changed memory")
        );
        assert_ne!(
            baseline,
            effective_durable_facts_hash(&scope(), &[original], &[changed_pending])
                .expect("changed pending state")
        );
    }

    #[test]
    fn ineligible_memory_is_not_an_effective_durable_fact() {
        let mut proposed = candidate("proposal", "untrusted candidate");
        proposed.status = MemoryRevisionStatus::Proposed;
        proposed.trust = TrustClass::UntrustedProposal;

        assert_eq!(
            effective_durable_facts_hash(&scope(), &[proposed], &[]).expect("filtered"),
            effective_durable_facts_hash(&scope(), &[], &[]).expect("empty")
        );
    }

    #[test]
    fn malformed_or_duplicate_facts_fail_closed() {
        let mut malformed = candidate("bad", "actual bytes");
        malformed.content_hash = digest('1');
        assert_eq!(
            effective_durable_facts_hash(&scope(), &[malformed], &[]),
            Err(MemoryError::InvalidCompactionFact)
        );

        let fact = PendingSessionFact::new(PendingSessionFactKind::Tool, digest('2'), digest('3'));
        let conflicting =
            PendingSessionFact::new(PendingSessionFactKind::Tool, digest('2'), digest('4'));
        assert_eq!(
            effective_durable_facts_hash(&scope(), &[], &[fact, conflicting]),
            Err(MemoryError::DuplicateCompactionFact)
        );
    }
}
