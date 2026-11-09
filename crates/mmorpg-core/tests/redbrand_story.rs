//! *The Captain Below* (#160): the follow-on story from Garrick Redbrand to Captain Ulric Thorn,
//! with the deferred victory credit of 5.1.37.
mod support;

use std::sync::Arc;

use mmorpg_core::greyhaven_vale::{self, quests};
use mmorpg_core::redbrand_depths::{self, THORN};
use mmorpg_core::{
    CharacterId, CharacterLocation, CharacterRecord, CreatureLife, DepthsRunStart, EntityRef,
    ItemId, QuestEntry, QuestId, QuestLog, QuestMarker, QuestObjective, ZoneCommand, ZoneEvent,
    ZoneId, ZoneSimulation, item_template,
};
use support::{arena, classes};

const SLOT: ZoneId = ZoneId::new(20_000);
const GREYHAVEN: ZoneId = ZoneId::new(1);
const ALL_CLEARED: u8 = 0b111;
const GREAVES: ItemId = ItemId::new(18);
/// The chain turned in up to *Into Redbrand Hollow*, Garrick's quest not yet.
const BEFORE_GARRICK: u32 = 0b0111_1111;

fn id(name: &str) -> CharacterId {
    CharacterId::new(name).unwrap()
}

/// A level 6 character with Garrick's quest active and `garrick_slain` kills of him.
fn character(name: &str, garrick_slain: u8) -> CharacterRecord {
    let mut donor = ZoneSimulation::with_content(GREYHAVEN, greyhaven_vale::content()).unwrap();
    donor.add_player(1).unwrap();
    let mut donor = classes::at_level(donor, 1, 6);
    donor
        .apply_command(1, 1, ZoneCommand::ChooseClass { class: 0, sex: 0 })
        .unwrap();
    donor.advance_tick().unwrap();
    let mut record = donor.capture_character(1, id(name)).unwrap();
    record.quests = QuestLog {
        entries: vec![QuestEntry {
            quest: quests::GARRICK_REDBRAND_QUEST,
            progress: [garrick_slain, 0, 0],
        }],
        completed: BEFORE_GARRICK,
    };
    record
}

/// Greyhaven with `record` standing beside the Marshal.
fn at_marshal(record: &CharacterRecord) -> Greyhaven {
    let content = greyhaven_vale::content();
    let [x, z] = content.npc(quests::MARSHAL).unwrap().position;
    let mut record = record.clone();
    record.location = CharacterLocation {
        zone_id: GREYHAVEN,
        position: [x + 100, 90, z + 100],
        facing: 0,
    };
    let mut zone = ZoneSimulation::with_content(GREYHAVEN, content).unwrap();
    zone.admit_character(1, &record).unwrap();
    tick(&mut zone, 2);
    Greyhaven {
        zone,
        name: record.character_id,
        sequence: 100,
    }
}

struct Greyhaven {
    zone: ZoneSimulation,
    name: CharacterId,
    sequence: u32,
}

impl Greyhaven {
    fn send(&mut self, command: ZoneCommand) -> Vec<ZoneEvent> {
        self.sequence += 1;
        self.zone.apply_command(1, self.sequence, command).unwrap();
        self.zone.advance_tick().unwrap();
        self.zone.snapshot_for_player(1).unwrap().events
    }

    fn accept(&mut self, quest: QuestId) -> Vec<ZoneEvent> {
        self.send(ZoneCommand::AcceptQuest {
            npc: quests::MARSHAL,
            quest: quest.get(),
        })
    }

    fn turn_in(&mut self, quest: QuestId) -> Vec<ZoneEvent> {
        self.send(ZoneCommand::CompleteQuest {
            npc: quests::MARSHAL,
            quest: quest.get(),
            choice: 0,
        })
    }

    fn record(&self) -> CharacterRecord {
        self.zone.capture_character(1, self.name.clone()).unwrap()
    }

    /// Restores the zone from its canonical state, as process recovery would.
    fn recover(&mut self) {
        let state = self.zone.snapshot().unwrap();
        self.zone = ZoneSimulation::from_snapshot(state, Arc::clone(self.zone.content())).unwrap();
    }

    fn sheet(&self) -> mmorpg_core::QuestSheet {
        self.zone.snapshot_for_player(1).unwrap().quests.unwrap()
    }

    fn marshal_marker(&self) -> Option<QuestMarker> {
        self.sheet()
            .markers
            .iter()
            .find(|marker| marker.npc == quests::MARSHAL)
            .map(|marker| marker.marker)
    }
}

fn tick(zone: &mut ZoneSimulation, n: u64) {
    for _ in 0..n {
        zone.advance_tick().unwrap();
    }
}

