use std::fmt::{self, Debug, Formatter};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use autoharness_domain::RetryAdvice;
use autoharness_provider::{CancellationToken, ProviderError, ProviderErrorKind};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use reqwest::Client;
use reqwest::redirect::Policy;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::{TcpListener, TcpStream};
use url::Url;
use uuid::Uuid;
use zeroize::{Zeroize as _, Zeroizing};

const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const AUTHORIZE_URL: &str = "https://auth.openai.com/oauth/authorize";
const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
const CALLBACK_ADDRESS: &str = "127.0.0.1:1455";
const CALLBACK_PATH: &str = "/auth/callback";
const SCOPE: &str = "openid profile email offline_access api.connectors.read api.connectors.invoke";
const LOGIN_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const HTTP_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_CALLBACK_BYTES: usize = 16 * 1024;
const MAX_CALLBACK_CONNECTIONS: usize = 8;
const CREDENTIAL_SCHEMA: u8 = 1;
const JWT_AUTH_CLAIM: &str = "https://api.openai.com/auth";

/// Safe progress from the native Codex authentication flow.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodexAuthProgress {
    /// The authorization URL was handed to the operating system browser.
    BrowserOpened,
}

/// OAuth credential stored as one opaque operating-system vault entry.
#[derive(Serialize, Deserialize)]
pub struct CodexOAuthCredential {
    schema: u8,
    access_token: String,
    refresh_token: String,
    expires_at_unix_ms: u64,
    account_id: String,
}

impl CodexOAuthCredential {
    /// Parses and validates one vault payload without exposing its contents.
    pub fn decode(encoded: &str) -> Result<Self, ProviderError> {
        let credential: Self = serde_json::from_str(encoded).map_err(|_| authentication_error())?;
        if credential.schema != CREDENTIAL_SCHEMA
            || credential.access_token.is_empty()
            || credential.refresh_token.is_empty()
            || credential.account_id.is_empty()
            || credential.access_token.len() > 16 * 1024
            || credential.refresh_token.len() > 16 * 1024
            || credential.account_id.len() > 512
            || credential.account_id.chars().any(char::is_control)
        {
            return Err(authentication_error());
        }
        Ok(credential)
    }

    /// Serializes one credential for storage in the operating-system vault.
    pub fn encode(&self) -> Result<Zeroizing<String>, ProviderError> {
        serde_json::to_string(self)
            .map(Zeroizing::new)
            .map_err(|_| internal_error())
    }

    pub(crate) fn access_token(&self) -> &str {
        &self.access_token
    }

    pub(crate) fn refresh_token(&self) -> &str {
        &self.refresh_token
    }

    pub(crate) fn account_id(&self) -> &str {
        &self.account_id
    }

    pub(crate) fn expires_soon(&self) -> bool {
        self.expires_at_unix_ms
            <= unix_time_ms().saturating_add(Duration::from_secs(60).as_millis() as u64)
    }
}

impl Debug for CodexOAuthCredential {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CodexOAuthCredential")
            .field("credential", &"[REDACTED]")
            .finish()
    }
}

impl Drop for CodexOAuthCredential {
    fn drop(&mut self) {
        self.access_token.zeroize();
        self.refresh_token.zeroize();
        self.account_id.zeroize();
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    expires_in: u64,
    id_token: Option<String>,
}

/// Runs the native browser flow without requiring an installed Codex CLI.
pub async fn login_with_browser<F>(
    cancellation: CancellationToken,
    progress: F,
) -> Result<Zeroizing<String>, ProviderError>
where
    F: Fn(CodexAuthProgress) + Send + 'static,
{
    if cancellation.is_cancelled() {
        return Err(cancelled_error());
    }
    let listener = TcpListener::bind(CALLBACK_ADDRESS)
        .await
        .map_err(|_| unavailable_error())?;
    let verifier = pkce_verifier();
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let state = random_urlsafe_value();
    let authorization_url = authorization_url(&state, &challenge)?;
    open_browser(authorization_url.as_str()).map_err(|_| unavailable_error())?;
    progress(CodexAuthProgress::BrowserOpened);

    let code = tokio::select! {
        biased;
        () = cancellation.cancelled() => return Err(cancelled_error()),
        result = tokio::time::timeout(
            LOGIN_TIMEOUT,
            receive_callback(&listener, &state, &cancellation),
        ) => result.map_err(|_| timeout_error())??,
    };
    let credential = exchange_authorization_code(&code, &verifier, REDIRECT_URI).await?;
    credential.encode()
}

pub(crate) async fn refresh_credential(
    credential: &CodexOAuthCredential,
) -> Result<CodexOAuthCredential, ProviderError> {
    let client = oauth_client()?;
    let response = client
        .post(TOKEN_URL)
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", credential.refresh_token()),
            ("client_id", CLIENT_ID),
        ])
        .send()
        .await
        .map_err(classify_transport_error)?;
    token_response(response, Some(credential.account_id())).await
}

