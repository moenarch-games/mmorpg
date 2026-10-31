//! Night raid encoding (#530): a raider's membership and the zone's raid state in canonical
//! snapshots, and the raid's signals in player projections.
use mmorpg_core::{
    MAX_PLAYERS_PER_ZONE, RaidIncident, RaidLeg, RaidMember, RaidOutcome, RaidResolution,
    RaidState, RaidView,
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
/// players (count `u16`, each `u32`, #531).
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
    Ok(RaidState {
        incidents,
        active,
        damage,
        last,
        aftermath,
        rewarded,
    })
}

/// One code byte: 0 without a signal, otherwise one more than the phase in its low three bits
/// and the open aid tasks (#531) in its high five; then the integrity percent (`u8`).
pub(crate) fn encode_view(payload: &mut Vec<u8>, view: Option<RaidView>) {
    match view {
        None => payload.push(0),
        Some(view) => {
            payload.extend_from_slice(&[(view.phase + 1) | (view.aid << 3), view.integrity])
        }
    }
}

pub(crate) fn decode_view(
    payload: &[u8],
    offset: &mut usize,
) -> Result<Option<RaidView>, ProtocolError> {
    let code = read_u8(payload, offset)?;
    let Some(phase) = (code & 0b111).checked_sub(1) else {
        return if code == 0 {
            Ok(None)
        } else {
            Err(ProtocolError::new("invalid raid view"))
        };
    };
    let integrity = read_u8(payload, offset)?;
    if phase > mmorpg_core::raid::RAID_PHASES || integrity > 100 {
        return Err(ProtocolError::new("invalid raid view"));
    }
    Ok(Some(RaidView {
        phase,
        integrity,
        aid: code >> 3,
    }))
}
