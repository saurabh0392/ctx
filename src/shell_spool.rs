//! Sandbox-safe recovery storage for shell trims.
//!
//! Codex executes a command rewritten by `PreToolUse` inside its filesystem sandbox. That child can
//! read CTX configuration, but it cannot create SQLite WAL/SHM files beside `~/.ctx/ctx.db`.
//! Recovery therefore crosses the sandbox boundary through a private, atomic file in the operating
//! system's temporary directory. The trusted `PostToolUse` hook imports the file into the normal
//! database before deleting it.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const SPOOL_VERSION: u32 = 1;
const MAX_SPOOL_BYTES: u64 = 64 * 1024 * 1024;
const STALE_AFTER: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const MARKER_ID_PREFIX: &str = "Full original id: ";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ShellTrimSpool {
    pub version: u32,
    pub rewind_id: String,
    pub prepared_at: String,
    pub session_id: Option<String>,
    pub tool_name: String,
    pub command: String,
    pub command_or_path: String,
    pub original: String,
    pub trimmed: String,
    pub strategy: String,
    pub chars_in: usize,
    pub chars_out: usize,
    pub lines_in: usize,
    pub lines_out: usize,
    pub surface: String,
    pub cwd: String,
}

impl ShellTrimSpool {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        command: &str,
        original: &str,
        trimmed: String,
        prepared_at: String,
        session_id: Option<String>,
        command_or_path: String,
        strategy: String,
        surface: String,
        cwd: String,
    ) -> Self {
        Self {
            version: SPOOL_VERSION,
            rewind_id: rewind_id(command, original.as_bytes()),
            prepared_at,
            session_id,
            tool_name: "Shell".to_string(),
            command: command.to_string(),
            command_or_path,
            chars_in: original.chars().count(),
            chars_out: trimmed.chars().count(),
            lines_in: original.lines().count(),
            lines_out: trimmed.lines().count(),
            original: original.to_string(),
            trimmed,
            strategy,
            surface,
            cwd,
        }
    }
}

pub(crate) fn rewind_id(command: &str, stdout: &[u8]) -> String {
    let mut hash = Sha256::new();
    hash.update(b"ctx-shell-rewind-v1\0");
    hash.update(command.as_bytes());
    hash.update([0]);
    hash.update(stdout);
    format!("shell-{:x}", hash.finalize())
}

pub(crate) fn id_from_text(text: &str) -> Option<String> {
    let start = text.rfind(MARKER_ID_PREFIX)? + MARKER_ID_PREFIX.len();
    let candidate = text.get(start..start + 70)?;
    valid_rewind_id(candidate).then(|| candidate.to_string())
}

pub(crate) fn write(record: &ShellTrimSpool) -> Result<()> {
    write_in(&spool_dir(), record)
}

pub(crate) fn load(id: &str) -> Result<Option<ShellTrimSpool>> {
    load_in(&spool_dir(), id)
}

pub(crate) fn remove(id: &str) -> Result<()> {
    if !valid_rewind_id(id) {
        anyhow::bail!("invalid shell rewind id");
    }
    let path = spool_dir().join(format!("{id}.json"));
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("remove {}", path.display())),
    }
}

fn spool_dir() -> PathBuf {
    #[cfg(test)]
    if let Ok(path) = std::env::var("CTX_TEST_SHELL_SPOOL") {
        return PathBuf::from(path);
    }
    std::env::temp_dir().join("ctx-shell-recovery")
}

fn write_in(directory: &Path, record: &ShellTrimSpool) -> Result<()> {
    validate(record, Some(&record.rewind_id))?;
    ensure_private_directory(directory)?;
    prune_stale_in(directory);

    let final_path = directory.join(format!("{}.json", record.rewind_id));
    if final_path.exists() {
        let existing = load_in(directory, &record.rewind_id)?
            .context("existing shell recovery spool disappeared")?;
        if existing == *record {
            return Ok(());
        }
        anyhow::bail!(
            "a different recovery receipt already exists for {}",
            record.rewind_id
        );
    }

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temp_path = directory.join(format!(
        ".{}.{}.{}.tmp",
        record.rewind_id,
        std::process::id(),
        nonce
    ));
    let encoded = serde_json::to_vec(record)?;
    if encoded.len() as u64 > MAX_SPOOL_BYTES {
        anyhow::bail!("shell recovery receipt exceeds the spool size limit");
    }

    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temp_path)
        .with_context(|| format!("create {}", temp_path.display()))?;
    let write_result = (|| -> Result<()> {
        file.write_all(&encoded)?;
        file.sync_all()?;
        crate::config::protect_private_file(&temp_path)?;
        fs::rename(&temp_path, &final_path)
            .with_context(|| format!("publish {}", final_path.display()))?;
        crate::config::protect_private_file(&final_path)?;
        Ok(())
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    write_result
}

fn load_in(directory: &Path, id: &str) -> Result<Option<ShellTrimSpool>> {
    if !valid_rewind_id(id) {
        anyhow::bail!("invalid shell rewind id");
    }
    let path = directory.join(format!("{id}.json"));
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("inspect {}", path.display())),
    };
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        anyhow::bail!("shell recovery receipt is not a regular file");
    }
    if metadata.len() > MAX_SPOOL_BYTES {
        anyhow::bail!("shell recovery receipt exceeds the spool size limit");
    }

    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    File::open(&path)
        .with_context(|| format!("open {}", path.display()))?
        .take(MAX_SPOOL_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_SPOOL_BYTES {
        anyhow::bail!("shell recovery receipt exceeds the spool size limit");
    }
    let record: ShellTrimSpool =
        serde_json::from_slice(&bytes).context("parse shell recovery receipt")?;
    validate(&record, Some(id))?;
    Ok(Some(record))
}

