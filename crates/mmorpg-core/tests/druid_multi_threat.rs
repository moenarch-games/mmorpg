//! The Druid's multi-threat fight (#934) at Millbrook Farm's field marauders in Greyhaven: the
//! wolf companion (#296) holds one marauder on an Attack order, Entangling Roots (#304) holds a
//! second in place, and the Druid fights the third with Wrath. Damage after the root's first
//! second breaks it, and the whole fight replays exactly.
mod support;

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::{
    AbilityId, CanonicalCreatureSnapshot, CanonicalZoneSnapshot, CreatureAi, CreatureId, EntityRef,
    MinionId, MinionOrder, TICK_HZ, ZoneCommand, ZoneEvent, ZoneId, ZoneSimulation, greyhaven_vale,
};

/// Three of the field marauders: the wolf's, the rooted one and the Druid's own.
const HELD: CreatureId = CreatureId::new(220);
const ROOTED: CreatureId = CreatureId::new(221);
const FOUGHT: CreatureId = CreatureId::new(224);
/// Open ground south of the fields, beyond every marauder's aggro radius and clear of the
/// farmhouse.
const START: [i32; 2] = [7_000, 4_900];
const WOLF: EntityRef = EntityRef::Minion(MinionId::new(1));
const DRUID: EntityRef = EntityRef::Player(1);
/// Entangling Roots: 6 s, unbreakable for its first second.
const ROOT_TICKS: u64 = 6 * TICK_HZ as u64;
const UNBREAKABLE_TICKS: u64 = TICK_HZ as u64;

struct Fight {
    zone: ZoneSimulation,
    sequence: u32,
    events: Vec<(u64, ZoneEvent)>,
}

impl Fight {
    /// A level-4 Druid standing at `START`.
    fn new() -> Self {
        let mut zone =
            ZoneSimulation::with_content(ZoneId::new(1), greyhaven_vale::content()).unwrap();
        zone.add_player(1).unwrap();
        let zone = support::classes::at_level(zone, 1, 4);
        let mut state = zone.snapshot().unwrap();
        let ground = zone.content().definition().ground_at(START[0], START[1]);
        state.players[0].position =
            mmorpg_core::physics::from_units([START[0], ground + 100, START[1]]);
        let zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
        let mut fight = Self {
            zone,
            sequence: 0,
            events: Vec::new(),
        };
        fight.send(ZoneCommand::ChooseClass { class: 6, sex: 1 });
        fight.tick();
        fight
    }

    fn send(&mut self, command: ZoneCommand) {
        self.sequence += 1;
        self.zone.apply_command(1, self.sequence, command).unwrap();
    }

    fn tick(&mut self) {
        self.zone.advance_tick().unwrap();
        let tick = self.zone.current_tick();
        let projection = self.zone.snapshot_for_player(1).unwrap();
        self.events
            .extend(projection.events.into_iter().map(|event| (tick, event)));
    }

    fn ticks(&mut self, count: u64) {
        for _ in 0..count {
            self.tick();
        }
    }

    /// Ticks until `done` holds, for at most `limit` ticks.
    fn until(&mut self, limit: u64, done: impl Fn(&CanonicalZoneSnapshot) -> bool) {
        for _ in 0..limit {
            if done(&self.state()) {
                return;
            }
            self.tick();
        }
        panic!("not reached within {limit} ticks");
    }

    /// Uses `ability`, facing the creature it is aimed at.
    fn cast(&mut self, ability: AbilityId, target: Option<CreatureId>) {
        let facing = target.map(|id| self.heading_to(id));
        self.send(ZoneCommand::UseAbility {
            ability: ability.get(),
            target: target.map(EntityRef::Creature),
            facing,
        });
    }

    /// The heading from the Druid toward creature `id`: 0 faces +Z, 16 384 faces +X.
    fn heading_to(&self, id: CreatureId) -> u16 {
        let state = self.state();
        let [x, _, z] = state.players[0].position;
        let [tx, _, tz] = creature(&state, id).position;
        let turn = (tx - x).atan2(tz - z).rem_euclid(std::f32::consts::TAU);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let heading = (turn / std::f32::consts::TAU * 65_536.0) as u32 as u16;
        heading
    }

    fn state(&self) -> CanonicalZoneSnapshot {
        self.zone.snapshot().unwrap()
    }

    /// The wolf still holds the marauder it was ordered onto, and that marauder fights it.
    fn assert_wolf_holds(&self) {
        let state = self.state();
        let [wolf] = &state.minions[..] else {
            panic!("the companion stands");
        };
        assert_eq!(
            wolf.minion.order,
            MinionOrder::Attack(EntityRef::Creature(HELD))
        );
        assert_eq!(wolf.minion.target, Some(EntityRef::Creature(HELD)));
        assert_eq!(
            top_threat(&state, HELD),
            Some(WOLF),
            "the held marauder fights the wolf"
        );
    }
}

