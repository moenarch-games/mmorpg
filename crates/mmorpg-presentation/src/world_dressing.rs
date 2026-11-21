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
//! Adding a placed asset is a data change: the kit that owns its key ([`KITS`]: the crag kit for
//! `crag.kit.*`, the shrine kit for `shrine.kit.*`) draws it with its levels of detail.
//! Outpost modules also carry cosmetic wear and may name an existing `fallbackCollider`
//! shared by their composition; this does not generate or mutate collision.
//! While that kit is not drawn the collider's own surface is the visible stand-in
//! ([`stand_in`]).
use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use mmorpg_core::UNITS_PER_METRE;
use mmorpg_scenery::Scenery;
use serde::{Deserialize, Serialize};

use crate::{
    Cull, Kit,
    batching::matrix,
    geometry::{Affine, compose, ry, scale, translate},
    lod::Placement,
};

/// Greyhaven Vale's dressing files.
const GREYHAVEN: [&str; 14] = [
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../content/greyhaven/dressing/vale.json"
    )),
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../content/greyhaven/dressing/sunken-shrine.json"
    )),
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../content/greyhaven/dressing/outpost.json"
    )),
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../content/greyhaven/dressing/market.json"
    )),
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../content/greyhaven/dressing/southmeadow.json"
    )),
    // The region's footbridges (#828).
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../content/greyhaven/dressing/crossings.json"
    )),
    // The region's settlement houses in the Outpost kit (#818).
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../content/greyhaven/dressing/settlements.json"
    )),
    // The nature bundle's trees, bushes and rocks (#49), composed offline by
    // `examples/export-nature-dressing.rs`.
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../content/greyhaven/dressing/nature.json"
    )),
    // The remaining procedural barrels, crates, carts, signposts, lamps, waystones and fences
    // as settlement props over their colliders, composed offline by
    // `examples/export-prop-dressing.rs`.
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../content/greyhaven/dressing/props.json"
    )),
    // The Wolfrun Den's mouth, bedding and bones (#963).
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../content/greyhaven/dressing/wolfrun-den.json"
    )),
    // The abandoned supply cart on the Wolfrun Trail, its cut sacks, the bait that drew the pack
    // and a red-ragged Redbrand marker pole (#984).
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../content/greyhaven/dressing/wolfrun-cart.json"
    )),
    // The Eastdown meadow's cairn and rock scar (#872).
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../content/greyhaven/dressing/eastdown.json"
    )),
    // The basin's ore veins and the smith's bench (#917).
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../content/greyhaven/dressing/gatherables.json"
    )),
    // The optional discoveries' hunter's cache, Redbrand lookout and fen supply cache (#913).
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../content/greyhaven/dressing/discoveries.json"
    )),
];

/// Every kit that draws dressing, by its asset keys' prefix, with the static-scene fallback
/// ([`Kit`]) it hides while drawn, if it has one.
pub const KITS: [(&str, Option<Kit>); 10] = [
    ("greyhaven.nature.", Some(Kit::Nature)),
    ("limestone-rocks.kit.", Some(Kit::Limestone)),
    ("crag.kit.", Some(Kit::Crag)),
    ("settlement.props.", Some(Kit::SettlementProps)),
    ("outpost.kit.", Some(Kit::Outpost)),
    ("greyhaven.fort.", Some(Kit::Outpost)),
    // The Sunken Shrine's modules (#431) stand on their own colliders and need no stand-in.
    ("shrine.kit.", None),
    ("greyhaven-meadow.kit.", None),
    ("river-stones.kit.", None),
    // The basin's ore veins (#917) stand on their own generated colliders.
    ("gatherables.kit.", None),
];

/// The gatherables kit's placed entries (asset-tooling#763): an ore vein and its spent look,
/// drawn through [`kit_placements`] and swapped by [`vein_slots`].
pub const GATHERABLE_ENTRIES: [&str; 2] = [
    "gatherables.kit.ore-vein",
    "gatherables.kit.ore-vein-depleted",
];

