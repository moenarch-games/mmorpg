//! Co-op combat (#958): a Druid and a Ranger, two hosted players in one zone, take on the
//! Eastwater Fen wolf pack (#932). The Druid pulls the pack with Wrath; the Ranger takes the
//! follower with Aimed Shot, so it runs straight at the Ranger; the Druid roots the charging lead
//! wolf with Entangling Roots and walks off east, drawing the rest after it. The Ranger steps
//! sideways until the follower and the rooted wolf line up and looses one Piercing Shot that
//! strikes exactly those two, sparing the flanker off its line. Replayed without the root, the
//! same commands find the lead wolf chasing the Druid off the line and the shot strikes one. No
//! duo bonus: the root, the projectile, threat and the players' projections are the ordinary
//! zone rules, each player acting on their own projection.
mod support;

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::greyhaven_vale::{self, fen_pack};
use mmorpg_core::{
    CanonicalZoneSnapshot, CreatureId, EntityRef, ZoneCommand, ZoneEvent, ZoneId, ZoneSimulation,
    ZoneSnapshot,
};
use support::{arena, classes};

const DRUID: u32 = 1;
const RANGER: u32 = 2;
/// North of the pack, 22 m from the lead wolf, the Druid 1.5 m east of the Ranger.
const RANGER_START: [i32; 2] = [16_000, 8_400];
const DRUID_START: [i32; 2] = [16_150, 8_400];
/// The Druid roots the lead wolf once it has run this far from its post.
const CHARGED_UNITS: f64 = 100.0;
/// Yaw 16 384 faces +X.
const EAST: u16 = 16_384;
/// After rooting, the Druid walks east this long.
const DRUID_WALK_TICKS: u64 = mmorpg_core::ticks_from_ms(3_000) as u64;
/// The Ranger looses once the line through both wolves passes this close.
const LINED_UP_UNITS: f64 = 20.0;

fn creature(id: CreatureId) -> EntityRef {
    EntityRef::Creature(id)
}

/// Level 4 Druid and Ranger joined to the hosted Greyhaven content, at their starts.
fn duo() -> ZoneSimulation {
    let content = greyhaven_vale::content();
    let mut zone = ZoneSimulation::with_content(ZoneId::new(9), Arc::clone(&content)).unwrap();
    zone.add_player(DRUID).unwrap();
    zone.add_player(RANGER).unwrap();
    let zone = classes::at_level(classes::at_level(zone, DRUID, 4), RANGER, 4);
    let mut state = zone.snapshot().unwrap();
    for record in &mut state.players {
        let [x, z] = if record.player_id == DRUID {
            DRUID_START
        } else {
            RANGER_START
        };
        let ground = greyhaven_vale::terrain().height_at(x, z);
        record.position = [x as f32, (ground + 95) as f32, z as f32];
    }
    ZoneSimulation::from_snapshot(state, content).unwrap()
}

