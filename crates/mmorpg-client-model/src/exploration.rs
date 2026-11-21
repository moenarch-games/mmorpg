//! The exploration map both clients draw on the world map and the minimap (#983): the
//! content's named areas, which of them the viewer discovered, how many of the discoverable
//! ones that is, and approximate areas for the active quests' open objectives.
//!
//! Everything is derived from immutable content, the viewer's discovered areas (the waystone
//! sheet, #916) and the received quest sheet. Quest hints are deliberately approximate
//! (Atlas 5.1.13): a whole named area, never a spawn point, and they follow only the progress
//! the zone confirmed.
use std::collections::BTreeSet;

use mmorpg_core::{
    Area, AreaId, CreatureTemplateId, QuestObjective, QuestSheet, UNITS_PER_METRE, ZoneContent,
};

use crate::quest::objective_label;

/// One named area on the map, in metres (x east, z south).
#[derive(Clone, Debug, PartialEq)]
pub struct MapArea {
    pub id: AreaId,
    pub name: String,
    pub min: [f32; 2],
    pub max: [f32; 2],
    /// Discovered by the viewer, or not a discovery at all (the starting area).
    pub discovered: bool,
}

impl MapArea {
    /// The area's centre in metres.
    #[must_use]
    pub fn center(&self) -> [f32; 2] {
        [
            f32::midpoint(self.min[0], self.max[0]),
            f32::midpoint(self.min[1], self.max[1]),
        ]
    }
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
    ExplorationMap {
        areas,
        discovered,
        total: discoverable.len(),
        hints,
    }
}

#[allow(clippy::cast_precision_loss)]
fn metres(units: i32) -> f32 {
    units as f32 / UNITS_PER_METRE as f32
}

fn map_area(area: &Area, discovered: bool) -> MapArea {
    let [min_x, min_z] = area.min_xz();
    let [max_x, max_z] = area.max_xz();
    MapArea {
        id: area.id(),
        name: area.name().to_owned(),
        min: [metres(min_x), metres(min_z)],
        max: [metres(max_x + 1), metres(max_z + 1)],
        discovered,
    }
}

/// One hint per open objective that has a place: explore and retrieve objectives name their
/// area, kill and collect objectives the area most of their creatures spawn in. Talking to an
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
                QuestObjective::Talk { .. } | QuestObjective::DefeatThorn => None,
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
                radius: 0.5 * (area.max[0] - area.min[0]).min(area.max[1] - area.min[1]),
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
