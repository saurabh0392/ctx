use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const REGISTRY_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayRegistry {
    #[serde(default = "registry_version")]
    pub version: u32,
    #[serde(default)]
    pub servers: BTreeMap<String, GatewayServer>,
    #[serde(default)]
    pub codex_backups: BTreeMap<String, CodexBackup>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexBackup {
    pub gateway_id: String,
    pub server: toml::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "transport", rename_all = "kebab-case")]
pub enum GatewayServer {
    Stdio(StdioServer),
    StreamableHttp(HttpServer),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StdioServer {
    pub command: PathBuf,
    #[serde(default)]
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    #[serde(default)]
    pub pass_env: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpServer {
    pub url: String,
    pub bearer_token_env: Option<String>,
    pub approved_at: String,
}

fn registry_version() -> u32 {
    REGISTRY_VERSION
}

fn registry_path() -> PathBuf {
    crate::config::ctx_dir().join("mcp-gateway.toml")
}

impl Default for GatewayRegistry {
    fn default() -> Self {
        Self {
            version: REGISTRY_VERSION,
            servers: BTreeMap::new(),
            codex_backups: BTreeMap::new(),
        }
    }
}

impl GatewayRegistry {
    pub fn load() -> Result<Self> {
        let path = registry_path();
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("read gateway registry {}", path.display()))?;
        let registry: Self = toml::from_str(&raw)
            .with_context(|| format!("parse gateway registry {}", path.display()))?;
        if registry.version != REGISTRY_VERSION {
            anyhow::bail!(
                "unsupported gateway registry version {} (expected {})",
                registry.version,
                REGISTRY_VERSION
            );
        }
        Ok(registry)
    }

    fn save(&self) -> Result<()> {
        crate::config::ensure_dir()?;
        let path = registry_path();
        let tmp = path.with_extension(format!("toml.tmp.{}", std::process::id()));
        let contents = toml::to_string_pretty(self)?;
        std::fs::write(&tmp, contents)
            .with_context(|| format!("write gateway registry temp {}", tmp.display()))?;
        std::fs::rename(&tmp, &path)
            .with_context(|| format!("replace gateway registry {}", path.display()))?;
        Ok(())
    }
}

pub fn codex_gateway_server_count() -> usize {
    GatewayRegistry::load()
        .map(|registry| registry.codex_backups.len())
        .unwrap_or(0)
}

pub fn add_stdio(
    id: &str,
    command: &str,
    args: Vec<String>,
    cwd: Option<String>,
    pass_env: Vec<String>,
) -> Result<()> {
    validate_id(id)?;
    let command = resolve_executable(command)?;
    let cwd = match cwd {
        Some(cwd) => {
            let path = std::fs::canonicalize(&cwd)
                .with_context(|| format!("resolve server cwd {cwd:?}"))?;
            if !path.is_dir() {
                anyhow::bail!("server cwd is not a directory: {}", path.display());
            }
            Some(path)
        }
        None => None,
    };
    for name in &pass_env {
        validate_env_name(name)?;
    }
    let mut registry = GatewayRegistry::load()?;
    if registry.servers.contains_key(id) {
        anyhow::bail!(
            "gateway server {id:?} already exists; remove it explicitly before replacing it"
        );
    }
    registry.servers.insert(
        id.to_owned(),
        GatewayServer::Stdio(StdioServer {
            command,
            args,
            cwd,
            pass_env,
        }),
    );
    registry.save()?;
    println!("Registered local stdio MCP server {id:?}.");
    println!("Traffic remains local to this device and that approved child process.");
    Ok(())
}

pub fn add_http(
    id: &str,
    url: &str,
    bearer_token_env: Option<String>,
    accept_remote_beta: bool,
) -> Result<()> {
    validate_id(id)?;
    if !accept_remote_beta {
        anyhow::bail!(
            "remote MCP is opt-in beta; review the destination and pass --accept-remote-beta"
        );
    }
    validate_http_url_syntax(url)?;
    if let Some(name) = bearer_token_env.as_deref() {
        validate_env_name(name)?;
    }
    let mut registry = GatewayRegistry::load()?;
    if registry.servers.contains_key(id) {
        anyhow::bail!(
            "gateway server {id:?} already exists; remove it explicitly before replacing it"
        );
    }
    registry.servers.insert(
        id.to_owned(),
        GatewayServer::StreamableHttp(HttpServer {
            url: url.to_owned(),
            bearer_token_env,
            approved_at: chrono::Utc::now().to_rfc3339(),
        }),
    );
    registry.save()?;
    println!("Registered approved remote MCP destination {id:?}: {url}");
    println!("Credentials remain in the named environment variable and are never stored by CTX.");
    Ok(())
}

pub fn list() -> Result<()> {
    let registry = GatewayRegistry::load()?;
    if registry.servers.is_empty() {
        println!("No MCP gateway destinations registered.");
        return Ok(());
    }
    for (id, server) in registry.servers {
        match server {
            GatewayServer::Stdio(server) => println!(
                "{id}\tstdio\t{}\t{} arg(s)\t{} inherited env var(s)",
                server.command.display(),
                server.args.len(),
                server.pass_env.len()
            ),
            GatewayServer::StreamableHttp(server) => println!(
                "{id}\tstreamable-http\t{}\tbearer:{}\tapproved:{}",
                server.url,
                server.bearer_token_env.as_deref().unwrap_or("none"),
                server.approved_at
            ),
        }
    }
    Ok(())
}

fn validate_http_url_syntax(url: &str) -> Result<()> {
    let parsed = reqwest::Url::parse(url).context("parse remote MCP URL")?;
    if parsed.username() != "" || parsed.password().is_some() || parsed.fragment().is_some() {
        anyhow::bail!("remote MCP URL cannot contain credentials or a fragment");
    }
    let host = parsed.host_str().context("remote MCP URL has no host")?;
    match parsed.scheme() {
        "https" => {}
        "http" if matches!(host, "localhost" | "127.0.0.1" | "::1") => {}
        "http" => anyhow::bail!("plain HTTP is allowed only for an explicit loopback endpoint"),
        scheme => anyhow::bail!("unsupported remote MCP URL scheme {scheme:?}"),
    }
    Ok(())
}

pub fn remove(id: &str) -> Result<()> {
    validate_id(id)?;
    let mut registry = GatewayRegistry::load()?;
    if registry.servers.remove(id).is_none() {
        anyhow::bail!("gateway server {id:?} is not registered");
    }
    registry.save()?;
    println!("Removed MCP gateway server {id:?}.");
    Ok(())
}

pub fn codex_enable(name: &str, accept_remote_beta: bool) -> Result<()> {
    validate_id(name)?;
    let path = codex_config_path()?;
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("read Codex config {}", path.display()))?;
    let mut config: toml::Value =
        toml::from_str(&raw).with_context(|| format!("parse Codex config {}", path.display()))?;
    let server = config
        .get("mcp_servers")
        .and_then(toml::Value::as_table)
        .and_then(|servers| servers.get(name))
        .cloned()
        .with_context(|| format!("Codex MCP server {name:?} does not exist"))?;
    let table = server
        .as_table()
        .with_context(|| format!("Codex MCP server {name:?} is not a table"))?;
    let executable = std::fs::canonicalize(std::env::current_exe()?)?;
    if is_current_gateway_command(table, name, &executable) {
        anyhow::bail!("Codex MCP server {name:?} is already routed through CTX");
    }

