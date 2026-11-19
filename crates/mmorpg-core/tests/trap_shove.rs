//! Knocking a bandit into a trap (#955): a Warden fighting a Redbrand bandit beside the
//! Redbrand camp's spike pit (#300) turns so the pit lies behind it and Shield Bash (#287) shoves
//! it in, where the spikes hit it on their own schedule while it stands stunned. A bash from too
//! far out falls short, one along the wrong line misses the pit, and a bandit still immune from
//! a forced move is not moved at all, so none of them earns the environmental hit. The same
//! commands replay to the same outcome.
mod support;

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::greyhaven_vale::{self, traps};
use mmorpg_core::{
    CanonicalZoneSnapshot, CreatureId, EntityRef, ForcedState, ZoneCommand, ZoneEvent, ZoneId,
    ZoneSimulation,
};
use support::{arena, classes};

const PLAYER: u32 = 1;
const WARDEN: u8 = 0;
/// The camp's bandit nearest the pit (spawn 400, 8 m north-east of it).
const BANDIT: CreatureId = CreatureId::new(400);
/// The pit's centre; it spans 1.5 m either side along X and 1 m along Z.
const PIT: [i32; 2] = [-1_800, -8_600];

fn bandit() -> EntityRef {
    EntityRef::Creature(BANDIT)
}

/// One fight: the Warden at `warden` and the bandit at `bandit` (XZ), already face to face, and
/// any forced-move state the bandit carries in.
struct Fight {
    warden: [i32; 2],
    bandit: [i32; 2],
    forced: Option<ForcedState>,
}

/// One run: every projected event by tick, the commands and the final canonical state.
struct Outcome {
    events: Vec<(u64, ZoneEvent)>,
    commands: Vec<(u64, ZoneCommand)>,
    end: CanonicalZoneSnapshot,
}

/// A level 6 Warden in the hosted Greyhaven content, standing at `fight.warden` with the bandit
/// at `fight.bandit`, both on the Hollow's ground.
fn start(fight: &Fight) -> ZoneSimulation {
    let content = greyhaven_vale::content();
    let mut zone = ZoneSimulation::with_content(ZoneId::new(9), Arc::clone(&content)).unwrap();
    zone.add_player(PLAYER).unwrap();
    zone.apply_command(
        PLAYER,
        1,
        ZoneCommand::ChooseClass {
            class: WARDEN,
            sex: 0,
        },
    )
    .unwrap();
    zone.advance_tick().unwrap();
    let zone = classes::at_level(zone, PLAYER, 6);
    let terrain = greyhaven_vale::terrain();
    let ground = |at: [i32; 2]| terrain.height_at(at[0], at[1]);
    let mut state = zone.snapshot().unwrap();
    state.players[0].position = [
        fight.warden[0] as f32,
        (ground(fight.warden) + 95) as f32,
        fight.warden[1] as f32,
    ];
    state.players[0].combat.abilities.resource = 100;
    let record = state
        .creatures
        .iter_mut()
        .find(|creature| creature.creature_id == BANDIT)
        .unwrap();
    record.position = [
        fight.bandit[0] as f32,
        (ground(fight.bandit) + 95) as f32,
        fight.bandit[1] as f32,
    ];
    if let Some(forced) = fight.forced {
        state.forced.push((bandit(), forced));
        state.forced.sort_by_key(|&(unit, _)| unit);
    }
    ZoneSimulation::from_snapshot(state, content).unwrap()
}

/// Settles the fight a moment, then bashes the bandit and watches 3 s.
fn play(fight: &Fight) -> Outcome {
    let mut zone = start(fight);
    let mut commands = Vec::new();
    let mut events = Vec::new();
    let mut send = |zone: &mut ZoneSimulation, command| {
        commands.push((zone.current_tick(), command));
        zone.apply_command(PLAYER, commands.len() as u32 + 1, command)
            .unwrap();
    };
    send(&mut zone, ZoneCommand::SelectTarget(Some(bandit())));
    for tick in 0..mmorpg_core::ticks_from_ms(3_500) {
        if tick == 10 {
            let (facing, _) = arena::aim_at(&zone, PLAYER, bandit()).unwrap();
            send(&mut zone, arena::stand(facing));
            send(
                &mut zone,
                ZoneCommand::UseAbility {
                    ability: ids::SHIELD_BASH.get(),
                    target: Some(bandit()),
                    facing: Some(facing),
                },
            );
        }
        zone.advance_tick().unwrap();
        let view = zone.snapshot_for_player(PLAYER).unwrap();
        events.extend(view.events.iter().map(|&event| (view.tick, event)));
    }
    Outcome {
        events,
        commands,
        end: zone.snapshot().unwrap(),
    }
}

