//! Every ability lands (#884, Atlas 5.2.i884): free-aimed missiles, arrows and beams meet a rat on
//! flat ground and a wolf up on a ledge, a selection inside the aim cone is the unit hit, Rapid
//! Fire shortens a Ranger's shot spacing, the damage-over-time openers land at once, and the
//! scaled numbers show.
use crate::support;

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::{
    AbilityId, CreatureFamily, CreatureId, CreatureTemplateId, EntityRef, SpawnGrid,
    StaticCollider, ZoneAreas, ZoneCommand, ZoneContent, ZoneDefinition, ZoneEvent, ZoneId,
    ZoneSimulation,
};
use support::arena::{self, NORTHWARD, WOLF};

const RAT: CreatureTemplateId = CreatureTemplateId::new(2);
const WARDEN: u8 = 0;
const RANGER: u8 = 1;
const ARCANIST: u8 = 2;
const NECROMANCER: u8 = 3;
const ROGUE: u8 = 4;
const CLERIC: u8 = 5;
const DRUID: u8 = 6;
/// The ledge's top, units above the flat ground: higher than a level missile flies (0.5 m).
const LEDGE: i32 = 120;
/// The ledge begins this far north, units.
const LEDGE_FROM: i32 = 1_500;

fn creature(id: u32) -> EntityRef {
    EntityRef::Creature(CreatureId::new(id))
}

/// Flat ground, a 1.2 m ledge from 15 m north on, and sturdy neutral units: wolves, or rats
/// whose 0.4 m bodies a level missile used to sail over, at `spawns` (`true` for a rat).
fn content(spawns: &[([i32; 2], bool)]) -> Arc<ZoneContent> {
    let colliders = vec![
        StaticCollider {
            id: 1,
            position: [0, -50, 0],
            half_extents: [10_000, 50, 10_000],
        },
        StaticCollider {
            id: 2,
            position: [0, LEDGE / 2, 2_500],
            half_extents: [3_000, LEDGE / 2, 1_000],
        },
    ];
    let definition = ZoneDefinition::with_spawn_grid(
        mmorpg_core::STANDARD_GRAVITY,
        SpawnGrid {
            origin: [-5_000, -5_000],
            columns: 32,
            spacing: 100,
        },
        colliders,
    )
    .unwrap();
    let spawns = spawns
        .iter()
        .zip(1..)
        .map(|(&(at, rat), id)| {
            // Content refuses a spawn inside the ledge: one there spawns mirrored south, and the
            // caster lifts it onto the ledge.
            let at = if at[1] >= LEDGE_FROM {
                [at[0], -at[1]]
            } else {
                at
            };
            arena::spawn(id, if rat { RAT } else { WOLF }, at)
        })
        .collect();
    let wolf = mmorpg_core::CreatureTemplate {
        behaviour: mmorpg_core::CreatureBehaviour::Neutral,
        ..arena::wolf(5_000, [1, 1])
    };
    let rat = mmorpg_core::CreatureTemplate {
        id: RAT,
        name: "Test Rat".into(),
        family: CreatureFamily::Vermin,
        half_extents: [25, 20, 25],
        ..wolf.clone()
    };
    Arc::new(
        ZoneContent::new(
            definition,
            ZoneAreas::default(),
            vec![wolf, rat],
            spawns,
            Vec::new(),
            arena::GRAVEYARD,
        )
        .unwrap()
        .with_creature_abilities(Vec::new())
        .unwrap(),
    )
}

struct Caster {
    zone: ZoneSimulation,
    sequence: u32,
    events: Vec<(u64, ZoneEvent)>,
}

impl Caster {
    /// A level-8 `class` at the origin facing north with a full resource; units north of 15 m
    /// stand on the ledge.
    fn new(content: Arc<ZoneContent>, class: u8) -> Self {
        let mut zone = ZoneSimulation::with_content(ZoneId::new(9), content).unwrap();
        zone.add_player(1).unwrap();
        let mut zone = support::classes::at_level(zone, 1, 8);
        zone.apply_command(1, 1, ZoneCommand::ChooseClass { class, sex: 0 })
            .unwrap();
        zone.advance_tick().unwrap();
        let mut state = zone.snapshot().unwrap();
        state.players[0].position = [0.0, 90.0, 0.0];
        state.players[0].combat.abilities.resource = 100;
        for record in &mut state.creatures {
            if record.position[2] <= -LEDGE_FROM as f32 {
                record.position[1] += LEDGE as f32;
                record.position[2] = -record.position[2];
            }
        }
        let zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
        let mut caster = Self {
            zone,
            sequence: 1,
            events: Vec::new(),
        };
        caster.send(arena::stand(NORTHWARD));
        caster.ticks(5);
        caster
    }

