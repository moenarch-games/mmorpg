//! Golden test fixtures that regenerate themselves.
//!
//! `MMORPG_UPDATE_FIXTURES=1 cargo test --workspace --locked` rewrites every golden file a
//! test checks (protocol bytes, traces, pinned content fingerprints, scenario outputs) from
//! the current code; without it, tests compare against the checked-in files. Review the
//! resulting diff like any other change. Test support only: no gameplay code reads it.

use std::path::Path;

/// The environment variable that turns golden comparisons into rewrites.
pub const UPDATE_FIXTURES: &str = "MMORPG_UPDATE_FIXTURES";

/// Whether this test run rewrites golden files.
#[must_use]
pub fn updating() -> bool {
    std::env::var_os(UPDATE_FIXTURES).is_some()
}

/// The checked-in contents of the golden file at `path`, or — when [`updating`] — `actual`,
/// written there first. A test then asserts `actual` equals the result.
///
/// # Panics
///
/// When the file can be neither read nor (while updating) written.
#[must_use]
pub fn golden(path: impl AsRef<Path>, actual: &str) -> String {
    let path = path.as_ref();
    if updating() {
        std::fs::write(path, actual)
            .unwrap_or_else(|error| panic!("cannot write {}: {error}", path.display()));
        return actual.to_owned();
    }
    std::fs::read_to_string(path).unwrap_or_else(|error| {
        panic!(
            "cannot read {}: {error}; rerun with {UPDATE_FIXTURES}=1 to create it",
            path.display()
        )
    })
}

/// [`golden`] for a byte fixture stored as one line of lowercase hex.
///
/// # Panics
///
/// As [`golden`], or when the stored file is not hex.
#[must_use]
pub fn golden_hex(path: impl AsRef<Path>, actual: &[u8]) -> Vec<u8> {
    let hex: String = actual.iter().map(|byte| format!("{byte:02x}")).collect();
    let stored = golden(path, &format!("{hex}\n"));
    let stored = stored.trim();
    (0..stored.len())
        .step_by(2)
        .map(|offset| u8::from_str_radix(&stored[offset..offset + 2], 16).expect("hex fixture"))
        .collect()
}

/// The workspace root, for fixture paths relative to it.
#[must_use]
pub fn workspace_root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}
