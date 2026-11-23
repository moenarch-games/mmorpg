//! A Ranger's firing line (#932) at the Eastwater Fen wolf pack: Concussive Shot slows the lead
//! wolf, the Ranger steps sideways until it and the wolf behind it line up, and one Piercing Shot
//! strikes exactly those two while the flanker, off the line, is spared. The authoritative
//! projectile decides the hits; the scripted player only aims and moves as a player would.
use crate::support;

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::greyhaven_vale::{self, fen_pack};
use mmorpg_core::{
    CreatureId, EntityRef, ZoneCommand, ZoneEvent, ZoneId, ZoneSimulation, ZoneSnapshot,
};
use support::{arena, classes};

const PLAYER: u32 = 1;
const RANGER: u8 = 1;
/// North of the pack, 22 m from the lead wolf.
const START: [i32; 2] = [16_000, 8_400];
/// The scripted player looses once the line through both wolves passes this close.
const LINED_UP_UNITS: f64 = 20.0;

fn creature(id: CreatureId) -> EntityRef {
    EntityRef::Creature(id)
}

/// A level 4 Ranger standing at [`START`] in the hosted Greyhaven content.
fn ranger() -> ZoneSimulation {
    let content = greyhaven_vale::content();
    let mut zone = ZoneSimulation::with_content(ZoneId::new(9), Arc::clone(&content)).unwrap();
    zone.add_player(PLAYER).unwrap();
    let zone = classes::at_level(zone, PLAYER, 4);
    let mut state = zone.snapshot().unwrap();
    let ground = greyhaven_vale::terrain().height_at(START[0], START[1]);
    state.players[0].position = [START[0] as f32, (ground + 95) as f32, START[1] as f32];
    ZoneSimulation::from_snapshot(state, content).unwrap()
}

/// `(x, z)` of `unit` relative to the viewer in its projection.
fn offset(view: &ZoneSnapshot, unit: EntityRef) -> Option<[f64; 2]> {
    let at = |entity| {
        view.entities
            .iter()
            .find(|record| record.entity() == entity)
            .map(|record| record.position)
    };
    let (me, it) = (at(EntityRef::Player(PLAYER))?, at(unit)?);
    Some([f64::from(it[0] - me[0]), f64::from(it[2] - me[2])])
}

fn length(v: [f64; 2]) -> f64 {
    v[0].hypot(v[1])
}

/// The foot of the perpendicular from the viewer (the origin) to the line through `near` and
/// `far`.
fn foot(near: [f64; 2], far: [f64; 2]) -> [f64; 2] {
    let line = [far[0] - near[0], far[1] - near[1]];
    let unit = [line[0] / length(line), line[1] / length(line)];
    let along = -(near[0] * unit[0] + near[1] * unit[1]);
    [near[0] + along * unit[0], near[1] + along * unit[1]]
}

/// The player's command this tick: Piercing Shot at the nearer of the lead and the follower once
/// both line up and it is ready, else a sidestep toward their line facing the nearer one.
fn decide(view: &ZoneSnapshot) -> Option<ZoneCommand> {
    let lead = offset(view, creature(fen_pack::LEAD))?;
    let follower = offset(view, creature(fen_pack::FOLLOWER))?;
    let (near, far) = if length(lead) <= length(follower) {
        (lead, follower)
    } else {
        (follower, lead)
    };
    let facing = mmorpg_core::trig::yaw_from_vector(near[0] as i32, near[1] as i32)?;
    let foot = foot(near, far);
    let ready = view.viewer.global_cooldown == 0
        && !view
            .cooldowns
            .iter()
            .any(|cooldown| cooldown.ability == ids::PIERCING_SHOT);
    if ready && length(foot) < LINED_UP_UNITS {
        return Some(ZoneCommand::UseAbility {
            ability: ids::PIERCING_SHOT.get(),
            target: None,
            facing: Some(facing),
        });
    }
    // Positive strafe is the character's right: (−z, x) of the facing.
    let right = [-near[1], near[0]];
    let side = foot[0] * right[0] + foot[1] * right[1];
    let strafe = if length(foot) < LINED_UP_UNITS / 2.0 {
        0
    } else if side > 0.0 {
        1
    } else {
        -1
    };
    Some(ZoneCommand::Move {
        forward: 0,
        strafe,
        facing,
    })
}

/// One run of the journey: the commands the player sent with the tick each was sent before,
/// the projected events by tick, and the final canonical state.
struct Journey {
    commands: Vec<(u64, ZoneCommand)>,
    events: Vec<(u64, ZoneEvent)>,
    end: mmorpg_core::CanonicalZoneSnapshot,
}

