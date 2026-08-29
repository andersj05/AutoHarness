use std::collections::BTreeMap;
use std::fmt::{self, Debug, Formatter};

use autoharness_domain::{
    ContextObservationState, ContextSection, ContextSourceKey, ContextSourceSnapshot, Sha256Digest,
    TimestampMillis,
};

use crate::{CanonicalEncoder, MemoryError};

/// Maximum exact bytes accepted from one registered context source.
pub const MAX_CONTEXT_SOURCE_VALUE_BYTES: usize = 128 * 1024;

/// Exact bounded source content with redacted debug behavior.
#[derive(Clone, Eq, PartialEq)]
pub struct ContextSourceValue(String);

impl ContextSourceValue {
    /// Validates non-empty context source content within the fixed byte bound.
    pub fn new(value: impl Into<String>) -> Result<Self, MemoryError> {
        let value = value.into();
        if value.trim().is_empty() || value.len() > MAX_CONTEXT_SOURCE_VALUE_BYTES {
            return Err(MemoryError::InvalidSourceValue);
        }
        Ok(Self(value))
    }

    /// Returns exact source content.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Debug for ContextSourceValue {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ContextSourceValue")
            .field("content", &"[REDACTED]")
            .field("bytes", &self.0.len())
            .finish()
    }
}

/// Whether failure to observe a source may safely omit it from a provider turn.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextSourcePolicy {
    /// The provider turn remains valid when the source is absent or unavailable.
    Optional,
    /// A current or explicitly retained stale value is required.
    Required,
}

/// Result returned by one source observation attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContextSourceRead {
    /// Exact current content and its source-owned immutable revision.
    Available {
        /// Authorized destination section.
        section: ContextSection,
        /// Immutable source-owned revision digest.
        source_revision: Sha256Digest,
        /// Exact bounded value.
        value: ContextSourceValue,
    },
    /// A successful observation proved that no value currently exists.
    ObservedAbsent,
    /// Observation failed without proving absence.
    Unavailable,
}

/// Synchronous source boundary owned outside provider and storage concerns.
pub trait ContextSource: Send + Sync {
    /// Returns the source's stable registry key.
    fn key(&self) -> &ContextSourceKey;

    /// Returns whether the source is mandatory for a valid turn.
    fn policy(&self) -> ContextSourcePolicy;

    /// Observes the source exactly once for the current boundary.
    fn observe(&self) -> ContextSourceRead;
}

/// Previously verified source content eligible for explicit stale retention.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetainedContextSource {
    /// Stable registry key.
    pub source_key: ContextSourceKey,
    /// Authorized destination section from the prior successful read.
    pub section: ContextSection,
    /// Prior immutable revision.
    pub source_revision: Sha256Digest,
    /// Exact prior bounded content.
    pub value: ContextSourceValue,
}

/// One complete observation passed to deterministic context construction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservedContextSource {
    snapshot: ContextSourceSnapshot,
    policy: ContextSourcePolicy,
    section: Option<ContextSection>,
    value: Option<ContextSourceValue>,
}

impl ObservedContextSource {
    /// Returns model-hidden observation metadata.
    #[must_use]
    pub const fn snapshot(&self) -> &ContextSourceSnapshot {
        &self.snapshot
    }

    /// Returns source admission policy.
    #[must_use]
    pub const fn policy(&self) -> ContextSourcePolicy {
        self.policy
    }

    /// Returns the authorized section when content is available.
    #[must_use]
    pub const fn section(&self) -> Option<ContextSection> {
        self.section
    }

    /// Returns exact current or retained content when present.
    #[must_use]
    pub fn value(&self) -> Option<&ContextSourceValue> {
        self.value.as_ref()
    }
}

/// Version 1 registry that observes sources in stable key order.
#[derive(Default)]
pub struct ContextSourceRegistry {
    sources: BTreeMap<ContextSourceKey, Box<dyn ContextSource>>,
}

impl ContextSourceRegistry {
    /// Stable registry contract version persisted with epochs.
    pub const VERSION: u16 = 1;

