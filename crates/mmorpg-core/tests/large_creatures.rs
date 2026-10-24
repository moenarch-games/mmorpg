//! Large creatures (#297): an ogre's big body blocks smaller units, it shoves a smaller unit
//! standing in its way (a forced move with immunity, walls stop it) and its Stomp hurts and
//! knocks back the players around it.
mod support;

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::{
    CreatureBehaviour, CreatureFamily, CreatureId, CreatureTemplate, CreatureTemplateId,
    SHOVE_UNITS, ZoneCommand, ZoneContent, ZoneEvent, ZoneId, ZoneSimulation,
};
use support::arena::{self, BOAR};

/// Spawn slots lie along +X and +Z, so the tests play out toward −Z.
const SOUTHWARD: u16 = 32_768;

const OGRE: CreatureTemplateId = CreatureTemplateId::new(3);
const OGRE_BODY: [f32; 2] = [70.0, 140.0];
const BOAR_BODY: [f32; 2] = [40.0, 45.0];
const PLAYER_BODY: [f32; 2] = [30.0, 90.0];

fn ogre(behaviour: CreatureBehaviour) -> CreatureTemplate {
    CreatureTemplate {
        id: OGRE,
        name: "Test Ogre".into(),
        family: CreatureFamily::Ogre,
        behaviour,
        min_level: 5,
        max_level: 5,
        health: 5_000,
        damage: [1, 1],
        swing_ticks: mmorpg_core::golden::t30(84) as _,
        half_extents: [70, 140, 70],
        ..arena::wolf(5_000, [1, 1])
    }
}

/// Flat ground with an ogre (creature 1) and, optionally, a sturdy neutral boar (creature 2).
fn content(
    behaviour: CreatureBehaviour,
    ogre_at: [i32; 2],
    boar_at: Option<[i32; 2]>,
) -> Arc<ZoneContent> {
    let mut spawns = vec![arena::spawn(1, OGRE, ogre_at)];
    spawns.extend(boar_at.map(|at| arena::spawn(2, BOAR, at)));
    let content = arena::arena(vec![ogre(behaviour), arena::boar()], spawns, Vec::new());
    Arc::new(
        Arc::unwrap_or_clone(content)
            .with_creature_abilities(
                mmorpg_core::ABILITY_CATALOG_REVISION,
                vec![(OGRE, ids::STOMP)],
            )
            .unwrap(),
    )
}

struct Run {
    zone: ZoneSimulation,
    sequence: u32,
    events: Vec<ZoneEvent>,
}

impl Run {
    fn new(content: Arc<ZoneContent>, players: usize) -> Self {
        let mut zone = ZoneSimulation::with_content(ZoneId::new(9), content).unwrap();
        for player in 1..=players {
            zone.add_player(u32::try_from(player).unwrap()).unwrap();
        }
        Self {
            zone,
            sequence: 0,
            events: Vec::new(),
        }
    }

    /// Moves player `id` to feet `[x, z]` before the run starts.
    fn place(&mut self, id: u32, at: [f32; 2]) {
        let mut state = self.zone.snapshot().unwrap();
        let player = state
            .players
            .iter_mut()
            .find(|player| player.player_id == id)
            .unwrap();
        player.position = [at[0], 90.0, at[1]];
        self.zone = ZoneSimulation::from_snapshot(state, Arc::clone(self.zone.content())).unwrap();
    }

    fn send(&mut self, player: u32, command: ZoneCommand) {
        self.sequence += 1;
        self.zone
            .apply_command(player, self.sequence, command)
            .unwrap();
    }

    fn tick(&mut self) {
        self.zone.advance_tick().unwrap();
        self.events
            .extend(self.zone.snapshot_for_player(1).unwrap().events);
    }

    fn creature_centre(&self, id: u32) -> [f32; 3] {
        let state = self.zone.snapshot().unwrap();
        state
            .creatures
            .iter()
            .find(|creature| creature.creature_id == CreatureId::new(id))
            .unwrap()
            .position
    }

    fn creature_at(&self, id: u32) -> [f32; 2] {
        let [x, _, z] = self.creature_centre(id);
        [x, z]
    }

    fn player(&self, id: u32) -> mmorpg_core::CanonicalPlayerSnapshot {
        self.zone
            .snapshot()
            .unwrap()
            .players
            .into_iter()
            .find(|player| player.player_id == id)
            .unwrap()
    }

    fn player_at(&self, id: u32) -> [f32; 2] {
        let position = self.player(id).position;
        [position[0], position[2]]
    }
}

