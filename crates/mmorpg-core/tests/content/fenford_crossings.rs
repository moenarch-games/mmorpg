//! Crossings under threat (#1019, slice 6b): Carter Aldo's cart seen over Fenford's ford while
//! Redbrands go for it, credited only when the cart reaches the far bank alive and tried again
//! when it falls (73); river crayfish brought down by blows outside their front arc, with Netter
//! Ilsa's lure turning one aside (76); her sister led off the Marsh Lake's island by the wading
//! line, balking at deep water (78); and the ford barricaded and held while the cart crosses
//! again (79).
//!
//! Every step is a server-checked command or the zone's own tick; refusals change nothing, each
//! objective is credited once, and every `stand_at` restores the zone from its canonical snapshot,
//! so recovery carries the crossing, the lure and the escort.
use crate::support::{arena, classes};

use std::collections::BTreeMap;
use std::sync::Arc;

use mmorpg_core::greyhaven_vale::{self, crayfish, fenford, quests, wilds};
use mmorpg_core::{
    CharacterId, CharacterLocation, CreatureId, CreatureLife, EntityRef, ErrorCode, EvidenceId,
    ItemId, LoadSpotId, NpcId, QuestId, QuestLog, ZoneCommand, ZoneEvent, ZoneId, ZoneSimulation,
    sell_price,
};

const GREYHAVEN: ZoneId = ZoneId::new(1);
const WARDEN: u8 = 0;
const OSK: NpcId = quests::FERRYMAN_OSK;
const ILSA: NpcId = quests::NETTER_ILSA;
const ALDO: NpcId = fenford::CARTER_ALDO;
const NELL: NpcId = fenford::REEDCUTTER_NELL;
const CROSSING: QuestId = quests::A_CROSSING_UNDER_FIRE;
const TEETH: QuestId = quests::THE_RIVERS_TEETH;
const REEDS: QuestId = quests::ACROSS_THE_REEDS;
const WATCH: QuestId = quests::THE_FORD_WATCH;

struct World {
    zone: ZoneSimulation,
    sequences: BTreeMap<u32, u32>,
}

impl World {
    /// Wardens `(player, turned-in quests)` admitted at level 6 beside the Marshal.
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
                        class: WARDEN,
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

