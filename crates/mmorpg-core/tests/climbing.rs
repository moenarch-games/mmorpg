//! Climbing and free ledge mantling (#490, Atlas 5.1.40) on Greyhaven's own physics: a player
//! climbs a highland crag and a tower's ladder, mantles the mill's loading dock and jumps up to
//! grab the stronghold's palisade, takes the climbing route up the highlands' escarpment, and is
//! refused by a sheer crag, a too-high wall and an obstructed ledge.

use std::sync::Arc;

use mmorpg_core::greyhaven_vale::{self, highlands, ids};
use mmorpg_core::{
    StaticCollider, Traversal, ZoneAreas, ZoneCommand, ZoneContent, ZoneDefinition, ZoneId,
    ZoneSimulation,
};

/// Yaw 32 768 faces −Z, north on the map.
const NORTH: u16 = 32_768;

struct Player {
    zone: ZoneSimulation,
    sequence: u32,
}

impl Player {
    fn in_zone(zone: ZoneSimulation, feet: [i32; 2]) -> Self {
        let mut state = zone.snapshot().unwrap();
        let ground = zone.content().definition().ground_at(feet[0], feet[1]);
        state.players[0].position =
            mmorpg_core::physics::from_units([feet[0], ground + 100, feet[1]]);
        let zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
        let mut player = Self { zone, sequence: 0 };
        player.ticks(30);
        player
    }

    /// A player standing in Greyhaven at `feet`.
    fn at(feet: [i32; 2]) -> Self {
        let mut zone =
            ZoneSimulation::with_content(ZoneId::new(1), greyhaven_vale::content()).unwrap();
        zone.add_player(1).unwrap();
        Self::in_zone(zone, feet)
    }

    fn command(&mut self, command: ZoneCommand) {
        self.sequence += 1;
        self.zone.apply_command(1, self.sequence, command).unwrap();
    }

    fn hold(&mut self, forward: i8, facing: u16) {
        self.command(ZoneCommand::Move {
            forward,
            strafe: 0,
            facing,
        });
    }

    fn jump(&mut self) {
        self.command(ZoneCommand::Jump);
    }

    fn ticks(&mut self, ticks: u64) {
        for _ in 0..ticks {
            self.zone.advance_tick().unwrap();
        }
    }

    fn feet(&self) -> [f32; 3] {
        let [x, y, z] = self.zone.snapshot().unwrap().players[0].position;
        [x, y - 90.0, z]
    }

    fn traversal(&self) -> Traversal {
        self.zone.snapshot().unwrap().players[0].combat.traversal
    }

    /// Holds `forward` toward `facing` until `done` or `limit` ticks pass; returns whether it
    /// was done.
    fn until(
        &mut self,
        forward: i8,
        facing: u16,
        limit: u64,
        done: impl Fn(&Self) -> bool,
    ) -> bool {
        self.hold(forward, facing);
        for _ in 0..limit {
            if done(self) {
                return true;
            }
            self.ticks(1);
        }
        done(self)
    }

    /// Jumps at the face ahead and keeps pressing toward it for two seconds: nothing grabs or
    /// mantles, the jump rises no higher than a jump (sliding a little up a steep face), and
    /// the player ends where it stood.
    fn refused(&mut self, facing: u16) {
        let foot = self.feet();
        self.jump();
        self.hold(1, facing);
        let mut highest = foot[1];
        for _ in 0..120 {
            self.ticks(1);
            assert_eq!(self.traversal(), Traversal::Free, "at {:?}", self.feet());
            highest = highest.max(self.feet()[1]);
        }
        assert!(highest < foot[1] + 200.0, "rose to {highest} from {foot:?}");
        assert!(
            (self.feet()[1] - foot[1]).abs() < 5.0,
            "{:?} from {foot:?}",
            self.feet()
        );
    }

    /// Walks into whatever stands ahead until the body stops.
    fn walk_into(&mut self, facing: u16) {
        self.hold(1, facing);
        let mut last = self.feet();
        for _ in 0..600 {
            self.ticks(1);
            let now = self.feet();
            if (now[0] - last[0]).hypot(now[2] - last[2]) < 0.01 {
                return;
            }
            last = now;
        }
        panic!("never stopped walking at {last:?}");
    }
}

fn collider(id: u32) -> StaticCollider {
    greyhaven_vale::content()
        .definition()
        .colliders()
        .iter()
        .find(|collider| collider.id == id)
        .unwrap()
        .clone()
}