fn distance(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// The gap between two upright capsules (`[radius, half height]`) centred on `a` and `b`.
fn gap(a: [f32; 3], a_body: [f32; 2], b: [f32; 3], b_body: [f32; 2]) -> f32 {
    let segment = |[radius, half]: [f32; 2]| (half - radius).max(0.0);
    let vertical = ((a[1] - b[1]).abs() - segment(a_body) - segment(b_body)).max(0.0);
    (a[0] - b[0]).hypot(a[2] - b[2]).hypot(vertical) - a_body[0] - b_body[0]
}

#[test]
fn a_charging_ogre_shoves_a_smaller_creature_out_of_its_way() {
    // The ogre aggroes the player 10 m north and runs at it through the boar between them.
    let mut run = Run::new(
        content(CreatureBehaviour::Aggressive, [0, -1_000], Some([0, -500])),
        1,
    );
    let boar = run.creature_at(2);
    let mut shoved = false;
    for _ in 0..mmorpg_core::golden::t30(60) {
        run.tick();
        let (ogre, now) = (run.creature_centre(1), run.creature_centre(2));
        assert!(
            gap(ogre, OGRE_BODY, now, BOAR_BODY) >= 0.0,
            "the boar never ends up inside the ogre: {ogre:?} {now:?}"
        );
        shoved |= distance(run.creature_at(2), boar) >= SHOVE_UNITS as f32 * 0.75;
    }
    assert!(
        shoved,
        "the boar was shoved from {boar:?} to {:?}",
        run.creature_at(2)
    );
    // Aside, off the ogre's path: the ogre went on past it toward its target.
    let (ogre, shoved_to) = (run.creature_at(1), run.creature_at(2));
    assert!(
        shoved_to[0].abs() >= SHOVE_UNITS as f32 * 0.75,
        "{shoved_to:?}"
    );
    assert!(ogre[1] > shoved_to[1] + 100.0, "{ogre:?} {shoved_to:?}");
}

#[test]
fn an_ogre_body_blocks_a_player_walking_into_it() {
    // A neutral ogre standing still: the player walks straight south into it.
    let mut run = Run::new(content(CreatureBehaviour::Neutral, [0, -400], None), 1);
    run.send(1, arena::walk(SOUTHWARD));
    for _ in 0..mmorpg_core::golden::t30(90) {
        run.tick();
        let gap = gap(
            run.creature_centre(1),
            OGRE_BODY,
            run.player(1).position,
            PLAYER_BODY,
        );
        assert!(gap >= 0.0, "the player stays outside the ogre: {gap}");
    }
    // Shoving is the ogre's: a player walking into it does not move it.
    assert_eq!(run.creature_at(1), [0.0, -400.0]);
}

#[test]
fn stomp_hurts_and_knocks_back_the_players_around_the_ogre() {
    // Player 1 stands in reach, player 2 well outside the stomp.
    let mut run = Run::new(content(CreatureBehaviour::Aggressive, [0, -400], None), 2);
    run.place(1, [0.0, -200.0]);
    run.place(2, [900.0, 400.0]);
    let start = run.player(1).combat.health;
    let mut stomped_at = None;
    for tick in 0..150 {
        run.tick();
        let used = run.events.iter().any(|event| {
            matches!(event, ZoneEvent::AbilityUsed { ability, .. } if *ability == ids::STOMP)
        });
        if used && stomped_at.is_none() {
            stomped_at = Some((tick, run.player_at(1)));
        }
    }
    let (_, before) = stomped_at.expect("the ogre stomped");
    assert!(
        run.player(1).combat.health < start,
        "the stomp hurt player 1"
    );
    assert!(
        distance(run.player_at(1), run.creature_at(1))
            > distance(before, run.creature_at(1)) + 100.0,
        "and knocked it back: {before:?} -> {:?}",
        run.player_at(1)
    );
    assert_eq!(
        run.player(2).combat.health,
        start,
        "player 2 stood outside the stomp"
    );
    assert_eq!(run.player_at(2), [900.0, 400.0]);
}

#[test]
fn ogres_are_large_and_ordinary_creatures_are_not() {
    let content = mmorpg_core::greyhaven_vale::content();
    let large: Vec<_> = content
        .creature_templates()
        .iter()
        .filter(|template| template.is_large())
        .map(|template| template.name.as_str())
        .collect();
    assert_eq!(large, ["Hill Ogre"]);
    assert_eq!(
        content.creature_ability(mmorpg_core::greyhaven_vale::units::templates::HILL_OGRE),
        Some(ids::STOMP)
    );
}
