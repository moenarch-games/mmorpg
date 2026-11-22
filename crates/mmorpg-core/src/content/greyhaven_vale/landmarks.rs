//! Greyhaven's named places on the map (#1063): its caves, mines and bandit holds, and its
//! towers, ruins and bridges, each where the content builds it. Dens, discovery caches,
//! waystones and the people who trade or give quests have their own tables.
use crate::{Landmark, LandmarkKind};

fn middle((min, max): ([i32; 2], [i32; 2])) -> [i32; 2] {
    [
        min[0] + (max[0] - min[0]) / 2,
        min[1] + (max[1] - min[1]) / 2,
    ]
}

/// The Vale structure `id`, at its centre.
fn structure(id: u32) -> [i32; 2] {
    super::vale::STRUCTURES
        .iter()
        .find(|footprint| footprint.0 == id)
        .map_or([0, 0], |footprint| middle((footprint.1, footprint.2)))
}

/// The structure of `kind` a settlement builds, at its centre.
fn settlement_structure(name: &str, kind: super::settlements::Building) -> Option<[i32; 2]> {
    super::settlements::SETTLEMENTS
        .iter()
        .find(|settlement| settlement.name == name)?
        .structures
        .iter()
        .find(|structure| structure.0 == kind)
        .map(|structure| structure.1)
}

fn place(name: &str, kind: LandmarkKind, at: [i32; 2]) -> Landmark {
    Landmark {
        name: name.to_owned(),
        kind,
        at,
    }
}

#[must_use]
pub fn landmarks() -> Vec<Landmark> {
    use super::settlements::Building;
    use LandmarkKind::{Dungeon, Sight};
    let cave = super::cave::GREYFANG.floors[0];
    let marsh_tower = super::marsh::BUILDINGS
        .iter()
        .find(|building| building.0 == super::marsh::Building::Tower)
        .map(|building| middle((building.1, building.2)));
    let brook_bridge = super::brook::colliders()
        .iter()
        .find(|collider| collider.id == super::brook::BRIDGE)
        .map_or([0, 0], |collider| {
            [collider.position[0], collider.position[2]]
        });
    let mut landmarks = vec![
        place("Greyfang Cave", Dungeon, middle(cave)),
        place(
            "Redbrand Mine",
            Dungeon,
            structure(super::ids::MINE_ENTRANCE),
        ),
        place(
            "Redbrand Stronghold",
            Dungeon,
            middle(super::highlands::STRONGHOLD),
        ),
        place("Sunken Shrine", Sight, middle(super::shrine::ALTAR_CHAMBER)),
        place(
            "Eastwater Watchtower",
            Sight,
            structure(super::ids::WATCHTOWER),
        ),
        place("Highland Lookout", Sight, middle(super::highlands::LOOKOUT)),
        place(
            "Ruined Farmstead",
            Sight,
            structure(*super::ids::RUINS.start()),
        ),
        place("Village Brook Bridge", Sight, brook_bridge),
    ];
    if let Some(mine) = settlement_structure("Delving Workings", Building::MineEntrance) {
        landmarks.push(place("Delving Mine", Dungeon, mine));
    }
    if let Some(tower) = settlement_structure("Greyfang Watch", Building::Tower) {
        landmarks.push(place("Greyfang Watchtower", Sight, tower));
    }
    if let Some(tower) = marsh_tower {
        landmarks.push(place("Drowned Watchtower", Sight, tower));
    }
    landmarks.extend(
        super::crossings::BRIDGES
            .iter()
            .map(|bridge| place("Delving Run Footbridge", Sight, bridge.centre)),
    );
    landmarks
}
