#![forbid(unsafe_code)]

//! Wire encoding of zone commands and snapshots (docs/PROTOCOL.md), identified
//! by [`PROTOCOL_ID`].
//!
//! Snapshots come in two scopes: canonical (trusted replay and recovery) and
//! player-visible (one player's projection within a single-datagram byte
//! budget). All multibyte fields are big-endian and every decoder is strict.

mod canonical;
mod character;
mod chat;
mod command;
mod inventory;
mod loot;
mod projection;
mod quest;
mod raid;
mod rewards;
mod wire;

pub use canonical::{decode_canonical_snapshot, encode_canonical_snapshot};
pub use character::{
    CHARACTER_RECORD_WIRE_VERSION, decode_character_record, encode_character_record,
};
pub use command::{
    decode_command, decode_observed_command, encode_command, encode_observed_command,
};
pub use projection::{
    DEBRIS_RECORD_BYTES, ENTITY_RECORD_BYTES, EVENT_RECORD_BYTES, MAX_WIRE_ENTITIES,
    PLAYER_SNAPSHOT_FIXED_BYTES, PackedSnapshot, decode_snapshot, encode_snapshot, pack_snapshot,
};
pub use rewards::{MAX_REWARD_RECEIPT_BYTES, decode_reward_receipt, encode_reward_receipt};

use std::error::Error;
use std::fmt;

/// Identity of the host <-> client wire format: commands, player projections, presentation
/// observations and canonical snapshots (and so checkpoints and saved browser worlds).
///
/// A random 24-bit token, not a counter. Any change to one of those layouts replaces it with
/// fresh random bytes by running `scripts/new-protocol-id.sh`, which also rewrites the browser's
/// copy in `web/src/protocol-id.ts`. Two parallel format changes therefore edit this line
/// differently and git refuses to merge them silently; decoders accept only this exact value
/// (docs/adr/0006-merge-safe-format-identities.md). Snapshots lead with its three bytes and
/// commands with its low byte.
pub const PROTOCOL_ID: u32 = 0x0084_dfaf;

/// Smallest WebTransport datagram payload measured over the pinned stack:
/// QUIC's 1,200-byte initial MTU before path MTU discovery, observed as 1,161
/// bytes by both peers (`mmorpg-client` `connected_world` tests). This remains
/// the current projection-policy budget even though the transport can fragment
/// larger session frames.
pub const MEASURED_MIN_DATAGRAM_BYTES: usize = 1_161;
/// `game-server` session frame header in front of every snapshot payload.
pub const SESSION_SNAPSHOT_HEADER_BYTES: usize = 20;
/// Headroom for transport overhead the measurement does not cover.
pub const DATAGRAM_SAFETY_MARGIN_BYTES: usize = 64;
/// Largest player projection payload under the current MMO relevance policy.
/// Transport fragmentation permits future sections to grow beyond this budget
/// once the projection policy and both decoders change together.
pub const MAX_PLAYER_PROJECTION_BYTES: usize =
    MEASURED_MIN_DATAGRAM_BYTES - SESSION_SNAPSHOT_HEADER_BYTES - DATAGRAM_SAFETY_MARGIN_BYTES;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolError {
    message: String,
}

impl ProtocolError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for ProtocolError {}
