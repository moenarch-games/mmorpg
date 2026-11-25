//! The missing family and the granary (#1019, slice 4c): the Tollys' trail from their empty
//! cottage (59), leading the family home in a line that fails and is retried (60), a muster a
//! party gathers in parallel with each character's own credit (61), and a defence of the western
//! hamlet's granary, a second raid site, with a breach to mend afterwards (62).
//!
//! Every step is a server-checked command or tick, each objective is credited once, and every
//! `stand_at` restores the zone from its canonical snapshot, so recovery carries the escort line
//! and the granary's raid state.
use crate::support;

use std::collections::BTreeMap;
use std::sync::Arc;

use mmorpg_core::greyhaven_vale::{self, granary, quests};
use mmorpg_core::raid::{PHASE_TICKS, RAID_PHASES};
use mmorpg_core::{
    CharacterId, CharacterLocation, ErrorCode, EvidenceId, NpcId, PartyMember, PartyRoster,
    PartyState, QuestId, QuestLog, RaidNotice, RaidOutcome, ZoneCommand, ZoneEvent, ZoneId,
    ZoneSimulation,
};
use support::classes;

const GREYHAVEN: ZoneId = ZoneId::new(1);
const WARDEN: u8 = 0;
const NECROMANCER: u8 = 3;
const ENA: NpcId = granary::GOODWIFE_ENA;
const HAL: NpcId = granary::HAL_TOLLY;
const FAMILY: [NpcId; 3] = [
    granary::HAL_TOLLY,
    granary::WENNA_TOLLY,
    granary::BRAM_TOLLY,
];
const GARR: NpcId = quests::PLOUGHMAN_GARR;
const JORY: NpcId = NpcId::new(41);
const DUNSTAN: NpcId = quests::WATCHMAN_DUNSTAN;
/// The granary is the content's second raid site.
const GRANARY: usize = 1;

/// The quests before the empty cottage: the Vale's and the hamlets' 1-32 with Hanne's sack.
fn before(quest: u32) -> Vec<QuestId> {
    (1..=32)
        .chain([52])
        .chain(59..quest)
        .map(QuestId::new)
        .collect()
}

struct World {
    zone: ZoneSimulation,
    sequences: BTreeMap<u32, u32>,
}

