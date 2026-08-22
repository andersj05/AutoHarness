use autoharness_domain::{ModelId, ProviderId};
use autoharness_provider::{
    CapabilitySupport, ModelCapabilities, ModelDescriptor, ProviderError, ProviderErrorKind,
};
use serde::Deserialize;

use crate::RouterCredential;

#[derive(Deserialize)]
pub(crate) struct ModelsPage {
    #[serde(default)]
    pub(crate) data: Vec<NativeModel>,
    #[serde(default)]
    pub(crate) has_more: bool,
    pub(crate) last_id: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct NativeModel {
    id: Option<String>,
    name: Option<String>,
    description: Option<String>,
    context_window: Option<u64>,
    max_output_tokens: Option<u64>,
    capabilities: Option<NativeCapabilities>,
}

#[derive(Deserialize)]
pub(crate) struct NativeCapabilities {
    chat: Option<bool>,
    streaming: Option<bool>,
    thinking: Option<bool>,
    tools: Option<bool>,
    function_calling: Option<bool>,
}

impl NativeModel {
    pub(crate) fn cursor(&self) -> Option<&str> {
        self.id.as_deref()
    }

    pub(crate) fn into_descriptor(
        self,
        provider_id: &ProviderId,
        credential: &RouterCredential,
    ) -> Option<ModelDescriptor> {
        let id = self.id?;
        if credential.contains(&id) {
            return None;
        }
        let model_id = ModelId::new(id.clone()).ok()?;
        let display_name = self
            .name
            .as_deref()
            .map(|value| credential.redact(value))
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(id);
        let capabilities = self.capabilities.map_or(
            ModelCapabilities {
                managed_interactions: CapabilitySupport::Unsupported,
                ..ModelCapabilities::default()
            },
            |value| ModelCapabilities {
                chat: support(value.chat),
                streaming: support(value.streaming),
                managed_interactions: CapabilitySupport::Unsupported,
                thinking: support(value.thinking),
                tool_calling: support(value.function_calling.or(value.tools)),
            },
        );
        Some(ModelDescriptor {
            provider_id: provider_id.clone(),
            model_id,
            display_name,
            description: self
                .description
                .as_deref()
                .map(|value| credential.redact(value)),
            input_token_limit: self.context_window,
            output_token_limit: self.max_output_tokens,
            capabilities,
        })
    }
}

pub(crate) fn request_model_name<'a>(
    model_id: &'a ModelId,
    credential: &RouterCredential,
) -> Result<&'a str, ProviderError> {
    if credential.contains(model_id.as_str()) {
        return Err(ProviderError::new(
            ProviderErrorKind::InvalidRequest,
            autoharness_domain::RetryAdvice::Never,
        ));
    }
    Ok(model_id.as_str())
}

const fn support(value: Option<bool>) -> CapabilitySupport {
    match value {
        Some(true) => CapabilitySupport::Supported,
        Some(false) => CapabilitySupport::Unsupported,
        None => CapabilitySupport::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_extensions_map_conservatively() {
        let native: NativeModel = serde_json::from_str(
            r#"{
                "id":"router-model",
                "capabilities":{"chat":true,"streaming":false}
            }"#,
        )
        .expect("model");
        let descriptor = native
            .into_descriptor(
                &ProviderId::new("router:test").expect("provider"),
                &RouterCredential::new("secret").expect("credential"),
            )
            .expect("descriptor");

        assert_eq!(descriptor.capabilities.chat, CapabilitySupport::Supported);
        assert_eq!(
            descriptor.capabilities.streaming,
            CapabilitySupport::Unsupported
        );
        assert_eq!(
            descriptor.capabilities.managed_interactions,
            CapabilitySupport::Unsupported
        );
        assert_eq!(descriptor.capabilities.thinking, CapabilitySupport::Unknown);
        assert_eq!(
            descriptor.capabilities.tool_calling,
            CapabilitySupport::Unknown
        );
    }
}
