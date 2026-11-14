//! Authored surfaces on production Rapier: Redbrand's cave and Millbrook's round windmill.
use mmorpg_core::{ZoneCommand, ZoneContent, ZoneId, ZoneSimulation, greyhaven_vale};
use std::sync::Arc;

struct Player {
    zone: ZoneSimulation,
    sequence: u32,
}
impl Player {
    fn at(feet: [i32; 3]) -> Self {
        // The real playable definition, without enemies interrupting the traversal demonstration.
        let content = Arc::new(ZoneContent::from_definition(
            greyhaven_vale::greyhaven_vale_definition(),
        ));
        let mut zone = ZoneSimulation::with_content(ZoneId::new(1), content.clone()).unwrap();
        zone.add_player(1).unwrap();
        let mut state = zone.snapshot().unwrap();
        state.players[0].position =
            mmorpg_core::physics::from_units([feet[0], feet[1] + 90, feet[2]]);
        let zone = ZoneSimulation::from_snapshot(state, content).unwrap();
        let mut player = Self { zone, sequence: 0 };
        player.ticks(10);
        player
    }
    fn command(&mut self, command: ZoneCommand) {
        self.sequence += 1;
        self.zone.apply_command(1, self.sequence, command).unwrap();
    }
    fn ticks(&mut self, count: usize) {
        for _ in 0..count {
            self.zone.advance_tick().unwrap();
        }
    }
    fn walk(&mut self, facing: u16, ticks: usize) {
        self.command(ZoneCommand::Move {
            forward: 1,
            strafe: 0,
            facing,
        });
        self.ticks(ticks);
        self.command(ZoneCommand::Move {
            forward: 0,
            strafe: 0,
            facing,
        });
    }
    fn feet(&self) -> [f32; 3] {
        let [x, y, z] = self.zone.snapshot().unwrap().players[0].position;
        [x, y - 90., z]
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub fn redbrand_entrance_walls_shelf_ceiling_and_recovery() {
    let mut p = Player::at([6400, 1100, -15600]);
    p.walk(0, 190); // Through the concealed north mouth from the terrace.
    assert!(p.feet()[2] > -14200., "enter cave: {:?}", p.feet());
    assert!((p.feet()[1] - 1100.).abs() < 1., "floor: {:?}", p.feet());
    p.walk(49152, 180); // West wall is solid, not its hollow bounds.
    assert!(
        (4829. ..4832.).contains(&p.feet()[0]),
        "wall: {:?}",
        p.feet()
    );
    p.walk(0, 60);
    assert!(p.feet()[2] <= -13229., "sealed south wall: {:?}", p.feet());
    // Approach the slope from the west, where it starts flush with the floor.
    p.walk(32768, 7);
    p.walk(16384, 125);
    assert!(p.feet()[0] > 5800., "reach shelf: {:?}", p.feet());
    assert!(
        (p.feet()[1] - 1320.).abs() < 1.,
        "upper elevation: {:?}",
        p.feet()
    );
    p.command(ZoneCommand::Jump);
    let mut highest = p.feet()[1];
    for _ in 0..60 {
        p.ticks(1);
        highest = highest.max(p.feet()[1]);
    }
    assert!(
        highest > 1330. && highest <= 1371.,
        "roof limits jump: {highest}"
    );
    assert!((p.feet()[1] - 1320.).abs() < 1.);
    let state = p.zone.snapshot().unwrap();
    let mut restored = ZoneSimulation::from_snapshot(state, p.zone.content().clone()).unwrap();
    for _ in 0..30 {
        p.zone.advance_tick().unwrap();
        restored.advance_tick().unwrap();
    }
    assert_eq!(p.zone.snapshot().unwrap(), restored.snapshot().unwrap());

    let mut lower = Player::at([6200, 1100, -13600]);
    lower.walk(0, 30);
    assert!(
        lower.feet()[2] > -13400.,
        "walk under shelf: {:?}",
        lower.feet()
    );
    assert!((lower.feet()[1] - 1100.).abs() < 1.);
    lower.command(ZoneCommand::Jump);
    for _ in 0..45 {
        lower.ticks(1);
        assert!(lower.feet()[1] <= 1121., "shelf underside blocks jump");
    }
    // Return across the same terrain seam to the surface.
    let mut exit = Player::at([6400, 1100, -14200]);
    exit.walk(32768, 170);
    assert!(exit.feet()[2] < -15400. && (exit.feet()[1] - 1100.).abs() < 1.);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub fn meshes_support_without_heightfield_and_change_content_identity() {
    use mmorpg_core::physics::{BodyId, RigidBody, Vec3i, World};
    let meshes = greyhaven_vale::cave::collision_meshes();
    let mut world = World::new(Vec3i::new(0, -64, 0));
    for mesh in &meshes {
        world
            .add_body(RigidBody::static_mesh(mesh).unwrap())
            .unwrap();
    }
    let id = BodyId(1_000_000);
    world
        .add_body(RigidBody::dynamic(
            id,
            Vec3i::new(5800, 1200, -13800),
            Vec3i::ZERO,
            Vec3i::new(30, 90, 30),
        ))
        .unwrap();
    for _ in 0..60 {
        world.step().unwrap();
    }
    assert!((world.body(id).unwrap().exact_position()[1] - 1190.).abs() < 1.);
    assert!(world.grounded(id));
    let build = |m| {
        ZoneContent::from_definition(
            mmorpg_core::ZoneDefinition::default()
                .with_static_meshes(m)
                .unwrap(),
        )
    };
    let a = build(meshes.clone());
    let mut reordered = meshes.clone();
    reordered.reverse();
    assert_eq!(a.revision(), build(reordered).revision());
    let mut changed = meshes;
    changed[0].vertices[0][0] += 1;
    assert_ne!(a.revision(), build(changed).revision());
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub fn millbrook_windmill_has_a_solid_round_footprint() {
    // This lane cuts through a corner of the former square proxy, outside the round tower.
    let mut corner = Player::at([9400, 1100, 2350]);
    corner.walk(49152, 50);
    assert!(
        corner.feet()[0] < 9050.,
        "no invisible square corner: {:?}",
        corner.feet()
    );
    assert!((corner.feet()[1] - 1100.).abs() < 1.);

    let mut wall = Player::at([9400, 1100, 2100]);
    wall.command(ZoneCommand::Move {
        forward: 1,
        strafe: 0,
        facing: 49152,
    });
    let mut closest = f32::INFINITY;
    for _ in 0..120 {
        wall.ticks(1);
        let p = wall.feet();
        let radius = (p[0] - 8800.).hypot(p[2] - 2100.);
        assert!(radius >= 265., "cannot enter solid tower: {p:?}");
        closest = closest.min(radius);
    }
    // The controller may slide around a curved wall, but must actually reach it and stay out.
    assert!(closest < 290., "reached the tower: {closest}");
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub fn hollow_tent_flaps_block_and_sloped_roofs_have_no_square_corners() {
    let mut flap = Player::at([-1300, 1100, -9450]);
    flap.walk(49152, 90);
    assert!(
        (-1380. ..-1350.).contains(&flap.feet()[0]),
        "closed canvas flap: {:?}",
        flap.feet()
    );

    // Drop onto the low edge of the roof. A leftover box would leave the player standing
    // 2.2 m up in empty air; the steep canvas instead sheds the character to the ground.
    let mut edge = Player::at([-1550, 1400, -9340]);
    edge.ticks(120);
    assert!(
        edge.feet()[1] < 1200.,
        "no invisible box roof: {:?}",
        edge.feet()
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub fn walk_in_houses_keep_the_roof_solid_when_hidden_indoors() {
    use mmorpg_core::physics::{BodyId, RigidBody, Vec3i, World};

    let definition = greyhaven_vale::greyhaven_vale_definition();
    for (id, origin, ceiling, eave, ridge, half_z) in [
        (
            greyhaven_vale::ids::INN_ROOF,
            greyhaven_vale::inn::ORIGIN,
            300,
            321,
            600,
            500,
        ),
        (
            greyhaven_vale::ids::MARSHAL_HOUSE_ROOF,
            greyhaven_vale::marshal_house::ORIGIN,
            295,
            350,
            552,
            335,
        ),
    ] {
        let mesh = definition
            .static_meshes()
            .iter()
            .find(|mesh| mesh.id == id)
            .expect("walk-in roof belongs to authoritative content");
        let mut world = World::new(Vec3i::new(0, -64, 0));
        world
            .add_body(RigidBody::static_mesh(mesh).unwrap())
            .unwrap();
        let roof = |from, travel| {
            world
                .cast_ray(from, travel, |body| body == BodyId(u64::from(id)))
                .expect("physical roof blocks ray")
        };
        let [x, _, z] = origin;
        let above = roof([x as f32, 800., z as f32], [0., -800., 0.]);
        assert!(
            ((800. - above.fraction * 800.) - ridge as f32).abs() < 2.,
            "roof {id} must follow its ridge: {above:?}"
        );
        for side in [-1, 1] {
            let slope = roof(
                [(x + 100) as f32, 800., (z + side * half_z / 2) as f32],
                [0., -800., 0.],
            );
            assert!(
                ((800. - slope.fraction * 800.) - ((eave + ridge) / 2) as f32).abs() < 3.,
                "roof {id} needs pitched collision on both sides: {slope:?}"
            );
        }
        let below = roof([x as f32, 200., z as f32], [0., 500., 0.]);
        assert!(
            ((200. + below.fraction * 500.) - ceiling as f32).abs() < 2.,
            "roof {id} must have a solid ceiling: {below:?}"
        );

        let mut player = Player::at([x, 0, z]);
        player.command(ZoneCommand::Jump);
        for _ in 0..50 {
            player.ticks(1);
            assert!(
                player.feet()[1] <= (ceiling - 180 + 4) as f32,
                "jumped through walk-in ceiling {id}: {:?}",
                player.feet()
            );
        }
    }
}
