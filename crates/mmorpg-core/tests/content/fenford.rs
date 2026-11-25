//! The village and the river (#1019, slice 6a): Ferryman Osk's rope block found on the ford's
//! bed (71), the ford's three stakes surveyed for him (72), Miller Wynne's brake wheel carried
//! from the cart and fitted (74), spring water carried from the alders to Hob Ferris (75), and the
//! smuggler's token handed to the Watch or left for Osk, with different rewards (77).
//!
//! Every step is a server-checked command; refusals change nothing, each objective is credited
//! once, and every `stand_at` restores the zone from its canonical snapshot, so recovery carries
//! the loads, the wheel and the decision.
use crate::support;

use std::collections::BTreeMap;
use std::sync::Arc;

use mmorpg_core::greyhaven_vale::{self, fenford, quests};
use mmorpg_core::{
    CharacterId, CharacterLocation, ErrorCode, EvidenceId, ItemId, LoadKind, LoadSpotId, NpcId,
    QuestId, QuestLog, ZoneCommand, ZoneEvent, ZoneId, ZoneSimulation, sell_price,
};
use support::classes;

const GREYHAVEN: ZoneId = ZoneId::new(1);
const WARDEN_CLASS: u8 = 0;
const HOB: NpcId = NpcId::new(27);
const WYNNE: NpcId = quests::MILLER_WYNNE;
const OSK: NpcId = quests::FERRYMAN_OSK;
const CORWIN: NpcId = fenford::WATCHMAN_CORWIN;
const TEAGUE: NpcId = fenford::BOATMAN_TEAGUE;
const ROPE: QuestId = quests::THE_FERRYMANS_ROPE;
const RECKONING: QuestId = quests::THE_FORDS_RECKONING;
const MILL_WHEEL: QuestId = quests::THE_MISSING_MILL_WHEEL;
const WATER: QuestId = quests::WATER_FOR_THE_REST;
const TOKEN: QuestId = quests::THE_SMUGGLERS_TOKEN;

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

    fn try_accept(&mut self, player: u32, npc: NpcId, quest: QuestId) -> Option<ErrorCode> {
        self.beside(player, npc);
        let events = self.send(
            player,
            ZoneCommand::AcceptQuest {
                npc,
                quest: quest.get(),
            },
        );
        refusal(&events)
    }

    fn accept(&mut self, player: u32, npc: NpcId, quest: QuestId) {
        assert_eq!(
            self.try_accept(player, npc, quest),
            None,
            "accepting {quest:?}"
        );
    }

    fn turn_in(&mut self, player: u32, npc: NpcId, quest: QuestId) -> Vec<ZoneEvent> {
        self.beside(player, npc);
        self.send(
            player,
            ZoneCommand::CompleteQuest {
                npc,
                quest: quest.get(),
                choice: 0,
            },
        )
    }

    /// Moves `player`'s body to stand at `[x, z]`, restoring the zone from its canonical state as
    /// recovery would.
    fn stand_at(&mut self, player: u32, [x, z]: [i32; 2]) {
        let y = self.zone.content().definition().ground_at(x, z) + 90;
        let mut state = self.zone.snapshot().unwrap();
        let record = state
            .players
            .iter_mut()
            .find(|record| record.player_id == player)
            .unwrap();
        record.position = [x as f32, y as f32, z as f32];
        record.velocity = [0.0; 3];
        let content = Arc::clone(self.zone.content());
        self.zone = ZoneSimulation::from_snapshot(state, content).unwrap();
        self.zone.advance_tick().unwrap();
    }

    fn beside(&mut self, player: u32, npc: NpcId) {
        let [x, z] = self.zone.npc_placement(npc).unwrap().position;
        self.stand_at(player, [x + 100, z + 100]);
    }

    fn evidence(&self, spot: EvidenceId) -> [i32; 3] {
        self.zone.content().evidence_spot(spot).unwrap().at
    }

    fn inspect(&mut self, player: u32, spot: EvidenceId) -> Vec<ZoneEvent> {
        let at = self.evidence(spot);
        self.stand_at(player, [at[0], at[2]]);
        self.send(player, ZoneCommand::Inspect { spot: spot.get() })
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

    fn act(&mut self, player: u32, npc: NpcId) -> Vec<ZoneEvent> {
        self.beside(player, npc);
        self.send(player, ZoneCommand::QuestAction { npc })
    }

    fn decide(&mut self, player: u32, npc: NpcId, option: u8) -> Vec<ZoneEvent> {
        self.beside(player, npc);
        self.send(player, ZoneCommand::Decide { npc, option })
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
    CharacterId::new(format!("fenford-{player}")).unwrap()
}

fn refusal(events: &[ZoneEvent]) -> Option<ErrorCode> {
    events.iter().find_map(|event| match event {
        ZoneEvent::Error { code, .. } => Some(*code),
        _ => None,
    })
}

/// Quests 1 to 32 (the opening, the region and the hamlets) turned in, and `extra`.
fn region_done(extra: &[u32]) -> Vec<QuestId> {
    (1..=32)
        .chain(extra.iter().copied())
        .map(QuestId::new)
        .collect()
}

#[test]
fn fenfords_spots_stand_on_their_ground_and_its_people_at_their_posts() {
    let content = greyhaven_vale::content();
    let ground = |at: [i32; 3]| content.definition().ground_at(at[0], at[2]);
    let spots = [
        quests::FRAYED_ROPE,
        quests::FORD_BED,
        quests::FORD_STAKES[0],
        quests::FORD_STAKES[1],
        quests::FORD_STAKES[2],
        quests::HIDDEN_SHIPMENT,
    ];
    for spot in spots {
        let spot = content.evidence_spot(spot).unwrap();
        assert!(
            (spot.at[1] - ground(spot.at)).abs() <= 50,
            "{} at {:?}: ground {}",
            spot.name,
            spot.at,
            ground(spot.at)
        );
    }
    // The block lies under wading water on the ford's bed; the middle stake stands in it.
    for spot in [quests::FORD_BED, quests::FORD_STAKES[1]] {
        let at = content.evidence_spot(spot).unwrap().at;
        let water = content
            .waters()
            .iter()
            .find(|water| water.contains(at[0], at[2]))
            .expect("in the river");
        let depth = water.surface - at[1];
        // Feet swim from 1.3 m down (`water::SWIM_DEPTH_UNITS`).
        assert!((1..130).contains(&depth), "{depth}");
    }
    for source in [fenford::CRATED_WHEEL, fenford::SPRING_JUG] {
        let spot = content.load_spot(source).unwrap();
        assert!(
            (spot.at[1] - ground(spot.at)).abs() <= 50,
            "{} at {:?}: ground {}",
            spot.name,
            spot.at,
            ground(spot.at)
        );
    }
    assert_eq!(
        content.load_spot(fenford::SPRING_JUG).unwrap().kind,
        LoadKind::WaterJug
    );
    // The spring is in the Fenwater Alders, some 95 m up the bank from the inn.
    let spring = content.load_spot(fenford::SPRING_JUG).unwrap().at;
    assert_eq!(
        greyhaven_vale::design::forest_at(spring[0], spring[2]).map(|forest| forest.name.as_str()),
        Some("Fenwater Alders")
    );
    let inn = content.npc(HOB).unwrap().position;
    let carry = f64::from(spring[0] - inn[0]).hypot(f64::from(spring[2] - inn[1]));
    assert!(carry > 9_000.0, "{carry}");
    // The block and the shipment are hidden, with their directions.
    for spot in [quests::FORD_BED, quests::HIDDEN_SHIPMENT] {
        assert!(content.evidence_spot(spot).unwrap().hidden());
    }
    for npc in [CORWIN, TEAGUE] {
        let position = content.npc(npc).unwrap().position;
        assert_eq!(
            content
                .areas()
                .area_at(position[0], position[1])
                .map(|area| area.id()),
            Some(greyhaven_vale::wilds::FENWATER_CROSSING)
        );
    }
    for item in [quests::ROPE_BLOCK, quests::SMUGGLERS_TOKEN] {
        assert_eq!(sell_price(item), 0);
    }
    // The catalogue holds IDs 1-72, 74, 75 and 77; 73, 76, 78 and 79 are slice 6b's.
    let ids: Vec<u32> = content
        .quests()
        .iter()
        .map(|quest| quest.id.get())
        .collect();
    assert_eq!(
        ids,
        (1..=72).chain([74, 75, 77]).collect::<Vec<_>>(),
        "75 quests"
    );
}

#[test]
fn osks_rope_block_is_found_on_the_fords_bed_from_his_directions() {
    let mut world = World::new(&[(1, region_done(&[]))]);
    let bed = world
        .zone
        .content()
        .evidence_spot(quests::FORD_BED)
        .unwrap()
        .clone();
    assert!(
        bed.directions
            .as_deref()
            .is_some_and(|directions| directions.contains("middle stake"))
    );
    // Before the quest, the bed hides nothing.
    assert_eq!(
        refusal(&world.inspect(1, quests::FORD_BED)),
        Some(ErrorCode::InvalidQuest)
    );
    world.accept(1, OSK, ROPE);
    let events = world.inspect(1, quests::FRAYED_ROPE);
    assert_eq!(refusal(&events), None, "{events:?}");
    // A step too far from the block finds nothing.
    world.stand_at(1, [bed.at[0] + bed.reach + 100, bed.at[2]]);
    let events = world.send(
        1,
        ZoneCommand::Inspect {
            spot: quests::FORD_BED.get(),
        },
    );
    assert_eq!(refusal(&events), Some(ErrorCode::OutOfRange));
    // Wading on the ford's bed, the player finds it, once.
    let events = world.inspect(1, quests::FORD_BED);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert_eq!(world.carries(1, quests::ROPE_BLOCK), 1);
    let events = world.send(
        1,
        ZoneCommand::Inspect {
            spot: quests::FORD_BED.get(),
        },
    );
    assert_eq!(refusal(&events), Some(ErrorCode::InvalidQuest));
    assert_eq!(
        world.progress(1, ROPE),
        [1, 0],
        "the log stores nothing for the find: the bag holds it"
    );
    assert_eq!(world.carries(1, quests::ROPE_BLOCK), 1);

    let copper = world.copper(1);
    let events = world.turn_in(1, OSK, ROPE);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert!(world.log(1).is_completed(ROPE));
    assert_eq!(world.carries(1, quests::ROPE_BLOCK), 0, "Osk has it back");
    assert_eq!(world.copper(1), copper + 20);
}

#[test]
fn the_fords_three_stakes_are_surveyed_and_reported_to_osk() {
    let mut world = World::new(&[(1, region_done(&[])), (2, region_done(&[71]))]);
    // The survey follows the rope.
    assert_eq!(
        world.try_accept(1, OSK, RECKONING),
        Some(ErrorCode::InvalidQuest)
    );
    world.accept(2, OSK, RECKONING);
    // Reporting before the survey is done is refused.
    let events = world.turn_in(2, OSK, RECKONING);
    assert_eq!(refusal(&events), Some(ErrorCode::QuestIncomplete));
    for (index, stake) in quests::FORD_STAKES.into_iter().enumerate() {
        let events = world.inspect(2, stake);
        assert_eq!(refusal(&events), None, "{events:?}");
        assert_eq!(world.progress(2, RECKONING)[index], 1);
    }
    // A stake inspected again credits nothing twice.
    let events = world.inspect(2, quests::FORD_STAKES[1]);
    assert_eq!(refusal(&events), Some(ErrorCode::InvalidQuest));
    world.reload(2);
    assert_eq!(world.progress(2, RECKONING), [1, 1, 1]);
    let events = world.turn_in(2, OSK, RECKONING);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert!(world.log(2).is_completed(RECKONING));
}

#[test]
fn wynnes_brake_wheel_is_carried_from_the_cart_and_fitted_at_her_mill() {
    let mut world = World::new(&[(1, region_done(&[]))]);
    // No quest, no crate.
    assert_eq!(
        refusal(&world.lift(1, fenford::CRATED_WHEEL)),
        Some(ErrorCode::InvalidQuest)
    );
    world.accept(1, WYNNE, MILL_WHEEL);
    assert!(
        world
            .zone
            .construction(fenford::BRAKE_WHEEL)
            .unwrap()
            .raised
            .is_none()
    );
    let events = world.lift(1, fenford::CRATED_WHEEL);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert_eq!(world.zone.carried_load(1), Some(LoadKind::Crate));
    // Wynne takes no crate from the player's arms: it is fitted at the mill.
    assert_eq!(world.progress(1, MILL_WHEEL), [0]);
    let piece = world
        .zone
        .content()
        .pieces()
        .iter()
        .find(|candidate| candidate.id == fenford::BRAKE_WHEEL)
        .unwrap()
        .clone();
    let [px, _, pz] = piece.position;
    world.stand_at(1, [px, pz - piece.half_extents[2] - 120]);
    let events = world.send(1, ZoneCommand::SetDown);
    assert_eq!(refusal(&events), None, "{events:?}");
    world.zone.advance_tick().unwrap();
    world.zone.advance_tick().unwrap();
    assert!(
        world
            .zone
            .construction(fenford::BRAKE_WHEEL)
            .unwrap()
            .raised
            .is_some(),
        "the wheel stands"
    );
    assert_eq!(world.zone.carried_load(1), None);
    assert_eq!(world.progress(1, MILL_WHEEL), [1]);
    world.reload(1);
    let events = world.turn_in(1, WYNNE, MILL_WHEEL);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert!(world.log(1).is_completed(MILL_WHEEL));
}

#[test]
fn spring_water_is_carried_on_foot_from_the_alders_to_hob_ferris() {
    let mut world = World::new(&[(1, region_done(&[]))]);
    world.accept(1, HOB, WATER);
    let events = world.lift(1, fenford::SPRING_JUG);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert_eq!(world.zone.carried_load(1), Some(LoadKind::WaterJug));
    // No waystone carries the jug: both hands hold it.
    let events = world.send(
        1,
        ZoneCommand::UseWaystone {
            waystone: mmorpg_core::GREYHAVEN_OUTPOST,
        },
    );
    assert_eq!(refusal(&events), Some(ErrorCode::HandsFull));
    assert_eq!(world.zone.carried_load(1), Some(LoadKind::WaterJug));
    // Set down on the way (a fight) and lifted again where it stands.
    world.stand_at(1, [100_500, 21_500]);
    let events = world.send(1, ZoneCommand::SetDown);
    assert_eq!(refusal(&events), None, "{events:?}");
    // Hob takes nothing from empty arms.
    let events = world.act(1, HOB);
    assert_eq!(refusal(&events), Some(ErrorCode::NotCarrying));
    world.stand_at(1, [100_500, 21_500]);
    let events = world.send(1, ZoneCommand::Lift { source: None });
    assert_eq!(refusal(&events), None, "{events:?}");
    assert_eq!(world.zone.carried_load(1), Some(LoadKind::WaterJug));

    let events = world.act(1, HOB);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert_eq!(world.zone.carried_load(1), None);
    assert_eq!(world.progress(1, WATER), [1]);
    let events = world.turn_in(1, HOB, WATER);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert!(world.log(1).is_completed(WATER));
}

#[test]
fn the_smugglers_token_goes_to_the_watch_or_is_left_for_osk_and_each_pays_its_own() {
    let mut world = World::new(&[(1, region_done(&[71])), (2, region_done(&[71]))]);
    for player in [1, 2] {
        world.accept(player, OSK, TOKEN);
        // Corwin hears nothing before the shipment is found and its owner questioned.
        let events = world.decide(player, CORWIN, 1);
        assert_eq!(refusal(&events), Some(ErrorCode::InvalidQuest));
        let events = world.inspect(player, quests::HIDDEN_SHIPMENT);
        assert_eq!(refusal(&events), None, "{events:?}");
        assert_eq!(world.carries(player, quests::SMUGGLERS_TOKEN), 1);
        let events = world.act(player, TEAGUE);
        assert_eq!(refusal(&events), None, "{events:?}");
        // The find counts from the bag, so the log stores zero for it.
        assert_eq!(world.progress(player, TOKEN), [0, 1, 0]);
    }
    // The client names what each option pays at the turn-in.
    let quest = world.zone.content().quest(TOKEN).unwrap().clone();
    assert_eq!(
        quest
            .decision
            .iter()
            .map(|option| option.reward)
            .collect::<Vec<_>>(),
        [20, 10]
    );

    for (player, option) in [(1, 1), (2, 2)] {
        let events = world.decide(player, CORWIN, option);
        assert_eq!(refusal(&events), None, "{events:?}");
        // Decided once: deciding again changes nothing.
        let events = world.decide(player, CORWIN, 3 - option);
        assert_eq!(refusal(&events), Some(ErrorCode::InvalidQuest));
        assert_eq!(world.progress(player, TOKEN), [0, 1, option]);
    }
    // Each outcome pays the quest's 15 copper and its own reward, once, at the turn-in.
    for (player, option, bonus) in [(1, 1, 20), (2, 2, 10)] {
        let copper = world.copper(player);
        let events = world.turn_in(player, OSK, TOKEN);
        assert_eq!(refusal(&events), None, "player {player}: {events:?}");
        assert_eq!(world.copper(player), copper + 15 + bonus);
        assert_eq!(world.log(player).outcome(TOKEN), Some(option));
        assert_eq!(world.carries(player, quests::SMUGGLERS_TOKEN), 0);
        let events = world.turn_in(player, OSK, TOKEN);
        assert_eq!(refusal(&events), Some(ErrorCode::InvalidQuest));
        assert_eq!(world.copper(player), copper + 15 + bonus);
    }
    // The Watch took Teague for the first player only; the outcome stays with the character.
    assert!(world.zone.npc_departed(1, TEAGUE));
    assert!(!world.zone.npc_departed(2, TEAGUE));
    world.reload(1);
    assert_eq!(world.log(1).outcome(TOKEN), Some(1));
    assert!(world.zone.npc_departed(1, TEAGUE));
}
