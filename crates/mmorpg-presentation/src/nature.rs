//! Greyhaven's trees, rocks and bushes from asset-tooling's `greyhaven-nature` bundle (#49,
//! #596).
//!
//! Every scenery tree, every bush and every rock that is not climbing rock becomes one placed
//! bundle entry: oaks and birches the broadleaf, pines the conifer, bushes a stable pick of the
//! three bushes, rocks one of the three rocks. A collider rock's entry is the one its zone
//! collider collides as ([`mmorpg_core::RockShape`]), fitted into the collider's box exactly
//! like the proxy, so the drawn rock and its collision agree. Bushes stay walk-through foliage
//! (#577). While a client does not draw the bundle, the static scene draws the procedural
//! trees and bushes ([`crate::Kit::Nature`] fallback groups) and rocks ([`crate::Kit::Limestone`]).
//!
//! Greyhaven's placements are world dressing (#683): [`compose_all`] composes them offline
//! (`examples/export-nature-dressing.rs`) into `content/greyhaven/dressing/nature.json`, and the
//! clients read that committed data. The colliders stay core's, unchanged.
//!
//! The shared [`Lods`] ([`crate::lod`]) picks each placement's level of detail from the
//! bundle's `lodSelection` metadata and cross-fades every switch. Presentation only.
use mmorpg_core::{Grip, RockShape, UNITS_PER_METRE};
use mmorpg_scenery::{Prop, PropKind, Scenery, TerrainGrid, TreeVariant};

use crate::{
    Cull, Kit,
    batching::matrix,
    geometry::{compose, ry, scale, translate},
    world_dressing,
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

/// One tree, bush or rock the bundle draws, as the offline export composes it (#683): its
/// entry, foot (metres), turn, per-axis scale and distance class.
#[derive(Clone, Debug, PartialEq)]
pub struct Composed {
    /// Persistent dressing ID: `nature:<collider>`, or `nature:<x>:<z>` for a prop without one.
    pub id: String,
    pub entry: &'static str,
    pub at: [f64; 3],
    pub yaw: u16,
    pub scale: [f64; 3],
    pub cull: Cull,
}

impl Composed {
    /// The placement as committed world dressing: units and thousandths.
    #[must_use]
    pub fn placed(&self) -> world_dressing::Placed {
        world_dressing::Placed {
            id: self.id.clone(),
            asset: self.entry.to_owned(),
            position: self
                .at
                .map(|metres| (metres * f64::from(UNITS_PER_METRE)).round() as i32),
            yaw: self.yaw,
            scale: self.scale.map(|s| (s * 1_000.).round() as u16),
            collider: None,
            fallback_collider: None,
            wear: 0,
            ground_detail: false,
            cull: Some(self.cull),
        }
    }

    fn placement(&self) -> Placement {
        let [x, y, z] = self.at;
        let [sx, sy, sz] = self.scale;
        Placement {
            entry: self.entry,
            model: matrix(compose(&[
                translate(x, y, z),
                ry(f64::from(self.yaw) / 65_536. * std::f64::consts::TAU),
                scale(sx, sy, sz),
            ])),
            scale: sx.max(sy).max(sz),
            cull: self.cull,
        }
    }
}

/// `p` on `s` (drawn terrain `g`), when the bundle draws it.
fn composed(s: &Scenery, g: &TerrainGrid, p: &Prop) -> Option<Composed> {
    if !draws(s, p) {
        return None;
    }
    let x = metres(p.position[0]);
    let z = metres(p.position[2]);
    let seed = crate::props::seed(p);
    let (entry, y, size, cull) = match p.kind {
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
            let cull = if background { Cull::Always } else { Cull::Far };
            (
                entry,
                crate::terrain::surface(g, x, z),
                [size, height, size],
                cull,
            )
        }
        PropKind::Bush => {
            // A stable pick of the three, scaled uniformly to the procedural bush's volume (its
            // half extents: about 1.2 × 0.9 × 1.2 m at scale 1), so each keeps its own shape.
            let pick = bush(p);
            let [w, h, d] = BUSH_SIZES[pick];
            let [hx, hy, hz] = p.half_extents.map(metres);
            let size = ((2. * hx / w) * (2. * hy / h) * (2. * hz / d)).cbrt();
            // Sunk a little, so a slope shows no gap under the rim.
            (
                BUSHES[pick],
                crate::terrain::surface(g, x, z) - 0.06 * size,
                [size; 3],
                Cull::Mid,
            )
        }
        PropKind::Rock(_) => {
            let shape = rock_shape(s, p);
            let [hx, hy, hz] = p.half_extents.map(metres);
            let (entry, [body_x, body_height, body_z]) = LIMESTONE_ROCKS[usize::from(shape.proxy)];
            // Fitted to the collider's box, which the drawn rock fills; physics keeps its proxy.
            let fit = [hx / body_x, 2. * hy / body_height, hz / body_z];
            // A collider rock stands on its body's bottom, where its proxy collides.
            if p.collider.is_some() {
                (entry, metres(p.position[1]), fit, Cull::Always)
            } else {
                (entry, crate::terrain::surface(g, x, z), fit, Cull::Mid)
            }
        }
        _ => return None,
    };
    let id = p.collider.map_or_else(
        || format!("nature:{}:{}", p.position[0], p.position[2]),
        |collider| format!("nature:{collider}"),
    );
    Some(Composed {
        id,
        entry,
        at: [x, y, z],
        yaw: p.yaw,
        scale: size,
        cull,
    })
}

