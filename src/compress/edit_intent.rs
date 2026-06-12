//! Edit-intent guard for Read (ADR 0001 / CTX-8).
//!
//! In Claude Code the canonical edit flow is read-before-edit, so a Read of a project source file
//! is very often the precursor to an Edit of that same file. Trimming such a read can hide the
//! exact region the agent is about to change, which forces re-reads (observed live during the
//! Read trim trial). This module decides whether a Read is a **reference read** that is safe to
//! trim, versus a **working read** of an editable file that must be left intact.
//!
//! Phase 1 is a pure, static classifier over the file path and the project root (`cwd`). It is
//! deliberately conservative: only files the agent is clearly not positioned to edit are eligible.
//! Phase 2 (a later ticket) layers session history (files already edited or re-read) on top.

/// Directory segments that mark vendored, generated, or build output. Reads under these paths are
/// reference material the agent does not hand-edit, so they are safe to trim. Matched as substrings
/// with surrounding slashes so `target/` matches `/repo/target/x` but not `mytargets.rs`.
const REFERENCE_DIRS: &[&str] = &[
    "/node_modules/",
    "/target/",
    "/dist/",
    "/build/",
    "/.next/",
    "/vendor/",
    "/.venv/",
    "/venv/",
    "/site-packages/",
    "/__pycache__/",
    "/.git/",
    "/coverage/",
    "/out/",
    "/.cargo/",
    "/.rustup/",
];

/// Filename suffixes that mark generated / lock / minified artifacts. Safe to trim.
const REFERENCE_SUFFIXES: &[&str] = &[".lock", ".min.js", ".min.css", ".map", ".sum"];

/// Exact filenames (lowercased) that are lockfiles. Safe to trim.
const REFERENCE_FILES: &[&str] = &[
    "package-lock.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "cargo.lock",
    "go.sum",
    "poetry.lock",
    "composer.lock",
    "gemfile.lock",
];

/// Whether a Read of `file_path` is eligible for trimming under the edit-intent guard.
///
/// Returns `true` only for reference reads (the agent is not positioned to edit the file):
/// vendored/generated/build paths, lock/minified/map artifacts, or absolute paths outside the
/// project root. Everything else, including an unknown path, is protected (returns `false`), so
/// working reads of editable project source are never trimmed.
pub fn read_is_trim_eligible(file_path: Option<&str>, cwd: &str) -> bool {
    // Unknown target: cannot prove it is reference material, so protect it.
    let Some(path) = file_path.map(str::trim).filter(|p| !p.is_empty()) else {
        return false;
    };
    let lower = path.to_lowercase();
    let lower_slashed = lower.replace('\\', "/");

    if REFERENCE_DIRS.iter().any(|d| lower_slashed.contains(d)) {
        return true;
    }
    if REFERENCE_SUFFIXES
        .iter()
        .any(|s| lower_slashed.ends_with(s))
    {
        return true;
    }
    if let Some(name) = lower_slashed.rsplit('/').next() {
        if REFERENCE_FILES.contains(&name) {
            return true;
        }
    }

    // Absolute path that resolves outside the project root is reference material (a dependency,
    // a system file, or another repo the agent is consulting, not editing).
    if path.starts_with('/') && !cwd.trim().is_empty() {
        let root = cwd.trim().trim_end_matches('/');
        if !root.is_empty() && !lower_slashed.starts_with(&root.to_lowercase()) {
            return true;
        }
    }

    // Otherwise this looks like an editable file inside the project: protect it.
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    const CWD: &str = "/Users/me/proj";

    #[test]
    fn project_source_is_protected() {
        assert!(!read_is_trim_eligible(Some("src/foo.rs"), CWD));
        assert!(!read_is_trim_eligible(
            Some("/Users/me/proj/src/foo.rs"),
            CWD
        ));
        assert!(!read_is_trim_eligible(
            Some("/Users/me/proj/web/components/FullConversation.tsx"),
            CWD
        ));
        assert!(!read_is_trim_eligible(
            Some("components/FullConversation.tsx"),
            CWD
        ));
    }

    #[test]
    fn unknown_or_empty_path_is_protected() {
        assert!(!read_is_trim_eligible(None, CWD));
        assert!(!read_is_trim_eligible(Some(""), CWD));
        assert!(!read_is_trim_eligible(Some("   "), CWD));
    }

    #[test]
    fn vendored_and_build_paths_are_eligible() {
        assert!(read_is_trim_eligible(
            Some("/Users/me/proj/node_modules/react/index.js"),
            CWD
        ));
        assert!(read_is_trim_eligible(
            Some("/Users/me/proj/target/debug/build.rs"),
            CWD
        ));
        assert!(read_is_trim_eligible(
            Some("web/.next/server/chunk.js"),
            CWD
        ));
        assert!(read_is_trim_eligible(
            Some("/Users/me/proj/dist/app.js"),
            CWD
        ));
    }

    #[test]
    fn lockfiles_minified_and_maps_are_eligible() {
        assert!(read_is_trim_eligible(
            Some("/Users/me/proj/package-lock.json"),
            CWD
        ));
        assert!(read_is_trim_eligible(Some("Cargo.lock"), CWD));
        assert!(read_is_trim_eligible(Some("web/static/app.min.js"), CWD));
        assert!(read_is_trim_eligible(Some("web/static/app.js.map"), CWD));
    }

    #[test]
    fn absolute_path_outside_project_is_eligible() {
        assert!(read_is_trim_eligible(
            Some("/usr/local/lib/python3.11/json/__init__.py"),
            CWD
        ));
        assert!(read_is_trim_eligible(
            Some("/Users/me/other-repo/src/lib.rs"),
            CWD
        ));
    }

    #[test]
    fn relative_path_inside_project_stays_protected_even_without_cwd() {
        assert!(!read_is_trim_eligible(Some("src/main.rs"), ""));
    }
}