    fn send(&mut self, command: ZoneCommand) {
        self.sequence += 1;
        self.zone.apply_command(1, self.sequence, command).unwrap();
    }

    fn ticks(&mut self, count: u64) {
        for _ in 0..mmorpg_core::golden::t30(count) {
            self.zone.advance_tick().unwrap();
            let now = self.zone.current_tick();
            let events = self.zone.snapshot_for_player(1).unwrap().events;
            self.events
                .extend(events.into_iter().map(|event| (now, event)));
        }
    }

    fn cast(&mut self, ability: AbilityId) {
        self.send(ZoneCommand::UseAbility {
            ability: ability.get(),
            target: None,
            facing: Some(NORTHWARD),
        });
    }

    /// Damage the player dealt to `unit`, in order.
    fn hits(&self, unit: EntityRef) -> Vec<u16> {
        self.events
            .iter()
            .filter_map(|(_, event)| match *event {
                ZoneEvent::DamageDealt { target, amount, .. } if target == unit => Some(amount),
                _ => None,
            })
            .collect()
    }

    fn refusals(&self) -> Vec<ZoneEvent> {
        self.events
            .iter()
            .filter(|(_, event)| matches!(event, ZoneEvent::Error { .. }))
            .map(|&(_, event)| event)
            .collect()
    }
}

/// Every damaging free missile, arrow, knife and beam with the class that casts it.
const AIMED: [(u8, AbilityId); 12] = [
    (ARCANIST, ids::FIREBOLT),
    (NECROMANCER, ids::SHADOW_BOLT),
    (CLERIC, ids::SMITE),
    (DRUID, ids::WRATH),
    (RANGER, ids::AIMED_SHOT),
    (RANGER, ids::SERPENT_STING),
    (RANGER, ids::PIERCING_SHOT),
    (RANGER, ids::RICOCHET_SHOT),
    (NECROMANCER, ids::CORRUPTION),
    (ROGUE, ids::THROWING_KNIFE),
    (ARCANIST, ids::LIGHTNING_BEAM),
    (CLERIC, ids::HOLY_BEAM),
];

#[test]
fn every_damaging_missile_and_beam_hits_a_rat_on_flat_ground_and_a_wolf_uphill() {
    for (at, rat, ground) in [
        ([0, 1_200], true, "a rat on flat ground"),
        ([0, 2_000], false, "a wolf on the ledge"),
    ] {
        for (class, ability) in AIMED {
            let mut caster = Caster::new(content(&[(at, rat)]), class);
            caster.cast(ability);
            caster.ticks(150);
            assert!(
                !caster.hits(creature(1)).is_empty(),
                "{ability:?} misses {ground}: {:?}",
                caster.refusals()
            );
        }
    }
}

#[test]
fn a_selection_inside_the_aim_cone_is_the_unit_hit() {
    // Boar 1 straight ahead; boar 2 10° off the line, inside the 15° cone; boar 3 40° off.
    let spawns = [([0, 1_400], true), ([174, 990], true), ([-643, 766], true)];
    let mut caster = Caster::new(content(&spawns), ARCANIST);
    caster.send(ZoneCommand::SelectTarget(Some(creature(2))));
    caster.cast(ids::FIREBOLT);
    caster.ticks(100);
    assert_eq!(caster.hits(creature(2)).len(), 1, "the selection is hit");
    assert!(caster.hits(creature(1)).is_empty());

    // A selection outside the cone is ignored for the unit in front.
    let mut caster = Caster::new(content(&spawns), ARCANIST);
    caster.send(ZoneCommand::SelectTarget(Some(creature(3))));
    caster.cast(ids::FIREBOLT);
    caster.ticks(100);
    assert_eq!(
        caster.hits(creature(1)).len(),
        1,
        "the unit in front is hit"
    );
    assert!(caster.hits(creature(3)).is_empty());
}

#[test]
fn a_cast_turns_to_the_facing_it_carries() {
    // Facing north, the cast carries east, where the only rat stands.
    let mut caster = Caster::new(content(&[([1_200, 0], true)]), ARCANIST);
    caster.send(ZoneCommand::UseAbility {
        ability: ids::FIREBOLT.get(),
        target: None,
        facing: Some(16_384),
    });
    caster.ticks(100);
    assert_eq!(caster.hits(creature(1)).len(), 1);
}

