//! Outpost overrides over the unchanged deterministic Greyhaven base (TECH-009).
//! Decoration is parsed and validated here too: its solid kinds (lamps and signposts) collide
//! (#577), so the layout is the one definition of where they stand; scenery draws every
//! decoration from it.
use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::dressing::{self, Piece};
use super::{GRAVITY, SPAWN_GRID, SPAWN_PLAZA, base_definition, clear_of_roads, ids, market};
use crate::{StaticCollider, XzBounds, ZoneDefinition, ZoneError};

pub const SOURCE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../content/greyhaven/outpost-layout.json"
));
pub const REGION: XzBounds = XzBounds {
    min: [-3_100, -900],
    max: [3_100, 4_900],
};

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    pub version: u32,
    pub generator: String,
    pub collision: Vec<Placement>,
    pub decoration: Vec<Decoration>,
}

/// Existing houses keep collider IDs 103–106; authored additions use 60000–64999.
/// A tombstone suppresses a base object without changing the generator.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Placement {
    pub id: u32,
    pub position: [i32; 2],
    #[serde(default)]
    pub removed: bool,
}

/// A decoration prefab the layout places.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DecorationKind {
    Lamp,
    Signpost,
    Flowers,
}

impl DecorationKind {
    /// The solid dressing it is, or `None` for walk-through foliage.
    #[must_use]
    pub const fn solid(self) -> Option<dressing::Kind> {
        match self {
            Self::Lamp => Some(dressing::Kind::Lamp),
            Self::Signpost => Some(dressing::Kind::Signpost),
            Self::Flowers => None,
        }
    }
}

/// One decoration, by a persistent ID.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Decoration {
    pub id: String,
    pub kind: DecorationKind,
    pub position: [i32; 2],
    pub yaw: u16,
}

impl Layout {
    /// The solid decorations as dressing, with collider IDs in layout order per kind. Their yaw
    /// rounds to the nearest quarter turn, so each box fits its body.
    #[must_use]
    pub fn dressing(&self) -> Vec<(&Decoration, Piece)> {
        let mut lamps = ids::LAYOUT_LAMPS;
        let mut signposts = ids::LAYOUT_SIGNPOSTS;
        self.decoration
            .iter()
            .filter_map(|decoration| {
                let kind = decoration.kind.solid()?;
                let id = match kind {
                    dressing::Kind::Lamp => lamps.next(),
                    _ => signposts.next(),
                }?;
                let eighth = u32::from(crate::trig::YAW_EIGHTH_TURN);
                let quarter = u32::from(crate::trig::YAW_QUARTER_TURN);
                let quarters =
                    u8::try_from((u32::from(decoration.yaw) + eighth) / quarter % 4).unwrap_or(0);
                Some((
                    decoration,
                    Piece {
                        id,
                        kind,
                        at: decoration.position,
                        quarters,
                        scale_permille: 1_000,
                    },
                ))
            })
            .collect()
    }
}

pub fn parse(source: &str) -> Result<Layout, ZoneError> {
    let layout: Layout =
        serde_json::from_str(source).map_err(|_| ZoneError::new("invalid layout JSON"))?;
    if layout.version != 1 || layout.generator != "greyhaven-15" {
        return Err(ZoneError::new(
            "layout generator/version conflict; explicitly rebase the overrides",
        ));
    }
    let mut seen = BTreeSet::new();
    for decoration in &layout.decoration {
        if decoration.id.is_empty()
            || decoration.id.len() > 80
            || !seen.insert(decoration.id.as_str())
            || !decoration
                .id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err(ZoneError::new(
                "decoration needs a unique persistent alphanumeric/hyphen ID",
            ));
        }
        if !REGION.contains(decoration.position[0], decoration.position[1]) {
            return Err(ZoneError::new(
                "decoration must stay inside the Outpost editing region",
            ));
        }
    }
    let solid = layout
        .decoration
        .iter()
        .filter(|decoration| decoration.kind.solid().is_some())
        .count();
    if layout.dressing().len() != solid {
        return Err(ZoneError::new("too many solid decorations"));
    }
    Ok(layout)
}

/// Whether the closed XZ rectangle `[min, max]` overlaps a collider's footprint.
fn overlaps(min: [i32; 2], max: [i32; 2], collider: &StaticCollider) -> bool {
    let [x, _, z] = collider.position;
    let [hx, _, hz] = collider.half_extents;
    min[0] < x + hx && x - hx < max[0] && min[1] < z + hz && z - hz < max[1]
}

