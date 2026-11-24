//! The Core Vale's evidence quests (#1019): Hilda sends a player back to the Wolfrun cart to
//! inspect its cut sacks, bait and Redbrand marker one by one. Inspection is a server-checked
//! intent: out of reach, unknown or unneeded evidence is refused and changes nothing, each
//! objective is credited once, and the credit survives a canonical snapshot and a saved
//! character like any stored quest progress.
use crate::support;

use std::sync::Arc;

use mmorpg_core::greyhaven_vale::{self, quests};
use mmorpg_core::{
    CharacterId, CharacterLocation, ErrorCode, EvidenceId, QuestEntry, QuestLog, QuestObjective,
    ZoneCommand, ZoneEvent, ZoneId, ZoneSimulation,
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

    fn inspect(&mut self, spot: EvidenceId) -> Vec<ZoneEvent> {
        self.send(ZoneCommand::Inspect { spot: spot.get() })
    }

    /// Moves the player's body to stand at `[x, z]`, as walking there would.
    fn stand_at(&mut self, [x, z]: [i32; 2]) {
        let y = self.zone.content().definition().ground_at(x, z) + 90;
        let mut state = self.zone.snapshot().unwrap();
        state.players[0].position = [x as f32, y as f32, z as f32];
        let content = Arc::clone(self.zone.content());
        self.zone = ZoneSimulation::from_snapshot(state, content).unwrap();
        self.zone.advance_tick().unwrap();
    }

    /// Stands beside the evidence's spot, within its reach.
    fn beside_evidence(&mut self, spot: EvidenceId) {
        let at = self.zone.content().evidence_spot(spot).unwrap().at;
        self.stand_at([at[0] + 60, at[2] + 60]);
    }

    fn beside(&mut self, npc: mmorpg_core::NpcId) {
        let [x, z] = self.zone.npc_placement(npc).unwrap().position;
        self.stand_at([x + 150, z + 150]);
    }

    fn log(&self) -> QuestLog {
        self.zone
            .capture_character(1, CharacterId::new("reader").unwrap())
            .unwrap()
            .quests
    }

    fn progress(&self, quest: mmorpg_core::QuestId) -> [u8; mmorpg_core::MAX_QUEST_OBJECTIVES] {
        self.log().entry(quest).unwrap().progress
    }
}

fn refusal(events: &[ZoneEvent]) -> Option<ErrorCode> {
    events.iter().find_map(|event| match event {
        ZoneEvent::Error { code, .. } => Some(*code),
        _ => None,
    })
}

/// A level 3 character with the saved quest `log`, beside the Tanner.
fn reader_with(log: QuestLog) -> Player {
    let mut donor = ZoneSimulation::with_content(GREYHAVEN, greyhaven_vale::content()).unwrap();
    donor.add_player(1).unwrap();
    let mut donor = classes::at_level(donor, 1, 3);
    donor
        .apply_command(1, 1, ZoneCommand::ChooseClass { class: 0, sex: 0 })
        .unwrap();
    donor.advance_tick().unwrap();
    let mut record = donor
        .capture_character(1, CharacterId::new("reader").unwrap())
        .unwrap();
    record.quests = log;
    let content = greyhaven_vale::content();
    let [x, z] = content.npc(quests::TANNER).unwrap().position;
    record.location = CharacterLocation {
        zone_id: GREYHAVEN,
        position: [x + 100, 90, z + 100],
        facing: 0,
    };
    let mut zone = ZoneSimulation::with_content(GREYHAVEN, content).unwrap();
    zone.admit_character(1, &record).unwrap();
    zone.advance_tick().unwrap();
    let mut player = Player { zone, sequence: 10 };
    player.beside(quests::TANNER);
    player
}

/// One who has brought Hilda her sacks back (*The Wolfrun Cart*).
fn reader() -> Player {
    reader_with(QuestLog {
        entries: Vec::new(),
        completed: [quests::THE_WOLFRUN_CART].into(),
    })
}

