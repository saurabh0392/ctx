use super::types::CompressKind;

pub fn classify_bash_command(command: &str) -> CompressKind {
    let c = command.trim();
    let lower = c.to_lowercase();
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

pub fn classify_tool(
    tool_name: &str,
    command: Option<&str>,
    file_path: Option<&str>,
) -> CompressKind {
    let name = tool_name.trim();
    if name.eq_ignore_ascii_case("bash") {
        return classify_bash_command(command.unwrap_or(""));
    }
    if name.eq_ignore_ascii_case("read") {
        return CompressKind::Read;
    }
    if name.eq_ignore_ascii_case("grep") || name.eq_ignore_ascii_case("glob") {
        return CompressKind::Grep;
    }
    if name.starts_with("mcp__") {
        return CompressKind::Mcp;
    }
    if file_path.is_some() && name.eq_ignore_ascii_case("read") {
        return CompressKind::Read;
    }
    CompressKind::Generic
}
