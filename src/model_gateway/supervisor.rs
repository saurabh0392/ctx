//! Per-route user service installation. M4 supports macOS launchd and Linux systemd user units;
//! Windows remains explicit experimental/manual setup until its lifecycle matrix is proven.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::registry::ModelRoute;

pub fn install(route: &ModelRoute, health_nonce: &str) -> Result<()> {
    validate_nonce(health_nonce)?;
    #[cfg(target_os = "macos")]
    return macos::install(route, health_nonce);
    #[cfg(target_os = "linux")]
    return linux::install(route, health_nonce);
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = route;
        anyhow::bail!(
            "automatic model-gateway service setup is not yet supported on this OS; no client configuration was changed"
        )
    }
}

pub fn uninstall(route_id: &str) -> Result<()> {
    validate_route_id(route_id)?;
    #[cfg(target_os = "macos")]
    return macos::uninstall(route_id);
    #[cfg(target_os = "linux")]
    return linux::uninstall(route_id);
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Ok(())
    }
}

fn current_binary() -> Result<PathBuf> {
    let path = std::env::current_exe().context("resolve current ctx binary for model gateway")?;
    if !path.is_file() {
        anyhow::bail!(
            "current ctx executable does not exist at {}",
            path.display()
        );
    }
    Ok(path)
}

fn validate_nonce(nonce: &str) -> Result<()> {
    if nonce.len() != 32 || !nonce.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        anyhow::bail!("model gateway health nonce is malformed");
    }
    Ok(())
}

pub(super) fn validate_nonce_for_lifecycle(nonce: &str) -> Result<()> {
    validate_nonce(nonce)
}

fn validate_route_id(route_id: &str) -> Result<()> {
    if route_id.is_empty()
        || route_id.len() > 64
        || !route_id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        anyhow::bail!("model route id is unsafe for a service name");
    }
    Ok(())
}

