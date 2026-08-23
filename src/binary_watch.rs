//! Notice when the ctx binary underneath a running service has been replaced.
//!
//! Upgrading swaps the file on disk; it does not touch processes already running, which keep
//! executing the old inode until something restarts them. `ctx update` handles that itself, but it
//! deliberately defers to Homebrew for brew installs, and `brew upgrade ctx` has no idea ctx has
//! background services. The result is an upgrade that reports success while the dashboard and the
//! model gateways keep serving the previous version indefinitely.
//!
//! The supervisors already know how to fix this: the launchd plists and systemd units set
//! KeepAlive/Restart and invoke ctx through a stable path (`/opt/homebrew/bin/ctx`, not the Cellar
//! path it resolves to). So for a supervised service, exiting *is* upgrading. All that is missing
//! is something to notice and exit.
//!
//! Watching identity rather than the install method keeps this honest for every way ctx gets
//! updated: Homebrew retargets a symlink, `cargo install` rewrites the file, the curl installer
//! renames over it. Each one changes the inode the launch path resolves to.

use std::path::{Path, PathBuf};
use std::time::Duration;

/// How often to look. Upgrades are rare and a minute of staleness costs nothing, so this is set to
/// stay invisible in `top` rather than to react fast.
const POLL: Duration = Duration::from_secs(30);

/// A replacement is only acted on once it looks finished. An installer that writes in place can be
/// observed mid-write, and restarting onto a half-written binary is worse than staying stale.
const SETTLE: Duration = Duration::from_secs(2);

/// Identity of the file a launch path resolves to. Inode covers the symlink swap and the rename;
/// length and mtime cover a rewrite that happens to reuse the inode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stamp {
    dev: u64,
    ino: u64,
    len: u64,
    mtime: Option<std::time::SystemTime>,
}

impl Stamp {
    pub fn of(path: &Path) -> Option<Stamp> {
        let meta = std::fs::metadata(path).ok()?;
        Some(Stamp::from_meta(&meta))
    }

    #[cfg(unix)]
    fn from_meta(meta: &std::fs::Metadata) -> Stamp {
        use std::os::unix::fs::MetadataExt;
        Stamp {
            dev: meta.dev(),
            ino: meta.ino(),
            len: meta.len(),
            mtime: meta.modified().ok(),
        }
    }

    #[cfg(not(unix))]
    fn from_meta(meta: &std::fs::Metadata) -> Stamp {
        // Windows has no stable inode through this API, so identity rests on size and mtime. Both
        // change on any real replacement; the cost of missing one is a stale service, not a crash.
        Stamp {
            dev: 0,
            ino: 0,
            len: meta.len(),
            mtime: meta.modified().ok(),
        }
    }
}

/// The path this process was launched through, which is what a supervisor will re-exec. Note this
/// is deliberately *not* canonicalized: under Homebrew the launch path is a symlink whose target
/// changes on upgrade, and resolving it would hide exactly the change worth noticing.
pub fn launch_path() -> Option<PathBuf> {
    std::env::current_exe().ok()
}

/// Resolves once the binary at `path` has been replaced and the replacement has stopped changing.
/// Never resolves if the path cannot be stat-ed at startup, because then there is no baseline to
/// compare against and exiting on a guess would loop a supervised service.
pub async fn wait_for_replacement(path: PathBuf) {
    let baseline = match Stamp::of(&path) {
        Some(baseline) => baseline,
        None => {
            // No baseline means no way to tell a replacement from a first look. Resolving here
            // would complete the shutdown future immediately and exit-loop a supervised service,
            // so wait forever instead and let the signal arm handle shutdown.
            std::future::pending::<()>().await;
            return;
        }
    };
    loop {
        tokio::time::sleep(POLL).await;
        let Some(current) = Stamp::of(&path) else {
            // Mid-upgrade the path can briefly not exist. That is not a reason to exit; wait for it
            // to come back and be judged on its merits.
            continue;
        };
        if current == baseline {
            continue;
        }
        tokio::time::sleep(SETTLE).await;
        match Stamp::of(&path) {
            // Settled on something new.
            Some(settled) if settled == current && settled != baseline => return,
            // Still moving, or it went back to what it was. Keep watching.
            _ => continue,
        }
    }
}

