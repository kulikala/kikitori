# kikitori installer for Windows (entry point). Run in PowerShell:
#   irm https://github.com/kulikala/kikitori/releases/latest/download/install.ps1 | iex
# This file is ASCII only on purpose: Windows PowerShell 5.1 does not decode `irm` output as UTF-8.
# It fetches install-windows.ps1 (the installer, written in Japanese) as bytes, decodes it as UTF-8 and runs it.
# Everything runs inside a script block, so nothing is left in the caller's session.
# KIKITORI_INSTALLER: a local install-windows.ps1 to run instead (for testing).
& {
    $ErrorActionPreference = "Stop"
    if ($env:KIKITORI_INSTALLER) {
        $bytes = [IO.File]::ReadAllBytes((Resolve-Path $env:KIKITORI_INSTALLER).Path)
    } else {
        $url = "__INSTALLER_URL__"
        if ($url -like "__INSTALLER*") { throw "Use install.ps1 from a GitHub Release, or set KIKITORI_INSTALLER." }
        [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
        $bytes = (New-Object Net.WebClient).DownloadData($url)
    }
    & ([scriptblock]::Create([Text.Encoding]::UTF8.GetString($bytes).TrimStart([char]0xFEFF)))
}
