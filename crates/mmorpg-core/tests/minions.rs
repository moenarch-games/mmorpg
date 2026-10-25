//! Necromancer minions (#295, Atlas 5.2.84) through the public zone API: raising skeletons,
//! their formation, group orders, fights with kill credit for the summoner, crumbling and
//! recovery.
mod support;

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::golden::t30;
use mmorpg_core::{
    CreatureId, EntityKind, EntityRef, ErrorCode, MAX_MINIONS_PER_SUMMONER, MinionId, MinionOrder,
    ZoneCommand, ZoneContent, ZoneEvent, ZoneId, ZoneSimulation,
};
use support::arena::{self, BOAR, WOLF};
use support::classes;

const WOLF_UNIT: EntityRef = EntityRef::Creature(CreatureId::new(1));

struct Summoner {
    zone: ZoneSimulation,
    sequence: u32,
    events: Vec<ZoneEvent>,
}

impl Summoner {
    /// A level-6 Necromancer at the origin facing north.
    fn new(content: Arc<ZoneContent>) -> Self {
        let mut zone = ZoneSimulation::with_content(ZoneId::new(9), content).unwrap();
        zone.add_player(1).unwrap();
        let mut summoner = Self {
            zone: classes::at_level(zone, 1, 6),
            sequence: 0,
            events: Vec::new(),
        };
        summoner.send(ZoneCommand::ChooseClass { class: 3, sex: 1 });
        summoner.tick();
        summoner
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

    /// Casts Raise Skeleton and waits out its cast and the global cooldown.
    fn raise(&mut self) {
        self.send(ZoneCommand::UseAbility {
            ability: ids::RAISE_SKELETON.get(),
            target: None,
        });
        self.ticks(t30(150));
    }

    fn order(&mut self, order: MinionOrder) {
        self.send(ZoneCommand::CommandMinions(order));
        self.tick();
    }

    fn minions(&self) -> Vec<mmorpg_core::CanonicalMinion> {
        self.zone.snapshot().unwrap().minions
    }

    /// Projected minions: kind, ID and XZ position.
    fn seen(&self) -> Vec<(u32, [i32; 2])> {
        self.zone
            .snapshot_for_player(1)
            .unwrap()
            .entities
            .iter()
            .filter(|entity| entity.kind == EntityKind::Minion)
            .map(|entity| (entity.id, [entity.position[0], entity.position[2]]))
            .collect()
    }

    fn errors(&self) -> Vec<ErrorCode> {
        self.events
            .iter()
            .filter_map(|event| match event {
                ZoneEvent::Error { code, .. } => Some(*code),
                _ => None,
            })
            .collect()
    }
}

fn distance([x, z]: [i32; 2]) -> f64 {
    f64::from(x).hypot(f64::from(z))
}

#[test]
fn skeletons_rise_form_up_behind_the_summoner_and_the_oldest_gives_way() {
    let content = classes::arena(vec![arena::boar()], vec![arena::spawn(1, BOAR, [0, 4_000])]);
    let mut summoner = Summoner::new(content);
    summoner.order(MinionOrder::Follow);
    assert_eq!(summoner.errors(), [ErrorCode::NoMinions]);

    for _ in 0..=MAX_MINIONS_PER_SUMMONER {
        summoner.raise();
    }
    let minions = summoner.minions();
    assert_eq!(minions.len(), MAX_MINIONS_PER_SUMMONER);
    assert_eq!(
        minions.first().map(|record| record.minion.id),
        Some(MinionId::new(2)),
        "the first skeleton crumbled for the fifth"
    );
    assert!(summoner.events.iter().any(|event| matches!(
        event,
        ZoneEvent::Died { entity: EntityRef::Minion(id), .. } if id.get() == 1
    )));
    summoner.ticks(t30(120));
    let seen = summoner.seen();
    assert_eq!(seen.len(), MAX_MINIONS_PER_SUMMONER);
    for (_, at) in &seen {
        // Behind (south of) the summoner, within a few metres, and apart from one another.
        assert!(at[1] < 0, "{seen:?}");
        assert!(distance(*at) < 350.0, "{seen:?}");
    }
    for (index, (_, left)) in seen.iter().enumerate() {
        for (_, right) in &seen[index + 1..] {
            assert!(
                distance([left[0] - right[0], left[1] - right[1]]) > 40.0,
                "{seen:?}"
            );
        }
    }
}

#[test]
fn ordered_skeletons_kill_a_wolf_for_their_summoner_then_retreat_and_follow() {
    // A level-6 wolf 20 m north, beyond its aggro radius, that bites weakly.
    let wolf = mmorpg_core::CreatureTemplate {
        min_level: 6,
        max_level: 6,
        ..arena::wolf(120, [1, 1])
    };
    let content = classes::arena(vec![wolf], vec![arena::spawn(1, WOLF, [0, 2_000])]);
    let mut summoner = Summoner::new(content);
    summoner.raise();
    summoner.raise();
    let experience = summoner.zone.snapshot().unwrap().players[0]
        .combat
        .experience;

    summoner.order(MinionOrder::Attack(WOLF_UNIT));
    assert!(summoner.errors().is_empty(), "{:?}", summoner.errors());
    let mut fought = false;
    for _ in 0..t30(1_200) {
        summoner.tick();
        let wolf = &summoner.zone.snapshot().unwrap().creatures[0];
        fought |= wolf
            .threat
            .iter()
            .any(|entry| matches!(entry.entity, EntityRef::Minion(_)));
        if wolf.health == 0 {
            break;
        }
    }
    assert!(fought, "the wolf turned on the skeletons");
    let snapshot = summoner.zone.snapshot().unwrap();
    assert_eq!(
        snapshot.creatures[0].health, 0,
        "the skeletons killed the wolf"
    );
    assert_eq!(snapshot.creatures[0].tapped_by, Some(1));
    assert!(
        snapshot.players[0].combat.experience > experience,
        "the summoner earned the kill"
    );
    assert!(summoner.events.iter().any(|event| matches!(
        event,
        ZoneEvent::Died { entity, killer: Some(EntityRef::Minion(_)) } if *entity == WOLF_UNIT
    )));

    summoner.order(MinionOrder::Retreat);
    summoner.ticks(t30(150));
    for record in summoner.minions() {
        assert_eq!(
            record.minion.order,
            MinionOrder::Follow,
            "back with the summoner"
        );
        assert_eq!(record.minion.target, None);
    }
    for (_, at) in summoner.seen() {
        assert!(distance(at) < 400.0);
    }
}

#[test]
fn orders_are_checked_points_are_held_and_skeletons_leave_with_their_summoner() {
    let content = classes::arena(
        vec![arena::wolf(400, [1, 1])],
        vec![arena::spawn(1, WOLF, [0, 4_000])],
    );
    let mut summoner = Summoner::new(content);
    summoner.raise();
    summoner.order(MinionOrder::Attack(EntityRef::Player(1)));
    summoner.order(MinionOrder::Attack(WOLF_UNIT));
    summoner.order(MinionOrder::Defend([0, 5_000]));
    assert_eq!(
        summoner.errors(),
        [
            ErrorCode::InvalidTarget,
            ErrorCode::OutOfRange,
            ErrorCode::OutOfRange
        ]
    );
    summoner.order(MinionOrder::Defend([500, 0]));
    summoner.ticks(t30(120));
    let [(_, at)] = summoner.seen()[..] else {
        panic!("one skeleton");
    };
    assert!(
        distance([at[0] - 500, at[1]]) < 300.0,
        "it holds the point: {at:?}"
    );

    // Recovery continues exactly.
    let mut restored = ZoneSimulation::from_snapshot(
        summoner.zone.snapshot().unwrap(),
        Arc::clone(summoner.zone.content()),
    )
    .unwrap();
    for _ in 0..t30(60) {
        summoner.zone.advance_tick().unwrap();
        restored.advance_tick().unwrap();
    }
    assert_eq!(
        summoner.zone.snapshot().unwrap(),
        restored.snapshot().unwrap()
    );

    assert!(summoner.zone.remove_player(1));
    assert!(summoner.zone.snapshot().unwrap().minions.is_empty());
}
