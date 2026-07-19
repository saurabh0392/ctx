//! OAuth 2.1 authorization-code + PKCE for remote MCP destinations.
//!
//! Tokens are stored only in the operating-system credential store. Discovery and token requests
//! use the same redirect-free, proxy-free, DNS-pinned client as normal gateway traffic.

use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use base64::Engine;
use rand::RngCore;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::registry::{GatewayRegistry, GatewayServer, HttpServer};

const KEYRING_SERVICE: &str = "dev.ctx.mcp-oauth";
const CALLBACK_LIMIT: usize = 16 * 1024;
const METADATA_LIMIT: usize = 1024 * 1024;
const TOKEN_LIMIT: usize = 256 * 1024;

#[derive(Debug, Deserialize)]
struct ProtectedResourceMetadata {
    #[serde(default)]
    authorization_servers: Vec<String>,
    #[serde(default)]
    scopes_supported: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct AuthorizationServerMetadata {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    registration_endpoint: Option<String>,
    #[serde(default)]
    scopes_supported: Vec<String>,
    #[serde(default)]
    code_challenge_methods_supported: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ClientRegistration {
    client_id: String,
    client_secret: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
    scope: Option<String>,
    token_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredCredential {
    access_token: String,
    refresh_token: Option<String>,
    expires_at: Option<u64>,
    token_endpoint: String,
    client_id: String,
    client_secret: Option<String>,
    resource: String,
    scope: Option<String>,
}

pub async fn login(server_id: &str, yes: bool) -> Result<()> {
    let server = remote_server(server_id)?;
    let resource = reqwest::Url::parse(&server.url)?;
    let resource_metadata = discover_resource_metadata(&resource).await?;
    let authorization_server = resource_metadata
        .authorization_servers
        .first()
        .context("protected resource metadata advertised no authorization server")?;
    let issuer = reqwest::Url::parse(authorization_server)?;
    let metadata = discover_authorization_server(&issuer).await?;
    if metadata.issuer.trim_end_matches('/') != issuer.as_str().trim_end_matches('/') {
        anyhow::bail!("authorization metadata issuer does not match the discovered issuer");
    }
    if !metadata
        .code_challenge_methods_supported
        .iter()
        .any(|method| method == "S256")
    {
        anyhow::bail!("authorization server does not advertise PKCE S256");
    }
    let authorization_url = validated_https_url(&metadata.authorization_endpoint)?;
    let token_url = validated_https_url(&metadata.token_endpoint)?;
    println!("OAuth destination review for {server_id:?}:");
    println!("  MCP resource: {resource}");
    println!("  Authorization page: {authorization_url}");
    println!("  Token exchange: {token_url}");
    println!("No token, authorization code, or tool result is written to CTX files or SQLite.");
    if !yes && !confirm()? {
        anyhow::bail!("OAuth authorization cancelled");
    }

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
    let callback = format!(
        "http://127.0.0.1:{}/oauth/callback",
        listener.local_addr()?.port()
    );
    let registration_url = metadata
        .registration_endpoint
        .as_deref()
        .context("authorization server does not support dynamic client registration")?;
    let registration_url = validated_https_url(registration_url)?;
    let registration_client = super::http::secure_client_for_url(&registration_url).await?;
    let registration_response = registration_client
        .post(registration_url)
        .json(&serde_json::json!({
            "client_name": "CTX local MCP gateway",
            "redirect_uris": [callback],
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"],
            "token_endpoint_auth_method": "none"
        }))
        .send()
        .await?;
    let registration: ClientRegistration = bounded_json(
        registration_response,
        METADATA_LIMIT,
        "dynamic OAuth client registration failed",
        "parse dynamic client registration",
    )
    .await?;

    let verifier = random_urlsafe(64);
    let state = random_urlsafe(32);
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(Sha256::digest(verifier.as_bytes()));
    let scopes = if !resource_metadata.scopes_supported.is_empty() {
        resource_metadata.scopes_supported.join(" ")
    } else {
        metadata.scopes_supported.join(" ")
    };
    let mut browser_url = authorization_url.clone();
    {
        let mut query = browser_url.query_pairs_mut();
        query
            .append_pair("response_type", "code")
            .append_pair("client_id", &registration.client_id)
            .append_pair("redirect_uri", &callback)
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", &state)
            .append_pair("resource", resource.as_str());
        if !scopes.is_empty() {
            query.append_pair("scope", &scopes);
        }
    }
    open::that(browser_url.as_str()).context("open OAuth authorization page")?;
    println!("Waiting for the browser authorization callback…");
    let code = tokio::time::timeout(
        std::time::Duration::from_secs(300),
        receive_callback(listener, &state),
    )
    .await
    .context("OAuth callback timed out")??;

    let token_client = super::http::secure_client_for_url(&token_url).await?;
    let mut form = vec![
        ("grant_type", "authorization_code".to_owned()),
        ("code", code),
        ("redirect_uri", callback),
        ("client_id", registration.client_id.clone()),
        ("code_verifier", verifier),
        ("resource", resource.as_str().to_owned()),
    ];
    if let Some(secret) = registration.client_secret.as_ref() {
        form.push(("client_secret", secret.clone()));
    }
    let token_response = token_client
        .post(token_url.clone())
        .form(&form)
        .send()
        .await?;
    let token: TokenResponse = bounded_json(
        token_response,
        TOKEN_LIMIT,
        "OAuth token exchange failed",
        "parse OAuth token response",
    )
    .await?;
    if !token.token_type.eq_ignore_ascii_case("bearer") || token.access_token.is_empty() {
        anyhow::bail!("OAuth server returned an unsupported token type");
    }
    let credential = StoredCredential {
        access_token: token.access_token,
        refresh_token: token.refresh_token,
        expires_at: token
            .expires_in
            .map(|seconds| now().saturating_add(seconds)),
        token_endpoint: token_url.to_string(),
        client_id: registration.client_id,
        client_secret: registration.client_secret,
        resource: resource.to_string(),
        scope: token
            .scope
            .or_else(|| (!scopes.is_empty()).then_some(scopes)),
    };
    store_credential(server_id, &credential).await?;
    println!("OAuth credential stored in the operating-system credential store for {server_id:?}.");
    Ok(())
}

pub async fn access_token(server_id: &str) -> Result<String> {
    let mut credential = load_credential(server_id).await?;
    if credential
        .expires_at
        .is_some_and(|expires| expires <= now().saturating_add(60))
    {
        let refresh = credential
            .refresh_token
            .clone()
            .context("OAuth access token expired and no refresh token is available")?;
        let token_url = validated_https_url(&credential.token_endpoint)?;
        let client = super::http::secure_client_for_url(&token_url).await?;
        let mut form = vec![
            ("grant_type", "refresh_token".to_owned()),
            ("refresh_token", refresh),
            ("client_id", credential.client_id.clone()),
            ("resource", credential.resource.clone()),
        ];
        if let Some(scope) = credential.scope.as_ref() {
            form.push(("scope", scope.clone()));
        }
        if let Some(secret) = credential.client_secret.as_ref() {
            form.push(("client_secret", secret.clone()));
        }
        let token_response = client.post(token_url).form(&form).send().await?;
        let token: TokenResponse = bounded_json(
            token_response,
            TOKEN_LIMIT,
            "OAuth token refresh failed",
            "parse OAuth refresh response",
        )
        .await?;
        if !token.token_type.eq_ignore_ascii_case("bearer") || token.access_token.is_empty() {
            anyhow::bail!("OAuth refresh returned an unsupported token type");
        }
        credential.access_token = token.access_token;
        if token.refresh_token.is_some() {
            credential.refresh_token = token.refresh_token;
        }
        credential.expires_at = token
            .expires_in
            .map(|seconds| now().saturating_add(seconds));
        if token.scope.is_some() {
            credential.scope = token.scope;
        }
        store_credential(server_id, &credential).await?;
    }
    Ok(credential.access_token)
}

pub fn logout(server_id: &str) -> Result<()> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, server_id)?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {
            println!("Deleted CTX OAuth credential for {server_id:?}.");
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

fn remote_server(server_id: &str) -> Result<HttpServer> {
    match GatewayRegistry::load()?.servers.get(server_id) {
        Some(GatewayServer::StreamableHttp(server)) => Ok(server.clone()),
        Some(GatewayServer::Stdio(_)) => anyhow::bail!("server {server_id:?} is local stdio"),
        None => anyhow::bail!("gateway server {server_id:?} is not registered"),
    }
}

async fn discover_resource_metadata(resource: &reqwest::Url) -> Result<ProtectedResourceMetadata> {
    let mut candidates = Vec::new();
    let mut path_specific = resource.clone();
    path_specific.set_query(None);
    path_specific.set_fragment(None);
    path_specific.set_path(&format!(
        "/.well-known/oauth-protected-resource{}",
        resource.path()
    ));
    candidates.push(path_specific);
    let mut root = resource.clone();
    root.set_query(None);
    root.set_fragment(None);
    root.set_path("/.well-known/oauth-protected-resource");
    if root != candidates[0] {
        candidates.push(root);
    }
    for candidate in candidates {
        let client = super::http::secure_client_for_url(&candidate).await?;
        if let Ok(response) = client.get(candidate).send().await {
            if let Ok(metadata) = bounded_json(
                response,
                METADATA_LIMIT,
                "protected-resource discovery failed",
                "parse protected-resource metadata",
            )
            .await
            {
                return Ok(metadata);
            }
        }
    }
    anyhow::bail!("remote MCP server did not publish protected-resource metadata")
}

async fn discover_authorization_server(
    issuer: &reqwest::Url,
) -> Result<AuthorizationServerMetadata> {
    let mut metadata_url = issuer.clone();
    let issuer_path = issuer.path().trim_start_matches('/');
    let metadata_path = if issuer_path.is_empty() {
        "/.well-known/oauth-authorization-server".to_owned()
    } else {
        format!("/.well-known/oauth-authorization-server/{issuer_path}")
    };
    metadata_url.set_path(&metadata_path);
    metadata_url.set_query(None);
    metadata_url.set_fragment(None);
    let client = super::http::secure_client_for_url(&metadata_url).await?;
    let response = client.get(metadata_url).send().await?;
    bounded_json(
        response,
        METADATA_LIMIT,
        "OAuth authorization-server discovery failed",
        "parse authorization-server metadata",
    )
    .await
}

async fn bounded_json<T: DeserializeOwned>(
    response: reqwest::Response,
    limit: usize,
    status_context: &str,
    parse_context: &str,
) -> Result<T> {
    let response = response
        .error_for_status()
        .context(status_context.to_owned())?;
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        anyhow::bail!("{parse_context}: response exceeded {limit} bytes");
    }
    let bytes = response.bytes().await?;
    if bytes.len() > limit {
        anyhow::bail!("{parse_context}: response exceeded {limit} bytes");
    }
    serde_json::from_slice(&bytes).context(parse_context.to_owned())
}

async fn receive_callback(
    listener: tokio::net::TcpListener,
    expected_state: &str,
) -> Result<String> {
    let (mut stream, _) = listener.accept().await?;
    let mut bytes = Vec::with_capacity(2_048);
    loop {
        let mut chunk = [0u8; 1_024];
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            break;
        }
        if bytes.len().saturating_add(read) > CALLBACK_LIMIT {
            anyhow::bail!("OAuth callback request exceeded the size limit");
        }
        bytes.extend_from_slice(&chunk[..read]);
        if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    let request = std::str::from_utf8(&bytes).context("OAuth callback was not UTF-8")?;
    let first_line = request.lines().next().context("OAuth callback was empty")?;
    let path = first_line
        .strip_prefix("GET ")
        .and_then(|line| line.split_once(' ').map(|parts| parts.0))
        .context("OAuth callback was not a GET request")?;
    let callback = reqwest::Url::parse(&format!("http://127.0.0.1{path}"))?;
    let params: std::collections::HashMap<_, _> = callback.query_pairs().into_owned().collect();
    let state = params
        .get("state")
        .context("OAuth callback omitted state")?;
    if !constant_time_equal(state.as_bytes(), expected_state.as_bytes()) {
        anyhow::bail!("OAuth callback state did not match");
    }
    if let Some(error) = params.get("error") {
        anyhow::bail!("OAuth authorization failed: {error}");
    }
    let code = params
        .get("code")
        .filter(|code| !code.is_empty() && code.len() <= 8_192)
        .cloned()
        .context("OAuth callback omitted the authorization code")?;
    let body = "Authorization complete. You can close this tab.";
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).await?;
    Ok(code)
}

async fn load_credential(server_id: &str) -> Result<StoredCredential> {
    let server_id = server_id.to_owned();
    tokio::task::spawn_blocking(move || {
        let raw = keyring::Entry::new(KEYRING_SERVICE, &server_id)?.get_password()?;
        Ok::<_, anyhow::Error>(serde_json::from_str(&raw)?)
    })
    .await?
}

async fn store_credential(server_id: &str, credential: &StoredCredential) -> Result<()> {
    let server_id = server_id.to_owned();
    let raw = serde_json::to_string(credential)?;
    tokio::task::spawn_blocking(move || {
        keyring::Entry::new(KEYRING_SERVICE, &server_id)?.set_password(&raw)?;
        Ok::<_, anyhow::Error>(())
    })
    .await?
}

fn validated_https_url(raw: &str) -> Result<reqwest::Url> {
    let url = reqwest::Url::parse(raw)?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        anyhow::bail!("OAuth endpoint must be an HTTPS URL without credentials or a fragment");
    }
    Ok(url)
}

fn confirm() -> Result<bool> {
    print!("Open this authorization server and allow it to issue a CTX credential? [y/N] ");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

fn random_urlsafe(bytes: usize) -> String {
    let mut value = vec![0u8; bytes];
    rand::rng().fill_bytes(&mut value);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(value)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0u8, |difference, (left, right)| difference | (left ^ right))
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_https_oauth_destinations() {
        assert!(validated_https_url("https://auth.example.test/oauth").is_ok());
        assert!(validated_https_url("http://auth.example.test/oauth").is_err());
        assert!(validated_https_url("https://secret@auth.example.test/oauth").is_err());
    }

    #[test]
    fn state_comparison_is_length_and_content_sensitive() {
        assert!(constant_time_equal(b"same", b"same"));
        assert!(!constant_time_equal(b"same", b"diff"));
        assert!(!constant_time_equal(b"same", b"same-longer"));
    }
}
