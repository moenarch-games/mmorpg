//! The protocol ID is a 24-bit token: snapshots lead with its three bytes. The browser reads it
//! through `mmorpg-wasm` (#541), so this crate holds the only copy.
use mmorpg_protocol::PROTOCOL_ID;

#[test]
fn the_protocol_id_is_24_bits() {
    assert_eq!(PROTOCOL_ID >> 24, 0, "the protocol ID is 24 bits");
}