#[test]
fn rapid_fire_shortens_a_rangers_shot_spacing() {
    let gcd_after_shot = |hasted: bool| {
        let mut caster = Caster::new(content(&[([0, 1_200], true)]), RANGER);
        if hasted {
            caster.cast(ids::RAPID_FIRE);
            caster.ticks(60);
        }
        caster.cast(ids::SERPENT_STING);
        caster.zone.advance_tick().unwrap();
        caster
            .zone
            .snapshot_for_player(1)
            .unwrap()
            .viewer
            .global_cooldown
    };
    let (plain, hasted) = (gcd_after_shot(false), gcd_after_shot(true));
    // 1.5 s, and a third less under Rapid Fire's 50% haste: 1 s.
    assert_eq!(
        plain - hasted,
        mmorpg_core::GLOBAL_COOLDOWN_TICKS - mmorpg_core::GLOBAL_COOLDOWN_TICKS * 2 / 3
    );
}

#[test]
fn damage_over_time_openers_land_at_once() {
    for (class, ability) in [
        (RANGER, ids::SERPENT_STING),
        (NECROMANCER, ids::CORRUPTION),
        (ROGUE, ids::POISONED_BLADE),
    ] {
        let mut caster = Caster::new(content(&[([0, 200], false)]), class);
        caster.cast(ability);
        // Before the first 3 s pulse.
        caster.ticks(30);
        assert_eq!(caster.hits(creature(1)).len(), 1, "{ability:?}");
    }
}

#[test]
fn control_abilities_stay_without_damage() {
    for (class, ability) in [
        (WARDEN, ids::SHIELD_BASH),
        (ROGUE, ids::GOUGE),
        (WARDEN, ids::CHARGE),
        (RANGER, ids::CONCUSSIVE_SHOT),
        (DRUID, ids::ENTANGLING_ROOTS),
    ] {
        let at = if ability == ids::CHARGE { 800 } else { 200 };
        let mut caster = Caster::new(content(&[([0, at], false)]), class);
        caster.cast(ability);
        caster.ticks(60);
        assert!(caster.hits(creature(1)).is_empty(), "{ability:?}");
        assert!(
            caster.refusals().is_empty(),
            "{ability:?}: {:?}",
            caster.refusals()
        );
    }
}

#[test]
fn frost_nova_scales_with_level() {
    let nova = |level: u8| {
        let mut zone =
            ZoneSimulation::with_content(ZoneId::new(9), content(&[([0, 200], false)])).unwrap();
        zone.add_player(1).unwrap();
        let mut zone = support::classes::at_level(zone, 1, level);
        let mut state = zone.snapshot().unwrap();
        state.players[0].position = [0.0, 90.0, 0.0];
        zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
        let mut caster = Caster {
            zone,
            sequence: 0,
            events: Vec::new(),
        };
        caster.send(ZoneCommand::ChooseClass {
            class: ARCANIST,
            sex: 0,
        });
        caster.ticks(5);
        caster.cast(ids::FROST_NOVA);
        caster.ticks(5);
        caster.hits(creature(1))
    };
    let (low, high) = (nova(2), nova(8));
    assert_eq!(low.len(), 1);
    // One more per level: six more at level 8, on the same roll.
    assert_eq!(high, vec![low[0] + 6]);
}

#[test]
fn cleave_scales_each_victim_by_the_wardens_stance_toward_it() {
    const SOUTH: u16 = 32_768;
    // Wolf 1 straight south, wolf 2 within the cleave beside it, well off the run's heading.
    let cleave = |running: bool| {
        let mut caster = Caster::new(content(&[([0, -200], false), ([250, -200], false)]), WARDEN);
        caster.send(if running {
            arena::walk(SOUTH)
        } else {
            arena::stand(SOUTH)
        });
        for _ in 0..3 {
            caster.zone.advance_tick().unwrap();
        }
        caster.send(ZoneCommand::UseAbility {
            ability: ids::CLEAVE.get(),
            target: None,
            facing: Some(SOUTH),
        });
        caster.ticks(1);
        [caster.hits(creature(1)), caster.hits(creature(2))]
    };
    let ([planted_1, planted_2], [running_1, running_2]) = (cleave(false), cleave(true));
    let scaled = |hits: &[u16], percent: u32| {
        hits.iter()
            .map(|&amount| u16::try_from((u32::from(amount) * percent / 100).max(1)).unwrap())
            .collect::<Vec<_>>()
    };
    assert_eq!(planted_1.len(), 1, "{planted_1:?}");
    assert_eq!(planted_2.len(), 1, "{planted_2:?}");
    // Charging at wolf 1 (120%), merely moving past wolf 2 (85%).
    assert_eq!(running_1, scaled(&planted_1, 120));
    assert_eq!(running_2, scaled(&planted_2, 85));
}
