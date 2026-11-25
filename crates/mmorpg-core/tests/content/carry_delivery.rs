//! Carried loads (#1077) and the Core Vale's first delivery, *The Last Sack* (#1019 slot 43): the
//! zone lets a player lift the last sack off the Wolfrun cart only for an open delivery, keeps
//! both hands busy while it is carried (no weapon, ability, roll or second load), lets it be set
//! down and lifted again, and takes it from the player's arms at Farmer Osric Mill's. A lost load
//! is fetched again from the cart; the load survives a canonical snapshot and is set down when its
//! carrier leaves, and the saved character never holds it.
use crate::support;

use std::sync::Arc;

use mmorpg_core::greyhaven_vale::{self, quests};
use mmorpg_core::{
    CharacterId, CharacterLocation, ErrorCode, LoadKind, LoadSpotId, NpcId, PLACED_LOAD_TICKS,
    QuestLog, ZoneCommand, ZoneEvent, ZoneId, ZoneSimulation,
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

    fn lift(&mut self, source: Option<LoadSpotId>) -> Vec<ZoneEvent> {
        self.send(ZoneCommand::Lift { source })
    }

    /// Moves the player's body to stand at `[x, z]`, as walking there would.
    fn stand_at(&mut self, [x, z]: [i32; 2]) {
        let y = self.zone.content().definition().ground_at(x, z) + 90;
        let mut state = self.zone.snapshot().unwrap();
        state.players[0].position = [x as f32, y as f32, z as f32];
        state.players[0].velocity = [0.; 3];
        let content = Arc::clone(self.zone.content());
        self.zone = ZoneSimulation::from_snapshot(state, content).unwrap();
        for _ in 0..3 {
            self.zone.advance_tick().unwrap();
        }
    }

    fn beside_cart(&mut self) {
        let at = self.zone.content().load_spot(quests::CART_SACK).unwrap().at;
        self.stand_at([at[0] + 150, at[2] + 150]);
    }

    fn beside(&mut self, npc: NpcId) {
        let [x, z] = self.zone.npc_placement(npc).unwrap().position;
        self.stand_at([x + 150, z + 150]);
    }

    fn carried(&self) -> Option<LoadKind> {
        self.zone.carried_load(1)
    }

    /// What the viewer's own projected record says it carries.
    fn projected(&self) -> Option<LoadKind> {
        self.zone.snapshot_for_player(1).unwrap().entities[0]
            .flags
            .load
    }

    fn log(&self) -> QuestLog {
        self.zone
            .capture_character(1, CharacterId::new("porter").unwrap())
            .unwrap()
            .quests
    }
}

fn refusal(events: &[ZoneEvent]) -> Option<ErrorCode> {
    events.iter().find_map(|event| match event {
        ZoneEvent::Error { code, .. } => Some(*code),
        _ => None,
    })
}

