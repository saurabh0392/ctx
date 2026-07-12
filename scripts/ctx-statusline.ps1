# ctx-managed Claude Code statusLine (PowerShell mirror of ctx-statusline.sh).
# Shows model + allowance and records a snapshot for the dashboard. No bash on Windows.
$ErrorActionPreference = 'SilentlyContinue'
$payload = [Console]::In.ReadToEnd()
$port = '__DASHBOARD_PORT__'

# Best-effort snapshot for the dashboard allowance meters. Bounded so a down dashboard cannot hang
# the status line; Claude Code's own statusLine timeout is the backstop.
try {
    Invoke-RestMethod -Uri "http://127.0.0.1:$port/api/allowance/snapshot" -Method Post `
        -ContentType 'application/json' -Body $payload -TimeoutSec 1 | Out-Null
} catch {}

$model = 'Claude'
$ctxPct = $null; $five = $null; $seven = $null
try {
    $data = $payload | ConvertFrom-Json
    if ($data.model.display_name) { $model = $data.model.display_name }
    elseif ($data.model.id) { $model = $data.model.id }
    $ctxPct = $data.context_window.used_percentage
    $five = $data.rate_limits.five_hour.used_percentage
    $seven = $data.rate_limits.seven_day.used_percentage
} catch {}

function Floor-Pct($v) { [int][math]::Floor([double]$v) }

$parts = @()
if ($null -ne $ctxPct) { $parts += ("ctx {0}%" -f (Floor-Pct $ctxPct)) }
if ($null -ne $five)   { $parts += ("5h {0}%"  -f (Floor-Pct $five)) }
if ($null -ne $seven)  { $parts += ("7d {0}%"  -f (Floor-Pct $seven)) }

$esc = [char]27
if ($parts.Count -gt 0) {
    $joined = [string]::Join(' · ', $parts)
    [Console]::Out.Write("$esc[90m$model$esc[0m  $joined")
} else {
    [Console]::Out.Write("$esc[90m$model$esc[0m")
}
