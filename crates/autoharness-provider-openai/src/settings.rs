use std::env;
use std::fmt::{self, Debug, Formatter};

use autoharness_domain::{ProviderId, RetryAdvice};
use autoharness_provider::{ProviderError, ProviderErrorKind};
use reqwest::Url;
use reqwest::header::HeaderName;
use sha2::{Digest, Sha256};

/// Required base URL for the configured router project.
pub const ROUTER_BASE_URL_ENV: &str = "AUTOHARNESS_ROUTER_BASE_URL";
/// Optional stable project identity used for cache and policy isolation.
pub const ROUTER_PROJECT_ENV: &str = "AUTOHARNESS_ROUTER_PROJECT";
/// Optional authentication header name, defaulting to `Authorization`.
pub const ROUTER_AUTH_HEADER_ENV: &str = "AUTOHARNESS_ROUTER_AUTH_HEADER";
/// Optional authentication value scheme, defaulting to `Bearer`.
pub const ROUTER_AUTH_SCHEME_ENV: &str = "AUTOHARNESS_ROUTER_AUTH_SCHEME";
/// Optional relative model-discovery path, defaulting to `v1/models`.
pub const ROUTER_MODELS_PATH_ENV: &str = "AUTOHARNESS_ROUTER_MODELS_PATH";
/// Optional relative streamed-chat path, defaulting to `v1/chat/completions`.
pub const ROUTER_CHAT_PATH_ENV: &str = "AUTOHARNESS_ROUTER_CHAT_PATH";

const DEFAULT_AUTH_HEADER: &str = "authorization";
const DEFAULT_AUTH_SCHEME: &str = "Bearer";
const DEFAULT_MODELS_PATH: &str = "v1/models";
const DEFAULT_CHAT_PATH: &str = "v1/chat/completions";
const MAX_SCHEME_BYTES: usize = 128;
const MAX_PATH_BYTES: usize = 2048;

/// Validated non-secret configuration for one OpenAI-compatible router project.
#[derive(Clone)]
pub struct RouterSettings {
    base_url: Url,
    provider_id: ProviderId,
    auth_header: HeaderName,
    auth_scheme: String,
    models_path: String,
    chat_path: String,
}

impl RouterSettings {
    /// Constructs settings with standard OpenAI-compatible paths and bearer authentication.
    pub fn new(base_url: Url, project: Option<&str>) -> Result<Self, ProviderError> {
        validate_base_url(&base_url)?;
        let project = project.map_or_else(|| fingerprint(&base_url), str::to_owned);
        let provider_id =
            ProviderId::new(format!("router:{project}")).map_err(|_| invalid_configuration())?;
        Ok(Self {
            base_url,
            provider_id,
            auth_header: HeaderName::from_static(DEFAULT_AUTH_HEADER),
            auth_scheme: DEFAULT_AUTH_SCHEME.to_owned(),
            models_path: DEFAULT_MODELS_PATH.to_owned(),
            chat_path: DEFAULT_CHAT_PATH.to_owned(),
        })
    }

    /// Reads validated non-secret router settings from environment variables.
    pub fn from_env() -> Result<Self, ProviderError> {
        let base_url = env::var(ROUTER_BASE_URL_ENV)
            .map_err(|_| invalid_configuration())?
            .parse::<Url>()
            .map_err(|_| invalid_configuration())?;
        let project = env::var(ROUTER_PROJECT_ENV).ok();
        let mut settings = Self::new(base_url, project.as_deref())?;
        if let Ok(header) = env::var(ROUTER_AUTH_HEADER_ENV) {
            let scheme =
                env::var(ROUTER_AUTH_SCHEME_ENV).unwrap_or_else(|_| DEFAULT_AUTH_SCHEME.to_owned());
            settings = settings.with_authentication(&header, &scheme)?;
        } else if let Ok(scheme) = env::var(ROUTER_AUTH_SCHEME_ENV) {
            settings = settings.with_authentication(DEFAULT_AUTH_HEADER, &scheme)?;
        }
        let models =
            env::var(ROUTER_MODELS_PATH_ENV).unwrap_or_else(|_| DEFAULT_MODELS_PATH.to_owned());
        let chat = env::var(ROUTER_CHAT_PATH_ENV).unwrap_or_else(|_| DEFAULT_CHAT_PATH.to_owned());
        settings.with_paths(&models, &chat)
    }

