//! Directed Heroic Strike (#154): the zone resolves the sword sweep's
//! physical coverage from the aimed side and height, reach and static
//! geometry, while sharing Heroic Strike's Rage, unlock and cooldowns.
mod support;

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::{
    ABILITY_CATALOG_REVISION, CreatureBehaviour, CreatureId, CreatureTemplate, CreatureTemplateId,
    EntityRef, ErrorCode, GLOBAL_COOLDOWN_TICKS, StaticCollider, StrikeAim, StrikeHeight,
    StrikeSide, ZoneAreas, ZoneCommand, ZoneContent, ZoneDefinition, ZoneEvent, ZoneId,
    ZoneSimulation,
};
use support::arena::{self, GRAVEYARD};

const PLAYER: EntityRef = EntityRef::Player(1);
/// Spawn slots fill +X/+Z from the origin, so creatures stand to the south.
const SOUTH: u16 = 32_768;
const WEST: u16 = 49_152;
const WOLF: CreatureTemplateId = CreatureTemplateId::new(1);
const RAT: CreatureTemplateId = CreatureTemplateId::new(2);
const BANDIT: CreatureTemplateId = CreatureTemplateId::new(3);
const LOW: StrikeHeight = StrikeHeight::Low;
const MIDDLE: StrikeHeight = StrikeHeight::Middle;
const HIGH: StrikeHeight = StrikeHeight::High;

/// A neutral, sturdy creature that stands still: 90 units tall like a wolf.
fn template(id: CreatureTemplateId, half_extents: [i32; 3]) -> CreatureTemplate {
    CreatureTemplate {
        id,
        behaviour: CreatureBehaviour::Neutral,
        half_extents,
        ..arena::wolf(500, [1, 1])
    }
}

fn templates() -> Vec<CreatureTemplate> {
    vec![
        template(WOLF, [40, 45, 40]),
        template(RAT, [25, 20, 25]),
        template(BANDIT, [30, 90, 30]),
    ]
}

/// Flat ground, optional extra colliders, and creatures as `(id, template, x, z)`.
fn content(
    colliders: Vec<StaticCollider>,
    creatures: &[(u32, CreatureTemplateId, i32, i32)],
) -> Arc<ZoneContent> {
    let mut definition_colliders = vec![StaticCollider {
        id: 1,
        position: [0, -50, 0],
        half_extents: [10_000, 50, 10_000],
    }];
    definition_colliders.extend(colliders);
    let definition = ZoneDefinition::with_spawn_grid(
        77,
        [0, -1, 0],
        mmorpg_core::SpawnGrid {
            origin: [0, 0],
            columns: 32,
            spacing: 100,
        },
        definition_colliders,
    )
    .unwrap();
    let spawns = creatures
        .iter()
        .map(|&(id, template, x, z)| arena::spawn(id, template, [x, z]))
        .collect();
    let content = ZoneContent::new(
        definition,
        ZoneAreas::default(),
        templates(),
        spawns,
        Vec::new(),
        GRAVEYARD,
    )
    .unwrap()
    .with_creature_abilities(ABILITY_CATALOG_REVISION, Default::default())
    .unwrap();
    Arc::new(content)
}

struct Warden {
    zone: ZoneSimulation,
    sequence: u32,
}