    let gateway_server = if let Some(command) = table.get("command").and_then(toml::Value::as_str) {
        if table.get("env").is_some() {
            anyhow::bail!(
                "Codex server {name:?} contains literal env values; move secrets to env_vars before importing"
            );
        }
        let args = toml_string_array(table.get("args"), "args")?;
        let pass_env = toml_string_array(table.get("env_vars"), "env_vars")?;
        for env in &pass_env {
            validate_env_name(env)?;
        }
        let cwd = table
            .get("cwd")
            .and_then(toml::Value::as_str)
            .map(std::fs::canonicalize)
            .transpose()
            .context("resolve Codex MCP cwd")?;
        GatewayServer::Stdio(StdioServer {
            command: resolve_executable(command)?,
            args,
            cwd,
            pass_env,
        })
    } else if let Some(url) = table.get("url").and_then(toml::Value::as_str) {
        if !accept_remote_beta {
            anyhow::bail!("remote MCP is opt-in beta; review {url} and pass --accept-remote-beta");
        }
        if table.get("http_headers").is_some() || table.get("env_http_headers").is_some() {
            anyhow::bail!(
                "custom HTTP headers are not imported yet; use bearer_token_env_var or keep this server direct"
            );
        }
        validate_http_url_syntax(url)?;
        let bearer_token_env = table
            .get("bearer_token_env_var")
            .and_then(toml::Value::as_str)
            .map(str::to_owned);
        if let Some(name) = bearer_token_env.as_deref() {
            validate_env_name(name)?;
        }
        GatewayServer::StreamableHttp(HttpServer {
            url: url.to_owned(),
            bearer_token_env,
            approved_at: chrono::Utc::now().to_rfc3339(),
        })
    } else {
        anyhow::bail!("Codex MCP server {name:?} has neither command nor url transport");
    };

