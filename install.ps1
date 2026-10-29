# kikitori binary installer for Windows PowerShell 5.1. ASCII only.
param([switch]$Uninstall)
$ErrorActionPreference = 'Stop'
$HomeDir = $env:USERPROFILE
$Root = if ($env:KIKITORI_INSTALL_DIR) { $env:KIKITORI_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'Programs\kikitori' }
$Receipt = Join-Path $env:APPDATA 'kikitori\receipt.txt'
# Mirror base() in crates/kikitori-refine/src/ollama.rs: add the scheme, the default
# port for http, and treat the wildcard address as localhost.
$OllamaValue = if ($env:KIKITORI_OLLAMA) { $env:KIKITORI_OLLAMA } elseif ($env:OLLAMA_HOST) { $env:OLLAMA_HOST } else { 'localhost:11434' }
if ($OllamaValue -notmatch '^[a-zA-Z][a-zA-Z0-9+.-]*://') { $OllamaValue = "http://$OllamaValue" }
$OllamaParts = $OllamaValue -split '://', 2
$OllamaScheme = $OllamaParts[0]
$OllamaRest = $OllamaParts[1].TrimEnd('/')
if ($OllamaScheme -eq 'http') {
    $OllamaTail = if ($OllamaRest.Contains(']')) { $OllamaRest.Substring($OllamaRest.LastIndexOf(']')) } else { $OllamaRest }
    if (-not $OllamaTail.Contains(':')) { $OllamaRest = "${OllamaRest}:11434" }
}
if ($OllamaRest.StartsWith('0.0.0.0:')) { $OllamaRest = 'localhost:' + $OllamaRest.Substring($OllamaRest.IndexOf(':') + 1) }
elseif ($OllamaRest -eq '0.0.0.0') { $OllamaRest = 'localhost:11434' }
$Ollama = "${OllamaScheme}://$OllamaRest"
# Windows PowerShell 5.1 turns a native program's stderr into errors when output is captured
# (2>&1, or a caller reading the pipes). Run native programs with errors left to $LASTEXITCODE.
function Invoke-Native([scriptblock]$Block) {
    $saved = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try { & $Block } finally { $ErrorActionPreference = $saved }
}
function Add-Receipt([string]$Kind, [string]$Value) {
    $parent = Split-Path $Receipt
    New-Item -ItemType Directory -Force -Path $parent | Out-Null
    Add-Content -Encoding ASCII -Path $Receipt -Value "$Kind`t$Value"
}
function Get-Asset([string]$Name, [string]$Destination) {
    if ($Base -match '^https?://') {
        $savedProgress = $ProgressPreference
        $ProgressPreference = 'SilentlyContinue'
        [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
        try { Invoke-WebRequest -UseBasicParsing "$Base/$Name" -OutFile $Destination }
        finally { $ProgressPreference = $savedProgress }
    }
    else {
        $localBase = $Base -replace '^file://', ''
        Copy-Item -LiteralPath (Join-Path $localBase $Name) -Destination $Destination
    }
}
if ($Uninstall) {
    if (Test-Path $Receipt) {
        foreach ($line in [IO.File]::ReadAllLines($Receipt)) {
            $parts = $line -split "`t", 2
            if ($parts.Length -eq 2 -and $parts[0] -in @('bin','skill')) { Remove-Item -Force -ErrorAction SilentlyContinue $parts[1] }
            if ($parts.Length -eq 2 -and $parts[0] -in @('models-dir','hf-repo','dict')) { Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $parts[1] }
            if ($parts.Length -eq 2 -and $parts[0] -eq 'ollama-model') {
                try { Invoke-RestMethod -Method Delete -Uri "$Ollama/api/delete" -ContentType 'application/json' -Body (@{name=$parts[1]} | ConvertTo-Json) | Out-Null }
                catch { Write-Host "Ollama is not running; kept $($parts[1]). Remove it with: ollama rm $($parts[1])" }
            }
            if ($parts.Length -eq 2 -and $parts[0] -eq 'path-line') {
                $userPath = [Environment]::GetEnvironmentVariable('Path','User') -split ';' | Where-Object { $_ -ne $parts[1] }
                [Environment]::SetEnvironmentVariable('Path', ($userPath -join ';'), 'User')
            }
        }
        Remove-Item -Force $Receipt
    }
    if ($env:KIKITORI_KEEP_MODELS -ne '1') { Remove-Item -Recurse -Force -ErrorAction SilentlyContinue (Join-Path $HomeDir '.cache\kikitori\models') }
    $ConfigDir = Join-Path $env:APPDATA 'kikitori'
    if ((Test-Path $ConfigDir) -and -not (Get-ChildItem -Force $ConfigDir | Select-Object -First 1)) { Remove-Item -Force $ConfigDir }
    exit 0
}
$Arch = [Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
if ($Arch -eq 'X64') { $Triple = 'x86_64-pc-windows-msvc' } elseif ($Arch -eq 'Arm64') { $Triple = 'aarch64-pc-windows-msvc' } else { throw "Unsupported architecture: $Arch" }
$Variant = if ($env:KIKITORI_VARIANT) { $env:KIKITORI_VARIANT } else { 'cpu' }
if ($Variant -eq 'cuda' -and $Arch -ne 'X64') { throw 'CUDA is supported only on Windows x64' }
$Suffix = if ($Variant -eq 'cuda') { '-cuda' } else { '' }
if ($env:KIKITORI_UPGRADE) { Write-Host 'KIKITORI_UPGRADE is ignored; installs replace the existing binary.' }
if ($env:KIKITORI_NO_HF) { Write-Host 'KIKITORI_NO_HF is treated as KIKITORI_NO_REFINE.'; if ($env:KIKITORI_NO_HF -eq '1') { $env:KIKITORI_NO_REFINE = '1' } }
$Version = if ($env:KIKITORI_VERSION) { $env:KIKITORI_VERSION } else { 'latest' }
$Repo = if ($env:KIKITORI_REPO) { $env:KIKITORI_REPO } else { 'kulikala/kikitori' }
if ($Version -eq 'latest' -and -not $env:KIKITORI_BASE_URL) {
    # Resolve the latest version by following the redirect of /releases/latest
    # (the redirect target ends in /tag/vX.Y.Z); avoids the rate-limited API.
    $req = [Net.HttpWebRequest]::Create("https://github.com/$Repo/releases/latest")
    $req.AllowAutoRedirect = $false
    try { $resp = $req.GetResponse(); $loc = $resp.Headers['Location']; $resp.Close() }
    catch { throw "Could not resolve the latest version of $Repo (network or repository problem)" }
    if (-not $loc) { throw 'Could not resolve the latest version; empty redirect' }
    $Version = ($loc -split '/tag/v')[-1]
    if ($Version -eq $loc -or $Version -notmatch '^[0-9.]+$') { throw "Unexpected latest tag: $loc" }
}
if ($env:KIKITORI_BASE_URL) {
    $Base = $env:KIKITORI_BASE_URL.TrimEnd('/')
    if ($Version -eq 'latest') {
        # A base URL without a version (a local dir most likely): pick the
        # newest matching asset and read the version from its name.
        $localBase = $Base -replace '^file://', ''
        if ($Base -match '^https?://') { throw 'KIKITORI_BASE_URL cannot resolve "latest"; pin KIKITORI_VERSION or use a local directory' }
        $cand = Get-ChildItem -LiteralPath $localBase -Filter "kikitori-*$Triple$Suffix.zip" | Sort-Object Name | Select-Object -Last 1
        if (-not $cand) { throw 'No matching asset was found in the base URL directory' }
        $Version = ($cand.Name -replace '^kikitori-', '') -replace "-$Triple$Suffix\.zip$", ''
        $Base = $localBase
    }
} else {
    $Base = "https://github.com/$Repo/releases/download/v$Version"
}
$Asset = "kikitori-$Version-$Triple$Suffix.zip"
$Temp = Join-Path ([IO.Path]::GetTempPath()) ([Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $Temp | Out-Null
try {
    Get-Asset $Asset (Join-Path $Temp $Asset)
    Get-Asset 'SHA256SUMS' (Join-Path $Temp 'SHA256SUMS')
    $line = [IO.File]::ReadAllLines((Join-Path $Temp 'SHA256SUMS')) | Where-Object { $_ -match "\s\*?$([regex]::Escape($Asset))$" } | Select-Object -First 1
    if (-not $line) { throw 'Asset is missing from SHA256SUMS' }
    $expected = ($line -split '\s+')[0].ToLowerInvariant()
    $actual = (Get-FileHash (Join-Path $Temp $Asset) -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($expected -ne $actual) { throw 'SHA-256 mismatch' }
    Expand-Archive (Join-Path $Temp $Asset) (Join-Path $Temp 'unpacked')
    $Candidate = Join-Path $Temp 'unpacked\kikitori.exe'
    Invoke-Native { & $Candidate --version }; if ($LASTEXITCODE -ne 0) { throw 'Downloaded binary failed --version' }
    $Uv = Get-Command uv -ErrorAction SilentlyContinue
    if ($Uv -and ((Invoke-Native { & uv tool list 2>$null }) -match '^kikitori\s')) {
        if ($env:KIKITORI_YES -ne '1') { $answer = Read-Host 'Remove Python kikitori installed by uv? [y/N]'; if ($answer -notmatch '^(y|yes)$') { throw 'Installation cancelled' } }
        Invoke-Native { & uv tool uninstall kikitori }; if ($LASTEXITCODE -ne 0) { throw 'uv tool uninstall failed' }
    }
    New-Item -ItemType Directory -Force $Root | Out-Null
    $Dest = Join-Path $Root 'kikitori.exe'; $Old = "$Dest.old"
    if (Test-Path $Dest) { Remove-Item -Force -ErrorAction SilentlyContinue $Old; Move-Item -Force $Dest $Old }
    Move-Item $Candidate $Dest
    Invoke-Native { & $Dest --version }
    if ($LASTEXITCODE -ne 0) { Remove-Item -Force $Dest; if (Test-Path $Old) { Move-Item $Old $Dest }; throw 'Installed binary failed; previous version restored' }
    if (Test-Path $Old) { Remove-Item -Force $Old }
    $Refine = Join-Path $Root 'kikitori-refine.cmd'
    Set-Content -Encoding ASCII $Refine '@echo off' ; Add-Content -Encoding ASCII $Refine '"%~dp0kikitori.exe" refine %*'
    Add-Receipt bin $Dest; Add-Receipt bin $Refine; Add-Receipt variant $Variant
    if ($env:KIKITORI_NO_MODIFY_PATH -ne '1') {
        $UserPath = [Environment]::GetEnvironmentVariable('Path','User')
        if (($UserPath -split ';') -notcontains $Root) { [Environment]::SetEnvironmentVariable('Path', (($UserPath.TrimEnd(';') + ';' + $Root).Trim(';')), 'User'); Add-Receipt path-line $Root }
    }
    # The new user PATH reaches new shells only; check the lookup order this shell would get.
    $MachinePath = [Environment]::GetEnvironmentVariable('Path','Machine')
    $env:Path = (@($MachinePath, [Environment]::GetEnvironmentVariable('Path','User')) | Where-Object { $_ }) -join ';'
    if ($env:KIKITORI_NO_MODIFY_PATH -eq '1') { $env:Path = "$Root;$env:Path" }
    $resolved = (Invoke-Native { where.exe kikitori 2>$null } | Select-Object -First 1)
    if ($resolved -and [IO.Path]::GetFullPath($resolved) -ne [IO.Path]::GetFullPath($Dest)) { throw "Another kikitori shadows the installed binary: $resolved" }
    if ($env:KIKITORI_YES -eq '1') { Invoke-Native { & $Dest --yes setup } } else { Invoke-Native { & $Dest setup } }
    if ($LASTEXITCODE -ne 0) { Write-Host "Note: kikitori setup stopped (exit $LASTEXITCODE). Run 'kikitori setup' later to fetch the models." }
    if ($env:KIKITORI_NO_REFINE -ne '1') {
        Invoke-Native { & $Dest refine --setup }
        if ($LASTEXITCODE -ne 0) { Write-Host "Note: kikitori refine --setup stopped (exit $LASTEXITCODE). Run it later to prepare refine." }
    }
} finally { Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $Temp }
