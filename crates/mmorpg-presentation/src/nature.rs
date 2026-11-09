//! Greyhaven's trees, rocks and bushes from asset-tooling's `greyhaven-nature` bundle (#49,
//! #596).
//!
//! Every scenery tree, every bush and every rock that is not climbing rock becomes one placed
//! bundle entry: oaks and birches the broadleaf, pines the conifer, bushes a stable pick of the
//! three bushes, rocks one of the three rocks. A collider rock's entry is the one its zone
//! collider collides as ([`mmorpg_core::RockShape`]), fitted into the collider's box exactly
//! like the proxy, so the drawn rock and its collision agree. Bushes stay walk-through foliage
//! (#577). While a client does not draw the bundle, the static scene draws the procedural
//! trees, rocks and bushes ([`crate::Kit::Nature`] fallback groups).
//!
//! The shared [`Lods`] ([`crate::lod`]) picks each placement's level of detail from the
//! bundle's `lodSelection` metadata and cross-fades every switch. Presentation only.
use mmorpg_core::{Grip, RockShape, UNITS_PER_METRE};
use mmorpg_scenery::{Prop, PropKind, Scenery, TerrainGrid, TreeVariant};

use crate::{
    Cull,
    batching::matrix,
    geometry::{compose, ry, scale, translate},
};

pub const BROADLEAF: &str = "greyhaven.nature.tree.broadleaf";
pub const CONIFER: &str = "greyhaven.nature.tree.conifer";
/// The round, spreading and upright bushes (#596).
pub const BUSHES: [&str; 3] = [
    "greyhaven.nature.bush.1",
    "greyhaven.nature.bush.2",
    "greyhaven.nature.bush.3",
];
/// Each bush's authored footprint (width, height, depth; metres), as the descriptor gives it.
pub const BUSH_SIZES: [[f64; 3]; 3] = [[1.4, 1.0, 1.3], [1.7, 0.85, 1.4], [1.1, 1.3, 1.0]];
/// The `limestone-rocks` block drawn for each rock proxy (asset-tooling#536), with its body's
/// half width, height and half depth in metres. Its origin is on the ground, under the body.
pub const LIMESTONE_ROCKS: [(&str, [f64; 3]); 3] = [
    ("limestone-rocks.kit.block-1", [1.9, 2.2, 1.5]),
    ("limestone-rocks.kit.block-2", [1.4, 1.4, 1.1]),
    ("limestone-rocks.kit.block-3", [0.95, 0.9, 0.8]),
];
/// The entries the limestone kit draws: the rocks' blocks and the brook sink's larger pieces
/// ([`crate::river`]).
pub const LIMESTONE_ENTRIES: [&str; 6] = [
    LIMESTONE_ROCKS[0].0,
    LIMESTONE_ROCKS[1].0,
    LIMESTONE_ROCKS[2].0,
    "limestone-rocks.kit.outcrop-1",
    "limestone-rocks.kit.outcrop-2",
    "limestone-rocks.kit.ledge",
];
/// The bundle's entries, sorted.
pub const ENTRIES: [&str; 8] = [
    BUSHES[0],
    BUSHES[1],
    BUSHES[2],
    "greyhaven.nature.rock.1",
    "greyhaven.nature.rock.2",
    "greyhaven.nature.rock.3",
    BROADLEAF,
    CONIFER,
];

pub use crate::lod::{Lods, Placement, Shown};

fn metres(units: i32) -> f64 {
    f64::from(units) / f64::from(UNITS_PER_METRE)
}

fn yaw(p: &Prop) -> f64 {
    f64::from(p.yaw) / 65_536. * std::f64::consts::TAU
}

/// Whether the bundle draws `p` (the procedural model is its fallback): every tree and bush,
/// and every rock that is not climbing rock, whose worn handholds (#512) stay on the
/// procedural rock.
#[must_use]
pub fn draws(s: &Scenery, p: &Prop) -> bool {
    match p.kind {
        PropKind::Tree(_) | PropKind::Bush => true,
        PropKind::Rock(_) => !p
            .collider
            .and_then(|id| s.climbing().collider(id))
            .is_some_and(|grip| grip == Grip::Rock),
        _ => false,
    }
}

/// The rock `p` is drawn as: its collider's proxy, or for a rock without one, a stable pick.
fn rock_shape(s: &Scenery, p: &Prop) -> RockShape {
    p.collider
        .and_then(|id| s.rock(id))
        .unwrap_or_else(|| RockShape {
            proxy: u8::try_from(
                (u64::from(p.position[0].unsigned_abs()) * 31
                    + u64::from(p.position[2].unsigned_abs()))
                    % 3,
            )
            .unwrap_or(0),
            quarters: 0,
        })
}

/// The bush `p` is drawn as, a stable pick from its position (an index into [`BUSHES`]).
fn bush(p: &Prop) -> usize {
    usize::try_from(
        (u64::from(p.position[0].unsigned_abs()) * 17 + u64::from(p.position[2].unsigned_abs()))
            % 3,
    )
    .unwrap_or(0)
}

