//! The exploration map both clients draw on the world map and the minimap (#983): the
//! content's named areas, which of them the viewer discovered, how many of the discoverable
//! ones that is, approximate areas for the active quests' open objectives, and the points of
//! interest in the discovered areas (#1063).
//!
//! Everything is derived from immutable content, the viewer's discovered areas (the waystone
//! sheet, #916) and the received quest sheet. Quest hints are deliberately approximate
//! (Atlas 5.1.13): a whole named area, never a spawn point, and they follow only the progress
//! the zone confirmed. A point of interest shows once its area is discovered; standing at one
//! discovers its area, so a visited point always shows.
use std::collections::BTreeSet;

use mmorpg_core::{
    Area, AreaId, CreatureTemplateId, LandmarkKind, NpcRole, QuestMarker, QuestObjective,
    QuestSheet, UNITS_PER_METRE, ZoneContent,
};

use crate::quest::objective_label;

/// One named area on the map, in metres (x east, z south).
#[derive(Clone, Debug, PartialEq)]
pub struct MapArea {
    pub id: AreaId,
    pub name: String,
    /// The outline's outer ring, then any holes; open, in metres.
    pub rings: Vec<Vec<[f32; 2]>>,
    /// A point well inside the area, where its name goes.
    pub centre: [f32; 2],
    /// How far `centre` lies from the area's edge, in metres.
    pub radius: f32,
    /// Discovered by the viewer, or not a discovery at all (the starting area).
    pub discovered: bool,
}

impl MapArea {
    /// Where the area's name goes, in metres.
    #[must_use]
    pub fn center(&self) -> [f32; 2] {
        self.centre
    }
}

/// What a point of interest is (#1063); each kind has its own marker.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum PointKind {
    Waystone,
    /// A cave, a mine, a bandits' hold or a wildlife den.
    Dungeon,
    /// A tower, a ruin, a bridge or a discovery cache.
    Landmark,
    /// A vendor or a spirit healer.
    Service,
    /// Someone with a quest to give or to take back.
    QuestGiver,
}

impl PointKind {
    /// The kind's stable name, as the browser receives it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Waystone => "waystone",
            Self::Dungeon => "dungeon",
            Self::Landmark => "landmark",
            Self::Service => "service",
            Self::QuestGiver => "quest",
        }
    }
}

/// One point of interest in a discovered area, in metres.
#[derive(Clone, Debug, PartialEq)]
pub struct MapPoint {
    pub kind: PointKind,
    pub name: String,
    /// What it is, e.g. "Waystone", "Vendor" or "Quest available".
    pub detail: String,
    pub at: [f32; 2],
}

/// An approximate place to look for one open objective of an active quest.
#[derive(Clone, Debug, PartialEq)]
pub struct QuestHint {
    pub quest: u32,
    /// The objective, e.g. "Explore Wolfrun Woods" or "Timber Wolf slain".
    pub label: String,
    /// The area the hint covers.
    pub area: AreaId,
    /// A circle in metres: the area's centre and the radius of its inscribed circle.
    pub center: [f32; 2],
    pub radius: f32,
}

/// The exploration map for one viewer.
#[derive(Clone, Debug, PartialEq)]
pub struct ExplorationMap {
    /// Every named area, in ID order.
    pub areas: Vec<MapArea>,
    /// Discoverable areas the viewer discovered.
    pub discovered: usize,
    /// Discoverable areas in the content.
    pub total: usize,
    pub hints: Vec<QuestHint>,
    /// Points of interest in the discovered areas, by kind, then in content order.
    pub points: Vec<MapPoint>,
}

/// The viewer's exploration map: `discoveries` from the waystone sheet, `sheet` the received
/// quest sheet, if any.
#[must_use]
pub fn exploration_map(
    content: &ZoneContent,
    discoveries: &BTreeSet<AreaId>,
    sheet: Option<&QuestSheet>,
) -> ExplorationMap {
    let discoverable = content.discoveries();
    let areas = content
        .areas()
        .areas()
        .iter()
        .map(|area| {
            map_area(
                area,
                discoveries.contains(&area.id()) || discoverable.binary_search(&area.id()).is_err(),
            )
        })
        .collect::<Vec<_>>();
    let discovered = discoverable
        .iter()
        .filter(|area| discoveries.contains(area))
        .count();
    let hints = sheet.map_or_else(Vec::new, |sheet| quest_hints(content, sheet, &areas));
    let points = points(content, &areas, sheet);
    ExplorationMap {
        areas,
        discovered,
        total: discoverable.len(),
        hints,
        points,
    }
}

