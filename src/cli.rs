use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(
    name = "ctx",
    version,
    about = "Local context efficiency and evidence for coding agents"
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
        /// Permanently delete all CTX-owned local data after uninstalling integrations
        #[arg(long, requires = "uninstall")]
        purge_data: bool,
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
        /// Deprecated no-op kept for old installer scripts
        #[arg(long, hide = true)]
        beta: bool,
    },
    /// Check local configuration, database, Claude hooks, and the dashboard
    Doctor {
        /// Print the stable machine-readable diagnostic schema
        #[arg(long)]
        json: bool,
    },
    /// Check for or install a checksum-verified release from GitHub
    Update {
        /// Only report whether a newer release exists
        #[arg(long)]
        check: bool,
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
        /// Agent surface that requested the wrapper; scopes both evidence and dashboard provenance.
        #[arg(long, default_value = "cursor")]
        surface: String,
        /// Stable agent session id, when the hook provides one.
        #[arg(long)]
        session: Option<String>,
        /// Shell contract used to execute the command.
        #[arg(long, value_enum, default_value_t = crate::cmd_run::ShellKind::Auto)]
        shell: crate::cmd_run::ShellKind,
        /// The command to run, exactly as it would be typed in a shell.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true)]
        command: Vec<String>,
    },
    /// Run as an MCP server over stdio (JSON-RPC). Exposes ctx data to LLM clients.
    Mcp,
    /// Manage local MCP servers routed through CTX's protocol-preserving gateway.
    Gateway {
        #[command(subcommand)]
        command: GatewayCommand,
    },
    /// Inspect, configure, and operate opt-in local model-path routes.
    ModelGateway {
        #[command(subcommand)]
        command: ModelGatewayCommand,
    },
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
    /// Generated locally; sharing the exported file is always an explicit user action.
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
        /// Aggregate omits paths, commands, repos, and tool/server names; detailed is local-only
        #[arg(long, value_enum, default_value_t = ReportPrivacy::Aggregate)]
        privacy: ReportPrivacy,
        /// Export a self-contained HTML report or the versioned JSON schema
        #[arg(long, value_enum, default_value_t = ReportFormat::Html)]
        format: ReportFormat,
    },
}

