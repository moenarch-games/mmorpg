//! Climbing and free ledge mantling (#490, Atlas 5.1.40) on Greyhaven's own physics: a player
//! climbs a highland crag, a tower's ladder, the Greyfang lookout's ladder (#913) and the rope up a sheer crag (#621), mantles the mill's loading dock and jumps up to
//! grab the stronghold's palisade, takes the climbing route up the highlands' escarpment, and is
//! refused by a sheer crag, a too-high wall and an obstructed ledge.

use std::sync::Arc;

use mmorpg_core::greyhaven_vale::{self, ids};
use mmorpg_core::{
    Grip, Ledge, StaticCollider, Traversal, ZoneAreas, ZoneCommand, ZoneContent, ZoneDefinition,
    ZoneId, ZoneSimulation,
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

/// A highland crag's outcrop (#1195): its collision is generated from the art, and it climbs
/// only where its dressing placement sets `"grip": "rock"`.
fn outcrop(id: u32) -> mmorpg_core::StaticMesh {
    greyhaven_vale::content()
        .definition()
        .static_meshes()
        .iter()
        .find(|mesh| mesh.id == id)
        .unwrap_or_else(|| panic!("no outcrop {id}"))
        .clone()
}

/// The crag flanking the gorge's top on the west is a ledged buttress over talus (#1195): a
/// player climbs its ledged eastern face, the side the trail passes, hangs on halfway, and
/// mantles over its crown onto the top.
#[test]
fn a_player_climbs_a_climbable_outcrop_crag_and_mantles_onto_it() {
    let rock = outcrop(*ids::HIGHLAND_CRAGS.start());
    let crag = rock.bounds().unwrap();
    assert_eq!(
        greyhaven_vale::climbing().collider(rock.id),
        Some(Grip::Rock)
    );
    let west = 49_152;
    let [x, _, z] = rock.position;
    let mut player = Player::at([crag.position[0] + crag.half_extents[0] + 300, z]);
    player.walk_into(west);
    player.jump();
    player.ticks(1);
    assert!(
        matches!(
            player.traversal(),
            Traversal::Climbing {
                grip: Grip::Rock,
                ..
            }
        ),
        "holds the crag: {:?} at {:?}",
        player.traversal(),
        player.feet()
    );
    let start = player.feet();
    // Halfway up, the climber hangs on without sliding when the keys go up.
    assert!(player.until(1, west, 600, |p| p.feet()[1] > start[1] + 400.0));
    player.hold(0, west);
    player.ticks(30);
    let hanging = player.feet();
    player.ticks(60);
    assert!((player.feet()[1] - hanging[1]).abs() < 0.5, "hangs on");
    // Climbing on mantles over the crown and stands on the crag.
    let mut mantled = false;
    for _ in 0..2_000 {
        player.hold(1, west);
        player.ticks(1);
        mantled |= matches!(player.traversal(), Traversal::Mantling { .. });
        if player.traversal() == Traversal::Free && player.feet()[1] > top(&crag) - 250.0 {
            break;
        }
    }
    assert!(
        mantled,
        "never mantled: {:?} {:?}",
        player.feet(),
        player.traversal()
    );
    player.hold(0, west);
    player.ticks(30);
    let feet = player.feet();
    assert_eq!(player.traversal(), Traversal::Free, "at {feet:?}");
    assert!(over(&crag, feet), "on the crag: {feet:?} from {x},{z}");
    assert!(
        feet[1] > top(&crag) - 250.0,
        "on its crown {}: {feet:?}",
        top(&crag)
    );
}

/// The two sheer crags are smooth spires and an overhanging buttress (#1195): none of their
/// outcrops climbs, and a player walking into their cores cannot grab them.
#[test]
fn a_sheer_outcrop_crag_refuses_a_climber() {
    let climbing = greyhaven_vale::climbing();
    // Each core from its open side: the first's spires stand north of it and its rope south,
    // the second's talus east of it.
    let [east, west] = [16_384, 49_152];
    for (index, facing) in greyhaven_vale::SHEER_CRAGS.into_iter().zip([west, east]) {
        let rock = outcrop(ids::HIGHLAND_CRAGS.start() + index as u32);
        assert_eq!(climbing.collider(rock.id), None, "crag {index}");
        let crag = rock.bounds().unwrap();
        let side = if facing == west { 1 } else { -1 };
        let mut player = Player::at([
            crag.position[0] + side * (crag.half_extents[0] + 300),
            rock.position[2],
        ]);
        player.walk_into(facing);
        player.refused(facing);
    }
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
    assert!(matches!(
        player.traversal(),
        Traversal::Climbing {
            grip: Grip::Ladder,
            ..
        }
    ));
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

/// The Greyfang lookout's ladder (#913) leads onto its deck, 2.25 m up, where its cache lies.
#[test]
fn a_player_climbs_the_greyfang_lookouts_ladder_onto_its_deck() {
    const EAST: u16 = 16_384;
    let platform = collider(*greyhaven_vale::discoveries::COLLIDERS.start() + 3);
    let ladder = collider(greyhaven_vale::discoveries::LOOKOUT_LADDER);
    let [x, _, z] = ladder.position;
    let mut player = Player::at([x - 200, z]);
    player.walk_into(EAST);
    player.jump();
    player.ticks(1);
    assert!(matches!(
        player.traversal(),
        Traversal::Climbing {
            grip: Grip::Ladder,
            ..
        }
    ));
    let deck = greyhaven_vale::discoveries::caches()[1].at[1] as f32;
    assert!(
        player.until(1, EAST, 2_400, |p| p.traversal() == Traversal::Free
            && p.feet()[1] > deck - 10.0),
        "{:?}",
        player.feet()
    );
    // A step on from the ladder's top lands on the deck.
    player.hold(1, EAST);
    player.ticks(8);
    player.hold(0, EAST);
    player.ticks(30);
    let feet = player.feet();
    assert!(
        over(&platform, feet) && (feet[1] - deck).abs() < 10.0,
        "on the deck: {feet:?}"
    );
}

/// The rope up the sheer crag (#621) is the only way up it: held as a rope, climbed to the top.
#[test]
fn a_player_climbs_the_rope_up_the_sheer_crag() {
    let rope = collider(*ids::ROPES.start() + 2);
    let crag = outcrop(greyhaven_vale::ROPES[2].host).bounds().unwrap();
    let [x, _, z] = rope.position;
    let mut player = Player::at([x, z + 200]);
    player.walk_into(NORTH);
    player.jump();
    player.ticks(1);
    assert!(matches!(
        player.traversal(),
        Traversal::Climbing {
            grip: Grip::Rope,
            ..
        }
    ));
    // The rope ends level with the buttress's domed crown, where the climber pulls over onto
    // it (#1195).
    assert!(
        player.until(1, NORTH, 3_000, |p| p.traversal() == Traversal::Free
            && p.feet()[1] > top(&rope) - 10.0),
        "{:?}",
        player.feet()
    );
    player.hold(0, NORTH);
    player.ticks(30);
    let feet = player.feet();
    assert!(over(&crag, feet), "on the crag: {feet:?}");
    assert!(feet[1] > top(&rope) - 10.0, "stays on the crown: {feet:?}");
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
    assert!(matches!(
        open.traversal(),
        Traversal::Mantling {
            ledge: Ledge::Mid,
            ..
        }
    ));
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

#[test]
fn the_quarry_rope_reaches_its_level_landing_on_the_south_rim() {
    // Use the real terrain, rock and climb definitions without camp combat interrupting it.
    let rope = collider(*ids::ROPES.start());
    let host = collider(greyhaven_vale::ROPES[0].host);
    let [x, _, z] = rope.position;
    // The rope hangs down the south wall's inner face, which faces north into the quarry.
    let south = 0;
    let mut player = Player::in_zone(hollow_without_its_camp(), [x, z - 200]);
    player.walk_into(south);
    player.jump();
    player.ticks(1);
    assert!(
        matches!(
            player.traversal(),
            Traversal::Climbing {
                grip: Grip::Rope,
                ..
            }
        ),
        "{:?} at {:?}",
        player.traversal(),
        player.feet()
    );
    assert!(
        player.until(1, south, 1800, |p| p.traversal() == Traversal::Free
            && p.feet()[1] > top(&host) - 10.),
        "rope exit: {:?} at {:?}",
        player.traversal(),
        player.feet()
    );
    player.hold(0, south);
    player.ticks(30);
    assert!(over(&host, player.feet()));
    assert!(
        player.feet()[1] > top(&host) - 10.,
        "stable rim landing: {:?}",
        player.feet()
    );
}

/// Greyhaven's real terrain, rock and climbing, without the Hollow camp's Redbrands, whose
/// fight would interrupt a climb there.
fn hollow_without_its_camp() -> ZoneSimulation {
    let content = ZoneContent::from_definition(greyhaven_vale::greyhaven_vale_definition())
        .with_climbing(greyhaven_vale::climbing())
        .unwrap();
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), Arc::new(content)).unwrap();
    zone.add_player(1).unwrap();
    zone
}

/// Climbs the quarry's benched wall from `start` on its floor toward `facing`: walks into each
/// cut, jumps to grab it, climbs and mantles onto the bench above, and goes again until no cut
/// is left. Returns the height (m above the floor) of each bench it stood on.
fn climb_the_benches(start: [i32; 2], facing: u16) -> (Player, Vec<(f32, bool)>) {
    let mut player = Player::in_zone(hollow_without_its_camp(), start);
    let floor = player.feet()[1];
    let mut benches = Vec::new();
    for cut in 0..4 {
        // Up to the cut ahead (the face turns a walker a little along it), then a jump at it.
        player.hold(1, facing);
        player.ticks(60);
        player.jump();
        player.ticks(1);
        if !player.until(1, facing, 30, |p| {
            matches!(
                p.traversal(),
                Traversal::Climbing {
                    grip: Grip::Rock,
                    ..
                }
            )
        }) {
            assert!(cut > 0, "never held the first cut at {:?}", player.feet());
            break;
        }
        // Up the cut until the climber stands again, over its top.
        let mut mantled = false;
        for _ in 0..1_500 {
            player.hold(1, facing);
            player.ticks(1);
            mantled |= matches!(player.traversal(), Traversal::Mantling { .. });
            if player.traversal() == Traversal::Free {
                break;
            }
        }
        assert_eq!(
            player.traversal(),
            Traversal::Free,
            "cut {cut} never topped out at {:?}",
            player.feet()
        );
        player.hold(0, facing);
        player.ticks(20);
        benches.push(((player.feet()[1] - floor) / 100., mantled));
    }
    (player, benches)
}

/// Checks a climb up the quarry's benches: at least two benches and the rim, each higher than
/// the one before, mantling onto a bench at least once on the way.
fn assert_benched_climb(benches: &[(f32, bool)]) {
    assert!(benches.len() >= 3, "benches: {benches:?}");
    assert!(
        benches.windows(2).all(|pair| pair[1].0 > pair[0].0 + 3.),
        "benches: {benches:?}"
    );
    assert!(benches.iter().any(|bench| bench.1), "never mantled: {benches:?}");
    assert!(benches.last().unwrap().0 > 12., "on the rim: {benches:?}");
}

/// The quarry's worked faces climb across the seam between two of its segments (#1195): on the
/// line where the north wall's collision passes from one mesh to the next, a player climbs the
/// floor's 4.4 m cut, mantles onto the bench, climbs the next cut and the last one, and stands
/// on the grassy rim.
#[test]
fn a_player_climbs_the_quarry_benches_across_a_segment_seam_onto_the_rim() {
    for id in [401, 402] {
        assert_eq!(greyhaven_vale::climbing().collider(id), Some(Grip::Rock));
    }
    let (west, east) = (outcrop(401).bounds().unwrap(), outcrop(402).bounds().unwrap());
    // The seam: the ring 401's mesh ends and 402's begins with, on the north wall's foot at
    // (-10.98, -134.01) m, its face turned 9° east of due north (`hollow_cliffs.py`).
    let (player, benches) = climb_the_benches([-1_068, -13_200], 34_368);
    assert_benched_climb(&benches);
    let feet = player.feet();
    assert!(
        over(&west, feet) || over(&east, feet),
        "on the seam's rock: {feet:?}"
    );
}

/// The same up the south wall, across the seam where the rope's straight cut (406) meets the
/// south-east corner (405) at x = 11.55 m.
#[test]
fn a_player_climbs_the_south_wall_across_the_rope_segments_seam() {
    let (player, benches) = climb_the_benches([1_155, -9_500], 0);
    assert_benched_climb(&benches);
    assert!(player.feet()[2] > -9_260.0, "behind the face: {:?}", player.feet());
}

/// The quarry's east wall over the old mine's adit is smooth, sheer rock (#1195): its authored
/// mesh sets `"grip": "sheer"`, and a player walking into it from the floor cannot grab it.
#[test]
fn the_quarrys_sheer_east_wall_refuses_a_climber() {
    let wall = outcrop(greyhaven_vale::SHEER_HOLLOW_WALL);
    assert_eq!(greyhaven_vale::climbing().collider(wall.id), None);
    let bounds = wall.bounds().unwrap();
    let east = 16_384;
    let mut player = Player::in_zone(
        hollow_without_its_camp(),
        [bounds.position[0] - bounds.half_extents[0] - 400, -10_700],
    );
    player.walk_into(east);
    player.refused(east);
}

#[test]
fn a_player_climbs_the_basin_cliff_over_its_rounded_rim_without_falling() {
    // The basin's west cliff under the Hollow rounds over at about 45° before the rim: the
    // climber holds on over it and pulls up, rather than letting go or hanging in the mantle.
    let west = 49_152;
    let mut player = Player::at([-3_000, -5_200]);
    player.hold(1, west);
    player.ticks(300);
    player.jump();
    player.ticks(1);
    player.hold(1, west);
    let mut climbed = false;
    for _ in 0..1_200 {
        player.ticks(1);
        match player.traversal() {
            Traversal::Climbing { .. } => climbed = true,
            Traversal::Mantling { ticks, .. } => assert!(ticks < 60, "{:?}", player.feet()),
            Traversal::Free if climbed && player.feet()[1] > 950.0 => break,
            _ if !climbed => {}
            other => panic!("{other:?} at {:?}", player.feet()),
        }
    }
    assert!(climbed && player.feet()[1] > 950.0, "{:?}", player.feet());
    player.hold(0, west);
    player.ticks(60);
    assert!(player.feet()[1] > 950.0, "on the rim: {:?}", player.feet());
}

/// A basin crag's collision is generated from its art (#683): the climber grabs the generated
/// rock surface and climbs it.
#[test]
fn a_player_climbs_a_dressed_basin_crag() {
    let crag = greyhaven_vale::content()
        .definition()
        .static_meshes()
        .iter()
        .find(|mesh| mesh.id == *greyhaven_vale::fort::BASIN_CRAGS.start())
        .unwrap()
        .clone();
    let [x, _, z] = crag.position;
    // From the east, downhill of it: the brook spur stands south of it.
    let west = 49_152;
    let mut player = Player::at([x + 700, z]);
    player.hold(1, west);
    player.ticks(200);
    player.jump();
    player.ticks(1);
    player.hold(1, west);
    let start = player.feet();
    let mut climbed = false;
    for _ in 0..300 {
        player.ticks(1);
        climbed |= matches!(player.traversal(), Traversal::Climbing { .. });
    }
    assert!(climbed, "never held the crag from {start:?}");
    assert!(
        player.feet()[1] > start[1] + 300.0,
        "climbed from {start:?} to {:?}",
        player.feet()
    );
}

/// A basin buttress's generated collision leaves its art's apron and rubble out (#683): a
/// player walks up onto the apron, to within a metre or so of the rock's foot, before the rock
/// stops them.
#[test]
fn a_player_walks_onto_a_dressed_buttress_apron() {
    let buttress = greyhaven_vale::content()
        .definition()
        .static_meshes()
        .iter()
        .find(|mesh| mesh.id == *greyhaven_vale::fort::BASIN_ROCKS.start())
        .unwrap()
        .clone();
    let [x, _, z] = buttress.position;
    // The rock's own extent south of its origin at the foot.
    let south = buttress
        .vertices
        .iter()
        .filter(|v| v[1] < 100)
        .map(|v| v[2])
        .max()
        .unwrap();
    let mut player = Player::at([x, z + south + 600]);
    player.walk_into(NORTH);
    let feet = player.feet();
    let gap = feet[2] - (z + south) as f32;
    assert!(
        gap < 150.0,
        "stopped {gap} units short of the rock's foot: {feet:?}"
    );
}

/// Stillwater's shore rocks collide as their talus art (#683): a player walking from the bank
/// meets the largest rock at its foot, jumps and mantles onto its top, and walks on along the
/// shore beside it.
#[test]
fn a_player_mantles_a_dressed_shore_rock_and_walks_past_it() {
    let rock = greyhaven_vale::content()
        .definition()
        .static_meshes()
        .iter()
        .find(|mesh| mesh.id == *greyhaven_vale::fort::SHORE_ROCKS.start())
        .unwrap()
        .clone();
    let [x, y, z] = rock.position;
    let east = rock.vertices.iter().map(|v| v[0]).max().unwrap();
    let top = y + rock.vertices.iter().map(|v| v[1]).max().unwrap();
    // The middle of the rock's north-south extent.
    let (north, south) = rock
        .vertices
        .iter()
        .fold((i32::MAX, i32::MIN), |(n, s), v| (n.min(v[2]), s.max(v[2])));
    let z = z + (north + south) / 2;
    let west = 49_152;
    // Walking west, the player stops at the rock's east face (a held press then slides it
    // around the rounded rock, so it jumps as soon as it is stopped).
    let mut player = Player::at([x + east + 300, z]);
    player.hold(1, west);
    let mut last = player.feet()[0];
    for _ in 0..60 {
        player.ticks(1);
        if (last - player.feet()[0]).abs() < 5.0 {
            break;
        }
        last = player.feet()[0];
    }
    let gap = player.feet()[0] - (x + east) as f32;
    assert!(
        (0.0..60.0).contains(&gap),
        "stopped {gap} units east of the rock"
    );
    player.jump();
    assert!(player.until(1, west, 60, |p| matches!(
        p.traversal(),
        Traversal::Mantling { .. }
    )));
    assert!(player.until(0, west, 120, |p| p.traversal() == Traversal::Free));
    player.ticks(30);
    let feet = player.feet();
    assert!(
        (feet[1] - top as f32).abs() < 40.0,
        "on its top {top}: {feet:?}"
    );
    // Along the bank, two metres east of the rock, nothing stands in the way.
    let mut walker = Player::at([x + east + 200, z + 600]);
    walker.hold(1, NORTH);
    walker.ticks(130);
    assert!(walker.feet()[2] < (z - 600) as f32, "{:?}", walker.feet());
}

/// The sandstone steading's cottage (#683.Q1) collides as its art: a player walking from the
/// yard at its closed door stops at the door's outer face, and its 4 m wall offers no climb or
/// mantle.
#[test]
fn a_player_stops_at_the_sandstone_cottage_door_and_cannot_climb_it() {
    let definition = greyhaven_vale::content().definition().clone();
    let door = definition
        .static_meshes()
        .iter()
        .find(|mesh| mesh.id == greyhaven_vale::fort::SANDSTONE_STEADING.start() + 3)
        .expect("the cottage's door leaf collides");
    // The cottage stands at (-23900, -37050) turned 135°, its door 3 m out along (1, -1)/√2.
    let out = [
        std::f32::consts::FRAC_1_SQRT_2,
        -std::f32::consts::FRAC_1_SQRT_2,
    ];
    let centre = [-23_900.0_f32, -37_050.0];
    let along = |feet: [f32; 3]| (feet[0] - centre[0]) * out[0] + (feet[2] - centre[1]) * out[1];
    let start = [centre[0] + out[0] * 700.0, centre[1] + out[1] * 700.0];
    let mut player = Player::at([start[0] as i32, start[1] as i32]);
    // Facing back along the door's axis: toward −X and +Z.
    let inward = 57_344;
    player.walk_into(inward);
    let feet = player.feet();
    assert!(
        (300.0..420.0).contains(&along(feet)),
        "stopped {} units out from the cottage centre (door leaf at {:?}): {feet:?}",
        along(feet),
        door.position
    );
    player.refused(inward);
}