async fn exchange_authorization_code(
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<CodexOAuthCredential, ProviderError> {
    let client = oauth_client()?;
    let response = client
        .post(TOKEN_URL)
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", CLIENT_ID),
            ("code", code),
            ("code_verifier", verifier),
            ("redirect_uri", redirect_uri),
        ])
        .send()
        .await
        .map_err(classify_transport_error)?;
    token_response(response, None).await
}

async fn token_response(
    response: reqwest::Response,
    existing_account_id: Option<&str>,
) -> Result<CodexOAuthCredential, ProviderError> {
    if !response.status().is_success() {
        return Err(authentication_error().with_http_status(response.status().as_u16()));
    }
    let token: TokenResponse = response.json().await.map_err(|_| protocol_error())?;
    if token.access_token.is_empty()
        || token.refresh_token.is_empty()
        || token.expires_in == 0
        || token.expires_in > 31 * 24 * 60 * 60
    {
        return Err(protocol_error());
    }
    let account_id = extract_account_id(&token.access_token)
        .or_else(|| token.id_token.as_deref().and_then(extract_account_id))
        .or_else(|| existing_account_id.map(str::to_owned))
        .ok_or_else(authentication_error)?;
    Ok(CodexOAuthCredential {
        schema: CREDENTIAL_SCHEMA,
        access_token: token.access_token,
        refresh_token: token.refresh_token,
        expires_at_unix_ms: unix_time_ms().saturating_add(token.expires_in.saturating_mul(1_000)),
        account_id,
    })
}

fn oauth_client() -> Result<Client, ProviderError> {
    Client::builder()
        .redirect(Policy::none())
        .timeout(HTTP_TIMEOUT)
        .build()
        .map_err(|_| internal_error())
}

fn authorization_url(state: &str, challenge: &str) -> Result<Url, ProviderError> {
    let mut url = Url::parse(AUTHORIZE_URL).map_err(|_| internal_error())?;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", CLIENT_ID)
        .append_pair("redirect_uri", REDIRECT_URI)
        .append_pair("scope", SCOPE)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", state)
        .append_pair("id_token_add_organizations", "true")
        .append_pair("codex_cli_simplified_flow", "true")
        .append_pair("originator", "codex_cli_rs");
    Ok(url)
}

async fn receive_callback(
    listener: &TcpListener,
    expected_state: &str,
    cancellation: &CancellationToken,
) -> Result<String, ProviderError> {
    for _ in 0..MAX_CALLBACK_CONNECTIONS {
        let (mut stream, _) = tokio::select! {
            biased;
            () = cancellation.cancelled() => return Err(cancelled_error()),
            accepted = listener.accept() => accepted.map_err(|_| transport_error())?,
        };
        match read_callback(&mut stream, expected_state).await {
            Ok(code) => {
                write_callback_response(&mut stream, true).await;
                return Ok(code);
            }
            Err(error) => {
                write_callback_response(&mut stream, false).await;
                if error.kind() != ProviderErrorKind::InvalidRequest {
                    return Err(error);
                }
            }
        }
    }
    Err(protocol_error())
}

