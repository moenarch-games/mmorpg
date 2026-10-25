//! Outpost overrides over the unchanged deterministic Greyhaven base (TECH-009).
//! Decoration is deliberately ignored here: its schema and application belong to scenery.
use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::{GRAVITY, SPAWN_GRID, SPAWN_PLAZA, base_definition, clear_of_roads};
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
pub struct Layout {
    pub version: u32,
    pub generator: String,
    pub collision: Vec<Placement>,
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

pub fn parse(source: &str) -> Result<Layout, ZoneError> {
    let layout: Layout =
        serde_json::from_str(source).map_err(|_| ZoneError::new("invalid layout JSON"))?;
    if layout.version != 1 || layout.generator != "greyhaven-15" {
        return Err(ZoneError::new(
            "layout generator/version conflict; explicitly rebase the overrides",
        ));
    }
    Ok(layout)
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
            // Reuse the existing house definition, including its exact collision footprint.
            let template = base
                .colliders()
                .iter()
                .find(|collider| collider.id == 103)
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
    for edit in &layout.collision {
        if let Some(left) = colliders.iter().find(|collider| collider.id == edit.id)
            && colliders.iter().any(|right| {
                right.id != left.id
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
    let definition = ZoneDefinition::with_spawn_grid(GRAVITY, SPAWN_GRID, colliders)?;
    match base.terrain() {
        Some(terrain) => definition.with_terrain(terrain.clone()),
        None => Ok(definition),
    }
}
