use super::types::CompressKind;

/// Classify a shell command by the strategy that best fits its output. Real commands are rarely a
/// bare verb: they are `cd pkg && grep ...`, `FOO=1 npx ...`, `grep ... | head -50`. A prefix-only
/// check misroutes all of those to Generic, whose blunt head-truncation guts structured output the
/// agent then re-runs (CTX-58). So we normalize first: split the compound command into segments,
/// strip each segment's leading env assignments, look at the stage that produces the output, and
/// return the first segment that classifies to a real structured kind.
pub fn classify_bash_command(command: &str) -> CompressKind {
    for seg in split_segments(command) {
        let eff = strip_env_assignments(seg.trim());
        // The producer of the shown lines is the pipeline's first stage; downstream `| head`,
        // `| sort`, `| grep` filter or format what it emitted.
        let producer = eff.split('|').next().unwrap_or(eff).trim();
        let kind = classify_simple(producer);
        if kind != CompressKind::Generic {
            return kind;
        }
    }
    CompressKind::Generic
}

/// If a command explicitly bounds its own output (`| head -50`, `tail -n 20`, `grep -m 30`), return
/// the tightest line cap it asked for. The agent deliberately narrowed the output, so trimming below
/// it is doubly wrong; the caller passes such output through untouched when the cap is modest
/// (CTX-58). A bare `head`/`tail` with no count is its shell default of 10 lines.
pub fn explicit_output_cap(command: &str) -> Option<usize> {
    let lower = command.to_lowercase();
    let tokens: Vec<&str> = lower.split_whitespace().collect();
    let mut caps: Vec<usize> = Vec::new();
    for (i, tok) in tokens.iter().enumerate() {
        match *tok {
            "head" | "tail" => {
                let next = tokens.get(i + 1).copied();
                if let Some(n) = next.and_then(dash_number) {
                    caps.push(n);
                } else if next == Some("-n") || next == Some("-c") {
                    if let Some(n) = tokens.get(i + 2).and_then(|t| t.parse().ok()) {
                        caps.push(n);
                    }
                } else {
                    caps.push(10); // shell default line count
                }
            }
            "-m" => {
                if let Some(n) = tokens.get(i + 1).and_then(|t| t.parse().ok()) {
                    caps.push(n);
                }
            }
            _ => {
                if let Some(n) = tok
                    .strip_prefix("--max-count=")
                    .and_then(|r| r.parse().ok())
                {
                    caps.push(n);
                }
            }
        }
    }
    caps.into_iter().min()
}

/// Parse a `-50` style single flag into its number (`50`). Returns None for `-n`, `-rn`, etc.
fn dash_number(tok: &str) -> Option<usize> {
    tok.strip_prefix('-').filter(|r| !r.is_empty()).and_then(|r| r.parse().ok())
}

/// Prefix classification of a single, already-normalized command (no leading `cd`/env, no
/// pipeline). This is the original rule set, now applied to the effective verb rather than the raw
/// string.
fn classify_simple(command: &str) -> CompressKind {
    let lower = command.trim().to_lowercase();
    if lower.starts_with("git status") || lower == "git status" {
        return CompressKind::GitStatus;
    }
    if lower.starts_with("git diff") || lower.starts_with("git show") {
        return CompressKind::GitDiff;
    }
    if lower.starts_with("git log") {
        return CompressKind::GitLog;
    }
    if is_test_command(&lower) {
        return CompressKind::TestRunner;
    }
    if lower.starts_with("rg ")
        || lower.starts_with("grep ")
        || lower.starts_with("git grep")
        || lower.contains(" ripgrep")
    {
        return CompressKind::Grep;
    }
    CompressKind::Generic
}

/// Split a compound command on top-level `;`, `&&`, and `||`. Best-effort: it does not honor
/// quoting, which is safe here because a mis-split only changes which verb we inspect, never the
/// output itself. All separators are ASCII, so byte offsets are valid char boundaries.
fn split_segments(command: &str) -> Vec<&str> {
    let bytes = command.as_bytes();
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b';' {
            out.push(&command[start..i]);
            i += 1;
            start = i;
        } else if i + 1 < bytes.len()
            && ((bytes[i] == b'&' && bytes[i + 1] == b'&')
                || (bytes[i] == b'|' && bytes[i + 1] == b'|'))
        {
            out.push(&command[start..i]);
            i += 2;
            start = i;
        } else {
            i += 1;
        }
    }
    out.push(&command[start..]);
    out.into_iter().filter(|s| !s.trim().is_empty()).collect()
}

/// Strip a leading run of `NAME=value` environment assignments (`FOO=1 BAR=2 cmd` -> `cmd`), so the
/// command classifies by its real verb. Values may be quoted; the run ends at the first unquoted
/// whitespace after each assignment.
fn strip_env_assignments(seg: &str) -> &str {
    let mut s = seg.trim_start();
    while let Some(rest) = strip_one_env(s) {
        s = rest.trim_start();
    }
    s
}

fn strip_one_env(s: &str) -> Option<&str> {
    let bytes = s.as_bytes();
    if bytes.is_empty() || !(bytes[0].is_ascii_alphabetic() || bytes[0] == b'_') {
        return None;
    }
    let mut i = 1;
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
        i += 1;
    }
    if i >= bytes.len() || bytes[i] != b'=' {
        return None;
    }
    i += 1; // past '='
    let (mut in_single, mut in_double) = (false, false);
    while i < bytes.len() {
        let c = bytes[i];
        if in_single {
            if c == b'\'' {
                in_single = false;
            }
        } else if in_double {
            if c == b'"' {
                in_double = false;
            }
        } else if c == b'\'' {
            in_single = true;
        } else if c == b'"' {
            in_double = true;
        } else if c == b' ' || c == b'\t' {
            break;
        }
        i += 1;
    }
    Some(&s[i..])
}

