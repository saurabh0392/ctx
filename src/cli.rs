use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "ctx",
    version,
    about = "Context Killer — MCP savings and analytics for Claude Code"
)]
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
    /// Manage system prompt injection (~/.ctx/system_prefix.md)
    Inject {
        #[command(subcommand)]
        command: InjectCommand,
    },
    /// One-command install: hooks, soft filter, launchd autostart, default system prefix
    Setup {
        /// Reverse everything ctx setup did
        #[arg(long)]
        uninstall: bool,
        /// Install hooks/services only; skip writing ~/.claude/settings.json
        /// Use this when Claude Code is open — close it, then re-run `ctx setup`
        #[arg(long)]
        no_install: bool,
        /// Deprecated no-op (kept so old scripts do not break)
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
    Ingest {
        /// Re-parse every session file, not just files changed since last ingest
        #[arg(long)]
        full: bool,
    },
    /// Claude Code hook entrypoints (stdin JSON → stdout; used from ~/.claude/settings.json)
    Hook {
        #[command(subcommand)]
        command: HookCommand,
    },
    /// Run a shell command and compact its output before the agent reads it (CTX-41).
    ///
    /// Used by the Cursor preToolUse Shell hook: it rewrites `<cmd>` to `ctx run <cmd>` so the
    /// compacted result returns as Shell's own output. The command's real exit code is preserved,
    /// and output is left untouched unless the earn-it gate says trim and it actually saves chars.
    Run {
        /// The command to run, exactly as it would be typed in a shell.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true)]
        command: Vec<String>,
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
    /// MCP filter mode (soft / strict / off) and session expansion
    Filter {
        #[command(subcommand)]
        command: FilterCommand,
    },
    /// Print the verbatim original of a trim by its rewind id (from the ctx trim marker).
    Expand {
        /// The rewind id shown in the "[ctx trimmed ... id: X]" marker.
        id: String,
    },
    /// Self-learning context controller: collection status, presets, training, activation
    Context {
        #[command(subcommand)]
        command: ContextCommand,
    },
    /// Reproducible agent-context benchmark (Act 2)
    Bench {
        #[command(subcommand)]
        command: BenchCommand,
    },
    /// Export a shareable, self-contained Context Report for a repo (CTX-56).
    ///
    /// Writes a single HTML file that opens in any browser on any machine, no ctx install needed.
    /// Local only: nothing leaves this machine except the file you choose to share.
    Report {
        /// Repo to report on, matched as a substring of its path. Omit to list repos.
        #[arg(long)]
        repo: Option<String>,
        /// Output file (default: ctx-report-<repo>.html in the current directory).
        #[arg(long)]
        out: Option<String>,
        /// List the repos ctx has data for, then exit.
        #[arg(long)]
        list: bool,
    },
}

#[derive(Subcommand)]
pub enum ContextCommand {
    /// Show the collection window: labels collected, corrections caused, per-tool progress
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Set the compression preset: off (collect only), safe (git/test/grep), full
    Preset { value: String },
    /// Turn on safe user-facing compression (alias for `preset safe`)
    On,
    /// Turn off user-facing compression, keep shadow collection (alias for `preset off`)
    Off,
    /// Archive the current DB to a timestamped backup, then start fresh with an empty schema.
    /// Destructive: requires --yes. The archive lands beside ctx.db as ctx.db.post-wipe-<ts>.
    Reset {
        /// Confirm the wipe. Without it, prints what would happen and exits.
        #[arg(long)]
        yes: bool,
    },
    /// Re-parse sessions, clean interrupt flags, rejoin outcome labels, retrain model
    Repair {
        /// Skip the full JSONL re-parse (only rejoin from existing turns)
        #[arg(long)]
        skip_ingest: bool,
        #[arg(long)]
        json: bool,
    },
    /// Train the local outcome model from collected labels and print the honesty gate
    Learn {
        #[arg(long)]
        json: bool,
    },
    /// Honest causal before/after: correction/re-read rate when a tool is trimmed vs not, with confidence intervals
    Proof {
        /// Only show the before/after for this exact tool name (e.g. Bash)
        #[arg(long)]
        tool: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Start or stop a deliberate trim trial for one tool: trims it live (even with preset off)
    /// so ctx can collect the trimmed "after" arm for the before/after proof. One tool at a time.
    Trial {
        /// Exact tool name to trial (e.g. Read, Bash). Omit with --off to clear all trials.
        tool: Option<String>,
        /// Start trimming this tool live to gather the after arm.
        #[arg(long, conflicts_with = "off")]
        on: bool,
        /// Stop trimming this tool (or all tools if no name is given) and return to shadow only.
        #[arg(long)]
        off: bool,
    },
    /// Audit positive labels: show the correction/re-read evidence behind each, to judge precision by hand
    Labels {
        /// Only show labels for this exact tool name (e.g. Bash, Read)
        #[arg(long)]
        tool: Option<String>,
        /// How many of the most recent positive labels to show
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    /// Cache-safety audit: does editing the cached prefix (MCP filtering, system injection)
    /// correlate with more cache writes and fewer cache reads in your own traffic. Read-only.
    CacheAudit {
        /// Only look at the last N days of requests (omit for all time)
        #[arg(long)]
        days: Option<i64>,
        #[arg(long)]
        json: bool,
    },
    /// Spot-check richer outcome signals (ADR 0019): per-signal counts and recent samples, so
    /// you can hand-label precision before any signal is allowed to influence the gate. Read-only.
    SignalAudit {
        /// Only show decisions where this signal fired (e.g. reedit, reread, correction_explicit)
        #[arg(long)]
        signal: Option<String>,
        /// How many recent samples to print per run
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum BenchCommand {
    /// Replay collected decisions through the arms and report outcome-first metrics
    Run {
        #[arg(long)]
        json: bool,
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
pub enum FilterCommand {
    /// Set filter mode: soft (default), strict, or off
    Mode { mode: String },
    /// Temporarily un-deny an MCP server or tool for this session (soft mode)
    Expand { target: String },
    /// Clear all session expansion entries
    ClearExpansion,
}

#[derive(Subcommand)]
pub enum ExperimentCommand {
    /// Show experiment state and ab-results.json recommendations
    Status,
    /// Apply recommendations to config.toml
    Apply,
    /// Clear ab-results.json
    Reset,
    /// Daily driver: apply phase config, ingest, digest, notify
    Tick {
        /// Show actions without writing config or journal
        #[arg(long)]
        dry_run: bool,
    },
    /// Print experiment digest (human or JSON)
    Digest {
        #[arg(long)]
        json: bool,
    },
    /// Install daily experiment tick (launchd on macOS, instructions elsewhere)
    InstallSchedule,
    /// Manage the 15-day experiment plan file
    Plan {
        #[command(subcommand)]
        command: ExperimentPlanCommand,
    },
    /// Analyze historical MCP tool usage (vector mix readiness)
    Analyze {
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum ExperimentPlanCommand {
    /// Create ~/.ctx/experiment-plan.toml from a template
    Init {
        /// Project path to filter hook traces (corpus)
        #[arg(long)]
        corpus: String,
        /// Template: gaffer (16-day), tool-mix (vector tool filtering A/B), or ctx (dogfood)
        #[arg(long, default_value = "gaffer")]
        template: String,
    },
    /// Show current plan day, phase, and last tick
    Status,
}

#[derive(Subcommand)]
pub enum ProfileCommand {
    /// List all profiles with token cost estimates
    List,
    /// Show profile details
    Show { name: String },
    /// Add a custom profile (--keep server prefixes and/or --keep-tool tool names)
    Add {
        name: String,
        #[arg(long, value_delimiter = ',')]
        keep: Option<Vec<String>>,
        #[arg(long = "keep-tool", value_delimiter = ',')]
        keep_tool: Option<Vec<String>>,
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
    /// Expand server-prefix keep lists into per-tool keep_tools from usage history
    MigrateTools {
        /// Profile slug (default: all profiles in profiles.toml)
        name: Option<String>,
        #[arg(long, help = "Re-migrate profiles that already use keep_tools")]
        force: bool,
    },
}

#[derive(Subcommand)]
pub enum HookCommand {
    /// Blocking hook: auto-profile, budget gate, system prefix injection
    UserPromptSubmit,
    /// PostToolUse: compress tool output via updatedToolOutput
    PostToolUse,
    /// Cursor postToolUse: observe Cursor tool results and record cursor decisions (ADR 0018)
    CursorPostToolUse,
    /// Cursor preToolUse: rewrite an earned Shell command to `ctx run <cmd>` (ADR 0024 / CTX-41)
    CursorPreToolUse,
    /// Cursor preCompact: record a live Cursor compaction event (ADR 0023 / CTX-31)
    CursorPreCompact,
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