#[test]
fn reading_the_wolfrun_carts_signs_one_by_one_completes_cut_sacks_red_rags() {
    let mut player = reader();
    // The cart's evidence counts for nobody before the quest: inspecting it is refused.
    player.beside_evidence(quests::CUT_SACKS);
    assert_eq!(
        refusal(&player.inspect(quests::CUT_SACKS)),
        Some(ErrorCode::InvalidQuest)
    );

    player.beside(quests::TANNER);
    let events = player.send(ZoneCommand::AcceptQuest {
        npc: quests::TANNER,
        quest: quests::CUT_SACKS_RED_RAGS.get(),
    });
    assert_eq!(refusal(&events), None, "{events:?}");
    // From the Outpost the cart's sacks are out of reach, and unknown evidence is no target.
    assert_eq!(
        refusal(&player.inspect(quests::CUT_SACKS)),
        Some(ErrorCode::OutOfRange)
    );
    assert_eq!(
        refusal(&player.inspect(EvidenceId::new(999))),
        Some(ErrorCode::InvalidTarget)
    );
    assert_eq!(player.progress(quests::CUT_SACKS_RED_RAGS), [0, 0, 0]);

    // At the sacks, inspecting them credits only their objective, once.
    player.beside_evidence(quests::CUT_SACKS);
    let events = player.inspect(quests::CUT_SACKS);
    assert!(
        events.contains(&ZoneEvent::QuestProgress {
            quest: quests::CUT_SACKS_RED_RAGS,
            objective: 0,
            count: 1,
        }),
        "{events:?}"
    );
    assert_eq!(player.progress(quests::CUT_SACKS_RED_RAGS), [1, 0, 0]);
    assert_eq!(
        refusal(&player.inspect(quests::CUT_SACKS)),
        Some(ErrorCode::InvalidQuest)
    );
    assert_eq!(player.progress(quests::CUT_SACKS_RED_RAGS), [1, 0, 0]);

    // The credit is stored progress: a canonical snapshot and a saved character carry it.
    let state = player.zone.snapshot().unwrap();
    let content = Arc::clone(player.zone.content());
    player.zone = ZoneSimulation::from_snapshot(state, content).unwrap();
    assert_eq!(player.progress(quests::CUT_SACKS_RED_RAGS), [1, 0, 0]);
    let mut player = reader_with(player.log());
    assert_eq!(player.progress(quests::CUT_SACKS_RED_RAGS), [1, 0, 0]);

    for (index, spot) in [(1, quests::BAIT_RACK), (2, quests::REDBRAND_MARKER)] {
        player.beside_evidence(spot);
        let events = player.inspect(spot);
        assert_eq!(refusal(&events), None, "{events:?}");
        assert_eq!(player.progress(quests::CUT_SACKS_RED_RAGS)[index], 1);
    }

    player.beside(quests::TANNER);
    let events = player.send(ZoneCommand::CompleteQuest {
        npc: quests::TANNER,
        quest: quests::CUT_SACKS_RED_RAGS.get(),
        choice: 0,
    });
    assert_eq!(refusal(&events), None, "{events:?}");
    let log = player.log();
    assert!(
        log.completed.contains(&quests::CUT_SACKS_RED_RAGS),
        "{log:?}"
    );
    // Hilda's two leads open: the bait's trail and her ledger.
    for quest in [quests::THE_BAIT_TRAIL, quests::THE_TANNERS_LEDGER] {
        let events = player.send(ZoneCommand::AcceptQuest {
            npc: quests::TANNER,
            quest: quest.get(),
        });
        assert_eq!(refusal(&events), None, "{events:?}");
    }
    assert_eq!(
        player.log().entries,
        [
            QuestEntry {
                quest: quests::THE_BAIT_TRAIL,
                progress: [0, 0, 0],
            },
            QuestEntry {
                quest: quests::THE_TANNERS_LEDGER,
                progress: [0, 0, 0],
            },
        ]
    );
}

/// The ledger quest mixes evidence with a talk: Hilda's ledger at her stall, Osric at Millbrook
/// and the tally on his delivery crates, then back to Hilda.
#[test]
fn the_tanners_ledger_is_read_at_both_ends_of_the_millbrook_road() {
    let mut player = reader_with(QuestLog {
        entries: vec![QuestEntry {
            quest: quests::THE_TANNERS_LEDGER,
            progress: [0, 0, 0],
        }],
        completed: [quests::THE_WOLFRUN_CART, quests::CUT_SACKS_RED_RAGS].into(),
    });
    player.beside_evidence(quests::HILDAS_LEDGER);
    assert_eq!(refusal(&player.inspect(quests::HILDAS_LEDGER)), None);
    player.beside(quests::FARMER);
    player.beside_evidence(quests::DELIVERY_TALLY);
    assert_eq!(refusal(&player.inspect(quests::DELIVERY_TALLY)), None);
    assert_eq!(player.progress(quests::THE_TANNERS_LEDGER), [1, 1, 1]);
    player.beside(quests::TANNER);
    let events = player.send(ZoneCommand::CompleteQuest {
        npc: quests::TANNER,
        quest: quests::THE_TANNERS_LEDGER.get(),
        choice: 0,
    });
    assert_eq!(refusal(&events), None, "{events:?}");
    assert!(player.log().is_completed(quests::THE_TANNERS_LEDGER));
}

/// Every evidence spot a quest names is drawn: the world dressing places art within a metre of
/// it, standing on the ground there, so both clients show what the player inspects.
#[test]
fn every_evidence_spot_has_its_art_and_its_quest() {
    #[derive(serde::Deserialize)]
    struct Placement {
        id: String,
        position: [i32; 3],
    }
    #[derive(serde::Deserialize)]
    struct Dressing {
        placements: Vec<Placement>,
    }
    let placements: Vec<Placement> = [
        include_str!("../../../../content/greyhaven/dressing/wolfrun-cart.json"),
        include_str!("../../../../content/greyhaven/dressing/evidence.json"),
    ]
    .into_iter()
    .flat_map(|source| serde_json::from_str::<Dressing>(source).unwrap().placements)
    .collect();
    let content = greyhaven_vale::content();
    // The 32 quests before #1019, slice 1's three evidence quests and slice 2's seven (35, 37–42).
    assert_eq!(content.quests().len(), 42);
    assert_eq!(content.evidence().len(), 8);
    for spot in content.evidence() {
        let art = placements.iter().find(|placement| {
            let (dx, dz) = (
                f64::from(placement.position[0] - spot.at[0]),
                f64::from(placement.position[2] - spot.at[2]),
            );
            dx.hypot(dz) <= 100.0
        });
        assert!(art.is_some(), "{}: no art within 1 m", spot.name);
        let ground = content.definition().ground_at(spot.at[0], spot.at[2]);
        assert!(
            (spot.at[1] - ground).abs() <= 50,
            "{} ({}) floats off the ground at {ground}",
            spot.name,
            art.unwrap().id
        );
        let named = content.quests().iter().any(|quest| {
            quest
                .objectives
                .contains(&QuestObjective::Inspect { spot: spot.id })
        });
        assert!(named, "{}: no quest inspects it", spot.name);
    }
}
