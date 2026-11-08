//! Private depths wire forms (#404): the canonical run state and the run start a host binds
//! through its host inputs (part 2), and Captain Thorn's boss outcome and loot view (part 3).

use mmorpg_core::{
    BossAttempt, BossDrop, BossOutcome, BossProgress, CharacterId, DepthsDropView, DepthsLootView,
    DepthsRun, DepthsRunStart, DropState, DropView, ItemId, MAX_BOSS_DROPS, MAX_CHARACTER_ID_BYTES,
    MAX_PARTY_MEMBERS, RollChoice,
};

use crate::ProtocolError;
use crate::wire::{decode_u8_count, encode_u8_count, read_bool, read_u8, take};

fn encode_character(payload: &mut Vec<u8>, character: &CharacterId) -> Result<(), ProtocolError> {
    let bytes = character.as_str().as_bytes();
    payload.push(encode_u8_count(
        bytes.len(),
        MAX_CHARACTER_ID_BYTES,
        "depths character ID is too long",
    )?);
    payload.extend_from_slice(bytes);
    Ok(())
}

fn decode_character(payload: &[u8], offset: &mut usize) -> Result<CharacterId, ProtocolError> {
    let length = usize::from(read_u8(payload, offset)?);
    let end = offset
        .checked_add(length)
        .filter(|end| *end <= payload.len())
        .ok_or_else(|| ProtocolError::new("truncated depths character"))?;
    let text = std::str::from_utf8(&payload[*offset..end])
        .map_err(|_| ProtocolError::new("depths character is not text"))?;
    *offset = end;
    CharacterId::new(text.to_owned()).map_err(|_| ProtocolError::new("depths character is invalid"))
}

/// A character list of at most the party size (a count of zero allowed).
fn encode_characters(
    payload: &mut Vec<u8>,
    characters: &[CharacterId],
) -> Result<(), ProtocolError> {
    payload.push(encode_u8_count(
        characters.len(),
        MAX_PARTY_MEMBERS,
        "too many depths characters",
    )?);
    for character in characters {
        encode_character(payload, character)?;
    }
    Ok(())
}

fn decode_characters(
    payload: &[u8],
    offset: &mut usize,
) -> Result<Vec<CharacterId>, ProtocolError> {
    let count = decode_u8_count(
        payload,
        offset,
        MAX_PARTY_MEMBERS,
        "too many depths characters",
    )?;
    let mut characters = Vec::with_capacity(count);
    for _ in 0..count {
        let character = decode_character(payload, offset)?;
        if characters.contains(&character) {
            return Err(ProtocolError::new("depths list repeats a character"));
        }
        characters.push(character);
    }
    Ok(characters)
}

fn encode_roster(payload: &mut Vec<u8>, roster: &[CharacterId]) -> Result<(), ProtocolError> {
    if roster.is_empty() {
        return Err(ProtocolError::new("depths roster size out of range"));
    }
    encode_characters(payload, roster)
}

fn decode_roster(payload: &[u8], offset: &mut usize) -> Result<Vec<CharacterId>, ProtocolError> {
    let roster = decode_characters(payload, offset)?;
    if roster.is_empty() {
        return Err(ProtocolError::new("depths roster size out of range"));
    }
    Ok(roster)
}

const fn choice_code(choice: RollChoice) -> u8 {
    match choice {
        RollChoice::Need => 0,
        RollChoice::Greed => 1,
        RollChoice::Pass => 2,
    }
}

pub(crate) fn decode_choice(code: u8) -> Result<RollChoice, ProtocolError> {
    match code {
        0 => Ok(RollChoice::Need),
        1 => Ok(RollChoice::Greed),
        2 => Ok(RollChoice::Pass),
        _ => Err(ProtocolError::new("invalid roll choice")),
    }
}

pub(crate) const fn encode_choice(choice: RollChoice) -> u8 {
    choice_code(choice)
}

