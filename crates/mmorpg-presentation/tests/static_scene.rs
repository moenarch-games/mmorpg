//! Behaviour of the shared static world both clients draw (#184).
use mmorpg_core::greyhaven_vale_definition;
use mmorpg_presentation::{Cull, Draw, Kits, StaticScene, geometry::V};
use mmorpg_scenery::{Prop, PropKind, RockSize, Scenery, TreeVariant, greyhaven_vale_scenery};

fn only(props: Vec<Prop>) -> StaticScene {
    let mut scenery = greyhaven_vale_scenery();
    scenery.props = props;
    StaticScene::new(&scenery)
}

fn first(scenery: &Scenery, kind: PropKind) -> Prop {
    *scenery.props.iter().find(|prop| prop.kind == kind).unwrap()
}

fn transform(model: &[f32; 16], p: V) -> V {
    let m = model.map(f64::from);
    [
        m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12],
        m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13],
        m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14],
    ]
}

/// Prop draws (everything but scene-wide terrain, patches, roads and lake beds) in world space.
fn prop_triangles(scene: &StaticScene) -> Vec<[V; 3]> {
    let rest = scene.motion(0., false);
    let placed = scene
        .groups
        .iter()
        .flat_map(|group| &group.draws)
        .filter(|draw| !draw.id.starts_with("static-all-"))
        .map(|draw| (draw, draw.model))
        .chain(
            scene
                .animated
                .iter()
                .zip(rest)
                .flat_map(|(part, model)| part.draws.iter().map(move |draw| (draw, model))),
        );
    placed
        .flat_map(|(draw, model)| {
            let g = &draw.geometry;
            g.indices
                .chunks_exact(3)
                .map(|tri| std::array::from_fn(|k| transform(&model, g.p[tri[k] as usize])))
                .collect::<Vec<_>>()
        })
        .collect()
}

fn assert_valid(draw: &Draw) {
    let g = &draw.geometry;
    assert!(
        !g.indices.is_empty() && g.indices.len() % 3 == 0,
        "{}",
        draw.id
    );
    assert_eq!(g.n.len(), g.p.len(), "{}", draw.id);
    assert!(
        g.colors.is_empty() || g.colors.len() == g.p.len(),
        "{}",
        draw.id
    );
    assert!(
        g.indices.iter().all(|i| (*i as usize) < g.p.len()),
        "{}",
        draw.id
    );
    for n in &g.n {
        let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        assert!((length - 1.).abs() < 1e-6, "{}: normal {n:?}", draw.id);
    }
}

#[test]
fn every_prop_kind_draws_valid_geometry_and_the_same_scenery_builds_the_same_scene() {
    let scenery = greyhaven_vale_scenery();
    let scene = StaticScene::new(&scenery);
    for draw in scene
        .groups
        .iter()
        .flat_map(|group| &group.draws)
        .chain(&scene.water)
        .chain(scene.animated.iter().flat_map(|part| &part.draws))
    {
        assert_valid(draw);
    }
    // The inn and the Sunken Shrine are drawn from their asset-tooling bundles
    // (`mmorpg_presentation::buildings`, `mmorpg_presentation::shrine`).
    for kind in PropKind::ALL
        .into_iter()
        .filter(|kind| !matches!(kind, PropKind::Inn | PropKind::ShrineKit))
    {
        assert!(
            !prop_triangles(&only(vec![first(&scenery, kind)])).is_empty(),
            "{kind:?} draws nothing"
        );
    }
    assert_eq!(StaticScene::new(&scenery).prefix, scene.prefix);
    assert!(
        scene
            .prefix
            .starts_with(&format!("scenery:{}:", scenery.content_revision))
    );
    // Terrain is vertex coloured with upward faces; water stays translucent; lights glow.
    let terrain: Vec<_> = scene
        .groups
        .iter()
        .flat_map(|group| &group.draws)
        .filter(|draw| draw.id.starts_with("static-all-terrain"))
        .collect();
    assert!(!terrain.is_empty());
    assert!(terrain.iter().all(|draw| !draw.geometry.colors.is_empty()));
    assert!(scene.water.iter().all(|draw| draw.material.opacity < 1.));
    assert!(
        scene
            .groups
            .iter()
            .flat_map(|group| &group.draws)
            .any(|draw| draw.material.emissive != [0.; 3])
    );
}

