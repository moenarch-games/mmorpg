//! The farmland's delivery, repair and village stories (#1019, slice 4a): the watermill's choked
//! sluice (51), a grain sack carried to Miller Hanne (52), Shepherd Lise's dispute with Old Bertil
//! settled with evidence or the player's own copper (54), the hurdles of her fold (56), Bertil's
//! locket found from landmark directions alone (57) and bread carried up to the tower's watch (58).
//!
//! Every step is a server-checked command; refusals change nothing, each objective is credited
//! once, and every `stand_at` restores the zone from its canonical snapshot, so recovery carries
//! the sluice, the loads, the hurdles and the decision.
use crate::support;

use std::collections::BTreeMap;
use std::sync::Arc;

use mmorpg_core::greyhaven_vale::{self, hamlet_stories as hamlet, quests};
use mmorpg_core::{
    CharacterId, CharacterLocation, ErrorCode, EvidenceId, ItemId, LoadKind, LoadSpotId, NpcId,
    PieceId, QuestId, QuestLog, SEAL_OPEN_TICKS, ZoneCommand, ZoneEvent, ZoneId, ZoneSimulation,
    sell_price,
};
use support::classes;

const GREYHAVEN: ZoneId = ZoneId::new(1);
const WARDEN_CLASS: u8 = 0;
const OSRIC: NpcId = NpcId::new(4);
const ENA: NpcId = NpcId::new(36);
const BERTIL: NpcId = NpcId::new(38);
const LISE: NpcId = NpcId::new(39);
const HANNE: NpcId = NpcId::new(40);
const LOCKET: ItemId = ItemId::new(43);
const MILLRACE: u32 = 11;
const GATE_TRACKS: u32 = 12;
const LOOSE_STONE: u32 = 13;
const ENAS_SACK: LoadSpotId = LoadSpotId::new(3);
const LOAVES: LoadSpotId = LoadSpotId::new(5);
const SLUICE: QuestId = QuestId::new(51);
const HANNES_SACK: QuestId = QuestId::new(52);
const BARGAIN: QuestId = QuestId::new(54);
const FENCE: QuestId = QuestId::new(56);
const HEIRLOOM: QuestId = QuestId::new(57);
const BREAD: QuestId = QuestId::new(58);

struct World {
    zone: ZoneSimulation,
    sequences: BTreeMap<u32, u32>,
}

impl World {
    /// Characters `(player, turned-in quests)` admitted at level 6 beside the Marshal.
    fn new(characters: &[(u32, Vec<QuestId>)]) -> Self {
        let content = greyhaven_vale::content();
        let mut zone = ZoneSimulation::with_content(GREYHAVEN, Arc::clone(&content)).unwrap();
        for (player, completed) in characters {
            let mut donor = ZoneSimulation::with_content(GREYHAVEN, Arc::clone(&content)).unwrap();
            donor.add_player(*player).unwrap();
            let mut donor = classes::at_level(donor, *player, 6);
            donor
                .apply_command(
                    *player,
                    1,
                    ZoneCommand::ChooseClass {
                        class: WARDEN_CLASS,
                        sex: 0,
                    },
                )
                .unwrap();
            donor.advance_tick().unwrap();
            let mut record = donor.capture_character(*player, id(*player)).unwrap();
            record.quests = QuestLog {
                completed: completed.iter().copied().collect(),
                ..QuestLog::default()
            };
            let [x, z] = content.npc(quests::MARSHAL).unwrap().position;
            record.location = CharacterLocation {
                zone_id: GREYHAVEN,
                position: [x + 100, 90, z + 100],
                facing: 0,
            };
            zone.admit_character(*player, &record).unwrap();
        }
        zone.advance_tick().unwrap();
        Self {
            zone,
            sequences: characters.iter().map(|c| (c.0, 10)).collect(),
        }
    }

    fn send(&mut self, player: u32, command: ZoneCommand) -> Vec<ZoneEvent> {
        let sequence = self.sequences.entry(player).or_default();
        *sequence += 1;
        self.zone.apply_command(player, *sequence, command).unwrap();
        self.zone.advance_tick().unwrap();
        self.zone.snapshot_for_player(player).unwrap().events
    }

    fn accept(&mut self, player: u32, npc: NpcId, quest: QuestId) {
        self.beside(player, npc);
        let events = self.send(
            player,
            ZoneCommand::AcceptQuest {
                npc,
                quest: quest.get(),
            },
        );
        assert_eq!(refusal(&events), None, "accepting {quest:?}: {events:?}");
    }

