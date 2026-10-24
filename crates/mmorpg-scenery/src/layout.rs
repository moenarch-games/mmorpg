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

fn base_lamp_id(prop: &Prop) -> Option<&'static str> {
    if prop.kind != PropKind::Lamp {
        return None;
    }
    match [prop.position[0], prop.position[2]] {
        [-1_800, 1_000] => Some("outpost-lamp-1"),
        [1_800, 1_000] => Some("outpost-lamp-2"),
        [-1_800, 3_000] => Some("outpost-lamp-3"),
        [1_800, 3_000] => Some("outpost-lamp-4"),
        _ => None,
    }
}

fn house(collider: &mmorpg_core::StaticCollider) -> Prop {
    Prop {
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
    }
}

pub fn compose(source: &str, definition: &ZoneDefinition) -> Result<Scenery, String> {
    let decoration = decorations(source)?;
    let mut scenery = base_scenery();
    // Preserve the frozen base and placement order. Overrides replace existing logical
    // objects in place; only author additions append. Never reroll the scatter RNG.
    scenery.props.retain_mut(|prop| {
        if let Some(id) = prop.collider.filter(|id| (103..=106).contains(id)) {
            if let Some(collider) = definition
                .colliders()
                .iter()
                .find(|collider| collider.id == id)
            {
                *prop = house(collider);
            } else {
                return false;
            }
        }
        true
    });
    for collider in definition
        .colliders()
        .iter()
        .filter(|collider| (60_000..65_000).contains(&collider.id))
    {
        scenery.props.push(house(collider));
    }
    let houses: Vec<_> = scenery
        .props
        .iter()
        .filter(|prop| {
            prop.collider
                .is_some_and(|id| (103..=106).contains(&id) || (60_000..65_000).contains(&id))
        })
        .map(Prop::footprint)
        .collect();
    // Suppress intersecting cosmetic scatter rather than move or reroll it. The
    // saved lamp layer is applied below, so a moved lamp can leave this footprint.
    scenery.props.retain(|prop| {
        prop.collider.is_some()
            || !houses
                .iter()
                .any(|bounds| prop.footprint().intersects(*bounds))
    });
    let mut placements = Vec::new();
    for prop in &decoration {
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
        placements.push((prop.id.as_str(), placed));
    }
    let mut replaced = BTreeSet::new();
    scenery.props.retain_mut(|prop| {
        if let Some(id) = base_lamp_id(prop) {
            if let Some((_, placement)) = placements.iter().find(|(key, _)| *key == id) {
                *prop = *placement;
                replaced.insert(id);
            } else {
                return false;
            }
        }
        true
    });
    scenery.props.extend(
        placements
            .into_iter()
            .filter(|(id, _)| !replaced.contains(id))
            .map(|(_, prop)| prop),
    );
    // The scenery belongs to the content revision this exact definition hosts.
    scenery.content_revision = greyhaven_vale::content_with_definition(definition.clone())
        .map_err(|error| error.to_string())?
        .revision();
    Ok(scenery)
}
