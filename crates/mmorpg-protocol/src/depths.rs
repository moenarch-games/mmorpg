//! Private depths run wire form (#404 part 2): the canonical run state, and the run start a
//! host binds to a depths zone through its host inputs.

use mmorpg_core::{
    CharacterId, DepthsRun, DepthsRunStart, MAX_CHARACTER_ID_BYTES, MAX_PARTY_MEMBERS,
};

use crate::ProtocolError;
use crate::wire::{read_bool, read_u8, take};

fn encode_roster(payload: &mut Vec<u8>, roster: &[CharacterId]) -> Result<(), ProtocolError> {
    if roster.is_empty() || roster.len() > MAX_PARTY_MEMBERS {
        return Err(ProtocolError::new("depths roster size out of range"));
    }
    payload.push(u8::try_from(roster.len()).map_err(|_| ProtocolError::new("depths roster size"))?);
    for character in roster {
        let bytes = character.as_str().as_bytes();
        let length = u8::try_from(bytes.len())
            .ok()
            .filter(|&length| usize::from(length) <= MAX_CHARACTER_ID_BYTES)
            .ok_or_else(|| ProtocolError::new("depths roster character too long"))?;
        payload.push(length);
        payload.extend_from_slice(bytes);
    }
    Ok(())
}

fn decode_roster(payload: &[u8], offset: &mut usize) -> Result<Vec<CharacterId>, ProtocolError> {
    let count = read_u8(payload, offset)?;
    if count == 0 || usize::from(count) > MAX_PARTY_MEMBERS {
        return Err(ProtocolError::new("depths roster size out of range"));
    }
    let mut roster = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        let length = usize::from(read_u8(payload, offset)?);
        let end = offset
            .checked_add(length)
            .filter(|end| *end <= payload.len())
            .ok_or_else(|| ProtocolError::new("truncated depths roster"))?;
        let text = std::str::from_utf8(&payload[*offset..end])
            .map_err(|_| ProtocolError::new("depths roster character is not text"))?;
        *offset = end;
        let character = CharacterId::new(text.to_owned())
            .map_err(|_| ProtocolError::new("depths roster character is invalid"))?;
        if roster.contains(&character) {
            return Err(ProtocolError::new("depths roster repeats a character"));
        }
        roster.push(character);
    }
    Ok(roster)
}

/// Canonical run state: a present flag, then the run ID, roster, cleared mask, checkpoint flag
/// and optional wipe tick.
pub(crate) fn encode_run(
    payload: &mut Vec<u8>,
    run: Option<&DepthsRun>,
) -> Result<(), ProtocolError> {
    payload.push(u8::from(run.is_some()));
    let Some(run) = run else {
        return Ok(());
    };
    payload.extend_from_slice(&run.run_id.to_be_bytes());
    encode_roster(payload, &run.roster)?;
    payload.push(run.cleared);
    payload.push(u8::from(run.checkpoint));
    payload.push(u8::from(run.wipe_at.is_some()));
    if let Some(at) = run.wipe_at {
        payload.extend_from_slice(&at.to_be_bytes());
    }
    Ok(())
}

pub(crate) fn decode_run(
    payload: &[u8],
    offset: &mut usize,
) -> Result<Option<DepthsRun>, ProtocolError> {
    if !read_bool(payload, offset)? {
        return Ok(None);
    }
    let run_id = u64::from_be_bytes(take(payload, offset)?);
    let roster = decode_roster(payload, offset)?;
    let cleared = read_u8(payload, offset)?;
    let checkpoint = read_bool(payload, offset)?;
    let wipe_at = if read_bool(payload, offset)? {
        Some(u64::from_be_bytes(take(payload, offset)?))
    } else {
        None
    };
    Ok(Some(DepthsRun {
        run_id,
        roster,
        cleared,
        checkpoint,
        wipe_at,
    }))
}

/// The run start a host binds to a depths zone: run ID, roster, cleared mask and checkpoint.
pub fn encode_depths_run_start(start: &DepthsRunStart) -> Result<Vec<u8>, ProtocolError> {
    let mut payload = Vec::with_capacity(32 + MAX_CHARACTER_ID_BYTES);
    payload.extend_from_slice(&start.run_id.to_be_bytes());
    encode_roster(&mut payload, &start.roster)?;
    payload.push(start.cleared);
    payload.push(u8::from(start.checkpoint));
    Ok(payload)
}

pub fn decode_depths_run_start(payload: &[u8]) -> Result<DepthsRunStart, ProtocolError> {
    let mut offset = 0;
    let run_id = u64::from_be_bytes(take(payload, &mut offset)?);
    let roster = decode_roster(payload, &mut offset)?;
    let cleared = read_u8(payload, &mut offset)?;
    let checkpoint = read_bool(payload, &mut offset)?;
    if offset != payload.len() {
        return Err(ProtocolError::new("trailing depths run bytes"));
    }
    Ok(DepthsRunStart {
        run_id,
        roster,
        cleared,
        checkpoint,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roster(names: &[&str]) -> Vec<CharacterId> {
        names
            .iter()
            .map(|name| CharacterId::new(*name).unwrap())
            .collect()
    }

    #[test]
    fn a_run_start_round_trips_and_bad_rosters_fail_closed() {
        let start = DepthsRunStart {
            run_id: 7,
            roster: roster(&["aria", "bren"]),
            cleared: 0b101,
            checkpoint: true,
        };
        let bytes = encode_depths_run_start(&start).unwrap();
        assert_eq!(decode_depths_run_start(&bytes).unwrap(), start);
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(decode_depths_run_start(&trailing).is_err());
        assert!(
            encode_depths_run_start(&DepthsRunStart {
                roster: Vec::new(),
                ..start
            })
            .is_err()
        );
    }

    #[test]
    fn the_canonical_run_state_round_trips() {
        let run = DepthsRun {
            run_id: 9,
            roster: roster(&["aria"]),
            cleared: 1,
            checkpoint: true,
            wipe_at: Some(120),
        };
        let mut bytes = Vec::new();
        encode_run(&mut bytes, Some(&run)).unwrap();
        let mut offset = 0;
        assert_eq!(decode_run(&bytes, &mut offset).unwrap(), Some(run));
        assert_eq!(offset, bytes.len());
    }
}