    fn turn_in(&mut self, player: u32, npc: NpcId, quest: QuestId, choice: u8) -> Vec<ZoneEvent> {
        self.beside(player, npc);
        self.send(
            player,
            ZoneCommand::CompleteQuest {
                npc,
                quest: quest.get(),
                choice,
            },
        )
    }

    /// Changes the canonical state and restores the zone from it, as recovery would.
    fn restore(&mut self, change: impl FnOnce(&mut mmorpg_core::CanonicalZoneSnapshot)) {
        let mut state = self.zone.snapshot().unwrap();
        change(&mut state);
        let content = Arc::clone(self.zone.content());
        self.zone = ZoneSimulation::from_snapshot(state, content).unwrap();
    }

    /// Moves `player`'s body to stand at `[x, z]`.
    fn stand_at(&mut self, player: u32, [x, z]: [i32; 2]) {
        let y = self.zone.content().definition().ground_at(x, z) + 90;
        self.restore(|state| {
            let record = state
                .players
                .iter_mut()
                .find(|record| record.player_id == player)
                .unwrap();
            record.position = [x as f32, y as f32, z as f32];
            record.velocity = [0.0; 3];
        });
        self.zone.advance_tick().unwrap();
    }

    fn beside(&mut self, player: u32, npc: NpcId) {
        let [x, z] = self.zone.npc_placement(npc).unwrap().position;
        self.stand_at(player, [x + 100, z + 100]);
    }

    fn at_spot(&mut self, player: u32, at: [i32; 3]) {
        self.stand_at(player, [at[0], at[2]]);
    }

    fn evidence(&self, spot: u32) -> [i32; 3] {
        self.zone
            .content()
            .evidence_spot(EvidenceId::new(spot))
            .unwrap()
            .at
    }

    fn inspect(&mut self, player: u32, spot: u32) -> Vec<ZoneEvent> {
        let at = self.evidence(spot);
        self.at_spot(player, at);
        self.send(player, ZoneCommand::Inspect { spot })
    }

    fn lift(&mut self, player: u32, source: LoadSpotId) -> Vec<ZoneEvent> {
        let [x, _, z] = self.zone.content().load_spot(source).unwrap().at;
        self.stand_at(player, [x + 100, z + 100]);
        self.send(
            player,
            ZoneCommand::Lift {
                source: Some(source),
            },
        )
    }

    fn log(&self, player: u32) -> QuestLog {
        self.zone
            .capture_character(player, id(player))
            .unwrap()
            .quests
    }

    fn progress(&self, player: u32, quest: QuestId) -> Vec<u8> {
        let objectives = self.zone.content().quest(quest).unwrap().objectives.len();
        self.log(player).entry(quest).unwrap().progress[..objectives].to_vec()
    }

    fn copper(&self, player: u32) -> u32 {
        self.zone
            .capture_character(player, id(player))
            .unwrap()
            .copper
    }

    fn carries(&self, player: u32, item: ItemId) -> u32 {
        self.zone
            .capture_character(player, id(player))
            .unwrap()
            .inventory
            .count(item)
    }

    fn ticks(&mut self, count: u32) {
        for _ in 0..count {
            self.zone.advance_tick().unwrap();
        }
    }

    /// A reloaded character: saved, then admitted into a fresh zone at the same spot.
    fn reload(&mut self, player: u32) {
        let record = self.zone.capture_character(player, id(player)).unwrap();
        let mut zone =
            ZoneSimulation::with_content(GREYHAVEN, Arc::clone(self.zone.content())).unwrap();
        zone.admit_character(player, &record).unwrap();
        zone.advance_tick().unwrap();
        self.zone = zone;
    }
}

fn id(player: u32) -> CharacterId {
    CharacterId::new(format!("hamlet-{player}")).unwrap()
}

fn refusal(events: &[ZoneEvent]) -> Option<ErrorCode> {
    events.iter().find_map(|event| match event {
        ZoneEvent::Error { code, .. } => Some(*code),
        _ => None,
    })
}

/// Quests 1 to 32 (the opening, the region and the hamlets) turned in, and `extra`.
fn hamlets_done(extra: &[u32]) -> Vec<QuestId> {
    (1..=32)
        .chain(extra.iter().copied())
        .map(QuestId::new)
        .collect()
}

