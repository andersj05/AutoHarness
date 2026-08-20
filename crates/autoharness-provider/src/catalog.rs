use std::fmt::{self, Debug, Formatter};

use autoharness_domain::{ModelId, ProviderId};
use serde::{Deserialize, Serialize};

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
}

impl Default for ModelCapabilities {
    fn default() -> Self {
        Self {
            chat: CapabilitySupport::Unknown,
            streaming: CapabilitySupport::Unknown,
            managed_interactions: CapabilitySupport::Unknown,
            thinking: CapabilitySupport::Unknown,
        }
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