fn write_private_atomic(path: &Path, body: &str) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("service definition has no parent"))?;
    std::fs::create_dir_all(parent)?;
    let temporary = path.with_extension(format!("tmp.{}", std::process::id()));
    std::fs::write(&temporary, body)?;
    crate::config::protect_private_file(&temporary)?;
    std::fs::rename(&temporary, path)?;
    Ok(())
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;

    fn label(route_id: &str) -> String {
        format!("com.ctx.model-gateway.{route_id}")
    }

    fn path(route_id: &str) -> Result<PathBuf> {
        let home = crate::config::home_dir_for_paths()
            .ok_or_else(|| anyhow::anyhow!("cannot resolve user home for launchd"))?;
        Ok(home
            .join("Library/LaunchAgents")
            .join(format!("{}.plist", label(route_id))))
    }

    fn uid() -> Result<String> {
        let output = std::process::Command::new("id")
            .arg("-u")
            .output()
            .context("read uid for launchd domain")?;
        if !output.status.success() {
            anyhow::bail!("id -u failed while preparing launchd service");
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }

    fn xml(value: &str) -> String {
        value
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&apos;")
    }

    fn definition(route: &ModelRoute, nonce: &str, binary: &Path) -> String {
        let stdout =
            crate::config::ctx_dir().join(format!("model-gateway-{}.stdout.log", route.id));
        let stderr =
            crate::config::ctx_dir().join(format!("model-gateway-{}.stderr.log", route.id));
        let ctx_home = std::env::var("CTX_HOME").ok().map(|value| {
            format!(
                "  <key>EnvironmentVariables</key><dict><key>CTX_HOME</key><string>{}</string></dict>\n",
                xml(&value)
            )
        }).unwrap_or_default();
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{label}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{binary}</string><string>model-gateway</string><string>serve</string>
    <string>{route_id}</string><string>--health-nonce</string><string>{nonce}</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>ProcessType</key><string>Interactive</string>
{ctx_home}  <key>StandardOutPath</key><string>{stdout}</string>
  <key>StandardErrorPath</key><string>{stderr}</string>
</dict>
</plist>
"#,
            label = xml(&label(&route.id)),
            binary = xml(&binary.display().to_string()),
            route_id = xml(&route.id),
            nonce = xml(nonce),
            stdout = xml(&stdout.display().to_string()),
            stderr = xml(&stderr.display().to_string()),
            ctx_home = ctx_home,
        )
    }

    pub fn install(route: &ModelRoute, nonce: &str) -> Result<()> {
        validate_route_id(&route.id)?;
        crate::config::ensure_dir()?;
        let service_path = path(&route.id)?;
        write_private_atomic(&service_path, &definition(route, nonce, &current_binary()?))?;
        let domain = format!("gui/{}", uid()?);
        let _ = std::process::Command::new("launchctl")
            .args(["bootout", &domain, service_path.to_string_lossy().as_ref()])
            .status();
        let status = std::process::Command::new("launchctl")
            .args([
                "bootstrap",
                &domain,
                service_path.to_string_lossy().as_ref(),
            ])
            .status()
            .context("bootstrap model gateway launchd service")?;
        if !status.success() {
            anyhow::bail!("launchd could not start the CTX model gateway; no client configuration was changed");
        }
        Ok(())
    }

    pub fn uninstall(route_id: &str) -> Result<()> {
        let service_path = path(route_id)?;
        if !service_path.exists() {
            return Ok(());
        }
        let domain = format!("gui/{}", uid()?);
        let _ = std::process::Command::new("launchctl")
            .args(["bootout", &domain, service_path.to_string_lossy().as_ref()])
            .status();
        std::fs::remove_file(&service_path)
            .with_context(|| format!("remove model gateway service {}", service_path.display()))?;
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::model_gateway::registry::{ModelRouteMode, ProviderTarget};
        use crate::model_gateway::route::{AuthenticationMode, WireProtocol};
        use crate::surface::SurfaceId;

        #[test]
        fn launchd_definition_has_fixed_route_and_no_credentials() {
            let route = ModelRoute {
                id: "codex-api".into(),
                surface: SurfaceId::Codex,
                protocol: WireProtocol::OpenAiResponses,
                authentication: AuthenticationMode::ApiKey,
                upstream: ProviderTarget::OpenAi,
                listen_port: 8871,
                mode: ModelRouteMode::Shadow,
            };
            let body = definition(
                &route,
                "00112233445566778899aabbccddeeff",
                Path::new("/tmp/ctx"),
            );
            assert!(body.contains("model-gateway"));
            assert!(body.contains("codex-api"));
            assert!(!body.to_ascii_lowercase().contains("api_key"));
            assert!(!body.to_ascii_lowercase().contains("authorization"));
        }
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;

    fn service_name(route_id: &str) -> String {
        format!("ctx-model-gateway-{route_id}.service")
    }

    fn path(route_id: &str) -> Result<PathBuf> {
        let base = dirs::config_dir().ok_or_else(|| {
            anyhow::anyhow!("cannot resolve systemd user configuration directory")
        })?;
        Ok(base.join("systemd/user").join(service_name(route_id)))
    }

    fn systemd_quote(value: &str) -> String {
        format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
    }

    fn definition(route: &ModelRoute, nonce: &str, binary: &Path) -> String {
        let environment = std::env::var("CTX_HOME")
            .ok()
            .map(|value| {
                format!(
                    "Environment={}\n",
                    systemd_quote(&format!("CTX_HOME={value}"))
                )
            })
            .unwrap_or_default();
        format!(
            "[Unit]\nDescription=CTX model gateway route {}\n\n[Service]\n{}ExecStart={} model-gateway serve {} --health-nonce {}\nRestart=on-failure\nRestartSec=1\n\n[Install]\nWantedBy=default.target\n",
            route.id,
            environment,
            systemd_quote(&binary.display().to_string()),
            route.id,
            nonce
        )
    }

    fn systemctl(args: &[&str]) -> Result<()> {
        let status = std::process::Command::new("systemctl")
            .arg("--user")
            .args(args)
            .status()
            .context("run systemctl --user for model gateway")?;
        if !status.success() {
            anyhow::bail!(
                "systemd user service operation failed; no client configuration was changed"
            );
        }
        Ok(())
    }

    pub fn install(route: &ModelRoute, nonce: &str) -> Result<()> {
        validate_route_id(&route.id)?;
        crate::config::ensure_dir()?;
        write_private_atomic(
            &path(&route.id)?,
            &definition(route, nonce, &current_binary()?),
        )?;
        systemctl(&["daemon-reload"])?;
        let name = service_name(&route.id);
        systemctl(&["enable", "--now", &name])
    }

    pub fn uninstall(route_id: &str) -> Result<()> {
        let service_path = path(route_id)?;
        if !service_path.exists() {
            return Ok(());
        }
        let name = service_name(route_id);
        let _ = std::process::Command::new("systemctl")
            .args(["--user", "disable", "--now", &name])
            .status();
        std::fs::remove_file(&service_path)?;
        systemctl(&["daemon-reload"])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_names_and_nonces_are_strictly_bounded() {
        assert!(validate_route_id("codex-api").is_ok());
        assert!(validate_route_id("../bad").is_err());
        assert!(validate_nonce("00112233445566778899aabbccddeeff").is_ok());
        assert!(validate_nonce("secret value").is_err());
    }
}
