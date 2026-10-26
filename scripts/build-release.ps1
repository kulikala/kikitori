# Build and package one release binary of kikitori (Windows host, PowerShell 5.1). ASCII only.
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('x86_64-pc-windows-msvc', 'aarch64-pc-windows-msvc')]
    [string]$Target,
    [switch]$Cuda
)
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')

# Feature policy (D-4 of the design doc, plan/02-design.md):
#   Windows CPU builds on both archs; --cuda on x86_64 only (cudarc loads libcuda at runtime).
#   RUSTFLAGS pins the CRT to the static variant sherpa-onnx's static-MT archive needs.
# Before the kikitori crate declares every engine feature, a configured feature
# that is missing aborts the build. KIKITORI_ALLOW_MISSING_FEATURES=1 drops it
# with a loud warning (for packaging tryouts).
$Suffix = ''
$Features = @()
if ($Target -ne 'x86_64-pc-windows-msvc' -and $Cuda) { throw '--cuda is only offered for x86_64 Windows' }
if ($Cuda) { $Features += 'cuda'; $Suffix = '-cuda' }
$Features = $Features | Where-Object { $_ }

$Cargo = Get-Command cargo -ErrorAction SilentlyContinue
if (-not $Cargo) { throw 'cargo is not on PATH' }
# Native stderr (cargo's progress) must not stop the script when output is captured.
function Invoke-Native([scriptblock]$Block) {
    $saved = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try { & $Block } finally { $ErrorActionPreference = $saved }
}
# cargo prints UTF-8 (the crate descriptions are Japanese); PowerShell 5.1 decodes with the OEM code page otherwise.
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$Meta = (Invoke-Native { & cargo metadata --format-version 1 2>$null }) | Out-String | ConvertFrom-Json
$pkg = $Meta.packages | Where-Object { $_.name -eq 'kikitori' }
if (-not $pkg) { throw 'cannot determine the version of the kikitori package' }
$Version = $pkg.version
$Declared = @($pkg.features | Get-Member -MemberType NoteProperty | ForEach-Object { $_.Name })
foreach ($f in $Features) {
    if ($Declared -notcontains $f) {
        if ($env:KIKITORI_ALLOW_MISSING_FEATURES -eq '1') {
            Write-Host "build-release: WARNING feature $f is not declared by the kikitori crate; building WITHOUT it"
        } else {
            throw "the kikitori crate does not declare the feature $f; wire engines first (or set KIKITORI_ALLOW_MISSING_FEATURES=1)"
        }
    }
}
$Features = @($Features | Where-Object { $Declared -contains $_ })

