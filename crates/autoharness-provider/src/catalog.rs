use std::fmt::{self, Debug, Formatter};

use autoharness_domain::{ModelId, ProviderId};
use serde::{Deserialize, Serialize};

/// Whether catalog discovery should prefer a valid cache or contact the provider.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CatalogRequest {
    /// Use a fresh durable catalog when one exists, otherwise refresh it.
    PreferCache,
    /// Contact the provider and use a bounded stale cache only on transient failure.
    Refresh,
}

/// Provenance and freshness of one provider-neutral catalog result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CatalogFreshness {
    /// The result was obtained from the provider during this request.
    Live,
    /// The result came from a cache that is still within its refresh interval.
    Cached,
    /// A transient refresh failure caused a bounded stale-cache fallback.
    Stale,
}

/// One complete provider-neutral model catalog.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelCatalog {
    models: Vec<ModelDescriptor>,
    freshness: CatalogFreshness,
}

impl ModelCatalog {
    /// Constructs a catalog result from validated provider descriptors.
    #[must_use]
    pub const fn new(models: Vec<ModelDescriptor>, freshness: CatalogFreshness) -> Self {
        Self { models, freshness }
    }

    /// Returns the discovered models in stable adapter order.
    #[must_use]
    pub fn models(&self) -> &[ModelDescriptor] {
        &self.models
    }

    /// Consumes the result and returns its descriptors.
    #[must_use]
    pub fn into_models(self) -> Vec<ModelDescriptor> {
        self.models
    }

    /// Returns the result's live, cached, or stale provenance.
    #[must_use]
    pub const fn freshness(&self) -> CatalogFreshness {
        self.freshness
    }

    /// Returns whether this result is a stale fallback after refresh failure.
    #[must_use]
    pub const fn is_stale(&self) -> bool {
        matches!(self.freshness, CatalogFreshness::Stale)
    }
}

/// Process-local provider availability before or after credential admission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderAvailability {
    /// The adapter is configured and can accept catalog or chat requests.
    Ready,
    /// A required provider credential has not been admitted.
    CredentialRequired,
}

/// Whether a provider explicitly reports support for a model capability.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilitySupport {
    /// The provider explicitly reports support.
    Supported,
    /// The provider explicitly reports no support.
    Unsupported,
    /// The discovery protocol does not describe this capability.
    Unknown,
}

/// Provider-neutral capabilities known from model discovery.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ModelCapabilities {
    /// The model accepts conversational content.
    pub chat: CapabilitySupport,
    /// The discovery protocol explicitly describes streaming support.
    pub streaming: CapabilitySupport,
    /// The model supports provider-managed interaction state.
    pub managed_interactions: CapabilitySupport,
    /// The discovery protocol explicitly describes thinking support.
    pub thinking: CapabilitySupport,
    /// The model accepts the adapter's exact custom-function dialect.
    #[serde(default = "unknown_capability")]
    pub tool_calling: CapabilitySupport,
}

const fn unknown_capability() -> CapabilitySupport {
    CapabilitySupport::Unknown
}

impl Default for ModelCapabilities {
    fn default() -> Self {
        Self {
            chat: CapabilitySupport::Unknown,
            streaming: CapabilitySupport::Unknown,
            managed_interactions: CapabilitySupport::Unknown,
            thinking: CapabilitySupport::Unknown,
            tool_calling: CapabilitySupport::Unknown,
        }
    }
}

impl ModelCapabilities {
    /// Returns whether a known unsupported capability forbids streamed chat.
    #[must_use]
    pub const fn supports_streamed_chat(&self) -> bool {
        !matches!(self.chat, CapabilitySupport::Unsupported)
            && !matches!(self.streaming, CapabilitySupport::Unsupported)
    }

    /// Returns whether known model metadata permits custom-function advertisement.
    #[must_use]
    pub const fn supports_tool_calling(&self) -> bool {
        matches!(self.tool_calling, CapabilitySupport::Supported)
    }
}

/// A dynamically discovered model without provider-native response fields.
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
pub struct ModelDescriptor {
    /// Adapter that owns the model.
    pub provider_id: ProviderId,
    /// Provider-owned model name, including any stable resource prefix.
    pub model_id: ModelId,
    /// Human-readable catalog label.
    pub display_name: String,
    /// Human-readable provider description.
    pub description: Option<String>,
    /// Maximum input size reported by the provider.
    pub input_token_limit: Option<u64>,
    /// Maximum output size reported by the provider.
    pub output_token_limit: Option<u64>,
    /// Explicitly mapped provider-neutral capability information.
    pub capabilities: ModelCapabilities,
}

impl Debug for ModelDescriptor {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ModelDescriptor")
            .field("provider_id", &self.provider_id)
            .field("model_id", &self.model_id)
            .field("display_name_bytes", &self.display_name.len())
            .field(
                "description_bytes",
                &self.description.as_ref().map(String::len),
            )
            .field("input_token_limit", &self.input_token_limit)
            .field("output_token_limit", &self.output_token_limit)
            .field("capabilities", &self.capabilities)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_cached_capabilities_default_tool_calling_to_unknown() {
        let capabilities: ModelCapabilities = serde_json::from_value(serde_json::json!({
            "chat":"supported",
            "streaming":"supported",
            "managed_interactions":"unknown",
            "thinking":"unknown"
        }))
        .expect("older capability snapshot");

        assert_eq!(capabilities.tool_calling, CapabilitySupport::Unknown);
        assert!(!capabilities.supports_tool_calling());
    }
}
