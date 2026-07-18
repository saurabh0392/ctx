//! Background services: launchd (macOS), systemd user units (Linux), or detached `ctx` children (other OS).

use anyhow::{Context, Result};
// The colored `.yellow()` output lives only in the Linux and generic-fallback cfg branches (macOS and
// Windows have their own service arms that do not use it), so gate the import to match and avoid an
// unused-import warning on those platforms.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
use colored::Colorize;
use std::path::PathBuf;

pub const DASHBOARD_LABEL: &str = "com.ctx.dashboard";
pub const INGEST_LABEL: &str = "com.ctx.ingest";
pub const EXPERIMENT_TICK_LABEL: &str = "com.ctx.experiment-tick";

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

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn home_display() -> String {
    dirs::home_dir()
        .map(|h| h.display().to_string())
        .unwrap_or_default()
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn cargo_bin_display() -> String {
    dirs::home_dir()
        .map(|h| h.join(".cargo").join("bin").display().to_string())
        .unwrap_or_default()
}

/// Preview line for setup: ctx runs hook-first with a local dashboard, no proxy.
pub fn dashboard_ingest_summary(dashboard_port: u16, periodic_ingest: bool) -> String {
    let ingest_tail = if periodic_ingest {
        ", periodic ingest (every 5 min)"
    } else {
        ""
    };
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        format!("Install launchd/systemd: ctx dashboard (:{dashboard_port}){ingest_tail}")
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        format!("Start ctx dashboard (:{dashboard_port}) in the background{ingest_tail}")
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

pub fn install_dashboard(port: u16) -> Result<()> {
    #[cfg(target_os = "macos")]
    return macos::write_dashboard_plist(port);
    #[cfg(target_os = "linux")]
    return linux::write_dashboard_unit(port);
    #[cfg(target_os = "windows")]
    return windows::install_dashboard_task(port);
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = port;
        Ok(())
    }
}

pub fn bootstrap_dashboard(dashboard_port: u16) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let _ = dashboard_port;
        macos::load_dashboard_plist()
    }
    #[cfg(target_os = "linux")]
    return linux::daemon_reload_and_enable(&["ctx-dashboard.service"]);
    #[cfg(target_os = "windows")]
    return windows::bootstrap_dashboard_task(dashboard_port);
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
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
    #[cfg(target_os = "windows")]
    return windows::install_ingest_task();
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
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
    #[cfg(target_os = "windows")]
    return windows::bootstrap_ingest_task();
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Ok(())
    }
}

pub fn install_experiment_tick() -> Result<()> {
    #[cfg(target_os = "macos")]
    return macos::write_experiment_tick_plist();
    #[cfg(target_os = "windows")]
    return windows::install_experiment_tick_task();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        println!(
            "  {} Daily experiment tick is not auto-installed on this OS.",
            "i".yellow()
        );
        println!("  Add a cron job: 0 9 * * * $(which ctx) experiment tick");
        Ok(())
    }
}