fn top(collider: &StaticCollider) -> f32 {
    (collider.position[1] + collider.half_extents[1]) as f32
}

fn over(collider: &StaticCollider, feet: [f32; 3]) -> bool {
    [0, 2].into_iter().all(|axis| {
        (feet[axis] - collider.position[axis] as f32).abs()
            <= collider.half_extents[axis] as f32 + 1.0
    })
}

#[test]
fn a_player_climbs_a_highland_crag_and_pulls_up_onto_its_top() {
    // The crag flanking the gorge's top on the west, 12 m tall; approach its southern face.
    let crag = collider(*ids::HIGHLAND_CRAGS.start());
    let mut player = Player::at([1_500, -28_900]);
    player.walk_into(NORTH);
    player.jump();
    player.ticks(1);
    assert!(
        matches!(player.traversal(), Traversal::Climbing { .. }),
        "holds the crag: {:?}",
        player.traversal()
    );
    let start = player.feet();
    // Halfway up, the climber hangs on without sliding when the keys go up.
    assert!(player.until(1, NORTH, 600, |p| p.feet()[1] > start[1] + 400.0));
    player.hold(0, NORTH);
    player.ticks(30);
    let hanging = player.feet();
    player.ticks(60);
    assert!((player.feet()[1] - hanging[1]).abs() < 0.5, "hangs on");
    // Climbing on tops out onto the crag.
    assert!(
        player.until(1, NORTH, 1_500, |p| p.traversal() == Traversal::Free
            && p.feet()[1] > top(&crag) - 10.0),
        "never topped out: {:?} {:?}",
        player.feet(),
        player.traversal()
    );
    player.hold(0, NORTH);
    player.ticks(30);
    let feet = player.feet();
    assert!(over(&crag, feet), "on the crag: {feet:?}");
    assert!(
        (feet[1] - top(&crag)).abs() < 2.0,
        "on its top {}: {feet:?} {crag:?}",
        top(&crag)
    );
}

#[test]
fn a_sheer_crag_refuses_a_climber() {
    let crag = collider(ids::HIGHLAND_CRAGS.start() + greyhaven_vale::SHEER_CRAGS[0] as u32);
    let [x, _, z] = crag.position;
    let mut player = Player::at([x, z + crag.half_extents[2] + 300]);
    player.walk_into(NORTH);
    player.refused(NORTH);
}

#[test]
fn a_player_mantles_the_mill_dock_from_the_yard() {
    let dock = collider(ids::MILL_DOCK);
    let mut player = Player::at([8_250, 2_300]);
    player.walk_into(NORTH);
    player.jump();
    player.ticks(1);
    assert!(
        matches!(player.traversal(), Traversal::Mantling { .. }),
        "{:?}",
        player.traversal()
    );
    assert!(player.until(0, NORTH, 120, |p| p.traversal() == Traversal::Free));
    player.ticks(30);
    let feet = player.feet();
    assert!(over(&dock, feet), "on the dock: {feet:?}");
    assert!((feet[1] - top(&dock)).abs() < 2.0, "on its deck: {feet:?}");
}

#[test]
fn a_running_jump_grabs_the_smooth_palisade_and_pulls_up_onto_it() {
    // The stronghold's northern palisade, 3 m high, is smooth-planked: it cannot be climbed,
    // but its top is in reach of a jump.
    let palisade = collider(ids::HIGHLAND_BUILDINGS.start() + 1);
    let mut player = Player::at([2_000, -55_600]);
    player.walk_into(NORTH);
    player.jump();
    player.ticks(1);
    assert_eq!(player.traversal(), Traversal::Free, "out of standing reach");
    assert!(player.until(1, NORTH, 60, |p| matches!(
        p.traversal(),
        Traversal::Mantling { .. }
    )));
    assert!(player.until(0, NORTH, 120, |p| p.traversal() == Traversal::Free));
    player.ticks(30);
    let feet = player.feet();
    assert!(over(&palisade, feet), "on the palisade: {feet:?}");
    assert!(
        (feet[1] - top(&palisade)).abs() < 2.0,
        "{} {feet:?} {palisade:?}",
        top(&palisade)
    );
}

#[test]
fn a_wall_out_of_reach_refuses_a_mantle() {
    // The smithy's 4.5 m wall: no building climbs, and its top is beyond a jump's reach.
    let smithy = collider(ids::SMITHY);
    let [x, _, z] = smithy.position;
    let mut player = Player::at([x, z + smithy.half_extents[2] + 200]);
    player.walk_into(NORTH);
    player.refused(NORTH);
}

