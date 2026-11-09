//! Movement spells and surface hazards (#294): Blink, Heroic Leap, Grappling Hook, ice, tar and
//! a canonical restore in flight.
mod support;

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::{
    AbilityId, CreatureSpawn, CreatureTemplate, EntityRef, ErrorCode, SpawnGrid, StaticCollider,
    SurfaceKind, SurfacePatch, ZoneCommand, ZoneContent, ZoneDefinition, ZoneId, ZoneSimulation,
};
use support::arena::{self, BOAR};

const WARDEN: u8 = 0;
const RANGER: u8 = 1;
const ARCANIST: u8 = 2;
/// Forward in these tests is toward -Z, out of the spawn grid that fills +Z.
const NORTH: u16 = 0x8000;

/// Flat ground with `walls`, a boar for each `(x, z)` spot and `surfaces`.
fn content(
    walls: Vec<StaticCollider>,
    boars: &[[i32; 2]],
    surfaces: Vec<SurfacePatch>,
) -> Arc<ZoneContent> {
    let mut colliders = vec![StaticCollider {
        id: 1,
        position: [0, -50, 0],
        half_extents: [10_000, 50, 10_000],
    }];
    colliders.extend(walls);
    let definition = ZoneDefinition::with_spawn_grid(
        mmorpg_core::STANDARD_GRAVITY,
        SpawnGrid {
            origin: [0, 0],
            columns: 32,
            spacing: 100,
        },
        colliders,
    )
    .unwrap();
    let spawns: Vec<CreatureSpawn> = boars
        .iter()
        .enumerate()
        .map(|(index, &at)| arena::spawn(index as u32 + 1, BOAR, [at[0], -at[1]]))
        .collect();
    let content = ZoneContent::new(
        definition,
        mmorpg_core::ZoneAreas::default(),
        vec![CreatureTemplate {
            health: 500,
            ..arena::boar()
        }],
        spawns,
        Vec::new(),
        arena::GRAVEYARD,
    )
    .unwrap();
    Arc::new(
        content
            .with_creature_abilities(Vec::new())
            .unwrap()
            .with_surfaces(surfaces)
            .unwrap(),
    )
}

fn wall(id: u32, position: [i32; 3], half: [i32; 3]) -> StaticCollider {
    StaticCollider {
        id,
        position: [position[0], position[1], -position[2]],
        half_extents: half,
    }
}

struct Hero {
    zone: ZoneSimulation,
    /// The next sequence per player id.
    sequence: [u32; 3],
}

impl Hero {
    fn new(content: Arc<ZoneContent>, classes: &[u8]) -> Self {
        let mut zone = ZoneSimulation::with_content(ZoneId::new(9), content).unwrap();
        for player in 1..=classes.len() as u32 {
            zone.add_player(player).unwrap();
            zone = support::classes::at_level(zone, player, 8);
        }
        let mut hero = Self {
            zone,
            sequence: [0; 3],
        };
        for (index, &class) in classes.iter().enumerate() {
            let player = index as u32 + 1;
            hero.send(player, ZoneCommand::ChooseClass { class, sex: 0 });
            hero.send(player, arena::stand(NORTH));
        }
        hero.ticks(1);
        hero
    }

    fn send(&mut self, player: u32, command: ZoneCommand) {
        let slot = &mut self.sequence[usize::try_from(player).unwrap()];
        *slot += 1;
        let sequence = *slot;
        self.zone.apply_command(player, sequence, command).unwrap();
    }

    fn ticks(&mut self, count: u32) {
        for _ in 0..count {
            self.zone.advance_tick().unwrap();
        }
    }

    /// Waits out the global cooldown, uses `ability` and returns the refusals of that tick.
    fn cast(&mut self, player: u32, ability: AbilityId) -> Vec<ErrorCode> {
        self.ticks(u32::from(mmorpg_core::GLOBAL_COOLDOWN_TICKS) + 1);
        self.use_now(player, ability)
    }

    /// Uses `ability` this tick and returns the refusals of that tick.
    fn use_now(&mut self, player: u32, ability: AbilityId) -> Vec<ErrorCode> {
        self.send(
            player,
            ZoneCommand::UseAbility {
                ability: ability.get(),
                target: None,
            },
        );
        self.zone.advance_tick().unwrap();
        self.zone
            .snapshot_for_player(player)
            .unwrap()
            .events
            .iter()
            .filter_map(|event| match event {
                mmorpg_core::ZoneEvent::Error { code, .. } => Some(*code),
                _ => None,
            })
            .collect()
    }

    fn position(&self, player: u32) -> [f32; 3] {
        let state = self.zone.snapshot().unwrap();
        let [x, y, z] = state
            .players
            .iter()
            .find(|candidate| candidate.player_id == player)
            .unwrap()
            .position;
        [x, y, -z]
    }