    let mut registry = GatewayRegistry::load()?;
    if registry.codex_backups.contains_key(name) || registry.servers.contains_key(name) {
        anyhow::bail!("CTX already owns a gateway definition or Codex backup named {name:?}");
    }
    registry.servers.insert(name.to_owned(), gateway_server);
    registry.codex_backups.insert(
        name.to_owned(),
        CodexBackup {
            gateway_id: name.to_owned(),
            server,
        },
    );
    registry.save()?;

    let current = config
        .get_mut("mcp_servers")
        .and_then(toml::Value::as_table_mut)
        .and_then(|servers| servers.get_mut(name))
        .and_then(toml::Value::as_table_mut)
        .context("Codex MCP table disappeared while rewriting")?;
    for key in [
        "command",
        "args",
        "cwd",
        "env",
        "env_vars",
        "url",
        "bearer_token_env_var",
        "http_headers",
        "env_http_headers",
    ] {
        current.remove(key);
    }
    current.insert(
        "command".into(),
        toml::Value::String(executable.to_string_lossy().into_owned()),
    );
    current.insert(
        "args".into(),
        toml::Value::Array(
            ["gateway", "serve", name, "--surface", "codex"]
                .into_iter()
                .map(|value| toml::Value::String(value.to_owned()))
                .collect(),
        ),
    );
    if let Err(error) = write_toml_atomic(&path, &config) {
        let mut rollback = GatewayRegistry::load().unwrap_or_default();
        rollback.servers.remove(name);
        rollback.codex_backups.remove(name);
        let _ = rollback.save();
        return Err(error);
    }
    println!("Codex MCP server {name:?} now runs through CTX. Restart Codex to activate it.");
    Ok(())
}

fn is_current_gateway_command(
    table: &toml::map::Map<String, toml::Value>,
    name: &str,
    executable: &Path,
) -> bool {
    let Some(command) = table.get("command").and_then(toml::Value::as_str) else {
        return false;
    };
    let Ok(command) = resolve_executable(command) else {
        return false;
    };
    if command != executable {
        return false;
    }
    let Some(args) = table.get("args").and_then(toml::Value::as_array) else {
        return false;
    };
    let expected = ["gateway", "serve", name, "--surface", "codex"];
    args.len() == expected.len()
        && args
            .iter()
            .zip(expected)
            .all(|(actual, expected)| actual.as_str() == Some(expected))
}

pub fn codex_disable(name: &str) -> Result<()> {
    validate_id(name)?;
    let mut registry = GatewayRegistry::load()?;
    let backup = registry
        .codex_backups
        .get(name)
        .cloned()
        .with_context(|| format!("no CTX-owned Codex backup exists for {name:?}"))?;
    let path = codex_config_path()?;
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("read Codex config {}", path.display()))?;
    let mut config: toml::Value = toml::from_str(&raw)?;
    let servers = config
        .get_mut("mcp_servers")
        .and_then(toml::Value::as_table_mut)
        .context("Codex config has no mcp_servers table")?;
    servers.insert(name.to_owned(), backup.server);
    write_toml_atomic(&path, &config)?;
    registry.codex_backups.remove(name);
    registry.servers.remove(&backup.gateway_id);
    registry.save()?;
    println!("Restored direct Codex MCP server {name:?}. Restart Codex to activate it.");
    Ok(())
}

/// Restore every Codex MCP definition that CTX routed through its gateway.
///
/// The registry is the ownership ledger, but a user may have edited a server after CTX enabled it.
/// Only overwrite entries that still have CTX's exact gateway argument shape. Stale ownership rows
/// are removed without touching a user-modified Codex definition.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct CodexRestoreReport {
    pub restored: Vec<String>,
    pub preserved: Vec<String>,
}

