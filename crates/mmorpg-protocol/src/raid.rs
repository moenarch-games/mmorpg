//! Night raid encoding (#530): a raider's membership and the zone's raid state in canonical
//! snapshots, and the raid's signals in player projections.
use mmorpg_core::{
    MAX_PLAYERS_PER_ZONE, RaidIncident, RaidLeg, RaidMember, RaidNotice, RaidOutcome, RaidPrep,
    RaidResolution, RaidState, RaidView,
};

use crate::ProtocolError;
use crate::wire::{decode_u8_count, encode_u8_count, read_bool, read_u8, take};

const fn leg_code(leg: RaidLeg) -> u8 {
    match leg {
        RaidLeg::Assault => 0,
        RaidLeg::Retreat => 1,
        RaidLeg::Camp => 2,
    }
}

/// Presence (`u8`), then incident (`u32`), leg (`u8`), waypoint (`u8`), anchor (`i32` × 2) and
/// the leg's start tick (`u64`).
pub(crate) fn encode_member(payload: &mut Vec<u8>, member: Option<RaidMember>) {
    payload.push(u8::from(member.is_some()));
    if let Some(member) = member {
        payload.extend_from_slice(&member.incident.to_be_bytes());
        payload.push(leg_code(member.leg));
        payload.push(member.waypoint);
        for value in member.anchor {
            payload.extend_from_slice(&value.to_be_bytes());
        }
        payload.extend_from_slice(&member.since.to_be_bytes());
    }
}

pub(crate) fn decode_member(
    payload: &[u8],
    offset: &mut usize,
) -> Result<Option<RaidMember>, ProtocolError> {
    if !read_bool(payload, offset)? {
        return Ok(None);
    }
    let incident = u32::from_be_bytes(take(payload, offset)?);
    let leg = match read_u8(payload, offset)? {
        0 => RaidLeg::Assault,
        1 => RaidLeg::Retreat,
        2 => RaidLeg::Camp,
        _ => return Err(ProtocolError::new("invalid raid leg")),
    };
    let waypoint = read_u8(payload, offset)?;
    let anchor = [
        i32::from_be_bytes(take(payload, offset)?),
        i32::from_be_bytes(take(payload, offset)?),
    ];
    let since = u64::from_be_bytes(take(payload, offset)?);
    Ok(Some(RaidMember {
        incident,
        leg,
        waypoint,
        anchor,
        since,
    }))
}

/// Incidents (`u32`) and damage (`u32`); the running incident: presence, ID (`u32`), phase (`u8`),
/// phase start (`u64`), the phases' participation snapshots (count `u8`, each `u8`) and the
/// participants (count `u8`, each `u32`); the last resolution: presence, incident (`u32`), tick
/// (`u64`) and outcome (`u8`, 0 defended, 1 lost); the open aid tasks (`u8`, #531); the rewarded
/// players (count `u16`, each `u32`, #531); the players who read the evening's tracks (count
/// `u16`, each `u32`, #532) and the readiness (`u8`, #532).
pub(crate) fn encode_state(payload: &mut Vec<u8>, state: &RaidState) -> Result<(), ProtocolError> {
    payload.extend_from_slice(&state.incidents.to_be_bytes());
    payload.extend_from_slice(&state.damage.to_be_bytes());
    payload.push(u8::from(state.active.is_some()));
    if let Some(incident) = &state.active {
        payload.extend_from_slice(&incident.id.to_be_bytes());
        payload.push(incident.phase);
        payload.extend_from_slice(&incident.phase_started.to_be_bytes());
        payload.push(encode_u8_count(
            incident.scaled_for.len(),
            usize::from(mmorpg_core::raid::RAID_PHASES),
            "raid phases exceed their count",
        )?);
        payload.extend_from_slice(&incident.scaled_for);
        payload.push(encode_u8_count(
            incident.participants.len(),
            MAX_PLAYERS_PER_ZONE,
            "raid participants exceed the zone capacity",
        )?);
        for player in &incident.participants {
            payload.extend_from_slice(&player.to_be_bytes());
        }
    }
    payload.push(u8::from(state.last.is_some()));
    if let Some(last) = state.last {
        payload.extend_from_slice(&last.incident.to_be_bytes());
        payload.extend_from_slice(&last.at.to_be_bytes());
        payload.push(match last.outcome {
            RaidOutcome::Defended => 0,
            RaidOutcome::Lost => 1,
        });
    }
    payload.push(state.aftermath);
    let rewarded = u16::try_from(state.rewarded.len())
        .ok()
        .filter(|&count| usize::from(count) <= MAX_PLAYERS_PER_ZONE)
        .ok_or_else(|| ProtocolError::new("raid rewards exceed the zone capacity"))?;
    payload.extend_from_slice(&rewarded.to_be_bytes());
    for player in &state.rewarded {
        payload.extend_from_slice(&player.to_be_bytes());
    }
    let scouted = u16::try_from(state.scouted.len())
        .ok()
        .filter(|&count| usize::from(count) <= MAX_PLAYERS_PER_ZONE)
        .ok_or_else(|| ProtocolError::new("raid scouts exceed the zone capacity"))?;
    payload.extend_from_slice(&scouted.to_be_bytes());
    for player in &state.scouted {
        payload.extend_from_slice(&player.to_be_bytes());
    }
    payload.push(state.readiness);
    Ok(())
}

