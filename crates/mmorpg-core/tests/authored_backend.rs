//! Real Redbrand JSON and Greyhaven rock proxies on both collision backends.
//! These tests use the same core fixtures on native and WASM, without substitute geometry.

use mmorpg_core::greyhaven_vale;
use mmorpg_core::physics::{Backend, BodyId, DebrisBody, RigidBody, Vec3i, World};

/// A two-sided ray through an actual input triangle, starting outside the mesh's bounds.
/// It remains meaningful when the region's authored coordinates and topology change.
fn face_ray(mesh: &mmorpg_core::StaticMesh) -> ([f32; 3], [f32; 3]) {
    let points = mesh.triangles[0].map(|index| {
        let local = mesh.vertices[index as usize];
        let world = std::array::from_fn(|axis| mesh.position[axis] + local[axis]);
        mmorpg_core::physics::from_units(world)
    });
    let [a, b, c] = points;
    let u: [f32; 3] = std::array::from_fn(|axis| b[axis] - a[axis]);
    let v: [f32; 3] = std::array::from_fn(|axis| c[axis] - a[axis]);
    let normal = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let norm = normal.iter().map(|axis| axis * axis).sum::<f32>().sqrt();
    let normal = normal.map(|axis| axis / norm);
    let mut low = [i32::MAX; 3];
    let mut high = [i32::MIN; 3];
    for vertex in &mesh.vertices {
        for axis in 0..3 {
            low[axis] = low[axis].min(vertex[axis]);
            high[axis] = high[axis].max(vertex[axis]);
        }
    }
    let extents =
        mmorpg_core::physics::from_units(std::array::from_fn(|axis| high[axis] - low[axis]));
    let reach = extents.iter().map(|axis| axis * axis).sum::<f32>().sqrt() * 2. + 100.;
    let centre: [f32; 3] = std::array::from_fn(|axis| (a[axis] + b[axis] + c[axis]) / 3.);
    (
        std::array::from_fn(|axis| centre[axis] + normal[axis] * reach),
        normal.map(|axis| -2. * axis * reach),
    )
}