/// The ticks the camp's spikes hit the bandit.
fn spiked(outcome: &Outcome) -> Vec<u64> {
    outcome
        .events
        .iter()
        .filter_map(|&(tick, event)| match event {
            ZoneEvent::TrapHit { trap, target, .. }
                if trap == traps::CAMP_SPIKES && target == bandit() =>
            {
                Some(tick)
            }
            _ => None,
        })
        .collect()
}

fn bashed(outcome: &Outcome) -> bool {
    outcome.events.iter().any(|&(_, event)| {
        matches!(event, ZoneEvent::AbilityUsed { ability, .. } if ability == ids::SHIELD_BASH)
    })
}

fn bandit_at(outcome: &Outcome) -> [f32; 3] {
    outcome
        .end
        .creatures
        .iter()
        .find(|creature| creature.creature_id == BANDIT)
        .unwrap()
        .position
}

/// The same commands applied before the same ticks to a fresh zone.
fn replay(fight: &Fight, outcome: &Outcome) -> CanonicalZoneSnapshot {
    let mut zone = start(fight);
    let mut pending = outcome.commands.iter().zip(2..).peekable();
    for _ in 0..mmorpg_core::ticks_from_ms(3_500) {
        while let Some(&(&(tick, command), sequence)) = pending.peek()
            && tick == zone.current_tick()
        {
            zone.apply_command(PLAYER, sequence, command).unwrap();
            pending.next();
        }
        zone.advance_tick().unwrap();
    }
    zone.snapshot().unwrap()
}

/// The Warden 1.7 m east of the bandit, which stands 2.3 m east of the pit's edge: the bash
/// shoves it 3 m west, into the pit.
const SHOVE: Fight = Fight {
    warden: [-1_250, PIT[1]],
    bandit: [-1_420, PIT[1]],
    forced: None,
};

#[test]
fn a_shield_bash_shoves_a_bandit_into_the_spike_pit_and_only_then() {
    let pit = (PIT[0] - 150, PIT[0] + 150);
    let outcome = play(&SHOVE);
    assert!(bashed(&outcome));
    let hits = spiked(&outcome);
    // Stunned in the pit, the bandit takes the spikes on their own schedule: once per re-hit
    // window, never twice.
    assert!(hits.len() >= 3, "spiked while stunned: {hits:?}");
    assert!(
        hits.windows(2)
            .all(|pair| pair[1] - pair[0] >= u64::from(traps::SPIKE_REHIT_TICKS)),
        "{hits:?}"
    );
    assert!(outcome.events.iter().all(|&(_, event)| !matches!(
        event,
        ZoneEvent::TrapHit { target, amount, .. }
            if target == bandit() && amount != traps::SPIKE_DAMAGE
    )));

    // Exact replay: the same commands give the same zone.
    assert_eq!(replay(&SHOVE, &outcome), outcome.end);
    assert_eq!(spiked(&play(&SHOVE)), hits);

    // Too early: bashed 2 m before it was backed to the pit, it lands short.
    let short = play(&Fight {
        warden: [SHOVE.warden[0] + 200, PIT[1]],
        bandit: [SHOVE.bandit[0] + 200, PIT[1]],
        forced: None,
    });
    assert!(bashed(&short));
    assert!(spiked(&short).is_empty(), "a short shove misses the pit");
    // The wrong line: the Warden on the pit's side shoves it away from the pit.
    let away = play(&Fight {
        warden: [SHOVE.bandit[0] - 170, PIT[1]],
        bandit: SHOVE.bandit,
        forced: None,
    });
    assert!(bashed(&away));
    assert!(spiked(&away).is_empty(), "shoved away from the pit");
    // Still immune from a forced move: stunned where it stands, never moved.
    let immune = play(&Fight {
        forced: Some(ForcedState {
            moving: None,
            immune: 20,
        }),
        ..SHOVE
    });
    assert!(bashed(&immune));
    assert!(spiked(&immune).is_empty(), "an immune bandit stays put");
    assert!(bandit_at(&immune)[0] > pit.1 as f32);
    // A missed push: bashed while still out of reach, nothing happens.
    let missed = play(&Fight {
        warden: [SHOVE.warden[0] + 450, PIT[1]],
        ..SHOVE
    });
    assert!(!bashed(&missed), "out of reach, refused");
    assert!(spiked(&missed).is_empty());
}