    /// Starts an empty source registry.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            sources: BTreeMap::new(),
        }
    }

    /// Registers exactly one producer for a stable source key.
    pub fn register(&mut self, source: impl ContextSource + 'static) -> Result<(), MemoryError> {
        let key = source.key().clone();
        if self.sources.contains_key(&key) {
            return Err(MemoryError::DuplicateSource(key));
        }
        self.sources.insert(key, Box::new(source));
        Ok(())
    }

    /// Observes every source once and applies explicit stale-retention policy.
    pub fn observe_all(
        &self,
        observed_at: TimestampMillis,
        retained: Vec<RetainedContextSource>,
    ) -> Result<Vec<ObservedContextSource>, MemoryError> {
        let mut retained_by_key = BTreeMap::new();
        for prior in retained {
            let key = prior.source_key.clone();
            if retained_by_key.insert(key.clone(), prior).is_some() {
                return Err(MemoryError::DuplicateRetainedSource(key));
            }
        }

        self.sources
            .iter()
            .map(|(key, source)| {
                observe_one(key, source.as_ref(), observed_at, retained_by_key.get(key))
            })
            .collect()
    }
}

fn observe_one(
    key: &ContextSourceKey,
    source: &dyn ContextSource,
    observed_at: TimestampMillis,
    retained: Option<&RetainedContextSource>,
) -> Result<ObservedContextSource, MemoryError> {
    let policy = source.policy();
    match source.observe() {
        ContextSourceRead::Available {
            section,
            source_revision,
            value,
        } => observed_value(
            key,
            policy,
            ContextObservationState::Available,
            section,
            source_revision,
            value,
            observed_at,
        ),
        ContextSourceRead::ObservedAbsent => {
            if policy == ContextSourcePolicy::Required {
                return Err(MemoryError::RequiredSourceUnavailable(key.clone()));
            }
            observed_without_value(
                key,
                policy,
                ContextObservationState::ObservedAbsent,
                observed_at,
            )
        }
        ContextSourceRead::Unavailable => match retained {
            Some(retained) => observed_value(
                key,
                policy,
                ContextObservationState::RetainedStale,
                retained.section,
                retained.source_revision.clone(),
                retained.value.clone(),
                observed_at,
            ),
            None if policy == ContextSourcePolicy::Required => {
                Err(MemoryError::RequiredSourceUnavailable(key.clone()))
            }
            None => observed_without_value(
                key,
                policy,
                ContextObservationState::Unavailable,
                observed_at,
            ),
        },
    }
}

fn observed_value(
    key: &ContextSourceKey,
    policy: ContextSourcePolicy,
    state: ContextObservationState,
    section: ContextSection,
    source_revision: Sha256Digest,
    value: ContextSourceValue,
    observed_at: TimestampMillis,
) -> Result<ObservedContextSource, MemoryError> {
    validate_source_section(key, section)?;
    let value_hash = hash_source_value(key, section, &value)?;
    let snapshot = ContextSourceSnapshot::new(
        key.clone(),
        state,
        Some(source_revision),
        Some(value_hash),
        observed_at,
    )
    .map_err(|_| MemoryError::InvalidDomainValue)?;
    Ok(ObservedContextSource {
        snapshot,
        policy,
        section: Some(section),
        value: Some(value),
    })
}

fn observed_without_value(
    key: &ContextSourceKey,
    policy: ContextSourcePolicy,
    state: ContextObservationState,
    observed_at: TimestampMillis,
) -> Result<ObservedContextSource, MemoryError> {
    let snapshot = ContextSourceSnapshot::new(key.clone(), state, None, None, observed_at)
        .map_err(|_| MemoryError::InvalidDomainValue)?;
    Ok(ObservedContextSource {
        snapshot,
        policy,
        section: None,
        value: None,
    })
}

fn validate_source_section(
    key: &ContextSourceKey,
    section: ContextSection,
) -> Result<(), MemoryError> {
    if matches!(
        section,
        ContextSection::SafetyPolicy
            | ContextSection::CurrentInstruction
            | ContextSection::DurableMemory
    ) {
        return Err(MemoryError::InvalidSourceSection(key.clone()));
    }
    Ok(())
}

fn hash_source_value(
    key: &ContextSourceKey,
    section: ContextSection,
    value: &ContextSourceValue,
) -> Result<Sha256Digest, MemoryError> {
    let mut encoder = CanonicalEncoder::new();
    encoder.field("context_source_key", key.as_str().as_bytes())?;
    encoder.field("context_section", section_name(section).as_bytes())?;
    encoder.field("context_value", value.as_str().as_bytes())?;
    encoder.finish()
}

