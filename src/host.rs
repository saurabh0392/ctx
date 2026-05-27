//! IDE / runtime detection for `ctx setup` (IDE-agnostic: Cursor, Windsurf, VS Code, terminal, Claude Desktop).

use std::path::{Path, PathBuf};

/// Which editor integration applies when running Claude Code inside an IDE.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdeKind {
    Cursor,
    Windsurf,
    VsCode,
    /// Unknown VS Code–compatible shell; still use reload-window style hints.
    Generic,
}

pub struct IdeHost {
    pub kind: IdeKind,
}

pub struct TerminalHost;

/// Claude Desktop app only (no Claude Code CLI detected).
pub struct DesktopHost;

/// What `ctx setup` should wire and print for the current process environment.
pub trait HostAdapter: Send + Sync {
    fn label(&self) -> &'static str;
    /// `Some` when the primary host is an IDE (not plain terminal).
    fn ide_kind(&self) -> Option<IdeKind>;
    /// Extra MCP JSON files (besides `~/.claude/settings.json`) to merge `ctx` into.
    fn mcp_extra_config_paths(&self) -> Vec<PathBuf>;
    fn needs_periodic_ingest(&self) -> bool;
    /// When false, skip `ctx proxy install` (NODE_OPTIONS + filter.js); Desktop Electron ignores it.
    fn supports_node_options(&self) -> bool;
    fn reload_instruction(&self) -> &'static str;
    fn offer_editor_rules(&self) -> bool;
    fn editor_rules_path(&self) -> Option<PathBuf>;
}

fn windsurf_detected(home: &Path) -> bool {
    std::env::var("WINDSURF_SESSION").is_ok() || home.join(".codeium/windsurf").is_dir()
}

fn cursor_detected(home: &Path) -> bool {
    std::env::var("CURSOR_TRACE_ID").is_ok()
        || std::env::var("VSCODE_PID").is_ok()
        || home.join(".cursor").join("extensions").is_dir()
}

fn vscode_shell() -> bool {
    std::env::var("TERM_PROGRAM").ok().as_deref() == Some("vscode")
}

/// Hint shown after `ctx setup --uninstall` (does not depend on host detection).
pub fn uninstall_reload_hint() -> &'static str {
    "Reload Window in your IDE (Cmd+Shift+P or Ctrl+Shift+P, search Reload Window). If you use Claude Code in a plain terminal, start a new shell so NODE_OPTIONS clears. If Claude Desktop is installed, quit and reopen it so MCP changes apply."
}

/// Primary host for this `ctx setup` run (Claude Code in an IDE vs terminal vs Desktop-only).
/// Claude Desktop MCP is wired separately whenever the Desktop data dir exists.
pub fn detect_primary_host() -> Box<dyn HostAdapter> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    detect_primary_host_for_home(&home)
}

/// Same as [`detect_primary_host`] but uses `home` for filesystem markers (for tests).
pub fn detect_primary_host_for_home(home: &Path) -> Box<dyn HostAdapter> {
    if windsurf_detected(home) {
        return Box::new(IdeHost {
            kind: IdeKind::Windsurf,
        });
    }
    if cursor_detected(home) {
        return Box::new(IdeHost {
            kind: IdeKind::Cursor,
        });
    }
    if vscode_shell() {
        return Box::new(IdeHost {
            kind: IdeKind::VsCode,
        });
    }
    if crate::config::claude_desktop_installed_for_home(home)
        && !crate::config::claude_code_cli_present_for_home(home)
    {
        return Box::new(DesktopHost);
    }
    Box::new(TerminalHost)
}

impl HostAdapter for IdeHost {
    fn label(&self) -> &'static str {
        match self.kind {
            IdeKind::Cursor => "Claude Code in Cursor (or Cursor-compatible)",
            IdeKind::Windsurf => "Claude Code in Windsurf",
            IdeKind::VsCode => "Claude Code in VS Code (or compatible shell)",
            IdeKind::Generic => "Claude Code in an IDE",
        }
    }

    fn ide_kind(&self) -> Option<IdeKind> {
        Some(self.kind)
    }

    fn mcp_extra_config_paths(&self) -> Vec<PathBuf> {
        let Some(home) = dirs::home_dir() else {
            return Vec::new();
        };
        match self.kind {
            IdeKind::Cursor => vec![home.join(".cursor").join("mcp.json")],
            IdeKind::Windsurf => vec![home.join(".codeium").join("windsurf").join("mcp_config.json")],
            IdeKind::VsCode | IdeKind::Generic => Vec::new(),
        }
    }

    fn needs_periodic_ingest(&self) -> bool {
        true
    }

    fn supports_node_options(&self) -> bool {
        true
    }

    fn reload_instruction(&self) -> &'static str {
        "Cmd+Shift+P (macOS) or Ctrl+Shift+P (Windows/Linux), type Reload Window, then Enter."
    }

    fn offer_editor_rules(&self) -> bool {
        matches!(self.kind, IdeKind::Cursor)
    }

    fn editor_rules_path(&self) -> Option<PathBuf> {
        match self.kind {
            IdeKind::Cursor => dirs::home_dir().map(|h| h.join(".cursor/rules/ctx.mdc")),
            _ => None,
        }
    }
}

