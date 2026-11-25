//! Shepherd Lise's flock (#1019, slice 4b): her missing ewe tracked by its wool and hoofprints and
//! driven into her fold (53), and a night watch over the fold with bait and a snare laid at its
//! hedges (55).
//!
//! Every step is a server-checked command or tick; each objective is credited once, and every
//! `stand_at` restores the zone from its canonical snapshot, so recovery carries the driven stray
//! and the running watch.
use crate::support;

use std::collections::BTreeMap;
use std::sync::Arc;

use mmorpg_core::greyhaven_vale::{self, flock, quests};
use mmorpg_core::{
    CharacterId, CharacterLocation, EntityKind, ErrorCode, EvidenceId, GAP_HOLD_TICKS, ItemId,
    NpcId, QuestId, QuestLog, STRAY_TIMEOUT_TICKS, StrayState, ZoneCommand, ZoneEvent, ZoneId,
    ZoneSimulation, sell_price,
};
use support::classes;

const GREYHAVEN: ZoneId = ZoneId::new(1);
const WARDEN_CLASS: u8 = 0;
const LISE: NpcId = flock::SHEPHERD_LISE;
const EWE: NpcId = flock::STRAY_EWE;
const MUTTON: ItemId = quests::TAINTED_MUTTON;
const SNARE: ItemId = quests::WOLF_SNARE;
const WOOL: u32 = 14;
const HOOFPRINTS: u32 = 15;
const BAIT_GAP: u32 = 16;
const SNARE_GAP: u32 = 17;
const MISSING_EWE: QuestId = quests::THE_MISSING_EWE;
const HEDGES: QuestId = quests::THE_HEDGES_AT_DUSK;
/// The quests before the missing ewe: the Vale's and the hamlets' 1-32 with Lise's wolves.
fn shepherd() -> Vec<QuestId> {
    (1..=32).map(QuestId::new).collect()
}

/// ... and before the watch, the ewe too.
fn watcher() -> Vec<QuestId> {
    (1..=32).chain([53]).map(QuestId::new).collect()
}

struct World {
    zone: ZoneSimulation,
    sequences: BTreeMap<u32, u32>,
    /// The wolves' records before any watch, reserved off the world.
    reserved: Vec<mmorpg_core::CanonicalCreatureSnapshot>,
}

