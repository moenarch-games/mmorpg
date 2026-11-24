//! The protocol ID is a 24-bit hash of the wire layout (#1066): snapshots lead with its three
//! bytes and commands with its low byte. The browser reads it through `mmorpg-wasm` (#541), so
//! this crate holds the only copy, and a peer of any other ID is refused.
use mmorpg_core::ZoneCommand;
use mmorpg_protocol::{
    PROTOCOL_ID, SnapshotDecoder, decode_command_envelope, encode_command, fixture,
};

#[test]
fn the_protocol_id_is_24_bits() {
    assert_eq!(PROTOCOL_ID >> 24, 0, "the protocol ID is 24 bits");
}

/// The native client decodes projections with [`SnapshotDecoder`] and zone hosts decode commands
/// with [`decode_command_envelope`]; neither accepts bytes of another ID.
#[test]
fn a_peer_of_another_protocol_id_is_refused() {
    let snapshot = fixture::bytes(
        &std::fs::read_to_string(
            mmorpg_core::golden::workspace_root().join("fixtures/protocol/player-snapshot.hex"),
        )
        .unwrap(),
    );
    assert!(SnapshotDecoder::new().decode(&snapshot).is_ok());
    for at in 0..3 {
        let mut other = snapshot.clone();
        other[at] ^= 1;
        assert!(
            SnapshotDecoder::new().decode(&other).is_err(),
            "ID byte {at}"
        );
    }
    let mut command = encode_command(ZoneCommand::Jump);
    assert!(decode_command_envelope(&command).is_ok());
    command[0] ^= 1;
    assert!(decode_command_envelope(&command).is_err());
}