#[test]
fn the_new_spots_stand_on_their_ground_and_the_watchman_keeps_the_tower() {
    let content = greyhaven_vale::content();
    let ground = |at: [i32; 3]| content.definition().ground_at(at[0], at[2]);
    for spot in [MILLRACE, GATE_TRACKS, LOOSE_STONE] {
        let spot = content.evidence_spot(EvidenceId::new(spot)).unwrap();
        assert!(
            (spot.at[1] - ground(spot.at)).abs() <= 50,
            "{} at {:?}: ground {}",
            spot.name,
            spot.at,
            ground(spot.at)
        );
    }
    for source in 3..=5 {
        let spot = content.load_spot(LoadSpotId::new(source)).unwrap();
        assert!(
            (spot.at[1] - ground(spot.at)).abs() <= 50,
            "{} at {:?}: ground {}",
            spot.name,
            spot.at,
            ground(spot.at)
        );
    }
    let gate = content.mechanism(hamlet::SLUICE_GATE).unwrap();
    assert_eq!(gate.at[1], ground(gate.at));
    // Of the farmland's spots only the locket's stone is hidden, and it carries its directions;
    // Fenford's ford bed and alder roots (27, 31) are the only others (#1019).
    let hidden: Vec<_> = content
        .evidence()
        .iter()
        .filter(|spot| spot.hidden())
        .map(|spot| spot.id.get())
        .collect();
    assert_eq!(hidden, [LOOSE_STONE, 27, 31]);
    let dunstan = content.npc(hamlet::WATCHMAN_DUNSTAN).unwrap();
    assert_eq!(
        content
            .areas()
            .area_at(dunstan.position[0], dunstan.position[1])
            .map(|area| area.id()),
        Some(greyhaven_vale::farmland::FARMLAND_AND_HAMLETS)
    );
}

#[test]
fn the_sluice_is_looked_over_cleared_by_its_gate_and_chokes_again() {
    let mut world = World::new(&[(1, hamlets_done(&[]))]);
    let sluice = |world: &World| world.zone.seal_views()[1];
    assert_eq!(sluice(&world).piece, hamlet::SLUICE_STOP);
    assert!(!sluice(&world).open, "the stop stands in the brook");
    world.accept(1, OSRIC, SLUICE);

    let events = world.inspect(1, MILLRACE);
    assert_eq!(refusal(&events), None, "{events:?}");
    let gate = world
        .zone
        .content()
        .mechanism(hamlet::SLUICE_GATE)
        .unwrap()
        .at;
    world.at_spot(1, gate);
    let events = world.send(
        1,
        ZoneCommand::Operate {
            mechanism: hamlet::SLUICE_GATE.get(),
        },
    );
    assert_eq!(refusal(&events), None, "{events:?}");
    assert!(sluice(&world).open, "the gate alone sinks the stop");
    assert_eq!(world.progress(1, SLUICE), [1, 1]);
    // Open, the gate rests; working it again credits nothing twice.
    let events = world.send(
        1,
        ZoneCommand::Operate {
            mechanism: hamlet::SLUICE_GATE.get(),
        },
    );
    assert_eq!(refusal(&events), Some(ErrorCode::SealOpen));

    // Five minutes on, with nobody at the stop, the brook chokes it again for the next player.
    let events = world.turn_in(1, OSRIC, SLUICE, 0);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert!(world.log(1).is_completed(SLUICE));
    world.restore(|state| state.tick += u64::from(SEAL_OPEN_TICKS));
    world.ticks(2);
    assert_eq!((sluice(&world).set, sluice(&world).open), (0, false));
}

#[test]
fn a_grain_sack_goes_from_enas_barn_to_hanne_and_is_fetched_again_when_set_down() {
    let mut world = World::new(&[(1, hamlets_done(&[]))]);
    // No quest, no sack.
    assert_eq!(
        refusal(&world.lift(1, ENAS_SACK)),
        Some(ErrorCode::InvalidQuest)
    );
    world.accept(1, ENA, HANNES_SACK);
    let events = world.lift(1, ENAS_SACK);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert_eq!(world.zone.carried_load(1), Some(LoadKind::Sack));
    // Hanne takes nothing from empty arms.
    world.send(1, ZoneCommand::SetDown);
    world.beside(1, HANNE);
    let events = world.send(1, ZoneCommand::QuestAction { npc: HANNE });
    assert_eq!(refusal(&events), Some(ErrorCode::NotCarrying));
    assert_eq!(world.progress(1, HANNES_SACK), [0]);

    // A fresh sack from the barn, carried down to the mill and handed over.
    let events = world.lift(1, ENAS_SACK);
    assert_eq!(refusal(&events), None, "{events:?}");
    world.beside(1, HANNE);
    let events = world.send(1, ZoneCommand::QuestAction { npc: HANNE });
    assert_eq!(refusal(&events), None, "{events:?}");
    assert_eq!(world.zone.carried_load(1), None);
    assert_eq!(world.progress(1, HANNES_SACK), [1]);
    let events = world.turn_in(1, HANNE, HANNES_SACK, 0);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert!(world.log(1).is_completed(HANNES_SACK));
}

