//! World traps (#300) in both clients: stand-in shapes for the plate, its rocks, the swinging
//! blades and the spike pit, placed from content and the projection's tick. A blade's pose and
//! a rock's fall are the zone's own functions (`Blade::pose_at`, `Rockfall::rock_at`), and only a
//! fired trap's age comes from the projection, so nothing here is simulated.
use std::f64::consts::SQRT_2;

use mmorpg_core::{Blade, Rockfall, Trap, TrapEffect, TrapTrigger, TriggerVolume, ZoneSnapshot};

use super::UnitPresenter;
use super::kit::{Frame, IQ, Primitive, Q};
use crate::{Draw, geometry::V};

/// Traps within this range (metres) of the viewer are drawn.
const DRAW_RANGE_METRES: f64 = 90.;

const PLATE_BASE: u32 = 0x3a342c;
const PLATE: u32 = 0xa8822f;
const PLATE_FIRED: u32 = 0x5c5240;
const ROCK: u32 = 0x76716a;
const ARM: u32 = 0x4d4b48;
const HEAD: u32 = 0xc8ccd2;
const PIT_FLOOR: u32 = 0x1c1a18;
const PIT_RIM: u32 = 0x8b8173;
const SPIKE: u32 = 0x6d6f73;

fn metres(value: f64) -> f64 {
    value / 100.
}

