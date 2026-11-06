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

#[test]
fn redbrand_json_rays_keep_floor_shelf_wall_and_ceiling_contacts() {
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

#[test]
fn engine_surface_features_are_stable_for_the_actual_authored_inputs() {
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

#[test]
fn real_rock_hull_ray_and_capsule_cast_retain_game_identity() {
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

#[test]
fn unsupported_dynamic_box_contacts_use_rapier_against_real_mesh_and_rock() {
    let mesh = greyhaven_vale::cave::collision_meshes()
        .into_iter()
        .find(|mesh| mesh.id == 640)
        .unwrap();
    let rock = first_greyhaven_rock();
    let rock_top = rock.exact_position();
    let mesh_drop = falling_box(above_mesh(&mesh));
    for (body, piece) in [
        (RigidBody::static_mesh(&mesh).unwrap(), mesh_drop),
        (
            rock,
            falling_box([rock_top[0], rock_top[1] + 250., rock_top[2]]),
        ),
    ] {
        let (engine, rapier) = worlds([body]);
        let (mut engine_debris, mut rapier_debris) = ([piece], [piece]);
        for _ in 0..25 {
            let a = engine.step_debris(&mut engine_debris, &[false]);
            let b = rapier.step_debris(&mut rapier_debris, &[false]);
            assert_eq!(a, b);
            assert_eq!(
                engine_debris, rapier_debris,
                "debris must use Rapier's full geometry"
            );
        }
        let (mut engine_prop, mut rapier_prop) = ([piece], [piece]);
        for _ in 0..25 {
            let a = engine.step_props(&mut engine_prop, &[]);
            let b = rapier.step_props(&mut rapier_prop, &[]);
            assert_eq!(a, b);
            assert_eq!(
                engine_prop, rapier_prop,
                "props must use Rapier's full geometry"
            );
        }
    }
}