#[test]
fn a_shepherds_bargain_is_settled_with_evidence_or_with_the_players_own_copper() {
    let mut world = World::new(&[(1, hamlets_done(&[])), (2, hamlets_done(&[]))]);
    for player in [1, 2] {
        world.accept(player, LISE, BARGAIN);
        world.beside(player, BERTIL);
        // Bertil hears the player out before anything is settled.
        let events = world.send(
            player,
            ZoneCommand::Decide {
                npc: BERTIL,
                option: 1,
            },
        );
        assert_eq!(refusal(&events), Some(ErrorCode::InvalidQuest));
        let events = world.send(player, ZoneCommand::QuestAction { npc: BERTIL });
        assert_eq!(refusal(&events), None, "{events:?}");
        // The settlement waits for the gate's evidence.
        let events = world.send(
            player,
            ZoneCommand::Decide {
                npc: BERTIL,
                option: 1,
            },
        );
        assert_eq!(refusal(&events), Some(ErrorCode::InvalidQuest));
        let events = world.inspect(player, GATE_TRACKS);
        assert_eq!(refusal(&events), None, "{events:?}");
        assert_eq!(world.progress(player, BARGAIN), [1, 1, 0]);
    }

    // The evidence settles it for nothing.
    world.restore(|state| state.players[0].copper = 7);
    world.beside(1, BERTIL);
    let events = world.send(
        1,
        ZoneCommand::Decide {
            npc: BERTIL,
            option: 1,
        },
    );
    assert_eq!(refusal(&events), None, "{events:?}");
    assert_eq!(world.copper(1), 7);

    // Paying keeps the peace with the player's own copper, refused without enough.
    world.restore(|state| state.players[1].copper = 19);
    world.beside(2, BERTIL);
    let events = world.send(
        2,
        ZoneCommand::Decide {
            npc: BERTIL,
            option: 2,
        },
    );
    assert_eq!(refusal(&events), Some(ErrorCode::NotEnoughMoney));
    assert_eq!(
        (world.copper(2), world.progress(2, BARGAIN)),
        (19, vec![1, 1, 0])
    );
    world.restore(|state| state.players[1].copper = 30);
    let events = world.send(
        2,
        ZoneCommand::Decide {
            npc: BERTIL,
            option: 2,
        },
    );
    assert_eq!(refusal(&events), None, "{events:?}");
    assert_eq!(world.copper(2), 10, "twenty copper paid");
    // Decided once: no second payment.
    let events = world.send(
        2,
        ZoneCommand::Decide {
            npc: BERTIL,
            option: 2,
        },
    );
    assert_eq!(refusal(&events), Some(ErrorCode::InvalidQuest));
    assert_eq!(world.copper(2), 10);

    for (player, option, copper) in [(1, 1, 7 + 25), (2, 2, 10 + 25)] {
        let events = world.turn_in(player, LISE, BARGAIN, 0);
        assert_eq!(refusal(&events), None, "player {player}: {events:?}");
        assert_eq!(world.copper(player), copper);
        assert_eq!(world.log(player).outcome(BARGAIN), Some(option));
    }
    // The settlement stays with the saved character.
    world.reload(2);
    assert_eq!(world.log(2).outcome(BARGAIN), Some(2));
}

