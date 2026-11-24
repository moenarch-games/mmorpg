//! Deterministic work samples of `claim-expiry` in `.performance/contract.json` (#1141),
//! printed as the `operations` part of `coding-tooling/work-evidence/v1`;
//! `scripts/operation-work.py` adds the envelope. Counters only; nothing is timed.
//!
//! A zone holds `characters` claimed sessions and `waiting` sessions that never claim.
//! Over the whole claim timeout, the per-tick expiry visits only the waiting sessions,
//! once each on the tick their wait ends, however many characters play.
use game_server::{MatchRuntime, ReconnectToken};
use mmorpg_core::{CharacterId, ClassChoice, ZoneId, ZoneSimulation, content_for_zone};
use mmorpg_game_server::{CLAIM_TIMEOUT_TICKS, ZoneGameServerAdapter};
use std::error::Error;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn token(index: usize) -> ReconnectToken {
    let mut bytes = [0; 16];
    bytes[..8].copy_from_slice(&(index as u64 + 1).to_be_bytes());
    ReconnectToken(bytes)
}

fn claim_expiry(characters: usize, waiting: usize) -> Result<String> {
    let zone_id = ZoneId::new(1);
    let mut adapter = ZoneGameServerAdapter::with_content(zone_id, content_for_zone(zone_id))?;
    adapter.require_durable_character_claims();
    let mut runtime = MatchRuntime::new(adapter, 30);
    let mut records = ZoneSimulation::with_content(zone_id, content_for_zone(zone_id))?;
    let class = ClassChoice::from_appearance(4).ok_or("class")?;
    for index in 0..characters {
        let session = runtime.admit(token(index))?;
        let player = u32::try_from(index)? + 1;
        records.create_character(player, class)?;
        let record =
            records.capture_character(player, CharacterId::new(format!("work-{index}"))?)?;
        let input = ZoneGameServerAdapter::character_admission_input(&record)?;
        runtime.submit_host_input(session.player_id, &input)?;
    }
    for index in 0..waiting {
        runtime.admit(token(characters + index))?;
    }
    let before = runtime.simulation().claim_expiry_visits();
    for _ in 0..=CLAIM_TIMEOUT_TICKS {
        runtime.advance_tick()?;
    }
    if runtime.slot_count() != characters {
        return Err(format!(
            "{} sessions remain, expected {characters}",
            runtime.slot_count()
        )
        .into());
    }
    let visited = runtime.simulation().claim_expiry_visits() - before;
    Ok(format!(
        "{{\"dimensions\":{{\"characters\":{characters},\"waiting\":{waiting}}},\"metrics\":{{\"claim_waits_visited\":{visited}}}}}"
    ))
}

fn main() -> Result<()> {
    let samples = [(16, 32), (128, 32), (448, 32), (448, 8)]
        .into_iter()
        .map(|(characters, waiting)| claim_expiry(characters, waiting))
        .collect::<Result<Vec<_>>>()?;
    println!(
        "{{\"operations\":[{{\"id\":\"claim-expiry\",\"samples\":[{}]}}]}}",
        samples.join(",")
    );
    Ok(())
}
