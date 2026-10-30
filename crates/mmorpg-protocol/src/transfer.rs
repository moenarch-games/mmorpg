//! Character transfer encoding (#145): what a source host sends a destination
//! host over the trusted LAN link and what the character store keeps while a
//! handoff is pending.
//!
//! `version u8 · transfer ID u64 · source zone u32 · cutover tick u64 ·
//! cutover sequence u32 · forward i8 · strafe i8 · rise i8 · velocity 3 × i32 ·
//! record length u32 · record`, big-endian, strict: another version, a zero
//! transfer ID, a truncated, over-long or trailing payload fails closed.

use mmorpg_core::{CharacterTransfer, TransferId, TransferMotion, ZoneId};

use crate::ProtocolError;
use crate::character::{decode_character_record, encode_character_record};
use crate::wire::{read_u8, take};

/// Identity of the layout above: a random token, not a counter (ADR 0006).
pub const CHARACTER_TRANSFER_WIRE_VERSION: u8 = 0x5c;
/// Far above any record; bounds what a corrupt length can make a reader take.
const MAX_TRANSFER_RECORD_BYTES: usize = 64 * 1024;

pub fn encode_character_transfer(transfer: &CharacterTransfer) -> Result<Vec<u8>, ProtocolError> {
    let record = encode_character_record(&transfer.record)?;
    let length = u32::try_from(record.len())
        .ok()
        .filter(|_| record.len() <= MAX_TRANSFER_RECORD_BYTES)
        .ok_or_else(|| ProtocolError::new("transferred record is too large"))?;
    let mut payload = Vec::with_capacity(48 + record.len());
    payload.push(CHARACTER_TRANSFER_WIRE_VERSION);
    payload.extend_from_slice(&transfer.transfer_id.get().to_be_bytes());
    payload.extend_from_slice(&transfer.source_zone.get().to_be_bytes());
    payload.extend_from_slice(&transfer.cutover_tick.to_be_bytes());
    payload.extend_from_slice(&transfer.cutover_sequence.to_be_bytes());
    let motion = transfer.motion;
    for axis in [motion.forward, motion.strafe, motion.rise] {
        payload.extend_from_slice(&axis.to_be_bytes());
    }
    for component in motion.velocity {
        payload.extend_from_slice(&component.to_be_bytes());
    }
    payload.extend_from_slice(&length.to_be_bytes());
    payload.extend_from_slice(&record);
    Ok(payload)
}

pub fn decode_character_transfer(payload: &[u8]) -> Result<CharacterTransfer, ProtocolError> {
    let offset = &mut 0;
    if read_u8(payload, offset)? != CHARACTER_TRANSFER_WIRE_VERSION {
        return Err(ProtocolError::new(
            "unsupported character transfer wire version",
        ));
    }
    let transfer_id = TransferId::new(u64::from_be_bytes(take(payload, offset)?))
        .map_err(|_| ProtocolError::new("transfer ID must be nonzero"))?;
    let source_zone = ZoneId::new(u32::from_be_bytes(take(payload, offset)?));
    let cutover_tick = u64::from_be_bytes(take(payload, offset)?);
    let cutover_sequence = u32::from_be_bytes(take(payload, offset)?);
    let forward = i8::from_be_bytes(take(payload, offset)?);
    let strafe = i8::from_be_bytes(take(payload, offset)?);
    let rise = i8::from_be_bytes(take(payload, offset)?);
    let mut velocity = [0; 3];
    for component in &mut velocity {
        *component = i32::from_be_bytes(take(payload, offset)?);
    }
    let length = usize::try_from(u32::from_be_bytes(take(payload, offset)?)).unwrap_or(usize::MAX);
    if length > MAX_TRANSFER_RECORD_BYTES {
        return Err(ProtocolError::new("transferred record is too large"));
    }
    let end = offset
        .checked_add(length)
        .filter(|end| *end == payload.len())
        .ok_or_else(|| ProtocolError::new("character transfer length is wrong"))?;
    let record = decode_character_record(&payload[*offset..end])?;
    Ok(CharacterTransfer {
        transfer_id,
        source_zone,
        cutover_tick,
        cutover_sequence,
        record,
        motion: TransferMotion {
            forward,
            strafe,
            rise,
            velocity,
        },
    })
}
