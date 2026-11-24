//! The Redbrand powder watch (#954): one setup on the camp's western powder barrels shows a
//! wasted and an intentional detonation through the existing barrel rules (#288). A Ranger who
//! shoots the powder at once catches only bandit 406, which stands beside it, and spends both
//! barrels. Once that bandit is dead and the barrels stand again, the Ranger walks up to draw
//! 406 and 404 together and shoots the powder from outside its 5 m: both are caught, and 410
//! behind the western tent is untouched. The same commands replay to the same zone.
use crate::support;

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::greyhaven_vale::{self, powder_watch};
use mmorpg_core::{
    CanonicalZoneSnapshot, CreatureAi, CreatureId, CreatureLife, EXPLOSION_RADIUS_UNITS, EntityRef,
    ZoneCommand, ZoneContent, ZoneEvent, ZoneId, ZoneSimulation,
};
use support::{arena, classes};

const PLAYER: u32 = 1;
const RANGER: u8 = 1;
/// Where the Ranger starts: 9.4 m east of the powder, beyond every watchman's aggro radius.
const START: [i32; 2] = [0, -9_000];
/// Where the Ranger draws the pair: 5.5 m east of the powder, just outside its blast.
const DRAW: [i32; 2] = [-250, -9_480];

/// The shipped terrain, colliders and templates with only the powder watch and its barrels, so
/// the rest of the camp does not join in. The seed is pinned: content seeds its zone from its
/// own fingerprint, so a collision edit anywhere in Greyhaven would otherwise move where the
/// bandits idle, and one may stand on [`DRAW`] when the Ranger walks there.
fn content() -> Arc<ZoneContent> {
    let shipped = greyhaven_vale::content();
    let watch: Vec<_> = powder_watch::BARRELS
        .into_iter()
        .chain(powder_watch::PAIR)
        .chain([powder_watch::COVERED])
        .collect();
    let spawns = shipped
        .creature_spawns()
        .iter()
        .filter(|spawn| watch.contains(&spawn.id))
        .cloned()
        .collect();
    Arc::new(
        ZoneContent::new(
            shipped.definition().clone(),
            shipped.areas().clone(),
            shipped.creature_templates().to_vec(),
            spawns,
            Vec::new(),
            shipped.graveyard(),
        )
        .unwrap()
        .with_creature_abilities(greyhaven_vale::units::creature_abilities())
        .unwrap()
        .with_rng_seed(0xb323_f98a_aa51_6cad),
    )
}

/// A level-8 Ranger at [`START`].
fn start() -> ZoneSimulation {
    let content = content();
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), Arc::clone(&content)).unwrap();
    zone.add_player(PLAYER).unwrap();
    let zone = classes::at_level(zone, PLAYER, 8);
    let mut state = zone.snapshot().unwrap();
    let [x, z] = START;
    let ground = content.definition().ground_at(x, z);
    state.players[0].position = mmorpg_core::physics::from_units([x, ground + 100, z]);
    ZoneSimulation::from_snapshot(state, content).unwrap()
}

fn unit(id: CreatureId) -> EntityRef {
    EntityRef::Creature(id)
}

fn record(
    state: &CanonicalZoneSnapshot,
    id: CreatureId,
) -> &mmorpg_core::CanonicalCreatureSnapshot {
    state
        .creatures
        .iter()
        .find(|creature| creature.creature_id == id)
        .unwrap()
}

fn alive(state: &CanonicalZoneSnapshot, id: CreatureId) -> bool {
    record(state, id).life == CreatureLife::Alive
}

/// Whether the Ranger can loose an Aimed Shot now.
fn ready(state: &CanonicalZoneSnapshot) -> bool {
    let abilities = &state.players[0].combat.abilities;
    abilities.global_cooldown == 0
        && abilities.resource >= 30
        && abilities.cast.is_none()
        && !abilities
            .cooldowns
            .iter()
            .any(|cooldown| cooldown.ability == ids::AIMED_SHOT)
}

/// XZ distance, units.
fn apart(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).hypot(a[2] - b[2])
}