/// The ordinary placed kits (#683): every kit with no specialization of its own, whose entries
/// stand at their authored placements ([`kit_placements`]) or take their share of the zone's
/// mixed inventory ([`MixedPlacements`]), optionally hiding a static-scene stand-in once drawn. `placed-kits.json` registers them once; the native client's build, the WASM host
/// and the browser all read this one inventory, so adding such a kit is a data change plus
/// fetching its bundle.
const PLACED_KITS: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/placed-kits.json"));

/// One ordinary placed kit from `placed-kits.json`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlacedKit {
    /// The kit's name, as the clients ask for it (`"den"`).
    pub name: &'static str,
    /// The bundle's directory under `web/public` (`scenery/wolf-den.kit`); its last component,
    /// followed by `.`, prefixes the kit's asset keys.
    pub bundle: &'static str,
    /// The bundle's descriptor attachment, which declares the levels of detail.
    pub descriptor: &'static str,
    /// What the kit draws, for readers of the inventory.
    pub about: &'static str,
    /// Where the kit's placements come from.
    #[serde(default)]
    pub placement: KitPlacement,
    /// The static-scene fallback the kit hides once drawn (#1011), if it has one; the same
    /// [`Kit`] its entries' prefix names in [`KITS`].
    #[serde(default, rename = "standIn")]
    pub stand_in: Option<Kit>,
    /// The kit's placed entries.
    pub entries: Vec<&'static str>,
}

/// Where an ordinary placed kit's placements come from.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum KitPlacement {
    /// Its entries' authored dressing placements ([`kit_placements`]).
    #[default]
    Authored,
    /// Its entries' share of the zone's mixed inventory ([`MixedPlacements::of`]), which is
    /// derived once for every kit that draws from it (#1010).
    Mixed,
}

impl PlacedKit {
    /// The bundle's directory name (`wolf-den.kit`), its asset keys' prefix before the `.`.
    #[must_use]
    pub fn label(&self) -> &'static str {
        self.bundle.rsplit('/').next().unwrap_or(self.bundle)
    }

    /// Whether `asset` is one of this kit's keys.
    #[must_use]
    pub fn owns(&self, asset: &str) -> bool {
        asset
            .strip_prefix(self.label())
            .is_some_and(|rest| rest.starts_with('.'))
    }

    /// The kit's placements on `s`, whose mixed inventory is `mixed` ([`MixedPlacements::new`]).
    #[must_use]
    pub fn placements(&'static self, s: &Scenery, mixed: &MixedPlacements) -> Vec<Placement> {
        match self.placement {
            KitPlacement::Authored => kit_placements(s, &self.entries),
            KitPlacement::Mixed => mixed.of(&self.entries),
        }
    }
}

/// Every ordinary placed kit, in inventory order.
#[must_use]
pub fn placed_kits() -> &'static [PlacedKit] {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields, bound(deserialize = "'de: 'static"))]
    struct Inventory {
        kits: Vec<PlacedKit>,
    }
    static INVENTORY: LazyLock<Vec<PlacedKit>> = LazyLock::new(|| {
        serde_json::from_str::<Inventory>(PLACED_KITS)
            .expect("the placed-kit inventory is valid")
            .kits
    });
    &INVENTORY
}

