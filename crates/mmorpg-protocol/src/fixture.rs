//! Wire byte fixtures (`fixtures/protocol/*.hex`) with the protocol ID as a placeholder (#1066).
//!
//! The ID is derived from the layout and these fixtures, so they cannot hold it. A fixture spells
//! each leading ID byte `PP` (three in a snapshot, the low byte in a command), and readers put the
//! current [`crate::PROTOCOL_ID`] back. A layout change elsewhere then leaves a fixture alone.
//! Test support for this crate and its clients.

use std::path::Path;

use crate::PROTOCOL_ID;

const PLACEHOLDER: &str = "PP";

fn hex_with_placeholder(bytes: &[u8], id_bytes: usize) -> String {
    let id = &PROTOCOL_ID.to_be_bytes()[4 - id_bytes..];
    assert!(
        bytes.starts_with(id),
        "a wire payload leads with the protocol ID"
    );
    let mut hex = PLACEHOLDER.repeat(id_bytes);
    for byte in &bytes[id_bytes..] {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

/// A command's fixture hex: its protocol byte as the placeholder.
///
/// # Panics
///
/// When `bytes` does not lead with the protocol byte.
#[must_use]
pub fn command_hex(bytes: &[u8]) -> String {
    hex_with_placeholder(bytes, 1)
}

/// A snapshot's fixture hex: its three protocol ID bytes as the placeholder.
///
/// # Panics
///
/// When `bytes` does not lead with the protocol ID.
#[must_use]
pub fn snapshot_hex(bytes: &[u8]) -> String {
    hex_with_placeholder(bytes, 3)
}

/// The bytes of fixture hex, with the current protocol ID in place of its placeholder.
///
/// # Panics
///
/// When `hex` is neither a command nor a snapshot fixture.
#[must_use]
pub fn bytes(hex: &str) -> Vec<u8> {
    let hex = hex.trim();
    let rest = hex.trim_start_matches('P');
    let id_bytes = (hex.len() - rest.len()) / PLACEHOLDER.len();
    assert!(
        matches!(id_bytes, 1 | 3) && rest.len().is_multiple_of(2),
        "fixture hex leads with a one- or three-byte protocol ID placeholder"
    );
    let mut bytes = PROTOCOL_ID.to_be_bytes()[4 - id_bytes..].to_vec();
    bytes.extend(
        (0..rest.len())
            .step_by(2)
            .map(|offset| u8::from_str_radix(&rest[offset..offset + 2], 16).expect("fixture hex")),
    );
    bytes
}

/// [`mmorpg_core::golden::golden_hex`] for a snapshot: the stored bytes, with the current ID.
///
/// # Panics
///
/// As [`mmorpg_core::golden::golden`], or when the fixture is not snapshot hex.
#[must_use]
pub fn golden_wire_hex(path: impl AsRef<Path>, actual: &[u8]) -> Vec<u8> {
    bytes(&mmorpg_core::golden::golden(
        path,
        &format!("{}\n", snapshot_hex(actual)),
    ))
}
