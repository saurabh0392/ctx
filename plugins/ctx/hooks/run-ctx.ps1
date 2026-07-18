$ErrorActionPreference = "SilentlyContinue"

if ($env:CTX_BIN -and (Test-Path -LiteralPath $env:CTX_BIN)) {
    & $env:CTX_BIN @args
    exit $LASTEXITCODE
}

$ctxCommand = Get-Command ctx -ErrorAction SilentlyContinue
if ($ctxCommand) {
    & $ctxCommand.Source @args
    exit $LASTEXITCODE
}

$fallback = Join-Path $HOME ".local\bin\ctx.exe"
if (Test-Path -LiteralPath $fallback) {
    & $fallback @args
    exit $LASTEXITCODE
}

if (($args -join " ") -match "codex-session-start") {
    Write-Output '{"systemMessage":"CTX plugin is installed, but the ctx binary is unavailable. Run the CTX installer, then restart Codex."}'
} else {
    Write-Output '{}'
}
exit 0
