//! Background services: launchd (macOS), systemd user units (Linux), or detached `ctx` children (other OS).

use anyhow::{Context, Result};
use std::path::PathBuf;

pub const PROXY_LABEL: &str = "com.ctx.proxy";
pub const DASHBOARD_LABEL: &str = "com.ctx.dashboard";
pub const INGEST_LABEL: &str = "com.ctx.ingest";

fn ctx_binary() -> String {
    // Use the path of the currently-running binary so plist entries stay correct
    // regardless of whether ctx was installed via cargo, the install.sh script
    // (which defaults to ~/.local/bin), or a custom CTX_INSTALL_DIR.
    if let Ok(exe) = std::env::current_exe() {
        if exe.exists() {
            return exe.to_string_lossy().into_owned();
        }
    }
    // Fallback: cargo install location
    dirs::home_dir()
        .map(|h| h.join(".cargo").join("bin").join("ctx"))
        .and_then(|p| p.to_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "ctx".to_string())
}

fn home_display() -> String {
    dirs::home_dir()
        .map(|h| h.display().to_string())
        .unwrap_or_default()
}

fn cargo_bin_display() -> String {
    dirs::home_dir()
        .map(|h| h.join(".cargo").join("bin").display().to_string())
        .unwrap_or_default()
}

/// Human-readable description for `ctx setup` preview lines.
pub fn background_services_summary(port: u16, upstream: &str, dashboard_port: u16, periodic_ingest: bool) -> String {
    let ingest_tail = if periodic_ingest {
        ", periodic ingest (every 5 min)"
    } else {
        ""
    };
    #[cfg(target_os = "macos")]
    {
        format!(
            "Install launchd agents: proxy (:{port} → {upstream}), dashboard (:{dashboard_port}){ingest_tail}"
        )
    }
    #[cfg(target_os = "linux")]
    {
        format!(
            "Install systemd user services: ctx-proxy (:{port} → {upstream}), ctx-dashboard (:{dashboard_port}){ingest_tail}"
        )
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        format!(
            "Start ctx proxy (:{port}), dashboard (:{dashboard_port}) in the background{ingest_tail}; add a scheduled task for `ctx ingest` if you want periodic indexing"
        )
    }
}

pub fn step1_banner() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "Step 1/5: Installing launchd agent (auto-start on login)..."
    }
    #[cfg(target_os = "linux")]
    {
        "Step 1/5: Installing systemd user services (auto-start on login)..."
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        "Step 1/5: Starting ctx proxy in the background..."
    }
}

pub fn step2_banner() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "Step 2/5: Starting ctx proxy via launchctl..."
    }
    #[cfg(target_os = "linux")]
    {
        "Step 2/5: Enabling ctx-proxy via systemctl --user..."
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        "Step 2/5: Waiting for ctx proxy..."
    }
}

pub fn step5_banner() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "Step 5/5: Starting ctx dashboard as background service..."
    }
    #[cfg(target_os = "linux")]
    {
        "Step 5/5: Starting ctx-dashboard systemd user service..."
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        "Step 5/5: Starting ctx dashboard in the background..."
    }
}

pub fn install_proxy(port: u16, upstream: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    return macos::write_proxy_plist(port, upstream);
    #[cfg(target_os = "linux")]
    return linux::write_proxy_unit(port, upstream);
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (port, upstream);
        Ok(())
    }
}

pub fn bootstrap_proxy(port: u16, upstream: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let _ = (port, upstream);
        return macos::load_proxy_plist();
    }
    #[cfg(target_os = "linux")]
    return linux::daemon_reload_and_enable(&["ctx-proxy.service"]);
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        try_spawn_proxy(port, upstream)?;
        Ok(())
    }
}

pub fn install_dashboard(port: u16) -> Result<()> {
    #[cfg(target_os = "macos")]
    return macos::write_dashboard_plist(port);
    #[cfg(target_os = "linux")]
    return linux::write_dashboard_unit(port);
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = port;
        Ok(())
    }
}

pub fn bootstrap_dashboard(dashboard_port: u16) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let _ = dashboard_port;
        return macos::load_dashboard_plist();
    }
    #[cfg(target_os = "linux")]
    return linux::daemon_reload_and_enable(&["ctx-dashboard.service"]);
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        try_spawn_dashboard(dashboard_port)?;
        Ok(())
    }
}