fn play() -> Journey {
    let mut zone = ranger();
    let mut commands = Vec::new();
    let mut events = Vec::new();
    let mut send = |zone: &mut ZoneSimulation, command| {
        commands.push((zone.current_tick(), command));
        zone.apply_command(PLAYER, commands.len() as u32, command)
            .unwrap();
    };
    send(
        &mut zone,
        ZoneCommand::ChooseClass {
            class: RANGER,
            sex: 0,
        },
    );
    zone.advance_tick().unwrap();
    // Select the lead wolf and slow it as it is pulled.
    let lead = creature(fen_pack::LEAD);
    let (facing, _) = arena::aim_at(&zone, PLAYER, lead).unwrap();
    send(&mut zone, ZoneCommand::SelectTarget(Some(lead)));
    send(
        &mut zone,
        ZoneCommand::UseAbility {
            ability: ids::CONCUSSIVE_SHOT.get(),
            target: None,
            facing: Some(facing),
        },
    );
    let mut loosed = false;
    for _ in 0..mmorpg_core::ticks_from_ms(6_000) {
        zone.advance_tick().unwrap();
        let view = zone.snapshot_for_player(PLAYER).unwrap();
        events.extend(view.events.iter().map(|&event| (view.tick, event)));
        if loosed {
            continue;
        }
        let Some(command) = decide(&view) else {
            continue;
        };
        loosed = matches!(command, ZoneCommand::UseAbility { .. });
        send(&mut zone, command);
    }
    assert!(loosed, "the wolves lined up before the fight closed in");
    Journey {
        commands,
        events,
        end: zone.snapshot().unwrap(),
    }
}

/// The same commands applied before the same ticks to a fresh zone.
fn replay(commands: &[(u64, ZoneCommand)], ticks: u64) -> mmorpg_core::CanonicalZoneSnapshot {
    let mut zone = ranger();
    let mut pending = commands.iter().zip(1..).peekable();
    for _ in 0..ticks {
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

#[test]
fn concussive_then_a_sidestep_lines_up_two_wolves_for_one_piercing_shot() {
    let journey = play();
    let used = |ability: mmorpg_core::AbilityId| {
        journey
            .events
            .iter()
            .find_map(|&(tick, event)| match event {
                ZoneEvent::AbilityUsed {
                    source: EntityRef::Player(PLAYER),
                    ability: used,
                    ..
                } if used == ability => Some(tick),
                _ => None,
            })
    };
    let slowed = journey
        .events
        .iter()
        .find_map(|&(tick, event)| match event {
            ZoneEvent::AuraApplied {
                target, ability, ..
            } if ability == ids::CONCUSSIVE_SHOT => Some((tick, target)),
            _ => None,
        });
    let pierced = used(ids::PIERCING_SHOT).unwrap();
    let (slowed_at, slowed) = slowed.unwrap();
    assert_eq!(slowed, creature(fen_pack::LEAD));
    assert!(used(ids::CONCUSSIVE_SHOT).unwrap() < slowed_at && slowed_at < pierced);

    // The Ranger stepped aside between the two shots.
    let sidesteps = journey
        .commands
        .iter()
        .filter(|(tick, command)| {
            *tick < pierced && matches!(command, ZoneCommand::Move { strafe, .. } if *strafe != 0)
        })
        .count();
    assert!(sidesteps > 30, "sidestepped for {sidesteps} ticks");

    // The piercing arrow struck the two wolves it passed through, nearer first, and spared the
    // flanker off its line.
    let struck: Vec<_> = journey
        .events
        .iter()
        .filter_map(|&(tick, event)| match event {
            ZoneEvent::DamageDealt {
                source: EntityRef::Player(PLAYER),
                target,
                amount,
                ..
            } if tick > pierced => Some((target, amount)),
            _ => None,
        })
        .collect();
    let targets: Vec<_> = struck.iter().map(|&(target, _)| target).collect();
    assert_eq!(
        targets,
        [creature(fen_pack::LEAD), creature(fen_pack::FOLLOWER)],
        "{struck:?}"
    );
    assert!(struck[0].1 > struck[1].1, "the second hit is weaker");
    let flanker_health = |state: &mmorpg_core::CanonicalZoneSnapshot| {
        state
            .creatures
            .iter()
            .find(|record| record.creature_id == fen_pack::FLANKER)
            .map(|record| record.health)
    };
    assert_eq!(
        flanker_health(&journey.end),
        flanker_health(&ranger().snapshot().unwrap()),
        "the flanker was spared"
    );

    // The same commands replay to the same canonical state.
    let ticks = journey.end.tick - ranger().current_tick();
    assert_eq!(replay(&journey.commands, ticks), journey.end);
}