pub fn compile(source: &str) -> Result<ZoneDefinition, ZoneError> {
    let layout = parse(source)?;
    let base = base_definition();
    let mut colliders = base.colliders().to_vec();
    let mut seen = BTreeSet::new();
    for edit in &layout.collision {
        if !seen.insert(edit.id)
            || !((103..=106).contains(&edit.id) || (60_000..65_000).contains(&edit.id))
        {
            return Err(ZoneError::new("duplicate or non-editable layout object ID"));
        }
        if (103..=106).contains(&edit.id)
            && !base
                .colliders()
                .iter()
                .any(|collider| collider.id == edit.id)
        {
            return Err(ZoneError::new("layout source ID conflict; rebase required"));
        }
        colliders.retain(|collider| collider.id != edit.id);
        if !edit.removed {
            // Reuse an existing 8 × 6 m house definition, including its exact collision footprint
            // (house 104: house 103 is Elden's walk-in home, #158).
            let template = base
                .colliders()
                .iter()
                .find(|collider| collider.id == 104)
                .ok_or_else(|| ZoneError::new("layout house source conflict"))?;
            let collider = StaticCollider {
                id: edit.id,
                position: [edit.position[0], template.position[1], edit.position[1]],
                half_extents: template.half_extents,
            };
            let [x, z] = edit.position;
            let [hx, _, hz] = collider.half_extents;
            let bounds = XzBounds {
                min: [x.saturating_sub(hx), z.saturating_sub(hz)],
                max: [x.saturating_add(hx), z.saturating_add(hz)],
            };
            if !REGION.contains(bounds.min[0], bounds.min[1])
                || !REGION.contains(bounds.max[0], bounds.max[1])
                || bounds.overlaps(SPAWN_PLAZA)
                || !clear_of_roads(&collider)
            {
                return Err(ZoneError::new(
                    "house must stay inside the Outpost, clear of roads and spawn plaza",
                ));
            }
            colliders.push(collider);
        }
    }
    // The market square follows house 106 and goes with it (#577).
    colliders.retain(|collider| !ids::MARKET.contains(&collider.id));
    let mut edited: Vec<u32> = layout.collision.iter().map(|edit| edit.id).collect();
    if let Some(house) = colliders
        .iter()
        .find(|collider| collider.id == market::HOUSE)
        .cloned()
    {
        colliders.extend(market::colliders(&house, *ids::MARKET.start()));
        edited.extend(ids::MARKET);
    }
    for id in edited {
        if let Some(left) = colliders.iter().find(|collider| collider.id == id)
            && colliders.iter().any(|right| {
                right.id != left.id
                    && !(ids::MARKET.contains(&left.id) && ids::MARKET.contains(&right.id))
                    && (0..3).all(|axis| {
                        (i64::from(left.position[axis]) - i64::from(right.position[axis])).abs()
                            < i64::from(left.half_extents[axis])
                                + i64::from(right.half_extents[axis])
                    })
            })
        {
            return Err(ZoneError::new("layout house overlaps existing collision"));
        }
    }
    // Every decoration keeps off the structures, the roads and the spawn plaza; the solid ones
    // then collide where they stand.
    let structures = colliders.clone();
    let dressing = layout.dressing();
    for decoration in &layout.decoration {
        let half = decoration
            .kind
            .solid()
            .map_or([18, 12, 18], dressing::Kind::half_extents);
        let [x, z] = decoration.position;
        let (min, max) = ([x - half[0], z - half[2]], [x + half[0], z + half[2]]);
        let plaza = XzBounds {
            min: SPAWN_PLAZA.min,
            max: SPAWN_PLAZA.max,
        };
        let marker = StaticCollider {
            id: 0,
            position: [x, half[1], z],
            half_extents: half,
        };
        if plaza.overlaps(XzBounds { min, max })
            || !clear_of_roads(&marker)
            || structures.iter().any(|collider| {
                collider.id != super::ids::GROUND
                    && !super::ids::BOUNDARY_WALLS.contains(&collider.id)
                    && overlaps(min, max, collider)
            })
        {
            return Err(ZoneError::new(
                "decoration overlaps a structure, road or spawn plaza",
            ));
        }
    }
    colliders.extend(dressing.iter().map(|(_, piece)| piece.collider()));
    let definition = ZoneDefinition::with_spawn_grid(GRAVITY, SPAWN_GRID, colliders)?;
    match base.terrain() {
        Some(terrain) => definition.with_terrain(terrain.clone()),
        None => Ok(definition),
    }
}