impl HostAdapter for TerminalHost {
    fn label(&self) -> &'static str {
        "Claude Code CLI (terminal)"
    }

    fn ide_kind(&self) -> Option<IdeKind> {
        None
    }

    fn mcp_extra_config_paths(&self) -> Vec<PathBuf> {
        Vec::new()
    }

    fn needs_periodic_ingest(&self) -> bool {
        crate::config::claude_desktop_installed()
    }

    fn supports_node_options(&self) -> bool {
        true
    }

    fn reload_instruction(&self) -> &'static str {
        "Start a new terminal session so NODE_OPTIONS applies."
    }

    fn offer_editor_rules(&self) -> bool {
        false
    }

    fn editor_rules_path(&self) -> Option<PathBuf> {
        None
    }
}

impl HostAdapter for DesktopHost {
    fn label(&self) -> &'static str {
        "Claude Desktop (standalone)"
    }

    fn ide_kind(&self) -> Option<IdeKind> {
        None
    }

    fn mcp_extra_config_paths(&self) -> Vec<PathBuf> {
        Vec::new()
    }

    fn needs_periodic_ingest(&self) -> bool {
        true
    }

    fn supports_node_options(&self) -> bool {
        false
    }

    fn reload_instruction(&self) -> &'static str {
        "Quit Claude Desktop completely and reopen it so MCP changes apply."
    }

    fn offer_editor_rules(&self) -> bool {
        false
    }

    fn editor_rules_path(&self) -> Option<PathBuf> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    /// IDE detection reads process env; clear it so temp-home tests are deterministic.
    struct ClearHostEnv {
        keys: &'static [&'static str],
        saved: Vec<Option<std::ffi::OsString>>,
    }

    impl ClearHostEnv {
        fn new(keys: &'static [&'static str]) -> Self {
            let saved: Vec<_> = keys.iter().map(|k| std::env::var_os(k)).collect();
            for k in keys {
                std::env::remove_var(k);
            }
            Self { keys, saved }
        }
    }

    impl Drop for ClearHostEnv {
        fn drop(&mut self) {
            for (k, v) in self.keys.iter().zip(self.saved.iter()) {
                if let Some(val) = v {
                    std::env::set_var(k, val);
                } else {
                    std::env::remove_var(k);
                }
            }
        }
    }

    #[test]
    fn desktop_host_disables_node_options() {
        let h = DesktopHost;
        assert!(!h.supports_node_options());
    }

    #[test]
    #[serial]
    fn detect_desktop_standalone_when_desktop_dir_and_no_settings() {
        let _env = ClearHostEnv::new(&["CURSOR_TRACE_ID", "VSCODE_PID", "WINDSURF_SESSION", "TERM_PROGRAM"]);
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        #[cfg(target_os = "macos")]
        {
            let p = home.join("Library/Application Support/Claude");
            std::fs::create_dir_all(&p).unwrap();
        }
        #[cfg(target_os = "linux")]
        {
            let p = home.join(".config/Claude");
            std::fs::create_dir_all(&p).unwrap();
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            // Skip: no Desktop support dir convention on this OS in config.rs
            return;
        }
        let h = detect_primary_host_for_home(home);
        assert_eq!(h.label(), "Claude Desktop (standalone)");
        assert!(!h.supports_node_options());
    }

    #[test]
    #[serial]
    fn detect_terminal_when_desktop_and_cli_settings_exist() {
        let _env = ClearHostEnv::new(&["CURSOR_TRACE_ID", "VSCODE_PID", "WINDSURF_SESSION", "TERM_PROGRAM"]);
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        #[cfg(target_os = "macos")]
        {
            let p = home.join("Library/Application Support/Claude");
            std::fs::create_dir_all(&p).unwrap();
        }
        #[cfg(target_os = "linux")]
        {
            let p = home.join(".config/Claude");
            std::fs::create_dir_all(&p).unwrap();
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            return;
        }
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        std::fs::write(home.join(".claude/settings.json"), "{}").unwrap();
        let h = detect_primary_host_for_home(home);
        assert_eq!(h.label(), "Claude Code CLI (terminal)");
        assert!(h.supports_node_options());
    }
}
