# Fetch the parts of the CUDA toolkit that the Windows x64 CUDA build needs
# (nvcc, the CCCL headers, the static CUDA runtime) from NVIDIA's redistributable
# archives, check each against the SHA-256 in NVIDIA's manifest, and unpack them
# into one folder. Prints the folder; set CUDA_PATH to it before building.
#
# Usage: scripts/fetch-cuda-windows.ps1 [-Dest <folder>] [-Version 12.6.3]
#
# The archives are x64 programs; on an arm64 PC they run under emulation.
param(
    [string]$Dest = (Join-Path ([IO.Path]::GetTempPath()) 'kikitori-cuda'),
    [string]$Version = '12.6.3'
)
$ErrorActionPreference = 'Stop'
$Base = 'https://developer.download.nvidia.com/compute/cuda/redist/'
$Manifest = Invoke-RestMethod -UseBasicParsing "${Base}redistrib_$Version.json"
if (Test-Path $Dest) { Remove-Item -Recurse -Force $Dest }
New-Item -ItemType Directory -Path $Dest | Out-Null
$Work = Join-Path ([IO.Path]::GetTempPath()) ("kikitori-cuda-dl-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $Work | Out-Null
try {
    foreach ($name in @('cuda_cccl', 'cuda_nvcc', 'cuda_cudart')) {
        $entry = $Manifest.$name.'windows-x86_64'
        if (-not $entry) { throw "the CUDA $Version manifest has no windows-x86_64 archive for $name" }
        $zip = Join-Path $Work (Split-Path $entry.relative_path -Leaf)
        Invoke-WebRequest -UseBasicParsing ($Base + $entry.relative_path) -OutFile $zip
        $hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($hash -ne $entry.sha256) { throw "SHA-256 mismatch for $name" }
        $unpacked = Join-Path $Work $name
        Expand-Archive $zip $unpacked
        # Each archive holds one top folder (bin, include, lib, ...) under its own name.
        $inner = (Get-ChildItem $unpacked -Directory | Select-Object -First 1).FullName
        Copy-Item -Recurse -Force (Join-Path $inner '*') $Dest
    }
} finally {
    Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $Work
}
if (-not (Test-Path (Join-Path $Dest 'bin\nvcc.exe'))) { throw 'nvcc.exe is missing after unpacking' }
if (-not (Test-Path (Join-Path $Dest 'lib\x64\cudart_static.lib'))) { throw 'cudart_static.lib is missing after unpacking' }
Write-Output $Dest