#[test]
fn props_stand_on_the_drawn_terrain() {
    let scenery = greyhaven_vale_scenery();
    let scene = StaticScene::new(&scenery);
    for kind in [
        PropKind::Tree(TreeVariant::Oak),
        PropKind::GrassTuft,
        PropKind::Flowers,
        PropKind::House,
    ] {
        let prop = first(&scenery, kind);
        let (x, z) = (
            f64::from(prop.position[0]) / 100.,
            f64::from(prop.position[2]) / 100.,
        );
        let surface = scene.surface_y(x, z);
        let lowest = prop_triangles(&only(vec![prop]))
            .into_iter()
            .flatten()
            .map(|p| p[1])
            .fold(f64::INFINITY, f64::min);
        // Trunks and blades reach a little into the ground; nothing hovers above it.
        assert!(
            lowest <= surface + 0.02,
            "{kind:?}: {lowest} above {surface}"
        );
        assert!(
            lowest > surface - 0.35,
            "{kind:?}: {lowest} sinks below {surface}"
        );
    }
}

/// XZ points where prop triangles cross the horizontal plane `y`.
fn slice(triangles: &[[V; 3]], y: f64) -> Vec<[f64; 2]> {
    let mut points = Vec::new();
    for tri in triangles {
        for (a, b) in [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
            if (a[1] - y) * (b[1] - y) < 0. {
                let t = (y - a[1]) / (b[1] - a[1]);
                points.push([a[0] + (b[0] - a[0]) * t, a[2] + (b[2] - a[2]) * t]);
            }
        }
    }
    points
}

#[test]
fn walls_at_body_height_stay_on_their_collider() {
    let scenery = greyhaven_vale_scenery();
    let definition = greyhaven_vale_definition();
    // Loose dressing (the barn's hay bales, the inn's barrels) may stand outside; walls may not.
    for kind in [
        PropKind::House,
        PropKind::Keep,
        PropKind::Windmill,
        PropKind::PalisadeSegment,
        PropKind::GatePost,
        PropKind::Waystone,
        PropKind::Well,
        PropKind::Cliff,
    ] {
        let prop = first(&scenery, kind);
        let collider = definition
            .colliders()
            .iter()
            .find(|collider| Some(collider.id) == prop.collider)
            .unwrap();
        let centre = collider.position.map(|v| f64::from(v) / 100.);
        let half = collider.half_extents.map(|v| f64::from(v) / 100.);
        let scene = only(vec![prop]);
        let ground = scene.surface_y(centre[0], centre[2]);
        let triangles = prop_triangles(&scene);
        let mut crossed = 0;
        for lift in [0.5, 1.0, 1.6] {
            for [x, z] in slice(&triangles, ground + lift) {
                crossed += 1;
                assert!((x - centre[0]).abs() <= half[0] + 0.35, "{kind:?} x {x}");
                assert!((z - centre[2]).abs() <= half[2] + 0.35, "{kind:?} z {z}");
            }
        }
        assert!(crossed > 0, "{kind:?} has no walls at body height");
    }
}

#[test]
fn repeated_props_add_instance_transforms_not_geometry() {
    let scenery = greyhaven_vale_scenery();
    for kind in [
        PropKind::Tree(TreeVariant::Oak),
        PropKind::Tree(TreeVariant::Pine),
        PropKind::GrassTuft,
        PropKind::Rock(RockSize::Small),
    ] {
        let prop = Prop {
            position: [0, 0, 0],
            ..first(&scenery, kind)
        };
        let single = only(vec![prop]);
        let repeated = only(vec![prop; 5]);
        let instanced = |scene: &StaticScene| {
            scene
                .groups
                .iter()
                .filter(|group| group.instanced)
                .map(|group| (group.draws[0].resource_key.clone(), group.draws.len()))
                .collect::<Vec<_>>()
        };
        let (one, five) = (instanced(&single), instanced(&repeated));
        assert!(!one.is_empty(), "{kind:?}");
        assert_eq!(one.len(), five.len(), "{kind:?}");
        for ((_, a), (_, b)) in one.iter().zip(&five) {
            assert_eq!(*b, a * 5, "{kind:?}");
        }
        let ordinary = |scene: &StaticScene| {
            scene
                .groups
                .iter()
                .filter(|group| !group.instanced && !group.draws[0].id.starts_with("static-all-"))
                .count()
        };
        assert_eq!((ordinary(&single), ordinary(&repeated)), (0, 0), "{kind:?}");
    }
}