impl Warden {
    /// A level-1 Warden (or another class) at the origin with full Rage.
    fn new(content: Arc<ZoneContent>, class: u8) -> Self {
        let mut zone = ZoneSimulation::with_content(ZoneId::new(9), Arc::clone(&content)).unwrap();
        zone.add_player(1).unwrap();
        let mut warden = Self { zone, sequence: 0 };
        warden.send(ZoneCommand::ChooseClass { class, sex: 0 });
        // Settle onto the ground.
        for _ in 0..20 {
            warden.tick();
        }
        let mut state = warden.zone.snapshot().unwrap();
        state.players[0].combat.abilities.resource = 100;
        warden.zone = ZoneSimulation::from_snapshot(state, content).unwrap();
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

    fn strike(&mut self, facing: u16, side: StrikeSide, height: StrikeHeight) -> Vec<ZoneEvent> {
        self.send(directed(facing, side, height));
        self.tick()
    }

    /// Strikes after any running global cooldown; returns the creature hit, if any.
    fn swing(&mut self, facing: u16, side: StrikeSide, height: StrikeHeight) -> Option<CreatureId> {
        for _ in 0..GLOBAL_COOLDOWN_TICKS {
            self.tick();
        }
        let events = self.strike(facing, side, height);
        swept(&events).expect("the strike was carried out")
    }

    fn rage(&self) -> u16 {
        self.zone
            .snapshot_for_player(1)
            .unwrap()
            .viewer
            .resource
            .unwrap()
            .value
    }
}

fn directed(facing: u16, side: StrikeSide, height: StrikeHeight) -> ZoneCommand {
    ZoneCommand::DirectedStrike {
        ability: ids::HEROIC_STRIKE.get(),
        facing,
        aim: StrikeAim::new(side, height),
    }
}

/// The creature a `StrikeSwept` event struck: `Some(None)` for a whiff,
/// `None` without a sweep.
fn swept(events: &[ZoneEvent]) -> Option<Option<CreatureId>> {
    events.iter().find_map(|event| match *event {
        ZoneEvent::StrikeSwept {
            source: PLAYER,
            target,
            ability,
            ..
        } => {
            assert_eq!(ability, ids::HEROIC_STRIKE);
            Some(target.map(|target| match target {
                EntityRef::Creature(id) => id,
                other => panic!("a sweep struck {other:?}"),
            }))
        }
        _ => None,
    })
}

fn damage_to(events: &[ZoneEvent], creature: u32) -> bool {
    events.iter().any(|event| {
        matches!(event, ZoneEvent::DamageDealt { target, amount, .. }
            if *target == EntityRef::Creature(CreatureId::new(creature)) && *amount > 0)
    })
}

fn errors(events: &[ZoneEvent]) -> Vec<ErrorCode> {
    events
        .iter()
        .filter_map(|event| match event {
            ZoneEvent::Error { code, .. } => Some(*code),
            _ => None,
        })
        .collect()
}

#[test]
fn height_decides_whether_the_sweep_reaches_a_short_creature() {
    let mut warden = Warden::new(content(Vec::new(), &[(1, WOLF, 0, -150)]), 0);
    let rage = warden.rage();
    let events = warden.strike(SOUTH, StrikeSide::Centre, HIGH);
    assert_eq!(
        swept(&events),
        Some(None),
        "a high slash passes over a wolf"
    );
    assert!(!damage_to(&events, 1));
    assert_eq!(
        warden.rage(),
        rage - 15,
        "the swing costs Heroic Strike's Rage"
    );
    // The shared global cooldown refuses the next swing of either mode.
    let events = warden.strike(SOUTH, StrikeSide::Centre, LOW);
    assert_eq!(errors(&events), [ErrorCode::NotReady]);
    warden.send(ZoneCommand::UseAbility {
        ability: ids::HEROIC_STRIKE.get(),
        target: Some(EntityRef::Creature(CreatureId::new(1))),
    });
    assert_eq!(errors(&warden.tick()), [ErrorCode::NotReady]);

    for height in [LOW, MIDDLE] {
        for _ in 0..GLOBAL_COOLDOWN_TICKS {
            warden.tick();
        }
        let events = warden.strike(SOUTH, StrikeSide::Centre, height);
        assert_eq!(swept(&events), Some(Some(CreatureId::new(1))), "{height:?}");
        assert!(damage_to(&events, 1), "{height:?}");
    }
    assert_eq!(warden.swing(SOUTH, StrikeSide::Centre, HIGH), None);
}

#[test]
fn side_and_height_pick_different_targets_around_the_warden() {
    // A rat ahead-left (−X is the Warden's left when facing −Z) and a
    // bandit ahead-right; both within reach.
    let mut warden = Warden::new(
        content(Vec::new(), &[(1, RAT, -120, -120), (2, BANDIT, 120, -120)]),
        0,
    );
    let rat = Some(CreatureId::new(1));
    let bandit = Some(CreatureId::new(2));
    assert_eq!(warden.swing(SOUTH, StrikeSide::Left, LOW), rat);
    assert_eq!(
        warden.swing(SOUTH, StrikeSide::Left, MIDDLE),
        None,
        "over the rat"
    );
    assert_eq!(warden.swing(SOUTH, StrikeSide::Left, HIGH), None);
    assert_eq!(warden.swing(SOUTH, StrikeSide::Right, HIGH), bandit);
    assert_eq!(warden.swing(SOUTH, StrikeSide::Right, LOW), bandit);
    // Both lie 45° off the facing, outside the centre arc's 30°.
    assert_eq!(warden.swing(SOUTH, StrikeSide::Centre, MIDDLE), None);
}

#[test]
fn reach_and_static_geometry_bound_the_sweep() {
    // Beyond melee reach.
    let mut far = Warden::new(content(Vec::new(), &[(1, WOLF, 0, -300)]), 0);
    assert_eq!(far.swing(SOUTH, StrikeSide::Centre, LOW), None);
    // A wall between the Warden and a wolf to the west blocks the sweep;
    // without it the same aim strikes.
    let wall = StaticCollider {
        id: 2,
        position: [-110, 100, 0],
        half_extents: [10, 100, 100],
    };
    let mut blocked = Warden::new(content(vec![wall], &[(1, WOLF, -200, 0)]), 0);
    assert_eq!(blocked.swing(WEST, StrikeSide::Centre, LOW), None);
    let mut open = Warden::new(content(Vec::new(), &[(1, WOLF, -200, 0)]), 0);
    assert_eq!(
        open.swing(WEST, StrikeSide::Centre, LOW),
        Some(CreatureId::new(1))
    );
}

#[test]
fn the_nearest_covered_creature_is_struck_and_ties_go_to_the_lowest_id() {
    let mut nearest = Warden::new(
        content(Vec::new(), &[(1, WOLF, 0, -220), (2, WOLF, 0, -120)]),
        0,
    );
    assert_eq!(
        nearest.swing(SOUTH, StrikeSide::Centre, LOW),
        Some(CreatureId::new(2))
    );
    let mut tied = Warden::new(
        content(Vec::new(), &[(2, WOLF, 50, -150), (1, WOLF, -50, -150)]),
        0,
    );
    assert_eq!(
        tied.swing(SOUTH, StrikeSide::Centre, LOW),
        Some(CreatureId::new(1))
    );
}

#[test]
fn only_a_learned_heroic_strike_has_a_directed_mode() {
    let wolf = || content(Vec::new(), &[(1, WOLF, 0, -150)]);
    let mut ranger = Warden::new(wolf(), 1);
    assert_eq!(
        errors(&ranger.strike(SOUTH, StrikeSide::Centre, LOW)),
        [ErrorCode::NotLearned]
    );
    let mut warden = Warden::new(wolf(), 0);
    warden.send(ZoneCommand::DirectedStrike {
        ability: ids::CLEAVE.get(),
        facing: SOUTH,
        aim: StrikeAim::new(StrikeSide::Centre, LOW),
    });
    assert_eq!(errors(&warden.tick()), [ErrorCode::NotLearned]);
    let mut poor = warden.zone.snapshot().unwrap();
    poor.players[0].combat.abilities.resource = 10;
    let mut poor = Warden {
        zone: ZoneSimulation::from_snapshot(poor, Arc::clone(warden.zone.content())).unwrap(),
        sequence: warden.sequence,
    };
    assert_eq!(
        errors(&poor.strike(SOUTH, StrikeSide::Centre, LOW)),
        [ErrorCode::NotEnoughResource]
    );
}

#[test]
fn identical_commands_and_a_checkpoint_replay_identically() {
    let run = |checkpoint: bool| {
        let content = content(Vec::new(), &[(1, RAT, -120, -120), (2, BANDIT, 120, -120)]);
        let mut warden = Warden::new(Arc::clone(&content), 0);
        warden.send(directed(SOUTH, StrikeSide::Right, HIGH));
        if checkpoint {
            // The pending directed intent survives recovery.
            let state = warden.zone.snapshot().unwrap();
            warden.zone = ZoneSimulation::from_snapshot(state, content).unwrap();
        }
        let mut events = warden.tick();
        events.extend((0..GLOBAL_COOLDOWN_TICKS).flat_map(|_| warden.tick()));
        warden.send(directed(SOUTH, StrikeSide::Left, LOW));
        events.extend(warden.tick());
        (events, warden.zone.snapshot().unwrap())
    };
    let (events, state) = run(false);
    assert_eq!(run(false), (events.clone(), state.clone()));
    assert_eq!(run(true), (events.clone(), state));
    assert!(damage_to(&events, 1) && damage_to(&events, 2));
}