async fn read_callback(
    stream: &mut TcpStream,
    expected_state: &str,
) -> Result<String, ProviderError> {
    let mut request = Vec::new();
    let mut chunk = [0_u8; 1024];
    loop {
        let read = stream
            .read(&mut chunk)
            .await
            .map_err(|_| transport_error())?;
        if read == 0 {
            return Err(protocol_error());
        }
        if request.len().saturating_add(read) > MAX_CALLBACK_BYTES {
            return Err(limit_error());
        }
        request.extend_from_slice(&chunk[..read]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    let request = std::str::from_utf8(&request).map_err(|_| protocol_error())?;
    parse_callback_request(request, expected_state)
}

fn parse_callback_request(request: &str, expected_state: &str) -> Result<String, ProviderError> {
    let mut fields = request
        .lines()
        .next()
        .ok_or_else(protocol_error)?
        .split_whitespace();
    if fields.next() != Some("GET") {
        return Err(ProviderError::new(
            ProviderErrorKind::InvalidRequest,
            RetryAdvice::Never,
        ));
    }
    let target = fields.next().ok_or_else(protocol_error)?;
    let url = Url::parse(&format!("http://localhost{target}")).map_err(|_| protocol_error())?;
    if url.path() != CALLBACK_PATH {
        return Err(ProviderError::new(
            ProviderErrorKind::InvalidRequest,
            RetryAdvice::Never,
        ));
    }
    let mut code = None;
    let mut state = None;
    let mut oauth_error = false;
    for (name, value) in url.query_pairs() {
        match name.as_ref() {
            "code" => code = Some(value.into_owned()),
            "state" => state = Some(value.into_owned()),
            "error" => oauth_error = true,
            _ => {}
        }
    }
    if oauth_error || state.as_deref() != Some(expected_state) {
        return Err(authentication_error());
    }
    let code = code.ok_or_else(authentication_error)?;
    if code.is_empty() || code.len() > 16 * 1024 || code.chars().any(char::is_control) {
        return Err(authentication_error());
    }
    Ok(code)
}

async fn write_callback_response(stream: &mut TcpStream, success: bool) {
    let (status, title, message) = if success {
        (
            "200 OK",
            "Connected to AutoHarness",
            "Authentication was received. You can close this tab and return to AutoHarness.",
        )
    } else {
        (
            "400 Bad Request",
            "Authentication could not be completed",
            "Return to AutoHarness and try signing in again.",
        )
    };
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{title}</title></head><body><main><h1>{title}</h1><p>{message}</p></main></body></html>"
    );
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}

fn open_browser(url: &str) -> std::io::Result<()> {
    let (program, arguments) = browser_command(url);
    Command::new(program).args(arguments).spawn().map(|_| ())
}

#[cfg(target_os = "windows")]
fn browser_command(url: &str) -> (&'static str, Vec<&str>) {
    ("rundll32.exe", vec!["url.dll,FileProtocolHandler", url])
}

#[cfg(target_os = "macos")]
fn browser_command(url: &str) -> (&'static str, Vec<&str>) {
    ("open", vec![url])
}

#[cfg(all(unix, not(target_os = "macos")))]
fn browser_command(url: &str) -> (&'static str, Vec<&str>) {
    ("xdg-open", vec![url])
}

fn pkce_verifier() -> String {
    random_urlsafe_value()
}

fn random_urlsafe_value() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

fn extract_account_id(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let decoded = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let claims: Value = serde_json::from_slice(&decoded).ok()?;
    claims
        .get(JWT_AUTH_CLAIM)?
        .get("chatgpt_account_id")?
        .as_str()
        .filter(|value| !value.is_empty() && value.len() <= 512)
        .map(str::to_owned)
}

pub(crate) fn extract_residency(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let decoded = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let claims: Value = serde_json::from_slice(&decoded).ok()?;
    let auth = claims.get(JWT_AUTH_CLAIM)?;
    ["chatgpt_data_residency", "chatgpt_compute_residency"]
        .into_iter()
        .find_map(|claim| {
            auth.get(claim)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty() && value.len() <= 128)
                .map(str::to_owned)
        })
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}

fn classify_transport_error(error: reqwest::Error) -> ProviderError {
    if error.is_timeout() {
        timeout_error()
    } else if error.is_connect() {
        unavailable_error()
    } else {
        transport_error()
    }
}

fn authentication_error() -> ProviderError {
    ProviderError::new(ProviderErrorKind::Authentication, RetryAdvice::Never)
}