/// The ordinary placed kit called `name`.
#[must_use]
pub fn placed_kit(name: &str) -> Option<&'static PlacedKit> {
    placed_kits().iter().find(|kit| kit.name == name)
}

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
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collider: Option<u32>,
    /// Existing gameplay body hidden by this art, without generating or changing collision.
    /// Multiple composed modules may share one body. Presentation only.
    #[serde(
        default,
        rename = "fallbackCollider",
        skip_serializing_if = "Option::is_none"
    )]
    pub fallback_collider: Option<u32>,
    /// Cosmetic wear in 255ths, independent from the collider identity.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub wear: u8,
    /// Recipe scatter rooted on the existing floor, drawn only within 60 m.
    #[serde(
        default,
        rename = "groundDetail",
        skip_serializing_if = "std::ops::Not::not"
    )]
    pub ground_detail: bool,
    /// The distance class beyond which the placement is hidden; [`Cull::Far`] when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cull: Option<Cull>,
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if"
)]
fn is_zero(wear: &u8) -> bool {
    *wear == 0
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

/// The fallback [`Kit`] of the kit that draws `asset`, by its key's prefix.
#[must_use]
pub fn kit(asset: &str) -> Option<Kit> {
    KITS.iter()
        .find(|(prefix, _)| asset.starts_with(prefix))
        .and_then(|(_, kit)| *kit)
}

/// Whether a kit in [`KITS`] or an ordinary placed kit ([`placed_kits`]) draws `asset`.
#[must_use]
pub fn drawn(asset: &str) -> bool {
    KITS.iter().any(|(prefix, _)| asset.starts_with(prefix))
        || placed_kits().iter().any(|kit| kit.owns(asset))
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
        if !drawn(&placed.asset) {
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

/// Every committed placement, whichever scenery shows it.
#[must_use]
pub fn committed() -> &'static [Placed] {
    &GREYHAVEN_PLACED
}

/// The committed placements whose asset key starts with `prefix`, in file order.
pub fn with_prefix(prefix: &str) -> impl Iterator<Item = &'static Placed> {
    committed()
        .iter()
        .filter(move |placed| placed.asset.starts_with(prefix))
}

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
    placements_of(s, &[kit])
}

/// The placements any of `kits` draws on `s`, in file order.
#[must_use]
pub fn placements_of(s: &Scenery, kits: &[Kit]) -> Vec<Placement> {
    placed(s)
        .iter()
        .filter(|placed| placed.kit().is_some_and(|kit| kits.contains(&kit)))
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
                cull: placed.cull.unwrap_or(Cull::Far),
            }
        })
        .collect()
}

/// Cosmetic ground dressing belonging to the supplied bundle entries. No gameplay body is created.
#[must_use]
pub fn ground_details(s: &Scenery, entries: &'static [&'static str]) -> Vec<Placement> {
    let grid = s.terrain_grid(crate::TERRAIN_STEP_UNITS);
    placed(s)
        .iter()
        .filter(|p| p.ground_detail && entries.contains(&p.asset.as_str()))
        .filter(|p| {
            let at = [p.position[0], p.position[2]];
            s.road_distance(at) > 250
                && s.water_distance(at) > 250
                && s.farmland().iter().all(|field| field.distance(at) > 200)
        })
        .map(|p| {
            let mut frame = p.frame();
            frame.t[1] += crate::terrain::surface(&grid, frame.t[0], frame.t[2]);
            Placement {
                entry: entries
                    .iter()
                    .copied()
                    .find(|entry| *entry == p.asset)
                    .expect("filtered bundle entry"),
                model: matrix(frame),
                scale: p
                    .scale
                    .into_iter()
                    .map(|v| f64::from(v) / 1000.)
                    .fold(0., f64::max),
                cull: Cull::Scatter,
            }
        })
        .collect()
}

/// The placements of the supplied bundle `entries` on `s` that stand at their authored height,
/// not as ground detail: every ordinary placed kit's ([`PlacedKit::placements`]) and the ore veins'.
#[must_use]
pub fn kit_placements(s: &Scenery, entries: &'static [&'static str]) -> Vec<Placement> {
    placed(s)
        .iter()
        .filter(|p| !p.ground_detail)
        .filter_map(|p| {
            let entry = entries.iter().copied().find(|entry| *entry == p.asset)?;
            Some(Placement {
                entry,
                model: matrix(p.frame()),
                scale: p
                    .scale
                    .into_iter()
                    .map(|permille| f64::from(permille) / 1_000.)
                    .fold(0., f64::max),
                cull: p.cull.unwrap_or(Cull::Far),
            })
        })
        .collect()
}

