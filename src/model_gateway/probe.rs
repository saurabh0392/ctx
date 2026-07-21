//! Read-only compatibility probes for model-path routing candidates.

use std::io::Read;
use std::path::Path;
use std::process::Command;

use serde::Serialize;

use crate::model_gateway::route::{
    AuthenticationMode, ConfigurationBoundary, ProbeStatus, RouteDecision, RouteTransport,
    UpstreamClass, WireProtocol,
};
use crate::surface::SurfaceId;

const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigEvidence {
    /// Stable symbolic location only. Absolute home paths are deliberately never emitted.
    pub location: &'static str,
    pub present: bool,
    pub parsed: bool,
    /// Allowlisted key names only; values and user-defined provider ids are never emitted.
    pub keys_present: Vec<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteProbeReceipt {
    pub schema_version: u32,
    pub surface: SurfaceId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_version: Option<String>,
    pub installed: bool,
    pub status: ProbeStatus,
    pub decision: RouteDecision,
    pub configuration_boundary: ConfigurationBoundary,
    pub protocol: WireProtocol,
    pub transports_to_capture: Vec<RouteTransport>,
    pub authentication: AuthenticationMode,
    pub upstream_class: UpstreamClass,
    pub config: Vec<ConfigEvidence>,
    pub reasons: Vec<&'static str>,
    pub credential_store: &'static str,
    pub client_process_executed: bool,
    pub client_state_mutation_possible: bool,
    pub ctx_state_mutated: bool,
    pub credential_contents_read: bool,
}

pub fn probe(surface: SurfaceId, run_client_version: bool) -> RouteProbeReceipt {
    let home = crate::config::home_dir_for_paths().unwrap_or_else(|| std::path::PathBuf::from("."));
    let (installed, passive_version, command) = match surface {
        SurfaceId::ClaudeCode => (executable_present("claude"), None, Some("claude")),
        SurfaceId::Cursor => {
            let app_version = cursor_app_version(&home);
            let command = if executable_present("cursor-agent") {
                Some("cursor-agent")
            } else if executable_present("cursor") {
                Some("cursor")
            } else {
                None
            };
            (
                command.is_some() || app_version.is_some(),
                app_version,
                command,
            )
        }
        SurfaceId::Codex => (executable_present("codex"), None, Some("codex")),
    };
    let execute = run_client_version && command.is_some() && installed;
    let version = if execute {
        command.and_then(command_version).or(passive_version)
    } else {
        passive_version
    };
    inspect_detected(surface, &home, installed, version, execute)
}

/// Inspect a supplied home and version receipt. Kept public so contract tests can prove the probe
/// neither writes to disk nor depends on the test runner's real user profile.
pub fn inspect(
    surface: SurfaceId,
    home: &Path,
    client_version: Option<String>,
) -> RouteProbeReceipt {
    let installed = client_version.is_some();
    inspect_detected(surface, home, installed, client_version, false)
}

fn inspect_detected(
    surface: SurfaceId,
    home: &Path,
    installed: bool,
    client_version: Option<String>,
    client_process_executed: bool,
) -> RouteProbeReceipt {
    match surface {
        SurfaceId::ClaudeCode => {
            inspect_claude(home, installed, client_version, client_process_executed)
        }
        SurfaceId::Cursor => {
            inspect_cursor(home, installed, client_version, client_process_executed)
        }
        SurfaceId::Codex => inspect_codex(home, installed, client_version, client_process_executed),
    }
}

fn base(
    surface: SurfaceId,
    installed: bool,
    client_version: Option<String>,
    client_process_executed: bool,
) -> RouteProbeReceipt {
    RouteProbeReceipt {
        schema_version: 1,
        surface,
        installed,
        client_version,
        status: ProbeStatus::NotInstalled,
        decision: RouteDecision::Hold,
        configuration_boundary: ConfigurationBoundary::NotFound,
        protocol: WireProtocol::Unknown,
        transports_to_capture: Vec::new(),
        authentication: AuthenticationMode::Unknown,
        upstream_class: UpstreamClass::Unknown,
        config: Vec::new(),
        reasons: Vec::new(),
        credential_store: "not-inspected",
        client_process_executed,
        client_state_mutation_possible: client_process_executed,
        ctx_state_mutated: false,
        credential_contents_read: false,
    }
}

fn inspect_codex(
    home: &Path,
    installed: bool,
    client_version: Option<String>,
    client_process_executed: bool,
) -> RouteProbeReceipt {
    let mut receipt = base(
        SurfaceId::Codex,
        installed,
        client_version,
        client_process_executed,
    );
    let path = home.join(".codex/config.toml");
    let raw = read_small_config(&path);
    let parsed = raw
        .as_deref()
        .and_then(|body| body.parse::<toml::Value>().ok());
    let mut keys = Vec::new();
    let openai_base_url = parsed
        .as_ref()
        .and_then(|v| v.get("openai_base_url"))
        .and_then(toml::Value::as_str);
    if openai_base_url.is_some() {
        keys.push("openai_base_url");
    }
    let chatgpt_base_url = parsed
        .as_ref()
        .and_then(|v| v.get("chatgpt_base_url"))
        .and_then(toml::Value::as_str);
    if chatgpt_base_url.is_some() {
        keys.push("chatgpt_base_url");
    }
    let selected_provider = parsed
        .as_ref()
        .and_then(|v| v.get("model_provider"))
        .and_then(toml::Value::as_str);
    if selected_provider.is_some() {
        keys.push("model_provider");
    }
    let provider_table = parsed
        .as_ref()
        .and_then(|v| v.get("model_providers"))
        .and_then(toml::Value::as_table);
    if provider_table.is_some() {
        keys.push("model_providers");
    }
    let forced_login = parsed
        .as_ref()
        .and_then(|v| v.get("forced_login_method"))
        .and_then(toml::Value::as_str);
    if forced_login.is_some() {
        keys.push("forced_login_method");
    }
    let custom_selected = selected_provider.is_some_and(|provider| {
        !matches!(
            provider,
            "openai" | "ollama" | "lmstudio" | "amazon-bedrock"
        )
    });
    let selected_custom_has_base_url = selected_provider
        .and_then(|provider| provider_table.and_then(|table| table.get(provider)))
        .and_then(toml::Value::as_table)
        .is_some_and(|table| {
            table
                .get("base_url")
                .and_then(toml::Value::as_str)
                .is_some()
        });

    receipt.config.push(ConfigEvidence {
        location: "~/.codex/config.toml",
        present: path.is_file(),
        parsed: parsed.is_some(),
        keys_present: keys,
    });
    receipt.protocol = WireProtocol::OpenAiResponses;
    receipt.transports_to_capture = vec![
        RouteTransport::Http,
        RouteTransport::Sse,
        RouteTransport::WebSocket,
    ];
    receipt.configuration_boundary = ConfigurationBoundary::SupportedUserConfig;
    receipt.upstream_class = if custom_selected
        || openai_base_url.is_some_and(|url| !is_official_openai_url(url))
        || chatgpt_base_url.is_some_and(|url| !is_official_chatgpt_url(url))
    {
        UpstreamClass::LocalOrCustom
    } else {
        UpstreamClass::OpenAi
    };
    receipt.authentication = match forced_login {
        Some("api") => AuthenticationMode::ApiKey,
        Some("chatgpt") => AuthenticationMode::ChatGptLogin,
        _ if custom_selected => AuthenticationMode::CustomProvider,
        _ => AuthenticationMode::Unknown,
    };
    receipt.credential_store = if home.join(".codex/auth.json").is_file() {
        "file-present-contents-not-read"
    } else {
        "not-detected"
    };

    if !receipt.installed {
        receipt.reasons = vec![
            "Codex executable was not detected; configuration presence is not installation proof",
        ];
    } else if path.is_file() && parsed.is_none() {
        receipt.status = ProbeStatus::InvalidConfiguration;
        receipt.reasons =
            vec!["Codex user config exists but is not valid TOML; no route can be inferred"];
    } else if openai_base_url.is_some()
        || chatgpt_base_url.is_some()
        || selected_custom_has_base_url
    {
        receipt.status = ProbeStatus::ReadyForCapture;
        receipt.decision = RouteDecision::Narrow;
        receipt.reasons = vec![
            "documented user-level provider routing is configured",
            "live request capture is still required before any route can become active",
            "hosted OpenAI tools remain outside the client-side mutation boundary",
        ];
    } else if receipt.installed {
        receipt.status = ProbeStatus::NeedsConfiguration;
        receipt.decision = RouteDecision::Narrow;
        receipt.reasons = vec![
            "Codex is installed but no explicit model base-URL route was detected",
            "ChatGPT login and API-key routes must earn separate compatibility receipts",
            "hosted OpenAI tools remain outside the client-side mutation boundary",
        ];
    } else {
        receipt.reasons = vec!["Codex was not detected"];
    }
    receipt
}

fn inspect_claude(
    home: &Path,
    installed: bool,
    client_version: Option<String>,
    client_process_executed: bool,
) -> RouteProbeReceipt {
    let mut receipt = base(
        SurfaceId::ClaudeCode,
        installed,
        client_version,
        client_process_executed,
    );
    let path = home.join(".claude/settings.json");
    let raw = read_small_config(&path);
    let parsed = raw
        .as_deref()
        .and_then(|body| serde_json::from_str::<serde_json::Value>(body).ok());
    let env = parsed
        .as_ref()
        .and_then(|v| v.get("env"))
        .and_then(serde_json::Value::as_object);
    let present_or_env = |name: &str| {
        env.is_some_and(|vars| vars.get(name).is_some()) || std::env::var_os(name).is_some()
    };
    let base_url = present_or_env("ANTHROPIC_BASE_URL");
    let api_key = present_or_env("ANTHROPIC_API_KEY");
    let auth_token = present_or_env("ANTHROPIC_AUTH_TOKEN");
    let bedrock = present_or_env("CLAUDE_CODE_USE_BEDROCK");
    let vertex = present_or_env("CLAUDE_CODE_USE_VERTEX");
    let foundry = present_or_env("CLAUDE_CODE_USE_FOUNDRY");
    let mut keys = Vec::new();
    if base_url {
        keys.push("ANTHROPIC_BASE_URL");
    }
    if api_key {
        keys.push("ANTHROPIC_API_KEY");
    }
    if auth_token {
        keys.push("ANTHROPIC_AUTH_TOKEN");
    }
    if bedrock {
        keys.push("CLAUDE_CODE_USE_BEDROCK");
    }
    if vertex {
        keys.push("CLAUDE_CODE_USE_VERTEX");
    }
    if foundry {
        keys.push("CLAUDE_CODE_USE_FOUNDRY");
    }

    receipt.config.push(ConfigEvidence {
        location: "~/.claude/settings.json",
        present: path.is_file(),
        parsed: parsed.is_some(),
        keys_present: keys,
    });
    receipt.protocol = WireProtocol::AnthropicMessages;
    receipt.transports_to_capture = vec![RouteTransport::Http, RouteTransport::Sse];
    receipt.configuration_boundary = ConfigurationBoundary::SupportedUserConfig;
    receipt.authentication = if api_key {
        AuthenticationMode::ApiKey
    } else if auth_token {
        AuthenticationMode::BearerToken
    } else {
        AuthenticationMode::Unknown
    };
    receipt.upstream_class = if bedrock {
        UpstreamClass::AmazonBedrock
    } else if vertex {
        UpstreamClass::GoogleVertex
    } else if foundry {
        UpstreamClass::MicrosoftFoundry
    } else if base_url {
        UpstreamClass::LocalOrCustom
    } else {
        UpstreamClass::Anthropic
    };

    if !receipt.installed {
        receipt.reasons = vec![
            "Claude Code executable was not detected; settings presence is not installation proof",
        ];
    } else if path.is_file() && parsed.is_none() {
        receipt.status = ProbeStatus::InvalidConfiguration;
        receipt.reasons = vec!["Claude Code settings exist but are not valid JSON"];
    } else if bedrock || vertex || foundry {
        receipt.status = ProbeStatus::NeedsManualCapture;
        receipt.decision = RouteDecision::Hold;
        receipt.protocol = WireProtocol::Unknown;
        receipt.reasons = vec![
            "a cloud-provider Claude Code mode is configured",
            "cloud signing and provider dialects are outside the first Anthropic Messages route",
            "this route stays held until a separate credential and protocol threat model passes",
        ];
    } else if base_url {
        receipt.status = ProbeStatus::ReadyForCapture;
        receipt.decision = RouteDecision::Narrow;
        receipt.reasons = vec![
            "a Claude Code base-URL route is configured",
            "subscription, bearer-token, and API-key paths require separate receipts",
            "hosted and surface-controlled tools remain outside this boundary",
        ];
    } else if receipt.installed {
        receipt.status = ProbeStatus::NeedsConfiguration;
        receipt.decision = RouteDecision::Narrow;
        receipt.reasons = vec![
            "Claude Code is installed but no explicit base-URL route was detected",
            "live request capture is required before any route can become active",
        ];
    } else {
        receipt.reasons = vec!["Claude Code was not detected"];
    }
    receipt
}

fn inspect_cursor(
    home: &Path,
    installed: bool,
    client_version: Option<String>,
    client_process_executed: bool,
) -> RouteProbeReceipt {
    let mut receipt = base(
        SurfaceId::Cursor,
        installed,
        client_version,
        client_process_executed,
    );
    let hooks = home.join(".cursor/hooks.json");
    let mcp = home.join(".cursor/mcp.json");
    let hooks_raw = read_small_config(&hooks);
    let mcp_raw = read_small_config(&mcp);
    receipt.config.push(ConfigEvidence {
        location: "~/.cursor/hooks.json",
        present: hooks.is_file(),
        parsed: hooks_raw
            .as_deref()
            .is_some_and(|raw| serde_json::from_str::<serde_json::Value>(raw).is_ok()),
        keys_present: Vec::new(),
    });
    receipt.config.push(ConfigEvidence {
        location: "~/.cursor/mcp.json",
        present: mcp.is_file(),
        parsed: mcp_raw
            .as_deref()
            .is_some_and(|raw| serde_json::from_str::<serde_json::Value>(raw).is_ok()),
        keys_present: Vec::new(),
    });
    receipt.status = if receipt.installed {
        ProbeStatus::NeedsManualCapture
    } else {
        ProbeStatus::NotInstalled
    };
    receipt.decision = RouteDecision::Hold;
    receipt.configuration_boundary = ConfigurationBoundary::SurfaceControlled;
    receipt.protocol = WireProtocol::Unknown;
    receipt.transports_to_capture = vec![
        RouteTransport::Http,
        RouteTransport::Sse,
        RouteTransport::WebSocket,
    ];
    receipt.authentication = AuthenticationMode::ManagedBySurface;
    receipt.upstream_class = UpstreamClass::CursorManaged;
    receipt.reasons = if receipt.installed {
        vec![
            "Cursor is installed, but no documented machine-editable model route was proven",
            "Cursor documents that BYOK requests still pass through its backend for prompt assembly",
            "a separate CLI endpoint or guided experiment must prove which requests cross loopback",
            "built-in Cursor model traffic remains surface-controlled until that proof exists",
        ]
    } else {
        vec!["Cursor was not detected"]
    };
    receipt
}

fn command_version(command: &str) -> Option<String> {
    let output = Command::new(command).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let value = if output.stdout.is_empty() {
        String::from_utf8_lossy(&output.stderr)
    } else {
        String::from_utf8_lossy(&output.stdout)
    };
    sanitize_version(&value)
}

fn is_official_openai_url(raw: &str) -> bool {
    reqwest::Url::parse(raw)
        .ok()
        .and_then(|url| url.host_str().map(str::to_ascii_lowercase))
        .is_some_and(|host| host == "api.openai.com" || host.ends_with(".api.openai.com"))
}

fn is_official_chatgpt_url(raw: &str) -> bool {
    reqwest::Url::parse(raw)
        .ok()
        .and_then(|url| url.host_str().map(str::to_ascii_lowercase))
        .is_some_and(|host| host == "chatgpt.com" || host.ends_with(".chatgpt.com"))
}

fn executable_present(command: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|directory| {
        if is_executable(&directory.join(command)) {
            return true;
        }
        #[cfg(windows)]
        {
            return ["exe", "cmd", "bat"]
                .iter()
                .any(|extension| is_executable(&directory.join(format!("{command}.{extension}"))));
        }
        #[cfg(not(windows))]
        false
    })
}

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    true
}