fn cancelled_error() -> ProviderError {
    ProviderError::new(ProviderErrorKind::Cancelled, RetryAdvice::Never)
}

fn internal_error() -> ProviderError {
    ProviderError::new(ProviderErrorKind::Internal, RetryAdvice::Never)
}

fn limit_error() -> ProviderError {
    ProviderError::new(ProviderErrorKind::LimitExceeded, RetryAdvice::Never)
}

fn protocol_error() -> ProviderError {
    ProviderError::new(ProviderErrorKind::Protocol, RetryAdvice::Never)
}

fn timeout_error() -> ProviderError {
    ProviderError::new(ProviderErrorKind::Timeout, RetryAdvice::Never)
}

fn transport_error() -> ProviderError {
    ProviderError::new(ProviderErrorKind::Transport, RetryAdvice::Never)
}

fn unavailable_error() -> ProviderError {
    ProviderError::new(ProviderErrorKind::Unavailable, RetryAdvice::Backoff)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jwt(payload: &str) -> String {
        format!("header.{}.signature", URL_SAFE_NO_PAD.encode(payload))
    }

    #[test]
    fn authorization_url_uses_pkce_and_the_fixed_loopback_callback() {
        let url = authorization_url("state-value", "challenge-value").expect("URL");
        let pairs = url
            .query_pairs()
            .collect::<std::collections::BTreeMap<_, _>>();

        assert_eq!(
            url.origin().ascii_serialization(),
            "https://auth.openai.com"
        );
        assert_eq!(
            pairs.get("client_id").map(|value| value.as_ref()),
            Some(CLIENT_ID)
        );
        assert_eq!(
            pairs.get("redirect_uri").map(|value| value.as_ref()),
            Some(REDIRECT_URI)
        );
        assert_eq!(
            pairs.get("state").map(|value| value.as_ref()),
            Some("state-value")
        );
        assert_eq!(
            pairs
                .get("code_challenge_method")
                .map(|value| value.as_ref()),
            Some("S256")
        );
    }

    #[test]
    fn callback_requires_the_exact_path_state_and_code() {
        let request =
            "GET /auth/callback?code=auth-code&state=expected HTTP/1.1\r\nHost: localhost\r\n\r\n";
        assert_eq!(
            parse_callback_request(request, "expected").expect("callback"),
            "auth-code"
        );
        assert!(parse_callback_request(request, "different").is_err());
        assert!(parse_callback_request("GET /favicon.ico HTTP/1.1\r\n\r\n", "expected").is_err());
    }

    #[test]
    fn credential_round_trip_and_debug_are_secret_safe() {
        let access = jwt(r#"{"https://api.openai.com/auth":{"chatgpt_account_id":"account-1"}}"#);
        let credential = CodexOAuthCredential {
            schema: CREDENTIAL_SCHEMA,
            access_token: access.clone(),
            refresh_token: "refresh-secret".to_owned(),
            expires_at_unix_ms: unix_time_ms().saturating_add(60_000),
            account_id: "account-1".to_owned(),
        };
        let encoded = credential.encode().expect("encode");
        let decoded = CodexOAuthCredential::decode(&encoded).expect("decode");

        assert_eq!(decoded.access_token(), access);
        assert_eq!(decoded.account_id(), "account-1");
        assert!(!format!("{decoded:?}").contains("refresh-secret"));
    }

    #[test]
    fn jwt_claims_supply_account_and_residency_without_accepting_invalid_tokens() {
        let token = jwt(
            r#"{"https://api.openai.com/auth":{"chatgpt_account_id":"account-2","chatgpt_data_residency":"us"}}"#,
        );
        assert_eq!(extract_account_id(&token).as_deref(), Some("account-2"));
        assert_eq!(extract_residency(&token).as_deref(), Some("us"));
        assert_eq!(extract_account_id("not-a-jwt"), None);
    }

    #[test]
    fn browser_command_passes_the_url_as_one_direct_argument() {
        let (program, arguments) = browser_command("https://auth.openai.com/example?a=b&c=d");
        assert!(!program.is_empty());
        assert_eq!(
            arguments.last(),
            Some(&"https://auth.openai.com/example?a=b&c=d")
        );
    }
}
