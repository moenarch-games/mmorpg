//! Presentation-owned Outpost decoration composed after base generation.
use std::collections::BTreeSet;

use mmorpg_core::{ZoneDefinition, greyhaven_vale};
use serde::{Deserialize, Serialize};

use crate::{Prop, PropKind, Rect, Scenery, base_half_extents, base_scenery};

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Decoration {
    pub id: String,
    pub kind: String,
    pub position: [i32; 2],
    pub yaw: u16,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u32,
    generator: String,
    collision: Vec<greyhaven_vale::layout::Placement>,
    decoration: Vec<Decoration>,
}

pub fn decorations(source: &str) -> Result<Vec<Decoration>, String> {
    let document: Document = serde_json::from_str(source).map_err(|error| error.to_string())?;
    if document.version != 1 || document.generator != "greyhaven-15" {
        return Err("Layout generator/version conflict".into());
    }
    // Core owns validation of these fields. Reading them here never creates collision.
    let _ = document.collision;
    let mut ids = BTreeSet::new();
    for prop in &document.decoration {
        if prop.id.is_empty()
            || prop.id.len() > 80
            || !ids.insert(&prop.id)
            || !prop
                .id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err("Decoration needs a unique persistent alphanumeric/hyphen ID".into());
        }
        kind(&prop.kind)?;
        if !greyhaven_vale::layout::REGION.contains(prop.position[0], prop.position[1]) {
            return Err("Decoration must stay inside the Outpost editing region".into());
        }
    }
    Ok(document.decoration)
}

fn kind(name: &str) -> Result<PropKind, String> {
    match name {
        "lamp" => Ok(PropKind::Lamp),
        "signpost" => Ok(PropKind::Signpost),
        "flowers" => Ok(PropKind::Flowers),
        _ => Err("Unsupported decoration prefab".into()),
    }
}

pub fn compose(source: &str, definition: &ZoneDefinition) -> Result<Scenery, String> {
    let decoration = decorations(source)?;
    let mut scenery = base_scenery();
    // Regeneration always starts from the same base; edits never reroll the scatter RNG.
    scenery
        .props
        .retain(|prop| !prop.collider.is_some_and(|id| (103..=106).contains(&id)));
    for collider in definition.colliders().iter().filter(|collider| {
        (103..=106).contains(&collider.id) || (60_000..65_000).contains(&collider.id)
    }) {
        let house = Prop {
            kind: PropKind::House,
            position: [
                collider.position[0],
                collider.position[1] - collider.half_extents[1],
                collider.position[2],
            ],
            yaw: 0,
            scale_permille: 1_000,
            collider: Some(collider.id),
            half_extents: collider.half_extents,
        };
        // Suppress intersecting cosmetic scatter rather than move or reroll it.
        scenery.props.retain(|prop| {
            prop.collider.is_some() || !prop.footprint().intersects(house.footprint())
        });
        scenery.props.push(house);
    }
    for prop in decoration {
        let kind = kind(&prop.kind)?;
        let half_extents = base_half_extents(kind);
        let placed = Prop {
            kind,
            position: [prop.position[0], 0, prop.position[1]],
            yaw: prop.yaw,
            scale_permille: 1_000,
            collider: None,
            half_extents,
        };
        let bounds = placed.footprint();
        let plaza = Rect {
            min: greyhaven_vale::SPAWN_PLAZA.min,
            max: greyhaven_vale::SPAWN_PLAZA.max,
        };
        if bounds.intersects(plaza)
            || !greyhaven_vale::clear_of_roads(&mmorpg_core::StaticCollider {
                id: 0,
                position: [prop.position[0], half_extents[1].max(1), prop.position[1]],
                half_extents: half_extents.map(|half| half.max(1)),
            })
            || scenery
                .props
                .iter()
                .any(|other| other.collider.is_some() && other.footprint().intersects(bounds))
        {
            return Err(format!(
                "{} overlaps a structure, road or spawn plaza",
                prop.id
            ));
        }
        scenery.props.push(placed);
    }
    Ok(scenery)
}
