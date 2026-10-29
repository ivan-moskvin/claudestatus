# Installs clistatus from the latest release. After that the utility looks
# after itself: clistatus update to update, clistatus uninstall to remove.
#
#   irm https://raw.githubusercontent.com/ivan-moskvin/clistatus/main/install.ps1 | iex

$ErrorActionPreference = "Stop"

$repo = "ivan-moskvin/clistatus"
$binDir = if ($env:CLISTATUS_BIN_DIR) { $env:CLISTATUS_BIN_DIR } else { Join-Path $env:LOCALAPPDATA "clistatus" }

# The messages follow the interface language of the system, the same way the
# binary picks it: CLISTATUS_LANG wins, then the culture Windows reports.
# Every language is one branch here — nothing below chooses wording again.
$lang = if ($env:CLISTATUS_LANG) { $env:CLISTATUS_LANG } else { [System.Globalization.CultureInfo]::CurrentUICulture.TwoLetterISOLanguageName }
$m = if ($lang -like "ru*") {
    @{
        UnknownArch  = "Неизвестная архитектура: {0} — соберите из исходников."
        Downloading  = "==> Скачиваю {0}"
        NoChecksum   = "В релизе нет контрольной суммы для {0}."
        BadChecksum  = "Контрольная сумма не сошлась — загрузка побилась."
        Installed    = "==> Поставлен в {0}"
        PathAdded    = "==> Каталог добавлен в PATH — заработает в новом терминале"
    }
} else {
    @{
        UnknownArch  = "Unknown architecture: {0} — build it from source."
        Downloading  = "==> Downloading {0}"
        NoChecksum   = "The release has no checksum for {0}."
        BadChecksum  = "The checksum does not match — the download is broken."
        Installed    = "==> Installed to {0}"
        PathAdded    = "==> The directory was added to PATH — it works in a new terminal"
    }
}

$arch = switch ($env:PROCESSOR_ARCHITECTURE) {
    "AMD64" { "amd64" }
    "ARM64" { "arm64" }
    default { throw ($m.UnknownArch -f $env:PROCESSOR_ARCHITECTURE) }
}

$asset = "clistatus_windows_$arch.exe"
$base = "https://github.com/$repo/releases/latest/download"
$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ([System.IO.Path]::GetRandomFileName())
New-Item -ItemType Directory -Path $tmp | Out-Null

try {
    Write-Host ($m.Downloading -f $asset)
    Invoke-WebRequest -Uri "$base/$asset" -OutFile "$tmp\clistatus.exe"
    Invoke-WebRequest -Uri "$base/checksums.txt" -OutFile "$tmp\checksums.txt"

    $want = (Get-Content "$tmp\checksums.txt" |
        Where-Object { $_ -match "\s\*?$([regex]::Escape($asset))$" } |
        ForEach-Object { ($_ -split '\s+')[0] })
    if (-not $want) { throw ($m.NoChecksum -f $asset) }

    $got = (Get-FileHash "$tmp\clistatus.exe" -Algorithm SHA256).Hash
    if ($got -ne $want.ToUpper()) { throw $m.BadChecksum }

    New-Item -ItemType Directory -Force -Path $binDir | Out-Null
    Move-Item -Force "$tmp\clistatus.exe" (Join-Path $binDir "clistatus.exe")
    Write-Host ($m.Installed -f (Join-Path $binDir "clistatus.exe"))

    # The directory goes into the user PATH — otherwise the clistatus command
    # will not be found.
    $current = [Environment]::GetEnvironmentVariable("Path", "User")
    $entries = @()
    if ($current) { $entries = $current -split ";" | Where-Object { $_ } }
    if ($entries -notcontains $binDir) {
        [Environment]::SetEnvironmentVariable("Path", (($entries + $binDir) -join ";"), "User")
        Write-Host $m.PathAdded
    }

    # The registry says nothing to a process that is already running: without
    # this the install below would inherit the old PATH and warn about a
    # directory it has just been put into.
    if (($env:Path -split ";") -notcontains $binDir) {
        $env:Path = "$env:Path;$binDir"
    }

    & (Join-Path $binDir "clistatus.exe") install
}
finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
