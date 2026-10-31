//! The Outpost layout's houses and decoration composed after base generation.
//!
//! Core parses and validates the layout (`mmorpg_core::greyhaven_vale::layout`); its lamps and
//! signposts collide there (#577), and this draws each decoration where core put it.
use mmorpg_core::greyhaven_vale::layout::{self as core_layout, DecorationKind};
use mmorpg_core::{ZoneDefinition, greyhaven_vale};

pub use core_layout::Decoration;

use crate::{Prop, PropKind, Scenery, base_half_extents, base_scenery, dressing_prop};

/// The layout's decorations, validated by core.
pub fn decorations(source: &str) -> Result<Vec<Decoration>, String> {
    core_layout::parse(source)
        .map(|layout| layout.decoration)
        .map_err(|error| error.to_string())
}

/// The drawn kind of a decoration prefab.
#[must_use]
pub const fn kind(kind: DecorationKind) -> PropKind {
    match kind {
        DecorationKind::Lamp => PropKind::Lamp,
        DecorationKind::Signpost => PropKind::Signpost,
        DecorationKind::Flowers => PropKind::Flowers,
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
    let layout = core_layout::parse(source).map_err(|error| error.to_string())?;
    let mut scenery = base_scenery();
    // Preserve the frozen base and placement order. Overrides replace existing logical
    // objects in place; only author additions append. Never reroll the scatter RNG.
    scenery.props.retain_mut(|prop| {
        if let Some(id) = prop.collider.filter(|id| (103..=106).contains(id)) {
            // Elden's walk-in house (#158) collides through its parts; its anchor stands for it.
            let anchor = (id == mmorpg_core::greyhaven_vale::ids::MARSHAL_HOUSE)
                .then(crate::marshal_house_anchor);
            if let Some(collider) = anchor.as_ref().or_else(|| {
                definition
                    .colliders()
                    .iter()
                    .find(|collider| collider.id == id)
            }) {
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
    let dressing: Vec<Prop> = layout
        .dressing()
        .iter()
        .map(|(_, piece)| dressing_prop(piece))
        .collect();
    // Suppress cosmetic scatter under the houses, the market square (which follows house 106)
    // and the layout's solid decorations rather than move or reroll it.
    let keep_out: Vec<_> = scenery
        .props
        .iter()
        .filter(|prop| {
            prop.collider
                .is_some_and(|id| (103..=106).contains(&id) || (60_000..65_000).contains(&id))
        })
        .chain(&dressing)
        .map(Prop::footprint)
        .chain(
            definition
                .colliders()
                .iter()
                .filter(|collider| greyhaven_vale::ids::MARKET.contains(&collider.id))
                .map(|collider| {
                    crate::Rect::around(
                        [collider.position[0], collider.position[2]],
                        [collider.half_extents[0], collider.half_extents[2]],
                    )
                }),
        )
        .collect();
    scenery.props.retain(|prop| {
        prop.collider.is_some()
            || !keep_out
                .iter()
                .any(|bounds| prop.footprint().intersects(*bounds))
    });
    scenery.props.extend(dressing);
    scenery.props.extend(
        layout
            .decoration
            .iter()
            .filter(|decoration| decoration.kind.solid().is_none())
            .map(|decoration| {
                let kind = kind(decoration.kind);
                Prop {
                    kind,
                    position: [decoration.position[0], 0, decoration.position[1]],
                    yaw: decoration.yaw,
                    scale_permille: 1_000,
                    collider: None,
                    half_extents: base_half_extents(kind),
                }
            }),
    );
    // The scenery belongs to the content revision this exact definition hosts.
    scenery.content_revision = greyhaven_vale::content_with_definition(definition.clone())
        .map_err(|error| error.to_string())?
        .revision();
    Ok(scenery)
}