    fn inspect(&mut self, player: u32, spot: EvidenceId) -> Vec<ZoneEvent> {
        let at = self.zone.content().evidence_spot(spot).unwrap().at;
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

    fn creature(&self, creature: CreatureId) -> mmorpg_core::CanonicalCreatureSnapshot {
        self.zone
            .snapshot()
            .unwrap()
            .creatures
            .into_iter()
            .find(|record| record.creature_id == creature)
            .unwrap()
    }

    fn feet(&self, creature: CreatureId) -> [i32; 2] {
        let record = self.creature(creature);
        [
            record.position[0].round() as i32,
            record.position[2].round() as i32,
        ]
    }

    /// Advances up to `ticks` ticks until `done` holds, collecting `player`'s events; whether it
    /// did.
    fn until(
        &mut self,
        player: u32,
        ticks: u64,
        heard: &mut Vec<ZoneEvent>,
        mut done: impl FnMut(&Self) -> bool,
    ) -> bool {
        for _ in 0..ticks {
            if done(self) {
                return true;
            }
            self.zone.advance_tick().unwrap();
            heard.extend(self.zone.snapshot_for_player(player).unwrap().events);
        }
        done(self)
    }

    /// Walks `player` along `path` in 4 m strides, each followed by ticks enough for a wounded
    /// walker 2 m behind to catch up; whether `npc` still followed at the end.
    fn lead_along(&mut self, player: u32, npc: NpcId, path: &[[i32; 2]]) -> bool {
        for leg in path.windows(2) {
            let (from, to) = (leg[0], leg[1]);
            let length = f64::from(to[0] - from[0]).hypot(f64::from(to[1] - from[1]));
            let strides = (length / 400.0).ceil().max(1.0) as i32;
            for stride in 1..=strides {
                let at = [
                    from[0] + (to[0] - from[0]) * stride / strides,
                    from[1] + (to[1] - from[1]) * stride / strides,
                ];
                self.stand_at(player, at);
                for _ in 0..200 {
                    self.zone.advance_tick().unwrap();
                }
                if self.zone.escort(npc).is_none() {
                    return false;
                }
            }
        }
        true
    }
}

fn id(player: u32) -> CharacterId {
    CharacterId::new(format!("crossings-{player}")).unwrap()
}

fn refusal(events: &[ZoneEvent]) -> Option<ErrorCode> {
    events.iter().find_map(|event| match event {
        ZoneEvent::Error { code, .. } => Some(*code),
        _ => None,
    })
}

fn heard(events: &[ZoneEvent], wanted: ErrorCode, npc: NpcId) -> bool {
    events.iter().any(|event| {
        matches!(
            event,
            ZoneEvent::Error {
                code,
                target: Some(EntityRef::Npc(target)),
            } if *code == wanted && *target == npc
        )
    })
}

/// Quests 1 to 32 (the opening, the region and the hamlets) turned in, and `extra`.
fn region_done(extra: &[u32]) -> Vec<QuestId> {
    (1..=32)
        .chain(extra.iter().copied())
        .map(QuestId::new)
        .collect()
}

fn water_depth(zone: &ZoneSimulation, [x, z]: [i32; 2]) -> i32 {
    let content = zone.content();
    content
        .waters()
        .iter()
        .find(|water| water.contains(x, z))
        .map_or(i32::MIN, |water| {
            water.surface - content.definition().ground_at(x, z)
        })
}

#[test]
fn the_crossings_stand_where_their_stories_put_them() {
    let content = greyhaven_vale::content();
    let zone = ZoneSimulation::with_content(GREYHAVEN, Arc::clone(&content)).unwrap();
    let crossing = content.crossing(ALDO).unwrap();
    // Aldo waits at the route's start on the south bank; the cart wades the ford at its middle
    // stake and never meets swimming water.
    assert_eq!(content.npc(ALDO).unwrap().position, crossing.route[0]);
    let length = crossing.length();
    let mut waded = false;
    for step in (0..=length).step_by(50) {
        let (at, _) = crossing.point_at(step);
        let depth = water_depth(&zone, at);
        assert!(
            depth <= mmorpg_core::water::SWIM_DEPTH_UNITS,
            "{at:?}: {depth}"
        );
        waded |= depth > 0;
        assert_eq!(
            content.areas().area_at(at[0], at[1]).map(|area| area.id()),
            Some(wilds::FENWATER_CROSSING)
        );
    }
    assert!(waded, "the cart fords the river");
    let (end, _) = crossing.point_at(length);
    let corwin = content.npc(fenford::WATCHMAN_CORWIN).unwrap().position;
    assert!(f64::from(end[0] - corwin[0]).hypot(f64::from(end[1] - corwin[1])) < 1_000.0);
    // Its raiders stand only while it crosses.
    for raider in &crossing.raiders {
        assert!(matches!(
            zone.snapshot()
                .unwrap()
                .creatures
                .iter()
                .find(|record| record.creature_id == raider.creature)
                .unwrap()
                .life,
            CreatureLife::Despawned { .. }
        ));
    }
    // A casting stake stands on the margin within a lure's draw of each crayfish.
    for (spot, crayfish) in quests::LURE_STONES.into_iter().zip(crayfish::SPAWN_IDS) {
        let stake = content.evidence_spot(spot).unwrap();
        assert!(
            (stake.at[1] - content.definition().ground_at(stake.at[0], stake.at[2])).abs() <= 50
        );
        let at = content
            .creature_spawns()
            .iter()
            .find(|spawn| spawn.id.get() == crayfish)
            .unwrap()
            .position;
        let far = f64::from(stake.at[0] - at[0]).hypot(f64::from(stake.at[2] - at[1]));
        assert!(
            far < f64::from(mmorpg_core::LURE_RADIUS_UNITS) / 2.0,
            "{far}"
        );
        assert_eq!(content.lure(spot).unwrap().item, quests::CRAYFISH_LURE);
    }
    // Nell stands on the island, ringed by water too deep to wade: straight east of her it swims.
    let nell = content.npc(NELL).unwrap().position;
    assert!(water_depth(&zone, nell) <= 0);
    assert!(
        (nell[0]..nell[0] + 6_000)
            .step_by(100)
            .any(|x| water_depth(&zone, [x, nell[1]]) > mmorpg_core::water::SWIM_DEPTH_UNITS)
    );
    assert_eq!(sell_price(quests::CRAYFISH_LURE), 0);
}

/// Player 1 on *A Crossing Under Fire*, having told Aldo to drive on; player 2 on it too.
fn crossing() -> World {
    let done = region_done(&[71, 72]);
    let mut world = World::new(&[(1, done.clone()), (2, done)]);
    world.accept(1, OSK, CROSSING);
    world.accept(2, OSK, CROSSING);
    let events = world.act(1, ALDO);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert!(world.zone.crossing_run(ALDO).is_some());
    world
}

#[test]
fn a_crossing_under_fire_credits_only_the_players_by_the_cart_when_it_arrives_alive() {
    let mut world = crossing();
    let crossing = world.zone.content().crossing(ALDO).unwrap().clone();
    // A second word while it rolls joins it and changes nothing.
    let events = world.act(2, ALDO);
    assert_eq!(refusal(&events), None, "{events:?}");
    // The raiders were driven off before they stood (as if cut down): the cart rolls unopposed.
    world.restore(|state| {
        state
            .crossing_runs
            .iter_mut()
            .find(|run| run.npc == ALDO)
            .unwrap()
            .stood = 0b111;
    });
    // Player 1 waits on the far bank, player 2 far up the lane. Reaching the bank oneself does
    // nothing.
    let end = *crossing.route.last().unwrap();
    world.stand_at(1, [end[0] - 300, end[1] - 300]);
    world.stand_at(2, [104_900, 27_200]);
    assert_eq!(world.progress(1, CROSSING), [0]);
    // The cart rolls, and recovery mid-way carries it on.
    let mut events = Vec::new();
    world.until(1, 600, &mut events, |_| false);
    let rolled = world.zone.crossing_run(ALDO).unwrap().progress;
    assert!(rolled > 0 && rolled < crossing.length(), "{rolled}");
    world.stand_at(1, [end[0] - 300, end[1] - 300]);
    assert!(world.zone.crossing_run(ALDO).unwrap().progress >= rolled);
    assert_eq!(
        world.zone.npc_placement(ALDO).unwrap().position,
        crossing
            .point_at(world.zone.crossing_run(ALDO).unwrap().progress)
            .0
    );
    let arrived = world.until(1, 4_000, &mut events, |world| {
        world
            .zone
            .crossing_run(ALDO)
            .is_some_and(|run| run.rests_until > 0)
    });
    assert!(arrived, "the cart never reached the bank");
    assert_eq!(world.progress(1, CROSSING), [1]);
    assert_eq!(world.progress(2, CROSSING), [0], "too far from the cart");
    // Over: Aldo rests with his cart on the far bank before he goes back for the next load.
    assert_eq!(world.zone.npc_placement(ALDO).unwrap().position, end);
    let events = world.turn_in(1, OSK, CROSSING);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert!(world.log(1).is_completed(CROSSING));
    assert_eq!(
        refusal(&world.act(2, ALDO)),
        Some(ErrorCode::CartResting),
        "the next crossing waits until Aldo is back"
    );
}

#[test]
fn a_cart_that_falls_fails_the_crossing_and_is_driven_again() {
    let mut world = crossing();
    // Nearly broken already, the cart meets the reed bandits on the south bank.
    world.restore(|state| {
        state
            .crossing_runs
            .iter_mut()
            .find(|run| run.npc == ALDO)
            .unwrap()
            .health = 4;
    });
    let mut events = Vec::new();
    let lost = world.until(1, 2_400, &mut events, |world| {
        world
            .zone
            .crossing_run(ALDO)
            .is_some_and(|run| run.rests_until > 0)
    });
    assert!(lost, "the bandits never reached the cart");
    let run = world.zone.crossing_run(ALDO).unwrap().clone();
    assert_eq!(run.health, 0);
    assert!(run.progress < world.zone.content().crossing(ALDO).unwrap().length());
    assert!(heard(&events, ErrorCode::CartLost, ALDO), "{events:?}");
    assert_eq!(world.progress(1, CROSSING), [0]);
    // The bandits leave with the cart broken, and Aldo mends it before the next try.
    world.zone.advance_tick().unwrap();
    for raider in world.zone.content().crossing(ALDO).unwrap().raiders.clone() {
        assert!(matches!(
            world.creature(raider.creature).life,
            CreatureLife::Despawned { .. }
        ));
    }
    assert_eq!(refusal(&world.act(1, ALDO)), Some(ErrorCode::CartResting));
    let tick = world.zone.snapshot().unwrap().tick;
    world.restore(|state| {
        state
            .crossing_runs
            .iter_mut()
            .find(|run| run.npc == ALDO)
            .unwrap()
            .rests_until = tick;
    });
    world.zone.advance_tick().unwrap();
    assert!(world.zone.crossing_run(ALDO).is_none());
    assert_eq!(
        world.zone.npc_placement(ALDO).unwrap().position,
        fenford::ROUTE[0]
    );
    // The retry: a whole cart sets out again.
    let events = world.act(1, ALDO);
    assert_eq!(refusal(&events), None, "{events:?}");
    let run = world.zone.crossing_run(ALDO).unwrap();
    assert_eq!(run.health, fenford::CART_HEALTH);
    assert_eq!(run.rests_until, 0);
}

#[test]
fn the_ford_watch_raises_the_barricade_then_holds_the_ford_while_the_cart_crosses() {
    let mut world = World::new(&[(1, region_done(&[71, 72, 73]))]);
    // The watch is Osk's once the first cart is over.
    let mut early = World::new(&[(2, region_done(&[71, 72]))]);
    early.beside(2, OSK);
    let events = early.send(
        2,
        ZoneCommand::AcceptQuest {
            npc: OSK,
            quest: WATCH.get(),
        },
    );
    assert_eq!(refusal(&events), Some(ErrorCode::InvalidQuest));
    world.accept(1, OSK, WATCH);
    // The crossing waits for the barricade.
    assert_eq!(refusal(&world.act(1, ALDO)), Some(ErrorCode::InvalidQuest));
    let piece = world
        .zone
        .content()
        .pieces()
        .iter()
        .find(|candidate| candidate.id == fenford::FORD_BARRICADE)
        .unwrap()
        .clone();
    let [px, _, pz] = piece.position;
    for _ in 0..2 {
        let events = world.lift(1, fenford::LANE_HURDLES);
        assert_eq!(refusal(&events), None, "{events:?}");
        world.stand_at(1, [px, pz - piece.half_extents[2] - 120]);
        let events = world.send(1, ZoneCommand::SetDown);
        assert_eq!(refusal(&events), None, "{events:?}");
        world.zone.advance_tick().unwrap();
    }
    world.zone.advance_tick().unwrap();
    assert!(
        world
            .zone
            .construction(fenford::FORD_BARRICADE)
            .unwrap()
            .raised
            .is_some(),
        "the barricade stands"
    );
    assert_eq!(world.progress(1, WATCH), [1, 0]);
    // Then the watch: Aldo drives on, and the player holds the ford by the north stake.
    let events = world.act(1, ALDO);
    assert_eq!(refusal(&events), None, "{events:?}");
    world.restore(|state| {
        state
            .crossing_runs
            .iter_mut()
            .find(|run| run.npc == ALDO)
            .unwrap()
            .stood = 0b111;
    });
    let stake = world
        .zone
        .content()
        .evidence_spot(quests::FORD_STAKES[0])
        .unwrap()
        .at;
    world.stand_at(1, [stake[0] + 200, stake[2] - 200]);
    let mut events = Vec::new();
    let over = world.until(1, 4_000, &mut events, |world| {
        world
            .zone
            .crossing_run(ALDO)
            .is_some_and(|run| run.rests_until > 0)
    });
    assert!(over);
    assert_eq!(world.progress(1, WATCH), [1, 1]);
    let events = world.turn_in(1, OSK, WATCH);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert!(world.log(1).is_completed(WATCH));
}

/// Casts the lure at `stake`, then cuts down crayfish `id`, drawn round to the bait, from the
/// bank side; the crayfish's facing when the blow landed.
fn flank_kill(world: &mut World, stake: EvidenceId, crayfish: CreatureId) {
    world.send(1, ZoneCommand::SetWeaponDrawn(false));
    let events = world.inspect(1, stake);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert!(world.zone.lure(stake).is_some());
    // The crayfish takes the player on, but turns to the bait as it does.
    for _ in 0..120 {
        world.zone.advance_tick().unwrap();
    }
    let at = world.feet(crayfish);
    let lure = world.zone.content().evidence_spot(stake).unwrap().at;
    let toward = mmorpg_core::trig::yaw_from_vector(lure[0] - at[0], lure[2] - at[1]).unwrap();
    let facing = world.creature(crayfish).facing;
    let gap = (facing.wrapping_sub(toward) as i16).unsigned_abs();
    assert!(gap < 2_000, "facing {facing}, the bait at {toward}");
    // Nearly beaten, it falls to a blow from the bank behind its claws.
    world.restore(|state| {
        state
            .creatures
            .iter_mut()
            .find(|record| record.creature_id == crayfish)
            .unwrap()
            .health = 1;
    });
    world.stand_at(1, [at[0], at[1] - 150]);
    let target = EntityRef::Creature(crayfish);
    world.send(1, ZoneCommand::SelectTarget(Some(target)));
    for _ in 0..600 {
        if world.creature(crayfish).life != CreatureLife::Alive {
            break;
        }
        match arena::swing_at(&world.zone, 1, target) {
            Some(swing) => {
                world.send(1, swing);
            }
            None => world.zone.advance_tick().unwrap(),
        }
    }
    assert_ne!(world.creature(crayfish).life, CreatureLife::Alive);
}

#[test]
fn the_rivers_teeth_counts_crayfish_cut_down_from_their_open_side() {
    let mut world = World::new(&[(1, region_done(&[])), (2, region_done(&[]))]);
    // No line, no cast.
    assert_eq!(
        refusal(&world.inspect(2, quests::LURE_STONES[0])),
        Some(ErrorCode::MissingMaterials)
    );
    world.accept(1, ILSA, TEETH);
    assert_eq!(world.carries(1, quests::CRAYFISH_LURE), 1);
    let crayfish: Vec<CreatureId> = crayfish::SPAWN_IDS.take(2).map(CreatureId::new).collect();
    flank_kill(&mut world, quests::LURE_STONES[0], crayfish[0]);
    assert_eq!(world.progress(1, TEETH), [1]);
    // The line is cast again: the bait stays in the bag.
    assert_eq!(world.carries(1, quests::CRAYFISH_LURE), 1);
    flank_kill(&mut world, quests::LURE_STONES[1], crayfish[1]);
    assert_eq!(world.progress(1, TEETH), [2]);
    // A spent lure leaves its stake.
    for _ in 0..mmorpg_core::LURE_TICKS {
        world.zone.advance_tick().unwrap();
    }
    assert!(world.zone.lure(quests::LURE_STONES[1]).is_none());
    world.send(1, ZoneCommand::SetWeaponDrawn(false));
    let events = world.turn_in(1, ILSA, TEETH);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert!(world.log(1).is_completed(TEETH));
    // Ilsa takes her line back.
    assert_eq!(world.carries(1, quests::CRAYFISH_LURE), 0);
}

/// Player 1 on *Across the Reeds*, having asked Nell to follow.
fn reeds() -> World {
    let mut world = World::new(&[(1, region_done(&[]))]);
    world.accept(1, ILSA, REEDS);
    let events = world.act(1, NELL);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert_eq!(world.zone.escort(NELL).unwrap().player, 1);
    world
}

#[test]
fn nell_balks_at_the_deep_water_round_the_island() {
    let mut world = reeds();
    let start = world.zone.npc_placement(NELL).unwrap().position;
    // Straight east off the island: into the lake's deep ring.
    let mut events = Vec::new();
    let mut followed = true;
    for stride in 1..=20 {
        world.stand_at(1, [start[0] + 300 * stride, start[1]]);
        for _ in 0..120 {
            world.zone.advance_tick().unwrap();
            events.extend(world.zone.snapshot_for_player(1).unwrap().events);
        }
        if world.zone.escort(NELL).is_none() {
            followed = false;
            break;
        }
    }
    assert!(!followed, "Nell swam");
    assert!(heard(&events, ErrorCode::TooDeep, NELL), "{events:?}");
    assert_eq!(world.progress(1, REEDS), [0]);
    // She is back on the island, and asked again she follows.
    assert_eq!(world.zone.npc_placement(NELL).unwrap().position, start);
    let events = world.act(1, NELL);
    assert_eq!(refusal(&events), None, "{events:?}");
}

#[test]
fn across_the_reeds_leads_nell_off_the_island_by_the_causeway_and_the_shallows() {
    let mut world = reeds();
    let start = world.zone.npc_placement(NELL).unwrap().position;
    // The causeway west, then the ankle-deep shelf round the lake's north side (… then off the
    // causeway onto the shelf), then east to the Fenwater's meadows.
    let path = [
        start,
        // South of the drowned tower and between its stones, onto the causeway …
        [47_200, -1_100],
        [44_800, -1_100],
        [44_000, -2_000],
        [39_500, -2_000],
        [39_000, -3_500],
        [42_000, -7_500],
        [54_000, -7_500],
        [56_500, -3_000],
        [66_500, -2_000],
    ];
    for leg in path.windows(2) {
        for step in 0..=20 {
            let at = [
                leg[0][0] + (leg[1][0] - leg[0][0]) * step / 20,
                leg[0][1] + (leg[1][1] - leg[0][1]) * step / 20,
            ];
            let depth = water_depth(&world.zone, at);
            assert!(
                depth <= mmorpg_core::water::SWIM_DEPTH_UNITS,
                "{at:?}: {depth}"
            );
        }
    }
    assert!(world.lead_along(1, NELL, &path), "Nell stopped following");
    let mut heard = Vec::new();
    let home = world.until(1, 2_400, &mut heard, |world| {
        world.progress(1, REEDS) == [1]
    });
    assert!(home, "{:?}", world.zone.escort(NELL));
    // She rests in the meadows before she goes back for her reeds.
    assert!(world.zone.escort(NELL).unwrap().rests_until > 0);
    let events = world.turn_in(1, ILSA, REEDS);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert!(world.log(1).is_completed(REEDS));
}
