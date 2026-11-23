//! The Necromancer's corpse payoffs (#935) through the public zone API: a victim that dies under
//! Corruption leaves one fresh corpse, spent once on either Raise Dead or Corpse Burst.
use crate::support;

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::golden::t30;
use mmorpg_core::{
    AuraKind, CanonicalCreatureSnapshot, CanonicalZoneSnapshot, CreatureId, CreatureLife,
    CreatureTemplate, CreatureTemplateId, EntityRef, ErrorCode, LootOutcome, LootTable,
    RAISE_DEAD_LIMIT, ZoneCommand, ZoneContent, ZoneEvent, ZoneId, ZoneSimulation,
};
use support::{arena, classes};

const VICTIM: CreatureTemplateId = CreatureTemplateId::new(7);
const TOUGH: CreatureTemplateId = CreatureTemplateId::new(8);
/// The first victim stands 3 m south with two tough boars beside it; a third stands 15 m west
/// of the Necromancer, and a second victim 17 m to its north-west.
const FIRST: u32 = 1;
const NEAR: [u32; 2] = [2, 3];
const FAR: u32 = 4;
const SECOND: u32 = 5;
const TOUGH_HEALTH: u32 = 400;

fn content() -> Arc<ZoneContent> {
    let victim = CreatureTemplate {
        id: VICTIM,
        name: "Test Victim".into(),
        health: 20,
        ..arena::boar()
    };
    let tough = CreatureTemplate {
        id: TOUGH,
        name: "Test Tough Boar".into(),
        health: TOUGH_HEALTH,
        ..arena::boar()
    };
    let content = classes::arena(
        vec![victim, tough],
        vec![
            arena::spawn(FIRST, VICTIM, [0, -300]),
            arena::spawn(NEAR[0], TOUGH, [350, -300]),
            arena::spawn(NEAR[1], TOUGH, [-350, -300]),
            arena::spawn(FAR, TOUGH, [-1_500, 0]),
            arena::spawn(SECOND, VICTIM, [-900, 1_500]),
        ],
    );
    // Victims always drop coin, so the corpse has loot a payoff must not take.
    let coin = LootTable::new([5, 9], &[LootOutcome::Nothing { weight: 1 }]).unwrap();
    Arc::new(
        Arc::unwrap_or_clone(content)
            .with_loot_tables(vec![(VICTIM, coin)])
            .unwrap(),
    )
}

struct Fight {
    zone: ZoneSimulation,
    sequence: u32,
    events: Vec<ZoneEvent>,
}

impl Fight {
    /// A level-6 Necromancer at the origin.
    fn new() -> Self {
        let mut zone = ZoneSimulation::with_content(ZoneId::new(9), content()).unwrap();
        zone.add_player(1).unwrap();
        let mut fight = Self {
            zone: classes::at_level(zone, 1, 6),
            sequence: 0,
            events: Vec::new(),
        };
        fight.send(ZoneCommand::ChooseClass { class: 3, sex: 1 });
        fight.tick();
        fight
    }

    fn send(&mut self, command: ZoneCommand) {
        self.sequence += 1;
        self.zone.apply_command(1, self.sequence, command).unwrap();
    }

    fn tick(&mut self) {
        self.zone.advance_tick().unwrap();
        let events = self.zone.snapshot_for_player(1).unwrap().events;
        self.events.extend(events);
    }

    fn ticks(&mut self, count: u64) {
        for _ in 0..count {
            self.tick();
        }
    }

    /// Uses `ability` toward `target` and waits out the global cooldown.
    fn cast(&mut self, ability: mmorpg_core::AbilityId, target: Option<u32>) {
        let target = target.map(|id| EntityRef::Creature(CreatureId::new(id)));
        let facing = target.and_then(|unit| arena::aim_at(&self.zone, 1, unit).map(|(yaw, _)| yaw));
        self.send(ZoneCommand::UseAbility {
            ability: ability.get(),
            target,
            facing,
        });
        self.ticks(t30(45));
    }

    /// Corrupts `victim` and waits until it dies.
    fn corrupt_to_death(&mut self, victim: u32) {
        self.cast(ids::CORRUPTION, Some(victim));
        for _ in 0..t30(600) {
            if self.creature(victim).life != CreatureLife::Alive {
                return;
            }
            self.tick();
        }
        panic!("creature {victim} survived its Corruption");
    }

    fn state(&self) -> CanonicalZoneSnapshot {
        self.zone.snapshot().unwrap()
    }

    fn creature(&self, id: u32) -> CanonicalCreatureSnapshot {
        self.state()
            .creatures
            .into_iter()
            .find(|creature| creature.creature_id.get() == id)
            .unwrap()
    }