fn validate(record: &ShellTrimSpool, expected_id: Option<&str>) -> Result<()> {
    if record.version != SPOOL_VERSION {
        anyhow::bail!("unsupported shell recovery receipt version");
    }
    if !valid_rewind_id(&record.rewind_id)
        || expected_id.is_some_and(|expected| expected != record.rewind_id)
    {
        anyhow::bail!("shell recovery receipt id mismatch");
    }
    if rewind_id(&record.command, record.original.as_bytes()) != record.rewind_id {
        anyhow::bail!("shell recovery receipt content hash mismatch");
    }
    if record.tool_name != "Shell"
        || crate::surface::SurfaceId::parse(&record.surface).is_none()
        || record.command.trim().is_empty()
        || record.strategy.trim().is_empty()
        || chrono::DateTime::parse_from_rfc3339(&record.prepared_at).is_err()
    {
        anyhow::bail!("invalid shell recovery receipt metadata");
    }
    let expected_marker = crate::compress::trim_marker(&record.rewind_id);
    if !record.trimmed.ends_with(&expected_marker)
        || record.chars_in != record.original.chars().count()
        || record.chars_out != record.trimmed.chars().count()
        || record.lines_in != record.original.lines().count()
        || record.lines_out != record.trimmed.lines().count()
        || record.chars_out >= record.chars_in
    {
        anyhow::bail!("invalid shell recovery receipt counts or marker");
    }
    Ok(())
}

fn valid_rewind_id(id: &str) -> bool {
    id.len() == 70
        && id.starts_with("shell-")
        && id[6..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn ensure_private_directory(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
                anyhow::bail!("shell recovery spool path is not a private directory");
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(path).with_context(|| format!("create {}", path.display()))?;
        }
        Err(error) => return Err(error).with_context(|| format!("inspect {}", path.display())),
    }
    crate::config::protect_private_directory(path)
}

fn prune_stale_in(directory: &Path) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let now = SystemTime::now();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !(name.ends_with(".json") || name.ends_with(".tmp")) {
            continue;
        }
        let stale = entry
            .metadata()
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age > STALE_AFTER);
        if stale {
            let _ = fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ShellTrimSpool {
        let command = "rg needle docs";
        let original = "needle\n".repeat(2_000);
        let id = rewind_id(command, original.as_bytes());
        let trimmed = format!("needle\n{}", crate::compress::trim_marker(&id));
        ShellTrimSpool::new(
            command,
            &original,
            trimmed,
            chrono::Utc::now().to_rfc3339(),
            Some("session-1".into()),
            command.into(),
            "grep".into(),
            "codex".into(),
            "/work/repo".into(),
        )
    }

    #[test]
    fn private_atomic_spool_round_trips_exact_original() {
        let temp = tempfile::tempdir().unwrap();
        let record = sample();
        write_in(temp.path(), &record).unwrap();
        let loaded = load_in(temp.path(), &record.rewind_id).unwrap().unwrap();
        assert_eq!(loaded, record);
        assert_eq!(
            id_from_text(&loaded.trimmed).as_deref(),
            Some(loaded.rewind_id.as_str())
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(temp.path()).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                fs::metadata(temp.path().join(format!("{}.json", loaded.rewind_id)))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn tampered_original_is_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let record = sample();
        write_in(temp.path(), &record).unwrap();
        let path = temp.path().join(format!("{}.json", record.rewind_id));
        let mut value = serde_json::to_value(&record).unwrap();
        value["original"] = serde_json::json!("tampered");
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(load_in(temp.path(), &record.rewind_id).is_err());
    }

    #[test]
    fn rejects_path_like_rewind_ids() {
        assert!(load_in(Path::new("/tmp"), "../../ctx.db").is_err());
        assert!(id_from_text("Full original id: ../../ctx.db").is_none());
    }
}
