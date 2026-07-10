#!/usr/bin/env bash
# Install ctx for Claude Desktop-only users.
# Run from any terminal: bash <(curl -sL <raw-url>) or ./scripts/install-desktop.sh [--yes|-y]
set -euo pipefail

REPO_URL="https://github.com/saurabh0392/ctx.git"
REPO_DIR="$HOME/Documents/ctx"
DASHBOARD_PORT=8789
GITHUB_MCP_BIN="$HOME/.local/bin/github-mcp-server"

AUTO_YES=false
for arg in "$@"; do
    case "$arg" in
        --yes|-y) AUTO_YES=true ;;
    esac
done

green()  { printf '\033[32m%s\033[0m\n' "$1"; }
yellow() { printf '\033[33m%s\033[0m\n' "$1"; }
red()    { printf '\033[31m%s\033[0m\n' "$1"; }
bold()   { printf '\033[1m%s\033[0m\n' "$1"; }

confirm() {
    if $AUTO_YES; then
        return 0
    fi
    printf '%s [Y/n] ' "$1"
    read -r answer || true
    case "$answer" in
        [nN]*) return 1 ;;
        *) return 0 ;;
    esac
}

# Idempotent: overwrites mcpServers.github each run.
merge_github_mcp() {
    local config_path="$1"
    local token="$2"
    local bin_path="$3"
    python3 -c "
import json, sys

config_path, token, bin_path = sys.argv[1], sys.argv[2], sys.argv[3]
server = {
    'command': bin_path,
    'args': ['stdio'],
    'env': {'GITHUB_PERSONAL_ACCESS_TOKEN': token},
}
with open(config_path) as f:
    config = json.load(f)
if 'mcpServers' not in config:
    config['mcpServers'] = {}
config['mcpServers']['github'] = server
with open(config_path, 'w') as f:
    json.dump(config, f, indent=2)
    f.write('\n')
" "$config_path" "$token" "$bin_path"
}

# Returns asset suffix (e.g. Darwin_arm64) on stdout, exit 1 if unsupported.
github_mcp_asset_suffix() {
    local os arch
    os=$(uname -s)
    arch=$(uname -m)
    case "${os}:${arch}" in
        Darwin:arm64 | Darwin:aarch64) echo "Darwin_arm64" ;;
        Darwin:x86_64 | Darwin:amd64) echo "Darwin_x86_64" ;;
        Linux:arm64 | Linux:aarch64) echo "Linux_arm64" ;;
        Linux:x86_64 | Linux:amd64) echo "Linux_x86_64" ;;
        *) return 1 ;;
    esac
}

# Downloads official github-mcp-server binary to GITHUB_MCP_BIN. Exit 0 on success.
download_github_mcp_binary() {
    local suffix url tmpdir found
    if ! suffix=$(github_mcp_asset_suffix); then
        yellow "  Unsupported OS/arch for pre-built GitHub MCP server ($(uname -s) $(uname -m))."
        return 1
    fi
    if ! command -v curl &>/dev/null; then
        yellow "  curl not found. Install curl, then re-run this script."
        return 1
    fi
    url="https://github.com/github/github-mcp-server/releases/latest/download/github-mcp-server_${suffix}.tar.gz"
    tmpdir=$(mktemp -d) || return 1
    echo "  Downloading GitHub MCP server (${suffix})..."
    if ! curl -fsSL "$url" -o "$tmpdir/archive.tar.gz"; then
        red "  Download failed (network or GitHub releases)."
        rm -rf "$tmpdir"
        return 1
    fi
    if ! tar -xzf "$tmpdir/archive.tar.gz" -C "$tmpdir"; then
        red "  Could not extract release archive."
        rm -rf "$tmpdir"
        return 1
    fi
    found=$(find "$tmpdir" -name 'github-mcp-server' -type f 2>/dev/null | head -1)
    if [ -z "$found" ]; then
        red "  Could not find github-mcp-server binary inside release archive."
        rm -rf "$tmpdir"
        return 1
    fi
    mkdir -p "$(dirname "$GITHUB_MCP_BIN")"
    mv "$found" "$GITHUB_MCP_BIN"
    chmod +x "$GITHUB_MCP_BIN"
    rm -rf "$tmpdir"
    green "  Installed $GITHUB_MCP_BIN"
    return 0
}

# ── Step 1: Rust ─────────────────────────────────────────────────────────────
echo ""
bold "Step 1/7: Checking Rust toolchain"

# shellcheck disable=SC1091
[ -f "$HOME/.cargo/env" ] && source "$HOME/.cargo/env"

if ! command -v rustc &>/dev/null; then
    red "Rust is not installed."
    echo ""
    echo "Install it now with:"
    echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    echo ""
    echo "After installation finishes, open a new terminal and re-run this script."
    exit 1
fi
green "  rustc $(rustc --version | awk '{print $2}') found"

# ── Step 2: Source ────────────────────────────────────────────────────────────
echo ""
bold "Step 2/7: Getting ctx source"

if [ -f "$REPO_DIR/Cargo.toml" ]; then
    cd "$REPO_DIR"
    if git remote get-url origin 2>/dev/null | grep -q "saurabh0392/ctx"; then
        echo "  Pulling latest from main..."
        git pull origin main
    else
        yellow "  Repo exists but remote differs. Building from existing source."
    fi
else
    echo "  Cloning into $REPO_DIR..."
    git clone "$REPO_URL" "$REPO_DIR"
    cd "$REPO_DIR"
fi
green "  Source ready at $REPO_DIR"

# ── Step 3: Build ────────────────────────────────────────────────────────────
echo ""
bold "Step 3/7: Building ctx"
cargo install --locked --path "$REPO_DIR"
green "  ctx installed to $(command -v ctx || echo '~/.cargo/bin/ctx')"

