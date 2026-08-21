# ctx installer for Windows. Downloads the latest release binary from GitHub, verifies its
# checksum, installs it to $env:LOCALAPPDATA\ctx\bin, and runs `ctx setup`.
#
#   irm https://raw.githubusercontent.com/saurabh0392/ctx/main/scripts/install.ps1 | iex
#
# Prefer `cargo install ctx-agent` when you have a Rust toolchain.
$ErrorActionPreference = 'Stop'

$repo = 'saurabh0392/ctx'
$target = 'x86_64-pc-windows-msvc'
$asset = "ctx-$target.tar.gz"
$installDir = Join-Path $env:LOCALAPPDATA 'ctx\bin'

$base = "https://github.com/$repo/releases/latest/download"
$tmp = Join-Path $env:TEMP ("ctx-install-" + [System.Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
    Write-Host "downloading $asset from $repo latest release..."
    try {
        Invoke-WebRequest -Uri "$base/$asset" -OutFile (Join-Path $tmp $asset)
    } catch {
        throw "no build for $target in the latest release. Use 'cargo install ctx-agent' instead."
    }
    Invoke-WebRequest -Uri "$base/checksums.txt" -OutFile (Join-Path $tmp 'checksums.txt')

    $line = (Get-Content (Join-Path $tmp 'checksums.txt')) | Where-Object { $_ -match [regex]::Escape($asset) }
    if (-not $line) { throw "checksums.txt has no entry for $asset" }
    $expected = ($line -split '\s+')[0].ToLower()
    $actual = (Get-FileHash -Algorithm SHA256 (Join-Path $tmp $asset)).Hash.ToLower()
    if ($expected -ne $actual) { throw "checksum mismatch for $asset" }

    tar -xzf (Join-Path $tmp $asset) -C $tmp
    if (-not (Test-Path (Join-Path $tmp 'ctx.exe'))) { throw 'archive did not contain ctx.exe' }

    New-Item -ItemType Directory -Force -Path $installDir | Out-Null
    Copy-Item (Join-Path $tmp 'ctx.exe') (Join-Path $installDir 'ctx.exe') -Force
    Write-Host "installed ctx to $installDir\ctx.exe"

    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if ($userPath -notlike "*$installDir*") {
        [Environment]::SetEnvironmentVariable('Path', "$userPath;$installDir", 'User')
        Write-Host "added $installDir to your user PATH (open a new terminal to pick it up)"
    }

    & (Join-Path $installDir 'ctx.exe') setup
} finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