/// Canonical run state: a present flag, then the run ID, roster, cleared mask, checkpoint flag
/// and optional wipe tick, then the boss progress.
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
    payload.push(u8::from(run.boss.announced));
    payload.push(u8::from(run.boss.attempt.is_some()));
    if let Some(attempt) = &run.boss.attempt {
        encode_characters(payload, &attempt.eligible)?;
        payload.push(attempt.phase);
        payload.push(u8::from(attempt.adds_called));
        payload.push(u8::from(attempt.next_is_final));
    }
    payload.push(u8::from(run.boss.outcome.is_some()));
    if let Some(outcome) = &run.boss.outcome {
        encode_outcome(payload, outcome)?;
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
    let announced = read_bool(payload, offset)?;
    let attempt = if read_bool(payload, offset)? {
        let eligible = decode_characters(payload, offset)?;
        let phase = read_u8(payload, offset)?;
        let adds_called = read_bool(payload, offset)?;
        let next_is_final = read_bool(payload, offset)?;
        Some(BossAttempt {
            eligible,
            phase,
            adds_called,
            next_is_final,
        })
    } else {
        None
    };
    let outcome = if read_bool(payload, offset)? {
        Some(decode_outcome(payload, offset)?)
    } else {
        None
    };
    Ok(Some(DepthsRun {
        run_id,
        roster,
        cleared,
        checkpoint,
        wipe_at,
        boss: BossProgress {
            attempt,
            outcome,
            announced,
        },
    }))
}

fn encode_outcome(payload: &mut Vec<u8>, outcome: &BossOutcome) -> Result<(), ProtocolError> {
    payload.extend_from_slice(&outcome.version.to_be_bytes());
    payload.extend_from_slice(&outcome.confirmed.to_be_bytes());
    payload.extend_from_slice(&outcome.roll_seed.to_be_bytes());
    encode_characters(payload, &outcome.eligible)?;
    encode_characters(payload, &outcome.credited)?;
    payload.push(encode_u8_count(
        outcome.drops.len(),
        MAX_BOSS_DROPS,
        "too many boss drops",
    )?);
    for drop in &outcome.drops {
        payload.extend_from_slice(&drop.item.get().to_be_bytes());
        match &drop.state {
            DropState::Rolling { deadline, choices } => {
                payload.push(0);
                payload.extend_from_slice(&deadline.to_be_bytes());
                payload.push(encode_u8_count(
                    choices.len(),
                    MAX_PARTY_MEMBERS,
                    "too many roll choices",
                )?);
                for (character, choice) in choices {
                    encode_character(payload, character)?;
                    payload.push(choice_code(*choice));
                }
            }
            DropState::Held => payload.push(1),
            DropState::Awarded {
                winner,
                roll,
                need,
                version,
                delivered,
                full_notified,
            } => {
                payload.push(2);
                encode_character(payload, winner)?;
                payload.push(*roll);
                payload.push(u8::from(*need));
                payload.extend_from_slice(&version.to_be_bytes());
                payload.push(u8::from(*delivered));
                payload.push(u8::from(*full_notified));
            }
        }
    }
    Ok(())
}

fn decode_outcome(payload: &[u8], offset: &mut usize) -> Result<BossOutcome, ProtocolError> {
    let version = u32::from_be_bytes(take(payload, offset)?);
    let confirmed = u32::from_be_bytes(take(payload, offset)?);
    let roll_seed = u64::from_be_bytes(take(payload, offset)?);
    let eligible = decode_characters(payload, offset)?;
    let credited = decode_characters(payload, offset)?;
    let drop_count = decode_u8_count(payload, offset, MAX_BOSS_DROPS, "too many boss drops")?;
    let mut drops = Vec::with_capacity(drop_count);
    for _ in 0..drop_count {
        let item = ItemId::new(u16::from_be_bytes(take(payload, offset)?));
        let state = match read_u8(payload, offset)? {
            0 => {
                let deadline = u64::from_be_bytes(take(payload, offset)?);
                let count =
                    decode_u8_count(payload, offset, MAX_PARTY_MEMBERS, "too many roll choices")?;
                let mut choices = Vec::with_capacity(count);
                for _ in 0..count {
                    let character = decode_character(payload, offset)?;
                    choices.push((character, decode_choice(read_u8(payload, offset)?)?));
                }
                DropState::Rolling { deadline, choices }
            }
            1 => DropState::Held,
            2 => {
                let winner = decode_character(payload, offset)?;
                let roll = read_u8(payload, offset)?;
                let need = read_bool(payload, offset)?;
                let version = u32::from_be_bytes(take(payload, offset)?);
                let delivered = read_bool(payload, offset)?;
                let full_notified = read_bool(payload, offset)?;
                DropState::Awarded {
                    winner,
                    roll,
                    need,
                    version,
                    delivered,
                    full_notified,
                }
            }
            _ => return Err(ProtocolError::new("invalid boss drop state")),
        };
        drops.push(BossDrop { item, state });
    }
    Ok(BossOutcome {
        version,
        confirmed,
        roll_seed,
        eligible,
        drops,
        credited,
    })
}