/// A zone's mixed placement inventory (#1010): the nature bundle's trees, bushes and limestone
/// rocks, the stones along the watercourses, then the meadow's and the river's ground details,
/// derived once. The nature, limestone, river and meadow kits each draw the placements of
/// their own entries from it ([`Self::of`]); the native client draws the parts in its own
/// library order.
#[derive(Clone, Debug, PartialEq)]
pub struct MixedPlacements {
    /// [`crate::nature::placements`].
    pub nature: Vec<Placement>,
    /// [`crate::river::placements`].
    pub river: Vec<Placement>,
    /// [`ground_details`] of the meadow's, then the river's entries.
    pub ground_details: Vec<Placement>,
}

impl MixedPlacements {
    /// Derives `s`'s mixed inventory.
    #[must_use]
    pub fn new(s: &Scenery) -> Self {
        let mut ground = ground_details(s, &crate::terrain_patch::MEADOW_ENTRIES);
        ground.extend(ground_details(s, &crate::river::ENTRIES));
        Self {
            nature: crate::nature::placements(s),
            river: crate::river::placements(s),
            ground_details: ground,
        }
    }

    /// Every placement, in inventory order.
    pub fn iter(&self) -> impl Iterator<Item = &Placement> {
        self.nature
            .iter()
            .chain(&self.river)
            .chain(&self.ground_details)
    }

    /// The placements of `entries`, in inventory order.
    #[must_use]
    pub fn of(&self, entries: &[&str]) -> Vec<Placement> {
        self.iter()
            .filter(|placement| entries.contains(&placement.entry))
            .copied()
            .collect()
    }
}
/// For each of the zone's ore veins (#917) in wire order, the indices of its full and its spent
/// look in [`kit_placements`] of [`GATHERABLE_ENTRIES`] on `s`: the placements standing at the
/// vein's centre. A client hides one of the two by the projection's spent veins. Empty off
/// Greyhaven.
#[must_use]
pub fn vein_slots(s: &Scenery) -> Vec<[usize; 2]> {
    if !s.greyhaven() {
        return Vec::new();
    }
    let drawn: Vec<&Placed> = placed(s)
        .iter()
        .filter(|p| !p.ground_detail && GATHERABLE_ENTRIES.contains(&p.asset.as_str()))
        .collect();
    let slot = |at: [i32; 2], entry: &str| {
        drawn
            .iter()
            .position(|p| p.asset == entry && [p.position[0], p.position[2]] == at)
            .expect("every ore vein has its full and spent placements")
    };
    mmorpg_core::greyhaven_vale::ore::VEINS
        .iter()
        .map(|&at| {
            [
                slot(at, GATHERABLE_ENTRIES[0]),
                slot(at, GATHERABLE_ENTRIES[1]),
            ]
        })
        .collect()
}

