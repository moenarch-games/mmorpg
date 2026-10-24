//! The browser decodes and encodes the same wire format, so its copy of the protocol ID must
//! be this crate's (`scripts/new-protocol-id.sh` rewrites both).
use mmorpg_protocol::PROTOCOL_ID;

#[test]
fn the_browser_carries_the_same_24_bit_protocol_id() {
    assert_eq!(PROTOCOL_ID >> 24, 0, "the protocol ID is 24 bits");
    let source = std::fs::read_to_string(
        mmorpg_core::golden::workspace_root().join("web/src/protocol-id.ts"),
    )
    .unwrap();
    let browser = source
        .lines()
        .find_map(|line| line.strip_prefix("export const PROTOCOL_ID = 0x"))
        .and_then(|rest| rest.strip_suffix(';'))
        .map(|hex| u32::from_str_radix(hex, 16).unwrap())
        .expect("web/src/protocol-id.ts exports PROTOCOL_ID as a hex literal");
    assert_eq!(browser, PROTOCOL_ID, "run scripts/new-protocol-id.sh");
}