impl World {
    /// Characters `players` of `classes`, each having turned in `completed`, admitted at level 6 by
    /// the Marshal.
    fn new(players: &[(u32, u8)], completed: &[QuestId]) -> Self {
        let content = greyhaven_vale::content();
        let mut zone = ZoneSimulation::with_content(GREYHAVEN, Arc::clone(&content)).unwrap();
        for &(player, class) in players {
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
            sequences: players.iter().map(|&(player, _)| (player, 10)).collect(),
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
        self.beside(player, ENA);
        let events = self.send(
            player,
            ZoneCommand::AcceptQuest {
                npc: ENA,
                quest: quest.get(),
            },
        );
        assert_eq!(refusal(&events), None, "accepting {quest:?}: {events:?}");
    }

    fn turn_in(&mut self, player: u32, quest: QuestId) -> Vec<ZoneEvent> {
        self.beside(player, ENA);
        self.send(
            player,
            ZoneCommand::CompleteQuest {
                npc: ENA,
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
        let [x, _, z] = self.zone.content().evidence_spot(spot).unwrap().at;
        self.stand_at(player, [x, z]);
        self.send(player, ZoneCommand::Inspect { spot: spot.get() })
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

    fn completed(&self, player: u32, quest: QuestId) -> bool {
        self.zone
            .capture_character(player, id(player))
            .unwrap()
            .quests
            .is_completed(quest)
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

    fn at(&self, npc: NpcId) -> [i32; 2] {
        self.zone.npc_placement(npc).unwrap().position
    }

    /// Walks `player` from where Hal stands toward `goal` in 4 m strides, each followed by ticks
    /// until Hal caught up, and stops beside `goal`; whether the line still followed.
    fn lead_to(&mut self, player: u32, goal: [i32; 2]) -> bool {
        let start = self.at(HAL);
        let length = f64::from(goal[0] - start[0]).hypot(f64::from(goal[1] - start[1]));
        let strides = (length / 400.0).ceil() as i32;
        for stride in 1..=strides {
            let at = [
                start[0] + (goal[0] - start[0]) * stride / strides + 150,
                start[1] + (goal[1] - start[1]) * stride / strides + 150,
            ];
            self.stand_at(player, at);
            for _ in 0..180 {
                self.zone.advance_tick().unwrap();
            }
            if self.zone.escort(HAL).is_none() {
                return false;
            }
        }
        for _ in 0..900 {
            self.zone.advance_tick().unwrap();
        }
        true
    }
}

fn id(player: u32) -> CharacterId {
    CharacterId::new(format!("tolly-{player}")).unwrap()
}

fn refusal(events: &[ZoneEvent]) -> Option<ErrorCode> {
    events.iter().find_map(|event| match event {
        ZoneEvent::Error { code, .. } => Some(*code),
        _ => None,
    })
}

fn distance(a: [i32; 2], b: [i32; 2]) -> f64 {
    f64::from(a[0] - b[0]).hypot(f64::from(a[1] - b[1]))
}

#[test]
fn the_empty_cottage_follows_the_tollys_trail_in_order() {
    let mut world = World::new(&[(1, WARDEN)], &before(59));
    let quest = quests::THE_EMPTY_COTTAGE;
    // Signs read before the quest is taken never count.
    assert_eq!(
        refusal(&world.inspect(1, quests::TOLLYS_CHEST)),
        Some(ErrorCode::InvalidQuest)
    );
    world.accept(1, quest);
    for (spot, progress) in [
        (quests::TOLLYS_CHEST, [1, 0, 0]),
        (quests::CHILDS_SHOE, [1, 1, 0]),
        (quests::HANDCART_RUTS, [1, 1, 1]),
    ] {
        let events = world.inspect(1, spot);
        assert_eq!(refusal(&events), None, "{spot:?}: {events:?}");
        assert_eq!(world.progress(1, quest), progress);
    }
    // A sign read twice credits nothing more.
    assert_eq!(
        refusal(&world.inspect(1, quests::CHILDS_SHOE)),
        Some(ErrorCode::InvalidQuest)
    );
    // The trail ends where the family hides.
    let ruts = world
        .zone
        .content()
        .evidence_spot(quests::HANDCART_RUTS)
        .unwrap()
        .at;
    assert!(distance([ruts[0], ruts[2]], world.at(HAL)) < 2_000.0);
    let events = world.turn_in(1, quest);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert!(world.completed(1, quest));
}

#[test]
fn bring_them_home_leads_the_family_in_a_line_fails_when_left_and_is_retried() {
    let mut world = World::new(&[(1, WARDEN)], &before(60));
    let quest = quests::BRING_THEM_HOME;
    world.accept(1, quest);
    let posts = FAMILY.map(|npc| world.at(npc));
    // Only Hal starts it; Wenna and Bram come with him.
    let events = world.act(1, granary::WENNA_TOLLY);
    assert_eq!(refusal(&events), Some(ErrorCode::InvalidQuest));
    let events = world.act(1, HAL);
    assert_eq!(refusal(&events), None, "{events:?}");
    for npc in FAMILY {
        assert_eq!(world.zone.escort(npc).unwrap().player, 1, "{npc:?}");
    }
    // The escort step visits each of the line once a tick (`escort-step`).
    world.zone.advance_tick().unwrap();
    assert_eq!(world.zone.tick_work().escorts_advanced, 3);
    // A second ask while they follow is no new escort.
    let events = world.send(1, ZoneCommand::QuestAction { npc: HAL });
    assert_eq!(refusal(&events), Some(ErrorCode::InvalidQuest));

    // They follow in a line: Hal behind the player, Wenna behind Hal and Bram behind her.
    let ena = world.at(ENA);
    let halfway = [(posts[0][0] + ena[0]) / 2, (posts[0][1] + ena[1]) / 2];
    assert!(world.lead_to(1, halfway));
    let [hal, wenna, bram] = FAMILY.map(|npc| world.at(npc));
    assert!(distance(hal, halfway) < 800.0, "{hal:?}");
    assert!(distance(wenna, hal) < 600.0 && distance(bram, wenna) < 600.0);
    assert!(distance(wenna, posts[1]) > 2_000.0);

    // Running off fails the whole line: everyone is back at their post and the player asks again.
    world.stand_at(1, ena);
    let mut lost = false;
    for _ in 0..1_000 {
        world.zone.advance_tick().unwrap();
        let events = world.zone.snapshot_for_player(1).unwrap().events;
        lost |= refusal(&events) == Some(ErrorCode::EscortLost);
        if lost {
            break;
        }
    }
    assert!(lost);
    for (npc, post) in FAMILY.into_iter().zip(posts) {
        assert!(world.zone.escort(npc).is_none());
        assert_eq!(world.at(npc), post, "{npc:?}");
    }
    assert_eq!(world.progress(1, quest), vec![0]);

    // Asked again, they follow all the way home to Ena and arrive together.
    let events = world.act(1, HAL);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert!(world.lead_to(1, ena));
    assert_eq!(world.progress(1, quest), vec![1]);
    for npc in FAMILY {
        let escort = world.zone.escort(npc).unwrap();
        assert!(escort.rests_until > 0, "{npc:?} rests where it arrived");
        assert!(distance(escort.position, ena) < 1_200.0, "{npc:?}");
    }
    let events = world.turn_in(1, quest);
    assert_eq!(refusal(&events), None, "{events:?}");
}

#[test]
fn the_harvest_muster_is_gathered_in_parallel_with_each_characters_own_credit() {
    // A Necromancer and a Warden in one party, and a Warden on their own.
    let mut world = World::new(&[(1, NECROMANCER), (2, WARDEN), (3, WARDEN)], &before(61));
    let quest = quests::THE_HARVEST_MUSTER;
    for player in [1, 2, 3] {
        world.accept(player, quest);
    }
    world.party(&[1, 2]);
    // Watchman Dunstan distrusts Necromancers: alone, the first player is refused.
    let events = world.act(1, DUNSTAN);
    assert_eq!(refusal(&events), Some(ErrorCode::Distrusted));
    assert_eq!(world.progress(1, quest), vec![0, 0, 0]);
    // The party splits up: in one tick, one asks Garr in the west and the other Jory in the south.
    world.beside(1, GARR);
    world.beside(2, JORY);
    world
        .zone
        .apply_command(1, 50, ZoneCommand::QuestAction { npc: GARR })
        .unwrap();
    world
        .zone
        .apply_command(2, 50, ZoneCommand::QuestAction { npc: JORY })
        .unwrap();
    world.zone.advance_tick().unwrap();
    world.sequences.insert(1, 50);
    world.sequences.insert(2, 50);
    // Each recruit counts for both, out of order, and nobody else.
    assert_eq!(world.progress(1, quest), vec![1, 1, 0]);
    assert_eq!(world.progress(2, quest), vec![1, 1, 0]);
    assert_eq!(world.progress(3, quest), vec![0, 0, 0]);
    // The Warden wins Dunstan over, for the Necromancer too.
    let events = world.act(2, DUNSTAN);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert_eq!(world.progress(1, quest), vec![1, 1, 1]);
    assert_eq!(world.progress(2, quest), vec![1, 1, 1]);
    // A recruit already won credits nothing twice.
    let events = world.act(2, GARR);
    assert_eq!(refusal(&events), Some(ErrorCode::InvalidQuest));
    // The solo player gathers their own: Dunstan hears a Warden.
    for npc in [DUNSTAN, GARR, JORY] {
        let events = world.act(3, npc);
        assert_eq!(refusal(&events), None, "{npc:?}: {events:?}");
    }
    assert_eq!(world.progress(3, quest), vec![1, 1, 1]);
    // Each character keeps its own credit through recovery and turns it in on its own.
    world.restore(|_| {});
    for player in [1, 2, 3] {
        let events = world.turn_in(player, quest);
        assert_eq!(refusal(&events), None, "{player}: {events:?}");
        assert!(world.completed(player, quest));
    }
}

#[test]
fn a_recommended_necromancer_is_heard_by_dunstan_alone() {
    let mut completed = before(61);
    completed.push(QuestId::new(58));
    let mut world = World::new(&[(1, NECROMANCER)], &completed);
    world.accept(1, quests::THE_HARVEST_MUSTER);
    let events = world.act(1, DUNSTAN);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert_eq!(world.progress(1, quests::THE_HARVEST_MUSTER), vec![0, 0, 1]);
}

/// A defender on *The Granary Stand*, its defence started at Ena.
fn defender() -> World {
    let mut world = World::new(&[(1, WARDEN)], &before(62));
    world.accept(1, quests::THE_GRANARY_STAND);
    assert!(world.zone.defence_ready_at(GRANARY));
    let events = world.act(1, ENA);
    assert_eq!(refusal(&events), None, "{events:?}");
    let incident = world.zone.raid_at(GRANARY).unwrap().active.clone().unwrap();
    assert_eq!(incident.civilians, vec![granary::HAND_HEALTH; 3]);
    // Millbrook's night raid knows nothing of it.
    assert_eq!(world.zone.raid(), &mmorpg_core::RaidState::default());
    // A second ask while it runs is refused.
    let events = world.send(1, ZoneCommand::QuestAction { npc: ENA });
    assert_eq!(refusal(&events), Some(ErrorCode::SiteNotReady));
    world
}

/// Brings the granary's defence to its last phase's end with the player as a participant and the
/// hands at `health`.
fn end_defence(world: &mut World, health: [u32; 3]) -> Vec<ZoneEvent> {
    // A whole phase has run.
    while world.zone.current_tick() < u64::from(PHASE_TICKS) {
        world.zone.advance_tick().unwrap();
    }
    let tick = world.zone.current_tick();
    world.restore(|state| {
        let incident = state.site_raids[0].active.as_mut().unwrap();
        incident.phase = RAID_PHASES;
        incident.scaled_for = vec![1; usize::from(RAID_PHASES)];
        incident.phase_started = tick + 1 - u64::from(PHASE_TICKS);
        incident.participants = vec![1];
        incident.civilians = health.to_vec();
        // The barn untouched: whatever happens to it, a defence here leaves a breach.
        state.site_raids[0].damage = 0;
    });
    world.zone.advance_tick().unwrap();
    world.zone.snapshot_for_player(1).unwrap().events
}

fn notices(events: &[ZoneEvent]) -> Vec<RaidNotice> {
    events
        .iter()
        .filter_map(|event| match event {
            ZoneEvent::Raid { notice } => Some(*notice),
            _ => None,
        })
        .collect()
}

#[test]
fn the_granary_stand_holds_with_two_hands_standing_then_a_breach_is_mended() {
    let mut world = defender();
    world.beside(1, ENA);
    // The raid signal at the granary is the granary's.
    let signal = world.zone.snapshot_for_player(1).unwrap().raid.unwrap();
    assert_eq!((signal.phase, signal.standing), (1, Some(3)));
    // Two of three hands stand, the barn untouched: it held.
    let events = end_defence(&mut world, [granary::HAND_HEALTH, 0, 100]);
    assert_eq!(notices(&events), vec![RaidNotice::Stood]);
    let state = world.zone.raid_at(GRANARY).unwrap().clone();
    let last = state.last.unwrap();
    assert!(last.defence && last.outcome == RaidOutcome::Defended);
    assert_eq!(world.progress(1, quests::THE_GRANARY_STAND), vec![1, 0]);
    // A held defence still leaves its first breach, and a tend task for each struck hand.
    assert_eq!(state.aftermath, [0, 3, 4].into());
    // The quest's defence site pays no raid reward of its own.
    assert!(state.rewarded.is_empty());
    // Mending the breach in reach credits the aid; it survives recovery.
    world.restore(|_| {});
    world.stand_at(1, [-11_550, 35_000]);
    let events = world.send(1, ZoneCommand::RaidAid { task: 0 });
    assert_eq!(refusal(&events), None, "{events:?}");
    assert_eq!(world.progress(1, quests::THE_GRANARY_STAND), vec![1, 1]);
    assert!(!world.zone.raid_at(GRANARY).unwrap().aftermath.contains(&0));
    let events = world.turn_in(1, quests::THE_GRANARY_STAND);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert!(world.completed(1, quests::THE_GRANARY_STAND));
}

#[test]
fn an_overrun_granary_completes_nothing_and_is_defended_again_once_recovered() {
    let mut world = defender();
    world.beside(1, ENA);
    let events = end_defence(&mut world, [0, 0, granary::HAND_HEALTH]);
    assert_eq!(notices(&events), vec![RaidNotice::Overrun]);
    assert_eq!(world.progress(1, quests::THE_GRANARY_STAND), vec![0, 0]);
    // Not ready while its aftermath is open.
    let events = world.send(1, ZoneCommand::QuestAction { npc: ENA });
    assert_eq!(refusal(&events), Some(ErrorCode::SiteNotReady));
    // Once the hands are up and the walls mended, Ena rings again.
    world.restore(|state| {
        state.site_raids[0].aftermath.clear();
        state.site_raids[0].damage = 0;
    });
    let events = world.act(1, ENA);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert!(world.zone.raid_at(GRANARY).unwrap().active.is_some());
}

#[test]
fn a_millbrook_defender_gets_no_credit_at_the_granary_and_the_reverse() {
    // A player with both defences open holds the granary: only the granary's objective is done.
    let mut completed = before(62);
    completed.extend([33, 36, 43, 44].map(QuestId::new));
    let mut world = World::new(&[(1, WARDEN)], &completed);
    world.accept(1, quests::THE_GRANARY_STAND);
    world.beside(1, quests::OSRIC_MILL);
    let events = world.send(
        1,
        ZoneCommand::AcceptQuest {
            npc: quests::OSRIC_MILL,
            quest: quests::HOLD_UNTIL_DAWN.get(),
        },
    );
    assert_eq!(refusal(&events), None, "{events:?}");
    let events = world.act(1, ENA);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert!(world.zone.raid().active.is_none());
    world.beside(1, ENA);
    end_defence(&mut world, [granary::HAND_HEALTH; 3]);
    assert_eq!(world.progress(1, quests::THE_GRANARY_STAND), vec![1, 0]);
    assert_eq!(world.progress(1, quests::HOLD_UNTIL_DAWN), vec![0]);
}

#[test]
fn granary_raiders_march_to_the_barn_and_strike_the_hands_first() {
    let mut world = defender();
    // Stand back by Ena and let the first wave come.
    world.beside(1, ENA);
    let mut struck = false;
    for _ in 0..9_000 {
        world.zone.advance_tick().unwrap();
        let incident = world.zone.raid_at(GRANARY).unwrap().active.clone();
        let Some(incident) = incident else {
            break;
        };
        if incident
            .civilians
            .iter()
            .any(|&health| health < granary::HAND_HEALTH)
        {
            struck = true;
            break;
        }
    }
    assert!(struck, "no raider reached the granary's hands");
    assert_eq!(world.zone.raid_at(GRANARY).unwrap().damage, 0);
}