#[derive(Subcommand)]
pub enum GatewayCommand {
    /// Register an explicitly approved local stdio MCP server.
    AddStdio {
        /// Immutable local server identity used in tool names and the contract cache.
        id: String,
        /// Server executable. CTX resolves and stores an absolute path.
        #[arg(long)]
        command: String,
        /// Server working directory.
        #[arg(long)]
        cwd: Option<String>,
        /// Environment variable names to copy into the isolated child process.
        #[arg(long = "pass-env", value_delimiter = ',')]
        pass_env: Vec<String>,
        /// Arguments passed directly to the executable, without invoking a shell.
        #[arg(last = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Register an opt-in remote Streamable HTTP MCP destination.
    AddHttp {
        id: String,
        /// Exact approved MCP endpoint. HTTPS is required except for loopback development.
        #[arg(long)]
        url: String,
        /// Environment variable containing a bearer token; its value is never persisted.
        #[arg(long)]
        bearer_token_env: Option<String>,
        /// Acknowledge that remote gateway support is experimental pending independent security review.
        #[arg(long = "accept-remote-preview", alias = "accept-remote-beta")]
        accept_remote_preview: bool,
    },
    /// List registered gateway destinations without printing credentials.
    List,
    /// Remove a registered server definition.
    Remove { id: String },
    /// Import a Codex MCP server and atomically route it through the CTX gateway.
    CodexEnable {
        /// Existing name under `[mcp_servers.<name>]` in ~/.codex/config.toml.
        name: String,
        /// Acknowledge the experimental remote HTTP gateway when importing a URL server.
        #[arg(long = "accept-remote-preview", alias = "accept-remote-beta")]
        accept_remote_preview: bool,
    },
    /// Restore the exact pre-gateway Codex MCP server definition.
    CodexDisable { name: String },
    /// Complete OAuth authorization for a registered remote MCP server.
    Login {
        id: String,
        /// Accept the discovered authorization destinations without an interactive prompt.
        #[arg(long)]
        yes: bool,
    },
    /// Delete CTX's OAuth credential for a remote MCP server from the OS credential store.
    Logout { id: String },
    /// Serve one registered MCP destination over stdio for an agent client.
    Serve {
        id: String,
        #[arg(long, default_value = "codex")]
        surface: String,
    },
}

#[derive(Subcommand)]
pub enum ModelGatewayCommand {
    /// Inspect documented local configuration boundaries without changing them.
    Probe {
        /// Surface to inspect: claude-code, cursor, codex, or all.
        #[arg(long, default_value = "all")]
        surface: String,
        /// Execute each installed client's version command. Some clients may perform startup
        /// maintenance even for --version, so the default probe only inspects file/config presence.
        #[arg(long)]
        run_client_version: bool,
        /// Print the stable redacted receipt schema.
        #[arg(long)]
        json: bool,
    },
    /// Convert an offline JSON capture envelope into a content-redacted structural receipt.
    SanitizeCapture {
        /// Read the raw envelope from this file; omit to read stdin. CTX never persists the input.
        #[arg(long)]
        input: Option<std::path::PathBuf>,
    },
    /// Register an advanced route with an explicit protocol and fixed provider destination.
    AddRoute {
        /// Immutable lowercase route id.
        id: String,
        /// claude-code or codex. Cursor remains held until its local boundary is captured.
        #[arg(long)]
        surface: String,
        /// anthropic-messages or openai-responses.
        #[arg(long)]
        protocol: String,
        /// Route-specific auth identity; credentials are never stored in this registry.
        #[arg(long)]
        authentication: String,
        /// Fixed provider class: anthropic or openai. Arbitrary URLs are not accepted.
        #[arg(long)]
        upstream: String,
        /// Unprivileged loopback port dedicated to this route.
        #[arg(long)]
        port: u16,
        /// shadow observes only; testing permits M3's narrow evidence-gated contracts.
        #[arg(long, default_value = "shadow")]
        mode: String,
    },
    /// Create a supported Wave 1 route with safe protocol/provider defaults.
    Setup {
        /// claude-code or codex. Cursor reports unavailable until its boundary is captured.
        surface: String,
        /// Route-specific auth identity; credentials are never stored.
        #[arg(long)]
        authentication: String,
        /// Override the stable generated route id.
        #[arg(long)]
        id: Option<String>,
        /// Override the default loopback port (8871 Codex, 8872 Claude Code).
        #[arg(long)]
        port: Option<u16>,
        /// shadow observes only; testing permits M3's narrow evidence-gated contracts.
        #[arg(long, default_value = "shadow")]
        mode: String,
    },
    /// List registered model routes, their local paths, and fixed destinations.
    ListRoutes,
    /// Remove a CTX model route. This does not edit coding-client configuration.
    RemoveRoute { id: String },
    /// Start a healthy route and atomically switch its supported client configuration.
    Enable {
        id: String,
        /// Confirm that this local CTX process may see model requests and authorization headers in
        /// memory while forwarding them to the displayed fixed provider.
        #[arg(long)]
        yes: bool,
    },
    /// Show route ownership, client configuration, service, and fixed destination state.
    Status {
        id: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Diagnose route/config/service agreement without requiring the gateway to be healthy.
    Doctor {
        id: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Evaluate local route proof and list the external gates still blocking commercial release.
    Readiness {
        /// Print the stable content-free Wave 1 readiness schema.
        #[arg(long)]
        json: bool,
    },
    /// Immediately restore the prior client route while retaining recoverable CTX route state.
    Bypass { id: String },
    /// Restore the prior client route and remove CTX's route/service ownership.
    Disable { id: String },
    /// Serve one registered route. Normally launched by `enable`.
    Serve {
        id: String,
        /// CTX-owned listener identity used by lifecycle health proof.
        #[arg(long, hide = true)]
        health_nonce: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum ReportPrivacy {
    Aggregate,
    Detailed,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum ReportFormat {
    Html,
    Json,
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
    /// so ctx can collect the trimmed comparison arm. One tool at a time.
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
    /// Cursor preCompact: record a live compaction attempt (ADR 0047)
    CursorPreCompact,
    /// Claude Code native compaction start observer.
    ClaudePreCompact,
    /// Claude Code native compaction completion observer.
    ClaudePostCompact,
    /// Codex plugin lifecycle heartbeat.
    CodexSessionStart,
    /// Codex user turn and correction-signal observer.
    CodexUserPromptSubmit,
    /// Codex shell input rewrite boundary.
    CodexPreToolUse,
    /// Codex local tool-result observer (never replaces the result).
    CodexPostToolUse,
    /// Codex native compaction start observer.
    CodexPreCompact,
    /// Codex native compaction completion observer.
    CodexPostCompact,
    /// Codex main-agent or subagent stop observer.
    CodexStop,
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