    fn boar(&self, id: u32) -> [f32; 3] {
        let state = self.zone.snapshot().unwrap();
        let [x, y, z] = state
            .creatures
            .iter()
            .find(|creature| creature.creature_id == mmorpg_core::CreatureId::new(id))
            .unwrap()
            .position;
        [x, y, -z]
    }

    fn resource(&self, player: u32) -> u16 {
        self.zone
            .snapshot_for_player(player)
            .unwrap()
            .viewer
            .resource
            .unwrap()
            .value
    }

    fn cooling(&self, player: u32, ability: AbilityId) -> bool {
        self.zone
            .snapshot()
            .unwrap()
            .players
            .iter()
            .find(|candidate| candidate.player_id == player)
            .unwrap()
            .combat
            .abilities
            .cooldowns
            .iter()
            .any(|cooldown| cooldown.ability == ability)
    }
}

#[test]
fn blink_lands_ten_metres_ahead_in_free_space() {
    let mut hero = Hero::new(content(Vec::new(), &[], Vec::new()), &[ARCANIST]);
    assert_eq!(hero.cast(1, ids::BLINK), []);
    let at = hero.position(1);
    assert!((at[2] - 1_000.0).abs() < 2.0, "{at:?}");
    hero.ticks(5);
    let settled = hero.position(1);
    assert!(hero.cooling(1, ids::BLINK));
    assert!(
        (settled[1] - 90.0).abs() < 3.0,
        "on the ground: {settled:?}"
    );
}

#[test]
fn blink_lands_short_of_a_wall_and_never_past_it() {
    let walled = vec![wall(2, [0, 150, 700], [400, 150, 20])];
    let mut hero = Hero::new(content(walled, &[], Vec::new()), &[ARCANIST]);
    assert_eq!(hero.cast(1, ids::BLINK), []);
    let at = hero.position(1);
    assert!(at[2] > 300.0 && at[2] < 680.0, "short of the wall: {at:?}");
}

#[test]
fn blink_is_refused_free_when_no_landing_is_far_enough() {
    let walled = vec![wall(2, [0, 150, 180], [400, 150, 20])];
    let mut hero = Hero::new(content(walled, &[], Vec::new()), &[ARCANIST]);
    let mana = hero.resource(1);
    assert_eq!(hero.cast(1, ids::BLINK), [ErrorCode::NoLanding]);
    assert_eq!(hero.resource(1), mana, "nothing was paid");
    assert!(!hero.cooling(1, ids::BLINK));
    let at = hero.position(1);
    assert!(at[2].abs() < 5.0, "stayed: {at:?}");
}

#[test]
fn blink_does_not_cross_a_summoned_wall() {
    let mut hero = Hero::new(content(Vec::new(), &[], Vec::new()), &[ARCANIST]);
    assert_eq!(hero.cast(1, ids::ICE_WALL), []);
    assert_eq!(hero.cast(1, ids::BLINK), []);
    let at = hero.position(1);
    assert!(at[2] > 150.0 && at[2] < 400.0, "short of the wall: {at:?}");
}

#[test]
fn heroic_leap_lands_ahead_with_falloff_knockback() {
    let boars = [[200, 800], [380, 780]];
    let mut hero = Hero::new(content(Vec::new(), &boars, Vec::new()), &[WARDEN]);
    assert_eq!(hero.cast(1, ids::HEROIC_LEAP), []);
    assert!(hero.cooling(1, ids::HEROIC_LEAP));
    let before = [hero.boar(1), hero.boar(2)];
    // Up in the air, then down and landing within the flight's ticks.
    hero.ticks(20);
    assert!(
        hero.position(1)[1] > 120.0,
        "airborne: {:?}",
        hero.position(1)
    );
    hero.ticks(70);
    let landed = hero.position(1);
    assert!((landed[2] - 800.0).abs() < 90.0, "{landed:?}");
    assert!((landed[1] - 90.0).abs() < 5.0, "{landed:?}");
    // Knockbacks ran out; the nearer boar was thrown farther.
    hero.ticks(18);
    let (near, far) = (hero.boar(1), hero.boar(2));
    let moved = |a: [f32; 3], b: [f32; 3]| (a[0] - b[0]).hypot(a[2] - b[2]);
    let near_push = moved(near, before[0]);
    let far_push = moved(far, before[1]);
    assert!(
        near_push > far_push && far_push > 60.0,
        "{near_push} {far_push}"
    );
}

/// Selects boar 1 for `player`.
fn select_boar(hero: &mut Hero, player: u32) {
    hero.send(
        player,
        ZoneCommand::SelectTarget(Some(EntityRef::Creature(mmorpg_core::CreatureId::new(1)))),
    );
    hero.ticks(1);
}

#[test]
fn grappling_hook_pulls_the_target_to_melee_reach() {
    let boars = [[0, 1_500]];
    let mut hero = Hero::new(content(Vec::new(), &boars, Vec::new()), &[RANGER]);
    select_boar(&mut hero, 1);
    assert_eq!(hero.cast(1, ids::GRAPPLING_HOOK), []);
    hero.ticks(90);
    let at = hero.boar(1);
    assert!(at[2] > 150.0 && at[2] < 320.0, "pulled in: {at:?}");
}

