//! Greyhaven's waterfall, brook banks, timber crossing and moving foam (#513).
//! Authored course, water level and traversable structures come from core; this is only art.
use crate::{
    Animated, Draw, Group, Kit, Motion,
    batching::{Batcher, Cull, material, matrix, mix, rgb},
    geometry::{Geometry, ID, at, blob, box_shape, frustum, norm},
    ground::{self, Ground, Layer},
    terrain::{metres, noise, surface},
};
use mmorpg_core::greyhaven_vale::brook as content;
use mmorpg_scenery::{Scenery, TerrainGrid};
use std::sync::Arc;

fn draw(id: &str, g: Geometry, color: u32, opacity: f32) -> Draw {
    let mut m = material(color, false);
    m.opacity = opacity;
    Draw {
        id: id.to_owned(),
        resource_key: format!("scenery:{id}"),
        geometry: Arc::new(g),
        model: matrix(ID),
        material: m,
    }
}
fn offset(points: &[[i32; 2]], i: usize, width: f64) -> [f64; 2] {
    let a = points[i.saturating_sub(1)].map(metres);
    let b = points[(i + 1).min(points.len() - 1)].map(metres);
    let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
    let l = dx.hypot(dz);
    [-dz / l * width, dx / l * width]
}

pub(crate) fn build(
    b: &mut Batcher,
    s: &Scenery,
    g: &TerrainGrid,
    water: &mut Vec<Draw>,
    animated: &mut Vec<Animated>,
) -> Vec<Group> {
    if !s.greyhaven() {
        return vec![];
    }
    let mut banks = Geometry::default();
    let mut shingle = Geometry::default();
    for (name, points, half_width, water_surface) in [
        (
            "greyhaven-brook",
            content::village(),
            content::HALF_WIDTH,
            content::SURFACE,
        ),
        (
            "stillwater-inflow",
            content::inflow(),
            content::UPPER_HALF_WIDTH,
            content::UPPER_SURFACE,
        ),
        (
            "stillwater-outlet",
            content::outlet(),
            content::UPPER_HALF_WIDTH,
            content::UPPER_SURFACE,
        ),
    ] {
        let mut channel = Geometry::default();
        let half_width = metres(half_width);
        let level = metres(water_surface) + 0.012;
        for i in 0..points.len() - 1 {
            let a = points[i].map(metres);
            let z = points[i + 1].map(metres);
            let left = offset(points, i, half_width);
            let right = offset(points, i + 1, half_width);
            let length = (z[0] - a[0]).hypot(z[1] - a[1]);
            let steps = (length / 2.).ceil() as usize;
            for step in 0..steps {
                let t0 = step as f64 / steps as f64;
                let t1 = (step + 1) as f64 / steps as f64;
                let p = |t: f64, side: f64, edge: f64| {
                    let x = a[0] + (z[0] - a[0]) * t;
                    let v = a[1] + (z[1] - a[1]) * t;
                    let nx = (left[0] + (right[0] - left[0]) * t) / half_width;
                    let nz = (left[1] + (right[1] - left[1]) * t) / half_width;
                    // Slightly uneven edges stay inside the authoritative water lane.
                    // World-space noise keeps adjoining segments joined at their endpoints.
                    let width = half_width * (0.86 + 0.14 * noise(x, v, 1.7, 570. + side))
                        + edge * (0.65 + 1.25 * noise(x, v, 4., 571.));
                    let x = x + nx * side * width;
                    let v = v + nz * side * width;
                    [x, surface(g, x, v) + 0.085, v]
                };
                let water_edge = |t, side| {
                    let [x, _, edge_z] = p(t, side, 0.);
                    let centre = [a[0] + (z[0] - a[0]) * t, a[1] + (z[1] - a[1]) * t];
                    // The physical water lane can meet the four-metre terrain mesh partway
                    // across a bank. Stop the visible surface at that intersection instead
                    // of letting its translucent plane cut through higher ground.
                    let mut inside = 0.;
                    let mut outside = 1.;
                    if surface(g, x, edge_z) < level - 0.015 {
                        inside = 1.;
                    } else {
                        for _ in 0..7 {
                            let middle = (inside + outside) / 2.;
                            let sx = centre[0] + (x - centre[0]) * middle;
                            let sz = centre[1] + (edge_z - centre[1]) * middle;
                            if surface(g, sx, sz) < level - 0.015 {
                                inside = middle;
                            } else {
                                outside = middle;
                            }
                        }
                    }
                    [
                        centre[0] + (x - centre[0]) * inside,
                        level,
                        centre[1] + (edge_z - centre[1]) * inside,
                    ]
                };
                let corners = [
                    water_edge(t0, 1.),
                    water_edge(t1, 1.),
                    water_edge(t1, -1.),
                    water_edge(t0, -1.),
                ];
                channel.face(&corners);
                let current = noise(
                    a[0] + (z[0] - a[0]) * t0,
                    a[1] + (z[1] - a[1]) * t0,
                    3.,
                    718.,
                );
                channel.colors.extend(corners.map(|point| {
                    let depth = ((level - surface(g, point[0], point[2])) / 0.35).clamp(0., 1.);
                    rgb(mix(
                        mix(0x667c61, 0x2f6565, depth),
                        0x7c9a83,
                        current * 0.10,
                    ))
                    .map(f64::from)
                }));
                if name == "greyhaven-brook" && step % 2 == 0 {
                    let t = (t0 + t1) * 0.5;
                    let x = a[0] + (z[0] - a[0]) * t;
                    let z = a[1] + (z[1] - a[1]) * t;
                    let y = surface(g, x, z);
                    if level - y > 0.12 && noise(x, z, 3., 51420.) > 0.4 {
                        for pebble in 0..5 {
                            let seed = (i * 31 + step * 7 + pebble) as f64;
                            let x = x + (noise(seed, 0., 0.7, 51421.) - 0.5) * 0.65;
                            let z = z + (noise(seed, 0., 0.7, 51422.) - 0.5) * 0.65;
                            let y = surface(g, x, z);
                            let r = 0.07 + noise(seed, 0., 0.7, 51423.) * 0.10;
                            let mut stone = blob(0, true, 0.15, seed);
                            stone.colors =
                                vec![
                                    rgb(mix(0x686c59, 0x99917b, noise(x, z, 0.3, 51424.)))
                                        .map(f64::from);
                                    stone.p.len()
                                ];
                            shingle.add(&stone, at(x, y + 0.005, z, r, 0.035, r * 0.8));
                        }
                    }
                }
                for side in [-1., 1.] {
                    // Small, partly buried shingle clusters break up the bare mud verge.
                    // Keep the bridge landings clear and omit steep ground where a pebble
                    // would float. These low decorative stones do not obstruct traversal.
                    let mid = p((t0 + t1) * 0.5, side, 0.55);
                    if name == "greyhaven-brook"
                        && noise(mid[0], mid[2], 2.5, 812.) > 0.47
                        && !(mid[0].abs() < 2.5 && (28.0..=36.0).contains(&mid[2]))
                    {
                        for pebble in 0..7 {
                            let seed = i as f64 * 31. + step as f64 * 7. + pebble as f64;
                            let x = mid[0] + (noise(seed, side, 0.7, 813.) - 0.5) * 1.25;
                            let z = mid[2] + (noise(seed, side, 0.7, 814.) - 0.5) * 1.25;
                            let y = surface(g, x, z);
                            let radius = 0.08 + 0.14 * noise(x, z, 0.3, 815.);
                            if [[radius, 0.], [-radius, 0.], [0., radius], [0., -radius]]
                                .iter()
                                .any(|[dx, dz]| (surface(g, x + dx, z + dz) - y).abs() > 0.055)
                            {
                                continue;
                            }
                            let mut stone = blob(0, true, 0.16, seed);
                            let color =
                                rgb(mix(0x777a70, 0xa7a28c, noise(x, z, 0.4, 816.))).map(f64::from);
                            stone.colors = vec![color; stone.p.len()];
                            shingle.add(&stone, at(x, y + 0.035, z, radius, 0.09, radius * 0.75));
                        }
                    }
                    for [inner, outer] in [[0., 0.45], [0.45, 1.]] {
                        let q = [
                            p(t0, side, inner),
                            p(t1, side, inner),
                            p(t1, side, outer),
                            p(t0, side, outer),
                        ];
                        let weight = |edge: f64| (1. - edge) / f64::from(ground::OVERLAY_ALPHA);
                        let alpha = [weight(inner), weight(inner), weight(outer), weight(outer)];
                        if side > 0. {
                            banks.face(&[q[3], q[2], q[1], q[0]]);
                            banks.alpha.extend(alpha.into_iter().rev());
                        } else {
                            banks.face(&q);
                            banks.alpha.extend(alpha);
                        }
                    }
                }
            }
            // Thin glints follow the current rather than painting white bars across every
            // reach. The deeper centre runs faster than the margins. Every translated
            // vertex stays over wet ground throughout its existing Flow animation.
            let (dx, dz) = ((z[0] - a[0]) / length, (z[1] - a[1]) / length);
            for lane in 0..2 {
                let across = if lane == 0 { 0. } else { 0.42 } * half_width;
                let mut streak = Geometry::default();
                for segment in 0..7 {
                    if (segment + lane + i) % 4 == 0 {
                        continue;
                    }
                    let point = |j: usize, edge: f64| {
                        let t = j as f64 / 7.;
                        let along = (t - 0.5) * 0.6;
                        let bend = (t * std::f64::consts::PI).sin() * 0.035;
                        let width = 0.008 * (t * std::f64::consts::PI).sin().max(0.15) * edge;
                        [
                            -dz * (across + bend + width) + dx * along,
                            0.,
                            dx * (across + bend + width) + dz * along,
                        ]
                    };
                    streak.face(&[
                        point(segment, -1.),
                        point(segment, 1.),
                        point(segment + 1, 1.),
                        point(segment + 1, -1.),
                    ]);
                }
                let inset = 0.4_f64.min(length * 0.2);
                let start = [a[0] + dx * inset, level + 0.004, a[1] + dz * inset];
                let end = [z[0] - dx * inset, level + 0.004, z[1] - dz * inset];
                if wet_flow(g, &streak, start, end, level) {
                    animated.push(Animated {
                        draws: vec![draw(
                            &format!("{name}-current-{i}-{lane}"),
                            streak,
                            0xa5b9a7,
                            0.26,
                        )],
                        motion: Motion::Flow {
                            start,
                            end,
                            period: (length / if lane == 0 { 0.72 } else { 0.43 }).max(1.),
                            phase: i as f64 * 0.31 + lane as f64 * 0.47,
                        },
                    });
                }
            }
        }
        // Apparent ripples affect lighting only; the authoritative water plane is unchanged.
        for (point, normal) in channel.p.iter().zip(&mut channel.n) {
            *normal = norm([
                (noise(point[0], point[2], 0.65, 719.) - 0.5) * 0.15,
                1.,
                (noise(point[0], point[2], 0.85, 720.) - 0.5) * 0.11,
            ]);
        }
        water.push(draw(name, channel, 0xffffff, 0.66));
    }
    let level = metres(content::SURFACE) + 0.012;
    let mut fall = Geometry::default();
    let cascade = [
        [-48., metres(content::UPPER_SURFACE) + 0.012],
        [-46.4, 10.8],
        [-45.8, 4.8],
        [-44., 1.4],
        [-42.4, level],
    ];
    for pair in cascade.windows(2) {
        fall.face(&[
            [pair[0][0], pair[0][1], 23.45],
            [pair[0][0], pair[0][1], 24.55],
            [pair[1][0], pair[1][1], 24.55],
            [pair[1][0], pair[1][1], 23.45],
        ]);
    }
    water.push(draw("greyhaven-waterfall", fall, 0x9ed1d2, 0.78));
    // White strands move down the actual cliff face, with a small plunge-pool foam ring.
    for i in 0..7 {
        let mut strand = Geometry::default();
        strand.add(&box_shape(), at(0., 0., 0., 0.028, 0.35, 0.025));
        animated.push(Animated {
            draws: vec![draw(&format!("fall-strand-{i}"), strand, 0xcce8df, 0.8)],
            motion: Motion::Flow {
                start: [
                    -47.95,
                    metres(content::UPPER_SURFACE) + 0.02,
                    23.55 + i as f64 * 0.15,
                ],
                end: [-42.4, 0.02, 23.55 + i as f64 * 0.15],
                period: 1.5,
                phase: i as f64 / 7.,
            },
        });
    }
    // Aerated water belongs at disturbances, not along every quiet stretch.
    for (name, start, end, speed) in [
        (
            "plunge",
            [-41.9, level + 0.006, 23.85],
            [-40.5, level + 0.006, 22.5],
            0.9,
        ),
        (
            "mill",
            [-32.7, level + 0.006, 20.],
            [-31.2, level + 0.006, 20.],
            0.65,
        ),
    ] {
        let dx = end[0] - start[0];
        let dz = end[2] - start[2];
        let length = dx.hypot(dz);
        for trail in 0..3 {
            let mut foam = Geometry::default();
            for segment in 0..12 {
                if (segment + trail) % 4 == 0 {
                    continue;
                }
                let point = |j: usize, edge: f64| {
                    let across = (j as f64 / 12. - 0.5) * 0.9;
                    let along = across * across * 0.7 + edge * 0.014;
                    [
                        (-dz * across + dx * along) / length,
                        0.,
                        (dx * across + dz * along) / length,
                    ]
                };
                foam.face(&[
                    point(segment, -1.),
                    point(segment + 1, -1.),
                    point(segment + 1, 1.),
                    point(segment, 1.),
                ]);
            }
            if wet_flow(g, &foam, start, end, level) {
                animated.push(Animated {
                    draws: vec![draw(
                        &format!("brook-{name}-foam-{trail}"),
                        foam,
                        0xc6d3bd,
                        0.38,
                    )],
                    motion: Motion::Flow {
                        start,
                        end,
                        period: length / speed,
                        phase: trail as f64 / 3.,
                    },
                });
            }
        }
    }
    // The settlement glTF supplies the textured crossing; retain this complete bridge
    // until that art is ready. Water, banks and culverts remain independent.
    let previous_fallback = b.fallback;
    b.fallback = Some(Kit::Outpost);
    // Deck and approach wedges follow the real bridge bodies; rails leave the same clear lane.
    for i in 0..40 {
        b.global("brook-deck", if i % 3 == 0 { 0x806040 } else { 0x9a7650 })
            .add(
                &box_shape(),
                at(0., 0.065, 30. + i as f64 * 0.1, 1.5, 0.055, 0.046),
            );
    }
    for (z, turn) in [(29., 0.), (35., std::f64::consts::PI)] {
        let shape = crate::geometry::wedge();
        b.global("brook-approach", 0x806040).add(
            &shape,
            crate::geometry::compose(&[
                crate::geometry::translate(0., 0., z),
                crate::geometry::ry(turn),
                crate::geometry::scale(1.5, 0.12, 1.),
            ]),
        );
    }
    for x in [-1.8, 1.8] {
        for z in [30., 31., 32., 33., 34.] {
            b.global("brook-posts", 0x6e4d30)
                .add(&box_shape(), at(x, 0.49, z, 0.08, 0.71, 0.08));
        }
        for y in [0.5, 1.05] {
            b.global("brook-rails", 0x896644)
                .add(&box_shape(), at(x, y, 32., 0.07, 0.06, 2.3));
        }
    }
    for z in [30., 31., 32., 33., 34.] {
        b.global("brook-crossbearers", 0x6e4d30)
            .add(&box_shape(), at(0., -0.12, z, 1.95, 0.10, 0.12));
    }
    b.fallback = previous_fallback;
    for c in content::colliders()
        .iter()
        .filter(|c| c.id >= content::CULVERT_POST)
    {
        let p = c.position.map(metres);
        let h = c.half_extents.map(metres);
        b.global("brook-drain-stone", 0x77796c)
            .add(&box_shape(), at(p[0], p[1], p[2], h[0], h[1], h[2]));
    }
    // Low, partly buried talus gathers at the western cliff foot. Decorative chips
    // remain below an ankle; supported ledges and walking collision stay authoritative.
    for row in 0..22 {
        for column in 0..8 {
            let seed = (row * 8 + column) as f64;
            let x = -49. + column as f64 * 1.8 + noise(seed, 0., 0.7, 51401.) * 1.1;
            let z = 16. + row as f64 * 2. + noise(seed, 1., 0.7, 51402.) * 1.2;
            if noise(x, z, 4., 51403.) < 0.51
                || s.road_distance([(x * 100.) as i32, (z * 100.) as i32]) < 250
            {
                continue;
            }
            let y = surface(g, x, z);
            if [[0.4, 0.], [-0.4, 0.], [0., 0.4], [0., -0.4]]
                .iter()
                .any(|[dx, dz]| (surface(g, x + dx, z + dz) - y).abs() > 0.12)
            {
                continue;
            }
            let r = 0.12 + 0.26 * noise(seed, 2., 0.7, 51404.);
            let mut chip = blob(0, true, 0.2, seed + 51400.);
            let color = rgb(mix(0x706c60, 0x99917e, noise(x, z, 1., 51405.))).map(f64::from);
            chip.colors = vec![color; chip.p.len()];
            shingle.add(&chip, at(x, y - 0.025, z, r, 0.045 + r * 0.08, r * 0.7));
        }
    }
    // Sparse bank reeds, safely outside the water lane.
    for (x, z) in [
        (-41., 15.),
        (-29., 25.),
        (-18., 32.8),
        (5., 40.),
        (-6.5, 61.),
        (-9.5, 86.),
    ] {
        reed_clump(b, g, x, z);
        for j in 0..3 {
            let y = surface(g, x + j as f64 * 0.12, z);
            b.global("brook-reeds", 0x84965c).add(
                &frustum(5, 0.25, true, true, 0.),
                at(
                    x + j as f64 * 0.12,
                    y,
                    z,
                    0.018,
                    0.45 + j as f64 * 0.12,
                    0.018,
                ),
            );
            b.global("brook-reed-seed-heads", 0x695235).add(
                &frustum(6, 0.65, true, true, 0.),
                at(
                    x + j as f64 * 0.12,
                    y + 0.45 + j as f64 * 0.12,
                    z,
                    0.025,
                    0.15,
                    0.025,
                ),
            );
        }
    }
    banks.colors = banks
        .p
        .iter()
        .map(|[x, _, z]| rgb(mix(0x635d40, 0x8c805d, noise(*x, *z, 5., 573.))).map(f64::from))
        .collect();
    ground::project_top(&mut banks, Layer::Mud);
    let mut bank = draw("brook-wet-banks", banks, 0xffffff, 1.);
    bank.material.ground = Some(Ground::near(Layer::Mud, true));
    bank.material.opacity = ground::OVERLAY_ALPHA;
    ground::project_rock(&mut shingle);
    let mut stones = draw("brook-bank-shingle", shingle, 0xffffff, 1.);
    stones.material.ground = Some(Ground::near(Layer::Rock, false));
    vec![Group {
        draws: vec![bank, stones],
        cull: Cull::Always,
        instanced: false,
        fallback: None,
        min: [-73., -136.],
        max: [117., 99.],
    }]
}