pub fn uninstall_all() -> Result<()> {
    #[cfg(target_os = "macos")]
    return macos::uninstall_launchd_agents();
    #[cfg(target_os = "linux")]
    return linux::uninstall_systemd_units();
    #[cfg(target_os = "windows")]
    return windows::uninstall_scheduled_tasks();
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        println!(
            "  {} Stop any `ctx dashboard` terminals you opened for ctx.",
            "i".yellow()
        );
        Ok(())
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn try_spawn_dashboard(port: u16) -> Result<()> {
    let bin = ctx_binary();
    let _ = std::process::Command::new(&bin)
        .args(["dashboard", "--port", &port.to_string(), "--no-open"])
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

    pub fn write_experiment_tick_plist() -> Result<()> {
        let bin = ctx_binary();
        let log_dir = crate::config::ctx_dir();
        std::fs::create_dir_all(&log_dir)?;
        let stdout_log = log_dir.join("experiment-tick.stdout.log");
        let stderr_log = log_dir.join("experiment-tick.stderr.log");

        let plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{EXPERIMENT_TICK_LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{bin}</string>
        <string>experiment</string>
        <string>tick</string>
    </array>
    <key>StartCalendarInterval</key>
    <dict>
        <key>Hour</key>
        <integer>9</integer>
        <key>Minute</key>
        <integer>0</integer>
    </dict>
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

        let p = plist_path(EXPERIMENT_TICK_LABEL);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&p, &plist)?;
        println!("  Written {}", p.display());

        let domain = format!("gui/{}", uid());
        let _ = std::process::Command::new("launchctl")
            .args(["bootout", &domain, p.to_str().unwrap_or("")])
            .status();
        let status = std::process::Command::new("launchctl")
            .args(["bootstrap", &domain, p.to_str().unwrap_or("")])
            .status()
            .context("launchctl bootstrap failed for experiment tick")?;
        if !status.success() {
            eprintln!("  Warning: experiment tick launchd failed to load. Run `ctx experiment tick` manually.");
        } else {
            println!("  ✓ Daily experiment tick scheduled for 09:00");
        }
        Ok(())
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
            eprintln!(
                "  Warning: dashboard launchd failed to start. Run `ctx dashboard` manually."
            );
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
            (DASHBOARD_LABEL, "dashboard"),
            (INGEST_LABEL, "ingest"),
            (EXPERIMENT_TICK_LABEL, "experiment-tick"),
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
                "ctx-dashboard.service",
                "ctx-ingest.timer",
            ])
            .status();
        if let Some(dir) = systemd_user_dir() {
            for f in [
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

// ---------------------------------------------------------------------------
// Windows Scheduled Tasks
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
mod windows {
    use super::*;

    // Per-user Task Scheduler names. Kept flat (not the reverse-DNS launchd labels) since schtasks
    // /TN is a plain name or a "\Folder\Name" path.
    const DASHBOARD_TASK: &str = "ctx-dashboard";
    const INGEST_TASK: &str = "ctx-ingest";
    const EXPERIMENT_TICK_TASK: &str = "ctx-experiment-tick";

    /// Build the `/TR` value: the exe path in escaped inner quotes (so a path with spaces parses as
    /// one token) followed by the ctx subcommand. std::process::Command adds the outer quoting.
    fn task_run(bin: &str, args: &str) -> String {
        format!("\"{bin}\" {args}")
    }

    fn schtasks(args: &[&str]) -> Result<std::process::ExitStatus> {
        std::process::Command::new("schtasks")
            .args(args)
            .status()
            .context("schtasks not found (Windows Task Scheduler CLI)")
    }

    pub fn install_dashboard_task(port: u16) -> Result<()> {
        let bin = ctx_binary();
        // Runs at each logon and stays up; the ctx dashboard is a long-lived server. `/RL LIMITED`
        // avoids an elevation prompt. Runs in the interactive user session (no stored credentials).
        let tr = task_run(&bin, &format!("dashboard --port {port} --no-open"));
        let st = schtasks(&[
            "/Create",
            "/F",
            "/SC",
            "ONLOGON",
            "/RL",
            "LIMITED",
            "/TN",
            DASHBOARD_TASK,
            "/TR",
            &tr,
        ])?;
        if !st.success() {
            anyhow::bail!("schtasks failed to create the ctx-dashboard task");
        }
        println!("  Registered scheduled task {DASHBOARD_TASK}");
        Ok(())
    }

    pub fn bootstrap_dashboard_task(_port: u16) -> Result<()> {
        // Start it now so the dashboard is up without waiting for the next logon.
        let st = schtasks(&["/Run", "/TN", DASHBOARD_TASK])?;
        if !st.success() {
            eprintln!(
                "  Warning: could not start the ctx-dashboard task. Run `ctx dashboard` manually."
            );
        }
        Ok(())
    }

    pub fn install_ingest_task() -> Result<()> {
        let bin = ctx_binary();
        let tr = task_run(&bin, "ingest");
        let st = schtasks(&[
            "/Create",
            "/F",
            "/SC",
            "MINUTE",
            "/MO",
            "5",
            "/RL",
            "LIMITED",
            "/TN",
            INGEST_TASK,
            "/TR",
            &tr,
        ])?;
        if !st.success() {
            anyhow::bail!("schtasks failed to create the ctx-ingest task");
        }
        println!("  Registered scheduled task {INGEST_TASK} (every 5 min)");
        Ok(())
    }

    pub fn bootstrap_ingest_task() -> Result<()> {
        let _ = schtasks(&["/Run", "/TN", INGEST_TASK]);
        Ok(())
    }

    pub fn install_experiment_tick_task() -> Result<()> {
        let bin = ctx_binary();
        let tr = task_run(&bin, "experiment tick");
        let st = schtasks(&[
            "/Create",
            "/F",
            "/SC",
            "DAILY",
            "/ST",
            "09:00",
            "/RL",
            "LIMITED",
            "/TN",
            EXPERIMENT_TICK_TASK,
            "/TR",
            &tr,
        ])?;
        if !st.success() {
            eprintln!("  Warning: daily experiment tick task failed to register.");
        } else {
            println!("  ✓ Daily experiment tick scheduled for 09:00");
        }
        Ok(())
    }

    pub fn uninstall_scheduled_tasks() -> Result<()> {
        for (task, name) in [
            (DASHBOARD_TASK, "dashboard"),
            (INGEST_TASK, "ingest"),
            (EXPERIMENT_TICK_TASK, "experiment-tick"),
        ] {
            let st = schtasks(&["/Delete", "/TN", task, "/F"]);
            if matches!(st, Ok(s) if s.success()) {
                println!("Removed {name} scheduled task");
            }
        }
        Ok(())
    }
}
