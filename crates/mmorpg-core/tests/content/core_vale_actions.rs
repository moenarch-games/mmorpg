//! The Core Vale's quest actions at NPCs (#1019, slice 2): a wounded watchman bandaged and
//! escorted home (35), the smith's trial arms lent and taken back (37), a distrustful resident
//! who hears an Arcanist only through a trusted party mate or a recommendation (40), a courier
//! who turns hostile when shown his pass (41) and a farmhand's medicine (42).
//!
//! Every action is a server-checked [`ZoneCommand::QuestAction`]; refusals change nothing,
//! each objective is credited once and stored in the log, so a canonical snapshot (every
//! `stand_at` here restores one) and a saved character carry it.
use crate::support;

use std::collections::BTreeMap;
use std::sync::Arc;

use mmorpg_core::greyhaven_vale::{self, quests};
use mmorpg_core::{
    CharacterId, CharacterLocation, CreatureLife, EntityKind, EntityRef, ErrorCode, ItemId, NpcId,
    PartyMember, PartyRoster, PartyState, QuestEntry, QuestId, QuestLog, ZoneCommand, ZoneEvent,
    ZoneId, ZoneSimulation,
};
use support::{arena, classes};

const GREYHAVEN: ZoneId = ZoneId::new(1);
const WARDEN: u8 = 0;
const ARCANIST: u8 = 2;
const CLERIC: u8 = 5;

struct World {
    zone: ZoneSimulation,
    sequences: BTreeMap<u32, u32>,
}

impl World {
    /// Characters `(player, class, level, turned-in quests)` admitted at the Marshal.
    fn new(characters: &[(u32, u8, u8, &[QuestId])]) -> Self {
        let content = greyhaven_vale::content();
        let mut zone = ZoneSimulation::with_content(GREYHAVEN, Arc::clone(&content)).unwrap();
        for &(player, class, level, completed) in characters {
            let mut donor = ZoneSimulation::with_content(GREYHAVEN, Arc::clone(&content)).unwrap();
            donor.add_player(player).unwrap();
            let mut donor = classes::at_level(donor, player, level);
            donor
                .apply_command(player, 1, ZoneCommand::ChooseClass { class, sex: 0 })
                .unwrap();
            donor.advance_tick().unwrap();
            let mut record = donor.capture_character(player, id(player)).unwrap();
            record.quests = QuestLog {
                entries: Vec::new(),
                completed: completed.iter().copied().collect(),
            };
            let [x, z] = content.npc(quests::MARSHAL).unwrap().position;
            record.location = CharacterLocation {
                zone_id: GREYHAVEN,
                position: [x + 100, 90, z + 100],
                facing: 0,
            };
            zone.admit_character(player, &record).unwrap();
        }
        zone.advance_tick().unwrap();
        let mut world = Self {
            zone,
            sequences: characters.iter().map(|c| (c.0, 10)).collect(),
        };
        for &(player, ..) in characters {
            world.beside(player, quests::MARSHAL);
        }
        world
    }

    fn send(&mut self, player: u32, command: ZoneCommand) -> Vec<ZoneEvent> {
        let sequence = self.sequences.entry(player).or_default();
        *sequence += 1;
        self.zone.apply_command(player, *sequence, command).unwrap();
        self.zone.advance_tick().unwrap();
        self.zone.snapshot_for_player(player).unwrap().events
    }

