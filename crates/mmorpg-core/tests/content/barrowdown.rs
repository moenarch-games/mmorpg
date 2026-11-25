//! Barrowdown's ring and barrow (#1019, slice 5a): Old Maud's warning about her stones (63), the
//! ring's measure worked in its inscription's order (64), the grave robber talked round or fought
//! for the barrow's torc (65), and Maud's distrust of a staff shown in hand, overcome by putting
//! it away, a trusted party mate or Gwen's word (66).
//!
//! Every step is a server-checked command; refusals change nothing, each objective is credited
//! once, and every `stand_at` restores the zone from its canonical snapshot, so recovery carries
//! the order worked so far, the decision and the fight.
use crate::support::{arena, classes};

use std::collections::BTreeMap;
use std::sync::Arc;

use mmorpg_core::greyhaven_vale::{self, barrowdown, quests};
use mmorpg_core::{
    CharacterId, CharacterLocation, CreatureLife, EntityRef, Equipment, EquipmentSlot, ErrorCode,
    EvidenceId, ItemId, MechanismId, NpcId, PartyMember, PartyRoster, PartyState, QuestId,
    QuestLog, ZoneCommand, ZoneEvent, ZoneId, ZoneSimulation, sell_price,
};

const GREYHAVEN: ZoneId = ZoneId::new(1);
const WARDEN: u8 = 0;
const ARCANIST: u8 = 2;
const GWEN: NpcId = quests::ALEWIFE;
const MAUD: NpcId = quests::OLD_MAUD;
const COBB: NpcId = barrowdown::COBB;
/// The Apprentice Wand, a staff-family main-hand weapon.
const WAND: ItemId = ItemId::new(4);

/// The quests before *Maud's Warning*: the opening and the region's 1-14 and 16, and `extra`.
/// *Grain for the Barrow Rest* (15) is left out unless asked for: it is Gwen's word for Maud.
fn before(extra: &[u32]) -> Vec<QuestId> {
    (1..=14)
        .chain([16])
        .chain(extra.iter().copied())
        .map(QuestId::new)
        .collect()
}

struct World {
    zone: ZoneSimulation,
    sequences: BTreeMap<u32, u32>,
}