fn read_small_config(path: &Path) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    if file.metadata().ok()?.len() > MAX_CONFIG_BYTES {
        return None;
    }
    let mut raw = String::new();
    file.read_to_string(&mut raw).ok()?;
    Some(raw)
}

fn sanitize_version(raw: &str) -> Option<String> {
    let first = raw.lines().next()?.trim();
    if first.is_empty() || first.len() > 160 {
        return Some("detected".into());
    }
    if first.chars().all(|c| {
        c.is_ascii_alphanumeric() || matches!(c, ' ' | '.' | '-' | '_' | '+' | '(' | ')' | '/')
    }) {
        Some(first.to_string())
    } else {
        Some("detected".into())
    }
}

#[cfg(target_os = "macos")]
fn cursor_app_version(home: &Path) -> Option<String> {
    let system = Path::new("/Applications/Cursor.app/Contents/Info.plist");
    let user = home.join("Applications/Cursor.app/Contents/Info.plist");
    let plist = if system.is_file() {
        system
    } else if user.is_file() {
        &user
    } else {
        return None;
    };
    let output = Command::new("/usr/libexec/PlistBuddy")
        .args(["-c", "Print :CFBundleShortVersionString"])
        .arg(plist)
        .output()
        .ok()?;
    if !output.status.success() {
        return Some("detected".into());
    }
    sanitize_version(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(not(target_os = "macos"))]
fn cursor_app_version(_home: &Path) -> Option<String> {
    None
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn passive_probe_does_not_execute_a_detected_client() {
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let temp = tempfile::tempdir().unwrap();
        let bin = temp.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let marker = temp.path().join("client-ran");
        let command = bin.join("codex");
        std::fs::write(
            &command,
            format!(
                "#!/bin/sh\nprintf ran > '{}'\nprintf 'codex 9.9.9\\n'\n",
                marker.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&command, std::fs::Permissions::from_mode(0o700)).unwrap();

        let old_path = std::env::var_os("PATH");
        let old_home = std::env::var_os("CTX_HOME");
        std::env::set_var("PATH", &bin);
        std::env::set_var("CTX_HOME", temp.path());
        let receipt = probe(SurfaceId::Codex, false);
        if let Some(value) = old_path {
            std::env::set_var("PATH", value);
        } else {
            std::env::remove_var("PATH");
        }
        if let Some(value) = old_home {
            std::env::set_var("CTX_HOME", value);
        } else {
            std::env::remove_var("CTX_HOME");
        }

        assert!(receipt.installed);
        assert!(receipt.client_version.is_none());
        assert!(!receipt.client_process_executed);
        assert!(!marker.exists());
    }
}