fn edit(
    zone: ZoneSimulation,
    change: impl FnOnce(&mut mmorpg_core::CanonicalZoneSnapshot),
) -> ZoneSimulation {
    let mut state = zone.snapshot().unwrap();
    let content = Arc::clone(zone.content());
    change(&mut state);
    ZoneSimulation::from_snapshot(state, content).unwrap()
}

/// A depths run `run_id` for `roster`, its chambers cleared and everyone in Thorn's chamber.
fn thorn_fight(roster: &[CharacterRecord], run_id: u64) -> ZoneSimulation {
    let mut zone = ZoneSimulation::with_content(SLOT, redbrand_depths::content()).unwrap();
    zone.begin_depths_run(DepthsRunStart {
        run_id,
        roster: roster
            .iter()
            .map(|record| record.character_id.clone())
            .collect(),
        cleared: ALL_CLEARED,
        checkpoint: true,
        outcome: None,
    })
    .unwrap();
    for (index, record) in roster.iter().enumerate() {
        let mut record = record.clone();
        let offset = i32::try_from(index).unwrap();
        record.location = CharacterLocation {
            zone_id: SLOT,
            position: [13_000, 90, -300 + offset * 150],
            facing: 16_384,
        };
        zone.admit_character(u32::try_from(index).unwrap() + 1, &record)
            .unwrap();
    }
    zone
}

/// Beats Thorn with real swings, and commits the outcome as the run directory would. Returns
/// the zone, the fight's progress events for player 1, and the party's records.
fn defeat_thorn(
    roster: &[CharacterRecord],
    run_id: u64,
) -> (ZoneSimulation, Vec<ZoneEvent>, Vec<CharacterRecord>) {
    let mut zone = thorn_fight(roster, run_id);
    tick(&mut zone, 2);
    for _ in 0..400 {
        if zone.depths_run().unwrap().boss.attempt.is_some() {
            break;
        }
        tick(&mut zone, 1);
    }
    assert!(zone.depths_run().unwrap().boss.attempt.is_some(), "engaged");
    zone = edit(zone, |state| {
        let thorn = state
            .creatures
            .iter_mut()
            .find(|creature| creature.creature_id == THORN)
            .unwrap();
        thorn.health = 1;
    });
    let target = EntityRef::Creature(THORN);
    let mut sequence: u32 = 1_000;
    let players = u32::try_from(roster.len()).unwrap();
    for _ in 0..1_500 {
        if zone.depths_run().unwrap().completed() {
            break;
        }
        for player in 1..=players {
            if let Some(command) = arena::swing_at(&zone, player, target) {
                sequence += 1;
                zone.apply_command(player, sequence, command).unwrap();
            }
        }
        zone = edit(zone, |state| {
            for player in &mut state.players {
                if player.position[0] < 13_100.0 {
                    player.position[0] = 13_100.0;
                }
            }
        });
        tick(&mut zone, 1);
    }
    assert!(zone.depths_run().unwrap().completed(), "Thorn fell");
    let thorn = zone
        .snapshot()
        .unwrap()
        .creatures
        .into_iter()
        .find(|creature| creature.creature_id == THORN)
        .unwrap();
    assert_ne!(thorn.life, CreatureLife::Alive);
    // Credit waits for the directory; the kill alone does not record evidence.
    let report = zone.boss_report().expect("a kill reports its outcome");
    zone.confirm_boss_outcome(report.run_id, report.outcome.version);
    zone.advance_tick().unwrap();
    let events = zone.snapshot_for_player(1).unwrap().events;
    let records = roster
        .iter()
        .enumerate()
        .map(|(index, record)| {
            zone.capture_character(
                u32::try_from(index).unwrap() + 1,
                record.character_id.clone(),
            )
            .unwrap()
        })
        .collect();
    (zone, events, records)
}

fn progress(sheet: &mmorpg_core::QuestSheet, quest: QuestId) -> Option<u8> {
    sheet
        .entries
        .iter()
        .find(|entry| entry.quest == quest)
        .map(|entry| entry.progress[0])
}

fn completed(events: &[ZoneEvent], quest: QuestId) -> bool {
    events
        .iter()
        .any(|event| matches!(event, ZoneEvent::QuestCompleted { quest: q } if *q == quest))
}

#[test]
fn the_captain_below_follows_garricks_quest_and_its_reward_is_its_own() {
    let content = greyhaven_vale::content();
    let quest = content.quest(quests::THE_CAPTAIN_BELOW).unwrap();
    assert_eq!(quest.name, "The Captain Below");
    assert_eq!(
        (quest.giver, quest.ender),
        (quests::MARSHAL, quests::MARSHAL)
    );
    assert_eq!(quest.prerequisite, Some(quests::GARRICK_REDBRAND_QUEST));
    assert_eq!(quest.objectives, vec![QuestObjective::DefeatThorn]);
    assert_eq!(quest.rewards.choices, vec![GREAVES]);
    assert_eq!(
        item_template(GREAVES).unwrap().name,
        "Marshal's Commendation Greaves"
    );
    let garrick = content.quest(quests::GARRICK_REDBRAND_QUEST).unwrap();
    assert!(
        garrick.rewards.choices.is_empty(),
        "Garrick's rewards are untouched"
    );
}