#[test]
fn scenery_classes_keep_their_distance_policy() {
    let scenery = greyhaven_vale_scenery();
    for (kind, collidable, cull) in [
        (PropKind::Tree(TreeVariant::Oak), false, Cull::Always),
        (PropKind::Tree(TreeVariant::Pine), true, Cull::Far),
        (PropKind::Rock(RockSize::Small), false, Cull::Mid),
        (PropKind::Rock(RockSize::Large), true, Cull::Always),
        (PropKind::GrassTuft, false, Cull::Near),
    ] {
        let original = first(&scenery, kind);
        let prop = Prop {
            collider: collidable.then_some(original.collider.unwrap_or(1)),
            ..original
        };
        let scene = only(vec![prop]);
        let groups: Vec<_> = scene
            .groups
            .iter()
            .filter(|group| group.instanced)
            .collect();
        assert!(!groups.is_empty(), "{kind:?}");
        for group in groups {
            assert_eq!(group.cull, cull, "{kind:?}");
            let centre = [
                (group.min[0] + group.max[0]) / 2.,
                (group.min[1] + group.max[1]) / 2.,
            ];
            let radius = (group.max[0] - group.min[0]).hypot(group.max[1] - group.min[1]) / 2.;
            let at = |x: f64| [x as f32, 5., centre[1] as f32];
            if cull == Cull::Always {
                assert!(group.visible(at(centre[0] + 1e6)));
            } else {
                let edge = centre[0] + radius + cull.reach();
                assert!(group.visible(at(edge - 0.01)), "{kind:?}");
                assert!(!group.visible(at(edge + 0.01)), "{kind:?}");
            }
        }
    }
}

#[test]
fn animated_parts_only_move_with_time() {
    let scene = StaticScene::new(&greyhaven_vale_scenery());
    assert!(!scene.animated.is_empty());
    assert_eq!(scene.motion(3., false), scene.motion(9., false));
    assert_ne!(scene.motion(9., true), scene.motion(3., false));
    let ids = |draws: Vec<Draw>| draws.into_iter().map(|draw| draw.id).collect::<Vec<_>>();
    assert_eq!(
        ids(scene.frame(9., true, [0., 5., 0.], None, Kits::default())),
        ids(scene.frame(3., false, [0., 5., 0.], None, Kits::default()))
    );
    assert!(
        scene
            .frame(0., false, [5_000., 5., 5_000.], None, Kits::default())
            .len()
            < scene
                .frame(0., false, [0., 5., 0.], None, Kits::default())
                .len()
    );
}

#[test]
fn the_cave_roof_is_cut_away_while_the_viewer_stands_under_it() {
    let scenery = greyhaven_vale_scenery();
    let roof = first(&scenery, PropKind::CaveRoof);
    let scene = only(vec![roof]);
    assert_eq!(scene.animated.len(), 1, "the roof is its own part");
    let [x, _, z] = roof.position.map(|units| f64::from(units) / 100.);
    let shown = |viewer| scene.shown(viewer, Kits::default())[0];
    assert!(shown(None));
    assert!(!shown(Some([x, 0., z])), "inside the cave");
    assert!(shown(Some([x, 6., z])), "standing on the roof");
    assert!(shown(Some([x, 0., z + 30.])), "outside");
    let drawn = |viewer| {
        scene
            .frame(0., false, [x as f32, 5., z as f32], viewer, Kits::default())
            .len()
    };
    assert!(drawn(Some([x, 0., z])) < drawn(None));
}

