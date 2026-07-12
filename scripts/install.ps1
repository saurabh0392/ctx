# ctx installer for Windows. No repo access, no gh, no Rust.
#
#   $env:CTX_TOKEN='<your-alpha-token>'; irm <endpoint>/install.ps1 | iex
#
# It asks the ctx distribution endpoint for a short-lived download link (gated by your token),
# verifies the checksum, installs the binary to %LOCALAPPDATA%\ctx, and runs `ctx setup`.

# Piped through `iex`, so use throw/return (not exit) to avoid killing the caller's shell.
& {
    $ErrorActionPreference = 'Stop'
    # Windows PowerShell 5.1 defaults to TLS 1.0/1.1, which API Gateway and S3 reject.
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

    # The endpoint is templated in when this script is served. CTX_ENDPOINT can override it for testing.
    $endpoint = if ($env:CTX_ENDPOINT) { $env:CTX_ENDPOINT } else { '__CTX_ENDPOINT__' }
    $installDir = if ($env:CTX_INSTALL_DIR) { $env:CTX_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'ctx' }

    try {
        # Sentinel assembled from two pieces so serve-time templating (which replaces the contiguous
        # placeholder) cannot rewrite this guard. If the placeholder was not replaced, refuse to run.
        $ph = '__CTX_' + 'ENDPOINT__'
        if ($endpoint -like "$ph*") {
            throw 'no endpoint configured. Fetch this script from the ctx distribution URL.'
        }
        if (-not $env:CTX_TOKEN) {
            throw "set CTX_TOKEN to your alpha token, e.g. `$env:CTX_TOKEN='xxxx'"
        }

        $target = 'x86_64-pc-windows-msvc'

        # --- ask the endpoint for a signed download link ---------------------------
        Write-Host "Requesting ctx for $target..."
        $body = @{ token = $env:CTX_TOKEN; target = $target } | ConvertTo-Json -Compress
        try {
            $resp = Invoke-RestMethod -Uri $endpoint -Method Post -ContentType 'application/json' -Body $body
        } catch {
            throw "the endpoint rejected the request (token invalid, revoked, or no build for $target)"
        }
        if (-not $resp.url) { throw "no download url returned: $($resp | ConvertTo-Json -Compress)" }

        # --- download + verify -----------------------------------------------------
        $tmp = Join-Path ([IO.Path]::GetTempPath()) ("ctx-" + [Guid]::NewGuid().ToString('N'))
        New-Item -ItemType Directory -Path $tmp -Force | Out-Null
        try {
            $archive = Join-Path $tmp 'ctx.tar.gz'
            $version = if ($resp.version) { $resp.version } else { 'latest' }
            Write-Host "Downloading ctx $version..."
            Invoke-WebRequest -Uri $resp.url -OutFile $archive -UseBasicParsing

            if ($resp.sha256) {
                $got = (Get-FileHash -Algorithm SHA256 -Path $archive).Hash
                if ($got -ine $resp.sha256) {
                    throw "checksum mismatch (expected $($resp.sha256), got $($got.ToLower())). Aborting."
                }
                Write-Host 'Checksum verified.'
            }

            # Windows 10 1803+ ships bsdtar as tar.exe; the archive holds ctx.exe.
            if (-not (Get-Command tar.exe -ErrorAction SilentlyContinue)) {
                throw 'tar.exe not found. Needs Windows 10 1803 or newer.'
            }
            & tar.exe -xzf $archive -C $tmp
            $extracted = Join-Path $tmp 'ctx.exe'
            if (-not (Test-Path $extracted)) { throw 'archive did not contain ctx.exe' }

            # --- install -----------------------------------------------------------
            New-Item -ItemType Directory -Path $installDir -Force | Out-Null
            $bin = Join-Path $installDir 'ctx.exe'
            Copy-Item -Path $extracted -Destination $bin -Force
            # Strip Mark-of-the-Web so later manual runs do not hit SmartScreen.
            Unblock-File -Path $bin -ErrorAction SilentlyContinue
        } finally {
            Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
        }

        # --- PATH (user scope, persistent + this session) --------------------------
        $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
        if (-not $userPath) { $userPath = '' }
        $onPath = ($userPath -split ';') -contains $installDir
        if (-not $onPath) {
            $newPath = if ($userPath.TrimEnd(';')) { "$($userPath.TrimEnd(';'));$installDir" } else { $installDir }
            [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
            Write-Host "Added $installDir to your user PATH (open a new terminal to use ``ctx`` directly)."
        }
        if (($env:Path -split ';') -notcontains $installDir) {
            $env:Path = "$($env:Path.TrimEnd(';'));$installDir"
        }

        # --- wire into your agent --------------------------------------------------
        # --yes: no TTY when piped through iex, so setup must not prompt.
        Write-Host 'Setting up ctx...'
        & $bin setup --yes
        if ($LASTEXITCODE -ne 0) { throw 'ctx setup failed' }

        Write-Host ''
        Write-Host 'ctx is installed. Dashboard: http://127.0.0.1:8789'
    } catch {
        Write-Host "error: $($_.Exception.Message)" -ForegroundColor Red
        return
    }
}
