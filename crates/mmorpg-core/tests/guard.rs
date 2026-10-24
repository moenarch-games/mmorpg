//! The Warden's directional shield guard (#156): the zone blocks creature
//! melee in the held lane within the shield's coverage, heavy charging blows
//! break the guard for a bounded recovery after which the same held lane
//! protects again, and a protecting guard slows movement.
mod support;

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::{
    CreatureBehaviour, CreatureTemplate, CreatureTemplateId, EntityRef, GUARD_BREAK_TICKS,
    StrikeAim, StrikeHeight, StrikeSide, ZoneAreas, ZoneCommand, ZoneContent, ZoneEvent, ZoneId,
    ZoneSimulation,
};
use support::arena::{self, GRAVEYARD};

const NORTH: u16 = 0;
const SOUTH: u16 = 32_768;
/// A wolf-sized biter: its contact point is in the middle lane.
const WOLF: CreatureTemplateId = CreatureTemplateId::new(1);
/// A tall, heavy brute: high lane, and its charge overpowers a guard.
const OGRE: CreatureTemplateId = CreatureTemplateId::new(2);
const LOW: StrikeHeight = StrikeHeight::Low;
const MIDDLE: StrikeHeight = StrikeHeight::Middle;
const HIGH: StrikeHeight = StrikeHeight::High;

fn template(id: CreatureTemplateId, half_extents: [i32; 3]) -> CreatureTemplate {
    CreatureTemplate {
        id,
        behaviour: CreatureBehaviour::Aggressive,
        swing_ticks: 10,
        half_extents,
        ..arena::wolf(500, [2, 3])
    }
}

/// Flat ground and aggressive creatures as `(id, template, x, z)`; the
/// Warden spawns at the origin.
fn content(creatures: &[(u32, CreatureTemplateId, i32, i32)]) -> Arc<ZoneContent> {
    let spawns = creatures
        .iter()
        .map(|&(id, template, x, z)| arena::spawn(id, template, [x, z]))
        .collect();
    let content = ZoneContent::new(
        arena::ground(),
        ZoneAreas::default(),
        vec![template(WOLF, [40, 45, 40]), template(OGRE, [60, 100, 60])],
        spawns,
        Vec::new(),
        GRAVEYARD,
    )
    .unwrap()
    .with_creature_abilities(Default::default())
    .unwrap();
    Arc::new(content)
}

struct Warden {
    zone: ZoneSimulation,
    sequence: u32,
}

impl Warden {
    /// A level-1 Warden at the origin facing `facing`, guarding `lane` from
    /// the first tick.
    fn new(content: Arc<ZoneContent>, facing: u16, lane: Option<StrikeHeight>) -> Self {
        Self::with_class(content, 0, facing, lane)
    }

    fn with_class(
        content: Arc<ZoneContent>,
        class: u8,
        facing: u16,
        lane: Option<StrikeHeight>,
    ) -> Self {
        let mut zone = ZoneSimulation::with_content(ZoneId::new(9), content).unwrap();
        zone.add_player(1).unwrap();
        let mut warden = Self { zone, sequence: 0 };
        warden.send(ZoneCommand::ChooseClass { class, sex: 0 });
        warden.send(arena::stand(facing));
        warden.send(ZoneCommand::Guard { lane });
        warden
    }

    fn send(&mut self, command: ZoneCommand) {
        self.sequence += 1;
        self.zone.apply_command(1, self.sequence, command).unwrap();
    }

    fn tick(&mut self) -> Vec<ZoneEvent> {
        self.zone.advance_tick().unwrap();
        self.zone.snapshot_for_player(1).unwrap().events
    }

    fn run(&mut self, ticks: u32) -> Tally {
        let mut tally = Tally::default();
        for _ in 0..ticks {
            tally.add(&self.tick());
        }
        tally
    }

    fn guard(&self) -> mmorpg_core::GuardView {
        self.zone.snapshot_for_player(1).unwrap().viewer.guard
    }

    fn health(&self) -> u32 {
        self.zone.snapshot_for_player(1).unwrap().viewer.health
    }
}

/// Outcomes of creature blows on the Warden.
#[derive(Debug, Default)]
struct Tally {
    blocked: Vec<StrikeHeight>,
    hits: u32,
    broken: Vec<StrikeHeight>,
}

