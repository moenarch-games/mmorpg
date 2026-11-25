//! The debt and the Downs road (#1019, slice 5b): Gwen's unpaid round traced between Corin and
//! Tobin and settled by the player's choice (67), the Downs signal lit and the supply patrol
//! watched from the lookout (68), the patrol stopped before its camp for the packbearer's order
//! pouch, or failed when it arrives and called again (69), and the peace Maud sends Gwen once the
//! torc and the slate are settled (70).
//!
//! Every step is a server-checked command or the zone's own tick; refusals change nothing, each
//! objective is credited once, and every `stand_at` restores the zone from its canonical snapshot,
//! so recovery carries the patrol's march.
use crate::support::{arena, classes};

use std::collections::BTreeMap;
use std::sync::Arc;

use mmorpg_core::greyhaven_vale::{self, downs, quests};
use mmorpg_core::{
    CharacterId, CharacterLocation, CreatureId, CreatureLife, EntityRef, ErrorCode, ItemId, NpcId,
    QuestId, QuestLog, ZoneCommand, ZoneEvent, ZoneId, ZoneSimulation, sell_price,
};

const GREYHAVEN: ZoneId = ZoneId::new(1);
const WARDEN: u8 = 0;
const GWEN: NpcId = quests::ALEWIFE;
const MAUD: NpcId = quests::OLD_MAUD;
const BRENNA: NpcId = downs::LOOKOUT;

/// The quests before *The Unpaid Ale*: the opening, the region's 1-16 and Maud's first three.
fn before(extra: &[u32]) -> Vec<QuestId> {
    (1..=16)
        .chain([63, 64, 65])
        .chain(extra.iter().copied())
        .map(QuestId::new)
        .collect()
}

struct World {
    zone: ZoneSimulation,
    sequences: BTreeMap<u32, u32>,
}

impl World {
    /// Wardens `players`, each with `copper` and having turned in `completed`, admitted at level 6
    /// by the Marshal.
    fn new(players: &[u32], completed: &[QuestId], copper: u32) -> Self {
        let content = greyhaven_vale::content();
        let mut zone = ZoneSimulation::with_content(GREYHAVEN, Arc::clone(&content)).unwrap();
        for &player in players {
            let mut donor = ZoneSimulation::with_content(GREYHAVEN, Arc::clone(&content)).unwrap();
            donor.add_player(player).unwrap();
            let mut donor = classes::at_level(donor, player, 6);
            donor
                .apply_command(
                    player,
                    1,
                    ZoneCommand::ChooseClass {
                        class: WARDEN,
                        sex: 0,
                    },
                )
                .unwrap();
            donor.advance_tick().unwrap();
            let mut record = donor.capture_character(player, id(player)).unwrap();
            record.quests = QuestLog {
                completed: completed.iter().copied().collect(),
                ..QuestLog::default()
            };
            record.copper = copper;
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
            sequences: players.iter().map(|&player| (player, 10)).collect(),
        }
    }

    fn send(&mut self, player: u32, command: ZoneCommand) -> Vec<ZoneEvent> {
        let sequence = self.sequences.entry(player).or_default();
        *sequence += 1;
        self.zone.apply_command(player, *sequence, command).unwrap();
        self.zone.advance_tick().unwrap();
        self.zone.snapshot_for_player(player).unwrap().events
    }

