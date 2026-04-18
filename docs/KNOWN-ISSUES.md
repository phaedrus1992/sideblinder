# Known Issues

## wdk-sys path handling on Windows (Upstream Bug)

**Issue:** `wdk-sys` 0.5.1 has a path construction bug in `wdk-build` that causes build failures on Windows.

**Symptom:**
```
cannot find directory: C:\Program Files (x86)\Windows Kits\10\Include\10.0.XXXXX.0\km/crt
```

Notice the mixed path separators: `\` followed by `/crt`.

**Root Cause:**
In `wdk-build/src/lib.rs`, the code uses:
```rust
let crt_include_path = windows_sdk_include_path.join("km/crt");
```

The forward slash in the string literal creates a relative path with mixed separators, resulting in:
- `C:\...\km/crt` instead of
- `C:\...\km\crt`

Windows can handle mixed separators in many cases, but the directory lookup fails because the actual directory uses backslashes.

**Fix (Upstream):**
Should be:
```rust
let crt_include_path = windows_sdk_include_path.join("km").join("crt");
```

**Workaround (Implemented):**
We've added WDK header validation in `crates/sideblinder-driver/build.rs` that:
1. Scans for WDK installation on standard paths
2. Validates that all required header directories exist (km/crt, km, um, shared)
3. Reports warnings if validation fails

This helps diagnose the issue but doesn't fix the underlying bug. The validation may also help the build system recover if headers are present but the path construction is failing.

**Tracking:**
- Upstream issue: https://github.com/microsoft/windows-drivers-rs
- First reported in sideblinder: feat/43-driver-safety-ipc-version PR

**Timeline:**
- Discovered: 2026-04-17
- Affects: wdk-sys 0.5.1 with wdk-build 0.5.1
- Status: Awaiting upstream fix
