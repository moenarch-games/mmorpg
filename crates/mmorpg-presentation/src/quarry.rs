//! The Redbrand quarry's old workings at blockout level (#1195, owner-approved mock-up "Redbrand
//! quarry mock-up (1195 3b)"): cart rails from the old mine's adit west across the floor and out
//! through the gateway, with a branch to a spoil heap in the north-east corner and a mine cart
//! on the line; a hand winch over an old shaft on the east rim above the adit; and a wooden
//! derrick crane by the south wall reaching over stacked cut blocks. Presentation only: nothing
//! here collides. The adit's timbering and its first rails are the mine entrance's prop.
use mmorpg_scenery::Scenery;

use crate::batching::Batcher;
use crate::geometry::{V, align_y, blob, box_shape, compose, scale, translate};

/// One scene-wide batch per colour, like the cave's torches: a handful of small pieces.
const WORKINGS: &str = "quarry-workings";
const TIMBER: u32 = 0x5c3f27;
const OLD_TIMBER: u32 = 0x4a3524;
const IRON: u32 = 0x3b3c40;
const SPOIL: u32 = 0x6e675c;
const ROPE: u32 = 0xb59a62;
const VOID: u32 = 0x0c0c0c;

/// The main line from the adit west across the floor and out through the gateway, and the
/// branch to the spoil heap (metres, x and z).
const MAIN_LINE: [[f64; 2]; 5] = [
    [13.5, -113.0],
    [12.0, -113.0],
    [4.0, -112.0],
    [-8.0, -111.5],
    [-19.0, -112.5],
];
const OUT_LINE: [[f64; 2]; 2] = [[-19.0, -112.5], [-30.0, -113.5]];
const TIP_BRANCH: [[f64; 2]; 3] = [[12.0, -113.0], [13.0, -120.0], [11.5, -126.0]];
const GAUGE: f64 = 0.9;
const CART: [f64; 2] = [14.5, -113.0];
const SPOIL_HEAP: [f64; 2] = [10.5, -128.5];
const WINDLASS: [f64; 2] = [26.0, -116.0];
const DERRICK: [f64; 2] = [9.5, -94.5];
/// Stacked cut blocks on the floor: (x, z, base above the ground, yaw in radians), each 1.3 m
/// long and 0.9 m wide and high.
const BLOCKS: [(f64, f64, f64, f64); 7] = [
    (6.0, -97.6, 0., 0.1),
    (7.4, -97.3, 0., -0.05),
    (6.7, -97.5, 0.9, 0.25),
    (-4.0, -130.4, 0., 0.3),
    (-2.6, -130.0, 0., 0.09),
    (-12.0, -96.4, 0., 0.),
    (3.6, -94.8, 0., 2.9),
];
const CUT_STONE: u32 = 0xb8b2a2;
const DERRICK_REACH: [f64; 2] = [4.0, -98.5];

/// The drawn ground at `(x, z)` metres.
fn floor(scenery: &Scenery, x: f64, z: f64) -> f64 {
    f64::from(scenery.surface_at((x * 100.).round() as i32, (z * 100.).round() as i32)) / 100.
}

/// The highest upward face of the authored rock over `(x, z)` metres, if any: there, the
/// quarry's rim.
fn rock_top(scenery: &Scenery, x: f64, z: f64) -> Option<f64> {
    scenery
        .collision_surfaces
        .iter()
        .flat_map(|mesh| {
            let point = |i: u32| {
                let v = mesh.vertices[i as usize];
                std::array::from_fn::<f64, 3, _>(|a| f64::from(v[a] + mesh.position[a]) / 100.)
            };
            mesh.triangles.iter().filter_map(move |&[a, b, c]| {
                let [a, b, c] = [point(a), point(b), point(c)];
                let d = (b[0] - a[0]) * (c[2] - a[2]) - (b[2] - a[2]) * (c[0] - a[0]);
                if d.abs() < 1e-9 {
                    return None;
                }
                let u = ((x - a[0]) * (c[2] - a[2]) - (z - a[2]) * (c[0] - a[0])) / d;
                let v = ((b[0] - a[0]) * (z - a[2]) - (b[2] - a[2]) * (x - a[0])) / d;
                (u >= 0. && v >= 0. && u + v <= 1.)
                    .then(|| a[1] + u * (b[1] - a[1]) + v * (c[1] - a[1]))
            })
        })
        .reduce(f64::max)
}

/// A square beam `r` thick from `a` to `b`.
fn beam(b: &mut Batcher, color: u32, from: V, to: V, r: f64) {
    let d = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    let length = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    let mid = [0, 1, 2].map(|i| (from[i] + to[i]) / 2.);
    b.global(WORKINGS, color).add(
        &box_shape(),
        compose(&[
            translate(mid[0], mid[1], mid[2]),
            align_y(d),
            scale(r, length / 2., r),
        ]),
    );
}

/// A box with half extents `half` round `centre`, turned `yaw` radians about +Y.
fn block(b: &mut Batcher, color: u32, centre: V, half: V, yaw: f64) {
    b.global(WORKINGS, color).add(
        &box_shape(),
        compose(&[
            translate(centre[0], centre[1], centre[2]),
            crate::geometry::ry(yaw),
            scale(half[0], half[1], half[2]),
        ]),
    );
}