fn creature(state: &CanonicalZoneSnapshot, id: CreatureId) -> &CanonicalCreatureSnapshot {
    state
        .creatures
        .iter()
        .find(|creature| creature.creature_id == id)
        .unwrap()
}

fn rooted(state: &CanonicalZoneSnapshot, id: CreatureId) -> bool {
    creature(state, id)
        .abilities
        .auras
        .iter()
        .any(|aura| aura.ability == ids::ENTANGLING_ROOTS)
}

/// An engaged creature's threat leader, the unit it fights.
fn top_threat(state: &CanonicalZoneSnapshot, id: CreatureId) -> Option<EntityRef> {
    let creature = creature(state, id);
    if creature.ai != CreatureAi::Engaged {
        return None;
    }
    creature
        .threat
        .iter()
        .max_by(|a, b| a.threat.cmp(&b.threat).then(b.entity.cmp(&a.entity)))
        .map(|entry| entry.entity)
}

fn xz(position: [f32; 3]) -> [f32; 2] {
    [position[0], position[2]]
}

fn distance(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// The whole fight, asserting each beat.
fn journey() -> Fight {
    let second = u64::from(TICK_HZ);
    let mut fight = Fight::new();
    // The wolf answers its call, then goes for the first marauder on an Attack order.
    fight.cast(ids::CALL_COMPANION, None);
    fight.ticks(second * 5 / 2);
    fight.send(ZoneCommand::CommandMinions(MinionOrder::Attack(
        EntityRef::Creature(HELD),
    )));
    fight.until(second * 5, |state| top_threat(state, HELD).is_some());
    fight.assert_wolf_holds();

    // Wrath pulls the third marauder, which runs at the Druid.
    fight.cast(ids::WRATH, Some(FOUGHT));
    fight.until(second * 4, |state| top_threat(state, FOUGHT) == Some(DRUID));

    // Entangling Roots holds the second marauder where it stands, angry at the Druid.
    fight.cast(ids::ENTANGLING_ROOTS, Some(ROOTED));
    fight.tick();
    let rooted_at = fight.zone.current_tick();
    let state = fight.state();
    assert!(rooted(&state, ROOTED), "the second marauder is rooted");
    assert_eq!(top_threat(&state, ROOTED), Some(DRUID));
    let held_at = xz(creature(&state, ROOTED).position);
    let health = creature(&state, ROOTED).health;

    // The Druid turns Wrath on the rooted marauder: the hit lands after the root's first
    // second, so it breaks the root. Until then the marauder stands still and the wolf keeps
    // the first.
    fight.ticks(second * 3 / 2);
    fight.cast(ids::WRATH, Some(ROOTED));
    let broken = loop {
        fight.tick();
        let state = fight.state();
        let elapsed = fight.zone.current_tick() - rooted_at;
        assert!(
            elapsed < ROOT_TICKS,
            "the Wrath hit breaks the root before it expires"
        );
        if !rooted(&state, ROOTED) {
            assert!(elapsed > UNBREAKABLE_TICKS);
            assert!(creature(&state, ROOTED).health < health, "damage broke it");
            break fight.zone.current_tick();
        }
        assert_eq!(
            xz(creature(&state, ROOTED).position),
            held_at,
            "a rooted marauder stays put"
        );
        fight.assert_wolf_holds();
    };
    assert!(fight.events.contains(&(
        broken,
        ZoneEvent::AuraRemoved {
            source: DRUID,
            target: EntityRef::Creature(ROOTED),
            ability: ids::ENTANGLING_ROOTS,
        }
    )));
    // Meanwhile the third marauder reached the Druid and fights it.
    let state = fight.state();
    assert_eq!(top_threat(&state, FOUGHT), Some(DRUID));
    assert!(
        distance(
            xz(creature(&state, FOUGHT).position),
            xz(state.players[0].position)
        ) < 300.0,
        "the third marauder fights the Druid at close range"
    );

    // Freed, the second marauder comes for the Druid while the wolf still holds the first.
    fight.cast(ids::WRATH, Some(FOUGHT));
    fight.ticks(second * 2);
    let state = fight.state();
    assert!(distance(xz(creature(&state, ROOTED).position), held_at) > 100.0);
    assert_eq!(top_threat(&state, ROOTED), Some(DRUID));
    fight.assert_wolf_holds();
    fight
}

#[test]
fn the_wolf_holds_one_marauder_roots_hold_another_and_the_druid_fights_the_third() {
    let first = journey();
    // The same commands replay to the same zone and the same events.
    let second = journey();
    assert_eq!(first.state(), second.state());
    assert_eq!(first.events, second.events);
}