#[test]
fn the_folds_hurdles_rise_with_timber_from_the_hamlet_barn() {
    let mut world = World::new(&[(1, hamlets_done(&[54]))]);
    world.accept(1, LISE, FENCE);
    for (index, piece) in [hamlet::NORTH_HURDLE, hamlet::EAST_HURDLE]
        .into_iter()
        .enumerate()
    {
        assert!(world.zone.construction(piece).unwrap().raised.is_none());
        let events = world.lift(1, hamlet::HAMLET_TIMBER);
        assert_eq!(refusal(&events), None, "{events:?}");
        let site = world
            .zone
            .content()
            .pieces()
            .iter()
            .find(|candidate| candidate.id == piece)
            .unwrap()
            .clone();
        let [px, _, pz] = site.position;
        let [hx, _, hz] = site.half_extents;
        world.stand_at(
            1,
            if hx > hz {
                [px, pz - hz - 120]
            } else {
                [px + hx + 120, pz]
            },
        );
        let events = world.send(1, ZoneCommand::SetDown);
        assert_eq!(refusal(&events), None, "{events:?}");
        world.ticks(2);
        assert!(
            world.zone.construction(piece).unwrap().raised.is_some(),
            "{piece:?} stands"
        );
        assert_eq!(world.progress(1, FENCE)[index], 1);
    }
    let health = |world: &World, piece: PieceId| {
        world
            .zone
            .snapshot()
            .unwrap()
            .pieces
            .iter()
            .find(|state| state.piece == piece)
            .unwrap()
            .health
    };
    assert_eq!(health(&world, hamlet::NORTH_HURDLE), 90);
    let events = world.turn_in(1, LISE, FENCE, 1);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert!(world.log(1).is_completed(FENCE));
}

#[test]
fn bertils_locket_is_found_from_his_directions_alone_and_is_worth_nothing_to_a_vendor() {
    let mut world = World::new(&[(1, hamlets_done(&[54]))]);
    let stone = world
        .zone
        .content()
        .evidence_spot(EvidenceId::new(LOOSE_STONE))
        .unwrap()
        .clone();
    assert!(stone.hidden());
    assert!(
        stone
            .directions
            .as_deref()
            .is_some_and(|directions| directions.contains("old tower"))
    );
    // Before the quest, the stone hides nothing.
    let events = world.inspect(1, LOOSE_STONE);
    assert_eq!(refusal(&events), Some(ErrorCode::InvalidQuest));
    world.accept(1, BERTIL, HEIRLOOM);
    // A step too far from the stone finds nothing.
    world.stand_at(1, [stone.at[0] - stone.reach - 100, stone.at[2]]);
    let events = world.send(1, ZoneCommand::Inspect { spot: LOOSE_STONE });
    assert_eq!(refusal(&events), Some(ErrorCode::OutOfRange));
    let events = world.inspect(1, LOOSE_STONE);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert_eq!(world.carries(1, LOCKET), 1);
    // Once in the bag, the stone gives no second locket.
    let events = world.send(1, ZoneCommand::Inspect { spot: LOOSE_STONE });
    assert_eq!(refusal(&events), Some(ErrorCode::InvalidQuest));
    assert_eq!(world.carries(1, LOCKET), 1);
    assert_eq!(sell_price(LOCKET), 0);

    let events = world.turn_in(1, BERTIL, HEIRLOOM, 0);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert!(world.log(1).is_completed(HEIRLOOM));
    assert_eq!(world.carries(1, LOCKET), 0, "Bertil has it back");
}

#[test]
fn bread_goes_up_the_farm_road_to_the_towers_watchman() {
    let mut world = World::new(&[(1, hamlets_done(&[52]))]);
    world.accept(1, HANNE, BREAD);
    let events = world.lift(1, LOAVES);
    assert_eq!(refusal(&events), None, "{events:?}");
    // Set down by the tower's yard (a fight) and lifted again where it lies.
    world.stand_at(1, [0, 43_000]);
    let events = world.send(1, ZoneCommand::SetDown);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert_eq!(world.zone.carried_load(1), None);
    let events = world.send(1, ZoneCommand::Lift { source: None });
    assert_eq!(refusal(&events), None, "{events:?}");
    assert_eq!(world.zone.carried_load(1), Some(LoadKind::Sack));

    world.beside(1, hamlet::WATCHMAN_DUNSTAN);
    let events = world.send(
        1,
        ZoneCommand::QuestAction {
            npc: hamlet::WATCHMAN_DUNSTAN,
        },
    );
    assert_eq!(refusal(&events), None, "{events:?}");
    assert_eq!(world.progress(1, BREAD), [1]);
    let events = world.turn_in(1, hamlet::WATCHMAN_DUNSTAN, BREAD, 0);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert!(world.log(1).is_completed(BREAD));
}