/// Two rails and their sleepers along `line`, on the ground.
fn rails(b: &mut Batcher, scenery: &Scenery, line: &[[f64; 2]]) {
    for pair in line.windows(2) {
        let ([ax, az], [bx, bz]) = (pair[0], pair[1]);
        let length = (bx - ax).hypot(bz - az);
        let (nx, nz) = (-(bz - az) / length, (bx - ax) / length);
        for side in [-0.5, 0.5] {
            let (ox, oz) = (nx * GAUGE * side, nz * GAUGE * side);
            beam(
                b,
                IRON,
                [ax + ox, floor(scenery, ax, az) + 0.12, az + oz],
                [bx + ox, floor(scenery, bx, bz) + 0.12, bz + oz],
                0.05,
            );
        }
        let sleepers = (length / 0.9).floor() as usize;
        for k in 0..=sleepers {
            let t = k as f64 / sleepers.max(1) as f64;
            let (x, z) = (ax + (bx - ax) * t, az + (bz - az) * t);
            let y = floor(scenery, x, z) + 0.03;
            beam(
                b,
                OLD_TIMBER,
                [x - nx * 0.75, y, z - nz * 0.75],
                [x + nx * 0.75, y, z + nz * 0.75],
                0.08,
            );
        }
    }
}

pub(crate) fn build(b: &mut Batcher, scenery: &Scenery) {
    if !scenery.greyhaven() {
        return;
    }
    rails(b, scenery, &MAIN_LINE);
    rails(b, scenery, &OUT_LINE);
    rails(b, scenery, &TIP_BRANCH);

    // A mine cart on the line near the adit: a timber box on four iron wheels.
    let [cx, cz] = CART;
    let y = floor(scenery, cx, cz);
    block(b, TIMBER, [cx, y + 0.9, cz], [0.8, 0.45, 0.55], 0.);
    block(b, IRON, [cx, y + 1.36, cz], [0.82, 0.02, 0.57], 0.);
    for (dx, dz) in [(-0.55, -0.5), (-0.55, 0.5), (0.55, -0.5), (0.55, 0.5)] {
        block(b, IRON, [cx + dx, y + 0.34, cz + dz], [0.26, 0.26, 0.05], 0.);
    }

    // The spoil heap the carts tipped in the north-east corner: a long, flat-topped mound.
    let [sx, sz] = SPOIL_HEAP;
    let y = floor(scenery, sx, sz);
    b.global(WORKINGS, SPOIL).add(
        &blob(1, false, 0.25, 11.),
        compose(&[
            translate(sx, y - 0.4, sz),
            crate::geometry::ry(0.6),
            scale(4.8, 2.4, 3.2),
        ]),
    );

    // A hand winch over an old shaft on the east rim above the adit.
    let [wx, wz] = WINDLASS;
    if let Some(top) = rock_top(scenery, wx, wz) {
        block(b, VOID, [wx, top + 0.05, wz], [0.9, 0.12, 0.9], 0.);
        for (k, dz) in [-1.2, 1.2].into_iter().enumerate() {
            beam(
                b,
                OLD_TIMBER,
                [wx, top, wz + dz],
                [wx, top + 1.7, wz + dz],
                0.1,
            );
            if k == 0 {
                beam(
                    b,
                    TIMBER,
                    [wx, top + 1.5, wz - 1.3],
                    [wx, top + 1.5, wz + 1.3],
                    0.18,
                );
            }
        }
        beam(
            b,
            ROPE,
            [wx, top + 1.5, wz + 0.2],
            [wx, top - 0.5, wz + 0.2],
            0.025,
        );
    }

    // A wooden derrick crane by the south wall, its boom over the cut blocks.
    let [dx, dz] = DERRICK;
    let [rx, rz] = DERRICK_REACH;
    let y = floor(scenery, dx, dz);
    let mast = [dx, y + 8.5, dz];
    let tip = [rx, y + 7.5, rz];
    beam(b, TIMBER, [dx, y, dz], mast, 0.18);
    beam(b, TIMBER, [dx, y + 1., dz], tip, 0.13);
    beam(b, ROPE, mast, tip, 0.03);
    beam(b, ROPE, mast, [dx + 2., y + 8.9, dz + 4.], 0.03);
    beam(b, ROPE, tip, [rx, y + 1.6, rz], 0.03);
    block(b, CUT_STONE, [rx, y + 1.1, rz], [0.5, 0.4, 0.35], 0.3);
    for (x, z, base, yaw) in BLOCKS {
        let y = floor(scenery, x, z) + base;
        block(b, CUT_STONE, [x, y + 0.45, z], [0.65, 0.45, 0.45], yaw);
    }
    for (fx, fz) in [(-1.4, 0.), (1.4, 0.), (0., 1.4)] {
        beam(
            b,
            OLD_TIMBER,
            [dx, y + 0.1, dz],
            [dx + fx, y + 0.1, dz + fz],
            0.12,
        );
    }
}
