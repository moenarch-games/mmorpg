//! The region's quests (#820): the Marshal sends a player out to Barrowdown, where the alewife
//! takes the turn-in and offers the next quest of her chain.
use crate::support;

use std::sync::Arc;

use mmorpg_core::greyhaven_vale::{self, quests};
use mmorpg_core::{
    CharacterId, CharacterLocation, QuestEntry, QuestLog, QuestObjective, ZoneCommand, ZoneEvent,
    ZoneId, ZoneSimulation, item_template,
};
use support::classes;

const GREYHAVEN: ZoneId = ZoneId::new(1);

struct Player {
    zone: ZoneSimulation,
    sequence: u32,
}

impl Player {
    fn send(&mut self, command: ZoneCommand) -> Vec<ZoneEvent> {
        self.sequence += 1;
        self.zone.apply_command(1, self.sequence, command).unwrap();
        self.zone.advance_tick().unwrap();
        self.zone.snapshot_for_player(1).unwrap().events
    }

    /// Moves the player's body beside `npc`'s current placement, as walking there would.
    fn beside(&mut self, npc: mmorpg_core::NpcId) {
        let [x, z] = self.zone.npc_placement(npc).unwrap().position;
        let (x, z) = (x + 150, z + 150);
        let y = self.zone.content().definition().ground_at(x, z) + 90;
        let mut state = self.zone.snapshot().unwrap();
        state.players[0].position = [x as f32, y as f32, z as f32];
        let content = Arc::clone(self.zone.content());
        self.zone = ZoneSimulation::from_snapshot(state, content).unwrap();
        self.zone.advance_tick().unwrap();
    }

    fn log(&self) -> QuestLog {
        self.zone
            .capture_character(1, CharacterId::new("walker").unwrap())
            .unwrap()
            .quests
    }
}

/// A level 3 character who has finished *The Wolfrun Cart*, beside the Marshal.
fn walker() -> Player {
    walker_with(QuestLog {
        entries: Vec::new(),
        completed: [quests::THE_WOLFRUN_CART].into(),
        outcomes: Default::default(),
    })
}

/// A level 3 character with the saved quest `log`, beside the Marshal.
fn walker_with(log: QuestLog) -> Player {
    let mut donor = ZoneSimulation::with_content(GREYHAVEN, greyhaven_vale::content()).unwrap();
    donor.add_player(1).unwrap();
    let mut donor = classes::at_level(donor, 1, 3);
    donor
        .apply_command(1, 1, ZoneCommand::ChooseClass { class: 0, sex: 0 })
        .unwrap();
    donor.advance_tick().unwrap();
    let mut record = donor
        .capture_character(1, CharacterId::new("walker").unwrap())
        .unwrap();
    record.quests = log;
    let content = greyhaven_vale::content();
    let [x, z] = content.npc(quests::MARSHAL).unwrap().position;
    record.location = CharacterLocation {
        zone_id: GREYHAVEN,
        position: [x + 100, 90, z + 100],
        facing: 0,
    };
    let mut zone = ZoneSimulation::with_content(GREYHAVEN, content).unwrap();
    zone.admit_character(1, &record).unwrap();
    zone.advance_tick().unwrap();
    Player { zone, sequence: 10 }
}

#[test]
fn the_marshal_sends_a_player_to_barrowdown_and_the_alewife_carries_on() {
    let mut player = walker();
    player.send(ZoneCommand::AcceptQuest {
        npc: quests::MARSHAL,
        quest: quests::WORD_FROM_THE_DOWNS.get(),
    });
    assert_eq!(
        player.log().entries,
        [QuestEntry {
            quest: quests::WORD_FROM_THE_DOWNS,
            progress: [0, 0, 0],
        }]
    );
    // Arriving at Barrowdown beside Gwen completes the talk objective; she takes the turn-in.
    player.beside(quests::ALEWIFE);
    player.send(ZoneCommand::CompleteQuest {
        npc: quests::ALEWIFE,
        quest: quests::WORD_FROM_THE_DOWNS.get(),
        choice: 0,
    });
    let log = player.log();
    assert!(log.entries.is_empty(), "{log:?}");
    assert!(log.completed.contains(&quests::WORD_FROM_THE_DOWNS));
    // Her own chain opens.
    player.send(ZoneCommand::AcceptQuest {
        npc: quests::ALEWIFE,
        quest: quests::RAIDERS_ON_THE_DOWNS.get(),
    });
    assert_eq!(
        player.log().entries.first().map(|entry| entry.quest),
        Some(quests::RAIDERS_ON_THE_DOWNS)
    );
}

#[test]
fn every_region_quest_has_its_givers_items_and_creatures_in_the_region() {
    let content = greyhaven_vale::content();
    let region: Vec<_> = quests::quests()
        .into_iter()
        .filter(|quest| {
            (quests::WORD_FROM_THE_DOWNS.get()..=quests::THE_SHEPHERDS_WOLVES.get())
                .contains(&quest.id.get())
        })
        .collect();
    assert_eq!(region.len(), 20);
    for quest in &region {
        assert!(content.npc(quest.giver).is_some(), "{}", quest.name);
        assert!(content.npc(quest.ender).is_some(), "{}", quest.name);
        for objective in &quest.objectives {
            let template = match *objective {
                QuestObjective::Kill { template, .. } => template,
                QuestObjective::Collect { item, source, .. } => {
                    assert!(item_template(item).is_some(), "{}", quest.name);
                    source
                }
                _ => continue,
            };
            assert!(
                content
                    .creature_spawns()
                    .iter()
                    .any(|spawn| spawn.template == template),
                "{}: nothing to hunt",
                quest.name
            );
        }
    }
}

/// A character saved part-way through *Trouble in the Woods*, the first quest before #984, with
/// five of its six wolves slain, still loads: the admitting zone fits that progress to *The
/// Wolfrun Cart*, whose woods it plainly reached, while the cart's sacks still need fetching. One
/// who turned in both old wolf quests keeps them done and is still offered the Millbrook lead.
#[test]
fn characters_saved_under_the_old_wolf_quests_still_load() {
    let mut progress = [0; mmorpg_core::MAX_QUEST_OBJECTIVES];
    progress[0] = 5;
    let player = walker_with(QuestLog {
        entries: vec![QuestEntry {
            quest: quests::THE_WOLFRUN_CART,
            progress,
        }],
        completed: Default::default(),
        outcomes: Default::default(),
    });
    assert_eq!(
        player.log().entries,
        [QuestEntry {
            quest: quests::THE_WOLFRUN_CART,
            progress: [1, 0, 0],
        }]
    );

    let mut player = walker_with(QuestLog {
        entries: Vec::new(),
        completed: [quests::THE_WOLFRUN_CART, quests::PELTS_FOR_THE_TANNER].into(),
        outcomes: Default::default(),
    });
    player.beside(quests::TANNER);
    let events = player.send(ZoneCommand::AcceptQuest {
        npc: quests::TANNER,
        quest: quests::THE_MISSING_FARMHAND.get(),
    });
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, ZoneEvent::Error { .. })),
        "{events:?}"
    );
    assert!(player.log().entry(quests::THE_MISSING_FARMHAND).is_some());
}