/// `p`'s placement on `s` (drawn terrain `g`), when the bundle draws it.
pub(crate) fn placement(s: &Scenery, g: &TerrainGrid, p: &Prop) -> Option<Placement> {
    if !draws(s, p) {
        return None;
    }
    let x = metres(p.position[0]);
    let z = metres(p.position[2]);
    let seed = crate::props::seed(p);
    let (entry, frame, largest, cull) = match p.kind {
        PropKind::Tree(variant) => {
            let background = p.collider.is_none();
            // Fitted to the procedural tree's height: about 7.5 m in the woods.
            let grow = if background {
                f64::from(p.scale_permille) / 1000.
            } else {
                0.85 + seed * 0.4
            };
            let (entry, size) = match variant {
                TreeVariant::Pine => (CONIFER, 0.86 * grow),
                TreeVariant::Oak | TreeVariant::Birch => (BROADLEAF, 0.8 * grow),
            };
            // Mature trees rise above young growth while their trunk footprint stays put.
            // The bundle's leafy crown is foliage; the existing core trunk owns collision.
            let height = size * (1.15 + seed * 0.45);
            let at = compose(&[
                translate(x, crate::terrain::surface(g, x, z), z),
                ry(yaw(p)),
                scale(size, height, size),
            ]);
            let cull = if background { Cull::Always } else { Cull::Far };
            (entry, at, height, cull)
        }
        PropKind::Bush => {
            // A stable pick of the three, scaled uniformly to the procedural bush's volume (its
            // half extents: about 1.2 × 0.9 × 1.2 m at scale 1), so each keeps its own shape.
            let pick = bush(p);
            let [w, h, d] = BUSH_SIZES[pick];
            let [hx, hy, hz] = p.half_extents.map(metres);
            let size = ((2. * hx / w) * (2. * hy / h) * (2. * hz / d)).cbrt();
            // Sunk a little, so a slope shows no gap under the rim.
            let at = compose(&[
                translate(x, crate::terrain::surface(g, x, z) - 0.06 * size, z),
                ry(yaw(p)),
                scale(size, size, size),
            ]);
            (BUSHES[pick], at, size, Cull::Mid)
        }
        PropKind::Rock(_) => {
            let shape = rock_shape(s, p);
            let [hx, hy, hz] = p.half_extents.map(metres);
            let (entry, [body_x, body_height, body_z]) = LIMESTONE_ROCKS[usize::from(shape.proxy)];
            // Fitted to the collider's box, which the drawn rock fills; physics keeps its proxy.
            let fit = [hx / body_x, 2. * hy / body_height, hz / body_z];
            // A collider rock stands on its body's bottom, where its proxy collides.
            let base = if p.collider.is_some() {
                metres(p.position[1])
            } else {
                crate::terrain::surface(g, x, z)
            };
            let at = compose(&[
                translate(x, base, z),
                ry(yaw(p)),
                scale(fit[0], fit[1], fit[2]),
            ]);
            let cull = if p.collider.is_some() {
                Cull::Always
            } else {
                Cull::Mid
            };
            (entry, at, fit[0].max(fit[1]).max(fit[2]), cull)
        }
        _ => return None,
    };
    Some(Placement {
        entry,
        model: matrix(frame),
        scale: largest,
        cull,
    })
}

/// Every placed tree, bush and rock the bundle draws on `s`, in prop order.
#[must_use]
pub fn placements(s: &Scenery) -> Vec<Placement> {
    let g = s.terrain_grid(crate::TERRAIN_STEP_UNITS);
    s.props.iter().filter_map(|p| placement(s, &g, p)).collect()
}

pub use crate::lod::EntryLods;

/// Every entry's levels from the bundle's `nature-descriptor` attachment JSON ([`crate::lod`]).
///
/// # Errors
///
/// For malformed JSON or levels of detail.
pub fn descriptor_lods(json: &str) -> Result<Vec<EntryLods>, String> {
    let value =
        serde_json::from_str(json).map_err(|error| format!("nature descriptor: {error}"))?;
    crate::lod::descriptor_lods(&value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tree_bush_and_unclimbable_rock_is_placed() {
        let scenery = mmorpg_scenery::greyhaven_vale_scenery();
        let placements = placements(&scenery);
        let drawn: Vec<_> = scenery
            .props
            .iter()
            .filter(|p| draws(&scenery, p))
            .collect();
        assert_eq!(placements.len(), drawn.len());
        assert!(drawn.iter().any(|p| matches!(p.kind, PropKind::Rock(_))));
        let mut hulls = 0;
        for (prop, placement) in drawn.iter().zip(&placements) {
            if let Some(shape) = prop.collider.and_then(|id| scenery.rock(id)) {
                assert_eq!(placement.entry, LIMESTONE_ROCKS[usize::from(shape.proxy)].0);
                hulls += 1;
            }
        }
        assert!(
            hulls > 300,
            "the loose rocks collide as their drawn rock: {hulls}"
        );
        // Every bush is one of the three bushes, each picked often, all walk-through.
        let bushes: Vec<_> = drawn
            .iter()
            .zip(&placements)
            .filter(|(prop, _)| prop.kind == PropKind::Bush)
            .collect();
        assert!(bushes.iter().all(|(prop, _)| prop.collider.is_none()));
        for entry in BUSHES {
            let picked = bushes.iter().filter(|(_, p)| p.entry == entry).count();
            assert!(picked * 5 > bushes.len(), "{entry}: {picked}");
        }
    }

    #[test]
    fn the_descriptor_parses_its_lod_levels() {
        let json = r#"{"lodSelection":{"metric":"projected-bounding-sphere-height-fraction"},
          "assets":[{"key":"k","lods":{"boundingSphere":{"center":[0,1,0],"radius":2},
          "levels":[{"variant":"default","minScreenSize":0.3},{"variant":"lod1","minScreenSize":0}]}}]}"#;
        let lods = descriptor_lods(json).unwrap();
        assert_eq!(
            lods[0].levels,
            [("default".into(), 0.3), ("lod1".into(), 0.)]
        );
        assert_eq!(lods[0].radius, 2.);
    }
}
