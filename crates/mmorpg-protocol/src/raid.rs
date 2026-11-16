//! Night raid encoding (#530): a raider's membership and the zone's raid state in canonical
//! snapshots, and the raid's signals in player projections.
use mmorpg_core::{
    MAX_PLAYERS_PER_ZONE, RaidIncident, RaidLeg, RaidMember, RaidNotice, RaidOutcome, RaidPrep,
    RaidResolution, RaidState, RaidView,
};

use crate::ProtocolError;
use crate::wire::{
    decode_u8_count, decode_u32_set, encode_u8_count, encode_u32_set, read_bool, read_u8, take,
};

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
/// (`u64`) and outcome (`u8`, 0 defended, 1 lost); the open aid tasks (a `u32` set, #531, #882);
/// the rewarded players (count `u16`, each `u32`, #531); the players who read the evening's
/// tracks (count `u16`, each `u32`, #532), whether the guards are warned (`u8`) and the braced
/// spots (a `u32` set, #532, #882).
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
    encode_u32_set(
        payload,
        &state.aftermath,
        "raid aid tasks exceed their count",
    )?;
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
    payload.push(u8::from(state.warned));
    encode_u32_set(payload, &state.braced, "raid braces exceed their count")?;
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
    let aftermath = decode_u32_set(payload, offset, "raid aid tasks are invalid")?;
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
    let warned = read_bool(payload, offset)?;
    let braced = decode_u32_set(payload, offset, "raid braces are invalid")?;
    Ok(RaidState {
        incidents,
        active,
        damage,
        last,
        aftermath,
        rewarded,
        scouted,
        warned,
        braced,
    })
}

/// The view's flag bits: the evening's signs, the warned guards and the viewer's scouting.
const SIGNS_BIT: u8 = 1;
const WARNED_BIT: u8 = 1 << 1;
const SCOUTED_BIT: u8 = 1 << 2;

/// Presence (`u8`); then the phase (`u8`, 0 once resolved), the integrity percent (`u8`), flags
/// (`u8`: signs, warned, scouted), the open aid tasks and the braced spots (`u32` sets, #531,
/// #532, #882). Signs come only without a phase and with the barn whole, scouting only with
/// signs; aid never comes with preparations, nor bracing without the warning.
pub(crate) fn encode_view(
    payload: &mut Vec<u8>,
    view: Option<&RaidView>,
) -> Result<(), ProtocolError> {
    let Some(view) = view else {
        payload.push(0);
        return Ok(());
    };
    if !view_valid(view) {
        return Err(ProtocolError::new("invalid raid view"));
    }
    payload.extend_from_slice(&[
        1,
        view.phase,
        view.integrity,
        flag(view.signs, SIGNS_BIT)
            | flag(view.warned, WARNED_BIT)
            | flag(view.scouted, SCOUTED_BIT),
    ]);
    encode_u32_set(payload, &view.aid, "raid aid tasks exceed their count")?;
    encode_u32_set(payload, &view.braced, "raid braces exceed their count")
}

const fn flag(set: bool, bit: u8) -> u8 {
    if set { bit } else { 0 }
}

fn view_valid(view: &RaidView) -> bool {
    view.phase <= mmorpg_core::raid::RAID_PHASES
        && view.integrity <= 100
        && (!view.signs || (view.phase == 0 && view.aid.is_empty() && view.integrity == 100))
        && (!view.scouted || view.signs)
        && (view.aid.is_empty() || (!view.warned && view.braced.is_empty()))
        && (view.braced.is_empty() || view.warned)
}

pub(crate) fn decode_view(
    payload: &[u8],
    offset: &mut usize,
) -> Result<Option<RaidView>, ProtocolError> {
    if !read_bool(payload, offset)? {
        return Ok(None);
    }
    let [phase, integrity, flags] = take(payload, offset)?;
    if flags & !(SIGNS_BIT | WARNED_BIT | SCOUTED_BIT) != 0 {
        return Err(ProtocolError::new("invalid raid view"));
    }
    let view = RaidView {
        phase,
        integrity,
        signs: flags & SIGNS_BIT != 0,
        warned: flags & WARNED_BIT != 0,
        scouted: flags & SCOUTED_BIT != 0,
        aid: decode_u32_set(payload, offset, "raid aid tasks are invalid")?,
        braced: decode_u32_set(payload, offset, "raid braces are invalid")?,
    };
    if !view_valid(&view) {
        return Err(ProtocolError::new("invalid raid view"));
    }
    Ok(Some(view))
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

/// A `RaidPrepare` step (#532): its kind (`u8`: 0 investigate, 1 report, 2 brace) and the brace
/// spot's index (`u32`, 0 otherwise).
pub(crate) const PREP_BYTES: usize = 1 + 4;

pub(crate) fn encode_prep(prep: RaidPrep) -> [u8; PREP_BYTES] {
    let (kind, index) = match prep {
        RaidPrep::Investigate => (0, 0),
        RaidPrep::Report => (1, 0),
        RaidPrep::Brace(index) => (2, index),
    };
    let [a, b, c, d] = index.to_be_bytes();
    [kind, a, b, c, d]
}

pub(crate) fn decode_prep(bytes: [u8; PREP_BYTES]) -> Result<RaidPrep, ProtocolError> {
    let [kind, a, b, c, d] = bytes;
    let index = u32::from_be_bytes([a, b, c, d]);
    Ok(match (kind, index) {
        (0, 0) => RaidPrep::Investigate,
        (1, 0) => RaidPrep::Report,
        (2, index) => RaidPrep::Brace(index),
        _ => return Err(ProtocolError::new("unknown raid preparation")),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    fn view(
        phase: u8,
        integrity: u8,
        aid: &[u32],
        signs: bool,
        braced: Option<&[u32]>,
    ) -> RaidView {
        RaidView {
            phase,
            integrity,
            aid: aid.iter().copied().collect(),
            signs,
            warned: braced.is_some(),
            braced: braced.unwrap_or_default().iter().copied().collect(),
            scouted: false,
        }
    }

    #[test]
    fn raid_views_round_trip() {
        let scouted = RaidView {
            scouted: true,
            ..view(0, 100, &[], true, Some(&[0, 1]))
        };
        for expected in [
            scouted,
            view(0, 100, &[], true, None),
            view(2, 63, &[], false, Some(&[0, 1, 2, 40])),
            view(0, 20, &[1, 2, 4, 70], false, None),
        ] {
            let mut payload = Vec::new();
            encode_view(&mut payload, Some(&expected)).unwrap();
            let mut offset = 0;
            assert_eq!(decode_view(&payload, &mut offset).unwrap(), Some(expected));
            assert_eq!(offset, payload.len());
        }
        // Aid and preparations never come together, nor signs with a phase.
        for invalid in [
            view(0, 20, &[0], false, Some(&[])),
            view(1, 100, &[], true, None),
            RaidView {
                braced: BTreeSet::from([1]),
                ..view(1, 50, &[], false, None)
            },
            RaidView {
                scouted: true,
                ..view(1, 50, &[], false, None)
            },
        ] {
            assert!(encode_view(&mut Vec::new(), Some(&invalid)).is_err());
        }
        for prep in [
            RaidPrep::Investigate,
            RaidPrep::Report,
            RaidPrep::Brace(70_000),
        ] {
            assert_eq!(decode_prep(encode_prep(prep)).unwrap(), prep);
        }
        assert!(decode_prep([3, 0, 0, 0, 0]).is_err());
        assert!(decode_prep([1, 0, 0, 0, 1]).is_err());
    }
}
