//! Redirect notice (#145, TECH-007): what a departed player's session receives
//! in place of its projection once the source zone retired it.
//!
//! The client reconnects to `route` and claims its character there; the
//! destination accepts only commands after `cutover_sequence`. The notice
//! comes only after the cutover is authorized (the source committed the
//! handoff), never on staging or acceptance.
//!
//! `common header (scope 3, source zone, source tick) · destination zone u32 ·
//! transfer ID u64 · cutover sequence u32 · route length u8 · route (UTF-8)`,
//! big-endian, strict: a zero transfer ID, an empty or over-long route, a
//! truncated or trailing payload fails closed.

use mmorpg_core::{TransferId, ZoneId};

use crate::ProtocolError;
use crate::wire::{REDIRECT_SCOPE, decode_common_header, encode_common_header, read_u8, take};

/// Upper bound on the route a notice carries.
pub const MAX_REDIRECT_ROUTE_BYTES: usize = 255;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RedirectNotice {
    /// The source zone the player left and its tick.
    pub zone_id: ZoneId,
    pub tick: u64,
    pub destination: ZoneId,
    pub transfer_id: TransferId,
    /// The last command sequence the source applied.
    pub cutover_sequence: u32,
    /// Where the destination zone's host accepts the player's connection.
    pub route: String,
}

pub fn encode_redirect_notice(notice: &RedirectNotice) -> Result<Vec<u8>, ProtocolError> {
    let route = notice.route.as_bytes();
    let length = u8::try_from(route.len())
        .ok()
        .filter(|&length| length > 0)
        .ok_or_else(|| ProtocolError::new("redirect route must be 1 to 255 bytes"))?;
    let mut payload = Vec::with_capacity(33 + route.len());
    encode_common_header(&mut payload, REDIRECT_SCOPE, notice.zone_id, notice.tick);
    payload.extend_from_slice(&notice.destination.get().to_be_bytes());
    payload.extend_from_slice(&notice.transfer_id.get().to_be_bytes());
    payload.extend_from_slice(&notice.cutover_sequence.to_be_bytes());
    payload.push(length);
    payload.extend_from_slice(route);
    Ok(payload)
}

pub fn decode_redirect_notice(payload: &[u8]) -> Result<RedirectNotice, ProtocolError> {
    let offset = &mut 0;
    let (zone_id, tick) = decode_common_header(payload, offset, REDIRECT_SCOPE)?;
    let destination = ZoneId::new(u32::from_be_bytes(take(payload, offset)?));
    let transfer_id = TransferId::new(u64::from_be_bytes(take(payload, offset)?))
        .map_err(|_| ProtocolError::new("transfer ID must be nonzero"))?;
    let cutover_sequence = u32::from_be_bytes(take(payload, offset)?);
    let length = usize::from(read_u8(payload, offset)?);
    let route = payload
        .get(*offset..)
        .filter(|rest| length > 0 && rest.len() == length)
        .ok_or_else(|| ProtocolError::new("invalid redirect route length"))?;
    let route = std::str::from_utf8(route)
        .map_err(|_| ProtocolError::new("redirect route must be UTF-8"))?
        .to_owned();
    Ok(RedirectNotice {
        zone_id,
        tick,
        destination,
        transfer_id,
        cutover_sequence,
        route,
    })
}

/// Whether a session snapshot payload is a redirect notice rather than a
/// player projection.
#[must_use]
pub fn is_redirect_notice(payload: &[u8]) -> bool {
    decode_common_header(payload, &mut 0, REDIRECT_SCOPE).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notice() -> RedirectNotice {
        RedirectNotice {
            zone_id: ZoneId::new(901),
            tick: 1_234,
            destination: ZoneId::new(902),
            transfer_id: TransferId::new(0xfeed).unwrap(),
            cutover_sequence: 17,
            route: "https://127.0.0.1:4434/game/zone-902".into(),
        }
    }

    #[test]
    fn notices_round_trip_and_fail_closed() {
        let payload = encode_redirect_notice(&notice()).unwrap();
        assert!(is_redirect_notice(&payload));
        assert_eq!(decode_redirect_notice(&payload).unwrap(), notice());
        assert!(decode_redirect_notice(&payload[..payload.len() - 1]).is_err());
        let mut trailing = payload.clone();
        trailing.push(0);
        assert!(decode_redirect_notice(&trailing).is_err());
        let empty = RedirectNotice {
            route: String::new(),
            ..notice()
        };
        assert!(encode_redirect_notice(&empty).is_err());
        let long = RedirectNotice {
            route: "x".repeat(MAX_REDIRECT_ROUTE_BYTES + 1),
            ..notice()
        };
        assert!(encode_redirect_notice(&long).is_err());
    }
}