impl Tally {
    fn add(&mut self, events: &[ZoneEvent]) {
        for event in events {
            match *event {
                ZoneEvent::Blocked {
                    target: EntityRef::Player(1),
                    lane,
                    ..
                } => self.blocked.push(lane),
                ZoneEvent::DamageTaken { amount, .. } if amount > 0 => self.hits += 1,
                ZoneEvent::GuardBroken {
                    target: EntityRef::Player(1),
                    lane,
                    ticks,
                    ..
                } => {
                    assert_eq!(ticks, GUARD_BREAK_TICKS);
                    self.broken.push(lane);
                }
                _ => {}
            }
        }
    }
}

#[test]
fn a_guard_in_the_blows_lane_blocks_it_and_a_wrong_lane_does_not() {
    let mut warden = Warden::new(content(&[(1, WOLF, 0, -150)]), SOUTH, Some(MIDDLE));
    let health = warden.health();
    let tally = warden.run(60);
    assert!(tally.blocked.len() >= 3, "{tally:?}");
    assert!(tally.blocked.iter().all(|lane| *lane == MIDDLE));
    assert_eq!((tally.hits, warden.health()), (0, health));
    assert_eq!(tally.broken, []);
    assert!(warden.guard().protecting);

    // A high guard passes over the wolf's bite.
    warden.send(ZoneCommand::Guard { lane: Some(HIGH) });
    let tally = warden.run(40);
    assert!(tally.blocked.is_empty() && tally.hits >= 2, "{tally:?}");
    assert!(warden.health() < health);

    // A low guard catches a small creature's bite only; here it fails too,
    // and a released guard blocks nothing.
    for lane in [Some(LOW), None] {
        warden.send(ZoneCommand::Guard { lane });
        let tally = warden.run(30);
        assert!(
            tally.blocked.is_empty() && tally.hits >= 1,
            "{lane:?}: {tally:?}"
        );
    }
}

#[test]
fn the_shield_covers_the_front_and_shield_flank_but_not_the_sword_flank_or_rear() {
    // Facing south (−Z), the shield (left) side is −X and the sword side
    // +X; spawn slots fill +X/+Z, so the sword-flank and rear wolves stand
    // off-axis.
    for (x, z, covered) in [
        (0, -150, true),
        (-150, 0, true),
        (150, -100, false),
        (-100, 160, false),
    ] {
        let mut warden = Warden::new(content(&[(1, WOLF, x, z)]), SOUTH, Some(MIDDLE));
        let tally = warden.run(50);
        if covered {
            assert!(
                tally.blocked.len() >= 3 && tally.hits == 0,
                "({x}, {z}): {tally:?}"
            );
        } else {
            assert!(
                tally.blocked.is_empty() && tally.hits >= 3,
                "({x}, {z}): {tally:?}"
            );
        }
    }
    // Turning to face the wolf behind brings it into coverage.
    let mut warden = Warden::new(content(&[(1, WOLF, -100, 160)]), SOUTH, Some(MIDDLE));
    warden.run(20);
    warden.send(arena::stand(NORTH));
    let tally = warden.run(40);
    assert!(tally.blocked.len() >= 3 && tally.hits == 0, "{tally:?}");
}

#[test]
fn a_charging_brute_breaks_the_guard_which_returns_while_still_held() {
    let run = |release: bool| {
        // The ogre charges from 7 m; its contact point is in the high lane.
        let mut warden = Warden::new(content(&[(1, OGRE, 0, -700)]), SOUTH, Some(HIGH));
        let mut tally = Tally::default();
        while tally.broken.is_empty() {
            tally.add(&warden.tick());
            assert!(warden.zone.current_tick() < 120, "the charge arrived");
        }
        assert_eq!(tally.broken, [HIGH]);
        assert!(tally.blocked.is_empty());
        assert_eq!(tally.hits, 1, "the overpowering blow lands");
        let guard = warden.guard();
        assert_eq!(
            (guard.raised, guard.protecting, guard.broken),
            (Some(HIGH), false, GUARD_BREAK_TICKS)
        );
        if release {
            warden.send(ZoneCommand::Guard { lane: None });
        }
        // Recovery: every blow lands.
        let recovery = warden.run(u32::from(GUARD_BREAK_TICKS) - 1);
        assert!(
            recovery.blocked.is_empty() && recovery.hits >= 3,
            "{recovery:?}"
        );
        // Recovered: the same held lane protects again without a new command;
        // a released guard stays down.
        let after = warden.run(40);
        (warden.guard(), after)
    };
    let (guard, held) = run(false);
    assert_eq!(
        (guard.raised, guard.protecting, guard.broken),
        (Some(HIGH), true, 0)
    );
    assert!(held.blocked.len() >= 3 && held.hits == 0, "{held:?}");
    let (guard, released) = run(true);
    assert_eq!((guard.raised, guard.protecting), (None, false));
    assert!(
        released.blocked.is_empty() && released.hits >= 3,
        "{released:?}"
    );
}