#[allow(clippy::cast_precision_loss)]
fn metres(units: i32) -> f32 {
    units as f32 / UNITS_PER_METRE as f32
}

fn xz([x, z]: [i32; 2]) -> [f32; 2] {
    [metres(x), metres(z)]
}

fn map_area(area: &Area, discovered: bool) -> MapArea {
    MapArea {
        id: area.id(),
        name: area.name().to_owned(),
        rings: area
            .rings()
            .iter()
            .map(|ring| ring.iter().copied().map(xz).collect())
            .collect(),
        centre: xz(area.centre()),
        radius: metres(area.radius()),
        discovered,
    }
}

/// The points of interest (#1063) whose area the viewer discovered: waystones, dungeons and
/// dens, landmarks and discovery caches, vendors and healers, and the people the quest sheet
/// marks with a quest to give or to take back (who then show as quest givers only).
fn points(content: &ZoneContent, areas: &[MapArea], sheet: Option<&QuestSheet>) -> Vec<MapPoint> {
    let shown = |at: [i32; 2]| {
        content
            .areas()
            .area_at(at[0], at[1])
            .and_then(|area| areas.iter().find(|shown| shown.id == area.id()))
            .is_some_and(|area| area.discovered)
    };
    let mut points = Vec::new();
    let mut point = |kind: PointKind, name: &str, detail: &str, at: [i32; 2]| {
        if shown(at) {
            points.push(MapPoint {
                kind,
                name: name.to_owned(),
                detail: detail.to_owned(),
                at: xz(at),
            });
        }
    };
    for site in content
        .waystones()
        .iter()
        .filter_map(|&id| mmorpg_core::waystone(id))
    {
        point(PointKind::Waystone, site.name, "Waystone", site.stone);
    }
    for landmark in content.landmarks() {
        let (kind, detail) = match landmark.kind {
            LandmarkKind::Dungeon => (PointKind::Dungeon, "Cave or hold"),
            LandmarkKind::Sight => (PointKind::Landmark, "Landmark"),
        };
        point(kind, &landmark.name, detail, landmark.at);
    }
    for den in content.dens() {
        point(PointKind::Dungeon, &den.name, "Den", den.den);
    }
    for cache in content.caches() {
        point(
            PointKind::Landmark,
            &cache.name,
            "Discovery",
            [cache.at[0], cache.at[2]],
        );
    }
    let marked = |npc| {
        sheet.and_then(|sheet| {
            sheet
                .markers
                .iter()
                .find(|marker| marker.npc == npc)
                .and_then(|marker| match marker.marker {
                    QuestMarker::Available => Some("Quest available"),
                    QuestMarker::Complete => Some("Quest ready"),
                    QuestMarker::InProgress => None,
                })
        })
    };
    for npc in content.npcs() {
        if let Some(detail) = marked(npc.id) {
            point(PointKind::QuestGiver, &npc.name, detail, npc.position);
            continue;
        }
        let detail = match npc.role {
            NpcRole::Vendor => "Vendor",
            NpcRole::SpiritHealer => "Spirit healer",
            NpcRole::QuestGiver | NpcRole::Guard | NpcRole::Resident => continue,
        };
        point(PointKind::Service, &npc.name, detail, npc.position);
    }
    points.sort_by_key(|point| point.kind);
    points
}

