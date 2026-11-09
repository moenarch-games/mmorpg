//! The first world traps (#300, Atlas 5.2.i300): pressure-plate falling rocks and swinging
//! blades in the Greyfang cave, and a spike pit in the Redbrand camp. All are visible (a raised
//! plate, a pit edged in stones, blades that swing on a fixed schedule) and leave the cave's
//! routes open: nothing here blocks a unit.
//!
//! Timings are ticks at the zone's 30 Hz; distances are units (centimetres).

use crate::{Blade, RockDrop, Rockfall, Trap, TrapEffect, TrapId, TrapTrigger, TriggerVolume};

/// The Greyfang cavern's falling rocks and its plate.
pub const ROCKFALL_TRAP: TrapId = TrapId::new(1);
/// The inner chamber passage's two blades.
pub const PASSAGE_BLADE_WEST: TrapId = TrapId::new(2);
pub const PASSAGE_BLADE_EAST: TrapId = TrapId::new(3);
/// The Redbrand camp's spike pit.
pub const CAMP_SPIKES: TrapId = TrapId::new(4);

/// The cave's floor (`super::cave::GREYFANG.floor_y`).
const CAVE_FLOOR: i32 = super::cave::GREYFANG.floor_y;
/// Blade pivots hang under the passage's roof.
const PIVOT_Y: i32 = CAVE_FLOOR + 400;
/// 40°.
const SWING_AMPLITUDE: u16 = 7_282;

/// Damage of one falling rock (a unit is hit at most once per fall).
pub const ROCK_DAMAGE: u16 = 26;
/// A plate re-arms 10 s after it fires, whatever stands on it.
pub const PLATE_REARM_TICKS: u32 = 300;
/// Damage of one blade hit.
pub const BLADE_DAMAGE: u16 = 12;
/// The ticks between hits from the same blade: enough for one pass per swing.
pub const BLADE_REHIT_TICKS: u16 = 30;
/// One full swing, there and back: 3 s.
pub const BLADE_PERIOD_TICKS: u32 = 90;
/// Damage of the spikes per hit.
pub const SPIKE_DAMAGE: u16 = 7;
/// A unit standing in the pit is hit about every 0.75 s.
pub const SPIKE_REHIT_TICKS: u16 = 22;

/// The ground under the Redbrand camp's spike pit, measured against the Hollow's terrain.
pub const SPIKE_FLOOR: i32 = 1_100;

/// Every trap of the zone, in ID order.
#[must_use]
pub fn traps() -> Vec<Trap> {
    let blade = |x: i32, phase: u32| Blade {
        // Across the 4 m passage (along Z), so a unit walking through meets the head.
        pivot: [x, PIVOT_Y, -13_800],
        arm: 280,
        half_extents: [20, 60, 70],
        swing_yaw: 0,
        amplitude: SWING_AMPLITUDE,
        period: BLADE_PERIOD_TICKS,
        phase,
    };
    vec![
        Trap {
            id: ROCKFALL_TRAP,
            trigger: TrapTrigger::Pressure {
                plate: TriggerVolume {
                    centre: [5_950, -14_150],
                    half: [70, 70],
                    floor: CAVE_FLOOR,
                    height: 25,
                },
                rearm_ticks: PLATE_REARM_TICKS,
            },
            effect: TrapEffect::Rockfall(Rockfall {
                // Over the plate, so whoever stands on it is under one of them.
                rocks: vec![
                    RockDrop {
                        at: [5_950, -14_150],
                        delay: 0,
                    },
                    RockDrop {
                        at: [5_910, -14_190],
                        delay: 4,
                    },
                    RockDrop {
                        at: [5_990, -14_190],
                        delay: 8,
                    },
                    RockDrop {
                        at: [5_910, -14_110],
                        delay: 12,
                    },
                    RockDrop {
                        at: [5_990, -14_110],
                        delay: 16,
                    },
                ],
                start_y: CAVE_FLOOR + 400,
                floor: CAVE_FLOOR,
                radius: 25,
                rubble_ticks: 60,
            }),
            damage: ROCK_DAMAGE,
            // The whole fall: one fall hits a unit once.
            rehit_ticks: 300,
        },
        Trap {
            id: PASSAGE_BLADE_WEST,
            trigger: TrapTrigger::Live,
            effect: TrapEffect::Blade(blade(6_870, 0)),
            damage: BLADE_DAMAGE,
            rehit_ticks: BLADE_REHIT_TICKS,
        },
        Trap {
            id: PASSAGE_BLADE_EAST,
            trigger: TrapTrigger::Live,
            effect: TrapEffect::Blade(blade(7_130, BLADE_PERIOD_TICKS / 2)),
            damage: BLADE_DAMAGE,
            rehit_ticks: BLADE_REHIT_TICKS,
        },
        Trap {
            id: CAMP_SPIKES,
            trigger: TrapTrigger::Live,
            effect: TrapEffect::Spikes(TriggerVolume {
                centre: [-1_800, -8_600],
                half: [150, 100],
                floor: SPIKE_FLOOR,
                height: 40,
            }),
            damage: SPIKE_DAMAGE,
            rehit_ticks: SPIKE_REHIT_TICKS,
        },
    ]
}