/// A boss outcome by itself: the blob a host records in the run directory.
pub fn encode_boss_outcome(outcome: &BossOutcome) -> Result<Vec<u8>, ProtocolError> {
    let mut payload = Vec::new();
    encode_outcome(&mut payload, outcome)?;
    Ok(payload)
}

/// Restores a recorded boss outcome; trailing or malformed bytes fail closed.
pub fn decode_boss_outcome(payload: &[u8]) -> Result<BossOutcome, ProtocolError> {
    let mut offset = 0;
    let outcome = decode_outcome(payload, &mut offset)?;
    if offset != payload.len() {
        return Err(ProtocolError::new("trailing boss outcome bytes"));
    }
    Ok(outcome)
}

/// The run start a host binds to a depths zone: run ID, roster, cleared mask, checkpoint and
/// the recorded boss outcome, if any.
pub fn encode_depths_run_start(start: &DepthsRunStart) -> Result<Vec<u8>, ProtocolError> {
    let mut payload = Vec::with_capacity(32 + MAX_CHARACTER_ID_BYTES);
    payload.extend_from_slice(&start.run_id.to_be_bytes());
    encode_roster(&mut payload, &start.roster)?;
    payload.push(start.cleared);
    payload.push(u8::from(start.checkpoint));
    payload.push(u8::from(start.outcome.is_some()));
    if let Some(outcome) = &start.outcome {
        encode_outcome(&mut payload, outcome)?;
    }
    Ok(payload)
}

pub fn decode_depths_run_start(payload: &[u8]) -> Result<DepthsRunStart, ProtocolError> {
    let mut offset = 0;
    let run_id = u64::from_be_bytes(take(payload, &mut offset)?);
    let roster = decode_roster(payload, &mut offset)?;
    let cleared = read_u8(payload, &mut offset)?;
    let checkpoint = read_bool(payload, &mut offset)?;
    let outcome = if read_bool(payload, &mut offset)? {
        Some(decode_outcome(payload, &mut offset)?)
    } else {
        None
    };
    if offset != payload.len() {
        return Err(ProtocolError::new("trailing depths run bytes"));
    }
    Ok(DepthsRunStart {
        run_id,
        roster,
        cleared,
        checkpoint,
        outcome,
    })
}

/// Bytes of a loot sheet record: drop and item (`1 + 2`), a state tag (`1`), an answer code (`1`),
/// `may_need`, `claimable`, `won`, `need`, `pending` (5 × `1`), roll (`1`), deadline (`8`).
const DROP_SHEET_BYTES: usize = 3 + 1 + 1 + 5 + 1 + 8;
/// The run ID and the count, then the drops.
pub(crate) const MAX_DEPTHS_LOOT_SHEET_BYTES: usize = 8 + 1 + MAX_BOSS_DROPS * DROP_SHEET_BYTES;

/// The depths loot sheet of a projection (absent when the viewer has none): the run and its drops.
pub(crate) fn encode_loot_sheet(view: &DepthsLootView) -> Result<Vec<u8>, ProtocolError> {
    let mut sheet = Vec::with_capacity(MAX_DEPTHS_LOOT_SHEET_BYTES);
    sheet.extend_from_slice(&view.run_id.to_be_bytes());
    sheet.push(encode_u8_count(
        view.drops.len(),
        MAX_BOSS_DROPS,
        "too many boss drops",
    )?);
    for drop in &view.drops {
        sheet.push(drop.drop);
        sheet.extend_from_slice(&drop.item.get().to_be_bytes());
        let (tag, deadline, answer, may_need, claimable, won, roll, need, pending) =
            match drop.state {
                DropView::Rolling {
                    deadline,
                    answer,
                    may_need,
                } => (
                    0u8,
                    deadline,
                    answer.map_or(3, choice_code),
                    may_need,
                    false,
                    false,
                    0,
                    false,
                    false,
                ),
                DropView::Held { claimable } => (1, 0, 3, false, claimable, false, 0, false, false),
                DropView::Awarded {
                    won,
                    roll,
                    need,
                    pending,
                } => (2, 0, 3, false, false, won, roll, need, pending),
            };
        sheet.push(tag);
        sheet.push(answer);
        for flag in [may_need, claimable, won, need, pending] {
            sheet.push(u8::from(flag));
        }
        sheet.push(roll);
        sheet.extend_from_slice(&deadline.to_be_bytes());
    }
    Ok(sheet)
}

pub(crate) fn decode_loot_sheet(bytes: &[u8]) -> Result<DepthsLootView, ProtocolError> {
    let mut offset = 0;
    let view = read_loot_sheet(bytes, &mut offset)?;
    if offset != bytes.len() {
        return Err(ProtocolError::new("trailing depths loot sheet bytes"));
    }
    Ok(view)
}