pub fn codex_restore_all() -> Result<CodexRestoreReport> {
    let mut registry = GatewayRegistry::load()?;
    if registry.codex_backups.is_empty() {
        return Ok(CodexRestoreReport::default());
    }

    let path = codex_config_path()?;
    if !path.is_file() {
        let backups: Vec<CodexBackup> = registry.codex_backups.values().cloned().collect();
        for backup in backups {
            registry.servers.remove(&backup.gateway_id);
        }
        registry.codex_backups.clear();
        registry.save()?;
        return Ok(CodexRestoreReport::default());
    }
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("read Codex config {}", path.display()))?;
    let mut config: toml::Value =
        toml::from_str(&raw).with_context(|| format!("parse Codex config {}", path.display()))?;
    let mut servers = config
        .get_mut("mcp_servers")
        .and_then(toml::Value::as_table_mut);

    let backups: Vec<(String, CodexBackup)> = registry
        .codex_backups
        .iter()
        .map(|(name, backup)| (name.clone(), backup.clone()))
        .collect();
    let mut report = CodexRestoreReport::default();
    for (name, backup) in &backups {
        let still_owned = servers
            .as_deref()
            .and_then(|servers| servers.get(name))
            .and_then(toml::Value::as_table)
            .is_some_and(|table| is_gateway_shape(table, name));
        if still_owned {
            if let Some(servers) = servers.as_deref_mut() {
                servers.insert(name.clone(), backup.server.clone());
            }
            report.restored.push(name.clone());
        } else if servers
            .as_deref()
            .is_some_and(|servers| servers.contains_key(name))
        {
            report.preserved.push(name.clone());
        }
    }
    if !report.restored.is_empty() {
        write_toml_atomic(&path, &config)?;
    }
    for (name, backup) in backups {
        registry.codex_backups.remove(&name);
        registry.servers.remove(&backup.gateway_id);
    }
    registry.save()?;
    Ok(report)
}

fn is_gateway_shape(table: &toml::map::Map<String, toml::Value>, name: &str) -> bool {
    let Some(command) = table.get("command").and_then(toml::Value::as_str) else {
        return false;
    };
    let executable_name = Path::new(command)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if !matches!(executable_name, "ctx" | "ctx.exe") {
        return false;
    }
    let Some(args) = table.get("args").and_then(toml::Value::as_array) else {
        return false;
    };
    let expected = ["gateway", "serve", name, "--surface", "codex"];
    args.len() == expected.len()
        && args
            .iter()
            .zip(expected)
            .all(|(actual, expected)| actual.as_str() == Some(expected))
}

fn codex_config_path() -> Result<PathBuf> {
    Ok(crate::config::home_dir_for_paths()
        .context("home directory is unavailable")?
        .join(".codex")
        .join("config.toml"))
}

fn toml_string_array(value: Option<&toml::Value>, field: &str) -> Result<Vec<String>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    value
        .as_array()
        .with_context(|| format!("Codex MCP {field} must be an array"))?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .with_context(|| format!("Codex MCP {field} contains a non-string"))
        })
        .collect()
}

fn write_toml_atomic(path: &Path, value: &toml::Value) -> Result<()> {
    let parent = path.parent().context("Codex config has no parent")?;
    std::fs::create_dir_all(parent)?;
    let tmp = path.with_extension(format!("toml.ctx-tmp.{}", std::process::id()));
    std::fs::write(&tmp, toml::to_string_pretty(value)?)?;
    if let Ok(metadata) = std::fs::metadata(path) {
        std::fs::set_permissions(&tmp, metadata.permissions())?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn validate_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        anyhow::bail!("server id must be 1-64 ASCII letters, digits, '.', '_' or '-'");
    }
    Ok(())
}

fn validate_env_name(name: &str) -> Result<()> {
    let mut bytes = name.bytes();
    let Some(first) = bytes.next() else {
        anyhow::bail!("empty environment variable name")
    };
    if !(first.is_ascii_alphabetic() || first == b'_')
        || !bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        anyhow::bail!("invalid environment variable name {name:?}");
    }
    Ok(())
}