#[test]
fn completing_garrick_then_defeating_thorn_completes_the_follow_on_quest() {
    let mut greyhaven = at_marshal(&character("aria", 1));
    let copper = greyhaven.record().copper;
    assert!(completed(
        &greyhaven.turn_in(quests::GARRICK_REDBRAND_QUEST),
        quests::GARRICK_REDBRAND_QUEST
    ));
    assert_eq!(greyhaven.record().copper, copper + 60);

    greyhaven.accept(quests::THE_CAPTAIN_BELOW);
    assert_eq!(
        progress(&greyhaven.sheet(), quests::THE_CAPTAIN_BELOW),
        Some(0)
    );
    // Not yet: the Marshal holds the reward until Thorn is beaten.
    assert!(!completed(
        &greyhaven.turn_in(quests::THE_CAPTAIN_BELOW),
        quests::THE_CAPTAIN_BELOW
    ));
    assert_eq!(greyhaven.record().inventory.count(GREAVES), 0);

    let (depths, events, records) = defeat_thorn(&[greyhaven.record()], 7);
    assert!(records[0].depths.thorn_defeated);
    assert!(
        events.iter().any(|event| matches!(
            event,
            ZoneEvent::QuestProgress { quest, objective: 0, count: 1 }
                if *quest == quests::THE_CAPTAIN_BELOW
        )),
        "the victory is reported to the quest log in the depths"
    );
    assert_eq!(
        progress(
            &depths.snapshot_for_player(1).unwrap().quests.unwrap(),
            quests::THE_CAPTAIN_BELOW
        ),
        Some(1),
        "the depths' quest sheet shows the carried quest done"
    );
    assert_eq!(
        records[0]
            .quests
            .entry(quests::THE_CAPTAIN_BELOW)
            .unwrap()
            .progress,
        [0; 3],
        "progress lives in the evidence, not the log"
    );

    let mut greyhaven = at_marshal(&records[0]);
    assert_eq!(
        progress(&greyhaven.sheet(), quests::THE_CAPTAIN_BELOW),
        Some(1)
    );
    assert_eq!(greyhaven.marshal_marker(), Some(QuestMarker::Complete));
    let copper = greyhaven.record().copper;
    assert!(completed(
        &greyhaven.turn_in(quests::THE_CAPTAIN_BELOW),
        quests::THE_CAPTAIN_BELOW
    ));
    let record = greyhaven.record();
    assert_eq!(record.inventory.count(GREAVES), 1);
    assert_eq!(record.copper, copper + 120);
    assert!(record.quests.is_completed(quests::THE_CAPTAIN_BELOW));
    assert!(record.quests.entry(quests::THE_CAPTAIN_BELOW).is_none());
}