/// Whether collider `id` on `s` is generated from a placement whose kit has no stand-in
/// ([`KITS`] or [`placed_kits`]): its art is always what shows, so the collider's own surface is never drawn.
#[must_use]
pub fn own_collider(s: &Scenery, id: u32) -> bool {
    static OWN: LazyLock<BTreeSet<u32>> = LazyLock::new(|| {
        committed()
            .iter()
            .filter(|placed| placed.kit().is_none())
            .filter_map(|placed| placed.collider)
            .collect()
    });
    // Indexed from the committed placements like [`stand_in`], not a hand-kept ID list.
    s.greyhaven() && OWN.get(&id).is_some()
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
    // Indexed once: tens of thousands of placements, looked up for every collider.
    static STAND_INS: LazyLock<BTreeMap<u32, Option<Kit>>> = LazyLock::new(|| {
        let mut kits = BTreeMap::new();
        for placed in committed() {
            for collider in [placed.collider, placed.fallback_collider]
                .into_iter()
                .flatten()
            {
                kits.entry(collider).or_insert_with(|| placed.kit());
            }
        }
        kits
    });
    if !s.greyhaven() {
        return None;
    }
    STAND_INS.get(&id).copied().flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each kit's share of the shared mixed inventory (#1010) is exactly what deriving the whole
    /// inventory per kit gave: membership, order, transforms and culling, on Greyhaven and on a
    /// zone without dressing.
    #[test]
    fn mixed_placements_give_each_kit_its_former_placements() {
        use crate::{nature, river, terrain_patch::MEADOW_ENTRIES};
        for s in [
            mmorpg_scenery::greyhaven_vale_scenery(),
            mmorpg_scenery::scenery_for_zone(mmorpg_core::seam_test::WEST_ZONE),
        ] {
            // The former per-kit derivation.
            let mut all = nature::placements(&s);
            all.extend(river::placements(&s));
            all.extend(ground_details(&s, &MEADOW_ENTRIES));
            all.extend(ground_details(&s, &river::ENTRIES));
            let mixed = MixedPlacements::new(&s);
            assert_eq!(mixed.iter().copied().collect::<Vec<_>>(), all);
            for entries in [
                &nature::ENTRIES[..],
                &nature::LIMESTONE_ENTRIES[..],
                &river::ENTRIES[..],
                &MEADOW_ENTRIES[..],
            ] {
                let before: Vec<Placement> = all
                    .iter()
                    .filter(|placement| entries.contains(&placement.entry))
                    .copied()
                    .collect();
                assert_eq!(mixed.of(entries), before, "{entries:?}");
            }
            if s.greyhaven() {
                assert!(!mixed.nature.is_empty() && !mixed.ground_details.is_empty());
                assert!(!mixed.of(&MEADOW_ENTRIES).is_empty());
            } else {
                assert!(mixed.ground_details.is_empty());
            }
        }
    }

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
    fn ordinary_placed_kits_own_their_entries_and_are_placed() {
        use crate::{nature, river, terrain_patch::MEADOW_ENTRIES};
        let s = mmorpg_scenery::greyhaven_vale_scenery();
        let mixed = MixedPlacements::new(&s);
        let kits = placed_kits();
        assert_eq!(
            kits.iter().map(|kit| kit.name).collect::<Vec<_>>(),
            ["limestone", "river", "meadow", "den", "discoveries"]
        );
        for kit in kits {
            assert!(
                kit.entries.iter().all(|entry| kit.owns(entry)),
                "{}",
                kit.name
            );
            assert!(
                kit.entries
                    .iter()
                    .all(|entry| drawn(entry) && self::kit(entry) == kit.stand_in),
                "{}",
                kit.name
            );
            assert!(
                !kit.placements(&s, &mixed).is_empty(),
                "{} is placed",
                kit.name
            );
        }
        // The mixed kits draw exactly the entries their derivation places, and with the nature
        // kit they draw the whole mixed inventory.
        let entries = |name| &placed_kit(name).unwrap().entries[..];
        assert_eq!(entries("limestone"), nature::LIMESTONE_ENTRIES);
        assert_eq!(entries("river"), river::ENTRIES);
        assert_eq!(entries("meadow"), MEADOW_ENTRIES);
        let drawn: usize = kits
            .iter()
            .filter(|kit| kit.placement == KitPlacement::Mixed)
            .map(|kit| kit.placements(&s, &mixed).len())
            .sum();
        assert_eq!(
            drawn + mixed.of(&nature::ENTRIES).len(),
            mixed.iter().count()
        );
    }

    #[test]
    fn invalid_dressing_is_refused() {
        let one = |body: &str| format!(r#"{{"version": 1, "area": "a", "placements": [{body}]}}"#);
        let rock = r#"{"id": "a", "asset": "crag.kit.outcrop-spire", "position": [0, 0, 0], "yaw": 0, "scale": [1000, 1000, 1000]}"#;
        assert!(parse(&[&one(rock)]).is_ok());
        assert!(parse(&[&one(&format!("{rock}, {rock}"))]).is_err());
        assert!(
            parse(&[&one(
                &rock.replace("crag.kit.outcrop-spire", "shrine.kit.altar")
            )])
            .is_ok()
        );
        assert!(parse(&[&one(&rock.replace("crag.kit.", "nowhere."))]).is_err());
        assert!(parse(&[&one(&rock.replace("[1000, 1000, 1000]", "[0, 1000, 1000]"))]).is_err());
        assert!(parse(&[&one(rock).replace("\"version\": 1", "\"version\": 2")]).is_err());
    }
}
