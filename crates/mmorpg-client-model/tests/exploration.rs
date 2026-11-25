//! The exploration map (#983) over Greyhaven's real content: the discovered count comes from
//! the content's discovery list, discovered areas show as such, and quest hints are whole areas
//! that follow only received progress.
use std::collections::BTreeSet;

use mmorpg_client_model::exploration::exploration_map;
use mmorpg_core::{QuestEntry, QuestObjective, QuestSheet, greyhaven_vale};

#[test]
fn discoveries_and_quest_hints_follow_the_received_sheets() {
    let content = greyhaven_vale::content();
    let total = content.discoveries().len();
    assert!(total > 1, "Greyhaven has discoverable areas");

    // A fresh character has discovered nothing; only the starting area is known.
    let fresh = exploration_map(&content, &BTreeSet::new(), None);
    assert_eq!((fresh.discovered, fresh.total), (0, total));
    assert_eq!(fresh.areas.len(), content.areas().areas().len());
    assert_eq!(
        fresh.areas.iter().filter(|area| area.discovered).count(),
        fresh.areas.len() - total
    );
    assert!(fresh.hints.is_empty());

    // Discovering two areas reveals exactly those.
    let found: BTreeSet<_> = content.discoveries().iter().take(2).copied().collect();
    let map = exploration_map(&content, &found, None);
    assert_eq!(map.discovered, 2);
    for area in &map.areas {
        let starting = content.discoveries().binary_search(&area.id).is_err();
        assert_eq!(
            area.discovered,
            starting || found.contains(&area.id),
            "{}",
            area.name
        );
        assert!(area.rings.first().is_some_and(|ring| ring.len() >= 3) && area.radius > 0.0);
    }

    // An active explore objective hints at its whole area until the zone confirms it.
    let (quest, index, area) = content
        .quests()
        .iter()
        .find_map(|quest| {
            quest
                .objectives
                .iter()
                .enumerate()
                .find_map(|(index, objective)| match objective {
                    QuestObjective::Explore { area } => Some((quest.id, index, *area)),
                    _ => None,
                })
        })
        .expect("a Greyhaven quest explores an area");
    let mut entry = QuestEntry {
        quest,
        progress: Default::default(),
    };
    let sheet = |entry: QuestEntry| QuestSheet {
        completed: BTreeSet::new(),
        entries: vec![entry],
        markers: Vec::new(),
    };
    let open = exploration_map(&content, &BTreeSet::new(), Some(&sheet(entry)));
    let hint = open
        .hints
        .iter()
        .find(|hint| hint.area == area)
        .expect("the open explore objective is hinted");
    let shown = open
        .areas
        .iter()
        .find(|candidate| candidate.id == area)
        .unwrap();
    assert_eq!(hint.center, shown.center());
    assert!(hint.radius > 0.0);
    assert_eq!(hint.quest, quest.get());

    // Its other open objectives (the Wolfrun Cart's sacks share the area) keep their hints.
    entry.progress[index] = 1;
    let done = exploration_map(&content, &BTreeSet::new(), Some(&sheet(entry)));
    assert!(
        done.hints
            .iter()
            .all(|other| other.quest != quest.get() || other.label != hint.label)
    );

    // A kill objective points at the area its creatures roam, never at a spawn.
    let kill = content
        .quests()
        .iter()
        .find(|quest| {
            quest
                .objectives
                .iter()
                .any(|objective| matches!(objective, QuestObjective::Kill { .. }))
        })
        .expect("a Greyhaven quest asks for kills");
    let hunting = exploration_map(
        &content,
        &BTreeSet::new(),
        Some(&sheet(QuestEntry {
            quest: kill.id,
            progress: Default::default(),
        })),
    );
    assert!(hunting.hints.iter().any(|hint| hint.quest == kill.id.get()));
}

