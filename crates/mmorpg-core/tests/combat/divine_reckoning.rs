//! Divine Reckoning (#936): a Cleric's pool against each enemy grows only from that enemy's
//! hits on the Cleric and nearby allies, survives heals and recovery, holds its cap, converts
//! and consumes only the target's pool, ends when the fight does, and replays exactly.
use crate::support;

use std::collections::BTreeMap;
use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::{
    CanonicalZoneSnapshot, CreatureId, CreatureTemplate, CreatureTemplateId, EntityRef,
    ZoneCommand, ZoneContent, ZoneEvent, ZoneId, ZoneSimulation,
};
use support::arena::{self, WOLF};
use support::classes;

const CLERIC: u32 = 1;
const ALLY: u32 = 2;
/// Bites the Cleric hard; the enemy kept alive for the reckoning.
const BRUTE: CreatureId = CreatureId::new(1);
/// Bites only the ally, once it walks into reach.
const HOUND: CreatureId = CreatureId::new(2);
const HOUND_TEMPLATE: CreatureTemplateId = CreatureTemplateId::new(7);
/// Divine Reckoning's provisional tuning (#936).
const PERCENT: u32 = 70;
const CAP: u32 = 200;

fn content() -> Arc<ZoneContent> {
    let hound = CreatureTemplate {
        id: HOUND_TEMPLATE,
        name: "Test Hound".into(),
        ..arena::wolf(5_000, [4, 4])
    };
    classes::arena(
        vec![arena::wolf(5_000, [12, 12]), hound],
        vec![
            // Inside a level 6 Cleric's aggro radius; the hound beyond it and the ally's.
            arena::spawn(BRUTE.get(), WOLF, [-5_000, -4_600]),
            arena::spawn(HOUND.get(), HOUND_TEMPLATE, [-3_500, -5_000]),
        ],
    )
}

/// The zone restored from its canonical snapshot after `edit`, as a recovering host would.
fn restored(
    zone: &ZoneSimulation,
    edit: impl FnOnce(&mut CanonicalZoneSnapshot),
) -> ZoneSimulation {
    let mut state = zone.snapshot().unwrap();
    edit(&mut state);
    ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap()
}

fn place(state: &mut CanonicalZoneSnapshot, player: u32, [x, z]: [f32; 2]) {
    let record = state
        .players
        .iter_mut()
        .find(|record| record.player_id == player)
        .unwrap();
    record.position = [x, record.position[1], z];
    record.velocity = [0.0; 3];
}

fn pools(zone: &ZoneSimulation) -> BTreeMap<CreatureId, u32> {
    let state = zone.snapshot().unwrap();
    let cleric = state
        .players
        .iter()
        .find(|record| record.player_id == CLERIC)
        .unwrap();
    cleric
        .combat
        .abilities
        .reckoning
        .iter()
        .map(|harm| (harm.creature, u32::from(harm.amount)))
        .collect()
}

/// One scripted encounter, checked against a plain sum of the hits each enemy landed.
struct Encounter {
    zone: ZoneSimulation,
    sequence: u32,
    /// What each enemy has dealt the Cleric and, once in reach, the ally; uncapped.
    harm: BTreeMap<CreatureId, u32>,
    /// The ally's hits count once it stands beside the Cleric.
    ally_counts: bool,
    /// Every tick's pools, for the replay comparison.
    trace: Vec<BTreeMap<CreatureId, u32>>,
    /// Damage the Cleric dealt the brute.
    struck: Vec<u16>,
    /// The Cleric's health rose this tick.
    healed: bool,
}

impl Encounter {
    fn send(&mut self, command: ZoneCommand) {
        self.sequence += 1;
        self.zone
            .apply_command(CLERIC, self.sequence, command)
            .unwrap();
    }

    fn tick(&mut self) {
        let health = self.health();
        self.zone.advance_tick().unwrap();
        self.healed = self.health() > health;
        for player in [CLERIC, ALLY] {
            for event in self.zone.snapshot_for_player(player).unwrap().events {
                match event {
                    ZoneEvent::DamageTaken {
                        source: EntityRef::Creature(creature),
                        amount,
                        ..
                    } if player == CLERIC || self.ally_counts => {
                        *self.harm.entry(creature).or_default() += u32::from(amount);
                    }
                    ZoneEvent::DamageDealt {
                        source: EntityRef::Player(CLERIC),
                        target: EntityRef::Creature(BRUTE),
                        amount,
                        ..
                    } if player == CLERIC => self.struck.push(amount),
                    _ => {}
                }
            }
        }
        let pools = pools(&self.zone);
        let expected: BTreeMap<_, _> = self
            .harm
            .iter()
            .map(|(&creature, &harm)| (creature, harm.min(CAP)))
            .collect();
        assert_eq!(pools, expected, "tick {}", self.zone.current_tick());
        self.trace.push(pools);
    }

    fn until(&mut self, limit: u64, done: impl Fn(&Self) -> bool) {
        for _ in 0..limit {
            self.tick();
            if done(self) {
                return;
            }
        }
        panic!("not reached within {limit} ticks");
    }