#[test]
fn guarding_slows_movement_and_a_jump_suspends_protection() {
    let distance = |lane: Option<StrikeHeight>| {
        let mut warden = Warden::new(content(&[]), NORTH, lane);
        warden.run(5);
        let start =
            mmorpg_core::physics::to_units(warden.zone.snapshot().unwrap().players[0].position)[2];
        warden.send(arena::walk(NORTH));
        warden.run(30);
        mmorpg_core::physics::to_units(warden.zone.snapshot().unwrap().players[0].position)[2]
            - start
    };
    let (free, guarded) = (distance(None), distance(Some(MIDDLE)));
    assert!(guarded > 0 && guarded < free, "{guarded} vs {free}");
    assert!(
        (50..=60).contains(&(guarded * 100 / free)),
        "{guarded} vs {free}"
    );

    let mut warden = Warden::new(content(&[]), NORTH, Some(LOW));
    warden.run(5);
    assert!(warden.guard().protecting);
    warden.send(arena::walk(NORTH));
    warden.send(ZoneCommand::Jump);
    warden.run(3);
    let guard = warden.guard();
    assert_eq!((guard.raised, guard.protecting), (Some(LOW), false));
    let velocity =
        mmorpg_core::physics::to_units(warden.zone.snapshot().unwrap().players[0].velocity);
    assert_eq!(
        velocity[2],
        mmorpg_core::RUN_SPEED_SUBUNITS_PER_TICK / mmorpg_core::physics::SUBUNITS_PER_UNIT,
        "no slowdown in the air"
    );
    // Landing restores protection in the held lane.
    warden.run(60);
    assert!(warden.guard().protecting);
}

#[test]
fn a_directed_strike_lowers_the_guard_and_other_classes_never_guard() {
    let mut warden = Warden::new(content(&[]), SOUTH, Some(MIDDLE));
    warden.run(5);
    let mut state = warden.zone.snapshot().unwrap();
    state.players[0].combat.abilities.resource = 100;
    warden.zone = ZoneSimulation::from_snapshot(state, Arc::clone(warden.zone.content())).unwrap();
    warden.send(ZoneCommand::DirectedStrike {
        ability: ids::HEROIC_STRIKE.get(),
        facing: SOUTH,
        aim: StrikeAim::new(StrikeSide::Centre, MIDDLE),
    });
    warden.run(1);
    assert_eq!(warden.guard().raised, None);

    let mut ranger = Warden::with_class(content(&[(1, WOLF, 0, -150)]), 1, SOUTH, Some(MIDDLE));
    let tally = ranger.run(40);
    assert!(tally.blocked.is_empty() && tally.hits >= 2, "{tally:?}");
}

#[test]
fn guard_outcomes_replay_identically_from_a_checkpoint_and_stale_commands_fail_closed() {
    let run = |checkpoint: bool| {
        let content = content(&[(1, OGRE, 0, -700)]);
        let mut warden = Warden::new(Arc::clone(&content), SOUTH, Some(HIGH));
        let mut events = Vec::new();
        for tick in 0..120 {
            if checkpoint && tick == 40 {
                let state = warden.zone.snapshot().unwrap();
                warden.zone = ZoneSimulation::from_snapshot(state, Arc::clone(&content)).unwrap();
            }
            if tick == 90 {
                warden.send(ZoneCommand::Guard { lane: Some(MIDDLE) });
            }
            events.extend(warden.tick());
        }
        (events, warden.zone.snapshot().unwrap())
    };
    let (events, state) = run(false);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, ZoneEvent::GuardBroken { .. }))
    );
    assert_eq!(run(true), (events, state.clone()));

    // A stale or duplicate guard command changes nothing.
    let mut zone =
        ZoneSimulation::from_snapshot(state.clone(), content(&[(1, OGRE, 0, -700)])).unwrap();
    let last = state.players[0].last_sequence;
    for sequence in [last, last - 1, 0] {
        assert!(
            zone.apply_command(1, sequence, ZoneCommand::Guard { lane: Some(LOW) })
                .is_err()
        );
    }
    assert_eq!(zone.snapshot().unwrap(), state);
}