    fn act(&mut self, player: u32, npc: NpcId) -> Vec<ZoneEvent> {
        self.send(player, ZoneCommand::QuestAction { npc })
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

    /// Moves `player`'s body to stand at `[x, z]`, as walking there would.
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

    fn carries(&self, player: u32, item: ItemId) -> u32 {
        let record = self.zone.capture_character(player, id(player)).unwrap();
        record.inventory.count(item) + u32::from(record.equipment.slots().contains(&Some(item)))
    }

    fn ticks(&mut self, count: u32) {
        for _ in 0..count {
            self.zone.advance_tick().unwrap();
        }
    }

    fn npc_seen(&self, player: u32, npc: NpcId) -> bool {
        self.zone
            .snapshot_for_player(player)
            .unwrap()
            .entities
            .iter()
            .any(|entity| entity.kind == EntityKind::Npc && entity.id == npc.get())
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
}

fn id(player: u32) -> CharacterId {
    CharacterId::new(format!("vale-{player}")).unwrap()
}

fn refusal(events: &[ZoneEvent]) -> Option<ErrorCode> {
    events.iter().find_map(|event| match event {
        ZoneEvent::Error { code, .. } => Some(*code),
        _ => None,
    })
}

fn progressed(events: &[ZoneEvent], quest: QuestId, objective: u8) -> bool {
    events.iter().any(|event| {
        matches!(event, ZoneEvent::QuestProgress { quest: q, objective: o, .. }
            if *q == quest && *o == objective)
    })
}

/// The Wolfrun Trail down from Tam's terrace: the cutting, the landing, the ledge and the
/// west gate into the Outpost.
const TRAIL: [[i32; 2]; 6] = [
    [-5_400, 1_200],
    [-5_400, -1_800],
    [-4_200, -1_800],
    [-4_200, 2_135],
    [-3_400, 2_135],
    [-2_800, 2_135],
];

#[test]
fn a_wounded_watchman_is_bandaged_and_led_home_and_a_lost_escort_is_retried() {
    let quest = quests::A_GUARDS_LAST_WATCH;
    let tam = quests::TAM_HOLLIS;
    let mut world = World::new(&[(
        1,
        WARDEN,
        4,
        &[quests::THE_WOLFRUN_CART, quests::CUT_SACKS_RED_RAGS],
    )]);
    world.accept(1, quests::MARSHAL, quest);
    // The Marshal hands over the bandage, once: abandoning takes it back, retaking gives one.
    assert_eq!(world.carries(1, quests::CLEAN_BANDAGE), 1);
    assert_eq!(refusal(&world.act(1, tam)), Some(ErrorCode::OutOfRange));

    // At Tam the action bandages him first: the escort waits for that step.
    world.beside(1, tam);
    let events = world.act(1, tam);
    assert!(progressed(&events, quest, 0), "{events:?}");
    assert_eq!(world.carries(1, quests::CLEAN_BANDAGE), 0);
    assert_eq!(world.progress(1, quest), [1, 0]);
    assert!(world.zone.escort(tam).is_none());

    // Then he follows; running off fails the escort and he is back on his terrace.
    assert_eq!(refusal(&world.act(1, tam)), None);
    assert!(world.zone.escort(tam).is_some());
    world.stand_at(1, [0, 2_000]);
    let mut events = Vec::new();
    for _ in 0..=mmorpg_core::ESCORT_LEASH_TICKS {
        world.zone.advance_tick().unwrap();
        events.extend(world.zone.snapshot_for_player(1).unwrap().events);
    }
    assert_eq!(refusal(&events), Some(ErrorCode::EscortLost), "{events:?}");
    assert!(world.zone.escort(tam).is_none());
    let post = world.zone.content().npc(tam).unwrap().position;
    assert_eq!(world.zone.npc_placement(tam).unwrap().position, post);
    assert_eq!(world.progress(1, quest), [1, 0]);

    // Retried, he follows the player's trail down the climb and in through the west gate.
    world.beside(1, tam);
    assert_eq!(refusal(&world.act(1, tam)), None);
    let mut at = world.zone.npc_placement(tam).unwrap().position;
    let mut arrived = Vec::new();
    for corner in TRAIL {
        while at != corner {
            let (dx, dz) = (corner[0] - at[0], corner[1] - at[1]);
            let length = f64::from(dx).hypot(f64::from(dz));
            let step = length.min(300.0) / length;
            at = [
                at[0] + (f64::from(dx) * step).round() as i32,
                at[1] + (f64::from(dz) * step).round() as i32,
            ];
            world.stand_at(1, at);
            // Wait for him to limp up behind before going on.
            for _ in 0..mmorpg_core::golden::t30(150) {
                let him = world.zone.npc_placement(tam).unwrap().position;
                if f64::from(him[0] - at[0]).hypot(f64::from(him[1] - at[1])) <= 400.0 {
                    break;
                }
                world.zone.advance_tick().unwrap();
                arrived.extend(world.zone.snapshot_for_player(1).unwrap().events);
            }
        }
    }
    world.ticks(mmorpg_core::golden::t30(90) as u32);
    let escort = world.zone.escort(tam).cloned().expect("he rests inside");
    assert!(escort.rests_until > 0, "{escort:?}");
    assert_eq!(world.progress(1, quest), [1, 1], "{arrived:?}");
    let him = world.zone.npc_placement(tam).unwrap().position;
    assert_eq!(
        world
            .zone
            .content()
            .areas()
            .area_at(him[0], him[1])
            .map(|area| area.id()),
        Some(greyhaven_vale::vale::OUTPOST)
    );
    // Done is done: he needs nothing more, and the credit is stored progress.
    world.beside(1, tam);
    assert_eq!(refusal(&world.act(1, tam)), Some(ErrorCode::InvalidQuest));
    assert_eq!(world.progress(1, quest), [1, 1]);

    let events = world.turn_in(1, quests::MARSHAL, quest);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert!(world.log(1).is_completed(quest));
    // His rest over, he is back at his post for the next rescuer.
    world.ticks(mmorpg_core::ESCORT_REST_TICKS);
    assert!(world.zone.escort(tam).is_none());
    assert_eq!(world.zone.npc_placement(tam).unwrap().position, post);
}

#[test]
fn a_distrustful_resident_hears_an_arcanist_only_through_a_trusted_mate_or_a_recommendation() {
    let quest = quests::A_QUIET_INTRODUCTION;
    let davin = quests::DAVIN_MOSS;
    let before = [
        quests::THE_WOLFRUN_CART,
        quests::CUT_SACKS_RED_RAGS,
        quests::THE_BAIT_TRAIL,
    ];
    let recommended = [
        quests::THE_WOLFRUN_CART,
        quests::CUT_SACKS_RED_RAGS,
        quests::THE_BAIT_TRAIL,
        quests::THE_TANNERS_LEDGER,
    ];
    let mut world = World::new(&[
        (1, ARCANIST, 4, &before),
        (2, WARDEN, 4, &before),
        (3, ARCANIST, 4, &recommended),
    ]);
    for player in [1, 2, 3] {
        world.accept(player, quests::MARSHAL, quest);
    }

    // Davin will not hear an Arcanist on their own: an actionable, authoritative refusal.
    world.beside(1, davin);
    let events = world.act(1, davin);
    assert!(
        events.contains(&ZoneEvent::Error {
            code: ErrorCode::Distrusted,
            target: Some(EntityRef::Npc(davin)),
        }),
        "{events:?}"
    );
    assert_eq!(world.progress(1, quest), [0]);

    // A party mate he trusts speaks for them only when beside him.
    world.party(&[1, 2]);
    assert_eq!(refusal(&world.act(1, davin)), Some(ErrorCode::Distrusted));
    world.beside(2, davin);
    world.beside(1, davin);
    let events = world.act(1, davin);
    assert!(progressed(&events, quest, 0), "{events:?}");
    assert_eq!(world.progress(1, quest), [1]);
    // Sponsoring completes nothing for the sponsor; the Warden is heard on their own.
    assert_eq!(world.progress(2, quest), [0]);
    assert!(progressed(&world.act(2, davin), quest, 0));

    // An Arcanist Hilda vouches for (The Tanner's Ledger) needs no sponsor.
    world.beside(3, davin);
    assert!(progressed(&world.act(3, davin), quest, 0));

    // The interview is stored progress: a saved character keeps it, and the turn-in follows.
    for player in [1, 2, 3] {
        let events = world.turn_in(player, quests::MARSHAL, quest);
        assert_eq!(refusal(&events), None, "{events:?}");
    }
}

#[test]
fn the_courier_turns_hostile_when_shown_his_pass_and_is_back_after_his_fall() {
    let quest = quests::THE_MAN_AT_THE_GATE;
    let courier = quests::COURIER;
    let mut world = World::new(&[(
        1,
        WARDEN,
        6,
        &[
            quests::THE_WOLFRUN_CART,
            quests::CUT_SACKS_RED_RAGS,
            quests::THE_BAIT_TRAIL,
            quests::A_QUIET_INTRODUCTION,
        ],
    )]);
    world.accept(1, quests::MARSHAL, quest);
    assert_eq!(world.carries(1, quests::FORGED_TOLL_PASS), 1);
    assert!(!world.zone.npc_hidden(courier));

    // Questioning him is talking to him; then the pass is shown and he draws steel.
    world.beside(1, courier);
    assert_eq!(world.progress(1, quest), [1, 0, 0]);
    let events = world.act(1, courier);
    assert!(progressed(&events, quest, 1), "{events:?}");
    assert_eq!(world.carries(1, quests::FORGED_TOLL_PASS), 0);
    assert!(world.zone.npc_hidden(courier));
    assert!(!world.npc_seen(1, courier));
    assert_eq!(
        refusal(&world.act(1, courier)),
        Some(ErrorCode::InvalidTarget)
    );
    let foe = world
        .zone
        .snapshot()
        .unwrap()
        .creatures
        .into_iter()
        .find(|creature| creature.creature_id == quests::COURIER_FOE)
        .unwrap();
    assert_eq!(foe.life, CreatureLife::Alive);
    assert!(
        foe.threat
            .iter()
            .any(|entry| entry.entity == EntityRef::Player(1)),
        "he attacks the one who showed him the pass: {:?}",
        foe.threat
    );

    // Felled (a restored zone keeps the fight), he counts for the quest's kill.
    world.restore(|state| {
        let foe = state
            .creatures
            .iter_mut()
            .find(|creature| creature.creature_id == quests::COURIER_FOE)
            .unwrap();
        foe.health = 1;
    });
    let target = EntityRef::Creature(quests::COURIER_FOE);
    world.send(1, ZoneCommand::SelectTarget(Some(target)));
    for _ in 0..mmorpg_core::golden::t30(300) {
        if world.progress(1, quest)[2] == 1 {
            break;
        }
        match arena::swing_at(&world.zone, 1, target) {
            Some(swing) => {
                world.send(1, swing);
            }
            None => world.ticks(1),
        }
    }
    assert_eq!(world.progress(1, quest), [1, 1, 1]);
    world.send(1, ZoneCommand::SetWeaponDrawn(false));
    let events = world.turn_in(1, quests::MARSHAL, quest);
    assert_eq!(refusal(&events), None, "{events:?}");

    // Once his body is gone he is back at the gate, his act resumed.
    world.ticks(mmorpg_core::unit::CORPSE_TICKS + 2);
    assert!(!world.zone.npc_hidden(courier));
    world.beside(1, courier);
    assert!(world.npc_seen(1, courier));
}

#[test]
fn the_smiths_trial_counts_kills_with_the_loaned_weapon_and_takes_it_back() {
    let quest = quests::THE_SMITHS_TRIAL;
    let chain = [
        quests::THE_WOLFRUN_CART,
        QuestId::new(3),
        QuestId::new(4),
        QuestId::new(5),
        QuestId::new(6),
    ];
    let mut world = World::new(&[
        (1, WARDEN, 5, &chain),
        (2, ARCANIST, 5, &chain),
        (3, CLERIC, 5, &chain),
    ]);
    // Each class borrows the trial arm it wields.
    for (player, loan) in [
        (1, quests::TRIAL_BLADE),
        (2, quests::TRIAL_STAFF),
        (3, quests::TRIAL_MACE),
    ] {
        world.accept(player, quests::MARSHAL, quest);
        assert_eq!(world.carries(player, loan), 1, "player {player}");
    }
    // Abandoning returns the loan.
    world.send(3, ZoneCommand::AbandonQuest { quest: quest.get() });
    assert_eq!(world.carries(3, quests::TRIAL_MACE), 0);

    let bandits: Vec<_> = world
        .zone
        .content()
        .creature_spawns()
        .iter()
        .filter(|spawn| spawn.template == greyhaven_vale::units::templates::REDBRAND_BANDIT)
        .filter(|spawn| {
            world
                .zone
                .content()
                .areas()
                .area_at(spawn.position[0], spawn.position[1])
                .is_some_and(|area| area.id() == greyhaven_vale::vale::REDBRAND_HOLLOW)
        })
        .map(|spawn| (spawn.id, spawn.position))
        .take(3)
        .collect();
    assert_eq!(bandits.len(), 3);
    let fell = |world: &mut World, (bandit, [x, z]): (mmorpg_core::CreatureId, [i32; 2])| {
        world.stand_at(1, [x + 80, z]);
        world.restore(|state| {
            let creature = state
                .creatures
                .iter_mut()
                .find(|creature| creature.creature_id == bandit)
                .unwrap();
            creature.health = 1;
        });
        let target = EntityRef::Creature(bandit);
        world.send(1, ZoneCommand::SelectTarget(Some(target)));
        for _ in 0..mmorpg_core::golden::t30(300) {
            let dead = world
                .zone
                .snapshot()
                .unwrap()
                .creatures
                .iter()
                .any(|c| c.creature_id == bandit && c.life != CreatureLife::Alive);
            if dead {
                break;
            }
            match arena::swing_at(&world.zone, 1, target) {
                Some(swing) => {
                    world.send(1, swing);
                }
                None => world.ticks(1),
            }
        }
        world.send(1, ZoneCommand::SetWeaponDrawn(false));
    };

    // A kill with the warden's own blade proves nothing about the smith's.
    fell(&mut world, bandits[0]);
    assert_eq!(world.progress(1, quest), [0, 0]);
    // With the trial blade in hand, each kill counts.
    let slot = world
        .zone
        .capture_character(1, id(1))
        .unwrap()
        .inventory
        .slots()
        .iter()
        .position(|stack| stack.is_some_and(|stack| stack.item() == quests::TRIAL_BLADE))
        .unwrap();
    world.send(
        1,
        ZoneCommand::EquipItem {
            bag_slot: u8::try_from(slot).unwrap(),
        },
    );
    fell(&mut world, bandits[1]);
    fell(&mut world, bandits[2]);
    assert_eq!(world.progress(1, quest), [2, 0]);
    world.beside(1, quests::SMITH);
    assert_eq!(world.progress(1, quest), [2, 1]);

    // The loan comes back at the turn-in, straight from the hand that holds it.
    let events = world.turn_in(1, quests::MARSHAL, quest);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert_eq!(world.carries(1, quests::TRIAL_BLADE), 0);

    // Without the loan there is no turn-in, whatever else is done.
    world.restore(|state| {
        let record = state
            .players
            .iter_mut()
            .find(|record| record.player_id == 2)
            .unwrap();
        record.inventory = mmorpg_core::Inventory::default();
        let entry = record
            .quests
            .entries
            .iter_mut()
            .find(|entry| entry.quest == quest)
            .unwrap();
        entry.progress = [2, 1, 0];
    });
    let events = world.turn_in(2, quests::MARSHAL, quest);
    assert_eq!(refusal(&events), Some(ErrorCode::QuestIncomplete));
}

#[test]
fn the_millers_medicine_goes_to_the_mill_and_then_to_jory() {
    let quest = quests::THE_MILLERS_MEDICINE;
    let mut world = World::new(&[(
        1,
        WARDEN,
        3,
        &[
            quests::THE_WOLFRUN_CART,
            QuestId::new(3),
            QuestId::new(4),
            QuestId::new(29),
        ],
    )]);
    world.accept(1, quests::HERBALIST, quest);
    assert_eq!(world.carries(1, quests::FEVER_DRAUGHT), 1);

    // Jory gets it only once the mill has the word: the action keeps the quest's order.
    world.beside(1, quests::FARMHAND_JORY);
    assert_eq!(world.progress(1, quest), [0, 0]);
    assert_eq!(
        refusal(&world.act(1, quests::FARMHAND_JORY)),
        Some(ErrorCode::InvalidQuest)
    );
    assert_eq!(world.carries(1, quests::FEVER_DRAUGHT), 1);
    world.beside(1, quests::MILLER);
    assert_eq!(world.progress(1, quest), [1, 0]);
    world.beside(1, quests::FARMHAND_JORY);
    let events = world.act(1, quests::FARMHAND_JORY);
    assert!(progressed(&events, quest, 1), "{events:?}");
    assert_eq!(world.carries(1, quests::FEVER_DRAUGHT), 0);
    let events = world.turn_in(1, quests::MILLER, quest);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert!(world.log(1).is_completed(quest));
    assert_eq!(
        world.log(1).entries,
        Vec::<QuestEntry>::new(),
        "nothing else open"
    );
}