/// A bandit's XZ distance to the nearer powder barrel's spot.
fn from_powder(at: [f32; 3]) -> f32 {
    let content = greyhaven_vale::content();
    powder_watch::BARRELS
        .iter()
        .map(|&barrel| {
            let spawn = content.creature_spawn(barrel).unwrap();
            #[allow(clippy::cast_precision_loss)]
            let spot = [spawn.position[0] as f32, 0.0, spawn.position[1] as f32];
            apart(at, spot)
        })
        .fold(f32::MAX, f32::min)
}

/// The zone, every command it was given by tick and every event the player saw.
struct Journey {
    zone: ZoneSimulation,
    commands: Vec<(usize, ZoneCommand)>,
    events: Vec<Vec<ZoneEvent>>,
    sequence: u32,
}

impl Journey {
    fn new() -> Self {
        Self {
            zone: start(),
            commands: Vec::new(),
            events: Vec::new(),
            sequence: 0,
        }
    }

    fn tick(&mut self, command: Option<ZoneCommand>) {
        if let Some(command) = command {
            self.sequence += 1;
            self.commands.push((self.events.len(), command));
            self.zone
                .apply_command(PLAYER, self.sequence, command)
                .unwrap();
        }
        self.zone.advance_tick().unwrap();
        let events = self.zone.snapshot_for_player(PLAYER).unwrap().events;
        self.events.push(events);
    }

    fn state(&self) -> CanonicalZoneSnapshot {
        self.zone.snapshot().unwrap()
    }

    /// Ticks, asking `next` for each tick's command, until `done`, at most `limit` ticks.
    fn until(
        &mut self,
        limit: u32,
        mut next: impl FnMut(&ZoneSimulation) -> Option<ZoneCommand>,
        done: impl Fn(&CanonicalZoneSnapshot) -> bool,
    ) {
        for _ in 0..limit {
            if done(&self.state()) {
                return;
            }
            let command = next(&self.zone);
            self.tick(command);
        }
        panic!("not reached in {limit} ticks");
    }

    /// Aimed Shot at `target`, turned toward it.
    fn shoot(&mut self, target: EntityRef) {
        self.until(600, |_| None, ready);
        let (facing, _) = arena::aim_at(&self.zone, PLAYER, target).unwrap();
        self.tick(Some(ZoneCommand::SelectTarget(Some(target))));
        self.tick(Some(ZoneCommand::UseAbility {
            ability: ids::AIMED_SHOT.get(),
            target: Some(target),
            facing: Some(facing),
        }));
    }

    /// Shoots the first barrel and ticks until the powder has gone up: the health each of the
    /// watch lost to it.
    fn detonate(&mut self) -> [u32; 3] {
        let [first, second] = powder_watch::BARRELS;
        let watch = [
            powder_watch::PAIR[0],
            powder_watch::PAIR[1],
            powder_watch::COVERED,
        ];
        let before = self.state();
        self.shoot(unit(first));
        self.until(
            240,
            |_| None,
            |state| !alive(state, first) && !alive(state, second),
        );
        let after = self.state();
        watch.map(|id| record(&before, id).health - record(&after, id).health)
    }
}

/// Moves the player toward `to` each tick until within 0.3 m.
fn walk_to(journey: &mut Journey, to: [i32; 2]) {
    #[allow(clippy::cast_precision_loss)]
    let goal = [to[0] as f32, 0.0, to[1] as f32];
    journey.until(
        1_200,
        |zone| {
            let me = zone.snapshot().unwrap().players[0].position;
            #[allow(clippy::cast_possible_truncation)]
            let facing = mmorpg_core::trig::yaw_from_vector(
                (goal[0] - me[0]) as i32,
                (goal[2] - me[2]) as i32,
            )
            .unwrap_or(0);
            Some(arena::walk(facing))
        },
        |state| apart(state.players[0].position, goal) < 30.,
    );
    journey.tick(Some(arena::stand(0)));
}