/// One hint per open objective that has a place: explore and retrieve objectives name their
/// area, inspect objectives (#1019) the area their evidence lies in, kill and collect objectives the area most of their creatures spawn in. Talking to an
/// NPC already has its quest marker, and the depths boss is no place on this map.
fn quest_hints(content: &ZoneContent, sheet: &QuestSheet, areas: &[MapArea]) -> Vec<QuestHint> {
    let mut hints = Vec::new();
    for entry in &sheet.entries {
        let Some(quest) = content.quest(entry.quest) else {
            continue;
        };
        for (index, objective) in quest.objectives.iter().enumerate() {
            let progress = entry.progress.get(index).copied().unwrap_or(0);
            if progress >= objective.required() {
                continue;
            }
            let area = match *objective {
                QuestObjective::Explore { area } | QuestObjective::Retrieve { area, .. } => {
                    Some(area)
                }
                QuestObjective::Kill { template, .. }
                | QuestObjective::Collect {
                    source: template, ..
                } => creature_area(content, template),
                // Evidence (#1019) hints at the area around its spot, never the spot itself.
                QuestObjective::Inspect { spot } => content
                    .evidence_spot(spot)
                    .filter(|spot| !spot.hidden())
                    .and_then(|spot| {
                        content
                            .areas()
                            .area_at(spot.at[0], spot.at[2])
                            .map(Area::id)
                    }),
                // The trial's and the boulder's foes (#1019) like kills; a structure the area it
                // stands in; an escort the area it leads to. NPC actions have their marker.
                QuestObjective::DefeatWithLoan { template, .. }
                | QuestObjective::Crush { template, .. } => creature_area(content, template),
                QuestObjective::Demolish { piece } => content
                    .pieces()
                    .iter()
                    .find(|candidate| candidate.id == piece)
                    .and_then(|piece| {
                        content
                            .areas()
                            .area_at(piece.position[0], piece.position[2])
                            .map(Area::id)
                    }),
                QuestObjective::Escort { area, .. } => Some(area),
                // A delivery (#1077) hints at where its load is fetched; its NPC has its marker.
                QuestObjective::DeliverLoad { source, .. } => {
                    content.load_spot(source).and_then(|spot| {
                        content
                            .areas()
                            .area_at(spot.at[0], spot.at[2])
                            .map(Area::id)
                    })
                }
                // A build (#1019) hints at the area its site stands in; a defence and the aid
                // after it at the raid site's.
                QuestObjective::Build { piece } => content
                    .pieces()
                    .iter()
                    .find(|candidate| candidate.id == piece)
                    .and_then(|piece| {
                        content
                            .areas()
                            .area_at(piece.position[0], piece.position[2])
                            .map(Area::id)
                    }),
                QuestObjective::Defend { .. } | QuestObjective::Aid { .. } => {
                    content.raid().and_then(|site| {
                        let [x, z] = site.centre();
                        content.areas().area_at(x, z).map(Area::id)
                    })
                }
                // A rubbing (#1019) hints at the area round its inscription, an unsealing at the area
                // its seal's first glyph lies in. A hidden find hints at nothing: its quest log reads
                // the landmark directions instead.
                QuestObjective::Transcribe { spot, .. } => content
                    .evidence_spot(spot)
                    .filter(|spot| !spot.hidden())
                    .and_then(|spot| {
                        content
                            .areas()
                            .area_at(spot.at[0], spot.at[2])
                            .map(Area::id)
                    }),
                QuestObjective::Unseal { piece } => content
                    .seal(piece)
                    .and_then(|seal| content.mechanism(*seal.mechanisms.first()?))
                    .and_then(|glyph| {
                        content
                            .areas()
                            .area_at(glyph.at[0], glyph.at[2])
                            .map(Area::id)
                    }),
                QuestObjective::Talk { .. }
                | QuestObjective::DefeatThorn
                | QuestObjective::UseItem { .. }
                | QuestObjective::Interview { .. } => None,
                // A decision (#1019) hints at where its NPC stands: it carries no quest marker.
                QuestObjective::Decide { npc } => content
                    .npc(npc)
                    .and_then(|npc| content.areas().area_at(npc.position[0], npc.position[1]))
                    .map(Area::id),
            };
            let Some(area) = area.and_then(|id| areas.iter().find(|area| area.id == id)) else {
                continue;
            };
            let center = area.center();
            hints.push(QuestHint {
                quest: quest.id.get(),
                label: objective_label(*objective, content),
                area: area.id,
                center,
                radius: area.radius,
            });
        }
    }
    hints
}

/// The named area most spawns of `template` stand in; ties go to the lower area ID.
fn creature_area(content: &ZoneContent, template: CreatureTemplateId) -> Option<AreaId> {
    let mut counts = std::collections::BTreeMap::<AreaId, usize>::new();
    for spawn in content
        .creature_spawns()
        .iter()
        .filter(|spawn| spawn.template == template)
    {
        if let Some(area) = content
            .areas()
            .area_at(spawn.position[0], spawn.position[1])
        {
            *counts.entry(area.id()).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))
        .map(|(area, _)| area)
}
