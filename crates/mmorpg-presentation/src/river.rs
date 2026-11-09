//! Greyhaven's watercourses dressed from asset-tooling's `river-stones` kit (#538), and the
//! limestone sink where the village brook goes underground.
//!
//! The stones are placed along the same rounded course the water is drawn on
//! ([`crate::brook::water_course`]) and clear of the bridge, the mill and the palisade gate. They
//! are presentation only and do not obstruct wading. The sink dresses the brook's drain, whose
//! colliders stay authoritative, with limestone rocks over a dark opening.
use mmorpg_core::greyhaven_vale::brook as content;
use mmorpg_scenery::{Scenery, TerrainGrid};

use crate::{
    Cull,
    batching::matrix,
    brook::{village_drawn, water_course},
    geometry::{compose, ry, scale, translate},
    lod::Placement,
    terrain::{metres, noise, surface},
};

const BED: [&str; 2] = ["river-stones.kit.bed-1", "river-stones.kit.bed-2"];
const BANK: [&str; 2] = ["river-stones.kit.bank-1", "river-stones.kit.bank-2"];
const BAR: [&str; 2] = ["river-stones.kit.bar-1", "river-stones.kit.bar-2"];
const BOULDER: [&str; 3] = [
    "river-stones.kit.boulder-1",
    "river-stones.kit.boulder-2",
    "river-stones.kit.boulder-3",
];
const STEP: [&str; 2] = ["river-stones.kit.step-1", "river-stones.kit.step-2"];
/// The bundle entries this module places.
pub const ENTRIES: [&str; 11] = [
    BANK[0], BANK[1], BAR[0], BAR[1], BED[0], BED[1], BOULDER[0], BOULDER[1], BOULDER[2], STEP[0],
    STEP[1],
];

/// Metres of course between each kind of piece.
const BED_EVERY: f64 = 2.3;
const BANK_EVERY: f64 = 2.2;
const BAR_EVERY: f64 = 6.5;
const BOULDER_EVERY: f64 = 7.5;
/// How much of the banks stay bare, so the line of stones is broken.
const BANK_GAPS: f64 = 0.24;
const BOULDER_CHANCE: f64 = 0.52;
/// Radians of turn over six metres of course from which a bend grows a gravel bar inside it.
const BAR_TURN: f64 = 0.28;
/// Where along the village brook (the point nearest this) the stepping stones cross it.
const FORD: [f64; 2] = [-6.6, 58.5];
const FORD_STONES: usize = 6;
/// How far above the water a stepping stone's top stands, metres.
const STEP_ABOVE_WATER: f64 = 0.1;
/// The height of a stepping stone's top above its origin, metres.
const STEP_TOP: f64 = 0.29;

/// The last metres of the village brook, under the sink's rocks, carry no stones.
const SINK_CLEAR: f64 = 3.0;

/// Where stones would crowd the bridge, the mill or the gate.
fn clear(x: f64, z: f64) -> bool {
    let bridge = x.abs() < 3.2 && (27.5..36.5).contains(&z);
    let mill = (-35.0..-29.0).contains(&x) && (17.5..25.0).contains(&z);
    let gate = (-7.5..-1.0).contains(&x) && (48.5..51.5).contains(&z);
    !(bridge || mill || gate)
}

struct Course {
    stations: Vec<[f64; 2]>,
    arcs: Vec<f64>,
}

impl Course {
    fn new(points: &[[i32; 2]]) -> Self {
        let stations = water_course(points);
        let mut arcs = vec![0.];
        for pair in stations.windows(2) {
            let last = arcs[arcs.len() - 1];
            arcs.push(last + (pair[1][0] - pair[0][0]).hypot(pair[1][1] - pair[0][1]));
        }
        Self { stations, arcs }
    }

    fn length(&self) -> f64 {
        self.arcs[self.arcs.len() - 1]
    }

    /// The point and unit direction `along` metres down the course.
    fn at(&self, along: f64) -> ([f64; 2], [f64; 2]) {
        let i = self
            .arcs
            .partition_point(|arc| *arc <= along)
            .clamp(1, self.arcs.len() - 1);
        let (a, b) = (self.stations[i - 1], self.stations[i]);
        let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
        let length = dx.hypot(dz).max(1e-6);
        let t = ((along - self.arcs[i - 1]) / length).clamp(0., 1.);
        ([a[0] + dx * t, a[1] + dz * t], [dx / length, dz / length])
    }

