//! World dressing (#683, TECH-008/TECH-009): art placed as data, per area, that both clients
//! draw through the placed-kit path.
//!
//! `content/greyhaven/dressing/*.json` lists placements in the Outpost layout's conventions:
//! a persistent string `id`, the kit `asset` key (for example `crag.kit.outcrop-buttress-1`),
//! the foot `position` in units, the `yaw` (65 536 per turn about +Y, +Z toward +X) and a
//! per-axis `scale` in thousandths. A placement may name the `collider` its art stands over.
//! That collider's mesh is not hand-authored: `content/greyhaven/authoring/dressing_collision.py`
//! generates it from the asset's `collisionMesh` and the placement, and the zone reads only the
//! committed output. Gameplay never reads this file.
//!
//! Adding a placed asset is a data change: the kit that owns its key (the crag kit for
//! `crag.kit.*`) draws it with its levels of detail, and while that kit is not drawn the
//! collider's own surface is the visible stand-in ([`stand_in`]).
use std::collections::BTreeSet;
use std::sync::LazyLock;

use mmorpg_core::UNITS_PER_METRE;
use mmorpg_scenery::Scenery;
use serde::Deserialize;

use crate::{
    Cull, Kit,
    batching::matrix,
    geometry::{Affine, compose, ry, scale, translate},
    lod::Placement,
};

/// Greyhaven Vale's dressing files.
const GREYHAVEN: [&str; 1] = [include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../content/greyhaven/dressing/vale.json"
))];

/// The dressing format's version.
pub const VERSION: u32 = 1;

/// One area's dressing.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dressing {
    pub version: u32,
    pub area: String,
    pub placements: Vec<Placed>,
}

/// One placed asset.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Placed {
    /// Persistent, unique across every area.
    pub id: String,
    /// The kit entry drawn.
    pub asset: String,
    /// The asset's origin (its foot) in units.
    pub position: [i32; 3],
    /// 65 536 per turn about +Y.
    pub yaw: u16,
    /// Per-axis scale in thousandths, before the turn.
    pub scale: [u16; 3],
    /// The collider generated from this placement's art, if it collides.
    #[serde(default)]
    pub collider: Option<u32>,
}

impl Placed {
    /// The asset-to-world transform, metres.
    #[must_use]
    pub fn frame(&self) -> Affine {
        let [x, y, z] = self
            .position
            .map(|units| f64::from(units) / f64::from(UNITS_PER_METRE));
        let [sx, sy, sz] = self.scale.map(|permille| f64::from(permille) / 1_000.);
        compose(&[
            translate(x, y, z),
            ry(f64::from(self.yaw) / 65_536. * std::f64::consts::TAU),
            scale(sx, sy, sz),
        ])
    }

    /// The kit that draws the asset, by its key's prefix.
    #[must_use]
    pub fn kit(&self) -> Option<Kit> {
        kit(&self.asset)
    }
}

/// The kit that draws `asset`, by its key's prefix.
#[must_use]
pub fn kit(asset: &str) -> Option<Kit> {
    asset.starts_with("crag.kit.").then_some(Kit::Crag)
}

/// Parses and validates dressing files: the known version, unique IDs and colliders, a known
/// kit for every asset and non-zero scales.
///
/// # Errors
///
/// For malformed or invalid dressing.
pub fn parse(sources: &[&str]) -> Result<Vec<Placed>, String> {
    let mut placements = Vec::new();
    for source in sources {
        let dressing: Dressing =
            serde_json::from_str(source).map_err(|error| format!("dressing: {error}"))?;
        if dressing.version != VERSION {
            return Err(format!(
                "{}: unknown dressing version {}",
                dressing.area, dressing.version
            ));
        }
        placements.extend(dressing.placements);
    }
    let mut ids = BTreeSet::new();
    let mut colliders = BTreeSet::new();
    for placed in &placements {
        if !ids.insert(placed.id.as_str()) {
            return Err(format!("dressing {} is placed twice", placed.id));
        }
        if placed.collider.is_some_and(|id| !colliders.insert(id)) {
            return Err(format!("{}: another placement has its collider", placed.id));
        }
        if placed.kit().is_none() {
            return Err(format!("{}: no kit draws {}", placed.id, placed.asset));
        }
        if placed.scale.contains(&0) {
            return Err(format!("{}: zero scale", placed.id));
        }
    }
    Ok(placements)
}

static GREYHAVEN_PLACED: LazyLock<Vec<Placed>> =
    LazyLock::new(|| parse(&GREYHAVEN).expect("committed Greyhaven dressing is valid"));

/// Every placement on `s`; only Greyhaven Vale is dressed.
#[must_use]
pub fn placed(s: &Scenery) -> &'static [Placed] {
    if s.greyhaven() {
        &GREYHAVEN_PLACED
    } else {
        &[]
    }
}

/// The placements `kit` draws on `s`, in file order.
#[must_use]
pub fn placements(s: &Scenery, kit: Kit) -> Vec<Placement> {
    placed(s)
        .iter()
        .filter(|placed| placed.kit() == Some(kit))
        .map(|placed| {
            let frame = placed.frame();
            Placement {
                entry: placed.asset.as_str(),
                model: matrix(frame),
                scale: placed
                    .scale
                    .into_iter()
                    .map(|permille| f64::from(permille) / 1_000.)
                    .fold(0., f64::max),
                cull: Cull::Far,
            }
        })
        .collect()
}

/// The distinct assets `kit` draws on `s`, sorted.
#[must_use]
pub fn assets(s: &Scenery, kit: Kit) -> Vec<&'static str> {
    placed(s)
        .iter()
        .filter(|placed| placed.kit() == Some(kit))
        .map(|placed| placed.asset.as_str())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// The kit whose art stands over collider `id` on `s`: the collider's own surface is then
/// only its stand-in while that kit is not drawn.
#[must_use]
pub fn stand_in(s: &Scenery, id: u32) -> Option<Kit> {
    placed(s)
        .iter()
        .find(|placed| placed.collider == Some(id))
        .and_then(Placed::kit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greyhaven_dressing_stands_over_its_generated_collision() {
        let s = mmorpg_scenery::greyhaven_vale_scenery();
        let placed = placed(&s);
        assert!(!placed.is_empty());
        for p in placed {
            let Some(id) = p.collider else { continue };
            let mesh = s
                .collision_surfaces
                .iter()
                .find(|mesh| mesh.id == id)
                .unwrap_or_else(|| panic!("{} has no collider {id}", p.id));
            // The generated mesh sits at the placement.
            assert_eq!(mesh.position, p.position, "{}", p.id);
        }
    }

    #[test]
    fn invalid_dressing_is_refused() {
        let one = |body: &str| format!(r#"{{"version": 1, "area": "a", "placements": [{body}]}}"#);
        let rock = r#"{"id": "a", "asset": "crag.kit.outcrop-spire", "position": [0, 0, 0], "yaw": 0, "scale": [1000, 1000, 1000]}"#;
        assert!(parse(&[&one(rock)]).is_ok());
        assert!(parse(&[&one(&format!("{rock}, {rock}"))]).is_err());
        assert!(parse(&[&one(&rock.replace("crag.kit.", "nowhere."))]).is_err());
        assert!(parse(&[&one(&rock.replace("[1000, 1000, 1000]", "[0, 1000, 1000]"))]).is_err());
        assert!(parse(&[&one(rock).replace("\"version\": 1", "\"version\": 2")]).is_err());
    }
}