impl World {
    /// Characters `players`, each having turned in `completed`, admitted at level 6 by the Marshal.
    fn new(players: &[u32], completed: &[QuestId]) -> Self {
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
                        class: WARDEN_CLASS,
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
            let [x, z] = content.npc(quests::MARSHAL).unwrap().position;
            record.location = CharacterLocation {
                zone_id: GREYHAVEN,
                position: [x + 100, 90, z + 100],
                facing: 0,
            };
            zone.admit_character(player, &record).unwrap();
        }
        zone.advance_tick().unwrap();
        let reserved = zone
            .snapshot()
            .unwrap()
            .creatures
            .into_iter()
            .filter(|record| flock::PROWLERS.contains(&record.creature_id))
            .collect();
        Self {
            zone,
            sequences: players.iter().map(|&player| (player, 10)).collect(),
            reserved,
        }
    }

    fn send(&mut self, player: u32, command: ZoneCommand) -> Vec<ZoneEvent> {
        let sequence = self.sequences.entry(player).or_default();
        *sequence += 1;
        self.zone.apply_command(player, *sequence, command).unwrap();
        self.zone.advance_tick().unwrap();
        self.zone.snapshot_for_player(player).unwrap().events
    }

    fn accept(&mut self, player: u32, quest: QuestId) {
        self.beside(player, LISE);
        let events = self.send(
            player,
            ZoneCommand::AcceptQuest {
                npc: LISE,
                quest: quest.get(),
            },
        );
        assert_eq!(refusal(&events), None, "accepting {quest:?}: {events:?}");
    }

    fn turn_in(&mut self, player: u32, quest: QuestId) -> Vec<ZoneEvent> {
        self.beside(player, LISE);
        self.send(
            player,
            ZoneCommand::CompleteQuest {
                npc: LISE,
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

    fn inspect(&mut self, player: u32, spot: u32) -> Vec<ZoneEvent> {
        let [x, _, z] = self
            .zone
            .content()
            .evidence_spot(EvidenceId::new(spot))
            .unwrap()
            .at;
        self.stand_at(player, [x, z]);
        self.send(player, ZoneCommand::Inspect { spot })
    }

    fn progress(&self, player: u32, quest: QuestId) -> Vec<u8> {
        let objectives = self.zone.content().quest(quest).unwrap().objectives.len();
        self.zone
            .capture_character(player, id(player))
            .unwrap()
            .quests
            .entry(quest)
            .unwrap()
            .progress[..objectives]
            .to_vec()
    }

    fn carries(&self, player: u32, item: ItemId) -> u32 {
        self.zone
            .capture_character(player, id(player))
            .unwrap()
            .inventory
            .count(item)
    }

    fn ewe_at(&self) -> [i32; 2] {
        self.zone.npc_placement(EWE).unwrap().position
    }

    /// Drives the ewe toward `+z` from 3 m behind it for at most `rounds` half-seconds, until it
    /// is penned; whether it was.
    fn drive_south(&mut self, player: u32, rounds: usize) -> bool {
        for _ in 0..rounds {
            let [x, z] = self.ewe_at();
            self.stand_at(player, [x, z - 300]);
            for _ in 0..30 {
                self.zone.advance_tick().unwrap();
            }
            if self
                .zone
                .stray(EWE)
                .is_some_and(|stray| stray.rests_until > 0)
            {
                return true;
            }
        }
        false
    }

    /// Lays the bait and the snare, then starts the watch at Lise.
    fn start_watch(&mut self, player: u32) {
        assert_eq!(refusal(&self.inspect(player, BAIT_GAP)), None);
        assert_eq!(refusal(&self.inspect(player, SNARE_GAP)), None);
        self.beside(player, LISE);
        let events = self.send(player, ZoneCommand::QuestAction { npc: LISE });
        assert_eq!(refusal(&events), None, "{events:?}");
        assert!(self.zone.watch().is_some());
    }

    /// Every wolf of the running watch stood and lies dead or gone; the next tick ends it.
    fn slay_prowlers(&mut self) {
        let reserved = self.reserved.clone();
        self.restore(|state| {
            state.watch.as_mut().unwrap().stood = 0b1111;
            for record in &mut state.creatures {
                if let Some(gone) = reserved
                    .iter()
                    .find(|gone| gone.creature_id == record.creature_id)
                {
                    *record = gone.clone();
                }
            }
        });
        self.zone.advance_tick().unwrap();
        assert!(self.zone.watch().unwrap().recovers_at > 0);
    }

    /// The projected health of sheep `npc` as `player` sees it.
    fn sheep_health(&self, player: u32, npc: NpcId) -> Option<u8> {
        self.zone
            .snapshot_for_player(player)
            .unwrap()
            .entities
            .iter()
            .find(|entity| entity.kind == EntityKind::Npc && entity.id == npc.get())
            .map(|entity| entity.health_percent)
    }
}

fn id(player: u32) -> CharacterId {
    CharacterId::new(format!("flock-{player}")).unwrap()
}

fn refusal(events: &[ZoneEvent]) -> Option<ErrorCode> {
    events.iter().find_map(|event| match event {
        ZoneEvent::Error { code, .. } => Some(*code),
        _ => None,
    })
}

#[test]
fn the_missing_ewe_is_tracked_by_its_trail_and_driven_into_the_fold() {
    let mut world = World::new(&[1], &shepherd());
    world.accept(1, MISSING_EWE);
    assert_eq!(refusal(&world.inspect(1, WOOL)), None);
    assert_eq!(refusal(&world.inspect(1, HOOFPRINTS)), None);
    assert_eq!(world.progress(1, MISSING_EWE), vec![1, 1, 0]);
    // At her post she shies south from a player north of her.
    let post = world.ewe_at();
    world.stand_at(1, [post[0], post[1] - 300]);
    for _ in 0..30 {
        world.zone.advance_tick().unwrap();
    }
    let moved = world.ewe_at();
    assert!(moved[1] > post[1] + 100, "{post:?} -> {moved:?}");
    assert_eq!(world.zone.stray(EWE).unwrap().driver, Some(1));
    // Driven down the hamlet's east side from above the fold, she ends in it.
    world.restore(|state| {
        state.strays = vec![StrayState {
            npc: EWE,
            position: [15_000, 44_600],
            facing: 0,
            velocity: [0, 0],
            driver: None,
            still: 0,
            rests_until: 0,
        }];
    });
    assert!(
        world.drive_south(1, 40),
        "ewe stopped at {:?}",
        world.ewe_at()
    );
    let (min, max) = flock::FOLD;
    let penned = world.ewe_at();
    assert!((min[0]..=max[0]).contains(&penned[0]) && (min[1]..=max[1]).contains(&penned[1]));
    assert_eq!(world.progress(1, MISSING_EWE), vec![1, 1, 1]);
    // Penned, she no longer shies, and Lise takes the news.
    let [x, z] = penned;
    world.stand_at(1, [x, z - 200]);
    assert_eq!(world.ewe_at(), penned);
    let events = world.turn_in(1, MISSING_EWE);
    assert_eq!(refusal(&events), None, "{events:?}");
    // After her rest she slips out to where she was found, for the next shepherd's helper.
    let tick = world.zone.current_tick();
    world.restore(|state| state.strays[0].rests_until = tick + 1);
    world.zone.advance_tick().unwrap();
    assert!(world.zone.stray(EWE).is_none());
    assert_eq!(world.ewe_at(), post);
}

#[test]
fn a_stray_left_alone_bolts_back_to_where_it_was_found() {
    let mut world = World::new(&[1], &shepherd());
    world.accept(1, MISSING_EWE);
    let post = world.ewe_at();
    world.stand_at(1, [post[0], post[1] - 300]);
    for _ in 0..10 {
        world.zone.advance_tick().unwrap();
    }
    assert_ne!(world.ewe_at(), post);
    // Left alone, she waits; her driver walked off to Lise.
    world.beside(1, LISE);
    let still = world.zone.stray(EWE).unwrap().still;
    assert!(still > 0);
    world.restore(|state| state.strays[0].still = STRAY_TIMEOUT_TICKS - 1);
    let mut events = Vec::new();
    for _ in 0..3 {
        world.zone.advance_tick().unwrap();
        events.extend(world.zone.snapshot_for_player(1).unwrap().events);
    }
    assert_eq!(refusal(&events), Some(ErrorCode::StrayBolted));
    assert!(world.zone.stray(EWE).is_none());
    assert_eq!(world.ewe_at(), post);
    assert_eq!(world.progress(1, MISSING_EWE), vec![0, 0, 0]);
}

#[test]
fn the_hedges_at_dusk_holds_when_two_ewes_stand_and_credits_the_party_on_watch() {
    let mut world = World::new(&[1, 2], &watcher());
    world.accept(1, HEDGES);
    world.accept(2, HEDGES);
    // Lise's supplies are worth nothing to a vendor.
    assert_eq!(world.carries(1, MUTTON), 1);
    assert_eq!(world.carries(1, SNARE), 1);
    assert_eq!((sell_price(MUTTON), sell_price(SNARE)), (0, 0));
    // The watch waits for the bait and the snare.
    world.beside(1, LISE);
    let events = world.send(1, ZoneCommand::QuestAction { npc: LISE });
    assert_eq!(refusal(&events), Some(ErrorCode::InvalidQuest));
    world.start_watch(1);
    assert_eq!(world.carries(1, MUTTON), 0);
    assert_eq!(world.carries(1, SNARE), 0);
    assert_eq!(world.progress(1, HEDGES), vec![1, 1, 0]);
    // Laid bait survives a reload of the character.
    let record = world.zone.capture_character(1, id(1)).unwrap();
    assert_eq!(record.quests.entry(HEDGES).unwrap().progress[..2], [1, 1]);
    // A second ask while it runs is refused; the first two wolves stood at once.
    let events = world.send(1, ZoneCommand::QuestAction { npc: LISE });
    assert_eq!(refusal(&events), Some(ErrorCode::FlockUnsettled));
    let watch = world.zone.watch().unwrap().clone();
    assert_eq!(watch.stood, 0b0011);
    assert_eq!(watch.flock, vec![40; 3]);
    assert_eq!(world.sheep_health(1, flock::FLOCK[0]), Some(100));
    // The second player keeps watch without laying anything: their watch objective waits for
    // their own bait and snare, so it is not credited.
    world.stand_at(2, [15_000, 46_900]);
    // The last wolf falls with two of three standing.
    world.restore(|state| state.watch.as_mut().unwrap().flock = vec![0, 12, 40]);
    world.slay_prowlers();
    assert!(world.zone.watch().unwrap().recovers_at > 0);
    assert_eq!(world.progress(1, HEDGES), vec![1, 1, 1]);
    assert_eq!(world.sheep_health(1, flock::FLOCK[0]), Some(1));
    assert_eq!(world.progress(2, HEDGES), vec![0, 0, 0]);
    let events = world.turn_in(1, HEDGES);
    assert_eq!(refusal(&events), None, "{events:?}");
}

#[test]
fn a_party_mate_with_bait_laid_shares_the_watch() {
    let mut world = World::new(&[1, 2], &watcher());
    world.accept(1, HEDGES);
    world.accept(2, HEDGES);
    assert_eq!(refusal(&world.inspect(2, BAIT_GAP)), None);
    assert_eq!(refusal(&world.inspect(2, SNARE_GAP)), None);
    world.start_watch(1);
    world.stand_at(2, [15_000, 46_900]);
    world.slay_prowlers();
    assert_eq!(world.progress(1, HEDGES), vec![1, 1, 1]);
    assert_eq!(world.progress(2, HEDGES), vec![1, 1, 1]);
}

#[test]
fn prowlers_are_held_at_their_gap_then_bite_the_flock() {
    let mut world = World::new(&[1], &watcher());
    world.accept(1, HEDGES);
    world.start_watch(1);
    // The player keeps away at Lise's door, so nobody draws the wolves off.
    world.stand_at(1, [12_400, 47_400]);
    let watch = world.zone.content().watch().unwrap().clone();
    // A wolf at its gap is held there by the bait ...
    let wolf = watch.prowlers[0];
    let ground = world
        .zone
        .content()
        .definition()
        .ground_at(wolf.gap[0], wolf.gap[1]);
    world.restore(|state| {
        let record = state
            .creatures
            .iter_mut()
            .find(|record| record.creature_id == wolf.creature)
            .unwrap();
        record.position = [wolf.gap[0] as f32, (ground + 40) as f32, wolf.gap[1] as f32];
        record.velocity = [0.; 3];
    });
    for _ in 0..10 {
        world.zone.advance_tick().unwrap();
    }
    let held = world.zone.watch().unwrap().held[0];
    assert!(held > 0 && held < GAP_HOLD_TICKS, "{held}");
    // ... then goes for the nearest ewe and bites.
    let sheep = flock::FLOCK[0];
    let at = world.zone.npc_placement(sheep).unwrap().position;
    let ground = world.zone.content().definition().ground_at(at[0], at[1]);
    world.restore(|state| {
        state.watch.as_mut().unwrap().held[0] = GAP_HOLD_TICKS;
        let record = state
            .creatures
            .iter_mut()
            .find(|record| record.creature_id == wolf.creature)
            .unwrap();
        record.position = [(at[0] + 100) as f32, (ground + 40) as f32, at[1] as f32];
        record.velocity = [0.; 3];
    });
    for _ in 0..5 {
        world.zone.advance_tick().unwrap();
    }
    let flock_health = world.zone.watch().unwrap().flock.clone();
    assert!(flock_health[0] < 40, "{flock_health:?}");
    assert!(world.sheep_health(1, sheep).unwrap() < 100);
}

#[test]
fn a_fallen_flock_fails_the_watch_and_it_is_tried_again_once_the_ewes_are_up() {
    let mut world = World::new(&[1], &watcher());
    world.accept(1, HEDGES);
    world.start_watch(1);
    world.stand_at(1, [15_000, 46_900]);
    let snapshot = world.zone.snapshot_for_player(1).unwrap();
    assert!(snapshot.entities.iter().any(|entity| {
        entity.kind == EntityKind::Creature && entity.id == flock::PROWLERS[0].get()
    }));
    world.restore(|state| state.watch.as_mut().unwrap().flock = vec![0, 0, 40]);
    world.zone.advance_tick().unwrap();
    let events = world.zone.snapshot_for_player(1).unwrap().events;
    assert_eq!(refusal(&events), Some(ErrorCode::FlockFell));
    assert_eq!(world.progress(1, HEDGES), vec![1, 1, 0]);
    // The wolves still about slink off.
    let snapshot = world.zone.snapshot_for_player(1).unwrap();
    assert!(
        !snapshot.entities.iter().any(|entity| {
            entity.kind == EntityKind::Creature
                && flock::PROWLERS.iter().any(|wolf| wolf.get() == entity.id)
        }),
        "a prowler stayed"
    );
    // Not again while the ewes are down ...
    world.beside(1, LISE);
    let events = world.send(1, ZoneCommand::QuestAction { npc: LISE });
    assert_eq!(refusal(&events), Some(ErrorCode::FlockUnsettled));
    // ... but once they are up, and the wolves are gone.
    let tick = world.zone.current_tick();
    world.restore(|state| {
        state.watch.as_mut().unwrap().recovers_at = tick + 1;
    });
    world.zone.advance_tick().unwrap();
    assert!(world.zone.watch().is_none());
    let events = world.send(1, ZoneCommand::QuestAction { npc: LISE });
    assert_eq!(refusal(&events), None, "{events:?}");
    assert_eq!(world.zone.watch().unwrap().flock, vec![40; 3]);
}
