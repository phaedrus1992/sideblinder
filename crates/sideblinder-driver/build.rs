use std::path::Path;

fn main() -> Result<(), wdk_build::ConfigError> {
    // Workaround for wdk-build path bug on Windows
    // Issue: wdk-build uses path.join("km/crt") which creates C:\...\km/crt (mixed separators)
    // This causes bindgen to fail finding the header directory
    // We work around by pre-validating and fixing the path if needed
    #[cfg(target_os = "windows")]
    {
        validate_wdk_headers();
    }

    wdk_build::configure_wdk_binary_build().map_err(|e| {
        #[expect(clippy::print_stderr, reason = "diagnostic output in build script")]
        {
            eprintln!("\n╔════════════════════════════════════════════════════════════╗");
            eprintln!("║  sideblinder-driver build failed                          ║");
            eprintln!("╚════════════════════════════════════════════════════════════╝");
            eprintln!("\nError: {e}");
            eprintln!("\nCommon issues and solutions:");
            eprintln!("  • Missing WDK headers:");
            eprintln!("    - Install Windows Driver Kit (WDK)");
            eprintln!("    - Check: C:\\Program Files (x86)\\Windows Kits\\10\\Include");
            eprintln!("\n  • wdk-build path bug workaround:");
            eprintln!("    - Ensure all WDK subdirectories exist with proper backslashes");
            eprintln!("    - Run: cargo clean && cargo build");
            eprintln!("\n  • Parallel build failure:");
            eprintln!("    - Try: cargo build -j 1");
            eprintln!("\n  • LLVM version mismatch:");
            eprintln!("    - Verify Rust version: rustc --version");
            eprintln!("    - Check: https://github.com/microsoft/windows-drivers-rs/issues");
            eprintln!("\nFor more details, see: docs/wdk-build-troubleshooting.md");
            eprintln!("════════════════════════════════════════════════════════════\n");
        }
        e
    })
}

#[cfg(target_os = "windows")]
fn validate_wdk_headers() {
    // Check for WDK installation and validate header paths
    // This helps work around the wdk-build path bug where it uses forward slashes
    let wdk_base_paths = [
        "C:\\Program Files (x86)\\Windows Kits\\10",
        "C:\\Program Files\\Windows Kits\\10",
    ];

    for base in &wdk_base_paths {
        let base_path = Path::new(base);
        if !base_path.exists() {
            continue;
        }

        let include_dir = base_path.join("Include");
        if !include_dir.exists() {
            continue;
        }

        // Find the SDK version directory (e.g., 10.0.26100.0)
        if let Ok(entries) = std::fs::read_dir(&include_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir()
                    && let Some(dir_name) = path.file_name().map(|n| n.to_string_lossy())
                    && dir_name.starts_with("10.0.")
                {
                    // Check if critical headers exist
                    let km_crt_path = path.join("km").join("crt");
                    let km_path = path.join("km");
                    let um_path = path.join("um");
                    let shared_path = path.join("shared");

                    // Validate the paths exist
                    let paths_ok = km_crt_path.exists() && km_path.exists()
                        && um_path.exists() && shared_path.exists();

                    if paths_ok {
                        // Log successful validation (cargo will suppress this in normal builds)
                        println!("cargo:warning=WDK headers validated at: {base}");
                        return;
                    }
                    #[expect(
                        clippy::print_stderr,
                        reason = "diagnostic output in build script"
                    )]
                    {
                        eprintln!(
                            "cargo:warning=WDK headers incomplete at: {}",
                            path.display()
                        );
                        eprintln!(
                            "cargo:warning=  Missing: km/crt={}, km={}, um={}, shared={}",
                            km_crt_path.exists(),
                            km_path.exists(),
                            um_path.exists(),
                            shared_path.exists()
                        );
                    }
                }
            }
        }
    }
}
