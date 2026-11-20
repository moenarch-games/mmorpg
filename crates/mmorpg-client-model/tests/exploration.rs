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
        assert!(area.min[0] < area.max[0] && area.min[1] < area.max[1]);
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

    entry.progress[index] = 1;
    let done = exploration_map(&content, &BTreeSet::new(), Some(&sheet(entry)));
    assert!(
        done.hints
            .iter()
            .all(|hint| hint.area != area || hint.quest != quest.get())
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