#[test]
fn a_player_climbs_a_ladder_onto_the_lookout_tower() {
    let tower = collider(ids::HIGHLAND_BUILDINGS.start() + 21);
    let ladder = collider(ids::LADDERS.start() + 1);
    let [x, _, z] = ladder.position;
    let mut player = Player::at([x, z + 200]);
    player.walk_into(NORTH);
    player.jump();
    player.ticks(1);
    assert!(matches!(player.traversal(), Traversal::Climbing { .. }));
    assert!(
        player.until(1, NORTH, 2_400, |p| p.traversal() == Traversal::Free
            && p.feet()[1] > top(&tower) - 10.0),
        "{:?}",
        player.feet()
    );
    player.hold(0, NORTH);
    player.ticks(30);
    let feet = player.feet();
    assert!(over(&tower, feet), "on the tower: {feet:?}");
}

#[test]
fn the_climbing_route_beside_the_highland_pass_leads_up_the_escarpment() {
    let route = greyhaven_vale::CLIMBING_ROUTES[0];
    let x = (route.min[0] + route.max[0]) / 2;
    let mut player = Player::at([x, -20_400]);
    player.walk_into(NORTH);
    player.jump();
    player.ticks(1);
    assert!(
        matches!(player.traversal(), Traversal::Climbing { .. }),
        "{:?}",
        player.traversal()
    );
    // Up the southern face onto the crest, then down its northern face onto the high ground.
    assert!(
        player.until(1, NORTH, 6_000, |p| p.feet()[2]
            < highlands::ESCARPMENT[1] as f32
            && p.traversal() == Traversal::Free),
        "{:?} {:?}",
        player.feet(),
        player.traversal()
    );
    assert!(player.until(1, NORTH, 1_200, |p| p.feet()[2] < -28_400.0));
    let feet = player.feet();
    assert_eq!(
        greyhaven_vale::area_at(feet[0] as i32, feet[2] as i32).map(|area| area.name()),
        Some("Bandit Highlands")
    );
    assert!(
        (feet[1] - highlands::PLATEAU_Y as f32).abs() < 400.0,
        "{feet:?}"
    );
}

#[test]
fn the_escarpment_beside_the_route_is_sheer() {
    let route = greyhaven_vale::CLIMBING_ROUTES[0];
    let mut player = Player::at([route.max[0] + 1_200, -20_400]);
    player.walk_into(NORTH);
    player.refused(NORTH);
}

/// Flat ground with a 1.5 m block 4 m north of the spawn; `roof` lays a slab 1 m above it.
fn block_arena(roof: bool) -> Arc<ZoneContent> {
    let mut colliders = vec![
        StaticCollider {
            id: 1,
            position: [0, -50, 0],
            half_extents: [10_000, 50, 10_000],
        },
        StaticCollider {
            id: 10,
            position: [0, 75, -600],
            half_extents: [300, 75, 200],
        },
    ];
    if roof {
        colliders.push(StaticCollider {
            id: 11,
            position: [0, 425, -600],
            half_extents: [300, 175, 200],
        });
    }
    let definition = ZoneDefinition::with_spawn_grid(
        mmorpg_core::STANDARD_GRAVITY,
        mmorpg_core::SpawnGrid {
            origin: [0, 0],
            columns: 32,
            spacing: 100,
        },
        colliders,
    )
    .unwrap();
    Arc::new(
        ZoneContent::new(
            definition,
            ZoneAreas::default(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            [0, 3_000],
        )
        .unwrap(),
    )
}

fn arena_player(roof: bool) -> Player {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), block_arena(roof)).unwrap();
    zone.add_player(1).unwrap();
    Player::in_zone(zone, [0, 0])
}

#[test]
fn a_roofed_ledge_refuses_a_mantle_its_open_twin_allows() {
    let mut open = arena_player(false);
    open.walk_into(NORTH);
    open.jump();
    open.ticks(1);
    assert!(matches!(open.traversal(), Traversal::Mantling { .. }));
    open.until(0, NORTH, 120, |p| p.traversal() == Traversal::Free);
    assert!((open.feet()[1] - 150.0).abs() < 2.0, "{:?}", open.feet());

    let mut roofed = arena_player(true);
    roofed.walk_into(NORTH);
    roofed.refused(NORTH);
}