    /// The fresh corpse the Necromancer's own projection shows (#935): its mark and time left.
    fn corpse_marks(&self) -> usize {
        let view = self.zone.snapshot_for_player(1).unwrap();
        view.auras
            .iter()
            .filter(|aura| aura.kind == AuraKind::Corpse && aura.ability == ids::RAISE_DEAD)
            .count()
    }

    fn helpers(&self) -> usize {
        self.state()
            .minions
            .iter()
            .filter(|record| record.minion.ability == ids::RAISE_DEAD)
            .count()
    }

    /// Drains and returns the error codes seen so far.
    fn errors(&mut self) -> Vec<ErrorCode> {
        std::mem::take(&mut self.events)
            .into_iter()
            .filter_map(|event| match event {
                ZoneEvent::Error { code, .. } => Some(code),
                _ => None,
            })
            .collect()
    }
}

fn xz_distance(a: &CanonicalCreatureSnapshot, b: &CanonicalCreatureSnapshot) -> f32 {
    (a.position[0] - b.position[0]).hypot(a.position[2] - b.position[2])
}

/// The shared setup: the first victim dies under Corruption and leaves its corpse mark, which a
/// recovery from the canonical snapshot keeps.
fn first_kill() -> Fight {
    let mut fight = Fight::new();
    fight.corrupt_to_death(FIRST);
    assert_eq!(
        fight.corpse_marks(),
        1,
        "the corrupted kill leaves one corpse"
    );
    assert!(
        fight.creature(FIRST).loot.is_some(),
        "the corpse keeps its loot"
    );
    let recovered =
        ZoneSimulation::from_snapshot(fight.state(), Arc::clone(fight.zone.content())).unwrap();
    assert_eq!(recovered.snapshot().unwrap(), fight.state());
    fight.errors();
    fight
}

#[test]
fn raise_dead_spends_the_corpse_once_and_its_helper_is_capped_and_expires() {
    let mut fight = first_kill();
    let loot = fight.creature(FIRST).loot;

    fight.cast(ids::RAISE_DEAD, None);
    assert_eq!(fight.helpers(), 1);
    assert_eq!(fight.corpse_marks(), 0, "the corpse is spent");
    assert_eq!(
        fight.creature(FIRST).loot,
        loot,
        "spending keeps the earned loot"
    );
    assert!(matches!(
        fight.creature(FIRST).life,
        CreatureLife::Corpse { .. }
    ));

    // The spent corpse neither bursts nor rises again.
    fight.cast(ids::CORPSE_BURST, None);
    fight.cast(ids::RAISE_DEAD, None);
    assert_eq!(fight.errors(), [ErrorCode::NoTarget, ErrorCode::NoTarget]);
    assert_eq!(fight.helpers(), 1);

    // A second corpse cannot raise past the cap while the helper stands, and keeps its mark.
    fight.corrupt_to_death(SECOND);
    assert_eq!(fight.corpse_marks(), 1);
    fight.cast(ids::RAISE_DEAD, None);
    assert_eq!(fight.errors(), [ErrorCode::NotReady]);
    assert_eq!(fight.helpers(), RAISE_DEAD_LIMIT);
    assert_eq!(fight.corpse_marks(), 1);

    // The helper crumbles when its 20 s run out.
    fight.ticks(t30(600));
    assert_eq!(fight.helpers(), 0);
    assert_eq!(
        fight.corpse_marks(),
        0,
        "the second corpse's 10 s window passed"
    );
}

#[test]
fn corpse_burst_hits_the_near_enemies_and_spares_the_far_one() {
    let mut fight = first_kill();
    let corpse = fight.creature(FIRST);
    for id in NEAR {
        assert!(xz_distance(&corpse, &fight.creature(id)) < 600.0);
    }
    assert!(xz_distance(&corpse, &fight.creature(FAR)) > 600.0);

    fight.cast(ids::CORPSE_BURST, None);
    for id in NEAR {
        let health = fight.creature(id).health;
        assert!(health < TOUGH_HEALTH, "near boar {id} took the burst");
    }
    assert_eq!(fight.creature(FAR).health, TOUGH_HEALTH);
    assert_eq!(fight.corpse_marks(), 0, "the corpse is spent");
    assert_eq!(fight.creature(FIRST).loot, corpse.loot);

    fight.cast(ids::RAISE_DEAD, None);
    fight.cast(ids::CORPSE_BURST, None);
    assert_eq!(fight.errors(), [ErrorCode::NoTarget, ErrorCode::NoTarget]);
    assert_eq!(fight.helpers(), 0);
}

#[test]
fn both_payoffs_replay_exactly() {
    let run = |payoff: mmorpg_core::AbilityId| {
        let mut fight = first_kill();
        fight.cast(payoff, None);
        fight.ticks(t30(90));
        fight.state()
    };
    for payoff in [ids::RAISE_DEAD, ids::CORPSE_BURST] {
        assert_eq!(run(payoff), run(payoff));
    }
}
