#![forbid(unsafe_code)]

//! Versioned wire encoding of zone commands and snapshots (docs/PROTOCOL.md).
//!
//! Commands use wire version 9. Snapshots use wire version 14 in two scopes:
//! canonical (trusted replay and recovery) and player-visible (one player's
//! projection within a single-datagram byte budget). All multibyte fields are
//! big-endian and every decoder is strict.

mod canonical;
mod chat;
mod command;
mod inventory;
mod loot;
mod projection;
mod quest;
mod wire;

pub use canonical::{decode_canonical_snapshot, encode_canonical_snapshot};
pub use command::{COMMAND_WIRE_VERSION, decode_command, encode_command};
pub use projection::{
    ENTITY_RECORD_BYTES, EVENT_RECORD_BYTES, MAX_WIRE_ENTITIES, PLAYER_SNAPSHOT_FIXED_BYTES,
    PackedSnapshot, decode_snapshot, encode_snapshot, pack_snapshot,
};

use std::error::Error;
use std::fmt;

pub const SNAPSHOT_WIRE_VERSION: u8 = 14;

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