fn cross(a: V, b: V) -> V {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// The quaternion rotating the box axes X and Y onto the orthonormal `x` and `y`.
fn from_basis(x: V, y: V) -> Q {
    let z = cross(x, y);
    let trace = x[0] + y[1] + z[2];
    if trace > 0. {
        let s = (trace + 1.).sqrt() * 2.;
        [
            (y[2] - z[1]) / s,
            (z[0] - x[2]) / s,
            (x[1] - y[0]) / s,
            0.25 * s,
        ]
    } else if x[0] > y[1] && x[0] > z[2] {
        let s = (1. + x[0] - y[1] - z[2]).sqrt() * 2.;
        [
            0.25 * s,
            (y[0] + x[1]) / s,
            (z[0] + x[2]) / s,
            (y[2] - z[1]) / s,
        ]
    } else if y[1] > z[2] {
        let s = (1. + y[1] - x[0] - z[2]).sqrt() * 2.;
        [
            (y[0] + x[1]) / s,
            0.25 * s,
            (z[1] + y[2]) / s,
            (z[0] - x[2]) / s,
        ]
    } else {
        let s = (1. + z[2] - x[0] - y[1]).sqrt() * 2.;
        [
            (z[0] + x[2]) / s,
            (z[1] + y[2]) / s,
            0.25 * s,
            (x[1] - y[0]) / s,
        ]
    }
}

impl UnitPresenter {
    /// Draws every world trap near the viewer (`viewer` is its feet, metres).
    pub(super) fn traps(
        &mut self,
        draws: &mut Vec<Draw>,
        snapshot: &ZoneSnapshot,
        viewer: Option<V>,
    ) {
        let content = std::sync::Arc::clone(&self.content);
        for trap in content.traps() {
            let [x, z] = trap.anchor();
            if let Some(at) = viewer {
                let (dx, dz) = (at[0] - f64::from(x) / 100., at[2] - f64::from(z) / 100.);
                if dx.hypot(dz) > DRAW_RANGE_METRES {
                    continue;
                }
            }
            let age = snapshot
                .traps
                .iter()
                .find(|view| view.id == trap.id)
                .map(|view| u32::from(view.age));
            self.draw_plate(draws, trap, age);
            match &trap.effect {
                TrapEffect::Rockfall(fall) => self.draw_rocks(draws, trap, fall, age),
                TrapEffect::Blade(blade) => self.draw_blade(draws, trap, blade, snapshot.tick),
                TrapEffect::Spikes(volume) => self.draw_spikes(draws, trap, *volume),
            }
        }
    }

    /// A pressure plate: a dark base and a brass top that dips and dulls once it fired.
    fn draw_plate(&mut self, draws: &mut Vec<Draw>, trap: &Trap, age: Option<u32>) {
        let TrapTrigger::Pressure { plate, .. } = trap.trigger else {
            return;
        };
        let (cx, cz) = (f64::from(plate.centre[0]), f64::from(plate.centre[1]));
        let centre = self.rendered([plate.centre[0], plate.floor, plate.centre[1]]);
        let (width, depth) = (
            metres(f64::from(plate.half[0])) * 2.,
            metres(f64::from(plate.half[1])) * 2.,
        );
        draws.push(self.resources.primitive(
            format!("trap-{}-plate-base", trap.id.get()),
            Primitive::Box([width + 0.2, 0.04, depth + 0.2]),
            PLATE_BASE,
            Frame {
                p: [metres(cx), f64::from(centre[1]) + 0.02, metres(cz)],
                q: IQ,
            },
        ));
        let (lift, color) = if age.is_some() {
            (0.03, PLATE_FIRED)
        } else {
            (0.06, PLATE)
        };
        draws.push(self.resources.primitive(
            format!("trap-{}-plate", trap.id.get()),
            Primitive::Box([width, 0.05, depth]),
            color,
            Frame {
                p: [metres(cx), f64::from(centre[1]) + lift, metres(cz)],
                q: IQ,
            },
        ));
    }

    /// Rocks wait at the top (they are the plate's other tell), fall and lie as rubble.
    fn draw_rocks(
        &mut self,
        draws: &mut Vec<Draw>,
        trap: &Trap,
        fall: &Rockfall,
        age: Option<u32>,
    ) {
        let radius = metres(f64::from(fall.radius));
        for (index, rock) in fall.rocks.iter().enumerate() {
            let pose = fall.rock_at(rock, age.unwrap_or(0));
            if pose.cleared {
                continue;
            }
            let [x, y, z] = self.rendered(pose.centre).map(f64::from);
            draws.push(self.resources.primitive(
                format!("trap-{}-rock-{index}", trap.id.get()),
                Primitive::Ball(radius),
                ROCK,
                Frame {
                    p: [x, y, z],
                    q: IQ,
                },
            ));
        }
    }

    /// A blade at the projection's tick: an iron arm from the pivot to a steel head.
    fn draw_blade(&mut self, draws: &mut Vec<Draw>, trap: &Trap, blade: &Blade, tick: u64) {
        let pose = blade.pose_at(tick);
        let id = trap.id.get();
        // The head and the arm are placed by the zone's pose, relief added like every unit.
        #[allow(clippy::cast_possible_truncation)]
        let head = self.rendered(pose.head.map(|axis| axis.round() as i32));
        let head = head.map(f64::from);
        let frame = Frame {
            p: head,
            q: from_basis(pose.across, [-pose.arm[0], -pose.arm[1], -pose.arm[2]]),
        };
        let half = blade.half_extents.map(f64::from);
        let size = [
            metres(half[0]) * 2.,
            metres(half[1]) * 2.,
            metres(half[2]) * 2.,
        ];
        draws.push(self.resources.primitive(
            format!("trap-{id}-head"),
            Primitive::Box(size),
            HEAD,
            frame,
        ));
        let reach = metres(f64::from(blade.arm));
        let mid = [
            head[0] - pose.arm[0] * reach / 2.,
            head[1] - pose.arm[1] * reach / 2.,
            head[2] - pose.arm[2] * reach / 2.,
        ];
        draws.push(self.resources.primitive(
            format!("trap-{id}-arm"),
            Primitive::Box([0.1, reach, 0.1]),
            ARM,
            Frame { p: mid, q: frame.q },
        ));
    }

    /// A spike pit: a dark floor edged with stones, and a grid of iron spikes (nothing hidden).
    fn draw_spikes(&mut self, draws: &mut Vec<Draw>, trap: &Trap, volume: TriggerVolume) {
        let id = trap.id.get();
        let centre = self.rendered([volume.centre[0], volume.floor, volume.centre[1]]);
        let [x, y, z] = centre.map(f64::from);
        let (hw, hd) = (
            metres(f64::from(volume.half[0])),
            metres(f64::from(volume.half[1])),
        );
        draws.push(self.resources.primitive(
            format!("trap-{id}-floor"),
            Primitive::Box([2. * hw, 0.04, 2. * hd]),
            PIT_FLOOR,
            Frame {
                p: [x, y + 0.03, z],
                q: IQ,
            },
        ));
        let rim = 0.18;
        for (part, (dx, dz, w, d)) in [
            (hw + rim / 2., 0., rim, 2. * hd + 2. * rim),
            (-hw - rim / 2., 0., rim, 2. * hd + 2. * rim),
            (0., hd + rim / 2., 2. * hw, rim),
            (0., -hd - rim / 2., 2. * hw, rim),
        ]
        .into_iter()
        .enumerate()
        {
            draws.push(self.resources.primitive(
                format!("trap-{id}-rim-{part}"),
                Primitive::Box([w, 0.14, d]),
                PIT_RIM,
                Frame {
                    p: [x + dx, y + 0.07, z + dz],
                    q: IQ,
                },
            ));
        }
        let spacing = 0.45;
        let (columns, rows) = ((hw / spacing).floor() as i32, (hd / spacing).floor() as i32);
        let mut index = 0;
        for column in -columns..=columns {
            for row in -rows..=rows {
                let (sx, sz) = (f64::from(column) * spacing, f64::from(row) * spacing);
                draws.push(self.resources.primitive(
                    format!("trap-{id}-spike-{index}"),
                    Primitive::Box([0.1 / SQRT_2, 0.42, 0.1 / SQRT_2]),
                    SPIKE,
                    Frame {
                        p: [x + sx, y + 0.21, z + sz],
                        q: IQ,
                    },
                ));
                index += 1;
            }
        }
    }
}
