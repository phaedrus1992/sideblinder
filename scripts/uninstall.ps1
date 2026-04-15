#Requires -RunAsAdministrator
<#
.SYNOPSIS
    Uninstall the Sideblinder virtual joystick driver (for the Microsoft Sidewinder Force Feedback 2).

.DESCRIPTION
    Removes the device node via devcon, then deletes the driver package from
    the Windows driver store via pnputil.

.EXAMPLE
    .\uninstall.ps1
#>

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

# ── Remove the device node ──────────────────────────────────────────────────

Write-Host "Removing virtual device node..."

$devconPath = Get-Command devcon.exe -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source
if ($devconPath) {
    $removeResult = & devcon.exe remove "root\SideblinderFFB2"
    if ($LASTEXITCODE -ne 0) {
        Write-Warning "devcon remove returned $LASTEXITCODE. The device may already be gone."
    }
    Write-Host $removeResult
} else {
    Write-Warning "devcon.exe not found — skipping device node removal."
    Write-Warning "You can remove it manually in Device Manager."
}

# ── Find the published INF name ─────────────────────────────────────────────

Write-Host ""
Write-Host "Looking for driver package in the driver store..."

$enumResult = pnputil.exe /enum-drivers | Out-String
$lines      = $enumResult -split "`n"

$publishedName = $null
$foundOem      = $false

foreach ($line in $lines) {
    if ($line -match "Published Name\s*:\s*(oem\d+\.inf)") {
        $candidate = $Matches[1]
        $foundOem  = $true
    }
    if ($foundOem -and ($line -match "Original Name\s*:.*sideblinder" -or $line -match "Original Name\s*:.*sidewinder")) {
        $publishedName = $candidate
        break
    }
    if ($line -match "Published Name") { $foundOem = $false }
}

if (-not $publishedName) {
    Write-Warning "Could not find a driver package with 'sideblinder' (or 'sidewinder') in its original name."
    Write-Warning "Run 'pnputil /enum-drivers' to find it manually, then:"
    Write-Warning "  pnputil /delete-driver <oemNNN.inf> /uninstall /force"
    exit 0
}

# ── Delete from driver store ────────────────────────────────────────────────

Write-Host "Removing driver package $publishedName from the driver store..."
$deleteResult = pnputil.exe /delete-driver "$publishedName" /uninstall /force

if ($LASTEXITCODE -ne 0) {
    Write-Error "pnputil /delete-driver failed (exit $LASTEXITCODE).`n$deleteResult"
}

Write-Host $deleteResult
Write-Host ""
Write-Host "Driver uninstalled successfully."