impl World {
    /// Characters `(player, class, staff in hand)`, each having turned in `completed`, admitted at
    /// level 6 by the Marshal.
    fn new(players: &[(u32, u8, bool)], completed: &[QuestId]) -> Self {
        let content = greyhaven_vale::content();
        let mut zone = ZoneSimulation::with_content(GREYHAVEN, Arc::clone(&content)).unwrap();
        for &(player, class, staff) in players {
            let mut donor = ZoneSimulation::with_content(GREYHAVEN, Arc::clone(&content)).unwrap();
            donor.add_player(player).unwrap();
            let mut donor = classes::at_level(donor, player, 6);
            donor
                .apply_command(player, 1, ZoneCommand::ChooseClass { class, sex: 0 })
                .unwrap();
            donor.advance_tick().unwrap();
            let mut record = donor.capture_character(player, id(player)).unwrap();
            record.quests = QuestLog {
                completed: completed.iter().copied().collect(),
                ..QuestLog::default()
            };
            let mut slots = *record.equipment.slots();
            slots[usize::from(EquipmentSlot::MainHand.index())] = staff.then_some(WAND);
            record.equipment = Equipment::from_slots(slots).unwrap();
            let [x, z] = content.npc(quests::MARSHAL).unwrap().position;
            record.location = CharacterLocation {
                zone_id: GREYHAVEN,
                position: [x + 100, 90, z + 100],
                facing: 0,
            };
            zone.admit_character(player, &record).unwrap();
        }
        zone.advance_tick().unwrap();
        Self {
            zone,
            sequences: players.iter().map(|&(player, ..)| (player, 10)).collect(),
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

    /// Changes the canonical state and restores the zone from it, as recovery would.
    fn restore(&mut self, change: impl FnOnce(&mut mmorpg_core::CanonicalZoneSnapshot)) {
        let mut state = self.zone.snapshot().unwrap();
        change(&mut state);
        let content = Arc::clone(self.zone.content());
        self.zone = ZoneSimulation::from_snapshot(state, content).unwrap();
    }

    /// Moves `player`'s body to stand at `[x, z]` and advances a tick.
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

    fn act(&mut self, player: u32, npc: NpcId) -> Vec<ZoneEvent> {
        self.beside(player, npc);
        self.send(player, ZoneCommand::QuestAction { npc })
    }

    fn decide(&mut self, player: u32, npc: NpcId, option: u8) -> Vec<ZoneEvent> {
        self.beside(player, npc);
        self.send(player, ZoneCommand::Decide { npc, option })
    }

    fn inspect(&mut self, player: u32, spot: EvidenceId) -> Vec<ZoneEvent> {
        let [x, _, z] = self.zone.content().evidence_spot(spot).unwrap().at;
        self.stand_at(player, [x, z]);
        self.send(player, ZoneCommand::Inspect { spot: spot.get() })
    }

    /// Works a ring stone from beside it, on the ring's side.
    fn work(&mut self, player: u32, stone: MechanismId) -> Vec<ZoneEvent> {
        let [x, _, z] = self.zone.content().mechanism(stone).unwrap().at;
        let (dx, dz) = (47_000 - x, 60_000 - z);
        self.stand_at(player, [x + dx.signum() * 120, z + dz.signum() * 120]);
        self.send(
            player,
            ZoneCommand::Operate {
                mechanism: stone.get(),
            },
        )
    }

    fn progress(&self, player: u32, quest: QuestId) -> Vec<u8> {
        let objectives = self.zone.content().quest(quest).unwrap().objectives.len();
        self.log(player).entry(quest).unwrap().progress[..objectives].to_vec()
    }

    fn log(&self, player: u32) -> QuestLog {
        self.zone
            .capture_character(player, id(player))
            .unwrap()
            .quests
    }

    fn carries(&self, player: u32, item: ItemId) -> u32 {
        self.zone
            .capture_character(player, id(player))
            .unwrap()
            .inventory
            .count(item)
    }

    fn party(&mut self, members: &[u32]) {
        let state = PartyState {
            revision: 5,
            roster: Some(PartyRoster {
                party: 42,
                version: 1,
                members: members
                    .iter()
                    .map(|&player| PartyMember {
                        name: format!("Member {player}"),
                        player: Some(player),
                        online: true,
                    })
                    .collect(),
            }),
            invite: None,
        };
        for &player in members {
            assert!(self.zone.apply_party_state(player, state.clone()).unwrap());
        }
    }

    fn foe_life(&self) -> Option<CreatureLife> {
        self.zone
            .snapshot()
            .unwrap()
            .creatures
            .into_iter()
            .find(|creature| creature.creature_id == barrowdown::GRAVE_ROBBER_FOE)
            .map(|creature| creature.life)
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
    CharacterId::new(format!("barrow-{player}")).unwrap()
}

fn refusal(events: &[ZoneEvent]) -> Option<ErrorCode> {
    events.iter().find_map(|event| match event {
        ZoneEvent::Error { code, .. } => Some(*code),
        _ => None,
    })
}

#[test]
fn the_ring_spots_and_stones_stand_on_their_ground_and_cobb_digs_west_of_it() {
    let content = greyhaven_vale::content();
    let ground = |at: [i32; 3]| content.definition().ground_at(at[0], at[2]);
    for spot in 21..=24 {
        let spot = content.evidence_spot(EvidenceId::new(spot)).unwrap();
        assert!((spot.at[1] - ground(spot.at)).abs() <= 50, "{}", spot.name);
    }
    let measure = content.sequence(barrowdown::RING_MEASURE).unwrap();
    assert_eq!(measure.mechanisms.len(), 4);
    for stone in &measure.mechanisms {
        let stone = content.mechanism(*stone).unwrap();
        assert_eq!(stone.at[1], ground(stone.at), "{}", stone.name);
    }
    // Old Maud gives and takes the ring's quests; Cobb is no quest giver and keeps no routine.
    assert_eq!(
        content.npc(MAUD).unwrap().role,
        mmorpg_core::NpcRole::QuestGiver
    );
    assert!(content.routine(COBB).is_none());
    assert_eq!(sell_price(quests::BARROW_TORC), 0);
}

#[test]
fn mauds_warning_hears_her_out_and_reads_the_two_stones() {
    let quest = quests::MAUDS_WARNING;
    let mut world = World::new(&[(1, WARDEN, false)], &before(&[]));
    world.accept(1, GWEN, quest);
    // Standing beside Maud is hearing her account.
    world.beside(1, MAUD);
    assert_eq!(world.progress(1, quest), [1, 0, 0]);
    for spot in [quests::SPADE_MARKS, quests::SCATTERED_OFFERING] {
        let events = world.inspect(1, spot);
        assert_eq!(refusal(&events), None, "{events:?}");
    }
    assert_eq!(world.progress(1, quest), [1, 1, 1]);
    // Read once: a second look credits nothing twice.
    world.inspect(1, quests::SPADE_MARKS);
    assert_eq!(world.progress(1, quest), [1, 1, 1]);
    let events = world.turn_in(1, MAUD, quest);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert!(world.log(1).is_completed(quest));
}

#[test]
fn the_rings_measure_is_worked_in_the_inscriptions_order_and_a_wrong_stone_starts_it_over() {
    use barrowdown::{DAWN_STONE, DUSK_STONE, NIGHT_STONE, NOON_STONE};
    let quest = quests::THE_RINGS_MEASURE;
    let mut world = World::new(&[(1, WARDEN, false), (2, WARDEN, false)], &before(&[63]));
    world.accept(1, MAUD, quest);
    // Before the lintel is read the stones answer nothing.
    world.work(1, DAWN_STONE);
    assert_eq!(world.progress(1, quest), [0, 0]);
    // A character without the quest works the stones to no effect on anyone.
    let events = world.work(2, DAWN_STONE);
    assert_eq!(refusal(&events), None);
    assert!(world.log(2).entry(quest).is_none());
    world.inspect(1, quests::RING_INSCRIPTION);
    assert_eq!(world.progress(1, quest), [1, 0]);
    // Dawn, then noon: two in order, and dawn's again is no harm once counted.
    for stone in [DAWN_STONE, NOON_STONE] {
        let events = world.work(1, stone);
        assert_eq!(refusal(&events), None, "{events:?}");
    }
    assert_eq!(world.progress(1, quest), [1, 2]);
    let events = world.work(1, NOON_STONE);
    assert_eq!(refusal(&events), None);
    assert_eq!(world.progress(1, quest), [1, 2]);
    // The night stone out of turn: the ring forgets, and the player is told.
    let events = world.work(1, NIGHT_STONE);
    assert_eq!(refusal(&events), Some(ErrorCode::OutOfOrder));
    assert_eq!(world.progress(1, quest), [1, 0]);
    // The order kept so far survives a reload; a wrong first stone starts over at the first.
    world.work(1, DAWN_STONE);
    world.reload(1);
    assert_eq!(world.progress(1, quest), [1, 1]);
    let events = world.work(1, DAWN_STONE);
    assert_eq!(refusal(&events), None);
    assert_eq!(world.progress(1, quest), [1, 1]);
    world.work(1, NIGHT_STONE);
    assert_eq!(world.progress(1, quest), [1, 0]);
    for stone in [DAWN_STONE, NOON_STONE, DUSK_STONE, NIGHT_STONE] {
        let events = world.work(1, stone);
        assert_eq!(refusal(&events), None, "{stone:?}: {events:?}");
    }
    assert_eq!(world.progress(1, quest), [1, 4]);
    // Done is done: further stones change nothing.
    let events = world.work(1, NOON_STONE);
    assert_eq!(refusal(&events), None);
    assert_eq!(world.progress(1, quest), [1, 4]);
    let events = world.turn_in(1, MAUD, quest);
    assert_eq!(refusal(&events), None, "{events:?}");
}

/// A character on *The Grave Robber* with the turned earth followed to Cobb.
fn robber(players: &[(u32, u8, bool)]) -> World {
    let mut world = World::new(players, &before(&[63, 64]));
    for &(player, ..) in players {
        world.accept(player, MAUD, quests::THE_GRAVE_ROBBER);
        let events = world.inspect(player, quests::TURNED_EARTH);
        assert_eq!(refusal(&events), None, "{events:?}");
    }
    world
}

#[test]
fn the_grave_robber_hands_the_torc_over_when_talked_round() {
    let quest = quests::THE_GRAVE_ROBBER;
    let mut world = robber(&[(1, WARDEN, false)]);
    assert_eq!(world.progress(1, quest), [1, 0, 0]);
    let events = world.decide(1, COBB, 1);
    assert_eq!(refusal(&events), None, "{events:?}");
    // The choice is stored and the torc is in the bag; Cobb stays at his diggings.
    assert_eq!(world.progress(1, quest), [1, 1, 0]);
    assert_eq!(world.carries(1, quests::BARROW_TORC), 1);
    assert!(!world.zone.npc_hidden(COBB));
    assert!(matches!(
        world.foe_life(),
        Some(CreatureLife::Despawned { .. })
    ));
    // Deciding again changes nothing; with the torc in the bag there is nothing to carry out.
    assert_eq!(
        refusal(&world.decide(1, COBB, 2)),
        Some(ErrorCode::InvalidQuest)
    );
    assert_eq!(refusal(&world.act(1, COBB)), Some(ErrorCode::InvalidQuest));
    // Turned in, the torc goes home and the peaceful choice stays with the character.
    let events = world.turn_in(1, MAUD, quest);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert_eq!(world.carries(1, quests::BARROW_TORC), 0);
    assert_eq!(world.log(1).outcome(quest), Some(1));
    world.reload(1);
    assert_eq!(world.log(1).outcome(quest), Some(1));
}

#[test]
fn the_grave_robber_fights_when_faced_and_the_torc_comes_from_his_body() {
    let quest = quests::THE_GRAVE_ROBBER;
    let mut world = robber(&[(1, WARDEN, false)]);
    let events = world.decide(1, COBB, 2);
    assert_eq!(refusal(&events), None, "{events:?}");
    // He turns: the NPC is gone while his foe stands and attacks the one who chose the fight.
    assert_eq!(world.progress(1, quest), [1, 2, 0]);
    assert_eq!(world.carries(1, quests::BARROW_TORC), 0);
    assert!(world.zone.npc_hidden(COBB));
    let foe = world
        .zone
        .snapshot()
        .unwrap()
        .creatures
        .into_iter()
        .find(|creature| creature.creature_id == barrowdown::GRAVE_ROBBER_FOE)
        .unwrap();
    assert_eq!(foe.life, CreatureLife::Alive);
    assert!(
        foe.threat
            .iter()
            .any(|entry| entry.entity == EntityRef::Player(1)),
        "{:?}",
        foe.threat
    );
    // Felled (a restored zone keeps the fight), his body yields the torc.
    world.restore(|state| {
        let foe = state
            .creatures
            .iter_mut()
            .find(|creature| creature.creature_id == barrowdown::GRAVE_ROBBER_FOE)
            .unwrap();
        foe.health = 1;
    });
    let target = EntityRef::Creature(barrowdown::GRAVE_ROBBER_FOE);
    world.send(1, ZoneCommand::SelectTarget(Some(target)));
    for _ in 0..mmorpg_core::golden::t30(600) {
        if world.carries(1, quests::BARROW_TORC) == 1 {
            break;
        }
        let view = world.zone.snapshot_for_player(1).unwrap();
        if let Some(loot) = view.loot {
            world.send(1, ZoneCommand::Loot(loot.claim));
            continue;
        }
        match arena::swing_at(&world.zone, 1, target) {
            Some(swing) => {
                world.send(1, swing);
            }
            None => world.zone.advance_tick().unwrap(),
        }
    }
    assert_eq!(world.carries(1, quests::BARROW_TORC), 1);
    assert_eq!(world.progress(1, quest)[..2], [1, 2]);
    world.send(1, ZoneCommand::SetWeaponDrawn(false));
    let events = world.turn_in(1, MAUD, quest);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert_eq!(world.log(1).outcome(quest), Some(2));
}

#[test]
fn a_robber_who_gives_up_his_fight_is_faced_again_at_his_diggings() {
    let quest = quests::THE_GRAVE_ROBBER;
    let mut world = robber(&[(1, WARDEN, false)]);
    world.decide(1, COBB, 2);
    assert!(world.zone.npc_hidden(COBB));
    // The player runs back to Maud and the fight drops off (his threat cleared): he leaves the
    // world and Cobb is back, the choice still stored.
    world.beside(1, MAUD);
    world.restore(|state| {
        let foe = state
            .creatures
            .iter_mut()
            .find(|creature| creature.creature_id == barrowdown::GRAVE_ROBBER_FOE)
            .unwrap();
        foe.threat.clear();
        foe.ai = mmorpg_core::CreatureAi::Idle {
            timer: 0,
            destination: None,
        };
        foe.combat_timer = 0;
    });
    for _ in 0..5 {
        world.zone.advance_tick().unwrap();
    }
    assert!(!world.zone.npc_hidden(COBB));
    assert_eq!(world.progress(1, quest), [1, 2, 0]);
    // A quest action at Cobb faces him again: the choice stands and he fights.
    let events = world.act(1, COBB);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert!(world.zone.npc_hidden(COBB));
    assert_eq!(world.foe_life(), Some(CreatureLife::Alive));
    assert_eq!(world.progress(1, quest), [1, 2, 0]);
}

/// A character on *A Matter of Belief* by Maud: `(player, class, staff in hand)`, with `extra`
/// turned in.
fn believer(players: &[(u32, u8, bool)], extra: &[u32]) -> World {
    let mut done = vec![63, 64, 65];
    done.extend_from_slice(extra);
    let mut world = World::new(players, &before(&done));
    for &(player, ..) in players {
        world.accept(player, GWEN, quests::A_MATTER_OF_BELIEF);
    }
    world
}

#[test]
fn maud_refuses_a_shown_staff_with_its_reason_and_hears_its_bearer_once_it_is_put_away() {
    let quest = quests::A_MATTER_OF_BELIEF;
    // Not a class rule: an Arcanist with the wand in hand and a Warden holding one alike.
    let mut world = believer(&[(1, ARCANIST, true), (2, WARDEN, true)], &[]);
    for player in [1, 2] {
        let events = world.act(player, MAUD);
        assert_eq!(refusal(&events), Some(ErrorCode::StaffShown), "{events:?}");
        assert_eq!(world.progress(player, quest), [0]);
    }
    // Put away into the bag, the staff no longer stands between them.
    world.send(
        1,
        ZoneCommand::UnequipItem {
            equipment_slot: EquipmentSlot::MainHand.index(),
        },
    );
    assert_eq!(world.carries(1, WAND), 1);
    let events = world.act(1, MAUD);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert_eq!(world.progress(1, quest), [1]);
    let events = world.turn_in(1, MAUD, quest);
    assert_eq!(refusal(&events), None, "{events:?}");
}

#[test]
fn a_trusted_party_mate_or_gwens_word_has_maud_hear_a_staff_bearer() {
    let quest = quests::A_MATTER_OF_BELIEF;
    // A party mate without a staff speaks for the Arcanist; it completes nothing for the mate.
    let mut world = believer(&[(1, ARCANIST, true), (2, WARDEN, false)], &[]);
    world.party(&[1, 2]);
    world.beside(2, MAUD);
    let events = world.act(1, MAUD);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert_eq!(world.progress(1, quest), [1]);
    assert_eq!(world.progress(2, quest), [0]);
    // A mate showing a staff too speaks for nobody.
    let mut world = believer(&[(1, ARCANIST, true), (2, WARDEN, true)], &[]);
    world.party(&[1, 2]);
    world.beside(2, MAUD);
    assert_eq!(refusal(&world.act(1, MAUD)), Some(ErrorCode::StaffShown));
    // Alone, Gwen's word (her grain brought back, quest 15) is enough.
    let mut world = believer(&[(1, ARCANIST, true)], &[15]);
    let events = world.act(1, MAUD);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert_eq!(world.progress(1, quest), [1]);
}