/// `(x, z)` of `unit` relative to `viewer` in the viewer's projection.
fn offset(view: &ZoneSnapshot, viewer: u32, unit: EntityRef) -> Option<[f64; 2]> {
    let at = |entity| {
        view.entities
            .iter()
            .find(|record| record.entity() == entity)
            .map(|record| record.position)
    };
    let (me, it) = (at(EntityRef::Player(viewer))?, at(unit)?);
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

/// The Ranger's command this tick: Piercing Shot at the nearer of the lead and the follower once
/// both line up and it is ready, else a sidestep toward their line facing the nearer one.
fn ranger_decides(view: &ZoneSnapshot) -> Option<ZoneCommand> {
    let lead = offset(view, RANGER, creature(fen_pack::LEAD))?;
    let follower = offset(view, RANGER, creature(fen_pack::FOLLOWER))?;
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

/// A command and the player who sent it before the tick it is keyed by.
type Sent = (u64, u32, ZoneCommand);

/// One run of the fight: every command sent, the Ranger's projected events by tick and the final
/// canonical state.
struct Fight {
    commands: Vec<Sent>,
    events: Vec<(u64, ZoneEvent)>,
    /// The lead wolf in the Ranger's projection, by tick.
    lead: Vec<(u64, [i32; 3])>,
    end: CanonicalZoneSnapshot,
}

const FIGHT_TICKS: u64 = mmorpg_core::ticks_from_ms(9_000) as u64;

/// The rooted fight, each player acting on their own projection.
fn play() -> Fight {
    let mut zone = duo();
    let mut commands: Vec<Sent> = Vec::new();
    let mut events = Vec::new();
    let mut lead = Vec::new();
    let mut sequences = [0_u32; 2];
    let mut send = |zone: &mut ZoneSimulation, player: u32, command| {
        commands.push((zone.current_tick(), player, command));
        let sequence = &mut sequences[player as usize - 1];
        *sequence += 1;
        zone.apply_command(player, *sequence, command).unwrap();
    };
    send(
        &mut zone,
        DRUID,
        ZoneCommand::ChooseClass { class: 6, sex: 1 },
    );
    send(
        &mut zone,
        RANGER,
        ZoneCommand::ChooseClass { class: 1, sex: 0 },
    );
    zone.advance_tick().unwrap();
    // The Druid pulls the lead wolf with Wrath.
    let wolf = creature(fen_pack::LEAD);
    let (facing, _) = arena::aim_at(&zone, DRUID, wolf).unwrap();
    send(&mut zone, DRUID, ZoneCommand::SelectTarget(Some(wolf)));
    send(
        &mut zone,
        DRUID,
        ZoneCommand::UseAbility {
            ability: ids::WRATH.get(),
            target: Some(wolf),
            facing: Some(facing),
        },
    );
    let post = offset(&zone.snapshot_for_player(DRUID).unwrap(), DRUID, wolf).unwrap();
    let (mut taken, mut rooted, mut loosed) = (false, false, false);
    let mut walk_from = u64::MAX - DRUID_WALK_TICKS;
    for _ in 0..FIGHT_TICKS {
        zone.advance_tick().unwrap();
        let view = zone.snapshot_for_player(RANGER).unwrap();
        events.extend(view.events.iter().map(|&event| (view.tick, event)));
        lead.extend(lead_in(&view));
        // Once the Wrath lands and the pack runs at the Druid, the Ranger takes the follower
        // with Aimed Shot, so it comes for the Ranger instead.
        let pulled = view.events.iter().any(|event| {
            matches!(event, ZoneEvent::DamageObserved { source: EntityRef::Player(DRUID), target, .. }
                if *target == wolf)
        });
        if pulled && !taken {
            taken = true;
            let follower = creature(fen_pack::FOLLOWER);
            let (facing, _) = arena::aim_at(&zone, RANGER, follower).unwrap();
            send(&mut zone, RANGER, ZoneCommand::SelectTarget(Some(follower)));
            send(
                &mut zone,
                RANGER,
                ZoneCommand::UseAbility {
                    ability: ids::AIMED_SHOT.get(),
                    target: None,
                    facing: Some(facing),
                },
            );
        }
        // The Druid roots the lead wolf once it charges.
        if !rooted {
            let seen = offset(&zone.snapshot_for_player(DRUID).unwrap(), DRUID, wolf).unwrap();
            if length([seen[0] - post[0], seen[1] - post[1]]) > CHARGED_UNITS {
                rooted = true;
                let (facing, _) = arena::aim_at(&zone, DRUID, wolf).unwrap();
                send(
                    &mut zone,
                    DRUID,
                    ZoneCommand::UseAbility {
                        ability: ids::ENTANGLING_ROOTS.get(),
                        target: Some(wolf),
                        facing: Some(facing),
                    },
                );
                // … and walks off east, taking whatever hunts it away from the Ranger.
                walk_from = zone.current_tick() + 1;
            }
            continue;
        }
        if zone.current_tick() == walk_from {
            send(&mut zone, DRUID, arena::walk(EAST));
        }
        if zone.current_tick() == walk_from + DRUID_WALK_TICKS {
            send(&mut zone, DRUID, arena::stand(EAST));
        }
        // The Ranger sets up and looses once.
        if loosed {
            continue;
        }
        let Some(command) = ranger_decides(&view) else {
            continue;
        };
        loosed = matches!(command, ZoneCommand::UseAbility { .. });
        send(&mut zone, RANGER, command);
    }
    assert!(rooted, "the lead wolf charged");
    assert!(loosed, "the wolves lined up before the fight closed in");
    Fight {
        commands,
        events,
        lead,
        end: zone.snapshot().unwrap(),
    }
}

/// The same commands applied before the same ticks to a fresh zone, for as many ticks as the
/// live fight.
fn replay(commands: &[Sent]) -> Fight {
    let mut zone = duo();
    let mut events = Vec::new();
    let mut lead = Vec::new();
    let mut sequences = [0_u32; 2];
    let mut pending = commands.iter().peekable();
    for _ in 0..=FIGHT_TICKS {
        while let Some(&&(tick, player, command)) = pending.peek()
            && tick == zone.current_tick()
        {
            let sequence = &mut sequences[player as usize - 1];
            *sequence += 1;
            zone.apply_command(player, *sequence, command).unwrap();
            pending.next();
        }
        zone.advance_tick().unwrap();
        let view = zone.snapshot_for_player(RANGER).unwrap();
        events.extend(view.events.iter().map(|&event| (view.tick, event)));
        lead.extend(lead_in(&view));
    }
    Fight {
        commands: commands.to_vec(),
        events,
        lead,
        end: zone.snapshot().unwrap(),
    }
}

impl Fight {
    /// The tick the Ranger's one Piercing Shot was loosed.
    fn loosed(&self) -> u64 {
        let shots: Vec<_> = self
            .events
            .iter()
            .filter_map(|&(tick, event)| match event {
                ZoneEvent::AbilityUsed {
                    source: EntityRef::Player(RANGER),
                    ability,
                    ..
                } if ability == ids::PIERCING_SHOT => Some(tick),
                _ => None,
            })
            .collect();
        assert_eq!(shots.len(), 1, "one Piercing Shot");
        shots[0]
    }

    /// The units the Ranger's damage struck after the Piercing Shot left, in order: the shot's
    /// hits, as the Ranger does nothing more.
    fn struck(&self) -> Vec<(EntityRef, u16)> {
        let loosed = self.loosed();
        self.events
            .iter()
            .filter_map(|&(tick, event)| match event {
                ZoneEvent::DamageDealt {
                    source: EntityRef::Player(RANGER),
                    target,
                    amount,
                    ..
                } if tick > loosed => Some((target, amount)),
                _ => None,
            })
            .collect()
    }

    /// `(x, z)` of the lead wolf in the Ranger's projection at `tick`.
    fn lead_at(&self, tick: u64) -> [f64; 2] {
        let &(_, [x, _, z]) = self.lead.iter().find(|&&(at, _)| at == tick).unwrap();
        [f64::from(x), f64::from(z)]
    }
}

/// The lead wolf's position in `view`, keyed by its tick.
fn lead_in(view: &ZoneSnapshot) -> Option<(u64, [i32; 3])> {
    view.entities
        .iter()
        .find(|record| record.entity() == creature(fen_pack::LEAD))
        .map(|record| (view.tick, record.position))
}

#[test]
fn the_druids_root_lines_up_two_wolves_for_the_rangers_piercing_shot() {
    let rooted = play();
    let loosed = rooted.loosed();
    let rooted_at = rooted.events.iter().find_map(|&(tick, event)| match event {
        ZoneEvent::AbilityUsed {
            source: EntityRef::Player(DRUID),
            target,
            ability,
        } if ability == ids::ENTANGLING_ROOTS => Some((tick, target)),
        _ => None,
    });
    let (rooted_at, held) = rooted_at.expect("the Druid rooted a wolf");
    assert_eq!(held, Some(creature(fen_pack::LEAD)));
    assert!(rooted_at < loosed, "rooted before the shot");
    // The root held the charging wolf where it stood until the shot.
    let held_at = rooted.lead_at(rooted_at + 1);
    assert!(
        (rooted_at + 1..=loosed).all(|tick| rooted.lead_at(tick) == held_at),
        "the rooted wolf stood still"
    );

    // Meanwhile the Ranger stepped aside to line the two wolves up.
    let sidesteps = rooted
        .commands
        .iter()
        .filter(|&&(tick, player, command)| {
            player == RANGER
                && tick > rooted_at
                && tick < loosed
                && matches!(command, ZoneCommand::Move { strafe, .. } if strafe != 0)
        })
        .count();
    assert!(sidesteps > 10, "sidestepped for {sidesteps} ticks");

    // The arrow struck the follower running at the Ranger and the rooted wolf behind it, once
    // each, the second weaker, and spared the flanker off its line: no phantom pierces.
    let struck = rooted.struck();
    let targets: Vec<_> = struck.iter().map(|&(target, _)| target).collect();
    assert_eq!(
        targets,
        [creature(fen_pack::FOLLOWER), creature(fen_pack::LEAD)],
        "{struck:?}"
    );
    assert!(struck[0].1 > struck[1].1, "the second hit is weaker");

    // The same fight without the root: the Druid's Wrath still pulls the pack and both players
    // make the very same moves, but the lead wolf chases the Druid off the line.
    let unrooted = replay(
        &rooted
            .commands
            .iter()
            .copied()
            .filter(|&(_, _, command)| {
                !matches!(command, ZoneCommand::UseAbility { ability, .. }
                    if ability == ids::ENTANGLING_ROOTS.get())
            })
            .collect::<Vec<_>>(),
    );
    assert_eq!(unrooted.loosed(), loosed, "the shot left on the same tick");
    let ran_to = unrooted.lead_at(loosed);
    assert!(
        length([held_at[0] - ran_to[0], held_at[1] - ran_to[1]]) > 200.0,
        "unrooted, the lead wolf moved on: {held_at:?} vs {ran_to:?}"
    );
    let spoiled = unrooted.struck();
    assert!(
        spoiled.len() < 2,
        "the spoiled shot strikes fewer: {spoiled:?}"
    );
    assert!(
        spoiled
            .iter()
            .all(|&(target, _)| target != creature(fen_pack::LEAD)
                && target != creature(fen_pack::FLANKER)),
        "{spoiled:?}"
    );

    // Both fights replay exactly from their commands.
    assert_eq!(replay(&rooted.commands).end, rooted.end);
    assert_eq!(replay(&unrooted.commands).end, unrooted.end);
}