pub(crate) fn decode_state(payload: &[u8], offset: &mut usize) -> Result<RaidState, ProtocolError> {
    let incidents = u32::from_be_bytes(take(payload, offset)?);
    let damage = u32::from_be_bytes(take(payload, offset)?);
    let active = if read_bool(payload, offset)? {
        let id = u32::from_be_bytes(take(payload, offset)?);
        let phase = read_u8(payload, offset)?;
        let phase_started = u64::from_be_bytes(take(payload, offset)?);
        let phases = decode_u8_count(
            payload,
            offset,
            usize::from(mmorpg_core::raid::RAID_PHASES),
            "raid phases exceed their count",
        )?;
        let scaled_for = (0..phases)
            .map(|_| read_u8(payload, offset))
            .collect::<Result<_, _>>()?;
        let count = decode_u8_count(
            payload,
            offset,
            MAX_PLAYERS_PER_ZONE,
            "raid participants exceed the zone capacity",
        )?;
        let participants = (0..count)
            .map(|_| take(payload, offset).map(u32::from_be_bytes))
            .collect::<Result<_, _>>()?;
        Some(RaidIncident {
            id,
            phase,
            phase_started,
            scaled_for,
            participants,
        })
    } else {
        None
    };
    let last = if read_bool(payload, offset)? {
        let incident = u32::from_be_bytes(take(payload, offset)?);
        let at = u64::from_be_bytes(take(payload, offset)?);
        let outcome = match read_u8(payload, offset)? {
            0 => RaidOutcome::Defended,
            1 => RaidOutcome::Lost,
            _ => return Err(ProtocolError::new("invalid raid outcome")),
        };
        Some(RaidResolution {
            incident,
            at,
            outcome,
        })
    } else {
        None
    };
    let aftermath = read_u8(payload, offset)?;
    let count = usize::from(u16::from_be_bytes(take(payload, offset)?));
    if count > MAX_PLAYERS_PER_ZONE {
        return Err(ProtocolError::new("raid rewards exceed the zone capacity"));
    }
    let rewarded = (0..count)
        .map(|_| take(payload, offset).map(u32::from_be_bytes))
        .collect::<Result<_, _>>()?;
    let count = usize::from(u16::from_be_bytes(take(payload, offset)?));
    if count > MAX_PLAYERS_PER_ZONE {
        return Err(ProtocolError::new("raid scouts exceed the zone capacity"));
    }
    let scouted = (0..count)
        .map(|_| take(payload, offset).map(u32::from_be_bytes))
        .collect::<Result<_, _>>()?;
    let readiness = read_u8(payload, offset)?;
    Ok(RaidState {
        incidents,
        active,
        damage,
        last,
        aftermath,
        rewarded,
        scouted,
        readiness,
    })
}

/// The view code's low three bits on a forewarned evening (#532): no phase and an intact barn.
const SIGNS_CODE: u8 = 5;
/// The high five bits on a forewarned evening: the readiness in bits 0–3, scouted in bit 4.
const SCOUTED_BIT: u8 = 1 << 4;

/// One code byte: 0 without a signal. Otherwise its low three bits are one more than the phase,
/// or [`SIGNS_CODE`] on a forewarned evening (#532). Its high five bits are the open aid tasks
/// after a resolved raid (#531), the readiness during an assault, or the readiness and the
/// viewer's scouted bit on a forewarned evening (#532). The integrity percent (`u8`) follows,
/// except on a forewarned evening, when the barn is whole.
pub(crate) fn encode_view(
    payload: &mut Vec<u8>,
    view: Option<RaidView>,
) -> Result<(), ProtocolError> {
    let Some(view) = view else {
        payload.push(0);
        return Ok(());
    };
    let invalid = || ProtocolError::new("invalid raid view");
    let readiness = u16::from(view.readiness);
    if view.signs {
        if view.phase != 0 || view.aid != 0 || view.integrity != 100 || readiness >= 1 << 4 {
            return Err(invalid());
        }
        payload
            .push(SIGNS_CODE | (view.readiness | if view.scouted { SCOUTED_BIT } else { 0 }) << 3);
        return Ok(());
    }
    let high = if view.phase == 0 {
        (view.readiness == 0).then_some(view.aid)
    } else {
        (view.aid == 0).then_some(view.readiness)
    };
    let high = high
        .filter(|&high| {
            high < 1 << 5 && !view.scouted && view.phase <= mmorpg_core::raid::RAID_PHASES
        })
        .ok_or_else(invalid)?;
    payload.extend_from_slice(&[(view.phase + 1) | high << 3, view.integrity]);
    Ok(())
}

