//! Breakable props and their debris (#288): procedural crates, barrels, powder barrels and fence
//! sections standing where the zone says, and the pieces the zone throws when one breaks. The
//! shapes match the zone's piece table (`mmorpg_core::debris_pieces`), so a broken crate's
//! panels are the panels it was built from. Placeholder shapes until the clients load
//! asset-tooling's `greyhaven-starter` debris pieces.
use mmorpg_core::{DebrisView, PropKind, debris_pieces};

use super::kit::{Frame, Primitive, Resources, child, qa};
use crate::Draw;

const WOOD: u32 = 0x8a6a45;
const DARK_WOOD: u32 = 0x5e4630;
const IRON: u32 = 0x4b4d52;
const POWDER: u32 = 0x9c3324;

fn metres(units: [i32; 3]) -> [f64; 3] {
    units.map(|axis| f64::from(axis) / 100.)
}

/// The colour of `kind`'s piece `index`.
fn piece_color(kind: PropKind, index: usize) -> u32 {
    match kind {
        // A movable crate (#58) is darker banded timber.
        PropKind::HeavyCrate => {
            if index < 2 {
                IRON
            } else {
                DARK_WOOD
            }
        }
        PropKind::Crate => {
            if index < 2 {
                DARK_WOOD
            } else {
                WOOD
            }
        }
        PropKind::Barrel => {
            if index < 6 {
                WOOD
            } else {
                DARK_WOOD
            }
        }
        PropKind::ExplosiveBarrel => {
            if index < 6 {
                POWDER
            } else {
                DARK_WOOD
            }
        }
        PropKind::Fence => {
            if index < 2 {
                DARK_WOOD
            } else {
                WOOD
            }
        }
    }
}

/// A standing prop: its pieces assembled about its body centre, `half_height` above the feet
/// `root` (turned by its yaw), plus iron hoops on barrels.
pub fn standing(
    resources: &mut Resources,
    id: &str,
    root: Frame,
    kind: PropKind,
    half_height: f64,
) -> Vec<Draw> {
    let centre = child(root, [0., half_height, 0.], qa(1, 0.));
    let mut draws: Vec<Draw> = debris_pieces(kind)
        .iter()
        .enumerate()
        .map(|(index, piece)| {
            let turn = f64::from(piece.yaw) / 65_536. * std::f64::consts::TAU;
            resources.primitive(
                format!("{id}-piece-{index}"),
                Primitive::Box(metres(piece.half_extents).map(|half| half * 2.)),
                piece_color(kind, index),
                child(centre, metres(piece.offset), qa(1, turn)),
            )
        })
        .collect();
    if matches!(kind, PropKind::Barrel | PropKind::ExplosiveBarrel) {
        for (index, y) in [-0.3, 0.3].into_iter().enumerate() {
            draws.push(resources.primitive(
                format!("{id}-hoop-{index}"),
                Primitive::Post(0.31, 0.05),
                IRON,
                child(centre, [0., y, 0.], qa(1, 0.)),
            ));
        }
    }
    draws
}

/// One flying or settled piece at its rendered centre, and a fireball over a powder barrel's
/// pieces for the first third of a second.
pub fn piece(resources: &mut Resources, view: &DebrisView, centre: [f32; 3]) -> Vec<Draw> {
    let Some(table) = debris_pieces(view.kind).get(usize::from(view.piece)) else {
        return Vec::new();
    };
    let q = view.rotation.map(|axis| f64::from(axis) / 32_767.);
    let frame = Frame {
        p: centre.map(f64::from),
        q,
    };
    let id = format!("debris-{}", view.id);
    let mut draws = vec![resources.primitive(
        format!("{id}-piece"),
        Primitive::Box(metres(table.half_extents).map(|half| half * 2.)),
        piece_color(view.kind, usize::from(view.piece)),
        frame,
    )];
    if view.kind == PropKind::ExplosiveBarrel && view.age < 10 {
        let mut flash = resources.primitive(
            format!("{id}-flash"),
            Primitive::Ball(0.7),
            0xffa23a,
            Frame {
                p: frame.p,
                q: qa(1, 0.),
            },
        );
        flash.material.emissive = [1.0, 0.55, 0.15];
        flash.material.unlit = true;
        flash.material.opacity = 0.9 * (1. - f32::from(view.age) / 10.);
        draws.push(flash);
    }
    draws
}