    fn nearest(&self, point: [f64; 2]) -> f64 {
        let squared = |i: usize| {
            (self.stations[i][0] - point[0]).powi(2) + (self.stations[i][1] - point[1]).powi(2)
        };
        let best = (0..self.stations.len())
            .min_by(|a, b| squared(*a).total_cmp(&squared(*b)))
            .unwrap_or(0);
        self.arcs[best]
    }
}

/// A yaw (about +Y) that turns a kit piece's +X along `direction`.
fn along(direction: [f64; 2]) -> f64 {
    (-direction[1]).atan2(direction[0])
}

fn place(out: &mut Vec<Placement>, entry: &'static str, x: f64, y: f64, z: f64, yaw: f64) {
    place_scaled(out, entry, [x, y, z], yaw, 1., Cull::Mid);
}

fn place_scaled(
    out: &mut Vec<Placement>,
    entry: &'static str,
    [x, y, z]: [f64; 3],
    yaw: f64,
    size: f64,
    cull: Cull,
) {
    out.push(Placement {
        entry,
        model: matrix(compose(&[
            translate(x, y, z),
            ry(yaw),
            scale(size, size, size),
        ])),
        scale: size,
        cull,
    });
}

/// One of `pieces` from a noise value, whose useful range is about 0.3 to 0.7.
fn pick<const N: usize>(pieces: [&'static str; N], chooser: f64) -> &'static str {
    let spread = ((chooser - 0.3) / 0.4).clamp(0., 0.999);
    pieces[(spread * N as f64) as usize]
}

fn dress_course(
    out: &mut Vec<Placement>,
    g: &TerrainGrid,
    points: &[[i32; 2]],
    half_width: f64,
    level: f64,
    is_village: bool,
) {
    let course = Course::new(points);
    let end = course.length() - if is_village { SINK_CLEAR } else { 0.5 };
    let mut at = 1.2;
    while at < end {
        let ([x, z], direction) = course.at(at);
        if clear(x, z) {
            let y = surface(g, x, z) + 0.01;
            place(
                out,
                pick(BED, noise(x, z, 1.3, 53801.)),
                x,
                y,
                z,
                along(direction),
            );
        }
        at += BED_EVERY;
    }
    let mut at = 0.8;
    while at < end {
        let ([x, z], direction) = course.at(at);
        let normal = [-direction[1], direction[0]];
        for side in [-1., 1.] {
            let reach = half_width + 0.2;
            let (bx, bz) = (x + normal[0] * side * reach, z + normal[1] * side * reach);
            if clear(bx, bz) && noise(bx, bz, 1.7, 53802.) > BANK_GAPS {
                let y = surface(g, bx, bz);
                place(
                    out,
                    pick(BANK, noise(bx, bz, 2.3, 53803.)),
                    bx,
                    y,
                    bz,
                    along(direction),
                );
            }
        }
        at += BANK_EVERY;
    }
    let mut at = 3.5;
    while at < end - 3. {
        let ([x, z], direction) = course.at(at);
        let (_, before) = course.at(at - 3.);
        let (_, after) = course.at(at + 3.);
        let turn = (before[0] * after[1] - before[1] * after[0])
            .atan2(before[0] * after[0] + before[1] * after[1]);
        if turn.abs() > BAR_TURN {
            // The inside of a bend, where the slower water drops its gravel.
            let side = turn.signum();
            let normal = [-direction[1], direction[0]];
            let (bx, bz) = (
                x + normal[0] * side * half_width * 0.55,
                z + normal[1] * side * half_width * 0.55,
            );
            if clear(bx, bz) {
                let y = surface(g, bx, bz) + 0.02;
                place(
                    out,
                    pick(BAR, noise(bx, bz, 2.9, 53804.)),
                    bx,
                    y,
                    bz,
                    along(direction),
                );
            }
        }
        at += BAR_EVERY;
    }
    let mut at = 4.;
    while at < end - 2. {
        let ([x, z], direction) = course.at(at);
        if noise(x, z, 3.7, 53805.) > BOULDER_CHANCE {
            let normal = [-direction[1], direction[0]];
            let across = (noise(x, z, 4.1, 53806.) - 0.5) * 1.3 * half_width;
            let (bx, bz) = (x + normal[0] * across, z + normal[1] * across);
            if clear(bx, bz) {
                // The largest boulder would fill the narrow village brook.
                let pieces = if is_village {
                    [BOULDER[1], BOULDER[2], BOULDER[2]]
                } else {
                    BOULDER
                };
                let yaw = noise(bx, bz, 5.3, 53807.) * std::f64::consts::TAU;
                let y = surface(g, bx, bz);
                place(
                    out,
                    pick(pieces, noise(bx, bz, 4.9, 53808.)),
                    bx,
                    y,
                    bz,
                    yaw,
                );
            }
        }
        at += BOULDER_EVERY;
    }
    if is_village {
        let ([x, z], direction) = course.at(course.nearest(FORD));
        let normal = [-direction[1], direction[0]];
        for i in 0..FORD_STONES {
            let across = (i as f64 / (FORD_STONES - 1) as f64 - 0.5) * 2. * (half_width + 0.15);
            let (sx, sz) = (x + normal[0] * across, z + normal[1] * across);
            let y = (level + STEP_ABOVE_WATER - STEP_TOP).max(surface(g, sx, sz));
            let yaw = noise(sx, sz, 6.1, 53809.) * std::f64::consts::TAU;
            place(out, STEP[i % 2], sx, y, sz, yaw);
        }
    }
}

/// The limestone over the drain at the village brook's last point: jambs either side of the
/// opening, a lintel across it and a rock mass behind, so the water runs in under the rock.
fn sink(out: &mut Vec<Placement>, g: &TerrainGrid) {
    let course = village_drawn();
    let end = course[course.len() - 1].map(metres);
    let on_ground = |x: f64, z: f64| [x, surface(g, x, z), z];
    let pieces: [(&'static str, [f64; 3], f64, f64); 6] = [
        (
            "limestone-rocks.kit.outcrop-1",
            on_ground(end[0], end[1] + 4.6),
            0.,
            1.,
        ),
        (
            "limestone-rocks.kit.outcrop-2",
            on_ground(end[0] - 6.8, end[1] + 3.8),
            2.3,
            0.9,
        ),
        (
            "limestone-rocks.kit.outcrop-2",
            // Smaller on this side: the South Road runs just to the east.
            on_ground(end[0] + 5.2, end[1] + 3.9),
            -1.1,
            0.6,
        ),
        (
            "limestone-rocks.kit.block-1",
            on_ground(end[0] - 3.6, end[1]),
            0.2,
            0.92,
        ),
        (
            "limestone-rocks.kit.block-1",
            on_ground(end[0] + 3.5, end[1]),
            3.0,
            0.9,
        ),
        (
            "limestone-rocks.kit.ledge",
            [end[0], surface(g, end[0], end[1]) + 1.15, end[1]],
            0.,
            0.52,
        ),
    ];
    for (entry, at, yaw, size) in pieces {
        place_scaled(out, entry, at, yaw, size, Cull::Far);
    }
}

/// Every stone and sink rock on `s`, or none outside Greyhaven.
#[must_use]
pub fn placements(s: &Scenery) -> Vec<Placement> {
    let mut out = Vec::new();
    if !s.greyhaven() {
        return out;
    }
    let g = s.terrain_grid(crate::TERRAIN_STEP_UNITS);
    for (points, half_width, surface_level, is_village) in [
        (village_drawn(), content::HALF_WIDTH, content::SURFACE, true),
        (
            content::inflow(),
            content::UPPER_HALF_WIDTH,
            content::UPPER_SURFACE,
            false,
        ),
        (
            content::outlet(),
            content::UPPER_HALF_WIDTH,
            content::UPPER_SURFACE,
            false,
        ),
    ] {
        dress_course(
            &mut out,
            &g,
            points,
            metres(half_width),
            metres(surface_level),
            is_village,
        );
    }
    sink(&mut out, &g);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stones_keep_clear_of_the_crossings_and_every_entry_is_used() {
        let scenery = mmorpg_scenery::greyhaven_vale_scenery();
        let placed = placements(&scenery);
        for placement in placed
            .iter()
            .filter(|p| p.entry.starts_with("river-stones"))
        {
            let (x, z) = (
                f64::from(placement.model[12]),
                f64::from(placement.model[14]),
            );
            assert!(clear(x, z), "{} at {x:.1},{z:.1}", placement.entry);
        }
        for entry in ENTRIES {
            assert!(
                placed.iter().any(|p| p.entry == entry),
                "{entry} is never placed"
            );
        }
        assert_eq!(
            placed
                .iter()
                .filter(|p| p.entry.starts_with("limestone"))
                .count(),
            6
        );
    }

    #[test]
    fn stepping_stones_stand_clear_of_the_water() {
        let scenery = mmorpg_scenery::greyhaven_vale_scenery();
        let level = metres(content::SURFACE) as f32;
        for placement in placements(&scenery)
            .iter()
            .filter(|p| STEP.contains(&p.entry))
        {
            assert!(
                placement.model[13] + STEP_TOP as f32 > level,
                "a drowned stepping stone"
            );
        }
    }
}