/// A level 3 Warden who has read Hilda's ledger (*The Tanner's Ledger*), beside the Tanner.
fn porter() -> Player {
    let mut donor = ZoneSimulation::with_content(GREYHAVEN, greyhaven_vale::content()).unwrap();
    donor.add_player(1).unwrap();
    let mut donor = classes::at_level(donor, 1, 3);
    donor
        .apply_command(1, 1, ZoneCommand::ChooseClass { class: 0, sex: 0 })
        .unwrap();
    donor.advance_tick().unwrap();
    let mut record = donor
        .capture_character(1, CharacterId::new("porter").unwrap())
        .unwrap();
    let content = greyhaven_vale::content();
    // The Tanner's Ledger and every quest before it, turned in.
    let mut completed = std::collections::BTreeSet::new();
    let mut next = Some(quests::THE_TANNERS_LEDGER);
    while let Some(quest) = next {
        completed.insert(quest);
        next = content.quest(quest).unwrap().prerequisite;
    }
    record.quests = QuestLog {
        entries: Vec::new(),
        completed,
        outcomes: Default::default(),
    };
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

/// A porter with *The Last Sack* accepted from Hilda.
fn porter_on_the_last_sack() -> Player {
    let mut player = porter();
    let events = player.send(ZoneCommand::AcceptQuest {
        npc: quests::TANNER,
        quest: quests::THE_LAST_SACK.get(),
    });
    assert_eq!(refusal(&events), None, "{events:?}");
    player
}

#[test]
fn the_last_sack_is_carried_from_the_wolfrun_cart_to_osric() {
    let mut player = porter();
    // Before the quest the cart hands nobody a sack.
    player.beside_cart();
    assert_eq!(
        refusal(&player.lift(Some(quests::CART_SACK))),
        Some(ErrorCode::InvalidQuest)
    );
    player.beside(quests::TANNER);
    let events = player.send(ZoneCommand::AcceptQuest {
        npc: quests::TANNER,
        quest: quests::THE_LAST_SACK.get(),
    });
    assert_eq!(refusal(&events), None, "{events:?}");
    // From the Outpost the cart is out of reach, and an unknown source is no target.
    assert_eq!(
        refusal(&player.lift(Some(quests::CART_SACK))),
        Some(ErrorCode::OutOfRange)
    );
    assert_eq!(
        refusal(&player.lift(Some(LoadSpotId::new(999)))),
        Some(ErrorCode::InvalidTarget)
    );
    assert_eq!(player.carried(), None);

    // At the cart the sack goes into both arms, and the projection shows it.
    player.beside_cart();
    let events = player.lift(Some(quests::CART_SACK));
    assert_eq!(refusal(&events), None, "{events:?}");
    assert_eq!(player.carried(), Some(LoadKind::Sack));
    assert_eq!(player.projected(), Some(LoadKind::Sack));
    // One load at a time.
    assert_eq!(
        refusal(&player.lift(Some(quests::CART_SACK))),
        Some(ErrorCode::HandsFull)
    );

    // Osric takes it from the porter's arms; the delivery is done and the load gone.
    player.beside(quests::OSRIC_MILL);
    let events = player.send(ZoneCommand::QuestAction {
        npc: quests::OSRIC_MILL,
    });
    assert_eq!(refusal(&events), None, "{events:?}");
    assert_eq!(player.carried(), None);
    assert!(player.zone.load(1).is_none());
    assert_eq!(
        player.log().entry(quests::THE_LAST_SACK).unwrap().progress[0],
        1
    );
    let events = player.send(ZoneCommand::CompleteQuest {
        npc: quests::OSRIC_MILL,
        quest: quests::THE_LAST_SACK.get(),
        choice: 0,
    });
    assert_eq!(refusal(&events), None, "{events:?}");
    assert!(player.log().is_completed(quests::THE_LAST_SACK));
    // Done, the cart has nothing more for this porter.
    player.beside_cart();
    assert_eq!(
        refusal(&player.lift(Some(quests::CART_SACK))),
        Some(ErrorCode::InvalidQuest)
    );
}

#[test]
fn both_hands_are_busy_while_a_load_is_carried() {
    let mut player = porter_on_the_last_sack();
    player.beside_cart();
    // Lifting sheathes a drawn weapon.
    player.send(ZoneCommand::SetWeaponDrawn(true));
    assert!(
        player
            .zone
            .snapshot_for_player(1)
            .unwrap()
            .viewer
            .weapon_drawn
    );
    let events = player.lift(Some(quests::CART_SACK));
    assert_eq!(refusal(&events), None, "{events:?}");
    assert!(
        !player
            .zone
            .snapshot_for_player(1)
            .unwrap()
            .viewer
            .weapon_drawn
    );

    // No weapon, guard, swing, ability, seat or dodge roll while carrying, each refused with
    // the actionable reason.
    for command in [
        ZoneCommand::SetWeaponDrawn(true),
        ZoneCommand::Swing { facing: 0 },
        ZoneCommand::UseAbility {
            ability: 1,
            target: None,
            facing: None,
        },
        ZoneCommand::Sit { seat: None },
        ZoneCommand::Roll,
    ] {
        let events = player.send(command);
        assert_eq!(
            refusal(&events),
            Some(ErrorCode::HandsFull),
            "{command:?}: {events:?}"
        );
    }
    let snapshot = player.zone.snapshot_for_player(1).unwrap();
    assert!(!snapshot.viewer.weapon_drawn);
    assert_eq!(
        snapshot.entities[0].flags.traversal,
        mmorpg_core::TraversalView::Free
    );
    assert_eq!(player.carried(), Some(LoadKind::Sack));
}

#[test]
fn a_set_down_sack_is_lifted_again_or_fetched_anew() {
    let mut player = porter_on_the_last_sack();
    // Nothing to set down yet.
    assert_eq!(
        refusal(&player.send(ZoneCommand::SetDown)),
        Some(ErrorCode::NotCarrying)
    );
    player.beside_cart();
    player.lift(Some(quests::CART_SACK));
    let events = player.send(ZoneCommand::SetDown);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert_eq!(player.carried(), None);
    let placed = player.zone.load(1).unwrap().placed.unwrap();
    // Everyone nearby sees it on the ground; this porter may lift it again.
    let loads = player.zone.snapshot_for_player(1).unwrap().loads;
    assert_eq!(loads.len(), 1);
    assert_eq!(
        (loads[0].kind, loads[0].own, loads[0].at),
        (LoadKind::Sack, true, placed.at)
    );

    // Osric takes no empty arms.
    player.beside(quests::OSRIC_MILL);
    assert_eq!(
        refusal(&player.send(ZoneCommand::QuestAction {
            npc: quests::OSRIC_MILL
        })),
        Some(ErrorCode::NotCarrying)
    );
    // Lifted again only beside it.
    assert_eq!(refusal(&player.lift(None)), Some(ErrorCode::OutOfRange));
    player.stand_at([placed.at[0] + 80, placed.at[2]]);
    let events = player.lift(None);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert_eq!(player.carried(), Some(LoadKind::Sack));

    // Set down and left too long, it is gone; the cart has another, so the quest goes on.
    player.send(ZoneCommand::SetDown);
    for _ in 0..PLACED_LOAD_TICKS {
        player.zone.advance_tick().unwrap();
    }
    assert!(player.zone.load(1).is_none());
    assert_eq!(refusal(&player.lift(None)), Some(ErrorCode::InvalidTarget));
    player.beside_cart();
    let events = player.lift(Some(quests::CART_SACK));
    assert_eq!(refusal(&events), None, "{events:?}");
    assert_eq!(player.carried(), Some(LoadKind::Sack));

    // Abandoning the delivery takes the load away with it.
    player.send(ZoneCommand::AbandonQuest {
        quest: quests::THE_LAST_SACK.get(),
    });
    assert!(player.zone.load(1).is_none());
}

#[test]
fn a_carried_load_replays_from_a_snapshot_and_is_set_down_at_logout() {
    let mut player = porter_on_the_last_sack();
    player.beside_cart();
    player.lift(Some(quests::CART_SACK));
    // A restored zone carries the load on, tick for tick like the original.
    let state = player.zone.snapshot().unwrap();
    assert_eq!(state.loads.len(), 1);
    let mut restored =
        ZoneSimulation::from_snapshot(state.clone(), Arc::clone(player.zone.content())).unwrap();
    assert_eq!(restored.snapshot().unwrap(), state);
    for zone in [&mut player.zone, &mut restored] {
        zone.apply_command(1, 100, ZoneCommand::SetDown).unwrap();
        for _ in 0..5 {
            zone.advance_tick().unwrap();
        }
    }
    assert_eq!(
        restored.snapshot().unwrap(),
        player.zone.snapshot().unwrap()
    );
    player
        .zone
        .apply_command(1, 101, ZoneCommand::Lift { source: None })
        .unwrap();
    player.zone.advance_tick().unwrap();
    assert_eq!(player.carried(), Some(LoadKind::Sack));
    player.sequence = 101;

    // The saved character holds no load; leaving sets it down where the porter stood.
    let record = player
        .zone
        .capture_character(1, CharacterId::new("porter").unwrap())
        .unwrap();
    assert!(player.zone.remove_player(1));
    let placed = player.zone.load(1).unwrap().placed.unwrap();
    // Back in the zone, the porter finds the sack where it was left.
    player.zone.admit_character(1, &record).unwrap();
    player.zone.advance_tick().unwrap();
    assert_eq!(player.carried(), None);
    player.stand_at([placed.at[0] + 80, placed.at[2]]);
    let events = player.lift(None);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert_eq!(player.carried(), Some(LoadKind::Sack));
}