#[test]
fn a_climb_and_a_mantle_continue_exactly_from_a_checkpoint() {
    let mut player = Player::at([1_500, -28_900]);
    player.walk_into(NORTH);
    player.jump();
    player.hold(1, NORTH);
    player.ticks(40);
    assert!(matches!(player.traversal(), Traversal::Climbing { .. }));
    let checkpoint = player.zone.snapshot().unwrap();
    let mut restored =
        ZoneSimulation::from_snapshot(checkpoint, Arc::clone(player.zone.content())).unwrap();
    for _ in 0..200 {
        player.zone.advance_tick().unwrap();
        restored.advance_tick().unwrap();
    }
    assert_eq!(
        player.zone.snapshot().unwrap(),
        restored.snapshot().unwrap()
    );
}

/// An experimental rule: abilities wait until the climber is off the face (#490).
static CLIMB_RULES: [mmorpg_core::action::CompatibilityRule; 6] = [
    mmorpg_core::action::RULES[0],
    mmorpg_core::action::RULES[1],
    mmorpg_core::action::RULES[2],
    mmorpg_core::action::RULES[3],
    mmorpg_core::action::RULES[4],
    mmorpg_core::action::CompatibilityRule {
        id: mmorpg_core::action::RuleId::ClimbingHolds,
        during: mmorpg_core::action::ActionId::Climb,
        input: mmorpg_core::action::ActionInput::Ability,
        conditions: &[],
        effect: mmorpg_core::action::RuleEffect::Hold,
    },
];

static CLIMB_GRAPH: mmorpg_core::action::ActionGraph = mmorpg_core::action::ActionGraph {
    rules: &CLIMB_RULES,
    ..mmorpg_core::action::BASELINE
};

#[test]
fn climbing_and_mantling_run_in_the_movement_region() {
    use mmorpg_core::action::{ActionId, ActionInput, Outcome, Region, RuleId};
    let movement = |player: &Player| {
        player.zone.action_inspection(1).unwrap().regions[Region::Movement as usize].action
    };
    let mut player = Player::at([1_500, -28_900]);
    player.zone.set_action_graph(&CLIMB_GRAPH).unwrap();
    player.walk_into(NORTH);
    assert_eq!(movement(&player), None);
    player.jump();
    player.ticks(1);
    assert_eq!(movement(&player), Some(ActionId::Climb));
    assert_eq!(
        player
            .zone
            .arbitrate_input(1, ActionInput::Ability)
            .unwrap()
            .outcome,
        Outcome::WaitForRule(RuleId::ClimbingHolds)
    );
    assert!(player.until(1, NORTH, 1_500, |p| matches!(
        p.traversal(),
        Traversal::Mantling { .. }
    )));
    assert_eq!(movement(&player), Some(ActionId::Mantle));
}

/// Climbs a boundary route from `start` toward `facing` until `beyond` holds of the feet, then
/// walks on into `area`.
fn take_route(start: [i32; 2], facing: u16, beyond: impl Fn([f32; 3]) -> bool, area: &str) {
    let mut player = Player::at(start);
    player.walk_into(facing);
    player.jump();
    player.ticks(1);
    assert!(
        matches!(player.traversal(), Traversal::Climbing { .. }),
        "{:?} at {:?}",
        player.traversal(),
        player.feet()
    );
    assert!(
        player.until(1, facing, 9_000, |p| beyond(p.feet())),
        "{:?} {:?}",
        player.feet(),
        player.traversal()
    );
    let feet = player.feet();
    assert_eq!(
        greyhaven_vale::area_at(feet[0] as i32, feet[2] as i32).map(|area| area.name()),
        Some(area),
        "{feet:?}"
    );
}

#[test]
fn the_climbing_route_beside_the_south_road_crosses_the_farmland_ridge() {
    let route = greyhaven_vale::CLIMBING_ROUTES[1];
    let x = (route.min[0] + route.max[0]) / 2;
    take_route(
        [x, 20_400],
        0,
        |feet| feet[2] > 25_600.0,
        "Farmland and Hamlets",
    );
}

#[test]
fn the_climbing_route_beside_the_marsh_pass_crosses_the_wetlands_ridge() {
    let route = greyhaven_vale::CLIMBING_ROUTES[2];
    let z = (route.min[1] + route.max[1]) / 2;
    take_route(
        [20_400, z],
        16_384,
        |feet| feet[0] > 25_600.0,
        "Marsh-Lake Wetlands",
    );
}
