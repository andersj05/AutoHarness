use autoharness_domain::{ModelId, ProviderId};
use autoharness_provider::{
    CapabilitySupport, ModelCapabilities, ModelDescriptor, ProviderError, ProviderErrorKind,
};
use serde::Deserialize;

use crate::GeminiApiKey;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModelsPage {
    #[serde(default)]
    pub(crate) models: Vec<NativeModel>,
    pub(crate) next_page_token: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NativeModel {
    name: Option<String>,
    display_name: Option<String>,
    description: Option<String>,
    input_token_limit: Option<u64>,
    output_token_limit: Option<u64>,
    #[serde(default)]
    supported_generation_methods: Vec<String>,
    thinking: Option<bool>,
}

impl NativeModel {
    pub(crate) fn into_descriptor(
        self,
        provider_id: &ProviderId,
        key: &GeminiApiKey,
    ) -> Option<ModelDescriptor> {
        if !self
            .supported_generation_methods
            .iter()
            .any(|method| method == "generateContent")
        {
            return None;
        }

        let name = self.name?;
        if key.contains(&name) || !is_supported_model_resource(&name) {
            return None;
        }
        let model_id = ModelId::new(name.clone()).ok()?;
        let display_name = self
            .display_name
            .as_deref()
            .map(|value| key.redact(value))
            .unwrap_or_else(|| name.trim_start_matches("models/").to_owned());
        let description = self.description.as_deref().map(|value| key.redact(value));
        let thinking = match self.thinking {
            Some(true) => CapabilitySupport::Supported,
            Some(false) => CapabilitySupport::Unsupported,
            None => CapabilitySupport::Unknown,
        };

        Some(ModelDescriptor {
            provider_id: provider_id.clone(),
            model_id,
            display_name,
            description,
            input_token_limit: self.input_token_limit,
            output_token_limit: self.output_token_limit,
            capabilities: ModelCapabilities {
                chat: CapabilitySupport::Supported,
                // models.list does not document either of these capabilities.
                streaming: CapabilitySupport::Unknown,
                managed_interactions: CapabilitySupport::Unknown,
                thinking,
            },
        })
    }
}

pub(crate) fn request_model_name(model_id: &ModelId) -> Result<&str, ProviderError> {
    let name = model_id.as_str();
    if !is_supported_model_resource(name) {
        return Err(ProviderError::new(
            ProviderErrorKind::InvalidRequest,
            autoharness_domain::RetryAdvice::Never,
        ));
    }
    Ok(name.trim_start_matches("models/"))
}

fn is_supported_model_resource(name: &str) -> bool {
    let Some(short_name) = name.strip_prefix("models/") else {
        return false;
    };
    !short_name.is_empty() && !short_name.contains('/') && short_name != "." && short_name != ".."
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_mapping_is_conservative() {
        let native: NativeModel = serde_json::from_str(
            r#"{
                "name":"models/gemini-test",
                "displayName":"Gemini Test",
                "supportedGenerationMethods":["generateContent"],
                "thinking":true
            }"#,
        )
        .expect("model fixture");
        let provider_id = ProviderId::new("gemini").expect("provider ID");
        let key = GeminiApiKey::new("secret-sentinel").expect("key");
        let mapped = native
            .into_descriptor(&provider_id, &key)
            .expect("compatible model");

        assert_eq!(mapped.capabilities.chat, CapabilitySupport::Supported);
        assert_eq!(mapped.capabilities.streaming, CapabilitySupport::Unknown);
        assert_eq!(
            mapped.capabilities.managed_interactions,
            CapabilitySupport::Unknown
        );
        assert_eq!(mapped.capabilities.thinking, CapabilitySupport::Supported);
    }

    #[test]
    fn non_chat_and_secret_bearing_models_are_excluded() {
        let key = GeminiApiKey::new("secret-sentinel").expect("key");
        let provider_id = ProviderId::new("gemini").expect("provider ID");
        for fixture in [
            r#"{"name":"models/embed","supportedGenerationMethods":["embedContent"]}"#,
            r#"{"name":"models/secret-sentinel","supportedGenerationMethods":["generateContent"]}"#,
        ] {
            let native: NativeModel = serde_json::from_str(fixture).expect("model fixture");
            assert!(native.into_descriptor(&provider_id, &key).is_none());
        }
    }
}