    fn accept(&mut self, player: u32, npc: NpcId, quest: QuestId) -> Vec<ZoneEvent> {
        self.beside(player, npc);
        self.send(
            player,
            ZoneCommand::AcceptQuest {
                npc,
                quest: quest.get(),
            },
        )
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

    fn life(&self, creature: CreatureId) -> Option<CreatureLife> {
        self.zone
            .snapshot()
            .unwrap()
            .creatures
            .into_iter()
            .find(|record| record.creature_id == creature)
            .map(|record| record.life)
    }

    fn feet(&self, creature: CreatureId) -> [i32; 2] {
        let record = self
            .zone
            .snapshot()
            .unwrap()
            .creatures
            .into_iter()
            .find(|record| record.creature_id == creature)
            .unwrap();
        [
            record.position[0].round() as i32,
            record.position[2].round() as i32,
        ]
    }

    /// Lights the beacon with the supplied tinder.
    fn light(&mut self, player: u32) -> Vec<ZoneEvent> {
        let [x, _, z] = self
            .zone
            .content()
            .evidence_spot(quests::DOWNS_BEACON)
            .unwrap()
            .at;
        self.stand_at(player, [x, z]);
        self.send(
            player,
            ZoneCommand::Inspect {
                spot: quests::DOWNS_BEACON.get(),
            },
        )
    }

    /// Advances up to `ticks` ticks until `done` holds; whether it did.
    fn until(&mut self, ticks: u64, mut done: impl FnMut(&Self) -> bool) -> bool {
        for _ in 0..ticks {
            if done(self) {
                return true;
            }
            self.zone.advance_tick().unwrap();
        }
        done(self)
    }
}

fn id(player: u32) -> CharacterId {
    CharacterId::new(format!("downs-{player}")).unwrap()
}

fn refusal(events: &[ZoneEvent]) -> Option<ErrorCode> {
    events.iter().find_map(|event| match event {
        ZoneEvent::Error { code, .. } => Some(*code),
        _ => None,
    })
}

#[test]
fn the_beacon_and_the_lookout_stand_above_the_road_the_patrol_walks() {
    let content = greyhaven_vale::content();
    let beacon = content.evidence_spot(quests::DOWNS_BEACON).unwrap();
    let ground = content.definition().ground_at(beacon.at[0], beacon.at[2]);
    assert!((beacon.at[1] - ground).abs() <= 50, "{}", beacon.name);
    let patrol = content.supply_patrol(BRENNA).unwrap();
    assert_eq!(patrol.carrier(), downs::PACKBEARER);
    assert!(content.routine(BRENNA).is_none());
    // The beacon is by the lookout, the road's formation point and camp out of her sight, the
    // road in between within it.
    let lookout = content.npc(BRENNA).unwrap().position;
    let far = |point: [i32; 2]| {
        let (dx, dz) = (
            i64::from(point[0] - lookout[0]),
            i64::from(point[1] - lookout[1]),
        );
        dx * dx + dz * dz
    };
    assert!(far([beacon.at[0], beacon.at[2]]) < 500_i64.pow(2));
    let sight = i64::from(patrol.sight).pow(2);
    assert!(far(patrol.route[0]) > sight);
    assert!(far(*patrol.route.last().unwrap()) > sight);
    assert!(patrol.route.iter().any(|&point| far(point) <= sight * 2));
    // The patrol's members are reserved: absent until called out.
    let zone = ZoneSimulation::with_content(GREYHAVEN, Arc::clone(&content)).unwrap();
    let state = zone.snapshot().unwrap();
    for member in &patrol.members {
        let record = state
            .creatures
            .iter()
            .find(|record| record.creature_id == *member)
            .unwrap();
        assert!(matches!(record.life, CreatureLife::Despawned { .. }));
    }
    assert_eq!(sell_price(quests::SIGNAL_TINDER), 0);
    assert_eq!(sell_price(quests::ORDER_POUCH), 0);
    // The catalogue holds IDs 1-70 first; later slices add theirs after them.
    let ids: Vec<u32> = content
        .quests()
        .iter()
        .map(|quest| quest.id.get())
        .collect();
    assert_eq!(ids[..70], (1..=70).collect::<Vec<_>>());
}

#[test]
fn the_unpaid_ale_is_split_between_corin_and_tobin_or_paid_by_the_player() {
    let quest = quests::THE_UNPAID_ALE;
    let mut world = World::new(&[1, 2], &before(&[]), 20);
    for player in [1, 2] {
        assert_eq!(refusal(&world.accept(player, GWEN, quest)), None);
        // Gwen will not settle before both have been heard.
        assert_eq!(
            refusal(&world.decide(player, GWEN, 1)),
            Some(ErrorCode::InvalidQuest)
        );
        for npc in [quests::SHEPHERD_CORIN, quests::FARMER_TOBIN] {
            let events = world.act(player, npc);
            assert_eq!(refusal(&events), None, "{events:?}");
        }
        assert_eq!(world.progress(player, quest), [1, 1, 0]);
    }
    // Split between them: free.
    let events = world.decide(1, GWEN, 1);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert_eq!(world.progress(1, quest), [1, 1, 1]);
    assert_eq!(world.copper(1), 20);
    // Paid by the player: the round's 15 copper go at once.
    let events = world.decide(2, GWEN, 2);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert_eq!(world.progress(2, quest), [1, 1, 2]);
    assert_eq!(world.copper(2), 5);
    // Settled once: a second choice changes nothing.
    assert_eq!(
        refusal(&world.decide(2, GWEN, 1)),
        Some(ErrorCode::InvalidQuest)
    );
    for (player, outcome) in [(1, 1), (2, 2)] {
        let events = world.turn_in(player, GWEN, quest, 0);
        assert_eq!(refusal(&events), None, "{events:?}");
        assert_eq!(world.log(player).outcome(quest), Some(outcome));
    }
}

#[test]
fn a_player_without_the_rounds_copper_cannot_pay_it() {
    let quest = quests::THE_UNPAID_ALE;
    let mut world = World::new(&[1], &before(&[]), 10);
    world.accept(1, GWEN, quest);
    world.act(1, quests::SHEPHERD_CORIN);
    world.act(1, quests::FARMER_TOBIN);
    assert_eq!(
        refusal(&world.decide(1, GWEN, 2)),
        Some(ErrorCode::NotEnoughMoney)
    );
    assert_eq!(world.progress(1, quest), [1, 1, 0]);
    assert_eq!(world.copper(1), 10);
}

#[test]
fn the_roadside_signal_lights_the_beacon_and_watches_the_patrol_come_up_the_road() {
    let quest = quests::THE_ROADSIDE_SIGNAL;
    let mut world = World::new(&[1], &before(&[67]), 0);
    assert_eq!(refusal(&world.accept(1, GWEN, quest)), None);
    assert_eq!(world.carries(1, quests::SIGNAL_TINDER), 1);
    // Before the beacon is lit Brenna calls nothing out.
    assert_eq!(
        refusal(&world.act(1, BRENNA)),
        Some(ErrorCode::InvalidQuest)
    );
    assert!(world.zone.supply_run(BRENNA).is_none());
    let events = world.light(1);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert_eq!(world.progress(1, quest), [1, 0]);
    assert_eq!(world.carries(1, quests::SIGNAL_TINDER), 0);
    // Asked to keep watch, she calls the patrol out: it forms up on the road out of her sight.
    let events = world.act(1, BRENNA);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert_eq!(world.progress(1, quest), [1, 0]);
    let run = world.zone.supply_run(BRENNA).unwrap().clone();
    assert_eq!(run.rests_until, 0);
    for member in &world.zone.content().supply_patrol(BRENNA).unwrap().members {
        assert_eq!(world.life(*member), Some(CreatureLife::Alive));
    }
    // Asked again while it marches, nothing changes.
    let events = world.act(1, BRENNA);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert_eq!(world.zone.supply_run(BRENNA).unwrap().started, run.started);
    // It walks down the road toward the lookout, and the watcher beside her sees it come.
    let start = world.feet(downs::PACKBEARER);
    let sighted = world.until(mmorpg_core::golden::t30(1_500), |world| {
        world.progress(1, quest) == [1, 1]
    });
    assert!(sighted, "the patrol never came into sight");
    let now = world.feet(downs::PACKBEARER);
    let walked = |a: [i32; 2], b: [i32; 2]| (a[0] - b[0]).abs() + (a[1] - b[1]).abs();
    assert!(walked(start, now) > 1_000, "{start:?} -> {now:?}");
    let events = world.turn_in(1, GWEN, quest, 0);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert!(world.log(1).is_completed(quest));
}

/// A character on *The Downs Ambush* who has called the patrol out at Brenna.
fn ambush() -> World {
    let mut world = World::new(&[1], &before(&[67, 68]), 0);
    assert_eq!(
        refusal(&world.accept(1, GWEN, quests::THE_DOWNS_AMBUSH)),
        None
    );
    let events = world.act(1, BRENNA);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert!(world.zone.supply_run(BRENNA).is_some());
    world
}

#[test]
fn the_downs_ambush_stops_the_packbearer_on_the_road_for_his_order_pouch() {
    let quest = quests::THE_DOWNS_AMBUSH;
    let mut world = ambush();
    // The march survives recovery: the player meets the column on the road, and the packbearer,
    // nearly beaten, falls there.
    world.until(mmorpg_core::golden::t30(300), |_| false);
    let at = world.feet(downs::PACKBEARER);
    world.stand_at(1, [at[0] + 150, at[1]]);
    world.restore(|state| {
        let carrier = state
            .creatures
            .iter_mut()
            .find(|record| record.creature_id == downs::PACKBEARER)
            .unwrap();
        carrier.health = 1;
    });
    let target = EntityRef::Creature(downs::PACKBEARER);
    world.send(1, ZoneCommand::SelectTarget(Some(target)));
    for _ in 0..mmorpg_core::golden::t30(600) {
        if world.carries(1, quests::ORDER_POUCH) == 1 {
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
    assert_eq!(world.carries(1, quests::ORDER_POUCH), 1);
    // Stopped on the road: the ambush is done, once (the pouch counts from the bag), and the
    // patrol's run is over.
    assert_eq!(world.progress(1, quest), [1, 0]);
    let run = world.zone.supply_run(BRENNA).unwrap();
    assert!(run.rests_until > 0);
    world.send(1, ZoneCommand::SetWeaponDrawn(false));
    assert_eq!(
        refusal(&world.act(1, BRENNA)),
        Some(ErrorCode::InvalidQuest)
    );
    let events = world.turn_in(1, GWEN, quest, 1);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert!(world.log(1).is_completed(quest));
    assert_eq!(world.carries(1, quests::ORDER_POUCH), 0);
}

#[test]
fn a_patrol_that_reaches_its_camp_fails_the_ambush_and_is_called_again() {
    let quest = quests::THE_DOWNS_AMBUSH;
    let mut world = ambush();
    // The player waits at Brenna's post while the column is already near the camp.
    let patrol = world.zone.content().supply_patrol(BRENNA).unwrap().clone();
    let last = patrol.route.len() - 1;
    let camp = patrol.route[last];
    let content = Arc::clone(world.zone.content());
    world.restore(|state| {
        let run = state
            .supply_runs
            .iter_mut()
            .find(|run| run.npc == BRENNA)
            .unwrap();
        for (index, member) in patrol.members.iter().enumerate() {
            let at = [camp[0] - 200 * (index as i32 + 1), camp[1] - 300];
            let y = content.definition().ground_at(at[0], at[1]) + 100;
            let record = state
                .creatures
                .iter_mut()
                .find(|record| record.creature_id == *member)
                .unwrap();
            record.position = [at[0] as f32, y as f32, at[1] as f32];
            record.velocity = [0.0; 3];
            run.marches[index].waypoint = last as u8;
            run.marches[index].anchor = at;
        }
    });
    let mut heard = Vec::new();
    let arrived = world.until(mmorpg_core::golden::t30(300), |world| {
        world
            .zone
            .supply_run(BRENNA)
            .is_some_and(|run| run.rests_until > 0)
    });
    assert!(arrived, "the patrol never reached its camp");
    heard.extend(world.zone.snapshot_for_player(1).unwrap().events);
    assert!(
        heard.iter().any(|event| matches!(
            event,
            ZoneEvent::Error {
                code: ErrorCode::PatrolArrived,
                target: Some(EntityRef::Npc(npc)),
            } if *npc == BRENNA
        )),
        "{heard:?}"
    );
    assert_eq!(world.progress(1, quest), [0, 0]);
    // The column goes into the camp, and the patrol rests before Brenna can call the next.
    world.zone.advance_tick().unwrap();
    for member in &patrol.members {
        assert!(matches!(
            world.life(*member),
            Some(CreatureLife::Despawned { .. })
        ));
    }
    assert_eq!(
        refusal(&world.act(1, BRENNA)),
        Some(ErrorCode::PatrolResting)
    );
    // Rested (a restored zone keeps the rest's end), the next patrol is called out: the retry.
    let tick = world.zone.snapshot().unwrap().tick;
    world.restore(|state| {
        state
            .supply_runs
            .iter_mut()
            .find(|run| run.npc == BRENNA)
            .unwrap()
            .rests_until = tick;
    });
    world.zone.advance_tick().unwrap();
    assert!(world.zone.supply_run(BRENNA).is_none());
    let events = world.act(1, BRENNA);
    assert_eq!(refusal(&events), None, "{events:?}");
    let run = world.zone.supply_run(BRENNA).unwrap();
    assert_eq!(run.rests_until, 0);
    assert!(run.marches.iter().all(|march| march.waypoint <= 1));
    assert_eq!(world.life(downs::PACKBEARER), Some(CreatureLife::Alive));
}

#[test]
fn peace_at_the_rest_needs_the_torc_home_and_the_slate_settled() {
    let quest = quests::PEACE_AT_THE_REST;
    // The slate settled but the torc still out: Maud offers nothing.
    let mut world = World::new(
        &[1],
        &[
            (1..=16).map(QuestId::new).collect::<Vec<_>>(),
            vec![QuestId::new(63), QuestId::new(64), quests::THE_UNPAID_ALE],
        ]
        .concat(),
        0,
    );
    assert_eq!(
        refusal(&world.accept(1, MAUD, quest)),
        Some(ErrorCode::InvalidQuest)
    );
    // Both settled.
    let mut world = World::new(&[1], &before(&[67]), 0);
    let events = world.accept(1, MAUD, quest);
    assert_eq!(refusal(&events), None, "{events:?}");
    for npc in [quests::SHEPHERD_CORIN, quests::FARMER_TOBIN] {
        world.beside(1, npc);
    }
    assert_eq!(world.progress(1, quest), [1, 1]);
    // Maud's word goes to Gwen, not back to Maud.
    assert_eq!(
        refusal(&world.turn_in(1, MAUD, quest, 0)),
        Some(ErrorCode::InvalidQuest)
    );
    let events = world.turn_in(1, GWEN, quest, 0);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert!(world.log(1).is_completed(quest));
}