    /// Replaces the sensitive authentication header name and value scheme.
    pub fn with_authentication(
        mut self,
        header: &str,
        scheme: &str,
    ) -> Result<Self, ProviderError> {
        let header =
            HeaderName::from_bytes(header.as_bytes()).map_err(|_| invalid_configuration())?;
        if is_forbidden_auth_header(&header)
            || scheme.len() > MAX_SCHEME_BYTES
            || !scheme.is_ascii()
            || scheme.chars().any(char::is_control)
        {
            return Err(invalid_configuration());
        }
        self.auth_header = header;
        self.auth_scheme = scheme.trim().to_owned();
        Ok(self)
    }

    /// Replaces relative model-discovery and streamed-chat paths.
    pub fn with_paths(mut self, models: &str, chat: &str) -> Result<Self, ProviderError> {
        validate_path(models)?;
        validate_path(chat)?;
        self.models_path = models.to_owned();
        self.chat_path = chat.to_owned();
        Ok(self)
    }

    /// Returns the durable provider-project identity.
    #[must_use]
    pub const fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }

    pub(crate) fn auth_header(&self) -> &HeaderName {
        &self.auth_header
    }

    pub(crate) fn auth_scheme(&self) -> &str {
        &self.auth_scheme
    }

    pub(crate) fn models_endpoint(&self) -> Result<Url, ProviderError> {
        self.endpoint(&self.models_path)
    }

    pub(crate) fn chat_endpoint(&self) -> Result<Url, ProviderError> {
        self.endpoint(&self.chat_path)
    }

    fn endpoint(&self, path: &str) -> Result<Url, ProviderError> {
        let url = self
            .base_url
            .join(path)
            .map_err(|_| invalid_configuration())?;
        if url.origin() != self.base_url.origin()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(invalid_configuration());
        }
        Ok(url)
    }
}

impl Debug for RouterSettings {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RouterSettings")
            .field("provider_id", &self.provider_id)
            .field("auth_header", &self.auth_header)
            .field("auth_scheme", &"[CONFIGURED]")
            .field("models_path_bytes", &self.models_path.len())
            .field("chat_path_bytes", &self.chat_path.len())
            .finish_non_exhaustive()
    }
}

fn validate_base_url(url: &Url) -> Result<(), ProviderError> {
    if !matches!(url.scheme(), "https" | "http")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.path().ends_with('/')
    {
        return Err(invalid_configuration());
    }
    if url.scheme() == "http" && !is_loopback_host(url.host_str()) {
        return Err(invalid_configuration());
    }
    Ok(())
}

fn is_loopback_host(host: Option<&str>) -> bool {
    host.is_some_and(|host| {
        host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|address| address.is_loopback())
    })
}

fn validate_path(path: &str) -> Result<(), ProviderError> {
    if path.is_empty()
        || path.len() > MAX_PATH_BYTES
        || path.starts_with('/')
        || path.contains(['?', '#'])
        || path
            .split('/')
            .any(|segment| matches!(segment, "" | "." | ".."))
        || path.chars().any(char::is_control)
        || path.chars().any(char::is_whitespace)
    {
        return Err(invalid_configuration());
    }
    Ok(())
}

fn is_forbidden_auth_header(header: &HeaderName) -> bool {
    matches!(
        header.as_str(),
        "accept" | "connection" | "content-length" | "content-type" | "host" | "transfer-encoding"
    )
}

fn fingerprint(url: &Url) -> String {
    let digest = Sha256::digest(url.as_str().as_bytes());
    digest[..12]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn invalid_configuration() -> ProviderError {
    ProviderError::new(ProviderErrorKind::InvalidRequest, RetryAdvice::Never)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_reject_credential_urls_absolute_paths_and_transport_headers() {
        for url in [
            "https://user:secret@example.test/",
            "https://example.test/?token=secret",
            "https://example.test/v1",
            "http://example.test/",
        ] {
            assert!(RouterSettings::new(Url::parse(url).expect("URL"), None).is_err());
        }
        let settings = RouterSettings::new(
            Url::parse("https://example.test/base/").expect("URL"),
            Some("project-a"),
        )
        .expect("settings");
        assert!(settings.clone().with_paths("/models", "chat").is_err());
        assert!(
            settings
                .with_authentication("Content-Length", "Bearer")
                .is_err()
        );
    }

    #[test]
    fn provider_identity_is_project_scoped_and_debug_excludes_origin() {
        let settings = RouterSettings::new(
            Url::parse("https://private.example.test/api/").expect("URL"),
            Some("project-a"),
        )
        .expect("settings");

        assert_eq!(settings.provider_id().as_str(), "router:project-a");
        assert!(!format!("{settings:?}").contains("private.example.test"));
        assert!(!format!("{settings:?}").contains("Bearer"));
    }
}
