#Requires -RunAsAdministrator
<#
.SYNOPSIS
    Install the Sidewinder Force Feedback 2 virtual joystick driver.

.DESCRIPTION
    Adds the driver package to the Windows driver store via pnputil, then
    triggers PnP to install it on the root\SidewinderFFB2 enumerator node.

.PARAMETER InfPath
    Path to the built sidewinder.inf (or compiled .inf from the INX).
    Defaults to .\sidewinder-driver\sidewinder.inf relative to this script.

.EXAMPLE
    .\install.ps1
    .\install.ps1 -InfPath "C:\build\sidewinder.inf"
#>
param(
    [string]$InfPath = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

# ── Resolve INF path ────────────────────────────────────────────────────────

if (-not $InfPath) {
    $scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
    $InfPath   = Join-Path $scriptDir "..\crates\sidewinder-driver\sidewinder.inf"
}

$InfPath = Resolve-Path $InfPath -ErrorAction Stop | Select-Object -ExpandProperty Path

Write-Host "Using INF: $InfPath"

# ── Add driver package to driver store ─────────────────────────────────────

Write-Host ""
Write-Host "Adding driver package to the Windows Driver Store..."
$addResult = pnputil.exe /add-driver "$InfPath" /install

if ($LASTEXITCODE -ne 0) {
    Write-Error "pnputil /add-driver failed (exit $LASTEXITCODE). Output:`n$addResult"
}

Write-Host $addResult

# ── Create the root enumerator device node ─────────────────────────────────
#
# The UMDF2 driver sits under MsHidUmdf.sys.  devcon is used to create the
# software device node that triggers PnP enumeration.  devcon.exe must be on
# PATH (available from the WDK or Windows Driver Kit Extras).

Write-Host ""
Write-Host "Creating virtual device node..."

$devconPath = Get-Command devcon.exe -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source
if (-not $devconPath) {
    Write-Warning "devcon.exe not found on PATH.  Install the WDK and add devcon to PATH."
    Write-Warning "You can manually install with:"
    Write-Warning "  pnputil /add-driver '$InfPath' /install"
    exit 0
}

$installResult = & devcon.exe install "$InfPath" "root\SidewinderFFB2"
if ($LASTEXITCODE -ne 0) {
    Write-Error "devcon install failed (exit $LASTEXITCODE). Output:`n$installResult"
}

Write-Host $installResult
Write-Host ""
Write-Host "Driver installed successfully."
Write-Host "The virtual Sidewinder FF2 joystick should now appear in Device Manager."