/// Shutdown future for a supervised service: whichever comes first, an operator interrupting it or
/// the binary being upgraded underneath it. Pairs with `axum::serve(..).with_graceful_shutdown(..)`,
/// so in-flight requests drain either way. That matters most for the model gateways, which sit
/// directly in an agent's request path.
pub async fn shutdown_or_upgrade(service: &str) {
    let upgraded = async {
        match launch_path() {
            Some(path) => {
                wait_for_replacement(path.clone()).await;
                eprintln!(
                    "ctx {service}: the binary at {} was replaced (running {}). Exiting so the \
                     supervisor restarts on the new version.",
                    path.display(),
                    env!("CARGO_PKG_VERSION")
                );
            }
            // No launch path means no baseline; fall back to signal-only shutdown.
            None => std::future::pending::<()>().await,
        }
    };
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = upgraded => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write(path: &Path, bytes: &[u8]) {
        let mut f = std::fs::File::create(path).unwrap();
        f.write_all(bytes).unwrap();
        f.sync_all().unwrap();
    }

    #[test]
    fn stamp_is_missing_for_a_path_that_does_not_exist() {
        let dir = tempfile::tempdir().unwrap();
        assert!(Stamp::of(&dir.path().join("absent")).is_none());
    }

    #[test]
    fn stamp_is_stable_across_reads_of_an_untouched_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("ctx");
        write(&p, b"one");
        assert_eq!(Stamp::of(&p), Stamp::of(&p));
    }

    #[test]
    fn rewriting_the_file_changes_its_stamp() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("ctx");
        write(&p, b"one");
        let before = Stamp::of(&p).unwrap();
        write(&p, b"two-different-length");
        assert_ne!(before, Stamp::of(&p).unwrap());
    }

    #[test]
    fn renaming_a_new_file_over_the_path_changes_its_stamp() {
        // How the curl installer and `cargo install` land an upgrade.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("ctx");
        let staged = dir.path().join("ctx.new");
        write(&p, b"old");
        let before = Stamp::of(&p).unwrap();
        write(&staged, b"old"); // identical content, so only identity distinguishes them
        std::fs::rename(&staged, &p).unwrap();
        assert_ne!(
            before,
            Stamp::of(&p).unwrap(),
            "a same-content replacement must still read as replaced"
        );
    }

    #[cfg(unix)]
    #[test]
    fn retargeting_a_symlink_changes_its_stamp() {
        // How Homebrew lands an upgrade: bin/ctx is a symlink into a versioned Cellar directory.
        let dir = tempfile::tempdir().unwrap();
        let v1 = dir.path().join("v1");
        let v2 = dir.path().join("v2");
        let link = dir.path().join("ctx");
        write(&v1, b"one");
        write(&v2, b"two");
        std::os::unix::fs::symlink(&v1, &link).unwrap();
        let before = Stamp::of(&link).unwrap();
        std::fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink(&v2, &link).unwrap();
        assert_ne!(before, Stamp::of(&link).unwrap());
    }

    #[tokio::test]
    async fn waiting_on_a_path_with_no_baseline_never_resolves() {
        // A supervised service must not exit-loop just because it cannot stat its own binary.
        let dir = tempfile::tempdir().unwrap();
        let absent = dir.path().join("absent");
        let r =
            tokio::time::timeout(Duration::from_millis(150), wait_for_replacement(absent)).await;
        assert!(r.is_err(), "should still be waiting");
    }

    #[tokio::test]
    async fn waiting_does_not_resolve_while_the_binary_is_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("ctx");
        write(&p, b"one");
        let r = tokio::time::timeout(Duration::from_millis(150), wait_for_replacement(p)).await;
        assert!(r.is_err(), "an untouched binary must not trigger a restart");
    }
}