$env:RUSTFLAGS = '-C target-feature=+crt-static'
if ($Cuda) {
    # PTX for compute capability 8.0 (Ampere); the driver JIT-compiles it for newer GPUs.
    if (-not $env:CUDA_COMPUTE_CAP) { $env:CUDA_COMPUTE_CAP = '80' }
    # Link the CUDA runtime statically so the binary starts without the toolkit;
    # cudarc opens nvcuda, cuBLAS and cuRAND at run time.
    if (-not $env:CUDA_PATH) { throw 'CUDA_PATH is not set (scripts/fetch-cuda-windows.ps1 prints a folder to use)' }
    # nvcc of CUDA 12.6 accepts MSVC up to 14.4x (VS 2022); the STL of newer MSVC fails its
    # static asserts. Load the x64-target environment of that toolset for nvcc and cargo.
    $VcVer = if ($env:KIKITORI_CUDA_VCVARS_VER) { $env:KIKITORI_CUDA_VCVARS_VER } else { '14.44' }
    $VsWhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    $VsRoot = & $VsWhere -latest -products * -property installationPath
    if (-not $VsRoot) { throw 'Visual Studio with the C++ build tools was not found' }
    $VcVars = Join-Path $VsRoot 'VC\Auxiliary\Build\vcvarsall.bat'
    $VcArch = if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') { 'arm64_amd64' } else { 'x64' }
    $VcEnv = cmd /c "`"$VcVars`" $VcArch -vcvars_ver=$VcVer >nul && set"
    if ($LASTEXITCODE -ne 0) { throw "MSVC $VcVer for $VcArch is not installed (VS component: MSVC v143 build tools)" }
    $VcEnv | ForEach-Object { if ($_ -match '^([^=]+)=(.*)$') { Set-Item "env:$($matches[1])" $matches[2] } }
    $env:Path = (Join-Path $env:CUDA_PATH 'bin') + ';' + $env:Path
    $CudartStatic = Join-Path $env:CUDA_PATH 'lib\x64\cudart_static.lib'
    if (-not (Test-Path $CudartStatic)) { throw "cudart_static.lib not found: $CudartStatic" }
    $CudartDir = Join-Path ([System.IO.Path]::GetTempPath()) ("kikitori-cudart-" + [guid]::NewGuid())
    New-Item -ItemType Directory -Path $CudartDir | Out-Null
    Copy-Item $CudartStatic (Join-Path $CudartDir 'cudart.lib')
    $env:RUSTFLAGS = "$env:RUSTFLAGS -L native=$CudartDir"
}
$CargoArgs = @('build', '--release', '--target', $Target, '-p', 'kikitori')
if ($Features.Count) { $CargoArgs += @( '--features', ($Features -join ',') ) }
Invoke-Native { & cargo @CargoArgs }
if ($LASTEXITCODE -ne 0) { throw "cargo build for $Target failed" }
$TargetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { 'target' }
$Bin = Join-Path $TargetDir "$Target\release\kikitori.exe"
if (-not (Test-Path $Bin)) { throw "binary not found: $Bin" }

$Asset = "kikitori-$Version-$Target$Suffix.zip"
# .NET methods resolve relative paths against the process directory, not the PowerShell location.
$Dist = Join-Path (Get-Location).Path 'dist'
New-Item -ItemType Directory -Force -Path $Dist | Out-Null
# The smoke test points these at a temp folder; put them back afterwards.
$SmokeVars = 'USERPROFILE', 'HOME', 'APPDATA', 'LOCALAPPDATA', 'XDG_CACHE_HOME', 'HF_HOME', 'KIKITORI_MODELS'
$SavedVars = @{}
foreach ($n in $SmokeVars) { $SavedVars[$n] = [Environment]::GetEnvironmentVariable($n, 'Process') }
$Stage = Join-Path ([IO.Path]::GetTempPath()) ([Guid]::NewGuid().ToString('N'))
$Smoke = Join-Path ([IO.Path]::GetTempPath()) ([Guid]::NewGuid().ToString('N'))
try {
    New-Item -ItemType Directory -Force -Path $Stage, $Smoke | Out-Null

    $notice = New-Object IO.StringWriter
    $notice.WriteLine("kikitori $Version ($Asset)")
    $notice.WriteLine('kikitori is under the Apache License 2.0 (LICENSE in this archive).')
    $notice.WriteLine('')
    $notice.WriteLine('Statically linked third-party components:')
    $notice.WriteLine('  - sherpa-onnx: Apache-2.0, https://github.com/k2-fsa/sherpa-onnx')
    $notice.WriteLine('    (embeds ONNX Runtime, MIT, https://github.com/microsoft/onnxruntime)')
    if ($Cuda) {
        $notice.WriteLine('  - NVIDIA CUDA runtime (cudart_static): CUDA Toolkit EULA,')
        $notice.WriteLine('    https://docs.nvidia.com/cuda/eula/ (redistributable runtime library)')
    }
    $notice.WriteLine('  - every Rust dependency of this build, listed below as')
    $notice.WriteLine('    name version<TAB>licence, taken from the licence fields of')
    $notice.WriteLine('    the cargo metadata of this build.')
    $notice.WriteLine('')
    $lines = $Meta.packages | Where-Object { $_.name -ne 'kikitori' } | ForEach-Object {
        $lic = $_.license
        if (-not $lic) { $lic = 'license not specified' }
        "$($_.name) $($_.version)`t$lic"
    } | Sort-Object -Unique
    foreach ($l in $lines) { $notice.WriteLine($l) }
    [IO.File]::WriteAllText((Join-Path $Stage 'THIRD-PARTY-NOTICES'), $notice.ToString())

    Copy-Item $Bin (Join-Path $Stage 'kikitori.exe')
    Copy-Item 'LICENSE', 'README.md' $Stage
    Compress-Archive -Path (Join-Path $Stage '*') -DestinationPath (Join-Path $Dist $Asset)

    $hash = (Get-FileHash (Join-Path $Dist $Asset) -Algorithm SHA256).Hash.ToLowerInvariant()
    $sums = Join-Path $Dist 'SHA256SUMS'
    # Replace any previous entry for this asset; keep LF line ends like build-release.sh.
    $pattern = '\s\*?' + [regex]::Escape($Asset) + '$'
    $lines = @()
    if (Test-Path $sums) { $lines = @(Get-Content $sums | Where-Object { $_ -and ($_ -notmatch $pattern) }) }
    $lines += "$hash  $Asset"
    [IO.File]::WriteAllText($sums, (($lines -join "`n") + "`n"))
    Write-Host 'SHA256SUMS:'
    Get-Content $sums

    Expand-Archive (Join-Path $Dist $Asset) $Smoke
    $env:USERPROFILE = $Smoke
    $env:HOME = $Smoke
    $env:APPDATA = (Join-Path $Smoke 'AppData\Roaming')
    $env:LOCALAPPDATA = (Join-Path $Smoke 'AppData\Local')
    $env:XDG_CACHE_HOME = Join-Path $Smoke '.cache'
    $env:HF_HOME = Join-Path $env:XDG_CACHE_HOME 'huggingface'
    $env:KIKITORI_MODELS = Join-Path $env:XDG_CACHE_HOME 'kikitori\models'
    New-Item -ItemType Directory -Force -Path $env:APPDATA, $env:LOCALAPPDATA, $env:KIKITORI_MODELS | Out-Null
    & (Join-Path $Smoke 'kikitori.exe') --version
    if ($LASTEXITCODE -ne 0) { throw 'kikitori --version failed on the unpacked archive' }
    & (Join-Path $Smoke 'kikitori.exe') doctor --json | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'kikitori doctor --json failed on the unpacked archive' }

    Write-Host "build-release: $Dist\$Asset ready, sha256 $hash"
} finally {
    Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $Stage, $Smoke
    foreach ($n in $SmokeVars) { [Environment]::SetEnvironmentVariable($n, $SavedVars[$n], 'Process') }
}