fn resolve_executable(command: &str) -> Result<PathBuf> {
    let candidate = Path::new(command);
    if candidate.is_absolute() || candidate.components().count() > 1 {
        return executable_path(candidate);
    }
    let path = std::env::var_os("PATH").context("PATH is unavailable")?;
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join(command);
        if let Ok(path) = executable_path(&candidate) {
            return Ok(path);
        }
        #[cfg(windows)]
        for extension in ["exe", "cmd", "bat", "com"] {
            if let Ok(path) = executable_path(&candidate.with_extension(extension)) {
                return Ok(path);
            }
        }
    }
    anyhow::bail!("could not resolve executable {command:?} on PATH")
}

fn executable_path(path: &Path) -> Result<PathBuf> {
    let path = std::fs::canonicalize(path)?;
    let metadata = std::fs::metadata(&path)?;
    if !metadata.is_file() {
        anyhow::bail!("not a file")
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            anyhow::bail!("not executable")
        }
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identities_and_environment_names_are_closed() {
        assert!(validate_id("linear-prod_1").is_ok());
        assert!(validate_id("../linear").is_err());
        assert!(validate_id("space server").is_err());
        assert!(validate_env_name("LINEAR_TOKEN").is_ok());
        assert!(validate_env_name("LINEAR_TOKEN=value").is_err());
        assert!(validate_http_url_syntax("https://mcp.example.test/rpc").is_ok());
        assert!(validate_http_url_syntax("http://mcp.example.test/rpc").is_err());
        assert!(validate_http_url_syntax("https://token@mcp.example.test/rpc").is_err());
    }

    #[test]
    fn routed_detection_requires_exact_executable_and_gateway_arguments() {
        let executable = std::fs::canonicalize(std::env::current_exe().unwrap()).unwrap();
        let mut table = toml::map::Map::new();
        table.insert(
            "command".into(),
            toml::Value::String(executable.to_string_lossy().into_owned()),
        );
        table.insert(
            "args".into(),
            toml::Value::Array(
                ["gateway", "serve", "linear", "--surface", "codex"]
                    .into_iter()
                    .map(|arg| toml::Value::String(arg.into()))
                    .collect(),
            ),
        );
        assert!(is_current_gateway_command(&table, "linear", &executable));
        assert!(!is_current_gateway_command(&table, "github", &executable));

        table.insert(
            "args".into(),
            toml::Value::Array(vec![toml::Value::String("doctor".into())]),
        );
        assert!(!is_current_gateway_command(&table, "linear", &executable));
    }

    #[test]
    fn restore_all_restores_only_entries_still_routed_through_ctx() {
        let _guard = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CTX_HOME", tmp.path());
        std::env::set_var("CTX_TEST_HOME", tmp.path());
        std::fs::create_dir_all(tmp.path().join(".codex")).unwrap();
        std::fs::write(
            tmp.path().join(".codex/config.toml"),
            r#"
[mcp_servers.linear]
command = "/usr/local/bin/ctx"
args = ["gateway", "serve", "linear", "--surface", "codex"]

[mcp_servers.user_changed]
command = "/usr/bin/custom"
args = ["serve"]
"#,
        )
        .unwrap();
        let mut registry = GatewayRegistry::default();
        for name in ["linear", "user_changed"] {
            registry.servers.insert(
                name.into(),
                GatewayServer::StreamableHttp(HttpServer {
                    url: format!("https://{name}.example.test/mcp"),
                    bearer_token_env: None,
                    approved_at: "now".into(),
                }),
            );
            registry.codex_backups.insert(
                name.into(),
                CodexBackup {
                    gateway_id: name.into(),
                    server: toml::from_str::<toml::Value>(&format!(
                        "command = \"/usr/bin/{name}\"\nargs = [\"direct\"]"
                    ))
                    .unwrap(),
                },
            );
        }
        registry.save().unwrap();

        let restored = codex_restore_all().unwrap();
        assert_eq!(restored.restored, vec!["linear"]);
        assert_eq!(restored.preserved, vec!["user_changed"]);
        let config = std::fs::read_to_string(tmp.path().join(".codex/config.toml")).unwrap();
        assert!(config.contains("command = \"/usr/bin/linear\""));
        assert!(config.contains("command = \"/usr/bin/custom\""));
        let registry = GatewayRegistry::load().unwrap();
        assert!(registry.codex_backups.is_empty());
        assert!(registry.servers.is_empty());

        std::env::remove_var("CTX_HOME");
        std::env::remove_var("CTX_TEST_HOME");
    }
}
