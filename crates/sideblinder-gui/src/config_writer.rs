//! In-place TOML config patching via `toml_edit`.
//!
//! Writes individual config fields back to the config file while preserving
//! all user comments and unrelated keys.  The file is read, the target key is
//! updated in the parsed document, and the result is written back to disk.
//!
//! The `sideblinder-app` `notify` watcher picks up any write within ~100 ms
//! and hot-reloads the config — no further coordination is needed.

use std::path::Path;
use thiserror::Error;
use toml_edit::{DocumentMut, Item, Table, value};

/// Errors from config write-back operations.
#[derive(Debug, Error)]
pub enum ConfigWriteError {
    /// The config file could not be read.
    #[error("could not read config: {0}")]
    Read(#[from] std::io::Error),
    /// The TOML document could not be parsed.
    #[error("could not parse config TOML: {0}")]
    Parse(toml_edit::TomlError),
    /// The config file could not be written.
    #[error("could not write config: {0}")]
    Write(String),
}

impl From<toml_edit::TomlError> for ConfigWriteError {
    fn from(e: toml_edit::TomlError) -> Self {
        Self::Parse(e)
    }
}

/// Write a single `f32` value to a dot-separated key path in the config file.
///
/// The key path uses `.` as a separator, e.g. `"axis_x.dead_zone"`.
/// If any table in the path is absent it is created.  All other keys and
/// comments in the file are preserved exactly.
///
/// # Errors
///
/// Returns [`ConfigWriteError`] on I/O or parse failure.
pub fn patch_f32(path: &Path, key_path: &str, val: f32) -> Result<(), ConfigWriteError> {
    patch_item(path, key_path, value(f64::from(val)))
}

/// Write a single `u8` value to a dot-separated key path in the config file.
///
/// # Errors
///
/// Returns [`ConfigWriteError`] on I/O or parse failure.
pub fn patch_u8(path: &Path, key_path: &str, val: u8) -> Result<(), ConfigWriteError> {
    patch_item(path, key_path, value(i64::from(val)))
}

/// Write a single `bool` value to a dot-separated key path in the config file.
///
/// # Errors
///
/// Returns [`ConfigWriteError`] on I/O or parse failure.
pub fn patch_bool(path: &Path, key_path: &str, val: bool) -> Result<(), ConfigWriteError> {
    patch_item(path, key_path, value(val))
}

/// Write a single string value to a dot-separated key path in the config file.
///
/// # Errors
///
/// Returns [`ConfigWriteError`] on I/O or parse failure.
pub fn patch_str(path: &Path, key_path: &str, val: &str) -> Result<(), ConfigWriteError> {
    patch_item(path, key_path, value(val))
}

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Apply a [`toml_edit::Item`] value at the given dot-separated key path.
fn patch_item(
    path: &Path,
    key_path: &str,
    item: Item,
) -> Result<(), ConfigWriteError> {
    let raw = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(ConfigWriteError::Read(e)),
    };
    let mut doc: DocumentMut = if raw.is_empty() {
        DocumentMut::new()
    } else {
        raw.parse::<DocumentMut>()?
    };

    let parts: Vec<&str> = key_path.split('.').collect();

    match parts.as_slice() {
        [] => {}
        [leaf] => {
            doc[*leaf] = item;
        }
        [table_keys @ .., leaf] => {
            let mut current = doc.as_table_mut();
            for &key in table_keys {
                // Ensure key is a table — overwrite if it exists as a scalar.
                if !current.contains_key(key) || !current[key].is_table() {
                    current[key] = toml_edit::Item::Table(Table::new());
                }
                #[expect(clippy::expect_used, reason = "key was just ensured to be a Table in the block above")]
                let table = current[key]
                    .as_table_mut()
                    .expect("key is a table — guaranteed by block above");
                current = table;
            }
            current[*leaf] = item;
        }
    }

    std::fs::write(path, doc.to_string())
        .map_err(|e| ConfigWriteError::Write(e.to_string()))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test code — panics are the failure mode")]
mod tests {
    use super::*;
    use std::io::Write as _;
    use tempfile::NamedTempFile;

    fn write_toml(content: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().expect("temp file");
        f.write_all(content.as_bytes()).expect("write");
        f
    }

    #[test]
    fn patch_f32_updates_existing_key() {
        let f = write_toml("[axis_x]\ndead_zone = 0.0\n");
        patch_f32(f.path(), "axis_x.dead_zone", 0.05).expect("patch");
        let result = std::fs::read_to_string(f.path()).expect("read");
        assert!(result.contains("dead_zone = 0.05"), "got: {result}");
    }

    #[test]
    fn patch_preserves_comments() {
        let toml = "# top-level comment\n[axis_x]\n# axis comment\ndead_zone = 0.0\n";
        let f = write_toml(toml);
        patch_f32(f.path(), "axis_x.dead_zone", 0.1).expect("patch");
        let result = std::fs::read_to_string(f.path()).expect("read");
        assert!(result.contains("# top-level comment"), "top comment gone");
        assert!(result.contains("# axis comment"), "axis comment gone");
    }

    #[test]
    fn patch_creates_missing_table() {
        let f = write_toml("ffb_gain = 255\n");
        patch_f32(f.path(), "axis_x.dead_zone", 0.1).expect("patch");
        let result = std::fs::read_to_string(f.path()).expect("read");
        assert!(result.contains("[axis_x]"), "table not created");
        assert!(result.contains("dead_zone"), "key not created");
    }

    #[test]
    fn patch_bool_false() {
        let f = write_toml("[axis_x]\ninvert = true\n");
        patch_bool(f.path(), "axis_x.invert", false).expect("patch");
        let result = std::fs::read_to_string(f.path()).expect("read");
        assert!(result.contains("invert = false"), "got: {result}");
    }

    #[test]
    fn patch_u8_sets_gain() {
        let f = write_toml("ffb_gain = 255\n");
        patch_u8(f.path(), "ffb_gain", 128).expect("patch");
        let result = std::fs::read_to_string(f.path()).expect("read");
        assert!(result.contains("ffb_gain = 128"), "got: {result}");
    }

    #[test]
    fn patch_str_updates_curve() {
        let f = write_toml("[axis_x]\ncurve = \"linear\"\n");
        patch_str(f.path(), "axis_x.curve", "s-curve").expect("patch");
        let result = std::fs::read_to_string(f.path()).expect("read");
        assert!(result.contains("\"s-curve\""), "got: {result}");
    }

    #[test]
    fn patch_empty_file_creates_document() {
        let f = write_toml("");
        patch_u8(f.path(), "ffb_gain", 200).expect("patch on empty file");
        let result = std::fs::read_to_string(f.path()).expect("read");
        assert!(result.contains("ffb_gain = 200"), "got: {result}");
    }
}