/// A hidden find (#1019, *Bertil's Heirloom*) has no map hint: its quest log reads Bertil's
/// landmark directions instead. A plain inspection (*A Shepherd's Bargain*'s gate) keeps its
/// area's hint, and the bargain's paid option names its price.
#[test]
fn a_hidden_find_reads_its_directions_and_marks_nothing_on_the_map() {
    use mmorpg_client_model::quest::{decision_at, objective_label};
    use mmorpg_core::{NpcId, QuestId};

    let content = greyhaven_vale::content();
    let sheet = |quest: u32, progress: [u8; 3]| QuestSheet {
        completed: BTreeSet::new(),
        entries: vec![QuestEntry {
            quest: QuestId::new(quest),
            progress,
        }],
        markers: Vec::new(),
    };
    let heirloom = content.quest(QuestId::new(57)).unwrap();
    assert_eq!(
        objective_label(heirloom.objectives[0], &content),
        "Find Bertil's Locket under a stone at the old tower's south-west corner"
    );
    let map = exploration_map(&content, &BTreeSet::new(), Some(&sheet(57, [0; 3])));
    assert!(map.hints.is_empty(), "{:?}", map.hints);

    let bargain = exploration_map(&content, &BTreeSet::new(), Some(&sheet(54, [1, 0, 0])));
    assert_eq!(
        bargain
            .hints
            .iter()
            .map(|hint| hint.label.as_str())
            .collect::<Vec<_>>(),
        [
            "Inspect the tracks at Lise's gate",
            "Decide what becomes of Old Bertil"
        ]
    );
    let decision = decision_at(&content, &sheet(54, [1, 1, 0]).entries, NpcId::new(38)).unwrap();
    assert_eq!(
        decision.options,
        [
            "Show Bertil the goat tracks at the gate",
            "Pay Lise for the grazing yourself (20 copper)"
        ]
    );
}

/// Points of interest (#1063) show only in discovered areas, each kind with its own marker, and
/// a quest giver shows only while the received sheet marks them.
#[test]
fn points_of_interest_follow_discovery_and_the_quest_sheet() {
    use mmorpg_client_model::exploration::PointKind;
    use mmorpg_core::{NpcMarker, QuestMarker};
    let content = greyhaven_vale::content();
    let fresh = exploration_map(&content, &BTreeSet::new(), None);
    // Only the Outpost is known: its waystone and its vendors and healer.
    let outpost = greyhaven_vale::vale::OUTPOST;
    for point in &fresh.points {
        let at = [point.at[0] * 100.0, point.at[1] * 100.0].map(|v| v.round() as i32);
        assert_eq!(
            greyhaven_vale::area_at(at[0], at[1]).map(|a| a.id()),
            Some(outpost),
            "{}",
            point.name
        );
    }
    let kinds: BTreeSet<_> = fresh.points.iter().map(|point| point.kind).collect();
    assert!(kinds.contains(&PointKind::Waystone) && kinds.contains(&PointKind::Service));

    // Everything discovered: every kind shows.
    let all: BTreeSet<_> = content.discoveries().iter().copied().collect();
    let marshal = content.npcs()[0].id;
    let sheet = QuestSheet {
        completed: BTreeSet::new(),
        entries: Vec::new(),
        markers: vec![NpcMarker {
            npc: marshal,
            marker: QuestMarker::Available,
        }],
    };
    let map = exploration_map(&content, &all, Some(&sheet));
    let kinds: BTreeSet<_> = map.points.iter().map(|point| point.kind).collect();
    assert_eq!(
        kinds,
        BTreeSet::from([
            PointKind::Waystone,
            PointKind::Dungeon,
            PointKind::Landmark,
            PointKind::Service,
            PointKind::QuestGiver
        ])
    );
    let givers: Vec<_> = map
        .points
        .iter()
        .filter(|point| point.kind == PointKind::QuestGiver)
        .map(|point| point.name.as_str())
        .collect();
    assert_eq!(givers, [content.npcs()[0].name.as_str()]);
    for name in [
        "Greyfang Cave",
        "Redbrand Mine",
        "Drowned Watchtower",
        "Wolfrun Den",
    ] {
        assert!(map.points.iter().any(|point| point.name == name), "{name}");
    }
}
