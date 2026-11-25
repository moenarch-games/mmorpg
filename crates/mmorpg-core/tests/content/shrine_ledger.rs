//! The Core Vale's shrine and ledger (#1019, slice 3b): a rubbing of the drowned inscription
//! (47), the two glyphs that sink the shrine's seal and the seal closing again (48), the warded
//! Shrine Warden whose weak point the vault glyph exposes (49), and the ledger's two endings for
//! Watchman Garth (50), kept with the character.
//!
//! Every step is a server-checked command; refusals change nothing, each objective is credited
//! once and stored in the log or the bag, and every `stand_at` restores the zone from its
//! canonical snapshot, so recovery carries the glyphs, the seal, the ward and the decision.
use crate::support;

use std::collections::BTreeMap;
use std::sync::Arc;

use mmorpg_core::greyhaven_vale::{self, quests, shrine::vault};
use mmorpg_core::{
    CharacterId, CharacterLocation, CreatureLife, EntityKind, EntityRef, ErrorCode, EvidenceId,
    ItemId, NpcId, QuestId, QuestLog, SEAL_OPEN_TICKS, SEAL_WINDOW_TICKS, ZoneCommand, ZoneEvent,
    ZoneId, ZoneSimulation,
};
use support::{arena, classes};

const GREYHAVEN: ZoneId = ZoneId::new(1);
const WARDEN_CLASS: u8 = 0;
const RUBBING: ItemId = ItemId::new(42);
const GARTH: NpcId = NpcId::new(34);
const DAVIN: NpcId = NpcId::new(13);
const INSCRIPTION: u32 = 9;
const TOLL_BOOK: u32 = 10;
const DROWNED: QuestId = QuestId::new(47);
const SEAL: QuestId = QuestId::new(48);
const GUARDIAN: QuestId = QuestId::new(49);
const LEDGER: QuestId = QuestId::new(50);

struct World {
    zone: ZoneSimulation,
    sequences: BTreeMap<u32, u32>,
}

