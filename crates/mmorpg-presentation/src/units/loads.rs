//! Loads set down on the ground (#1077): a crate, sack, firewood, hay, basket or water jug a
//! player put down where the projection says. Procedural stand-ins drawn the same in both
//! clients, as the breakable props are, until the clients place the `load.*` items themselves;
//! a carried load rides the figure's chest socket instead ([`super::chores::Chores::load`]).
//! Presentation only: the zone owns every load.
use mmorpg_core::{LoadKind, LoadView};

use super::kit::{Frame, Primitive, Resources, child, qa};
use crate::Draw;

const WOOD: u32 = 0x8a6a45;
const DARK_WOOD: u32 = 0x5e4630;
const BURLAP: u32 = 0xb59a6a;
const STRAW: u32 = 0xd6bf6a;
const WICKER: u32 = 0xa07a45;
const CLAY: u32 = 0xa8603c;

/// `load`'s pieces standing on `base` (render metres), turned to its facing.
pub fn set_down(
    resources: &mut Resources,
    index: usize,
    load: &LoadView,
    base: [f32; 3],
) -> Vec<Draw> {
    let id = format!("load-{index}");
    let turn = f64::from(load.facing) / 65_536. * std::f64::consts::TAU;
    let root = Frame {
        p: base.map(f64::from),
        q: qa(1, turn),
    };
    // Each piece: its shape, colour and centre above the base.
    let pieces: &[(Primitive, u32, [f64; 3])] = match load.kind {
        LoadKind::Crate => &[
            (Primitive::Box([0.5, 0.45, 0.5]), WOOD, [0., 0.225, 0.]),
            (
                Primitive::Box([0.52, 0.06, 0.52]),
                DARK_WOOD,
                [0., 0.42, 0.],
            ),
        ],
        LoadKind::Sack => &[
            (Primitive::Post(0.22, 0.4), BURLAP, [0., 0.2, 0.]),
            (Primitive::Ball(0.2), BURLAP, [0., 0.42, 0.]),
        ],
        LoadKind::Firewood => &[
            (Primitive::Box([0.6, 0.12, 0.14]), WOOD, [0., 0.06, -0.08]),
            (
                Primitive::Box([0.6, 0.12, 0.14]),
                DARK_WOOD,
                [0., 0.06, 0.08],
            ),
            (Primitive::Box([0.6, 0.12, 0.14]), WOOD, [0., 0.18, 0.]),
        ],
        LoadKind::Hay => &[(Primitive::Box([0.6, 0.4, 0.4]), STRAW, [0., 0.2, 0.])],
        LoadKind::Basket => &[(Primitive::Post(0.25, 0.3), WICKER, [0., 0.15, 0.])],
        LoadKind::WaterJug => &[
            (Primitive::Post(0.16, 0.36), CLAY, [0., 0.18, 0.]),
            (Primitive::Post(0.07, 0.12), CLAY, [0., 0.42, 0.]),
        ],
    };
    pieces
        .iter()
        .enumerate()
        .map(|(piece, &(shape, color, centre))| {
            resources.primitive(
                format!("{id}-{piece}"),
                shape,
                color,
                child(root, centre, qa(1, 0.)),
            )
        })
        .collect()
}
