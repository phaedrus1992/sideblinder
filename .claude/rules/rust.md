---
paths:
  - "**/*.rs"
  - "**/Cargo.toml"
---

# Rust Conventions

**Adding dependencies:** Run `/sync-crate-skills` after adding a crate to `Cargo.toml`.

**Lints:** Workspace-level clippy pedantic. Strict deny on `unwrap_used`, `panic`, `todo`,
`dbg_macro`. Use `#[expect(..., reason = "...")]` (not `#[allow(...)]`) —
`allow_attributes` is denied.

**unsafe:** Denied at workspace level (`unsafe_code = "deny"`). Use `deny` not `forbid` so that
`#[expect(unsafe_code, reason = "...")]` can override it in the two platform modules that wrap
Win32 HID APIs. Keep unsafe blocks minimal and localized to `sideblinder-hid`. Any new unsafe block
requires a `// SAFETY:` comment explaining the invariant.

**Errors:**
- `sideblinder-hid` (library): `thiserror` enums with typed variants. Include `# Errors` doc
  section on all public functions returning `Result`.
- `sideblinder-app`, `sideblinder-diag` (binaries): `Box<dyn std::error::Error>` for
  application-level glue and cross-layer error propagation. (`anyhow` is the goal for new
  code, but has not been added as a dependency yet; update this doc when the migration lands.)

**Async boundaries:**
- `sideblinder-hid` — synchronous only; no async. HID I/O is blocking by design.
- `sideblinder-app`, `sideblinder-diag` — tokio runtime; async for event loops and IPC.

Keep async confined to the binary crates; `sideblinder-hid` must stay pure synchronous.

## Workspace Lint Policy

Add to `[workspace.lints.clippy]` in the root `Cargo.toml`:

```toml
[workspace.lints.clippy]
pedantic = { level = "warn", priority = -1 }
unwrap_used = "deny"
expect_used = "warn"
panic = "deny"
panic_in_result_fn = "deny"
unimplemented = "deny"
allow_attributes = "deny"
dbg_macro = "deny"
todo = "deny"
await_holding_lock = "deny"
exit = "deny"
mem_forget = "deny"
module_name_repetitions = "allow"
similar_names = "allow"

[workspace.lints.rust]
unsafe_code = "deny"
```

Use `#[expect(lint, reason = "...")]` over `#[allow(lint)]` — warns when the suppression becomes
stale.

Hardcoded magic values (timeouts, retry counts, report IDs, axis ranges) require a comment
explaining *why that value*.

## Newtypes

Use newtypes to make invalid states unrepresentable. The principle is **"parse, don't validate"**
— validate in the constructor so that a value's existence guarantees its validity. Downstream code
never re-validates.

**When to use:** domain values with constraints (axis ranges, button indices, report IDs),
semantic disambiguation (a function taking `(u8, u8)` can silently accept args in the wrong order;
`(ReportId, AxisValue)` cannot), units where mixing would be silent bugs.

**Don't** wrap types that have no constraints and no confusion risk — a newtype with neither is
just noise.

**Structure — keep inner field private:**
```rust
pub struct AxisValue(i16);

impl AxisValue {
    pub fn new(raw: i16) -> Result<Self, AxisError> {
        if raw >= AXIS_MIN && raw <= AXIS_MAX {
            Ok(Self(raw))
        } else {
            Err(AxisError::OutOfRange(raw))
        }
    }
}
```

**Derive traits generously** — downstream code can't add them due to the orphan rule:
- Always: `Debug, Clone, PartialEq`
- When inner supports it: `Eq, PartialOrd, Ord, Hash, Copy`
- Skip `Default` unless zero/empty is semantically meaningful

**TryFrom delegates to new() — never duplicate validation:**
```rust
impl TryFrom<i16> for AxisValue {
    type Error = AxisError;
    fn try_from(raw: i16) -> Result<Self, Self::Error> { Self::new(raw) }
}
```

**Access — prefer `AsRef` over `Deref` for constrained types.** Use explicit accessors:
```rust
impl AxisValue {
    pub fn into_inner(self) -> i16 { self.0 }
    pub fn as_inner(&self) -> i16 { self.0 }  // Copy types: return by value
}
```

**Don't implement `Borrow<T>`** unless the newtype hashes and compares identically to the inner
type. Violating the `Borrow` contract silently breaks `HashMap`/`HashSet` lookups.

## General Conventions

- All public types must derive or implement `Debug`
- No glob re-exports (`pub use foo::*`) — re-export items individually
- Avoid vague names (`Manager`, `Handler`, `Processor`) — prefer domain-specific names
  (`HidTransport`, `FfbEffect`, `InputReport`)
- Use enums for state machines and effect types, not boolean flags or magic integers
- In library crates, use `tracing` (`error!`/`warn!`/`info!`/`debug!`) for all output — never `println!` or `eprintln!`
- In binary crates with CLI output (e.g. `sideblinder-diag`), `println!` is fine for user-facing output; use `tracing` for diagnostic/debug logging