#[test]
fn an_early_victory_satisfies_the_objective_only_after_garricks_quest() {
    // Aria enters with a qualified friend and has not ended Garrick herself.
    let mut friend = character("bren", 1);
    friend.quests.entries.clear();
    friend.quests.completed = BEFORE_GARRICK | quests::GARRICK_REDBRAND_QUEST.bit().unwrap();
    let aria = character("aria", 0);
    let copper = aria.copper;

    let (_, _, records) = defeat_thorn(&[aria, friend], 7);
    let aria = records[0].clone();
    assert!(aria.depths.thorn_defeated, "the evidence is kept");
    assert!(!aria.quests.is_completed(quests::GARRICK_REDBRAND_QUEST));
    assert!(!aria.quests.is_completed(quests::THE_CAPTAIN_BELOW));
    assert!(aria.quests.entry(quests::THE_CAPTAIN_BELOW).is_none());
    assert_eq!(aria.inventory.count(GREAVES), 0);
    assert_eq!(aria.copper, copper, "no story money from the victory alone");

    let mut greyhaven = at_marshal(&aria);
    // The evidence neither unlocks the quest nor stands in for Garrick.
    greyhaven.accept(quests::THE_CAPTAIN_BELOW);
    assert!(
        greyhaven
            .record()
            .quests
            .entry(quests::THE_CAPTAIN_BELOW)
            .is_none()
    );
    assert!(!completed(
        &greyhaven.turn_in(quests::GARRICK_REDBRAND_QUEST),
        quests::GARRICK_REDBRAND_QUEST
    ));

    // She slays Garrick herself and turns his quest in.
    let mut record = greyhaven.record();
    record.quests.entries[0].progress[0] = 1;
    let mut greyhaven = at_marshal(&record);
    assert!(completed(
        &greyhaven.turn_in(quests::GARRICK_REDBRAND_QUEST),
        quests::GARRICK_REDBRAND_QUEST
    ));
    let record = greyhaven.record();
    assert_eq!(record.copper, copper + 60);
    assert!(
        record.quests.entry(quests::THE_CAPTAIN_BELOW).is_none(),
        "no automatic acceptance"
    );
    assert!(
        !record.quests.is_completed(quests::THE_CAPTAIN_BELOW),
        "no automatic turn-in"
    );
    assert_eq!(record.inventory.count(GREAVES), 0);

    // Accepted, the saved victory satisfies the objective with no new fight.
    greyhaven.accept(quests::THE_CAPTAIN_BELOW);
    assert_eq!(
        progress(&greyhaven.sheet(), quests::THE_CAPTAIN_BELOW),
        Some(1)
    );
    assert_eq!(greyhaven.marshal_marker(), Some(QuestMarker::Complete));
    let record = greyhaven.record();
    assert_eq!(record.inventory.count(GREAVES), 0, "turn-in stays manual");
    assert!(!record.quests.is_completed(quests::THE_CAPTAIN_BELOW));

    assert!(completed(
        &greyhaven.turn_in(quests::THE_CAPTAIN_BELOW),
        quests::THE_CAPTAIN_BELOW
    ));
    let record = greyhaven.record();
    assert_eq!(record.inventory.count(GREAVES), 1);
    assert_eq!(record.copper, copper + 60 + 120);
}

#[test]
fn rewards_are_granted_once_across_recovery_and_repeated_entry() {
    let aria = character("aria", 1);
    let mut greyhaven = at_marshal(&aria);
    greyhaven.turn_in(quests::GARRICK_REDBRAND_QUEST);
    greyhaven.accept(quests::THE_CAPTAIN_BELOW);
    let (_, _, records) = defeat_thorn(&[greyhaven.record()], 7);

    let mut greyhaven = at_marshal(&records[0]);
    greyhaven.recover();
    let copper = greyhaven.record().copper;
    let experience = greyhaven.record().experience;
    assert!(completed(
        &greyhaven.turn_in(quests::THE_CAPTAIN_BELOW),
        quests::THE_CAPTAIN_BELOW
    ));
    let settled = greyhaven.record();
    assert_eq!(settled.inventory.count(GREAVES), 1);
    assert_eq!(settled.copper, copper + 120);
    assert!(settled.experience > experience || settled.level > records[0].level);

    // A retried turn-in, before and after recovery, changes nothing.
    assert!(!completed(
        &greyhaven.turn_in(quests::THE_CAPTAIN_BELOW),
        quests::THE_CAPTAIN_BELOW
    ));
    greyhaven.recover();
    assert!(!completed(
        &greyhaven.turn_in(quests::THE_CAPTAIN_BELOW),
        quests::THE_CAPTAIN_BELOW
    ));
    greyhaven.accept(quests::THE_CAPTAIN_BELOW);
    assert_eq!(greyhaven.record().inventory.count(GREAVES), 1);
    assert_eq!(greyhaven.record().copper, copper + 120);
    assert!(
        greyhaven
            .record()
            .quests
            .entry(quests::THE_CAPTAIN_BELOW)
            .is_none()
    );

    // A fresh run and its repeatable boss rewards leave the story reward alone.
    let (depths, _, again) = defeat_thorn(&[greyhaven.record()], 9);
    assert_eq!(again[0].depths.receipt_run, 9);
    assert!(again[0].depths.thorn_defeated, "the evidence stays");
    assert_eq!(again[0].inventory.count(GREAVES), 1);
    assert!(
        !depths
            .depths_run()
            .unwrap()
            .boss
            .outcome
            .as_ref()
            .unwrap()
            .drops
            .iter()
            .any(|drop| drop.item == GREAVES),
        "Thorn's repeatable pool never holds the story reward"
    );
    let mut back = at_marshal(&again[0]);
    assert!(!completed(
        &back.turn_in(quests::THE_CAPTAIN_BELOW),
        quests::THE_CAPTAIN_BELOW
    ));
    assert!(!completed(
        &back.turn_in(quests::GARRICK_REDBRAND_QUEST),
        quests::GARRICK_REDBRAND_QUEST
    ));
    let record = back.record();
    assert_eq!(record.inventory.count(GREAVES), 1);
    assert!(record.quests.is_completed(quests::GARRICK_REDBRAND_QUEST));
    assert!(record.quests.is_completed(quests::THE_CAPTAIN_BELOW));
}