pub(crate) fn decode_view(
    payload: &[u8],
    offset: &mut usize,
) -> Result<Option<RaidView>, ProtocolError> {
    let code = read_u8(payload, offset)?;
    let (low, high) = (code & 0b111, code >> 3);
    let invalid = || ProtocolError::new("invalid raid view");
    if low == SIGNS_CODE {
        return Ok(Some(RaidView {
            phase: 0,
            integrity: 100,
            aid: 0,
            signs: true,
            readiness: high & !SCOUTED_BIT,
            scouted: high & SCOUTED_BIT != 0,
        }));
    }
    let Some(phase) = low.checked_sub(1) else {
        return if code == 0 { Ok(None) } else { Err(invalid()) };
    };
    let integrity = read_u8(payload, offset)?;
    if phase > mmorpg_core::raid::RAID_PHASES || integrity > 100 {
        return Err(invalid());
    }
    let (aid, readiness) = if phase == 0 { (high, 0) } else { (0, high) };
    Ok(Some(RaidView {
        phase,
        integrity,
        aid,
        signs: false,
        readiness,
        scouted: false,
    }))
}

/// A raid notice event's flags byte (#532).
pub(crate) const fn notice_code(notice: RaidNotice) -> u8 {
    match notice {
        RaidNotice::Tracks => 0,
        RaidNotice::Warned => 1,
        RaidNotice::Braced => 2,
        RaidNotice::Held => 3,
        RaidNotice::Fell => 4,
    }
}

pub(crate) fn decode_notice(code: u8) -> Result<RaidNotice, ProtocolError> {
    Ok(match code {
        0 => RaidNotice::Tracks,
        1 => RaidNotice::Warned,
        2 => RaidNotice::Braced,
        3 => RaidNotice::Held,
        4 => RaidNotice::Fell,
        _ => return Err(ProtocolError::new("unknown raid notice")),
    })
}

/// A `RaidPrepare` step's byte (#532): 0 investigate, 1 report, `2 + k` brace `k`.
pub(crate) fn prep_code(prep: RaidPrep) -> u8 {
    match prep {
        RaidPrep::Investigate => 0,
        RaidPrep::Report => 1,
        RaidPrep::Brace(index) => index.saturating_add(2),
    }
}

pub(crate) fn decode_prep(code: u8) -> Result<RaidPrep, ProtocolError> {
    Ok(match code {
        0 => RaidPrep::Investigate,
        1 => RaidPrep::Report,
        _ if usize::from(code - 2) < mmorpg_core::raid::MAX_BRACES => RaidPrep::Brace(code - 2),
        _ => return Err(ProtocolError::new("unknown raid preparation")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(
        phase: u8,
        integrity: u8,
        aid: u8,
        signs: bool,
        readiness: u8,
        scouted: bool,
    ) -> RaidView {
        RaidView {
            phase,
            integrity,
            aid,
            signs,
            readiness,
            scouted,
        }
    }

    #[test]
    fn raid_views_round_trip_in_one_or_two_bytes() {
        for (expected, bytes) in [
            (view(0, 100, 0, true, 0b011, true), 1),
            (view(0, 100, 0, true, 0, false), 1),
            (view(2, 63, 0, false, 0b111, false), 2),
            (view(0, 20, 0b1_0110, false, 0, false), 2),
        ] {
            let mut payload = Vec::new();
            encode_view(&mut payload, Some(expected)).unwrap();
            assert_eq!(payload.len(), bytes);
            let mut offset = 0;
            assert_eq!(decode_view(&payload, &mut offset).unwrap(), Some(expected));
        }
        // Aid and readiness never share the code byte, nor signs a phase.
        for invalid in [
            view(0, 20, 0b1, false, 0b1, false),
            view(1, 20, 0b1, false, 0, false),
            view(1, 100, 0, true, 0, false),
            view(1, 50, 0, false, 0, true),
        ] {
            assert!(encode_view(&mut Vec::new(), Some(invalid)).is_err());
        }
        for prep in [RaidPrep::Investigate, RaidPrep::Report, RaidPrep::Brace(2)] {
            assert_eq!(decode_prep(prep_code(prep)).unwrap(), prep);
        }
        assert!(decode_prep(5).is_err());
    }
}
