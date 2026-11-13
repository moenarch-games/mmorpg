//! Everything in Greyhaven that looks solid collides (#577): every placed prop of a solid kind
//! inside the walls is drawn from a collider of the hosted content, and the market square's
//! kit props each have their collision proxies. Foliage alone stays walk-through.
use mmorpg_core::greyhaven_vale::{self, ids};
use mmorpg_core::{StaticCollider, greyhaven_vale_definition};
use mmorpg_scenery::{PropKind, Rect, greyhaven_vale_scenery};

/// Whether a kind looks solid. Every kind decides here, so a new prop kind cannot ship
/// without choosing.
const fn solid(kind: PropKind) -> bool {
    match kind {
        PropKind::GrassTuft
        | PropKind::Flowers
        | PropKind::Reeds
        | PropKind::Bush
        | PropKind::CropRow => false,
        PropKind::Keep
        | PropKind::Inn
        | PropKind::House
        | PropKind::Smithy
        | PropKind::Barn
        | PropKind::Farmhouse
        | PropKind::Windmill
        | PropKind::Well
        | PropKind::PalisadeSegment
        | PropKind::GatePost
        | PropKind::Waystone
        | PropKind::Gravestone
        | PropKind::Tree(_)
        | PropKind::Rock(_)
        | PropKind::Cliff
        | PropKind::Fence
        | PropKind::Tent
        | PropKind::Campfire
        | PropKind::Crate
        | PropKind::Barrel
        | PropKind::Cart
        | PropKind::Signpost
        | PropKind::Lamp
        | PropKind::MineEntrance
        | PropKind::Dock
        | PropKind::CaveRoof
        | PropKind::Ramp
        | PropKind::LoadingDock
        | PropKind::ShrineKit
        | PropKind::Ladder
        | PropKind::Rope
        | PropKind::GrayboxWall
        | PropKind::GrayboxTower
        | PropKind::GrayboxWalkway
        | PropKind::GrayboxRamp => true,
    }
}

fn footprint(collider: &StaticCollider) -> Rect {
    Rect::around(
        [collider.position[0], collider.position[2]],
        [collider.half_extents[0], collider.half_extents[2]],
    )
}

#[test]
fn every_solid_prop_inside_the_walls_has_its_collider() {
    let definition = greyhaven_vale_definition();
    let scenery = greyhaven_vale_scenery();
    let mut missing = Vec::new();
    for prop in &scenery.props {
        let [x, _, z] = prop.position;
        // The backdrop forest on the mountains beyond the walls is out of reach.
        if !solid(prop.kind) || !greyhaven_vale::inside_walls(x, z) {
            continue;
        }
        if prop.kind == PropKind::Ramp {
            assert!(
                greyhaven_vale::ramps()
                    .iter()
                    .any(|ramp| ramp.position[0] == x && ramp.position[2] == z),
                "a ramp is a ramp body"
            );
            continue;
        }
        let collider = prop.collider.and_then(|id| {
            definition
                .colliders()
                .iter()
                .find(|collider| collider.id == id)
        });
        match collider {
            // Its collider fills its footprint, the inn and Elden's house through their parts.
            Some(collider) => {
                if prop.collider != Some(ids::INN) && prop.collider != Some(ids::MARSHAL_HOUSE) {
                    assert_eq!(prop.footprint(), footprint(collider), "{prop:?}");
                }
            }
            None if prop.collider == Some(ids::INN)
                || prop.collider == Some(ids::MARSHAL_HOUSE) => {}
            None => missing.push(prop),
        }
    }
    assert!(
        missing.is_empty(),
        "{} solid props have no collider, e.g. {:?}",
        missing.len(),
        &missing[..missing.len().min(5)]
    );
}

#[test]
fn foliage_stays_walk_through() {
    for prop in &greyhaven_vale_scenery().props {
        if !solid(prop.kind) {
            assert_eq!(prop.collider, None, "{prop:?}");
        }
    }
}