impl World {
    /// Characters `(player, level, turned-in quests)` admitted at the Marshal.
    fn new(characters: &[(u32, u8, Vec<QuestId>)]) -> Self {
        let content = greyhaven_vale::content();
        let mut zone = ZoneSimulation::with_content(GREYHAVEN, Arc::clone(&content)).unwrap();
        for (player, level, completed) in characters {
            let mut donor = ZoneSimulation::with_content(GREYHAVEN, Arc::clone(&content)).unwrap();
            donor.add_player(*player).unwrap();
            let mut donor = classes::at_level(donor, *player, *level);
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
        let mut world = Self {
            zone,
            sequences: characters.iter().map(|c| (c.0, 10)).collect(),
        };
        for (player, ..) in characters {
            world.beside(*player, quests::MARSHAL);
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

    fn accept(&mut self, player: u32, quest: QuestId) {
        self.beside(player, quests::MARSHAL);
        let events = self.send(
            player,
            ZoneCommand::AcceptQuest {
                npc: quests::MARSHAL,
                quest: quest.get(),
            },
        );
        assert_eq!(refusal(&events), None, "accepting {quest:?}: {events:?}");
    }

    fn turn_in(&mut self, player: u32, quest: QuestId, choice: u8) -> Vec<ZoneEvent> {
        self.beside(player, quests::MARSHAL);
        self.send(
            player,
            ZoneCommand::CompleteQuest {
                npc: quests::MARSHAL,
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

    /// Moves `player`'s body to stand (or float, under water) at `[x, z]`.
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

    fn npc_seen(&self, player: u32, npc: NpcId) -> bool {
        self.zone
            .snapshot_for_player(player)
            .unwrap()
            .entities
            .iter()
            .any(|entity| entity.kind == EntityKind::Npc && entity.id == npc.get())
    }

    fn glyph(&self, id: mmorpg_core::MechanismId) -> [i32; 3] {
        self.zone.content().mechanism(id).unwrap().at
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
    CharacterId::new(format!("shrine-{player}")).unwrap()
}

fn refusal(events: &[ZoneEvent]) -> Option<ErrorCode> {
    events.iter().find_map(|event| match event {
        ZoneEvent::Error { code, .. } => Some(*code),
        _ => None,
    })
}

/// Quests 1 to `last` turned in, and `extra`.
fn through(last: u32, extra: &[u32]) -> Vec<QuestId> {
    (1..=last)
        .chain(extra.iter().copied())
        .map(QuestId::new)
        .collect()
}

#[test]
fn a_diver_takes_a_rubbing_of_the_drowned_inscription_and_the_marshal_reads_it() {
    let mut world = World::new(&[(1, 5, through(10, &[]))]);
    world.accept(1, DROWNED);
    let spot = world
        .zone
        .content()
        .evidence_spot(EvidenceId::new(INSCRIPTION))
        .unwrap()
        .at;
    // From the lake's surface the inscription is out of reach: a diver must go down to it.
    world.restore(|state| {
        let record = &mut state.players[0];
        record.position = [spot[0] as f32, (spot[1] + 700) as f32, spot[2] as f32];
    });
    let events = world.send(1, ZoneCommand::Inspect { spot: INSCRIPTION });
    assert_eq!(refusal(&events), Some(ErrorCode::OutOfRange));
    assert_eq!(world.carries(1, RUBBING), 0);

    world.at_spot(1, spot);
    let events = world.send(1, ZoneCommand::Inspect { spot: INSCRIPTION });
    assert_eq!(refusal(&events), None, "{events:?}");
    assert_eq!(world.carries(1, RUBBING), 1);
    assert_eq!(
        world.progress(1, DROWNED),
        [0],
        "progress is the bag's count"
    );
    // A second rubbing is refused while the bag holds one, and no vendor buys it.
    let events = world.send(1, ZoneCommand::Inspect { spot: INSCRIPTION });
    assert_eq!(refusal(&events), Some(ErrorCode::InvalidQuest));
    assert_eq!(world.carries(1, RUBBING), 1);
    assert_eq!(mmorpg_core::sell_price(RUBBING), 0);

    // A saved and reloaded character keeps the rubbing, and the Marshal takes it.
    world.reload(1);
    assert_eq!(world.carries(1, RUBBING), 1);
    let events = world.turn_in(1, DROWNED, 0);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert_eq!(world.carries(1, RUBBING), 0);
    assert!(world.log(1).is_completed(DROWNED));
    // The relic's quest is untouched: its altar still stands before the seal.
    assert!(world.log(1).is_completed(quests::THE_SUNKEN_SHRINE));
}

#[test]
fn two_glyphs_sink_the_seal_for_their_openers_and_it_rises_again() {
    let mut world = World::new(&[(1, 5, through(10, &[47])), (2, 5, through(10, &[47]))]);
    world.accept(1, SEAL);
    world.accept(2, SEAL);
    let (lakeward, cave) = (
        world.glyph(vault::LAKEWARD_GLYPH),
        world.glyph(vault::CAVE_GLYPH),
    );
    let seal = |world: &World| world.zone.seal_views()[0];

    // One glyph alone drops back after its window: a clear retry.
    world.at_spot(1, lakeward);
    let events = world.send(
        1,
        ZoneCommand::Operate {
            mechanism: vault::LAKEWARD_GLYPH.get(),
        },
    );
    assert_eq!(refusal(&events), None, "{events:?}");
    assert_eq!((seal(&world).set, seal(&world).open), (0b01, false));
    // (Waiting out the window on the shore, not drowning beside the glyph.)
    world.beside(1, quests::MARSHAL);
    world.ticks(SEAL_WINDOW_TICKS);
    assert_eq!(seal(&world).set, 0, "the glyph dropped back");
    world.at_spot(1, lakeward);

    // Together, within the window, the two glyphs sink it, and both partners opened it.
    world.send(
        1,
        ZoneCommand::Operate {
            mechanism: vault::LAKEWARD_GLYPH.get(),
        },
    );
    world.at_spot(2, cave);
    let events = world.send(
        2,
        ZoneCommand::Operate {
            mechanism: vault::CAVE_GLYPH.get(),
        },
    );
    assert_eq!(refusal(&events), None, "{events:?}");
    assert!(seal(&world).open);
    assert_eq!(world.progress(1, SEAL), [1]);
    assert_eq!(world.progress(2, SEAL), [1]);
    // The open seal's glyphs rest; working one again is refused and credits nothing twice.
    let events = world.send(
        2,
        ZoneCommand::Operate {
            mechanism: vault::CAVE_GLYPH.get(),
        },
    );
    assert_eq!(refusal(&events), Some(ErrorCode::SealOpen));

    // A diver in the vault holds it open past its time; once out, it rises and every glyph is
    // unset again.
    let vault_floor = world.glyph(vault::VAULT_GLYPH);
    world.at_spot(1, vault_floor);
    // (Five minutes pass at once.)
    world.restore(|state| state.tick += u64::from(SEAL_OPEN_TICKS));
    world.ticks(3);
    assert!(seal(&world).open, "a living player inside holds it open");
    world.at_spot(1, lakeward);
    world.ticks(2);
    assert_eq!((seal(&world).set, seal(&world).open), (0, false));

    for player in [1, 2] {
        let events = world.turn_in(player, SEAL, 0);
        assert_eq!(refusal(&events), None, "player {player}: {events:?}");
    }
}

#[test]
fn the_warden_turns_every_blow_aside_until_the_vault_glyph_bares_it() {
    let mut world = World::new(&[(1, 6, through(10, &[47, 48]))]);
    world.accept(1, GUARDIAN);
    let warden = vault::WARDEN;
    let target = EntityRef::Creature(warden);
    world.restore(|state| {
        let creature = state
            .creatures
            .iter_mut()
            .find(|creature| creature.creature_id == warden)
            .unwrap();
        creature.health = 1;
    });
    let spawn = world
        .zone
        .content()
        .creature_spawns()
        .iter()
        .find(|spawn| spawn.id == warden)
        .unwrap()
        .position;
    let strike = |world: &mut World| -> (bool, Vec<ZoneEvent>) {
        let mut seen = Vec::new();
        world.send(1, ZoneCommand::SelectTarget(Some(target)));
        for _ in 0..mmorpg_core::golden::t30(120) {
            let dead = world
                .zone
                .snapshot()
                .unwrap()
                .creatures
                .iter()
                .any(|c| c.creature_id == warden && c.life != CreatureLife::Alive);
            if dead {
                return (true, seen);
            }
            match arena::swing_at(&world.zone, 1, target) {
                Some(swing) => seen.extend(world.send(1, swing)),
                None => world.ticks(1),
            }
        }
        (false, seen)
    };

    // Warded, a swing that lands is turned aside.
    world.stand_at(1, [spawn[0] + 120, spawn[1]]);
    let (dead, events) = strike(&mut world);
    assert!(!dead, "the ward holds");
    assert!(events.contains(&ZoneEvent::Error {
        code: ErrorCode::Warded,
        target: Some(target),
    }));
    assert_eq!(world.zone.ward_views()[0].exposed, 0);

    // The vault glyph bares its weak point; the next blow lands, and the kill counts.
    world.at_spot(1, world.glyph(vault::VAULT_GLYPH));
    let events = world.send(
        1,
        ZoneCommand::Operate {
            mechanism: vault::VAULT_GLYPH.get(),
        },
    );
    assert_eq!(refusal(&events), None, "{events:?}");
    assert!(world.zone.ward_views()[0].exposed > 0);
    world.stand_at(1, [spawn[0] + 120, spawn[1]]);
    let (dead, _) = strike(&mut world);
    assert!(dead, "the exposed warden falls");
    assert_eq!(world.progress(1, GUARDIAN), [1]);
    world.send(1, ZoneCommand::SetWeaponDrawn(false));
    let events = world.turn_in(1, GUARDIAN, 1);
    assert_eq!(refusal(&events), None, "{events:?}");
}

#[test]
fn the_ledger_ends_with_garth_exposed_or_kept_and_the_character_remembers() {
    // Quests 1–41 turned in: the courier unmasked (41), the tanner's ledger read (36) and
    // Garth's own Greyfang quests done (25–28).
    let mut world = World::new(&[(1, 8, through(41, &[])), (2, 8, through(41, &[]))]);
    let book = world
        .zone
        .content()
        .evidence_spot(EvidenceId::new(TOLL_BOOK))
        .unwrap()
        .at;
    for (player, option) in [(1, 1), (2, 2)] {
        world.accept(player, LEDGER);
        // Garth hears no verdict before the evidence is in.
        world.beside(player, GARTH);
        let events = world.send(player, ZoneCommand::Decide { npc: GARTH, option });
        assert_eq!(refusal(&events), Some(ErrorCode::InvalidQuest));
        world.beside(player, DAVIN);
        world.ticks(1);
        world.at_spot(player, book);
        let events = world.send(player, ZoneCommand::Inspect { spot: TOLL_BOOK });
        assert_eq!(refusal(&events), None, "{events:?}");
        assert_eq!(world.progress(player, LEDGER), [1, 1, 0]);
        world.beside(player, GARTH);
        // An option the decision does not offer changes nothing.
        let events = world.send(
            player,
            ZoneCommand::Decide {
                npc: GARTH,
                option: 3,
            },
        );
        assert_eq!(refusal(&events), Some(ErrorCode::InvalidQuest));
        let events = world.send(player, ZoneCommand::Decide { npc: GARTH, option });
        assert_eq!(refusal(&events), None, "{events:?}");
        assert_eq!(world.progress(player, LEDGER), [1, 1, option]);
        let events = world.turn_in(player, LEDGER, 0);
        assert_eq!(refusal(&events), None, "{events:?}");
        assert_eq!(world.log(player).outcome(LEDGER), Some(option));
    }

    // Exposed, Garth has left his post for the one who exposed him; kept, he stays.
    world.beside(1, GARTH);
    world.beside(2, GARTH);
    assert!(!world.npc_seen(1, GARTH));
    assert!(world.npc_seen(2, GARTH));
    let events = world.send(1, ZoneCommand::QuestAction { npc: GARTH });
    assert_eq!(refusal(&events), Some(ErrorCode::InvalidTarget));
    // Both endings keep the main story reachable: nothing Garth gives is left undone.
    let content = Arc::clone(world.zone.content());
    for player in [1, 2] {
        let log = world.log(player);
        assert!(
            content
                .quests()
                .iter()
                .filter(|quest| quest.giver == GARTH || quest.ender == GARTH)
                .all(|quest| log.is_completed(quest.id))
        );
    }

    // The decision is the character's: a saved and reloaded character keeps it.
    world.reload(1);
    world.beside(1, GARTH);
    assert_eq!(world.log(1).outcome(LEDGER), Some(1));
    assert!(!world.npc_seen(1, GARTH));
}