    fn health(&self) -> u32 {
        self.zone.snapshot_for_player(CLERIC).unwrap().viewer.health
    }

    fn ready(&self) -> bool {
        self.zone
            .snapshot()
            .unwrap()
            .players
            .iter()
            .find(|record| record.player_id == CLERIC)
            .is_some_and(|record| record.combat.abilities.global_cooldown == 0)
    }
}

fn encounter() -> Vec<BTreeMap<CreatureId, u32>> {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(9), content()).unwrap();
    zone.add_player(CLERIC).unwrap();
    zone.add_player(ALLY).unwrap();
    let mut fight = Encounter {
        zone: classes::at_level(zone, CLERIC, 6),
        sequence: 0,
        harm: BTreeMap::new(),
        ally_counts: false,
        trace: Vec::new(),
        struck: Vec::new(),
        healed: false,
    };
    fight.send(ZoneCommand::ChooseClass { class: 5, sex: 1 });
    fight.tick();
    // The Cleric away from the spawn grid, the ally 15 m west: beyond every aggro radius but
    // the brute's.
    fight.zone = restored(&fight.zone, |state| {
        place(state, CLERIC, [-5_000.0, -5_000.0]);
        place(state, ALLY, [-6_500.0, -5_000.0]);
    });

    // Endure: the brute's bites fill its pool alone.
    fight.until(600, |fight| {
        fight.harm.get(&BRUTE).is_some_and(|&harm| harm >= 50)
    });
    assert!(!fight.harm.contains_key(&HOUND));

    // Heal: Renew on the Cleric (faith granted through recovery) restores health, never harm.
    fight.zone = restored(&fight.zone, |state| {
        state.players[0].combat.abilities.resource = 60;
    });
    fight.send(ZoneCommand::UseAbility {
        ability: ids::RENEW.get(),
        target: None,
        facing: None,
    });
    fight.until(300, |fight| fight.healed);

    // The ally walks into the hound's reach 10 m from the Cleric: its bites count against the
    // hound only, never the brute.
    fight.zone = restored(&fight.zone, |state| {
        place(state, ALLY, [-4_000.0, -5_000.0])
    });
    fight.ally_counts = true;
    fight.until(900, |fight| {
        fight.harm.get(&HOUND).is_some_and(|&harm| harm >= 8)
    });

    // Endure more: past the cap, a full heal between bites leaves the pool where it is.
    while fight.harm[&BRUTE] <= CAP {
        fight.until(1_800, |fight| {
            fight.harm[&BRUTE] > CAP || fight.health() < 40
        });
        if fight.health() < 40 {
            let full = fight
                .zone
                .snapshot_for_player(CLERIC)
                .unwrap()
                .viewer
                .max_health;
            fight.zone = restored(&fight.zone, |state| state.players[0].combat.health = full);
        }
    }
    assert_eq!(pools(&fight.zone)[&BRUTE], CAP, "the cap holds");

    // Retaliate: the reckoning shows against the selected brute, strikes it for 70% of the
    // capped pool and consumes only that pool.
    fight.send(ZoneCommand::SelectTarget(Some(EntityRef::Creature(BRUTE))));
    fight.tick();
    let view = fight.zone.snapshot_for_player(CLERIC).unwrap();
    assert_eq!(u32::from(view.viewer.reckoning), CAP);
    fight.zone = restored(&fight.zone, |state| {
        state.players[0].combat.abilities.resource = 60;
    });
    fight.until(120, Encounter::ready);
    let hound = fight.harm[&HOUND];
    let (facing, _) = arena::aim_at(&fight.zone, CLERIC, EntityRef::Creature(BRUTE)).unwrap();
    fight.send(arena::stand(facing));
    fight.send(ZoneCommand::UseAbility {
        ability: ids::DIVINE_RECKONING.get(),
        target: Some(EntityRef::Creature(BRUTE)),
        facing: None,
    });
    fight.harm.remove(&BRUTE);
    fight.struck.clear();
    fight.tick();
    assert_eq!(fight.struck, [u16::try_from(CAP * PERCENT / 100).unwrap()]);
    assert_eq!(fight.harm[&HOUND], hound, "the hound's pool stays");

    // Disengage: far from both, the fight ends and so does every pool.
    fight.zone = restored(&fight.zone, |state| {
        place(state, CLERIC, [-5_000.0, 5_000.0]);
        place(state, ALLY, [-4_500.0, 5_000.0]);
    });
    fight.harm.clear();
    fight.ally_counts = false;
    for _ in 0..1_800 {
        fight.zone.advance_tick().unwrap();
        if pools(&fight.zone).is_empty() {
            break;
        }
    }
    assert!(
        pools(&fight.zone).is_empty(),
        "the pools end with the fight"
    );
    fight.trace
}

#[test]
fn a_cleric_reckons_only_the_harm_its_target_dealt_and_replays_it_exactly() {
    assert_eq!(
        encounter(),
        encounter(),
        "the same encounter replays exactly"
    );
}
