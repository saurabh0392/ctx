//! Capability-authenticated binary updater for the unsigned Mac/Linux beta.

use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::time::Duration;

#[derive(Debug, Deserialize)]
struct ReleaseResponse {
    version: String,
    url: String,
    sha256: String,
    #[serde(default)]
    credential: Option<String>,
}

fn target() -> Result<&'static str> {
    match (std::env::consts::ARCH, std::env::consts::OS) {
        ("aarch64", "macos") => Ok("aarch64-apple-darwin"),
        ("x86_64", "macos") => Ok("x86_64-apple-darwin"),
        ("x86_64", "linux") => Ok("x86_64-unknown-linux-gnu"),
        (_, "windows") => bail!("in-place update is experimental on Windows; rerun install.ps1"),
        (arch, os) => bail!("no beta build for {arch}-{os}"),
    }
}

fn semver_tuple(value: &str) -> Option<(u64, u64, u64)> {
    let core = value.trim_start_matches('v').split('-').next()?;
    let p: Vec<_> = core.split('.').collect();
    if p.len() != 3 {
        return None;
    }
    Some((p[0].parse().ok()?, p[1].parse().ok()?, p[2].parse().ok()?))
}

fn require_secure_url(value: &str, label: &str) -> Result<()> {
    let url = reqwest::Url::parse(value).with_context(|| format!("parse {label} URL"))?;
    let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "::1"));
    if url.scheme() != "https" && !(url.scheme() == "http" && loopback) {
        bail!("{label} must use HTTPS (HTTP is allowed only for loopback testing)");
    }
    Ok(())
}

async fn release(state: &crate::beta::BetaState) -> Result<ReleaseResponse> {
    if state.credential.is_empty() {
        bail!("beta capability is missing; reinstall with your current invite");
    }
    require_secure_url(&state.dist_endpoint, "distribution endpoint")?;
    let response = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?
        .post(&state.dist_endpoint)
        .json(&serde_json::json!({
            "credential": state.credential,
            "target": target()?,
        }))
        .send()
        .await
        .context("contact ctx distribution service")?;
    let status = response.status();
    if !status.is_success() {
        let detail = response.text().await.unwrap_or_default();
        bail!("distribution service returned {status}: {detail}");
    }
    let release: ReleaseResponse = response.json().await.context("parse release manifest")?;
    if semver_tuple(&release.version).is_none() {
        bail!("distribution service returned an invalid release version");
    }
    if release.sha256.len() != 64 || !release.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!("distribution service returned an invalid SHA-256");
    }
    require_secure_url(&release.url, "binary download")?;
    if let Some(credential) = &release.credential {
        let (participant, expiry) = crate::beta::capability_details(credential)
            .context("distribution service returned a malformed capability")?;
        if participant != state.participant_id || expiry <= chrono::Utc::now() {
            bail!("distribution service returned a capability for the wrong participant or expiry");
        }
    }
    Ok(release)
}

