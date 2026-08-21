//! Self-updater backed by public GitHub releases, with checksum verification and staged swap.

use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::time::Duration;

const REPO: &str = "saurabh0392/ctx";

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    assets: Vec<GithubAsset>,
}

struct ReleaseInfo {
    version: String,
    url: String,
    sha256: String,
}

fn target() -> Result<&'static str> {
    match (std::env::consts::ARCH, std::env::consts::OS) {
        ("aarch64", "macos") => Ok("aarch64-apple-darwin"),
        ("x86_64", "macos") => Ok("x86_64-apple-darwin"),
        ("x86_64", "linux") => Ok("x86_64-unknown-linux-gnu"),
        (_, "windows") => bail!("in-place update is experimental on Windows; rerun install.ps1"),
        (arch, os) => bail!("no prebuilt release for {arch}-{os}; use `cargo install ctx-agent`"),
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

fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(concat!("ctx/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(30))
        .build()?)
}

async fn latest_release() -> Result<ReleaseInfo> {
    let asset_name = format!("ctx-{}.tar.gz", target()?);
    let release: GithubRelease = client()?
        .get(format!(
            "https://api.github.com/repos/{REPO}/releases/latest"
        ))
        .send()
        .await
        .context("contact GitHub releases")?
        .error_for_status()
        .context("GitHub releases request")?
        .json()
        .await
        .context("parse GitHub release")?;
    if semver_tuple(&release.tag_name).is_none() {
        bail!("latest release tag is not a version: {}", release.tag_name);
    }
    let asset = release
        .assets
        .iter()
        .find(|a| a.name == asset_name)
        .with_context(|| {
            format!("release {} has no {asset_name}; use `cargo install ctx-agent` or `brew upgrade ctx`", release.tag_name)
        })?;
    let checksums_url = release
        .assets
        .iter()
        .find(|a| a.name == "checksums.txt")
        .context("release has no checksums.txt")?
        .browser_download_url
        .clone();
    let checksums = client()?
        .get(checksums_url)
        .send()
        .await
        .context("download checksums.txt")?
        .error_for_status()?
        .text()
        .await?;
    let sha256 = checksums
        .lines()
        .find_map(|line| {
            let mut parts = line.split_whitespace();
            let hash = parts.next()?;
            let name = parts.next()?;
            (name == asset_name).then(|| hash.to_string())
        })
        .context("checksums.txt has no entry for this target")?;
    if sha256.len() != 64 || !sha256.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!("release checksum is not a valid SHA-256");
    }
    Ok(ReleaseInfo {
        version: release.tag_name,
        url: asset.browser_download_url.clone(),
        sha256,
    })
}

pub async fn run(check_only: bool) -> Result<()> {
    let release = latest_release().await?;
    let current = env!("CARGO_PKG_VERSION");
    let newer = match (semver_tuple(&release.version), semver_tuple(current)) {
        (Some(remote), Some(local)) => remote > local,
        _ => release.version.trim_start_matches('v') != current,
    };
    if !newer {
        println!("ctx {current} is current.");
        return Ok(());
    }
    println!(
        "ctx {} is available (installed: {current}).",
        release.version
    );
    if check_only {
        println!("Upgrade with `brew upgrade ctx`, `cargo install ctx-agent`, or `ctx update`.");
        return Ok(());
    }

    let current_path = std::env::current_exe()?.canonicalize()?;
    if current_path.components().any(|c| c.as_os_str() == "Cellar") {
        bail!("this ctx was installed with Homebrew; run `brew upgrade ctx` instead");
    }

    let updates = crate::config::ctx_dir().join("updates");
    std::fs::create_dir_all(&updates)?;
    let archive = updates.join(format!("ctx-{}.tar.gz", release.version));
    let mut file = std::fs::File::create(&archive)?;
    let response = reqwest::Client::builder()
        .user_agent(concat!("ctx/", env!("CARGO_PKG_VERSION")))
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
    let _ = std::fs::remove_dir_all(&extract_dir);
    let _ = std::fs::remove_file(&archive);

    println!(
        "Updated ctx to {}. Re-wiring hooks and restarting services...",
        release.version
    );
    let setup_status = std::process::Command::new(&current_path)
        .args(["setup", "--yes"])
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
            "setup refresh failed and automatic rollback could not restore {}; reinstall via brew or cargo",
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
        assert_eq!(semver_tuple("0.5.2-rc.1"), Some((0, 5, 2)));
        assert!(semver_tuple("0.5.2") > semver_tuple("0.5.1"));
    }
}