fn is_test_command(lower: &str) -> bool {
    lower.contains("cargo test")
        || lower.contains("npm test")
        || lower.contains("pnpm test")
        || lower.contains("yarn test")
        || lower.contains("pytest")
        || lower.contains("go test")
        || lower.contains("jest")
        || lower.contains("vitest")
        || lower.contains(" ruff ")
        || lower.starts_with("ruff ")
}

/// True for an MCP tool on any surface: Claude Code's `mcp__server__tool` or Cursor's `MCP:tool`
/// (ADR 0018). One definition so classification, the allow-list, and the apply path all agree on
/// what counts as MCP regardless of which agent named it.
pub fn is_mcp_tool(tool_name: &str) -> bool {
    let name = tool_name.trim();
    name.starts_with("mcp__") || name.get(..4).is_some_and(|p| p.eq_ignore_ascii_case("mcp:"))
}

pub fn classify_tool(
    tool_name: &str,
    command: Option<&str>,
    file_path: Option<&str>,
) -> CompressKind {
    let name = tool_name.trim();
    // Edit/Write confirmations get their own strategy so the shadow measurement reflects real
    // savings on long-line echoes (CTX-60). This classification feeds `compute_shadow_decision`
    // (measurement) only: on the apply path, edit tools are absent from `compress_tools`, so
    // `compress_tool_output`'s `tool_allowed` gate returns None and the agent still sees the full
    // echo. So classifying an edit here never trims what the agent reads, it only measures.
    if crate::outcome_signals::is_edit_tool(name) {
        return CompressKind::Edit;
    }
    // Claude Code names the shell tool "Bash"; Cursor names it "Shell". They are the same surface,
    // so both classify by their command (git/grep/test) rather than falling through to Generic.
    if name.eq_ignore_ascii_case("bash") || name.eq_ignore_ascii_case("shell") {
        return classify_bash_command(command.unwrap_or(""));
    }
    if name.eq_ignore_ascii_case("read") {
        return CompressKind::Read;
    }
    if name.eq_ignore_ascii_case("grep") || name.eq_ignore_ascii_case("glob") {
        return CompressKind::Grep;
    }
    if is_mcp_tool(name) {
        return CompressKind::Mcp;
    }
    if file_path.is_some() && name.eq_ignore_ascii_case("read") {
        return CompressKind::Read;
    }
    CompressKind::Generic
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_commands_still_classify() {
        assert_eq!(classify_bash_command("git status"), CompressKind::GitStatus);
        assert_eq!(classify_bash_command("git diff HEAD"), CompressKind::GitDiff);
        assert_eq!(classify_bash_command("git log --oneline"), CompressKind::GitLog);
        assert_eq!(classify_bash_command("cargo test --lib"), CompressKind::TestRunner);
        assert_eq!(classify_bash_command("grep -n foo src/lib.rs"), CompressKind::Grep);
        assert_eq!(classify_bash_command("echo hi"), CompressKind::Generic);
    }

    #[test]
    fn cd_prefix_routes_to_the_real_verb() {
        // The exact failure from the attribution rows: a `cd … && grep …` classified as Generic and
        // got head-truncated. It must route to Grep now.
        assert_eq!(
            classify_bash_command("cd /Users/me/pkg/sim && grep -rn \"decideOnBall\" src/sim"),
            CompressKind::Grep
        );
        assert_eq!(
            classify_bash_command("cd repo && git diff"),
            CompressKind::GitDiff
        );
    }

    #[test]
    fn env_prefix_is_stripped_before_classify() {
        assert_eq!(
            classify_bash_command("TRANSITION=1 DR_GAIN=1.2 npx tsx src/sim/probe.ts"),
            CompressKind::Generic
        );
        // A test command behind env vars still classifies as a test run.
        assert_eq!(
            classify_bash_command("RUST_LOG=debug cargo test outcome"),
            CompressKind::TestRunner
        );
    }

    #[test]
    fn grep_producer_survives_a_downstream_pipe() {
        // `grep … | head -50` is grep output; the head is narrowing, not a reclassification.
        assert_eq!(
            classify_bash_command("grep -n \"a\\|b\\|c\" src/x.rs | head -50"),
            CompressKind::Grep
        );
    }

    #[test]
    fn explicit_output_cap_reads_head_tail_grep() {
        assert_eq!(explicit_output_cap("grep -n foo x.rs | head -50"), Some(50));
        assert_eq!(explicit_output_cap("cat big.log | tail -n 20"), Some(20));
        assert_eq!(explicit_output_cap("grep -m 30 pattern file"), Some(30));
        assert_eq!(explicit_output_cap("ls -la | head"), Some(10)); // bare head default
        // Tightest cap wins when several are present.
        assert_eq!(
            explicit_output_cap("grep x f | head -100 | tail -5"),
            Some(5)
        );
        assert_eq!(explicit_output_cap("cargo build"), None);
        // `-rn` on grep is flags, not a count.
        assert_eq!(explicit_output_cap("grep -rn foo src"), None);
    }

    #[test]
    fn first_structured_segment_wins_over_trailing_echo() {
        // `grep … && echo done` is dominated by the grep output, not the trailing echo.
        assert_eq!(
            classify_bash_command("grep -rn TODO src && echo done"),
            CompressKind::Grep
        );
    }
}