fn worlds(bodies: impl IntoIterator<Item = RigidBody>) -> (World, World) {
    let mut engine = World::with_backend(Vec3i::new(0, -64, 0), Backend::Engine);
    let mut rapier = World::with_backend(Vec3i::new(0, -64, 0), Backend::Rapier);
    for body in bodies {
        engine.add_body(body.clone()).unwrap();
        rapier.add_body(body).unwrap();
    }
    (engine, rapier)
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub fn mill_towpath_departure_settles_before_horizontal_motion() {
    let mesh = greyhaven_vale::fort::meshes()
        .into_iter()
        .find(|mesh| mesh.id == 65125)
        .expect("authored mill towpath");
    let player = BodyId(1_000_001);
    let (mut engine, mut rapier) = worlds([
        RigidBody::floor(BodyId(1), 0),
        RigidBody::static_mesh(&mesh).unwrap(),
        RigidBody::moving(
            player,
            [-3516.45, 96.25, 2142.5],
            [0.0; 3],
            Vec3i::new(30, 90, 30),
        ),
    ]);
    for tick in 0..8 {
        for world in [&mut engine, &mut rapier] {
            world.set_velocity(player, Vec3i::new(-2688, 0, 0)).unwrap();
            world.step().unwrap();
        }
        let a = engine.body(player).unwrap().exact_position();
        let b = rapier.body(player).unwrap().exact_position();
        for axis in 0..3 {
            assert!(
                (a[axis] - b[axis]).abs() < 0.3,
                "tick {tick}: {a:?} vs {b:?}"
            );
        }
    }
    assert!((engine.body(player).unwrap().exact_position()[1] - 90.0).abs() < 0.01);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub fn redbrand_json_rays_keep_floor_shelf_wall_and_ceiling_contacts() {
    let meshes = greyhaven_vale::cave::collision_meshes();
    // The committed floor, roof, shelf and wall IDs; no hardcoded collision coordinates.
    for expected in [640, 547, 645, 543] {
        let mesh = meshes.iter().find(|mesh| mesh.id == expected).unwrap();
        let (engine, rapier) = worlds([RigidBody::static_mesh(mesh).unwrap()]);
        let (origin, ray) = face_ray(mesh);
        let a = engine.cast_ray(origin, ray, |_| true).unwrap();
        let b = rapier.cast_ray(origin, ray, |_| true).unwrap();
        assert_eq!(a.body, BodyId(u64::from(expected)), "engine ray {origin:?}");
        assert_eq!(b.body, a.body, "Rapier ray {origin:?}");
        assert!(
            (a.fraction - b.fraction).abs() < 0.02,
            "authored contact fraction {origin:?}: {a:?} vs {b:?}"
        );
    }
}

fn first_greyhaven_rock() -> RigidBody {
    let content = greyhaven_vale::content();
    let &(id, shape) = content.rocks().first().expect("authored Greyhaven rocks");
    let collider = content
        .definition()
        .colliders()
        .iter()
        .find(|collider| collider.id == id)
        .unwrap();
    let [x, y, z] = collider.position;
    let [hx, hy, hz] = collider.half_extents;
    RigidBody::rock(
        BodyId(u64::from(id)),
        Vec3i::new(x, y, z),
        Vec3i::new(hx, hy, hz),
        shape,
    )
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub fn unit_placement_capsules_query_actual_meshes_and_hulls_on_both_backends() {
    let mesh = greyhaven_vale::cave::collision_meshes()
        .into_iter()
        .find(|mesh| mesh.id == 645)
        .unwrap();
    let (origin, ray) = face_ray(&mesh);
    let face = std::array::from_fn(|axis| origin[axis] + ray[axis] * 0.5);
    let rock = first_greyhaven_rock();
    let rock_centre = rock.exact_position();
    for (body, contact) in [
        (RigidBody::static_mesh(&mesh).unwrap(), face),
        (rock, rock_centre),
    ] {
        let expected = body.id();
        let (engine, rapier) = worlds([body]);
        for world in [&engine, &rapier] {
            assert_eq!(
                world.overlap_capsule(contact, [10., 20., 10.]).unwrap(),
                vec![expected]
            );
            let outside = [contact[0] + 100_000., contact[1], contact[2]];
            assert!(
                world
                    .overlap_capsule(outside, [10., 20., 10.])
                    .unwrap()
                    .is_empty()
            );
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub fn engine_surface_features_are_stable_for_the_actual_authored_inputs() {
    use physics_engine::approximate::{
        Config, ConvexHullDesc, Material, QueryFilter, RayCast, RayFeature, TriangleMeshDesc,
        Vector,
    };

    let mesh = greyhaven_vale::cave::collision_meshes()
        .into_iter()
        .find(|mesh| mesh.id == 645)
        .unwrap();
    let rock = first_greyhaven_rock();
    let to_vector = |value| {
        let [x, y, z] = mmorpg_core::physics::from_units(value);
        Vector(x, y, z)
    };
    let sample = || {
        let mut world = physics_engine::approximate::World::new(Config {
            length_unit: 100.,
            ..Config::default()
        })
        .unwrap();
        let mesh_handle = world
            .add_triangle_mesh(
                TriangleMeshDesc {
                    position: to_vector(mesh.position),
                    vertices: mesh.vertices.iter().copied().map(to_vector).collect(),
                    triangles: mesh.triangles.clone(),
                    material: Material::default(),
                },
                Some(u64::from(mesh.id)),
            )
            .unwrap();
        let (origin, ray) = face_ray(&mesh);
        let shelf = world
            .cast_ray_first(
                RayCast::new(
                    Vector(origin[0], origin[1], origin[2]),
                    Vector(ray[0], ray[1], ray[2]),
                ),
                QueryFilter::default(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(shelf.body, mesh_handle);
        assert!(matches!(shelf.feature, RayFeature::TriangleMeshTriangle(_)));
        let rock_centre = rock.exact_position();
        let mut rock_world = physics_engine::approximate::World::new(Config {
            length_unit: 100.,
            ..Config::default()
        })
        .unwrap();
        let rock_handle = rock_world
            .add_convex_hull(
                ConvexHullDesc {
                    position: Vector(rock_centre[0], rock_centre[1], rock_centre[2]),
                    vertices: greyhaven_vale::content()
                        .rock(u32::try_from(rock.id().0).unwrap())
                        .unwrap()
                        .hull(rock.half_extents())
                        .into_iter()
                        .map(|[x, y, z]| Vector(x, y, z))
                        .collect(),
                    material: Material::default(),
                },
                Some(rock.id().0),
            )
            .unwrap();
        let top = rock_centre[1]
            + mmorpg_core::physics::from_units([0, rock.half_extents().y, 0])[1]
            + 400.;
        let face = rock_world
            .cast_ray_first(
                RayCast::new(
                    Vector(rock_centre[0], top, rock_centre[2]),
                    Vector(0., -1_000., 0.),
                ),
                QueryFilter::default(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(face.body, rock_handle);
        assert!(matches!(face.feature, RayFeature::ConvexHullFace(_)));
        (shelf.feature, face.feature)
    };
    assert_eq!(
        sample(),
        sample(),
        "authored triangle/face IDs must be stable"
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub fn real_rock_hull_ray_and_capsule_cast_retain_game_identity() {
    let rock = first_greyhaven_rock();
    let centre = rock.exact_position();
    let half = rock.half_extents();
    let (mut engine, rapier) = worlds([rock.clone()]);
    let half_y = mmorpg_core::physics::from_units([0, half.y, 0])[1];
    let origin = [centre[0], centre[1] + half_y + 400., centre[2]];
    let ray = [0., -2. * half_y - 800., 0.];
    let a = engine.cast_ray(origin, ray, |_| true).unwrap();
    let b = rapier.cast_ray(origin, ray, |_| true).unwrap();
    assert_eq!(a.body, rock.id());
    assert_eq!(b.body, rock.id());
    assert!((a.fraction - b.fraction).abs() < 0.02, "{a:?} vs {b:?}");
    let capsule = Vec3i::new(30, 90, 30);
    let a = engine.cast_shape(origin, capsule, ray, |_| true).unwrap();
    let b = rapier.cast_shape(origin, capsule, ray, |_| true).unwrap();
    assert_eq!(a.body, rock.id());
    assert_eq!(b.body, rock.id());
    // Removing and re-admitting this immutable proxy must not change game-facing ray contact.
    assert_eq!(engine.remove_body(rock.id()), Some(rock.clone()));
    engine.add_body(rock).unwrap();
    assert_eq!(engine.cast_ray(origin, ray, |_| true).unwrap().body, b.body);
}

fn falling_box(at: [f32; 3]) -> DebrisBody {
    DebrisBody {
        position: at,
        rotation: [0., 0., 0., 1.],
        velocity: [0., -25., 0.],
        spin: [0.; 3],
        half_extents: Vec3i::new(25, 25, 25),
        resting: false,
    }
}

fn above_mesh(mesh: &mmorpg_core::StaticMesh) -> [f32; 3] {
    // The centroid of a real floor triangle stays over its surface even if the author changes
    // the mesh to a concave outline (its bounding-box centre could land in an open hole).
    let triangle = mesh.triangles[0].map(|index| mesh.vertices[index as usize]);
    let top = mesh.vertices.iter().map(|vertex| vertex[1]).max().unwrap();
    mmorpg_core::physics::from_units([
        mesh.position[0] + triangle.iter().map(|point| point[0]).sum::<i32>() / 3,
        mesh.position[1] + top + 250,
        mesh.position[2] + triangle.iter().map(|point| point[2]).sum::<i32>() / 3,
    ])
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub fn authored_rigid_boxes_keep_contacts_replay_and_exact_midpoint_recovery() {
    use rapier3d::prelude::{Cuboid, Pose, Rotation, SharedShape, Vector};
    let metres = |at: [f32; 3]| Vector::from(at) / 100.;
    let mesh = greyhaven_vale::cave::collision_meshes()
        .into_iter()
        .find(|mesh| mesh.id == 640)
        .unwrap();
    let mesh_shape = SharedShape::trimesh(
        mesh.vertices
            .iter()
            .map(|&vertex| metres(mmorpg_core::physics::from_units(vertex)))
            .collect(),
        mesh.triangles.clone(),
    )
    .unwrap();
    let rock = first_greyhaven_rock();
    let rock_top = rock.exact_position();
    let rock_shape = greyhaven_vale::content().rocks()[0].1;
    let hull = SharedShape::convex_hull(
        &rock_shape
            .hull(rock.half_extents())
            .into_iter()
            .map(metres)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let cases = [
        (
            RigidBody::static_mesh(&mesh).unwrap(),
            falling_box(above_mesh(&mesh)),
            metres(mmorpg_core::physics::from_units(mesh.position)),
            mesh_shape,
        ),
        (
            rock,
            falling_box([rock_top[0], rock_top[1] + 250., rock_top[2]]),
            metres(rock_top),
            hull,
        ),
    ];
    for (body, piece, origin, shape) in cases {
        // A catch floor below the entire authored shape permits a piece that rolls off a
        // rock to settle. Contact with the authored shape itself is required independently.
        let floor_y = body.position().y - body.half_extents().y - 100;
        let floor = RigidBody::floor(BodyId(9_000_001), floor_y);
        for upright in [false, true] {
            let make = || worlds([body.clone(), floor.clone()]);
            let (mut engine, mut rapier) = make();
            let mut replay = make().0;
            let mut restored: Option<(World, Vec<DebrisBody>)> = None;
            let (mut engine_piece, mut rapier_piece, mut replay_piece) =
                ([piece], [piece], [piece]);
            let step = |world: &mut World, pieces: &mut [DebrisBody]| {
                if upright {
                    world.step_props(pieces, &[])
                } else {
                    world.step_debris(pieces, &[true])
                }
            };
            let separation = |piece: &DebrisBody, position: Vector, target: &SharedShape| {
                let [x, y, z, w] = piece.rotation;
                let pose =
                    Pose::from_parts(metres(piece.position), Rotation::from_xyzw(x, y, z, w));
                let half = metres(mmorpg_core::physics::from_units([
                    piece.half_extents.x,
                    piece.half_extents.y,
                    piece.half_extents.z,
                ]));
                rapier3d::parry::query::contact(
                    &pose,
                    &Cuboid::new(half),
                    &Pose::from_translation(position),
                    target.as_ref(),
                    0.005,
                )
                .unwrap()
                .map(|contact| contact.dist * 100.)
            };
            let mut depths = [0_f32; 2];
            let mut touched = [false; 2];
            let mut drift = 0_f32;
            for tick in 0..360 {
                step(&mut engine, &mut engine_piece);
                step(&mut rapier, &mut rapier_piece);
                step(&mut replay, &mut replay_piece);
                assert_eq!(
                    engine_piece, replay_piece,
                    "authored rigid replay, tick {tick}"
                );
                if let Some((world, state)) = &mut restored {
                    step(world, state);
                    assert_eq!(
                        engine_piece.as_slice(),
                        state.as_slice(),
                        "authored rigid recovery, tick {tick}"
                    );
                }
                if tick == 179 {
                    restored = Some((make().0, engine_piece.to_vec()));
                }
                let [a, b] = [engine_piece[0].position, rapier_piece[0].position];
                drift = drift.max(
                    (0..3)
                        .map(|axis| (a[axis] - b[axis]).powi(2))
                        .sum::<f32>()
                        .sqrt(),
                );
                for (index, state) in [engine_piece[0], rapier_piece[0]].iter().enumerate() {
                    assert!(
                        state
                            .position
                            .iter()
                            .chain(&state.velocity)
                            .chain(&state.rotation)
                            .all(|value| value.is_finite())
                    );
                    if let Some(depth) = separation(state, origin, &shape) {
                        touched[index] = true;
                        depths[index] = depths[index].min(depth);
                    }
                    let [x, y, z, w] = state.rotation;
                    let rotation = Rotation::from_xyzw(x, y, z, w);
                    let half = metres(mmorpg_core::physics::from_units([
                        state.half_extents.x,
                        state.half_extents.y,
                        state.half_extents.z,
                    ]));
                    let radius_y = (rotation * Vector::new(half.x, 0., 0.)).y.abs()
                        + (rotation * Vector::new(0., half.y, 0.)).y.abs()
                        + (rotation * Vector::new(0., 0., half.z)).y.abs();
                    let floor_gap = state.position[1]
                        - radius_y * 100.
                        - mmorpg_core::physics::from_units([0, floor_y, 0])[1];
                    depths[index] = depths[index].min(floor_gap);
                }
            }
            println!(
                "authored {:?} upright={upright}: max drift={drift}, min separations={depths:?}, final Engine={:?}, Rapier={:?}",
                body.id(),
                engine_piece[0],
                rapier_piece[0]
            );
            assert_eq!(
                touched, [true; 2],
                "both solvers must contact actual authored geometry"
            );
            // The upstream real-cave oracle permits 0.5 cm beyond Rapier's worst depth,
            // with an absolute five-centimetre ceiling. Tumbling orientations may diverge.
            assert!(
                depths[0] >= depths[1] - 0.5 && depths[0] >= -5.,
                "authored rigid nonembedding: {depths:?}"
            );
            assert!(
                engine_piece[0].resting && rapier_piece[0].resting,
                "both complete the fall and settle"
            );
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub fn an_authored_wall_scrape_preserves_ascent_but_its_underside_stops_it() {
    let mesh = greyhaven_vale::fort::meshes()
        .into_iter()
        .find(|mesh| mesh.id == 65057)
        .unwrap();
    for backend in [Backend::Engine, Backend::Rapier] {
        for (at, desired, ceiling) in [
            (
                [3339.7393, 211.5, 967.08386],
                [4.0585938, 1.0, 2.8398438],
                false,
            ),
            ([3400.0, -90.5, 850.0], [0.0, 1.0, 0.0], true),
        ] {
            let mut world = World::with_backend(Vec3i::new(0, -64, 0), backend);
            world
                .add_body(RigidBody::static_mesh(&mesh).unwrap())
                .unwrap();
            let mut velocity = desired;
            velocity[1] += 0.25;
            world
                .add_body(RigidBody::moving(
                    BodyId(9000000),
                    at,
                    velocity,
                    Vec3i::new(30, 90, 30),
                ))
                .unwrap();
            world.step().unwrap();
            let body = world.body(BodyId(9000000)).unwrap();
            if ceiling {
                assert_eq!(
                    body.velocity_units().y,
                    0,
                    "{backend:?}: underside stops ascent"
                );
            } else {
                assert!(
                    body.velocity_units().y > 0,
                    "{backend:?}: side scrape is no ceiling"
                );
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub fn terrain_probes_agree_at_the_charge_paths_half_unit_rounding_tie() {
    let content = greyhaven_vale::content();
    let terrain = content.definition().terrain().unwrap();
    let heights = [Backend::Engine, Backend::Rapier].map(|backend| {
        let mut world = World::with_backend(Vec3i::ZERO, backend);
        world.set_terrain(BodyId(1), terrain.clone());
        world.terrain_height(-7100, -900).unwrap()
    });
    assert_eq!(heights[0].round(), heights[1].round(), "{heights:?}");
    assert!((heights[0] - heights[1]).abs() < 0.0005, "{heights:?}");
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub fn the_slanted_redbrand_wall_still_stops_an_upward_move() {
    let mesh = greyhaven_vale::cave::collision_meshes()
        .into_iter()
        .find(|mesh| mesh.id == 544)
        .unwrap();
    for backend in [Backend::Engine, Backend::Rapier] {
        let mut world = World::with_backend(Vec3i::new(0, -64, 0), backend);
        world
            .add_body(RigidBody::static_mesh(&mesh).unwrap())
            .unwrap();
        world
            .add_body(RigidBody::moving(
                BodyId(9000000),
                [8060.0, 1300.2351, -13805.5],
                [0.0, 2.75, 0.0],
                Vec3i::new(30, 90, 30),
            ))
            .unwrap();
        world.step().unwrap();
        assert_eq!(
            world.body(BodyId(9000000)).unwrap().velocity_units().y,
            0,
            "{backend:?}: real slanted contact still stops ascent"
        );
    }
}
