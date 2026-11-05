//! Greyhaven's waterfall, brook banks, timber crossing and moving foam (#513).
//! Authored course, water level and traversable structures come from core; this is only art.
use crate::{
    Animated, Draw, Group, Motion,
    batching::{Batcher, Cull, material, matrix, rgb},
    geometry::{Geometry, ID, at, box_shape, disc, frustum},
    ground::{self, Ground, Layer},
    terrain::{metres, surface},
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
    let mut channel = Geometry::default();
    let mut banks = Geometry::default();
    let points = content::COURSE;
    let level = metres(content::SURFACE) + 0.012;
    for i in 0..points.len() - 1 {
        let a = points[i].map(metres);
        let z = points[i + 1].map(metres);
        let left = offset(points, i, metres(content::HALF_WIDTH));
        let right = offset(points, i + 1, metres(content::HALF_WIDTH));
        channel.face(&[
            [a[0] + left[0], level, a[1] + left[1]],
            [z[0] + right[0], level, z[1] + right[1]],
            [z[0] - right[0], level, z[1] - right[1]],
            [a[0] - left[0], level, a[1] - left[1]],
        ]);
        let length = (z[0] - a[0]).hypot(z[1] - a[1]);
        let steps = (length / 2.).ceil() as usize;
        for step in 0..steps {
            let t0 = step as f64 / steps as f64;
            let t1 = (step + 1) as f64 / steps as f64;
            let p = |t: f64, side: f64, width: f64| {
                let x = a[0] + (z[0] - a[0]) * t;
                let v = a[1] + (z[1] - a[1]) * t;
                let nx = (left[0] + (right[0] - left[0]) * t) / 0.9;
                let nz = (left[1] + (right[1] - left[1]) * t) / 0.9;
                let x = x + nx * side * width;
                let v = v + nz * side * width;
                [x, surface(g, x, v) + 0.008, v]
            };
            for side in [-1., 1.] {
                let q = [
                    p(t0, side, 0.9),
                    p(t1, side, 0.9),
                    p(t1, side, 2.6),
                    p(t0, side, 2.6),
                ];
                if side > 0. {
                    banks.face(&[q[3], q[2], q[1], q[0]]);
                } else {
                    banks.face(&q);
                }
            }
        }
        // Short, broken ripple bands travel with the course; reduced motion holds them in place.
        let mut foam = Geometry::default();
        foam.add(&disc(10), at(0., 0., 0., 0.35, 1., 0.07));
        animated.push(Animated {
            draws: vec![draw(&format!("brook-current-{i}"), foam, 0xa7d0c8, 0.65)],
            motion: Motion::Flow {
                start: [a[0], level + 0.006, a[1]],
                end: [z[0], level + 0.006, z[1]],
                period: (length / 0.7).max(1.),
                phase: i as f64 * 0.17,
            },
        });
    }
    water.push(draw("greyhaven-brook", channel, 0x397b7a, 0.78));
    let mut fall = Geometry::default();
    let cascade = [
        [-48., 11.1],
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
                start: [-47.95, 11.12, 23.55 + i as f64 * 0.15],
                end: [-42.4, 0.02, 23.55 + i as f64 * 0.15],
                period: 1.5,
                phase: i as f64 / 7.,
            },
        });
    }
    let mut foam = Geometry::default();
    foam.add(&disc(18), at(-42.4, level + 0.008, 24., 1.2, 1., 0.85));
    water.push(draw("waterfall-foam", foam, 0xd1e8d8, 0.6));
    let mut pool = Geometry::default();
    pool.add(&disc(24), at(-50., 11.1, 24., 2., 1., 1.5));
    water.push(draw("cliffside-spring", pool, 0x4c999b, 0.8));
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
                .add(&box_shape(), at(x, 0.65, z, 0.08, 0.55, 0.08));
        }
        for y in [0.5, 1.05] {
            b.global("brook-rails", 0x896644)
                .add(&box_shape(), at(x, y, 32., 0.07, 0.06, 2.3));
        }
    }
    for c in content::colliders()
        .iter()
        .filter(|c| c.id >= content::CULVERT_POST)
    {
        let p = c.position.map(metres);
        let h = c.half_extents.map(metres);
        b.global("brook-drain-stone", 0x77796c)
            .add(&box_shape(), at(p[0], p[1], p[2], h[0], h[1], h[2]));
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
        for j in 0..3 {
            let y = surface(g, x, z);
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
        }
    }
    banks.colors = vec![rgb(0x75664c).map(f64::from); banks.p.len()];
    ground::project_top(&mut banks, Layer::Mud);
    let mut bank = draw("brook-wet-banks", banks, 0xffffff, 1.);
    bank.material.ground = Some(Ground::near(Layer::Mud, false));
    vec![Group {
        draws: vec![bank],
        cull: Cull::Always,
        instanced: false,
        fallback: None,
        min: [-50., 10.],
        max: [8., 99.],
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brook_water_clears_the_actual_terrain_triangles() {
        let scenery = mmorpg_scenery::greyhaven_vale_scenery();
        let grid = scenery.terrain_grid(crate::TERRAIN_STEP_UNITS);
        let water = metres(content::SURFACE);
        for pair in content::COURSE.windows(2) {
            let (a, b) = (pair[0].map(metres), pair[1].map(metres));
            let length = (b[0] - a[0]).hypot(b[1] - a[1]);
            let side = [-(b[1] - a[1]) / length, (b[0] - a[0]) / length];
            for step in 0..=20 {
                let t = f64::from(step) / 20.;
                for offset in [-0.85, 0., 0.85] {
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
