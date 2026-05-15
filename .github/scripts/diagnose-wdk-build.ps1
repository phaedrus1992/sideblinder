# Diagnostic script for WDK build issues on Windows
# This script collects information useful for debugging wdk-sys bindgen failures

Write-Host "=== WDK Build Environment Diagnostics ===" -ForegroundColor Cyan

Write-Host "`nSystem Information:"
systeminfo | Select-String "OS Version", "Total Physical Memory"

Write-Host "`nRust Toolchain:"
rustc --version
cargo --version

Write-Host "`nWDK Installation Check:"
if (Test-Path "C:\Program Files (x86)\Windows Kits") {
    Get-ChildItem "C:\Program Files (x86)\Windows Kits" | ForEach-Object { Write-Host "  Found: $_" }
} else {
    Write-Host "  WARNING: Windows Kits directory not found"
}

Write-Host "`nLLVM/Clang Check:"
clang --version 2>&1 | Select-Object -First 1

Write-Host "`nCargo Environment:"
$env:CARGO_BUILD_JOBS
Write-Host "  CARGO_BUILD_JOBS: $($env:CARGO_BUILD_JOBS ?? 'not set')"
Write-Host "  Available CPU cores: $([System.Environment]::ProcessorCount)"

Write-Host "`nKnown Issues:"
Write-Host "  - wdk-sys 0.5.1 has flaky bindgen thread failures on Windows CI"
Write-Host "  - Likely causes: resource exhaustion, LLVM version incompatibility"
Write-Host "  - Workaround: reduce parallel jobs with 'cargo build -j 2'"
Write-Host "  - Reference: https://github.com/microsoft/windows-drivers-rs/discussions/591"

Write-Host "`n=== End Diagnostics ===" -ForegroundColor Cyan