#[test]
fn grappling_hook_is_stopped_by_a_low_wall_and_honours_immunity() {
    // A beam above the sight line (a boar's centre is 45 high), low enough to catch its body.
    let beam = vec![wall(2, [0, 190, 700], [300, 110, 20])];
    let boars = [[0, 1_500]];
    let mut hero = Hero::new(content(beam, &boars, Vec::new()), &[RANGER, RANGER]);
    select_boar(&mut hero, 1);
    select_boar(&mut hero, 2);
    assert_eq!(hero.cast(1, ids::GRAPPLING_HOOK), []);
    // Immediately after, the target is mid-pull and cannot be hooked again.
    let focus = hero.resource(2);
    assert_eq!(
        hero.use_now(2, ids::GRAPPLING_HOOK),
        [ErrorCode::MovementImmune]
    );
    assert_eq!(hero.resource(2), focus, "nothing was paid");
    hero.ticks(90);
    let at = hero.boar(1);
    assert!(at[2] > 700.0, "the wall stopped it: {at:?}");
}

/// Runs for `ticks` ticks along +Z with `player`'s movement.
fn run(hero: &mut Hero, player: u32, ticks: u32) {
    hero.send(player, arena::walk(NORTH));
    hero.ticks(ticks);
}

fn stop(hero: &mut Hero, player: u32) {
    hero.send(player, arena::stand(NORTH));
}

fn hazard(kind: SurfaceKind) -> Vec<SurfacePatch> {
    vec![SurfacePatch {
        id: 1,
        kind,
        centre: [0, 0],
        radii: [5_000, 5_000],
        height: [-60, 60],
    }]
}

#[test]
fn tar_slows_a_runner() {
    let mut clear = Hero::new(content(Vec::new(), &[], Vec::new()), &[WARDEN]);
    let mut tarred = Hero::new(
        content(Vec::new(), &[], hazard(SurfaceKind::Tar)),
        &[WARDEN],
    );
    run(&mut clear, 1, 60);
    run(&mut tarred, 1, 60);
    let (clear_z, tarred_z) = (clear.position(1)[2], tarred.position(1)[2]);
    assert!(
        tarred_z > 0.0 && tarred_z < clear_z * 0.55,
        "{tarred_z} vs {clear_z}"
    );
}

#[test]
fn ice_accelerates_slowly_and_lets_a_stopped_runner_slide() {
    let mut clear = Hero::new(content(Vec::new(), &[], Vec::new()), &[WARDEN]);
    let mut icy = Hero::new(
        content(Vec::new(), &[], hazard(SurfaceKind::Ice)),
        &[WARDEN],
    );
    run(&mut clear, 1, 20);
    run(&mut icy, 1, 20);
    assert!(
        icy.position(1)[2] < clear.position(1)[2] * 0.7,
        "runs up slowly"
    );
    // Let both reach speed, then stop: the icy runner keeps sliding.
    clear.ticks(120);
    icy.ticks(120);
    stop(&mut clear, 1);
    stop(&mut icy, 1);
    let (clear_stop, icy_stop) = (clear.position(1)[2], icy.position(1)[2]);
    clear.ticks(30);
    icy.ticks(30);
    assert!(
        clear.position(1)[2] - clear_stop < 5.0,
        "a runner on dry ground stops at once"
    );
    assert!(icy.position(1)[2] - icy_stop > 40.0, "ice lets it slide");
}

#[test]
fn a_leap_in_flight_and_a_blink_cooldown_survive_a_canonical_restore() {
    let boars = [[200, 800]];
    let content_a = content(Vec::new(), &boars, hazard(SurfaceKind::Ice));
    let mut hero = Hero::new(Arc::clone(&content_a), &[WARDEN]);
    assert_eq!(hero.cast(1, ids::HEROIC_LEAP), []);
    hero.ticks(15);
    let mut restored = ZoneSimulation::from_snapshot(hero.zone.snapshot().unwrap(), content_a)
        .expect("a mid-flight state restores");
    for _ in 0..100 {
        hero.zone.advance_tick().unwrap();
        restored.advance_tick().unwrap();
    }
    let original = hero.zone.snapshot().unwrap();
    let copy = restored.snapshot().unwrap();
    assert_eq!(original.players[0].position, copy.players[0].position);
    assert_eq!(original.creatures[0].position, copy.creatures[0].position);
    assert_eq!(original.forced, copy.forced);
}

#[test]
fn greyhaven_has_its_authored_hazards() {
    let content = mmorpg_core::greyhaven_vale::content();
    let kinds: Vec<_> = content.surfaces().iter().map(|patch| patch.kind).collect();
    assert_eq!(kinds, [SurfaceKind::Ice, SurfaceKind::Tar]);
}