/// A short straight current animation must not sweep foam or glints over dry banks.
fn wet_flow(g: &TerrainGrid, shape: &Geometry, start: [f64; 3], end: [f64; 3], level: f64) -> bool {
    (0..=16).all(|step| {
        let t = f64::from(step) / 16.;
        shape.p.iter().all(|p| {
            let x = start[0] + (end[0] - start[0]) * t + p[0];
            let z = start[2] + (end[2] - start[2]) * t + p[2];
            surface(g, x, z) < level - 0.02
        })
    })
}

/// Folded, arching reed leaves rooted on the same ground as their stems.
fn reed_clump(b: &mut Batcher, g: &TerrainGrid, x: f64, z: f64) {
    use std::f64::consts::{PI, TAU};
    let y = surface(g, x, z) - 0.025;
    for leaf in 0..9 {
        let angle = leaf as f64 * TAU / 9. + noise(x, z, 2., 817.) * TAU;
        let (dx, dz) = (angle.sin(), angle.cos());
        let height = 0.45 + 0.4 * noise(leaf as f64, x, 0.7, 818.);
        let point = |step: usize, side: f64| {
            let t = step as f64 / 5.;
            let spread = (0.28 + height * 0.3) * t * t;
            let width = 0.045 * (PI * t).sin().max(0.).sqrt() * side;
            let fold = 0.018 * (PI * t).sin() * (1. - side.abs());
            [
                x + dx * spread - dz * width,
                y + height * (t * PI * 0.78).sin() + fold,
                z + dz * spread + dx * width,
            ]
        };
        let shape = b.global(
            "brook-reed-leaves",
            if leaf % 3 == 0 { 0x7f9050 } else { 0x596e39 },
        );
        for step in 0..5 {
            for side in [-1., 1.] {
                let q = [
                    point(step, side),
                    point(step + 1, side),
                    point(step + 1, 0.),
                    point(step, 0.),
                ];
                shape.face(&q);
                shape.face(&[q[3], q[2], q[1], q[0]]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn currents_stay_in_water_and_foam_is_local_to_disturbances() {
        let scenery = mmorpg_scenery::greyhaven_vale_scenery();
        let grid = scenery.terrain_grid(crate::TERRAIN_STEP_UNITS);
        let mut animated = vec![];
        build(
            &mut Batcher::default(),
            &scenery,
            &grid,
            &mut vec![],
            &mut animated,
        );
        for disturbance in ["brook-plunge-foam", "brook-mill-foam"] {
            assert!(
                animated.iter().any(|part| part
                    .draws
                    .iter()
                    .any(|draw| draw.id.starts_with(disturbance))),
                "missing {disturbance}"
            );
        }
        for part in &animated {
            if !part
                .draws
                .iter()
                .any(|draw| draw.id.contains("-current-") || draw.id.contains("-foam-"))
            {
                continue;
            }
            let Motion::Flow { period, .. } = part.motion else {
                panic!("current must flow")
            };
            for step in 0..20 {
                let transform = crate::motion(part.motion, f64::from(step) * period / 20., true);
                for draw in &part.draws {
                    assert!(
                        draw.geometry.n.iter().all(|n| n[1] > 0.99),
                        "{} faces away from the water surface",
                        draw.id
                    );
                    for p in &draw.geometry.p {
                        let p = crate::geometry::point(transform, *p);
                        assert!(
                            p[1] - surface(&grid, p[0], p[2]) > 0.015,
                            "{} crossed a dry bank at {p:?}",
                            draw.id
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn brook_water_clears_the_actual_terrain_triangles() {
        let scenery = mmorpg_scenery::greyhaven_vale_scenery();
        let grid = scenery.terrain_grid(crate::TERRAIN_STEP_UNITS);
        for (points, water_surface, width) in [
            (content::village(), content::SURFACE, 0.85),
            (content::inflow(), content::UPPER_SURFACE, 1.25),
            (content::outlet(), content::UPPER_SURFACE, 1.25),
        ] {
            let water = metres(water_surface);
            for pair in points.windows(2) {
                let (a, b) = (pair[0].map(metres), pair[1].map(metres));
                let length = (b[0] - a[0]).hypot(b[1] - a[1]);
                let side = [-(b[1] - a[1]) / length, (b[0] - a[0]) / length];
                for step in 0..=20 {
                    let t = f64::from(step) / 20.;
                    for offset in [-width, 0., width] {
                        let x = a[0] + (b[0] - a[0]) * t + side[0] * offset;
                        let z = a[1] + (b[1] - a[1]) * t + side[1] * offset;
                        let bed = surface(&grid, x, z);
                        assert!(
                            bed < water - 0.02,
                            "water intersects terrain at {x:.2},{z:.2}: bed {bed:.3}, water {water}"
                        );
                    }
                }
            }
        }
    }
}