/// Every tree, bush and rock the bundle draws on `s`, in prop order, composed from the scenery's
/// props. Offline only (`examples/export-nature-dressing.rs`): the clients read Greyhaven's
/// committed result, `content/greyhaven/dressing/nature.json`, through [`placements`].
#[must_use]
pub fn compose_all(s: &Scenery) -> Vec<Composed> {
    let g = s.terrain_grid(crate::TERRAIN_STEP_UNITS);
    s.props.iter().filter_map(|p| composed(s, &g, p)).collect()
}

/// Every placed tree, bush and rock the bundle draws on `s`: Greyhaven's committed world
/// dressing (#683), in file order. A zone without dressing still composes its props.
#[must_use]
pub fn placements(s: &Scenery) -> Vec<Placement> {
    if s.greyhaven() {
        world_dressing::placements_of(s, &[Kit::Nature, Kit::Limestone])
    } else {
        compose_all(s).iter().map(Composed::placement).collect()
    }
}

pub use crate::lod::EntryLods;

/// Where a tree leaves its `default` and `lod1` levels, as a projected screen size (#615).
///
/// The bundle's descriptor keeps both levels out to 90 m and 300 m, far enough that the woods
/// spend almost 80 % of their triangles on trees, on a frame budget they miss. These switch
/// points replace the descriptor's for the trees alone: at the game's 50° field of view an
/// average tree keeps `default` to about 40 m and `lod1` to about 270 m. `lod2` has no foliage
/// cards, so the second switch stays far enough out that trees on the valley's slopes keep
/// theirs. Bushes and rocks keep the descriptor's distances (asset-tooling#534).
pub const TREE_MIN_SCREEN_SIZES: [f64; 2] = [0.35, 0.05];

/// Every entry's levels from the bundle's `nature-descriptor` attachment JSON ([`crate::lod`]),
/// with the trees' switch points replaced by [`TREE_MIN_SCREEN_SIZES`].
///
/// # Errors
///
/// For malformed JSON or levels of detail.
pub fn descriptor_lods(json: &str) -> Result<Vec<EntryLods>, String> {
    let value =
        serde_json::from_str(json).map_err(|error| format!("nature descriptor: {error}"))?;
    let mut entries = crate::lod::descriptor_lods(&value)?;
    for entry in &mut entries {
        if entry.entry == BROADLEAF || entry.entry == CONIFER {
            for ((_, min), size) in entry.levels.iter_mut().zip(TREE_MIN_SCREEN_SIZES) {
                *min = size;
            }
        }
    }
    Ok(entries)
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
    fn the_committed_dressing_is_the_composed_nature() {
        // Regenerate with `examples/export-nature-dressing.rs` when the scenery's props move.
        let scenery = mmorpg_scenery::greyhaven_vale_scenery();
        let composed = compose_all(&scenery);
        let committed: Vec<_> = world_dressing::committed()
            .iter()
            .filter(|placed| matches!(placed.kit(), Some(Kit::Nature | Kit::Limestone)))
            .collect();
        assert_eq!(committed.len(), composed.len());
        let drawn = placements(&scenery);
        for ((placed, composed), drawn) in committed.iter().zip(&composed).zip(&drawn) {
            assert_eq!(**placed, composed.placed());
            // Only the data's centimetres and thousandths separate the drawn rock or tree from
            // the composed one.
            let exact = composed.placement();
            assert_eq!((drawn.entry, drawn.cull), (exact.entry, exact.cull));
            assert!((drawn.scale - exact.scale).abs() < 1e-3, "{}", placed.id);
            for (a, b) in drawn.model.iter().zip(exact.model) {
                assert!((a - b).abs() < 0.02, "{}", placed.id);
            }
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

    #[test]
    fn only_the_trees_take_the_nearer_switch_points() {
        let json = format!(
            r#"{{"lodSelection":{{"metric":"projected-bounding-sphere-height-fraction"}},"assets":[
              {{"key":"{BROADLEAF}","lods":{{"boundingSphere":{{"center":[0,1,0],"radius":2}},"levels":[
                {{"variant":"default","minScreenSize":0.11}},{{"variant":"lod1","minScreenSize":0.03}},
                {{"variant":"lod2","minScreenSize":0}}]}}}},
              {{"key":"{}","lods":{{"boundingSphere":{{"center":[0,1,0],"radius":1}},"levels":[
                {{"variant":"default","minScreenSize":0.08}},{{"variant":"lod1","minScreenSize":0}}]}}}}]}}"#,
            BUSHES[0]
        );
        let lods = descriptor_lods(&json).unwrap();
        let sizes = |index: usize| -> Vec<f64> { lods[index].levels.iter().map(|l| l.1).collect() };
        assert_eq!(
            sizes(0),
            [TREE_MIN_SCREEN_SIZES[0], TREE_MIN_SCREEN_SIZES[1], 0.]
        );
        assert_eq!(sizes(1), [0.08, 0.]);
    }
}