#[test]
fn the_outpost_kit_buildings_keep_their_blockouts_only_as_the_fallback() {
    let scenery = greyhaven_vale_scenery();
    let scene = StaticScene::new(&scenery);
    let fallback = |kit| {
        scene
            .frame(
                0.,
                false,
                [0., 5., 20.],
                None,
                Kits {
                    outpost: kit,
                    ..Kits::default()
                },
            )
            .iter()
            .filter(|draw| draw.id.contains("outpost-fallback-"))
            .count()
    };
    assert!(fallback(false) > 0);
    assert_eq!(fallback(true), 0);
    // Every wall module stands on its building's unchanged collider footprint.
    let parts = mmorpg_presentation::outpost_kit::parts(&scenery);
    assert_eq!(parts.len(), 29 + 28);
    for collider in [102, 105] {
        let prop = scenery
            .props
            .iter()
            .find(|prop| prop.collider == Some(collider))
            .unwrap();
        let [cx, _, cz] = prop.position.map(|units| f64::from(units) / 100.);
        let [hx, _, hz] = prop.half_extents.map(|units| f64::from(units) / 100.);
        let walls = mmorpg_scenery::outpost_kit::placements(&scenery)
            .into_iter()
            .filter(|placement| {
                placement.collider == collider && placement.entry.starts_with("outpost.kit.wall")
            })
            .collect::<Vec<_>>();
        assert!(!walls.is_empty());
        for wall in walls {
            let [x, _, z] = wall.position.map(|units| f64::from(units) / 100.);
            let on_edge = ((x - cx).abs() - hx).abs() < 1e-9 || ((z - cz).abs() - hz).abs() < 1e-9;
            assert!(on_edge, "{} at {x}, {z} on collider {collider}", wall.entry);
        }
    }
}

#[test]
fn the_market_square_dresses_the_outpost_without_touching_colliders_or_paths() {
    use mmorpg_scenery::settlement_props;
    let scenery = greyhaven_vale_scenery();
    let scene = StaticScene::new(&scenery);
    let fallback = |kit| {
        scene
            .frame(
                0.,
                false,
                [24., 5., 27.],
                None,
                Kits {
                    settlement_props: kit,
                    ..Kits::default()
                },
            )
            .iter()
            .filter(|draw| draw.id.contains("settlement-fallback-"))
            .count()
    };
    assert!(fallback(false) > 0);
    assert_eq!(fallback(true), 0);
    // One adopted prop or more from each family, shared by entry.
    let parts = mmorpg_presentation::settlement_props::parts(&scenery);
    assert_eq!(parts.len(), 21);
    for family in [
        ["stall", "cart", "crate"],
        ["signpost", "lantern-post", "bench"],
        ["fence", "planter", "firewood"],
    ] {
        for prop in family {
            let entry = format!("settlement.props.{prop}");
            assert!(parts.iter().any(|part| part.entry == entry), "{entry}");
        }
    }
    // The square stays off the roads, the spawn plaza and every collider's footprint.
    let [cx, cz] = settlement_props::centre(&scenery).unwrap();
    let [hx, hz] = settlement_props::HALF_EXTENTS;
    let square = mmorpg_core::StaticCollider {
        id: 0,
        position: [cx, 100, cz],
        half_extents: [hx, 100, hz],
    };
    assert!(mmorpg_core::greyhaven_vale::clear_of_roads(&square));
    let plaza = mmorpg_core::greyhaven_vale::SPAWN_PLAZA;
    assert!(cx - hx > plaza.max[0] || cz - hz > plaza.max[1]);
    for collider in greyhaven_vale_definition().colliders() {
        let [px, _, pz] = collider.position;
        let [phx, _, phz] = collider.half_extents;
        let apart = (px - cx).abs() >= phx + hx || (pz - cz).abs() >= phz + hz;
        assert!(apart || collider.id == 1, "collider {}", collider.id);
    }
}
