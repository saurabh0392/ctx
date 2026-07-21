use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::surface::SurfaceId;

/// Model API dialect at the local route boundary. Detection must inspect the body contract as well
/// as the URL before any later milestone may transform it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WireProtocol {
    #[serde(rename = "openai-responses")]
    OpenAiResponses,
    #[serde(rename = "openai-chat-completions")]
    OpenAiChatCompletions,
    AnthropicMessages,
    Unknown,
}

impl WireProtocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenAiResponses => "openai-responses",
            Self::OpenAiChatCompletions => "openai-chat-completions",
            Self::AnthropicMessages => "anthropic-messages",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RouteTransport {
    Http,
    Sse,
    #[serde(rename = "websocket")]
    WebSocket,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContentEncoding {
    Identity,
    Gzip,
    Deflate,
    #[serde(rename = "br")]
    Brotli,
    Zstd,
    Unknown,
}

impl RouteTransport {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Sse => "sse",
            Self::WebSocket => "websocket",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AuthenticationMode {
    ApiKey,
    BearerToken,
    #[serde(rename = "chatgpt-login")]
    ChatGptLogin,
    Subscription,
    CustomProvider,
    ManagedBySurface,
    None,
    Unknown,
}

impl AuthenticationMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ApiKey => "api-key",
            Self::BearerToken => "bearer-token",
            Self::ChatGptLogin => "chatgpt-login",
            Self::Subscription => "subscription",
            Self::CustomProvider => "custom-provider",
            Self::ManagedBySurface => "managed-by-surface",
            Self::None => "none",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UpstreamClass {
    #[serde(rename = "openai")]
    OpenAi,
    Anthropic,
    AmazonBedrock,
    GoogleVertex,
    MicrosoftFoundry,
    CursorManaged,
    LocalOrCustom,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConfigurationBoundary {
    SupportedUserConfig,
    GuidedManualSetting,
    SurfaceControlled,
    NotFound,
}

impl ConfigurationBoundary {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SupportedUserConfig => "supported-user-config",
            Self::GuidedManualSetting => "guided-manual-setting",
            Self::SurfaceControlled => "surface-controlled",
            Self::NotFound => "not-found",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProbeStatus {
    ReadyForCapture,
    NeedsConfiguration,
    NeedsManualCapture,
    NotInstalled,
    InvalidConfiguration,
}

impl ProbeStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadyForCapture => "ready-for-capture",
            Self::NeedsConfiguration => "needs-configuration",
            Self::NeedsManualCapture => "needs-manual-capture",
            Self::NotInstalled => "not-installed",
            Self::InvalidConfiguration => "invalid-configuration",
        }
    }
}

/// The M0 support decision. `Support` still means capture evidence is required before routing;
/// `Narrow` means only a named auth/configuration path is a candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RouteDecision {
    Support,
    Narrow,
    Hold,
    Kill,
}

impl RouteDecision {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Support => "support",
            Self::Narrow => "narrow",
            Self::Hold => "hold",
            Self::Kill => "kill",
        }
    }
}

/// Exact evidence identity for a future model-path activation. M0 defines and tests this key but
/// does not create activation receipts or enable transformations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivationIdentity {
    pub surface: SurfaceId,
    pub surface_version: String,
    pub surface_contract: String,
    pub authentication: AuthenticationMode,
    pub protocol: WireProtocol,
    pub protocol_version: String,
    pub transport: RouteTransport,
    pub content_encoding: ContentEncoding,
    pub upstream_class: UpstreamClass,
    pub normalized_tool_identity: String,
    pub result_shape: String,
    pub transform_version: String,
}

impl ActivationIdentity {
    /// A display-safe deterministic key. The constituent values stay in the local receipt; logs
    /// and UI joins can use this digest without exposing tool/provider identifiers.
    pub fn stable_key(&self) -> String {
        let encoded = serde_json::to_vec(self).expect("activation identity is serializable");
        let digest = Sha256::digest(encoded);
        format!("model-route:{digest:x}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> ActivationIdentity {
        ActivationIdentity {
            surface: SurfaceId::Codex,
            surface_version: "0.145".into(),
            surface_contract: "codex-responses-v1".into(),
            authentication: AuthenticationMode::ApiKey,
            protocol: WireProtocol::OpenAiResponses,
            protocol_version: "responses-v1".into(),
            transport: RouteTransport::Sse,
            content_encoding: ContentEncoding::Identity,
            upstream_class: UpstreamClass::OpenAi,
            normalized_tool_identity: "shell".into(),
            result_shape: "function-call-output-text".into(),
            transform_version: "shadow-v1".into(),
        }
    }

    #[test]
    fn authentication_cannot_reuse_another_routes_activation_key() {
        let api_key = identity();
        let mut chatgpt = api_key.clone();
        chatgpt.authentication = AuthenticationMode::ChatGptLogin;
        assert_ne!(api_key.stable_key(), chatgpt.stable_key());
    }

    #[test]
    fn protocol_cannot_reuse_another_routes_activation_key() {
        let responses = identity();
        let mut chat = responses.clone();
        chat.protocol = WireProtocol::OpenAiChatCompletions;
        assert_ne!(responses.stable_key(), chat.stable_key());
    }
}