pub async fn run(check_only: bool) -> Result<()> {
    let state = crate::beta::load_state().context(
        "ctx update is available to token-gated beta installs; run the beta installer first",
    )?;
    let release = release(&state).await?;
    let current = env!("CARGO_PKG_VERSION");
    let newer = match (semver_tuple(&release.version), semver_tuple(current)) {
        (Some(remote), Some(local)) => remote > local,
        _ => release.version != current,
    };
    if !newer {
        println!("ctx {current} is current (beta channel).");
        return Ok(());
    }
    println!(
        "ctx {} is available (installed: {current}).",
        release.version
    );
    if check_only {
        return Ok(());
    }

    let updates = crate::config::ctx_dir().join("updates");
    std::fs::create_dir_all(&updates)?;
    let archive = updates.join(format!("ctx-{}.tar.gz", release.version));
    let mut file = std::fs::File::create(&archive)?;
    let response = reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .build()?
        .get(&release.url)
        .send()
        .await
        .context("download ctx update")?;
    if !response.status().is_success() {
        bail!("binary download returned {}", response.status());
    }
    let mut stream = response.bytes_stream();
    let mut hasher = Sha256::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        hasher.update(&chunk);
        file.write_all(&chunk)?;
    }
    file.sync_all()?;
    let got = format!("{:x}", hasher.finalize());
    if got != release.sha256.to_lowercase() {
        let _ = std::fs::remove_file(&archive);
        bail!(
            "checksum mismatch (expected {}, got {got}); the installed binary was not changed",
            release.sha256
        );
    }

    let extract_dir = updates.join(format!(
        "extract-{}-{}",
        std::process::id(),
        UtcStamp::now()
    ));
    std::fs::create_dir_all(&extract_dir)?;
    let listing = std::process::Command::new("tar")
        .args(["-tzf"])
        .arg(&archive)
        .output()
        .context("inspect update archive")?;
    let listing_text = String::from_utf8_lossy(&listing.stdout);
    let entries: Vec<_> = listing_text
        .lines()
        .map(|line| line.trim_start_matches("./"))
        .filter(|line| !line.is_empty())
        .collect();
    if !listing.status.success() || entries != ["ctx"] {
        bail!("update archive must contain exactly one regular ctx binary");
    }
    let status = std::process::Command::new("tar")
        .args(["-xzf"])
        .arg(&archive)
        .arg("-C")
        .arg(&extract_dir)
        .status()
        .context("run tar to extract update")?;
    if !status.success() {
        bail!("could not extract update; the installed binary was not changed");
    }
    let candidate = extract_dir.join("ctx");
    if !std::fs::symlink_metadata(&candidate)
        .map(|metadata| metadata.file_type().is_file())
        .unwrap_or(false)
    {
        bail!("update archive did not contain ctx; the installed binary was not changed");
    }

    let current_path = std::env::current_exe()?.canonicalize()?;
    let parent = current_path
        .parent()
        .context("installed binary has no parent")?;
    let staged = parent.join(".ctx-update-new");
    let backup = parent.join(".ctx-update-previous");
    std::fs::copy(&candidate, &staged).with_context(|| {
        format!(
            "stage update beside {}; check directory permissions",
            current_path.display()
        )
    })?;
    std::fs::set_permissions(&staged, std::fs::metadata(&current_path)?.permissions())?;
    let staged_version = std::process::Command::new(&staged)
        .arg("--version")
        .output()
        .context("start staged update")?;
    let version_text = String::from_utf8_lossy(&staged_version.stdout);
    if !staged_version.status.success()
        || !version_text.contains(release.version.trim_start_matches('v'))
    {
        let _ = std::fs::remove_file(&staged);
        bail!(
            "staged binary did not report version {}; the installed binary was not changed",
            release.version
        );
    }
    if backup.exists() {
        std::fs::remove_file(&backup)?;
    }
    std::fs::rename(&current_path, &backup)?;
    if let Err(e) = std::fs::rename(&staged, &current_path) {
        let _ = std::fs::rename(&backup, &current_path);
        bail!("could not activate update ({e}); restored the previous binary");
    }
    if let Some(credential) = release.credential {
        if let Err(error) = crate::beta::refresh_credential(&credential) {
            eprintln!("warning: update installed, but capability rotation was not saved: {error}");
        }
    }
    let _ = std::fs::remove_dir_all(&extract_dir);
    let _ = std::fs::remove_file(&archive);

    println!(
        "Updated ctx to {}. Re-wiring hooks and restarting services...",
        release.version
    );
    let setup_status = std::process::Command::new(&current_path)
        .args(["setup", "--beta", "--yes"])
        .status();
    if !matches!(setup_status, Ok(status) if status.success()) {
        let failed = parent.join(".ctx-update-failed");
        let _ = std::fs::remove_file(&failed);
        let _ = std::fs::rename(&current_path, &failed);
        let restored = std::fs::rename(&backup, &current_path).is_ok();
        let _ = std::fs::remove_file(&failed);
        if restored {
            bail!("setup refresh failed; restored ctx {current}");
        }
        bail!(
            "setup refresh failed and automatic rollback could not restore {}; reinstall with your beta invite",
            current_path.display(),
        );
    }
    let _ = std::fs::remove_file(&backup);
    println!("ctx {} is ready.", release.version);
    Ok(())
}

struct UtcStamp;

impl UtcStamp {
    fn now() -> i64 {
        chrono::Utc::now().timestamp_millis()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_comparison_ignores_v_and_prerelease() {
        assert_eq!(semver_tuple("v0.5.2"), Some((0, 5, 2)));
        assert_eq!(semver_tuple("0.5.2-beta.1"), Some((0, 5, 2)));
        assert!(semver_tuple("0.5.2") > semver_tuple("0.5.1"));
    }
}