pub fn install_periodic_ingest() -> Result<()> {
    #[cfg(target_os = "macos")]
    return macos::write_ingest_plist();
    #[cfg(target_os = "linux")]
    return linux::write_ingest_unit_and_timer();
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        println!(
            "  {} On this OS ctx does not install a periodic ingest job. Run `ctx ingest` manually or add a scheduler (every 5 minutes).",
            "i".yellow()
        );
        Ok(())
    }
}

pub fn bootstrap_ingest() -> Result<()> {
    #[cfg(target_os = "macos")]
    return macos::load_ingest_plist();
    #[cfg(target_os = "linux")]
    return linux::daemon_reload_and_enable(&["ctx-ingest.timer"]);
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Ok(())
    }
}

pub fn uninstall_all() -> Result<()> {
    #[cfg(target_os = "macos")]
    return macos::uninstall_launchd_agents();
    #[cfg(target_os = "linux")]
    return linux::uninstall_systemd_units();
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        println!(
            "  {} Stop any `ctx proxy start` / `ctx dashboard` terminals you opened for ctx.",
            "i".yellow()
        );
        Ok(())
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn try_spawn_proxy(port: u16, upstream: &str) -> Result<()> {
    use colored::Colorize;
    let bin = ctx_binary();
    let status = std::process::Command::new(&bin)
        .args([
            "proxy",
            "start",
            "--port",
            &port.to_string(),
            "--upstream",
            upstream,
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    match status {
        Ok(s) if s.success() => {}
        Ok(_) | Err(_) => {
            eprintln!(
                "  {} Could not auto-start the proxy. Start it manually before `ctx proxy install`:",
                "!".yellow()
            );
            eprintln!("    {bin} proxy start --port {port} --upstream {upstream}");
        }
    }
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn try_spawn_dashboard(port: u16) -> Result<()> {
    let bin = ctx_binary();
    let _ = std::process::Command::new(&bin)
        .args([
            "dashboard",
            "--port",
            &port.to_string(),
            "--no-open",
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    Ok(())
}

// ---------------------------------------------------------------------------
// macOS launchd
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
mod macos {
    use super::*;

    fn plist_path(label: &str) -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Library")
            .join("LaunchAgents")
            .join(format!("{label}.plist"))
    }

    fn uid() -> String {
        std::process::Command::new("id")
            .arg("-u")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "501".to_string())
    }

    pub fn write_proxy_plist(port: u16, upstream: &str) -> Result<()> {
        let bin = ctx_binary();
        let log_dir = crate::config::ctx_dir();
        let stdout_log = log_dir.join("proxy.stdout.log");
        let stderr_log = log_dir.join("proxy.stderr.log");
        let ca_cert = crate::ca::canonical_ca_cert_path_string()?
            .replace('&', "&amp;")
            .replace('<', "&lt;");

        let plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{PROXY_LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{bin}</string>
        <string>proxy</string>
        <string>start</string>
        <string>--port</string>
        <string>{port}</string>
        <string>--upstream</string>
        <string>{upstream}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>{stdout}</string>
    <key>StandardErrorPath</key>
    <string>{stderr}</string>
    <key>EnvironmentVariables</key>
    <dict>
        <key>HOME</key>
        <string>{home}</string>
        <key>PATH</key>
        <string>/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin:{cargo_bin}</string>
        <key>NODE_EXTRA_CA_CERTS</key>
        <string>{ca_cert}</string>
    </dict>
</dict>
</plist>"#,
            stdout = stdout_log.display(),
            stderr = stderr_log.display(),
            home = home_display(),
            cargo_bin = cargo_bin_display(),
        );

        let p = plist_path(PROXY_LABEL);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&p, &plist)?;
        println!("  Written {}", p.display());
        Ok(())
    }

    pub fn write_dashboard_plist(port: u16) -> Result<()> {
        let bin = ctx_binary();
        let log_dir = crate::config::ctx_dir();
        let stdout_log = log_dir.join("dashboard.stdout.log");
        let stderr_log = log_dir.join("dashboard.stderr.log");

        let plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{DASHBOARD_LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{bin}</string>
        <string>dashboard</string>
        <string>--port</string>
        <string>{port}</string>
        <string>--no-open</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>{stdout}</string>
    <key>StandardErrorPath</key>
    <string>{stderr}</string>
    <key>EnvironmentVariables</key>
    <dict>
        <key>HOME</key>
        <string>{home}</string>
        <key>PATH</key>
        <string>/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin:{cargo_bin}</string>
    </dict>
</dict>
</plist>"#,
            stdout = stdout_log.display(),
            stderr = stderr_log.display(),
            home = home_display(),
            cargo_bin = cargo_bin_display(),
        );

        let p = plist_path(DASHBOARD_LABEL);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&p, &plist)?;
        println!("  Written {}", p.display());
        Ok(())
    }

    pub fn write_ingest_plist() -> Result<()> {
        let bin = ctx_binary();
        let log_dir = crate::config::ctx_dir();
        let stdout_log = log_dir.join("ingest.stdout.log");
        let stderr_log = log_dir.join("ingest.stderr.log");

        let plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{INGEST_LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{bin}</string>
        <string>ingest</string>
    </array>
    <key>StartInterval</key>
    <integer>300</integer>
    <key>StandardOutPath</key>
    <string>{stdout}</string>
    <key>StandardErrorPath</key>
    <string>{stderr}</string>
    <key>EnvironmentVariables</key>
    <dict>
        <key>HOME</key>
        <string>{home}</string>
        <key>PATH</key>
        <string>/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin:{cargo_bin}</string>
    </dict>
</dict>
</plist>"#,
            stdout = stdout_log.display(),
            stderr = stderr_log.display(),
            home = home_display(),
            cargo_bin = cargo_bin_display(),
        );

        let p = plist_path(INGEST_LABEL);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&p, &plist)?;
        println!("  Written {}", p.display());
        Ok(())
    }

    fn bootout_bootstrap(label: &str, friendly: &str) -> Result<()> {
        let p = plist_path(label);
        let domain = format!("gui/{}", uid());
        let _ = std::process::Command::new("launchctl")
            .args(["bootout", &domain, p.to_str().unwrap_or("")])
            .status();

        let status = std::process::Command::new("launchctl")
            .args(["bootstrap", &domain, p.to_str().unwrap_or("")])
            .status()
            .with_context(|| format!("launchctl bootstrap failed for {friendly}"))?;

        if status.success() {
            println!("  launchd agent bootstrapped ({label})");
        } else {
            anyhow::bail!(
                "launchctl bootstrap failed for {friendly}. Try manually:\n  launchctl bootstrap {} {}",
                domain,
                p.display()
            );
        }
        Ok(())
    }

    pub fn load_proxy_plist() -> Result<()> {
        bootout_bootstrap(PROXY_LABEL, "proxy")
    }

    pub fn load_dashboard_plist() -> Result<()> {
        let p = plist_path(DASHBOARD_LABEL);
        let domain = format!("gui/{}", uid());
        let _ = std::process::Command::new("launchctl")
            .args(["bootout", &domain, p.to_str().unwrap_or("")])
            .status();

        let status = std::process::Command::new("launchctl")
            .args(["bootstrap", &domain, p.to_str().unwrap_or("")])
            .status()
            .context("launchctl bootstrap failed for dashboard")?;

        if status.success() {
            println!("  launchd dashboard bootstrapped ({DASHBOARD_LABEL})");
        } else {
            eprintln!("  Warning: dashboard launchd failed to start. Run `ctx dashboard` manually.");
        }
        Ok(())
    }

    pub fn load_ingest_plist() -> Result<()> {
        let p = plist_path(INGEST_LABEL);
        let domain = format!("gui/{}", uid());
        let _ = std::process::Command::new("launchctl")
            .args(["bootout", &domain, p.to_str().unwrap_or("")])
            .status();

        let status = std::process::Command::new("launchctl")
            .args(["bootstrap", &domain, p.to_str().unwrap_or("")])
            .status()
            .context("launchctl bootstrap failed for ingest")?;

        if !status.success() {
            eprintln!("  Warning: ingest launchd failed to start. Run `ctx ingest` manually.");
        }
        Ok(())
    }

    pub fn uninstall_launchd_agents() -> Result<()> {
        let domain = format!("gui/{}", uid());
        for (label, name) in [
            (PROXY_LABEL, "proxy"),
            (DASHBOARD_LABEL, "dashboard"),
            (INGEST_LABEL, "ingest"),
        ] {
            let p = plist_path(label);
            if p.exists() {
                let _ = std::process::Command::new("launchctl")
                    .args(["bootout", &domain, p.to_str().unwrap_or("")])
                    .status();
                let _ = std::fs::remove_file(&p);
                println!("Removed {name} launchd agent");
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Linux systemd --user
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
mod linux {
    use super::*;

    fn systemd_user_dir() -> Option<PathBuf> {
        let mut d = dirs::config_dir()?;
        d.push("systemd");
        d.push("user");
        Some(d)
    }

    fn write_unit(path: &PathBuf, body: &str) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, body)?;
        println!("  Written {}", path.display());
        Ok(())
    }

    pub fn write_proxy_unit(port: u16, upstream: &str) -> Result<()> {
        let bin = ctx_binary();
        let ca = crate::ca::canonical_ca_cert_path_string().unwrap_or_default();
        let home = home_display();
        let path = systemd_user_dir()
            .ok_or_else(|| anyhow::anyhow!("no XDG config dir for systemd user units"))?
            .join("ctx-proxy.service");
        let body = format!(
            r#"[Unit]
Description=ctx MITM proxy for Anthropic API
After=network-online.target

[Service]
Type=simple
ExecStart={bin} proxy start --port {port} --upstream {upstream}
Restart=always
WorkingDirectory={home}
Environment=HOME={home}
Environment=PATH=/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin:{cbin}
Environment=NODE_EXTRA_CA_CERTS={ca}

[Install]
WantedBy=default.target
"#,
            cbin = cargo_bin_display(),
        );
        write_unit(&path, &body)
    }

    pub fn write_dashboard_unit(port: u16) -> Result<()> {
        let bin = ctx_binary();
        let home = home_display();
        let path = systemd_user_dir()
            .ok_or_else(|| anyhow::anyhow!("no XDG config dir for systemd user units"))?
            .join("ctx-dashboard.service");
        let body = format!(
            r#"[Unit]
Description=ctx dashboard

[Service]
Type=simple
ExecStart={bin} dashboard --port {port} --no-open
Restart=always
WorkingDirectory={home}
Environment=HOME={home}
Environment=PATH=/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin:{cbin}

[Install]
WantedBy=default.target
"#,
            cbin = cargo_bin_display(),
        );
        write_unit(&path, &body)
    }

    pub fn write_ingest_unit_and_timer() -> Result<()> {
        let bin = ctx_binary();
        let home = home_display();
        let dir = systemd_user_dir().ok_or_else(|| anyhow::anyhow!("no XDG config dir"))?;
        let svc = dir.join("ctx-ingest.service");
        let timer = dir.join("ctx-ingest.timer");
        let svc_body = format!(
            r#"[Unit]
Description=ctx ingest (Claude Code + Desktop JSONL)

[Service]
Type=oneshot
ExecStart={bin} ingest
WorkingDirectory={home}
Environment=HOME={home}
Environment=PATH=/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin:{cbin}
"#,
            cbin = cargo_bin_display(),
        );
        let timer_body = r#"[Unit]
Description=Run ctx ingest every 5 minutes

[Timer]
OnBootSec=2min
OnUnitActiveSec=5min
Unit=ctx-ingest.service

[Install]
WantedBy=timers.target
"#;
        write_unit(&svc, &svc_body)?;
        write_unit(&timer, timer_body)?;
        Ok(())
    }

    pub fn daemon_reload_and_enable(units: &[&str]) -> Result<()> {
        let st = std::process::Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .status()
            .context("systemctl --user daemon-reload")?;
        if !st.success() {
            anyhow::bail!("systemctl --user daemon-reload failed (is systemd available?)");
        }
        for u in units {
            let st = std::process::Command::new("systemctl")
                .args(["--user", "enable", "--now", u])
                .status()
                .with_context(|| format!("systemctl enable --now {u}"))?;
            if !st.success() {
                anyhow::bail!("systemctl --user enable --now {u} failed");
            }
            println!("  systemd --user: enabled {u}");
        }
        Ok(())
    }

    pub fn uninstall_systemd_units() -> Result<()> {
        let _ = std::process::Command::new("systemctl")
            .args([
                "--user",
                "disable",
                "--now",
                "ctx-proxy.service",
                "ctx-dashboard.service",
                "ctx-ingest.timer",
            ])
            .status();
        if let Some(dir) = systemd_user_dir() {
            for f in [
                "ctx-proxy.service",
                "ctx-dashboard.service",
                "ctx-ingest.service",
                "ctx-ingest.timer",
            ] {
                let p = dir.join(f);
                if p.exists() {
                    let _ = std::fs::remove_file(&p);
                    println!("Removed {}", p.display());
                }
            }
        }
        let _ = std::process::Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .status();
        Ok(())
    }
}
