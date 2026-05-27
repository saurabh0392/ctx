use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "ctx", version, about = "Context Killer — MCP savings and analytics for Claude Code")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Switch active MCP profile
    Use {
        /// Profile name: carrier, design, data, minimal, all
        profile: String,
        /// Allow switch even when quality guard reports critical MCP usage on stripped servers
        #[arg(long)]
        force: bool,
    },
    /// Show active profile and per-turn token cost estimate
    Status,
    /// Show cumulative token savings
    Gain {
        /// Print a single summary line (stderr) for manual use in scripts
        #[arg(long)]
        brief: bool,
    },
    /// Manage custom profiles
    Profile {
        #[command(subcommand)]
        command: ProfileCommand,
    },
    /// Manage the MCP-filtering proxy
    Proxy {
        #[command(subcommand)]
        command: ProxyCommand,
    },
    /// Manage system prompt injection (~/.ctx/system_prefix.md)
    Inject {
        #[command(subcommand)]
        command: InjectCommand,
    },
    /// One-command install: proxy, launchd autostart, default system prefix
    Setup {
        #[arg(long, default_value = "8788")]
        port: u16,
        #[arg(long, default_value = "https://api.anthropic.com")]
        upstream: String,
        /// Reverse everything ctx setup did
        #[arg(long)]
        uninstall: bool,
        /// Start proxy + launchd only; skip writing ~/.claude/settings.json
        /// Use this when Claude Code is open — close it, then run `ctx proxy install`
        #[arg(long)]
        no_install: bool,
        /// Skip the optional ~/.zshrc prompt for NODE_EXTRA_CA_CERTS
        #[arg(long)]
        no_zshrc_prompt: bool,
        /// Print planned actions and exit without changing anything
        #[arg(long)]
        dry_run: bool,
        /// Skip the interactive confirmation prompt (for scripts / CI)
        #[arg(long)]
        yes: bool,
    },
    /// Open the savings dashboard in your browser
    Dashboard {
        #[arg(long, default_value = "8789")]
        port: u16,
        /// Run as background service without opening a browser
        #[arg(long)]
        no_open: bool,
    },
    /// Scan Claude Code JSONL into ~/.ctx/ctx.db (sessions, turns, tool invocations)
    Ingest,
    /// Claude Code hook entrypoints (stdin JSON → stdout; used from ~/.claude/settings.json)
    Hook {
        #[command(subcommand)]
        command: HookCommand,
    },
    /// Run as an MCP server over stdio (JSON-RPC). Exposes ctx data to LLM clients.
    Mcp,
    /// Dry-run a prompt through the ctx pipeline (no tokens consumed)
    Simulate {
        /// Prompt text (reads from stdin if omitted)
        #[arg(long)]
        prompt: Option<String>,
        /// Working directory to simulate from
        #[arg(long)]
        cwd: Option<String>,
        /// Session ID for coaching context
        #[arg(long)]
        session: Option<String>,
        /// Override profile (default: auto-select or current)
        #[arg(long)]
        profile: Option<String>,
        /// Compare all profiles side by side
        #[arg(long)]
        all_profiles: bool,
        /// Replay the last N hook traces
        #[arg(long)]
        replay_last: Option<usize>,
        /// Output JSON instead of formatted text
        #[arg(long)]
        json: bool,
    },
    /// Named context modes (profile + toggles bundled)
    #[command(args_conflicts_with_subcommands = true)]
    Mode {
        /// Mode name to activate (e.g. ctx mode debug)
        name: Option<String>,
        #[command(subcommand)]
        command: Option<ModeCommand>,
    },
    /// A/B experiment status and self-tuning recommendations
    Experiment {
        #[command(subcommand)]
        command: ExperimentCommand,
    },
}

#[derive(Subcommand)]
pub enum ModeCommand {
    /// List configured modes
    List,
    /// Show one mode definition
    Show { name: String },
    /// Save current settings as a named mode
    Save { name: String },
}

#[derive(Subcommand)]
pub enum ExperimentCommand {
    /// Show experiment state and ab-results.json recommendations
    Status,
    /// Apply recommendations to config.toml
    Apply,
    /// Clear ab-results.json
    Reset,
}

#[derive(Subcommand)]
pub enum ProfileCommand {
    /// List all profiles with token cost estimates
    List,
    /// Show profile details
    Show { name: String },
    /// Add a custom profile (--keep mcp__claude_ai_Foo__,mcp__claude_ai_Bar__)
    Add {
        name: String,
        #[arg(long, value_delimiter = ',', required = true)]
        keep: Vec<String>,
    },
    /// Remove a custom profile
    Remove { name: String },
    /// Build `personal` profile from MCP tool_use history (requires `ctx ingest` first)
    Auto {
        #[arg(long)]
        refresh: bool,
    },
    /// Auto-generate profiles from your actual MCP server stack (no history needed)
    Generate,
}

#[derive(Subcommand)]
pub enum HookCommand {
    /// Blocking hook: auto-profile, budget gate, system prefix injection
    UserPromptSubmit,
}

#[derive(Subcommand)]
pub enum ProxyCommand {
    /// Start the filtering proxy in the foreground
    Start {
        #[arg(long, default_value = "8788")]
        port: u16,
        #[arg(long, default_value = "https://api.anthropic.com")]
        upstream: String,
    },
    /// Install: update ~/.claude/settings.json (allowedMcpServers + hooks; no NODE_OPTIONS filter)
    Install {
        #[arg(long, default_value = "8788")]
        port: u16,
        #[arg(long, default_value = "https://api.anthropic.com")]
        upstream: String,
    },
    /// Uninstall: remove ctx wiring from settings.json (NODE_OPTIONS, optional ANTHROPIC_BASE_URL restore) and strip legacy ctx hook lines
    Uninstall,
    /// Show proxy configuration
    Status,
}

#[derive(Subcommand)]
pub enum InjectCommand {
    /// Print the current system prefix
    Show,
    /// Open system_prefix.md in $EDITOR
    Edit,
    /// Disable injection for all requests
    Disable,
    /// Re-enable injection
    Enable,
}