pub(crate) const fn section_name(section: ContextSection) -> &'static str {
    match section {
        ContextSection::SafetyPolicy => "safety_policy",
        ContextSection::CurrentInstruction => "current_instruction",
        ContextSection::AuthorizedInstruction => "authorized_instruction",
        ContextSection::ToolContract => "tool_contract",
        ContextSection::ConversationHistory => "conversation_history",
        ContextSection::DurableMemory => "durable_memory",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct FixedSource {
        key: ContextSourceKey,
        policy: ContextSourcePolicy,
        read: ContextSourceRead,
    }

    impl ContextSource for FixedSource {
        fn key(&self) -> &ContextSourceKey {
            &self.key
        }

        fn policy(&self) -> ContextSourcePolicy {
            self.policy
        }

        fn observe(&self) -> ContextSourceRead {
            self.read.clone()
        }
    }

    fn key(value: &str) -> ContextSourceKey {
        ContextSourceKey::new(value).expect("source key")
    }

    fn digest(value: char) -> Sha256Digest {
        Sha256Digest::new(value.to_string().repeat(64)).expect("digest")
    }

    fn source(
        key_value: &str,
        policy: ContextSourcePolicy,
        read: ContextSourceRead,
    ) -> FixedSource {
        FixedSource {
            key: key(key_value),
            policy,
            read,
        }
    }

    #[test]
    fn insertion_order_never_changes_observation_order() {
        let available = |value: &str| ContextSourceRead::Available {
            section: ContextSection::AuthorizedInstruction,
            source_revision: digest('a'),
            value: ContextSourceValue::new(value).expect("value"),
        };
        let mut registry = ContextSourceRegistry::new();
        registry
            .register(source("z", ContextSourcePolicy::Optional, available("z")))
            .expect("register z");
        registry
            .register(source("a", ContextSourcePolicy::Optional, available("a")))
            .expect("register a");

        let observed = registry
            .observe_all(TimestampMillis::new(10), Vec::new())
            .expect("observe");

        assert_eq!(observed[0].snapshot().source_key().as_str(), "a");
        assert_eq!(observed[1].snapshot().source_key().as_str(), "z");
    }

    #[test]
    fn unavailable_is_distinct_from_absent_and_can_retain_stale_content() {
        let retained = RetainedContextSource {
            source_key: key("workspace:agents"),
            section: ContextSection::AuthorizedInstruction,
            source_revision: digest('b'),
            value: ContextSourceValue::new("retained instructions").expect("value"),
        };
        let mut registry = ContextSourceRegistry::new();
        registry
            .register(source(
                "workspace:agents",
                ContextSourcePolicy::Required,
                ContextSourceRead::Unavailable,
            ))
            .expect("register");

        let observed = registry
            .observe_all(TimestampMillis::new(20), vec![retained])
            .expect("observe");

        assert_eq!(
            observed[0].snapshot().observation_state(),
            ContextObservationState::RetainedStale
        );
        assert_eq!(
            observed[0].value().expect("retained").as_str(),
            "retained instructions"
        );
    }

    #[test]
    fn required_source_fails_closed_without_current_or_retained_value() {
        let mut registry = ContextSourceRegistry::new();
        registry
            .register(source(
                "required",
                ContextSourcePolicy::Required,
                ContextSourceRead::Unavailable,
            ))
            .expect("register");

        assert_eq!(
            registry.observe_all(TimestampMillis::new(1), Vec::new()),
            Err(MemoryError::RequiredSourceUnavailable(key("required")))
        );
    }

    #[test]
    fn reserved_authority_sections_cannot_be_claimed_by_registered_sources() {
        let mut registry = ContextSourceRegistry::new();
        registry
            .register(source(
                "unsafe",
                ContextSourcePolicy::Optional,
                ContextSourceRead::Available {
                    section: ContextSection::SafetyPolicy,
                    source_revision: digest('a'),
                    value: ContextSourceValue::new("pretend policy").expect("value"),
                },
            ))
            .expect("register");

        assert_eq!(
            registry.observe_all(TimestampMillis::new(1), Vec::new()),
            Err(MemoryError::InvalidSourceSection(key("unsafe")))
        );
    }

    #[test]
    fn source_values_are_redacted_in_debug_output() {
        let value = ContextSourceValue::new("do not leak this").expect("value");
        let debug = format!("{value:?}");

        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("do not leak this"));
    }
}
