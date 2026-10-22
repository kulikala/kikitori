# kikitori を入れ、文字起こしと補正の環境を作る（Windows。実機では未検証）。
#   irm https://github.com/kulikala/kikitori/releases/latest/download/install.ps1 | iex
# 環境変数は install.sh と同じ（KIKITORI_NO_REFINE・KIKITORI_NO_HF・KIKITORI_YES・KIKITORI_UPGRADE・KIKITORI_WHEEL）。
$ErrorActionPreference = "Stop"

$Wheel = if ($env:KIKITORI_WHEEL) { $env:KIKITORI_WHEEL } else { "__WHEEL_URL__" }
if ($Wheel -like "__WHEEL*") { throw "wheel の場所が決まっていません。Release の install.ps1 を使うか、KIKITORI_WHEEL を指定してください" }
if ($Wheel -notmatch '^(https?|file)://') { $Wheel = "file:///" + ((Resolve-Path $Wheel).Path -replace '\\', '/') }

$Extras = @("directml")
$Refine = $env:KIKITORI_NO_REFINE -ne "1"
if ($Refine) { $Extras += "refine" }

# uv（Python と道具の管理）。入れたばかりの uv は今のシェルの PATH に無いので、絶対パスで呼ぶ
$Uv = (Get-Command uv -ErrorAction SilentlyContinue).Source
if (-not $Uv) {
    Write-Host "uv を入れます（https://astral.sh/uv）"
    powershell -ExecutionPolicy ByPass -c "irm https://astral.sh/uv/install.ps1 | iex"
    $Uv = Join-Path $env:USERPROFILE ".local\bin\uv.exe"
    if (-not (Test-Path $Uv)) { throw "uv を入れられませんでした" }
}

$Spec = "kikitori[$($Extras -join ',')] @ $Wheel"
$UvArgs = @("tool", "install", "--python", "3.12", "--torch-backend", "auto")
if ($env:KIKITORI_UPGRADE -eq "1") { $UvArgs += "--force" }
Write-Host "kikitori を入れます: $Spec"
& $Uv @UvArgs $Spec
if ($LASTEXITCODE -ne 0) { throw "kikitori を入れられませんでした" }

$Bin = (& $Uv tool dir --bin).Trim()
& (Join-Path $Bin "kikitori.exe") --setup

$Status = 0
if ($Refine) {
    $SetupArgs = @("--setup")
    if ($env:KIKITORI_YES -eq "1") { $SetupArgs += "--yes" }
    if ($env:KIKITORI_NO_HF -eq "1") { $SetupArgs += "--no-hf" }
    & (Join-Path $Bin "kikitori-refine.exe") @SetupArgs
    $Status = $LASTEXITCODE
}

Write-Host ""
Write-Host "== kikitori の導入の結果 =="
if ($Status -eq 0) { Write-Host "済み" } else { Write-Host "未完了: 上の案内のあと、kikitori-refine --setup を打ち直してください" }
if (-not (Get-Command ffmpeg -ErrorAction SilentlyContinue)) {
    Write-Host "未完了: ffmpeg がありません。入れるまで文字起こしできません: winget install ffmpeg"
}
if (($env:PATH -split ';') -notcontains $Bin) {
    Write-Host "PATH に $Bin がありません。$Uv tool update-shell を打ち、端末を開き直してください"
}
exit $Status