fn journey() -> Journey {
    let mut j = Journey::new();
    let [beside, beyond] = powder_watch::PAIR;
    let covered = powder_watch::COVERED;
    j.tick(Some(ZoneCommand::ChooseClass {
        class: RANGER,
        sex: 0,
    }));
    j.tick(None);

    // Too early: the powder goes up with only the bandit beside it in reach.
    let lost = j.detonate();
    assert!(lost[0] > 0, "the bandit beside the powder is caught");
    assert_eq!(lost[1..], [0, 0], "the others are not");
    let state = j.state();
    assert_eq!(state.debris.len(), 16, "both barrels went up");
    assert!(matches!(record(&state, beyond).ai, CreatureAi::Idle { .. }));

    // That bandit charges; the Ranger shoots and fights it down.
    j.until(
        7_200,
        |zone| {
            let ready = ready(&zone.snapshot().unwrap());
            match arena::aim_at(zone, PLAYER, unit(beside)) {
                Some((facing, _)) if ready => Some(ZoneCommand::UseAbility {
                    ability: ids::AIMED_SHOT.get(),
                    target: Some(unit(beside)),
                    facing: Some(facing),
                }),
                _ => arena::swing_at(zone, PLAYER, unit(beside)),
            }
        },
        |state| !alive(state, beside),
    );

    // The reset: the bandit stands at its post again after a minute, the barrels after five.
    j.until(
        19_200,
        |_| None,
        |state| {
            alive(state, beside)
                && powder_watch::BARRELS.iter().all(|&id| alive(state, id))
                && state.debris.is_empty()
        },
    );
    let state = j.state();
    #[allow(clippy::cast_precision_loss)]
    let post = content()
        .creature_spawn(beside)
        .unwrap()
        .position
        .map(|axis| axis as f32);
    let at = record(&state, beside).position;
    assert!(apart(at, [post[0], 0., post[1]]) < 50., "back at its post");
    assert!(matches!(record(&state, beside).ai, CreatureAi::Idle { .. }));

    // Intentional: the Ranger walks up beside the powder, where the bandit beside it notices
    // them, and draws the other with an arrow; both come for them.
    walk_to(&mut j, DRAW);
    j.shoot(unit(beyond));
    j.until(
        600,
        |_| None,
        |state| {
            let me = state.players[0].position;
            [beside, beyond].iter().all(|&id| {
                let creature = record(state, id);
                creature.ai == CreatureAi::Engaged && apart(me, creature.position) < 260.
            })
        },
    );
    // Both stand within the powder's 5 m and the Ranger outside it.
    let state = j.state();
    #[allow(clippy::cast_precision_loss)]
    let radius = EXPLOSION_RADIUS_UNITS as f32;
    for id in [beside, beyond] {
        let reach = from_powder(record(&state, id).position);
        assert!(reach < radius, "{} at {reach}", id.get());
    }
    let reach = from_powder(state.players[0].position);
    assert!(reach > radius, "the Ranger stands clear at {reach}");
    let shot = j.events.len();
    let lost = j.detonate();
    assert!(
        lost[0] > 0 && lost[1] > 0,
        "both bandits are caught: {lost:?}"
    );
    assert_eq!(lost[2], 0, "the bandit behind the tent is not");
    let state = j.state();
    assert_ne!(record(&state, covered).ai, CreatureAi::Engaged);
    assert!(
        !j.events[shot..].iter().flatten().any(|event| matches!(
            *event,
            ZoneEvent::DamageTaken { source, .. } if powder_watch::BARRELS.iter().any(|&b| source == unit(b))
        )),
        "the blast never reaches the Ranger"
    );
    j
}

#[test]
fn early_powder_catches_one_drawn_pair_both_and_the_covered_bandit_none_and_replays_exactly() {
    let played = journey();
    let mut replay = Journey::new();
    let mut commands = played.commands.iter().peekable();
    for tick in 0..played.events.len() {
        let command = commands
            .next_if(|(at, _)| *at == tick)
            .map(|&(_, command)| command);
        replay.tick(command);
    }
    assert_eq!(replay.events, played.events);
    assert_eq!(replay.state(), played.state());
}
