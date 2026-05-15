# WDK Build Troubleshooting

## Issue: "bindgen XXX.rs generator" thread failed to exit successfully

### Symptoms

When building the `sideblinder-driver` crate on Windows (especially in CI), you may see:

```
error: failed to run custom build command for `wdk-sys v0.5.1`
...
Error: "bindgen constants.rs generator" thread failed to exit successfully
```

Or more specifically:

```
cannot find directory: C:\Program Files (x86)\Windows Kits\10\Include\10.0.XXXXX.0\km/crt
```

The error occurs during the bindgen phase when wdk-sys tries to generate FFI bindings to Windows APIs.

### Root Cause

This is typically caused by an incomplete or missing Windows Driver Kit (WDK) installation. The bindgen code generation process needs access to WDK header files. Possible root causes:

1. **Missing WDK Installation**: WDK is not installed or not in the expected location
2. **Incomplete WDK Install**: Required header files or components are missing
3. **Wrong Windows SDK Version**: The SDK version doesn't match the expected path
4. **Resource Exhaustion** (secondary): Multiple bindgen threads exhausting memory during header processing

### Solutions

#### Step 1: Verify WDK Installation

**On Local Machine (Windows):**

```powershell
# Check if WDK is installed
Get-ChildItem "C:\Program Files (x86)\Windows Kits\10\Include"

# Look for the version directory (e.g., 10.0.26100.0)
Get-ChildItem "C:\Program Files (x86)\Windows Kits\10\Include\10.0.*\km"
```

**Required Directories:**
- `C:\Program Files (x86)\Windows Kits\10\Include\10.0.XXXXX.0\km` - Kernel mode headers
- `C:\Program Files (x86)\Windows Kits\10\Include\10.0.XXXXX.0\km\crt` - C runtime headers

If these are missing, reinstall the WDK:
https://learn.microsoft.com/en-us/windows-hardware/drivers/download-the-wdk

#### Step 2: Reduce Parallel Build Jobs

If you see resource exhaustion, reduce parallelism:

```bash
# Build with only 1 parallel job (fully sequential)
cargo build -j 1

# Or set globally for this session
set CARGO_BUILD_JOBS=1
```

#### Step 3: For CI/Workflows

In GitHub Actions, ensure the Windows environment has WDK installed. The `windows-latest` runner may need additional setup:

```yaml
- name: Install WDK (if needed)
  run: |
    # This depends on your CI setup; the runner may already have WDK
    # Check the runner setup: https://github.com/actions/runner-images

- name: Build
  run: cargo build -j 1
  env:
    CARGO_BUILD_JOBS: 1
```

#### Long-term Solutions

1. **Verify WDK availability in CI runner**
   - GitHub `windows-latest` runner should have WDK pre-installed
   - If not, you may need a custom runner or different image

2. **Update wdk-sys** when a new version fixes header detection
   - Check: https://github.com/microsoft/windows-drivers-rs/releases
   - Current: 0.5.1 (latest stable)

### Debugging

Run the diagnostic script to gather system information:

```powershell
.\.github\scripts\diagnose-wdk-build.ps1
```

This collects:
- System memory and CPU information
- Rust/cargo versions
- WDK installation status
- LLVM/Clang version
- Known issues and workarounds

### Reporting Issues

If you encounter this issue persistently:

1. Run the diagnostic script and save the output
2. Check: https://github.com/microsoft/windows-drivers-rs/issues
3. Report with:
   - Rust version (`rustc --version`)
   - LLVM version (`clang --version`)
   - System specs (RAM, CPU cores)
   - Full build output with `RUST_LOG=debug`

### References

- [wdk-sys GitHub Discussion #591](https://github.com/microsoft/windows-drivers-rs/discussions/591)
- [windows-drivers-rs Issues](https://github.com/microsoft/windows-drivers-rs/issues)
- [Bindgen Documentation](https://rust-lang.github.io/chalk/book/binding/index.html)
