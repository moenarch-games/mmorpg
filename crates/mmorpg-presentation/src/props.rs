//! Greyhaven prop compositions: one low-poly model per scenery prop kind.
use super::{Animated, Draw, Kit, Motion, batching::*, geometry::*, terrain::*};
use mmorpg_scenery::{Prop, PropKind, Scenery, TerrainGrid, TreeVariant};
use std::{
    collections::BTreeMap,
    f64::consts::{PI, TAU},
    sync::Arc,
};
const PLASTER: u32 = 0xe3d6b8;
const TIMBER: u32 = 0x5c3f28;
const THATCH: u32 = 0xc19a4f;
const RED: u32 = 0xa3472f;
const SLATE: u32 = 0x55606f;
const ROOF_WOOD: u32 = 0x6d4a2d;
const STONE: u32 = 0xaaa396;
const STONE_DARK: u32 = 0x7c766c;
const STONE_LIGHT: u32 = 0xc7c0b0;
const WOOD: u32 = 0x8d6641;
const DARK: u32 = 0x5f4430;
const LIGHT: u32 = 0xb08a5b;
const IRON: u32 = 0x55585e;
const BARN: u32 = 0x9b3b2c;
const TRIM: u32 = 0xefe6d2;
const WINDOW: u32 = 0x2e3a4a;
const WINDOW_LIT: u32 = 0xf2c46a;
const DOOR: u32 = 0x4a3222;
const CANVAS: u32 = 0xd9cbad;
const BANNER: u32 = 0x2f5f8f;
const RUNE: u32 = 0x7fe6ff;
const GLOW: u32 = 0xffd98a;
const FIRE: u32 = 0xff8a2a;
const CORE: u32 = 0xffe27a;
const EMBER: u32 = 0x3b2a22;
const ROCK: u32 = 0x9a948a;
const ROCK_DARK: u32 = 0x77726a;
const MOSS: u32 = 0x62804a;
const CROP: u32 = 0xd8bd5a;
const STEM: u32 = 0x93a74b;
struct Part {
    c: u32,
    g: Geometry,
    a: Affine,
    cull: Cull,
}
struct Model {
    parts: Vec<Part>,
    cull: Cull,
}
impl Model {
    fn new(cull: Cull) -> Self {
        Self {
            parts: Vec::new(),
            cull,
        }
    }
    fn put(&mut self, c: u32, g: Geometry, a: Affine) {
        self.parts.push(Part {
            c,
            g,
            a,
            cull: self.cull,
        });
    }
    fn box_at(&mut self, c: u32, p: V, h: V) {
        self.rot_box(c, p, h, ID);
    }
    fn rot_box(&mut self, c: u32, p: V, h: V, r: Affine) {
        self.put(
            c,
            box_shape(),
            compose(&[translate(p[0], p[1], p[2]), r, scale(h[0], h[1], h[2])]),
        );
    }
    fn beam(&mut self, c: u32, a: V, b: V, r: f64, sides: usize) {
        let d = std::array::from_fn(|i| b[i] - a[i]);
        let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        self.put(
            c,
            frustum(sides, 1., true, true, 0.),
            compose(&[translate(a[0], a[1], a[2]), align_y(d), scale(r, l, r)]),
        );
    }
    fn cylinder(&mut self, c: u32, sides: usize, top: f64, a: Affine) {
        self.put(c, frustum(sides, top, true, true, 0.), a);
    }
}
fn half(p: &Prop) -> V {
    p.half_extents.map(metres)
}
fn seed(p: &Prop) -> f64 {
    hash(&[
        f64::from(p.position[0]),
        f64::from(p.position[2]),
        f64::from(p.yaw),
    ])
}
fn yaw(p: &Prop) -> f64 {
    f64::from(p.yaw) / 65536. * TAU
}
fn frame(p: &Prop, g: &TerrainGrid, extra: f64) -> Affine {
    let x = metres(p.position[0]);
    let z = metres(p.position[2]);
    compose(&[translate(x, surface(g, x, z), z), ry(yaw(p) + extra)])
}
fn target(s: &Scenery, g: &TerrainGrid, p: &Prop) -> [f64; 2] {
    let mut plaza = [0.; 2];
    let mut count = 0.;
    for (i, b) in g.biomes.iter().enumerate() {
        if b.name() == "plaza" {
            plaza[0] += metres(g.origin[0] + (i % g.columns) as i32 * g.step);
            plaza[1] += metres(g.origin[1] + (i / g.columns) as i32 * g.step);
            count += 1.;
        }
    }
    if count > 0. {
        plaza = plaza.map(|v| v / count);
    }
    if matches!(
        p.kind,
        PropKind::Keep
            | PropKind::Inn
            | PropKind::Smithy
            | PropKind::House
            | PropKind::Well
            | PropKind::Waystone
    ) {
        return plaza;
    }
    let (x, z) = (metres(p.position[0]), metres(p.position[2]));
    let mut best = plaza;
    let mut distance = f64::INFINITY;
    for road in &s.roads {
        for pair in road.points.windows(2) {
            let [ax, az] = pair[0].map(metres);
            let [bx, bz] = pair[1].map(metres);
            let (dx, dz) = (bx - ax, bz - az);
            let l = dx * dx + dz * dz;
            let t = if l == 0. {
                0.
            } else {
                (((x - ax) * dx + (z - az) * dz) / l).clamp(0., 1.)
            };
            let p = [ax + dx * t, az + dz * t];
            let d = (p[0] - x).hypot(p[1] - z);
            if d < distance {
                best = p;
                distance = d;
            }
        }
    }
    best
}
fn building(s: &Scenery, g: &TerrainGrid, p: &Prop) -> (Affine, f64, f64, f64, f64) {
    let [hx, hy, hz] = half(p);
    let [t_x, t_z] = target(s, g, p);
    let (dx, dz) = (t_x - metres(p.position[0]), t_z - metres(p.position[2]));
    let q = if dx.abs() > dz.abs() {
        if dx > 0. { 1 } else { 3 }
    } else if dz >= 0. {
        0
    } else {
        2
    };
    let extra = f64::from(q) * PI / 2.;
    let (w, d) = if q % 2 == 1 { (hz, hx) } else { (hx, hz) };
    (frame(p, g, extra), w, d, hy * 2., yaw(p) + extra)
}
fn roof(m: &mut Model, c: u32, w: f64, d: f64, base: f64, rise: f64, over: f64) {
    let (rw, rd, r) = if w >= d {
        (w, d, ID)
    } else {
        (d, w, ry(PI / 2.))
    };
    m.put(
        c,
        gable(),
        compose(&[
            translate(0., base, 0.),
            r,
            scale(rw + over, rise, rd + over),
        ]),
    );
    m.put(
        shade(c, 0.72),
        box_shape(),
        compose(&[
            translate(0., base + rise, 0.),
            r,
            scale(rw + over + 0.05, 0.09, 0.12),
        ]),
    );
}
fn windows(
    m: &mut Model,
    c: u32,
    span: f64,
    face: f64,
    y: f64,
    spacing: f64,
    skip: bool,
    rotation: Affine,
) {
    let count = (span * 2. / spacing).floor().max(1.) as usize;
    for i in 0..count {
        let offset =
            -span + spacing / 2. + i as f64 * ((span * 2. - spacing) / (count as f64 - 1.).max(1.));
        let x = if count == 1 { 0. } else { offset };
        if skip && x.abs() < 1.2 {
            continue;
        }
        let z = face + face.signum() * 0.04;
        for (c, p, h) in [
            (TIMBER, [x, y, z], [0.46, 0.52, 0.05]),
            (c, [x, y, z + face.signum() * 0.02], [0.34, 0.4, 0.05]),
        ] {
            m.put(
                c,
                box_shape(),
                compose(&[rotation, at(p[0], p[1], p[2], h[0], h[1], h[2])]),
            );
        }
    }
}
fn door(m: &mut Model, d: f64, w: f64, h: f64) {
    m.box_at(
        TIMBER,
        [0., h / 2. + 0.05, d + 0.05],
        [w / 2. + 0.14, h / 2. + 0.1, 0.06],
    );
    m.box_at(DOOR, [0., h / 2., d + 0.09], [w / 2., h / 2., 0.06]);
}
fn chimney(m: &mut Model, c: u32, x: f64, z: f64, base: f64, top: f64) {
    m.box_at(
        c,
        [x, (base + top) / 2., z],
        [0.42, (top - base) / 2., 0.42],
    );
    m.box_at(shade(c, 0.75), [x, top + 0.08, z], [0.5, 0.1, 0.5]);
}
fn timber(m: &mut Model, w: f64, d: f64, from: f64, to: f64) {
    for sx in [-1., 1.] {
        for sz in [-1., 1.] {
            m.box_at(
                TIMBER,
                [sx * (w - 0.08), (from + to) / 2., sz * (d - 0.08)],
                [0.16, (to - from) / 2., 0.16],
            );
        }
    }
    for y in [from + 0.1, to - 0.12] {
        m.box_at(TIMBER, [0., y, d + 0.02], [w, 0.1, 0.04]);
        m.box_at(TIMBER, [0., y, -d - 0.02], [w, 0.1, 0.04]);
        m.box_at(TIMBER, [w + 0.02, y, 0.], [0.04, 0.1, d]);
        m.box_at(TIMBER, [-w - 0.02, y, 0.], [0.04, 0.1, d]);
    }
}
fn house(m: &mut Model, w: f64, d: f64, h: f64, c: u32, lit: bool) {
    let skirt = 0.9f64.min(h * 0.18);
    m.box_at(STONE, [0., skirt / 2., 0.], [w, skirt / 2., d]);
    m.box_at(
        PLASTER,
        [0., (skirt + h) / 2., 0.],
        [w, (h - skirt) / 2., d],
    );
    timber(m, w, d, skirt, h);
    let rise = w.min(d) * 0.95;
    roof(m, c, w, d, h, rise, 0.55);
    chimney(m, STONE_DARK, w * 0.55, -d * 0.35, h * 0.6, h + rise * 0.85);
    door(m, d, 1.2, 2.2);
    let glass = if lit { WINDOW_LIT } else { WINDOW };
    windows(m, glass, w, d, h * 0.58, 2.6, true, ID);
    windows(m, glass, w, -d, h * 0.58, 2.6, false, ID);
    for side in [1., -1.] {
        windows(m, glass, d, w, h * 0.58, 3.2, false, ry(side * PI / 2.));
    }
}
fn keep(m: &mut Model, w: f64, d: f64, h: f64) {
    m.box_at(STONE, [0., h / 2., 0.], [w, h / 2., d]);
    m.box_at(STONE_DARK, [0., 0.35, 0.], [w + 0.08, 0.35, d + 0.08]);
    m.box_at(STONE_DARK, [0., h * 0.62, 0.], [w + 0.12, 0.14, d + 0.12]);
    m.box_at(STONE_DARK, [0., h + 0.25, 0.], [w + 0.3, 0.25, d + 0.3]);
    let mut x = -w + 0.6;
    while x <= w - 0.5 {
        for z in [d + 0.02, -d - 0.02] {
            m.box_at(STONE, [x, h + 0.95, z], [0.42, 0.45, 0.42]);
        }
        x += 1.6;
    }
    let mut z = -d + 0.6;
    while z <= d - 0.5 {
        for x in [w + 0.02, -w - 0.02] {
            m.box_at(STONE, [x, h + 0.95, z], [0.42, 0.45, 0.42]);
        }
        z += 1.6;
    }
    let r = 2.3;
    let top = h + 3.2;
    for sx in [-1., 1.] {
        for sz in [-1., 1.] {
            let (x, z) = (sx * (w - r), sz * (d - r));
            m.put(
                STONE_LIGHT,
                frustum(8, 1., false, true, 0.5),
                at(x, 0., z, r, top, r),
            );
            m.put(
                STONE_DARK,
                frustum(8, 1., false, true, 0.5),
                at(x, top, z, r + 0.3, 0.4, r + 0.3),
            );
            m.put(
                SLATE,
                frustum(8, 0., false, true, 0.5),
                at(x, top + 0.4, z, r + 0.35, 3.6, r + 0.35),
            );
            m.box_at(
                WINDOW,
                [x + sx * 0.3, top - 1.6, z + sz * (r - 0.1)],
                [0.16, 0.5, 0.2],
            );
        }
    }
    m.box_at(IRON, [w - r, top + 4.9, d - r], [0.05, 1.6, 0.05]);
    m.box_at(BANNER, [w - r + 0.75, top + 5.8, d - r], [0.7, 0.42, 0.03]);
    m.box_at(STONE_LIGHT, [0., 2.3, d + 0.08], [2.1, 2.3, 0.1]);
    m.box_at(DOOR, [0., 2., d + 0.14], [1.6, 2., 0.08]);
    m.box_at(IRON, [0., 2., d + 0.2], [1.6, 0.08, 0.04]);
    for sx in [-1., 1.] {
        m.box_at(BANNER, [sx * 3.6, 5.2, d + 0.06], [0.75, 1.8, 0.04]);
        m.box_at(GLOW, [sx * 3.6, 5.6, d + 0.1], [0.28, 0.28, 0.02]);
        m.put(
            BANNER,
            pyramid(),
            compose(&[
                translate(sx * 3.6, 3.4, d + 0.06),
                rx(PI),
                scale(0.75, 0.45, 0.04),
            ]),
        );
    }
    for x in [-w * 0.55, w * 0.55] {
        for z in [d + 0.05, -d - 0.05] {
            m.box_at(WINDOW, [x, h * 0.8, z], [0.14, 0.6, 0.05]);
        }
    }
}
fn barn(m: &mut Model, w: f64, d: f64, h: f64) {
    m.box_at(BARN, [0., h / 2., 0.], [w, h / 2., d]);
    for sx in [-1., 1.] {
        for sz in [-1., 1.] {
            m.box_at(
                TRIM,
                [sx * (w - 0.1), h / 2., sz * (d - 0.1)],
                [0.14, h / 2., 0.14],
            );
        }
    }
    roof(m, ROOF_WOOD, w, d, h, w.min(d) * 0.85, 0.6);
    let dh = 2.2f64.min(w * 0.4);
    m.box_at(TRIM, [0., 2.1, d + 0.04], [dh + 0.15, 2.2, 0.04]);
    m.box_at(BARN, [0., 2., d + 0.08], [dh, 2., 0.04]);
    for sign in [1., -1.] {
        m.rot_box(
            TRIM,
            [0., 2., d + 0.12],
            [dh.hypot(2.), 0.1, 0.03],
            rz(4f64.atan2(dh * 2.) * sign),
        );
    }
    m.box_at(TRIM, [0., h - 0.9, d + 0.05], [0.8, 0.7, 0.04]);
    m.box_at(WINDOW, [0., h - 0.9, d + 0.08], [0.6, 0.5, 0.04]);
    m.box_at(CROP, [w - 1.2, 0.45, d + 1.], [0.8, 0.45, 0.55]);
    m.box_at(CROP, [w - 1.9, 0.45, d + 1.1], [0.8, 0.45, 0.55]);
    m.box_at(
        shade(CROP, 0.9),
        [w - 1.55, 1.3, d + 1.05],
        [0.8, 0.4, 0.55],
    );
}
fn animated(m: Model, name: impl Fn(usize) -> String, motion: Motion) -> Animated {
    let mut by = BTreeMap::<u32, Geometry>::new();
    for p in m.parts {
        by.entry(p.c).or_default().add(&p.g, p.a);
    }
    Animated {
        draws: by
            .into_iter()
            .enumerate()
            .map(|(i, (c, g))| {
                let id = name(i);
                Draw {
                    resource_key: format!("scenery:{id}"),
                    id,
                    geometry: Arc::new(g),
                    model: matrix(ID),
                    material: material(c, false),
                }
            })
            .collect(),
        motion,
    }
}
fn tree(m: &mut Model, p: &Prop, v: TreeVariant) {
    let [hx, hy, _] = half(p);
    let background = p.collider.is_none();
    m.cull = if background { Cull::Always } else { Cull::Far };
    let s = seed(p);
    let grow = if background {
        f64::from(p.scale_permille) / 1000.
    } else {
        0.85 + s * 0.4
    };
    let radius = hx.max(0.18);
    let detail = usize::from(!background);
    let vs = (s * 4.).floor();
    let trunk = |m: &mut Model, c, r, h, sides| {
        m.put(
            c,
            frustum(sides, 0.62, true, false, 0.),
            at(0., -0.3, 0., r, h + 0.3, r),
        )
    };
    match v {
        TreeVariant::Oak => {
            let h = if background { hy * 1.1 } else { hy * 2. * 0.72 };
            trunk(m, 0x6a4a30, radius, h + 0.6, if background { 5 } else { 7 });
            m.put(
                0x4e7d32,
                blob(detail, true, 0.18, vs),
                at(0., h + 1.5 * grow, 0., 2.5 * grow, 2. * grow, 2.5 * grow),
            );
            if !background {
                m.put(
                    0x6f9c3c,
                    blob(1, true, 0.2, vs + 4.),
                    at(
                        1.2 * grow,
                        h + grow,
                        0.7 * grow,
                        1.6 * grow,
                        1.4 * grow,
                        1.6 * grow,
                    ),
                );
                m.put(
                    0x4e7d32,
                    blob(1, true, 0.2, vs + 8.),
                    at(
                        -grow,
                        h + 0.9 * grow,
                        -0.9 * grow,
                        1.5 * grow,
                        1.3 * grow,
                        1.5 * grow,
                    ),
                );
                m.put(
                    0x6f9c3c,
                    blob(0, true, 0.15, vs + 12.),
                    at(
                        -0.3 * grow,
                        h + 2.9 * grow,
                        0.2 * grow,
                        1.3 * grow,
                        grow,
                        1.3 * grow,
                    ),
                );
            }
        }
        TreeVariant::Pine => {
            let h = if background { hy * 0.5 } else { hy * 2. * 0.3 };
            trunk(m, 0x6a4a30, radius * 0.9, h + 1.2 * grow, 5);
            let tiers = if background { 3 } else { 4 };
            for tier in 0..tiers {
                let t = tier as f64;
                let shrink = 1. - t / (tiers as f64 + 0.6);
                m.put(
                    if tier % 2 == 0 { 0x2e5a36 } else { 0x3e6e3f },
                    frustum(if background { 6 } else { 8 }, 0., true, true, t * 0.37),
                    at(
                        0.,
                        h + t * 1.35 * grow,
                        0.,
                        2.2 * grow * shrink,
                        2.5 * grow,
                        2.2 * grow * shrink,
                    ),
                );
            }
        }
        TreeVariant::Birch => {
            let h = if background { hy * 1.2 } else { hy * 2. * 0.78 };
            trunk(m, 0xe7e2d6, radius * 0.75, h + 0.8, 6);
            if !background {
                for mark in 0..4 {
                    let mark = mark as f64;
                    m.rot_box(
                        EMBER,
                        [0., 0.6 + mark * h * 0.22, radius * 0.62],
                        [radius * 0.35, 0.05, 0.03],
                        ry(mark * 1.7),
                    );
                }
            }
            m.put(
                0x8fb54e,
                blob(detail, true, 0.2, vs + 16.),
                at(0., h + 1.6 * grow, 0., 1.6 * grow, 2.4 * grow, 1.6 * grow),
            );
            if !background {
                m.put(
                    0x8fb54e,
                    blob(0, true, 0.2, vs + 20.),
                    at(
                        0.8 * grow,
                        h + 0.6 * grow,
                        0.3 * grow,
                        1.1 * grow,
                        1.4 * grow,
                        1.1 * grow,
                    ),
                );
            }
        }
    }
}
fn grass(m: &mut Model, p: &Prop) {
    m.cull = Cull::Near;
    let [hx, hy, _] = half(p);
    let s = seed(p);
    let variant = (s * 4.).floor();
    let mut tuft = Geometry::default();
    for blade_i in 0..4 {
        let i = blade_i as f64;
        let a = i / 4. * TAU + variant * 1.5;
        let lean = 0.2 + hash(&[variant, i]) * 0.35;
        let h = 3.4 * (0.7 + hash(&[variant, i, 1.]) * 0.6);
        let spread = 0.3 + hash(&[variant, i, 2.]) * 0.5;
        tuft.add(
            &blade(),
            compose(&[
                translate(a.sin() * spread, 0., a.cos() * spread),
                ry(a),
                rx(-lean),
                scale(0.34, h, 0.34),
            ]),
        );
    }
    m.put(
        if s > 0.5 { 0x6f9f44 } else { 0x5b8a39 },
        tuft,
        compose(&[translate(0., -0.02, 0.), scale(hx, hy, hx)]),
    );
}
fn cliff(m: &mut Model, p: &Prop) {
    let [hx, hy, hz] = half(p);
    let h = hy * 2.;
    let s = seed(p);
    for layer in 0..3 {
        let l = layer as f64;
        let from = h * l / 3.;
        let to = h * (l + 1.) / 3.;
        m.box_at(
            if layer % 2 == 0 { ROCK } else { ROCK_DARK },
            [0., (from + to) / 2., 0.],
            [hx - l * 0.25, (to - from) / 2., hz - l * 0.25],
        );
    }
    m.box_at(MOSS, [0., h + 0.08, 0.], [hx - 0.7, 0.1, hz - 0.7]);
    let count = (hx * hz / 6.).round().clamp(4., 26.) as usize;
    for i in 0..count {
        let j = i as f64;
        let u = hash(&[s * 1e6, j, 1.]);
        let v = hash(&[s * 1e6, j, 2.]);
        let size = 0.9 + hash(&[s * 1e6, j, 3.]) * 1.6;
        let top = i % 3 == 0 || h < 3.;
        let side = (v * 4.).floor() as usize;
        let along = u * 2. - 1.;
        let inset = size * 0.75;
        let (x, z) = if top {
            (along * (hx - size), (v * 2. - 1.) * (hz - size))
        } else {
            match side {
                0 => (along * hx, hz - inset),
                1 => (along * hx, -hz + inset),
                2 => (hx - inset, along * hz),
                _ => (-hx + inset, along * hz),
            }
        };
        let y = if top {
            h + size * 0.2
        } else {
            2.2 + size + hash(&[s * 1e6, j, 4.]) * (h - 2.2 - size).max(0.)
        };
        m.put(
            if i % 2 == 0 { ROCK_DARK } else { ROCK },
            blob(0, false, 0.28, (i % 5) as f64),
            at(x, y, z, size, size * if top { 0.6 } else { 0.9 }, size),
        );
    }
}
fn campfire(m: &mut Model, p: &Prop, index: usize, f: Affine) -> Animated {
    m.cull = Cull::Mid;
    let [hx, _, hz] = half(p);
    let r = hx.min(hz) * 0.85;
    for i in 0..9 {
        let a = i as f64 / 9. * TAU;
        m.put(
            if i % 2 == 0 { STONE_DARK } else { STONE },
            blob(0, false, 0.25, i as f64),
            at(a.sin() * r, 0.05, a.cos() * r, 0.16, 0.12, 0.16),
        );
    }
    for i in 0..3 {
        let a = i as f64 / 3. * PI;
        let (dx, dz) = (a.sin() * r * 0.8, a.cos() * r * 0.8);
        m.beam(DARK, [-dx, 0.05, -dz], [dx, 0.18, dz], 0.07, 5);
    }
    m.put(EMBER, disc(8), at(0., 0.03, 0., r * 0.7, 1., r * 0.7));
    let mut flame = Geometry::default();
    flame.add(
        &frustum(6, 0., true, true, 0.),
        at(0., 0., 0., 0.32, 0.9, 0.32),
    );
    flame.add(
        &frustum(5, 0., true, true, 0.),
        at(0.12, 0., 0.05, 0.18, 0.62, 0.18),
    );
    let mut core = Geometry::default();
    core.add(
        &frustum(5, 0., true, true, 0.),
        at(-0.04, 0., -0.03, 0.18, 0.55, 0.18),
    );
    let draws = [
        (format!("campfire-{index}-flame"), FIRE, flame),
        (format!("campfire-{index}-core"), CORE, core),
    ]
    .into_iter()
    .map(|(id, c, g)| Draw {
        resource_key: format!("scenery:{id}"),
        id,
        geometry: Arc::new(g),
        model: matrix(ID),
        material: material(c, false),
    })
    .collect();
    Animated {
        draws,
        motion: Motion::Flame {
            base: [f.t[0], f.t[1] + 0.12, f.t[2]],
            seed: index as f64,
        },
    }
}
/// A cave's roof slab (#159): rock with a mossy cap, drawn as its own part so clients can
/// cut it away while the viewer stands under it.
fn cave_roof(p: &Prop, index: usize) -> Animated {
    let [hx, hy, hz] = half(p);
    let t = hy * 2.;
    let s = seed(p);
    let mut m = Model::new(Cull::Always);
    m.box_at(ROCK_DARK, [0., t * 0.3, 0.], [hx, t * 0.3, hz]);
    m.box_at(ROCK, [0., t * 0.8, 0.], [hx - 0.2, t * 0.2, hz - 0.2]);
    m.box_at(MOSS, [0., t + 0.06, 0.], [hx - 0.8, 0.08, hz - 0.8]);
    for i in 0..((hx * hz / 8.).round().clamp(4., 24.) as usize) {
        let j = i as f64;
        let size = 0.8 + hash(&[s * 1e6, j, 3.]) * 1.4;
        let x = (hash(&[s * 1e6, j, 1.]) * 2. - 1.) * (hx - size);
        let z = (hash(&[s * 1e6, j, 2.]) * 2. - 1.) * (hz - size);
        m.put(
            if i % 2 == 0 { ROCK } else { ROCK_DARK },
            blob(0, false, 0.3, s + j),
            at(x, t, z, size, size * 0.5, size * 0.9),
        );
    }
    let at = [
        metres(p.position[0]),
        metres(p.position[1]),
        metres(p.position[2]),
    ];
    animated(
        m,
        |part| format!("cave-roof-{index}-{part}"),
        Motion::Roof {
            at,
            min: [at[0] - hx, at[2] - hz],
            max: [at[0] + hx, at[2] + hz],
        },
    )
}
fn sails(index: usize, hub: V, yaw: f64) -> Animated {
    let mut m = Model::new(Cull::Always);
    m.box_at(DARK, [0.; 3], [0.32, 0.32, 0.22]);
    for arm in 0..4 {
        let r = rz(arm as f64 * PI / 2.);
        m.put(
            WOOD,
            box_shape(),
            compose(&[r, at(0., 3.1, 0.05, 0.1, 3.1, 0.1)]),
        );
        m.put(
            CANVAS,
            box_shape(),
            compose(&[r, at(0.62, 3.6, 0.08, 0.5, 2.3, 0.03)]),
        );
        for y in [1.6, 2.8, 4., 5.2] {
            m.put(
                LIGHT,
                box_shape(),
                compose(&[r, at(0.62, y, 0.12, 0.56, 0.04, 0.03)]),
            );
        }
    }
    animated(
        m,
        |part| format!("windmill-{index}-sails-{part}"),
        Motion::Sail { hub, yaw },
    )
}
pub fn build(b: &mut Batcher, s: &Scenery, g: &TerrainGrid) -> Vec<Animated> {
    let mut animations = Vec::new();
    let mut reeds = BTreeMap::<usize, Model>::new();
    let mut windmills = 0;
    let mut fires = 0;
    let mut roofs = 0;
    for p in &s.props {
        // The inn is asset-tooling's walk-in bundle (`crate::buildings`), not a procedural block.
        // The shrine (#431) is asset-tooling's kit (`crate::shrine`).
        if matches!(p.kind, PropKind::Inn | PropKind::ShrineKit) {
            continue;
        }
        let [hx, hy, hz] = half(p);
        let mut m = Model::new(Cull::Always);
        let mut f = frame(p, g, 0.);
        let (mut w, mut d, mut h, mut door_yaw) = (hx, hz, hy * 2., yaw(p));
        if matches!(
            p.kind,
            PropKind::Keep
                | PropKind::Inn
                | PropKind::House
                | PropKind::Farmhouse
                | PropKind::Smithy
                | PropKind::Barn
                | PropKind::Windmill
                | PropKind::Tent
                | PropKind::MineEntrance
        ) {
            (f, w, d, h, door_yaw) = building(s, g, p);
        }
        match p.kind {
            PropKind::Keep => keep(&mut m, w, d, h),
            PropKind::Inn | PropKind::ShrineKit => {
                unreachable!("the inn and the shrine are drawn from their bundles")
            }
            PropKind::House | PropKind::Farmhouse => {
                house(&mut m, w, d, h, THATCH, p.kind == PropKind::Farmhouse)
            }
            PropKind::Smithy => {
                m.box_at(STONE_DARK, [0., h / 2., 0.], [w, h / 2., d]);
                m.box_at(STONE, [0., 0.3, 0.], [w + 0.02, 0.3, d + 0.02]);
                let rise = w.min(d) * 0.8;
                roof(&mut m, SLATE, w, d, h, rise, 0.5);
                chimney(&mut m, STONE, -w * 0.5, -d * 0.4, h * 0.5, h + rise + 1.4);
                door(&mut m, d, 1.8, 2.6);
                m.box_at(FIRE, [0., 1.3, d + 0.12], [0.7, 0.35, 0.04]);
                m.cylinder(DARK, 7, 1., at(w * 0.55, 0., d + 1.3, 0.32, 0.55, 0.32));
                m.box_at(IRON, [w * 0.55, 0.68, d + 1.3], [0.34, 0.12, 0.16]);
                m.box_at(IRON, [w * 0.55 + 0.4, 0.72, d + 1.3], [0.12, 0.07, 0.1]);
            }
            PropKind::Barn => barn(&mut m, w, d, h),
            PropKind::Windmill => {
                let r = w.min(d);
                m.put(
                    STONE_LIGHT,
                    frustum(8, 0.78, false, true, 0.5),
                    at(0., 0., 0., r, h, r),
                );
                m.put(
                    STONE_DARK,
                    frustum(8, 1., false, true, 0.5),
                    at(0., 0., 0., r + 0.04, 0.5, r + 0.04),
                );
                let top = r * 0.78;
                m.put(
                    ROOF_WOOD,
                    frustum(8, 0., false, true, 0.5),
                    at(0., h, 0., top + 0.35, 2.6, top + 0.35),
                );
                door(&mut m, r * 0.92, 1., 2.);
                m.box_at(WINDOW, [0., h * 0.6, r * 0.86], [0.3, 0.4, 0.06]);
                m.box_at(DARK, [0., h + 0.9, top + 0.45], [0.18, 0.18, 0.5]);
                let anchor = frame(p, g, 0.).t;
                let hub = [
                    anchor[0] + (top + 0.9) * door_yaw.sin(),
                    anchor[1] + h + 0.9,
                    anchor[2] + (top + 0.9) * door_yaw.cos(),
                ];
                animations.push(sails(windmills, hub, door_yaw));
                windmills += 1;
            }
            PropKind::Well => {
                let r = hx.min(hz);
                m.put(
                    STONE,
                    frustum(10, 1., false, true, 0.5),
                    at(0., 0., 0., r, 0.95, r),
                );
                m.put(
                    STONE_DARK,
                    frustum(10, 1., false, true, 0.5),
                    at(0., 0.95, 0., r + 0.05, 0.12, r + 0.05),
                );
                m.put(0x23384a, disc(10), at(0., 1.08, 0., r - 0.18, 1., r - 0.18));
                for sx in [-1., 1.] {
                    m.box_at(WOOD, [sx * (r - 0.1), 1.6, 0.], [0.09, 1.6, 0.09]);
                }
                m.box_at(DARK, [0., 2.2, 0.], [r + 0.1, 0.06, 0.06]);
                m.box_at(IRON, [0., 1.75, 0.], [0.12, 0.16, 0.12]);
                roof(&mut m, RED, r + 0.1, r * 0.75, 3.1, 0.9, 0.25);
            }
            PropKind::Tree(v) => tree(&mut m, p, v),
            PropKind::Rock(_) => {
                m.cull = if p.collider.is_some() {
                    Cull::Always
                } else {
                    Cull::Mid
                };
                let s = seed(p);
                m.put(
                    if s > 0.5 { ROCK } else { ROCK_DARK },
                    blob(0, false, 0.25, (s * 7.).floor() + 60.),
                    at(0., hy * 0.7, 0., hx, hy * 1.4, hz),
                );
                if p.collider.is_some() {
                    m.put(
                        MOSS,
                        blob(0, false, 0.2, 70.),
                        at(hx * 0.2, hy * 1.75, 0., hx * 0.6, hy * 0.3, hz * 0.6),
                    );
                }
            }
            PropKind::Bush => {
                m.cull = Cull::Mid;
                let s = (seed(p) * 6.).floor();
                for (c, seed, p, h) in [
                    (0x4b7a34, s + 30., [0., hy * 0.7, 0.], [hx, hy * 1.1, hx]),
                    (
                        0x5f8f3a,
                        s + 40.,
                        [hx * 0.55, hy * 0.55, hx * 0.3],
                        [hx * 0.7, hy * 0.85, hx * 0.7],
                    ),
                    (
                        0x4b7a34,
                        s + 50.,
                        [-hx * 0.5, hy * 0.45, -hx * 0.35],
                        [hx * 0.62, hy * 0.75, hx * 0.62],
                    ),
                ] {
                    m.put(
                        c,
                        blob(0, true, 0.22, seed),
                        at(p[0], p[1], p[2], h[0], h[1], h[2]),
                    );
                }
            }
            PropKind::GrassTuft => grass(&mut m, p),
            PropKind::Flowers => {
                m.cull = Cull::Near;
                let c = [0xf2d64b, 0xb98ae0, 0xf4f1ea, 0xe5675c][usize::from(p.yaw) % 4];
                m.put(
                    0x5b8a39,
                    blade(),
                    at(0., -0.02, 0., hx * 0.5, hy * 1.4, hx * 0.5),
                );
                for i in 0..3 {
                    let a = i as f64 / 3. * TAU;
                    let h = hy * (1.5 + hash(&[f64::from(p.position[0]), i as f64]) * 0.8);
                    m.put(
                        c,
                        blob(0, true, 0., 90.),
                        at(
                            a.sin() * hx * 0.45,
                            h,
                            a.cos() * hx * 0.45,
                            0.07,
                            0.05,
                            0.07,
                        ),
                    );
                }
            }
            PropKind::Cliff => cliff(&mut m, p),
            PropKind::CaveRoof => {
                animations.push(cave_roof(p, roofs));
                roofs += 1;
            }
            PropKind::Campfire => {
                animations.push(campfire(&mut m, p, fires, f));
                fires += 1;
            }
            PropKind::PalisadeSegment => {
                let along = hx >= hz;
                let l = if along { hx } else { hz } * 2.;
                let r = if along { hz } else { hx };
                let count = (l / (r * 2.)).round().max(1.) as usize;
                let spacing = l / count as f64;
                for i in 0..count {
                    let offset = -l / 2. + spacing * (i as f64 + 0.5);
                    let (x, z) = if along { (offset, 0.) } else { (0., offset) };
                    let j = hash(&[f64::from(p.position[0]), f64::from(p.position[2]), i as f64]);
                    let h = hy * 2. - 0.25 + j * 0.4;
                    let c = if j > 0.5 { WOOD } else { DARK };
                    m.cylinder(c, 6, 1., at(x, 0., z, r, h, r));
                    m.cylinder(c, 6, 0., at(x, h, z, r, 0.55, r));
                }
                for y in [0.9, hy * 2. - 0.7] {
                    m.box_at(
                        TIMBER,
                        [0., y, 0.],
                        if along {
                            [hx, 0.08, r + 0.06]
                        } else {
                            [r + 0.06, 0.08, hz]
                        },
                    );
                }
            }
            PropKind::GatePost => {
                let r = hx.min(hz);
                m.cylinder(DARK, 7, 1., at(0., 0., 0., r, h, r));
                m.cylinder(DARK, 7, 0., at(0., h, 0., r, 0.7, r));
                m.box_at(IRON, [0., h * 0.45, 0.], [r + 0.03, 0.08, r + 0.03]);
                let (x, z) = (metres(p.position[0]), metres(p.position[2]));
                let mut best = None;
                let mut dist = 7.;
                for q in &s.props {
                    if q.kind != PropKind::GatePost {
                        continue;
                    }
                    let other = [metres(q.position[0]), metres(q.position[2])];
                    let d = (other[0] - x).hypot(other[1] - z);
                    if d > 0.01 && d < dist {
                        best = Some(other);
                        dist = d;
                    }
                }
                if let Some(q) = best {
                    if x < q[0] - 1e-6 || ((x - q[0]).abs() < 1e-6 && z < q[1]) {
                        let y = h - 0.35;
                        m.beam(TIMBER, [0., y, 0.], [q[0] - x, y, q[1] - z], 0.22, 6);
                        let mid = [(q[0] - x) / 2., y - 0.6, (q[1] - z) / 2.];
                        m.box_at(IRON, [mid[0], y - 0.3, mid[2]], [0.03, 0.25, 0.03]);
                        m.box_at(GLOW, mid, [0.16, 0.2, 0.16]);
                    }
                }
            }
            PropKind::Fence => {
                m.cull = Cull::Mid;
                let along = hx >= hz;
                let l = if along { hx } else { hz } * 2.;
                let posts = (l / 2.).round().max(1.) as usize + 1;
                for i in 0..posts {
                    let offset = -l / 2. + l * i as f64 / (posts - 1) as f64;
                    let (x, z) = if along { (offset, 0.) } else { (0., offset) };
                    m.box_at(DARK, [x, hy, z], [0.09, hy, 0.09]);
                }
                for y in [hy * 0.8, hy * 1.6] {
                    m.box_at(
                        WOOD,
                        [0., y, 0.],
                        if along {
                            [hx, 0.05, 0.04]
                        } else {
                            [0.04, 0.05, hz]
                        },
                    );
                }
            }
            PropKind::Tent => {
                m.put(CANVAS, gable(), compose(&[ry(PI / 2.), scale(d, h, w)]));
                m.put(
                    0xa8472f,
                    gable(),
                    compose(&[
                        translate(0., h * 0.02, 0.),
                        ry(PI / 2.),
                        scale(d + 0.02, h * 0.35, w + 0.02),
                    ]),
                );
                m.put(
                    EMBER,
                    pyramid(),
                    compose(&[translate(0., 0., d + 0.02), scale(w * 0.35, h * 0.62, 0.02)]),
                );
                for z in [d, -d] {
                    m.box_at(DARK, [0., h / 2. + 0.15, z], [0.05, h / 2. + 0.15, 0.05]);
                }
            }
            PropKind::Crate => {
                m.cull = Cull::Mid;
                m.box_at(LIGHT, [0., hy, 0.], [hx, hy, hz]);
                for y in [hy * 0.45, hy * 1.55] {
                    m.box_at(DARK, [0., y, 0.], [hx + 0.02, 0.05, hz + 0.02]);
                }
            }
            PropKind::Barrel => {
                m.cull = Cull::Mid;
                m.put(
                    WOOD,
                    frustum(9, 1.12, true, false, 0.),
                    at(0., 0., 0., hx * 0.88, h / 2., hx * 0.88),
                );
                m.cylinder(
                    WOOD,
                    9,
                    1. / 1.12,
                    at(0., h / 2., 0., hx * 0.98, h / 2., hx * 0.98),
                );
                for y in [h * 0.18, h * 0.8] {
                    m.put(
                        IRON,
                        frustum(9, 1., true, false, 0.),
                        at(0., y, 0., hx * 0.96, 0.05, hx * 0.96),
                    );
                }
            }
            PropKind::Cart => {
                m.cull = Cull::Mid;
                m.box_at(WOOD, [0., 0.62, 0.], [hx * 0.8, 0.08, hz]);
                m.box_at(LIGHT, [0., 0.9, hz - 0.04], [hx * 0.8, 0.22, 0.04]);
                m.box_at(LIGHT, [0., 0.9, -hz + 0.04], [hx * 0.8, 0.22, 0.04]);
                m.box_at(LIGHT, [hx * 0.8 - 0.04, 0.9, 0.], [0.04, 0.22, hz]);
                for side in [1., -1.] {
                    m.put(
                        DARK,
                        frustum(10, 1., true, true, 0.),
                        compose(&[
                            translate(hx * 0.25, 0.45, side * (hz + 0.08)),
                            rx(PI / 2.),
                            scale(0.45, 0.1, 0.45),
                        ]),
                    );
                    m.beam(
                        DARK,
                        [-hx * 0.8, 0.6, side * hz * 0.6],
                        [-hx * 1.7, 0.08, side * hz * 0.45],
                        0.05,
                        5,
                    );
                }
                m.box_at(CROP, [0.1, 0.95, 0.], [hx * 0.55, 0.2, hz * 0.7]);
            }
            PropKind::Signpost => {
                m.cull = Cull::Mid;
                m.box_at(WOOD, [0., h / 2., 0.], [0.07, h / 2., 0.07]);
                for (y, turn, l) in [(h - 0.25, 0., 0.55), (h - 0.6, PI * 0.6, 0.5)] {
                    m.rot_box(
                        LIGHT,
                        [0., y, 0.],
                        [l, 0.12, 0.03],
                        compose(&[ry(turn), translate(l * 0.55, 0., 0.)]),
                    );
                }
            }
            PropKind::Lamp => {
                m.cull = Cull::Mid;
                m.box_at(STONE_DARK, [0., 0.15, 0.], [0.18, 0.15, 0.18]);
                m.cylinder(IRON, 6, 0.7, at(0., 0., 0., 0.08, h, 0.08));
                m.box_at(IRON, [0.25, h - 0.05, 0.], [0.3, 0.035, 0.035]);
                m.box_at(GLOW, [0.48, h - 0.42, 0.], [0.13, 0.18, 0.13]);
                m.put(IRON, pyramid(), at(0.48, h - 0.24, 0., 0.18, 0.18, 0.18));
            }
            PropKind::MineEntrance => {
                m.box_at(
                    0x141414,
                    [0., h * 0.45, -d * 0.2],
                    [w * 0.72, h * 0.45, d * 0.6],
                );
                for sx in [-1., 1.] {
                    m.box_at(WOOD, [sx * w * 0.78, h / 2., d * 0.4], [0.28, h / 2., 0.28]);
                    m.put(
                        ROCK_DARK,
                        blob(0, false, 0.3, sx + 5.),
                        at(sx * (w + 0.4), 0.3, d * 0.5, 0.9, 0.7, 0.8),
                    );
                }
                m.box_at(DARK, [0., h - 0.25, d * 0.4], [w * 0.95, 0.3, 0.32]);
                m.box_at(DARK, [0., h * 0.72, d * 0.4], [w * 0.78, 0.12, 0.2]);
                for sx in [-0.45, 0.45] {
                    m.box_at(IRON, [sx, 0.05, d + 2.4], [0.04, 0.04, 2.8]);
                }
                for i in 0..6 {
                    m.box_at(
                        DARK,
                        [0., 0.02, d + 0.2 + i as f64 * 0.9],
                        [0.7, 0.04, 0.12],
                    );
                }
            }
            // A loading dock (#59): a stone block with a plank deck.
            PropKind::LoadingDock => {
                m.box_at(STONE, [0., hy - 0.03, 0.], [hx, hy - 0.03, hz]);
                m.box_at(WOOD, [0., hy * 2. - 0.03, 0.], [hx, 0.03, hz]);
            }
            // A ramp (#59): planks over a stone wedge, rising toward local +Z.
            PropKind::Ramp => {
                m.put(STONE, wedge(), at(0., 0., 0., hx, hy * 2., hz));
                m.put(WOOD, wedge(), at(0., 0.03, 0., hx * 0.9, hy * 2., hz));
            }
            PropKind::Dock => {
                m.cull = Cull::Mid;
                let deck = 0.34;
                let planks = (hx * 2. / 0.5).round() as usize;
                for i in 0..planks {
                    m.box_at(
                        if i % 2 == 0 { LIGHT } else { WOOD },
                        [-hx + 0.25 + i as f64 * 0.5, deck, 0.],
                        [0.24, 0.05, hz],
                    );
                }
                for x in [-hx + 0.3, 0., hx - 0.3] {
                    for z in [hz - 0.1, -hz + 0.1] {
                        m.cylinder(DARK, 6, 1., at(x, -0.4, z, 0.1, deck + 0.75, 0.1));
                    }
                }
            }
            PropKind::CropRow => {
                m.cull = Cull::Far;
                m.box_at(shade(0x6e4d32, 0.9), [0., 0.06, 0.], [hx, 0.08, hz * 0.8]);
                m.cull = Cull::Near;
                let count = (hx * 2. / 0.7).floor() as usize;
                for i in 0..count {
                    let x = -hx + 0.35 + i as f64 * 0.7;
                    let j = hash(&[f64::from(p.position[2]), i as f64]);
                    let h = 0.55 + j * 0.25;
                    m.put(
                        STEM,
                        blade(),
                        compose(&[
                            translate(x, 0.08, 0.),
                            ry(j * 6.),
                            scale(0.12, h * 0.8, 0.12),
                        ]),
                    );
                    m.put(
                        CROP,
                        blade(),
                        compose(&[
                            translate(x + 0.12, 0.08, 0.05),
                            ry(j * 9. + 1.),
                            scale(0.13, h, 0.13),
                        ]),
                    );
                }
                m.cull = Cull::Far;
                m.box_at(mix(STEM, CROP, 0.5), [0., 0.3, 0.], [hx - 0.2, 0.12, 0.16]);
            }
            PropKind::Waystone => {
                m.box_at(STONE_DARK, [0., 0.12, 0.], [hx + 0.05, 0.12, hz + 0.05]);
                m.put(
                    0x8e98a6,
                    frustum(4, 0.7, false, true, 0.5),
                    at(0., 0.24, 0., hx * 1.25, h - 0.6, hz * 1.25),
                );
                m.put(
                    0x8e98a6,
                    frustum(4, 0., false, true, 0.5),
                    at(0., h - 0.36, 0., hx * 0.88, 0.5, hz * 0.88),
                );
                for (x, z, r) in [
                    (0., 1., 0.),
                    (0., -1., PI),
                    (1., 0., PI / 2.),
                    (-1., 0., -PI / 2.),
                ] {
                    m.rot_box(
                        RUNE,
                        [x * hx * 1.02, h * 0.55, z * hz * 1.02],
                        [0.12, 0.28, 0.03],
                        ry(r),
                    );
                }
            }
            PropKind::Gravestone => {
                m.cull = Cull::Mid;
                let top = hy * 2. - hx;
                m.box_at(STONE_DARK, [0., top / 2., 0.], [hx, top / 2., hz]);
                m.put(
                    STONE_DARK,
                    frustum(8, 1., true, true, 0.),
                    compose(&[translate(0., top, -hz), rx(PI / 2.), scale(hx, hz * 2., hx)]),
                );
                m.box_at(0x6e4d32, [0., 0.06, hz + 0.55], [hx * 0.9, 0.08, 0.5]);
            }
            PropKind::Reeds => {
                let group = (seed(p) * 3.).floor() as usize;
                let target = reeds
                    .entry(group)
                    .or_insert_with(|| Model::new(Cull::Always));
                for i in 0..5 {
                    let a = i as f64 / 5. * TAU;
                    let h = hy
                        * 2.
                        * (0.8
                            + hash(&[
                                f64::from(p.position[0]),
                                f64::from(p.position[2]),
                                i as f64,
                            ]) * 0.45);
                    let mut local = Model::new(Cull::Always);
                    local.put(
                        0x8e9a52,
                        blade(),
                        compose(&[
                            translate(a.sin() * hx * 1.2, -0.05, a.cos() * hx * 1.2),
                            ry(a),
                            rx(-0.08),
                            scale(0.06, h, 0.06),
                        ]),
                    );
                    if i % 2 == 0 {
                        local.cylinder(
                            0x6b4a2e,
                            5,
                            1.,
                            at(
                                a.sin() * hx * 1.2,
                                h * 0.78,
                                a.cos() * hx * 1.2,
                                0.045,
                                0.2,
                                0.045,
                            ),
                        );
                    }
                    for mut part in local.parts {
                        part.a = compose(&[f, part.a]);
                        target.parts.push(part);
                    }
                }
            }
        }
        // An Outpost kit building (#420) keeps its blockout as the fallback while the kit is
        // not drawn.
        if let Some(id) = p
            .collider
            .filter(|&id| mmorpg_scenery::outpost_kit::adopts(id))
        {
            animations.push(animated(
                m,
                |part| format!("outpost-fallback-{id}-{part}"),
                Motion::Fallback {
                    frame: f,
                    kit: Kit::Outpost,
                },
            ));
            continue;
        }
        let instance = matches!(
            p.kind,
            PropKind::Tree(_) | PropKind::Rock(_) | PropKind::GrassTuft
        );
        for part in m.parts {
            b.put(
                part.c,
                &part.g,
                compose(&[f, part.a]),
                metres(p.position[0]),
                metres(p.position[2]),
                part.cull,
                instance,
            );
        }
    }
    for (group, m) in reeds {
        animations.push(animated(
            m,
            |part| format!("reeds-{group}-{part}"),
            Motion::Reed {
                phase: group as f64 * 2.1,
                ground_y: 0.,
            },
        ));
    }
    // The market square's settlement props (#421) stand as boxes of their collision proxies
    // while the client does not draw the props.
    let square = crate::settlement_props::frames(s);
    if !square.is_empty() {
        let mut m = Model::new(Cull::Always);
        for (entry, frame) in square {
            for &([x, y, z], [hx, hy, hz], colour) in crate::settlement_props::fallback(entry) {
                m.put(
                    colour,
                    box_shape(),
                    compose(&[frame, translate(x, y, z), scale(hx, hy, hz)]),
                );
            }
        }
        animations.push(animated(
            m,
            |part| format!("settlement-fallback-{part}"),
            Motion::Fallback {
                frame: ID,
                kit: Kit::SettlementProps,
            },
        ));
    }
    animations
}
