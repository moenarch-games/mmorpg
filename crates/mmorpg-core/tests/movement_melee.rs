//! Movement-dependent Warden melee (#155, Atlas 5.2.69–5.2.70): the same
//! swing deals more charging or airborne and less while moving away, a
//! planted Warden takes less creature melee damage, and melee requested
//! during a landing waits for the recovery to end.
mod support;

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::{
    ABILITY_CATALOG_REVISION, CreatureBehaviour, CreatureId, CreatureTemplate, EntityRef,
    ErrorCode, MeleeStance, ZoneAreas, ZoneCommand, ZoneContent, ZoneEvent, ZoneId, ZoneSimulation,
};
use support::arena::{self, GRAVEYARD, WOLF};

/// The creature stands south of the spawn grid, which grows north.
const SOUTH: u16 = 32_768;
const TARGET: EntityRef = EntityRef::Creature(CreatureId::new(1));

/// A creature 1.5 m south of the Warden's spawn at the origin.
fn content(behaviour: CreatureBehaviour, damage: [u16; 2]) -> Arc<ZoneContent> {
    let template = CreatureTemplate {
        behaviour,
        swing_ticks: mmorpg_core::golden::t30(20) as _,
        ..arena::wolf(5_000, damage)
    };
    Arc::new(
        ZoneContent::new(
            arena::ground(),
            ZoneAreas::default(),
            vec![template],
            vec![arena::spawn(1, WOLF, [0, -150])],
            Vec::new(),
            GRAVEYARD,
        )
        .unwrap()
        .with_creature_abilities(ABILITY_CATALOG_REVISION, Default::default())
        .unwrap(),
    )
}

struct Warden {
    zone: ZoneSimulation,
    sequence: u32,
}

impl Warden {
    fn new(content: Arc<ZoneContent>) -> Self {
        let mut zone = ZoneSimulation::with_content(ZoneId::new(9), content).unwrap();
        zone.add_player(1).unwrap();
        let mut warden = Self { zone, sequence: 0 };
        warden.send(ZoneCommand::ChooseClass { class: 0, sex: 0 });
        warden.send(arena::stand(SOUTH));
        warden.tick();
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

    fn stance(&self) -> Option<MeleeStance> {
        self.zone.snapshot_for_player(1).unwrap().viewer.stance
    }
}

/// The first auto-attack swing's damage and the stance shown that tick.
fn first_swing(movement: &[ZoneCommand]) -> (u16, Option<MeleeStance>) {
    let mut warden = Warden::new(content(CreatureBehaviour::Neutral, [1, 1]));
    warden.send(ZoneCommand::SelectTarget(Some(TARGET)));
    warden.send(ZoneCommand::StartAttack);
    for &command in movement {
        warden.send(command);
    }
    let events = warden.tick();
    let dealt = events
        .iter()
        .find_map(|event| match *event {
            ZoneEvent::DamageDealt { amount, .. } => Some(amount),
            _ => None,
        })
        .expect("the first swing hits");
    (dealt, warden.stance())
}

#[test]
fn the_same_swing_hits_harder_charging_or_airborne_and_softer_backing_away() {
    let (planted, stance) = first_swing(&[]);
    assert_eq!(stance, Some(MeleeStance::Planted));
    let scaled = |percent: u32| u16::try_from((u32::from(planted) * percent / 100).max(1)).unwrap();
    let (charging, stance) = first_swing(&[arena::walk(SOUTH)]);
    assert_eq!(stance, Some(MeleeStance::Charging));
    assert_eq!(charging, scaled(120));
    let (airborne, stance) = first_swing(&[ZoneCommand::Jump]);
    assert_eq!(stance, Some(MeleeStance::Airborne));
    assert_eq!(airborne, scaled(135));
    let backing = ZoneCommand::Move {
        forward: -1,
        strafe: 0,
        facing: SOUTH,
    };
    let (moving, stance) = first_swing(&[backing]);
    assert_eq!(stance, Some(MeleeStance::Moving));
    assert_eq!(moving, scaled(85));
    assert!(airborne > charging && charging > planted && planted > moving);
}

/// The first creature blow the Warden takes while holding `movement`.
fn first_blow(movement: ZoneCommand) -> u16 {
    let mut warden = Warden::new(content(CreatureBehaviour::Aggressive, [20, 20]));
    warden.send(movement);
    for _ in 0..mmorpg_core::golden::t30(200) {
        for event in warden.tick() {
            if let ZoneEvent::DamageTaken { amount, .. } = event {
                return amount;
            }
        }
    }
    panic!("the creature never hit");
}

#[test]
fn a_planted_warden_braces_against_creature_blows() {
    assert_eq!(first_blow(arena::stand(SOUTH)), 18);
    // Pressing into the creature slides the Warden around it (#273), moving and unbraced.
    assert_eq!(first_blow(arena::walk(SOUTH)), 20);
}

#[test]
fn melee_requested_while_landing_waits_for_the_recovery_to_end() {
    let mut warden = Warden::new(content(CreatureBehaviour::Neutral, [1, 1]));
    warden.send(ZoneCommand::SelectTarget(Some(TARGET)));
    warden.send(ZoneCommand::Jump);
    let mut ticks = 0;
    while warden.stance() != Some(MeleeStance::Landing) {
        warden.tick();
        ticks += 1;
        assert!(ticks < 120, "the jump lands");
    }
    // A fresh Warden has no Rage: the zone answers the request only once
    // the landing recovery ends, instead of refusing or resolving it early.
    warden.send(ZoneCommand::UseAbility {
        ability: ids::HEROIC_STRIKE.get(),
        target: Some(TARGET),
    });
    let mut waited = 0;
    loop {
        let events = warden.tick();
        let answered = events.iter().any(|event| {
            matches!(
                event,
                ZoneEvent::Error {
                    code: ErrorCode::NotEnoughResource,
                    ..
                }
            )
        });
        if answered {
            break;
        }
        waited += 1;
        assert!(waited < 30, "the request resolves after the landing");
    }
    // It resolves on the first tick after the 0.3 s recovery.
    assert_eq!(waited, u32::from(mmorpg_core::LANDING_RECOVERY_TICKS));
    assert_eq!(warden.stance(), Some(MeleeStance::Planted));
}

#[test]
fn other_classes_have_no_stance() {
    let mut zone =
        ZoneSimulation::with_content(ZoneId::new(9), content(CreatureBehaviour::Neutral, [1, 1]))
            .unwrap();
    zone.add_player(1).unwrap();
    zone.apply_command(1, 1, ZoneCommand::ChooseClass { class: 1, sex: 0 })
        .unwrap();
    zone.advance_tick().unwrap();
    assert_eq!(zone.snapshot_for_player(1).unwrap().viewer.stance, None);
}