# ── Step 4: Setup ────────────────────────────────────────────────────────────
echo ""
bold "Step 4/7: Running ctx setup"
ctx setup --yes
green "  Setup complete"

# ── Step 5: Verify ───────────────────────────────────────────────────────────
echo ""
bold "Step 5/7: Verifying"

TRIES=0
DASHBOARD_OK=false
while [ $TRIES -lt 10 ]; do
    CODE=$(curl -s -o /dev/null -w "%{http_code}" "http://127.0.0.1:$DASHBOARD_PORT/" 2>/dev/null || echo "000")
    if [ "$CODE" = "200" ]; then
        DASHBOARD_OK=true
        break
    fi
    sleep 1
    TRIES=$((TRIES + 1))
done

if $DASHBOARD_OK; then
    green "  Dashboard: http://127.0.0.1:$DASHBOARD_PORT (200 OK)"
else
    yellow "  Dashboard: http://127.0.0.1:$DASHBOARD_PORT (not responding yet, check manually)"
fi

if ctx status &>/dev/null; then
    green "  ctx status: OK"
else
    yellow "  ctx status: returned non-zero (check output manually)"
fi

# ── Step 6: Ingest ───────────────────────────────────────────────────────────
echo ""
bold "Step 6/7: Populating session data"
ctx ingest 2>/dev/null && green "  Ingest complete" || yellow "  No sessions to ingest yet (expected on first run)"

# ── Step 7: GitHub MCP server (binary, interactive) ─────────────────────────
echo ""
bold "Step 7/7: GitHub MCP server"

GITHUB_MCP_OK=false

if ! command -v python3 &>/dev/null; then
    yellow "  python3 not found. Skipping GitHub MCP server."
    echo "  Install Python 3 (e.g. Xcode Command Line Tools on macOS), then re-run this script."
elif ! confirm "Install the GitHub MCP server? (PRs, issues, repos from chat; uses ~/.local/bin/github-mcp-server)"; then
    yellow "  Skipped GitHub MCP server."
else
    if ! download_github_mcp_binary; then
        yellow "  GitHub MCP binary install failed. Skipping config merge."
    elif ! confirm "Read your GitHub token from the gh CLI?"; then
        yellow "  Skipped token read. Binary is at $GITHUB_MCP_BIN — configure MCP manually if needed."
    elif ! command -v gh &>/dev/null; then
        yellow "  gh CLI not found. Install https://cli.github.com then re-run, or paste a PAT when we add manual entry."
    else
        GH_TOKEN=$(gh auth token 2>/dev/null || true)
        if [ -z "$GH_TOKEN" ]; then
            yellow "  gh is not authenticated. Run: gh auth login"
        else
            preview=$(printf '%.8s' "$GH_TOKEN")
            echo "  Token preview: ${preview}****"
            if ! confirm "Use this token for the GitHub MCP server in JSON configs?"; then
                yellow "  Skipped writing token to configs. Binary remains at $GITHUB_MCP_BIN."
            else
                BIN_ABS=$(cd "$(dirname "$GITHUB_MCP_BIN")" && pwd)/$(basename "$GITHUB_MCP_BIN")

                DESKTOP_CONFIG=""
                case "$(uname -s)" in
                    Darwin)
                        DESKTOP_CONFIG="$HOME/Library/Application Support/Claude/claude_desktop_config.json"
                        ;;
                    Linux)
                        DESKTOP_CONFIG="$HOME/.config/Claude/claude_desktop_config.json"
                        ;;
                esac

                if [ -n "$DESKTOP_CONFIG" ] && [ -f "$DESKTOP_CONFIG" ]; then
                    if confirm "Add GitHub MCP server to claude_desktop_config.json?"; then
                        merge_github_mcp "$DESKTOP_CONFIG" "$GH_TOKEN" "$BIN_ABS"
                        green "  GitHub MCP merged into claude_desktop_config.json"
                        GITHUB_MCP_OK=true
                    fi
                elif [ -n "$DESKTOP_CONFIG" ]; then
                    yellow "  claude_desktop_config.json not found. Skipping Desktop MCP merge."
                fi

                CURSOR_MCP="$HOME/.cursor/mcp.json"
                if [ -d "$HOME/.cursor" ]; then
                    if confirm "Add GitHub MCP server to ~/.cursor/mcp.json?"; then
                        if [ ! -f "$CURSOR_MCP" ]; then
                            echo '{"mcpServers":{}}' > "$CURSOR_MCP"
                        fi
                        merge_github_mcp "$CURSOR_MCP" "$GH_TOKEN" "$BIN_ABS"
                        green "  GitHub MCP merged into ~/.cursor/mcp.json"
                        GITHUB_MCP_OK=true
                    fi
                fi
            fi
        fi
    fi
fi

# ── Done ─────────────────────────────────────────────────────────────────────
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
green "ctx is installed."
echo ""
bold "Next: Quit Claude Desktop completely and reopen it."
echo "  Desktop reads MCP config only on startup. The ctx MCP server"
echo "  (ctx_spend, ctx_sessions, ctx_tips, etc.) will appear after restart."
if $GITHUB_MCP_OK; then
    echo ""
    echo "  GitHub MCP uses the binary at $GITHUB_MCP_BIN (no Docker)."
fi
echo ""
echo "  Dashboard: http://127.0.0.1:$DASHBOARD_PORT"
echo ""
echo "  Tool filtering (NODE_OPTIONS / filter.js) is not available on Desktop"
echo "  alone. Install the Claude Code CLI for full token savings."
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