pub(crate) fn read_loot_sheet(
    bytes: &[u8],
    offset: &mut usize,
) -> Result<DepthsLootView, ProtocolError> {
    let run_id = u64::from_be_bytes(take(bytes, offset)?);
    let count = decode_u8_count(bytes, offset, MAX_BOSS_DROPS, "too many boss drops")?;
    let mut drops = Vec::with_capacity(count);
    for _ in 0..count {
        let drop = read_u8(bytes, offset)?;
        let item = ItemId::new(u16::from_be_bytes(take(bytes, offset)?));
        let tag = read_u8(bytes, offset)?;
        let answer = read_u8(bytes, offset)?;
        let may_need = read_bool(bytes, offset)?;
        let claimable = read_bool(bytes, offset)?;
        let won = read_bool(bytes, offset)?;
        let need = read_bool(bytes, offset)?;
        let pending = read_bool(bytes, offset)?;
        let roll = read_u8(bytes, offset)?;
        let deadline = u64::from_be_bytes(take(bytes, offset)?);
        let state = match tag {
            0 => DropView::Rolling {
                deadline,
                answer: if answer == 3 {
                    None
                } else {
                    Some(decode_choice(answer)?)
                },
                may_need,
            },
            1 => DropView::Held { claimable },
            2 => DropView::Awarded {
                won,
                roll,
                need,
                pending,
            },
            _ => return Err(ProtocolError::new("invalid boss drop view")),
        };
        drops.push(DepthsDropView { drop, item, state });
    }
    Ok(DepthsLootView { run_id, drops })
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

    fn outcome() -> BossOutcome {
        BossOutcome {
            version: 3,
            confirmed: 2,
            roll_seed: 77,
            eligible: roster(&["aria", "bren"]),
            drops: vec![
                BossDrop {
                    item: ItemId::new(15),
                    state: DropState::Awarded {
                        winner: CharacterId::new("bren").unwrap(),
                        roll: 71,
                        need: true,
                        version: 2,
                        delivered: true,
                        full_notified: false,
                    },
                },
                BossDrop {
                    item: ItemId::new(16),
                    state: DropState::Rolling {
                        deadline: 90,
                        choices: vec![(CharacterId::new("aria").unwrap(), RollChoice::Pass)],
                    },
                },
            ],
            credited: roster(&["aria"]),
        }
    }

    #[test]
    fn a_run_start_round_trips_and_bad_rosters_fail_closed() {
        let start = DepthsRunStart {
            run_id: 7,
            roster: roster(&["aria", "bren"]),
            cleared: 0b101,
            checkpoint: true,
            outcome: Some(outcome()),
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
            boss: BossProgress {
                attempt: Some(BossAttempt {
                    eligible: roster(&["aria"]),
                    phase: 2,
                    adds_called: true,
                    next_is_final: false,
                }),
                outcome: Some(outcome()),
                announced: true,
            },
        };
        let mut bytes = Vec::new();
        encode_run(&mut bytes, Some(&run)).unwrap();
        let mut offset = 0;
        assert_eq!(decode_run(&bytes, &mut offset).unwrap(), Some(run));
        assert_eq!(offset, bytes.len());
    }

    #[test]
    fn a_boss_outcome_round_trips_alone_and_the_loot_sheet_keeps_a_viewer_answer() {
        let outcome = outcome();
        let bytes = encode_boss_outcome(&outcome).unwrap();
        assert_eq!(decode_boss_outcome(&bytes).unwrap(), outcome);
        let mut trailing = bytes.clone();
        trailing.push(9);
        assert!(decode_boss_outcome(&trailing).is_err());

        let view = DepthsLootView {
            run_id: 4,
            drops: vec![
                DepthsDropView {
                    drop: 0,
                    item: ItemId::new(16),
                    state: DropView::Rolling {
                        deadline: 90,
                        answer: Some(RollChoice::Greed),
                        may_need: true,
                    },
                },
                DepthsDropView {
                    drop: 1,
                    item: ItemId::new(15),
                    state: DropView::Awarded {
                        won: true,
                        roll: 71,
                        need: false,
                        pending: true,
                    },
                },
            ],
        };
        let bytes = encode_loot_sheet(&view).unwrap();
        assert!(bytes.len() <= MAX_DEPTHS_LOOT_SHEET_BYTES);
        assert_eq!(decode_loot_sheet(&bytes).unwrap(), view);
    }
}
