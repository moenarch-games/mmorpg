//! Millbrook's defence (#1019, slice 3a): *Barricades at Millbrook* (44) raises two construction
//! sites from timber carried from the farm's pile, *Hold Until Dawn* (45) runs a quest-started
//! defence whose standing farmfolk, not the raiders slain, decide it, and *After the Raid* (46)
//! credits the aid actually given in its aftermath. Everything is decided by the zone, once.
use crate::support;

use std::sync::Arc;

use mmorpg_core::greyhaven_vale::{self, millbrook_defence as farm, quests};
use mmorpg_core::raid::{
    CIVILIAN_RECOVERY_TICKS, INJURED_HEALTH_PERCENT, PHASE_TICKS, RAID_PHASES,
};
use mmorpg_core::{
    CONSTRUCTION_STAND_TICKS, CharacterId, CharacterLocation, EntityKind, ErrorCode, NpcId,
    PieceId, QuestId, QuestLog, RaidNotice, RaidOutcome, RaidResolution, ZoneCommand, ZoneEvent,
    ZoneId, ZoneSimulation,
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

    /// Edits the canonical state and restores it, as recovery would.
    fn restore(&mut self, edit: impl FnOnce(&mut mmorpg_core::CanonicalZoneSnapshot)) {
        let mut state = self.zone.snapshot().unwrap();
        edit(&mut state);
        let content = Arc::clone(self.zone.content());
        self.zone = ZoneSimulation::from_snapshot(state, content).unwrap();
    }

    /// Moves the player's body to stand at `[x, z]`, as walking there would.
    fn stand_at(&mut self, [x, z]: [i32; 2]) {
        let y = self.zone.content().definition().ground_at(x, z) + 90;
        self.restore(|state| {
            state.players[0].position = [x as f32, y as f32, z as f32];
            state.players[0].velocity = [0.; 3];
        });
        for _ in 0..3 {
            self.zone.advance_tick().unwrap();
        }
    }

    fn beside(&mut self, npc: NpcId) {
        let [x, z] = self.zone.npc_placement(npc).unwrap().position;
        self.stand_at([x + 150, z + 150]);
    }

    fn quest(&mut self, quest: QuestId, accept: bool) -> Vec<ZoneEvent> {
        self.beside(quests::OSRIC_MILL);
        self.send(if accept {
            ZoneCommand::AcceptQuest {
                npc: quests::OSRIC_MILL,
                quest: quest.get(),
            }
        } else {
            ZoneCommand::CompleteQuest {
                npc: quests::OSRIC_MILL,
                quest: quest.get(),
                choice: 0,
            }
        })
    }

    fn progress(&self, quest: QuestId) -> Option<[u8; 3]> {
        self.log()
            .entry(quest)
            .map(|entry| [entry.progress[0], entry.progress[1], entry.progress[2]])
    }

    fn log(&self) -> QuestLog {
        self.zone
            .capture_character(1, CharacterId::new("farmhand").unwrap())
            .unwrap()
            .quests
    }

    fn piece_health(&self, piece: PieceId) -> u32 {
        self.zone
            .snapshot()
            .unwrap()
            .pieces
            .iter()
            .find(|state| state.piece == piece)
            .unwrap()
            .health
    }

    /// Fetches one load of timber from the pile and sets it down beside `piece`.
    fn carry_timber_to(&mut self, piece: PieceId) -> Vec<ZoneEvent> {
        let [x, _, z] = self.zone.content().load_spot(farm::TIMBER_PILE).unwrap().at;
        self.stand_at([x + 100, z + 100]);
        let events = self.send(ZoneCommand::Lift {
            source: Some(farm::TIMBER_PILE),
        });
        assert_eq!(refusal(&events), None, "{events:?}");
        let site = self
            .zone
            .content()
            .pieces()
            .iter()
            .find(|candidate| candidate.id == piece)
            .unwrap()
            .clone();
        // Beside the barricade's long side, outside its box.
        let [px, _, pz] = site.position;
        let [hx, _, hz] = site.half_extents;
        self.stand_at(if hx > hz {
            [px, pz + hz + 120]
        } else {
            [px + hx + 120, pz]
        });
        self.send(ZoneCommand::SetDown)
    }
}

fn refusal(events: &[ZoneEvent]) -> Option<ErrorCode> {
    events.iter().find_map(|event| match event {
        ZoneEvent::Error { code, .. } => Some(*code),
        _ => None,
    })
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

/// A level 5 Warden who turned in `done` and every quest before it, beside Osric.
fn farmhand(done: QuestId) -> Player {
    let mut donor = ZoneSimulation::with_content(GREYHAVEN, greyhaven_vale::content()).unwrap();
    donor.add_player(1).unwrap();
    let mut donor = classes::at_level(donor, 1, 5);
    donor
        .apply_command(1, 1, ZoneCommand::ChooseClass { class: 0, sex: 0 })
        .unwrap();
    donor.advance_tick().unwrap();
    let mut record = donor
        .capture_character(1, CharacterId::new("farmhand").unwrap())
        .unwrap();
    let content = greyhaven_vale::content();
    let mut completed = std::collections::BTreeSet::new();
    let mut next = Some(done);
    while let Some(quest) = next {
        completed.insert(quest);
        next = content.quest(quest).unwrap().prerequisite;
    }
    record.quests = QuestLog {
        entries: Vec::new(),
        completed,
        outcomes: Default::default(),
    };
    let [x, z] = content.npc(quests::OSRIC_MILL).unwrap().position;
    record.location = CharacterLocation {
        zone_id: GREYHAVEN,
        position: [x + 150, 1_190, z + 150],
        facing: 0,
    };
    let mut zone = ZoneSimulation::with_content(GREYHAVEN, content).unwrap();
    zone.admit_character(1, &record).unwrap();
    zone.advance_tick().unwrap();
    let mut player = Player { zone, sequence: 10 };
    // Ten days on, at the same hour of the day: late enough to step back over any timer.
    player.restore(|state| state.tick += 10 * u64::from(mmorpg_core::DAY_TICKS));
    player.beside(quests::OSRIC_MILL);
    player
}

#[test]
fn barricades_rise_from_carried_timber_and_stand_for_a_while() {
    let mut player = farmhand(quests::THE_LAST_SACK);
    // Both barricades start unbuilt: no health and no collider.
    for piece in [farm::NORTH_BARRICADE, farm::EAST_BARRICADE] {
        assert_eq!(player.piece_health(piece), 0);
    }
    // Before the quest the pile hands nobody timber.
    let [x, _, z] = player
        .zone
        .content()
        .load_spot(farm::TIMBER_PILE)
        .unwrap()
        .at;
    player.stand_at([x + 100, z + 100]);
    let events = player.send(ZoneCommand::Lift {
        source: Some(farm::TIMBER_PILE),
    });
    assert_eq!(refusal(&events), Some(ErrorCode::InvalidQuest));
    let events = player.quest(quests::BARRICADES_AT_MILLBROOK, true);
    assert_eq!(refusal(&events), None, "{events:?}");

    // One load is not a barricade.
    let events = player.carry_timber_to(farm::NORTH_BARRICADE);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert_eq!(player.zone.load(1), None, "the timber went into the site");
    let site = player.zone.construction(farm::NORTH_BARRICADE).unwrap();
    assert_eq!(
        (site.materials, site.builders.clone(), site.raised),
        (1, vec![1], None)
    );
    assert_eq!(player.piece_health(farm::NORTH_BARRICADE), 0);
    assert_eq!(
        player.progress(quests::BARRICADES_AT_MILLBROOK),
        Some([0, 0, 0])
    );
    // The second raises it: full health, its collider, and the build objective done once.
    let events = player.carry_timber_to(farm::NORTH_BARRICADE);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert_eq!(player.piece_health(farm::NORTH_BARRICADE), 90);
    let site = player.zone.construction(farm::NORTH_BARRICADE).unwrap();
    assert_eq!((site.materials, site.builders.is_empty()), (0, true));
    assert!(site.raised.is_some());
    assert_eq!(
        player.progress(quests::BARRICADES_AT_MILLBROOK),
        Some([1, 0, 0])
    );
    // More timber at a standing barricade is only set down on the ground.
    player.carry_timber_to(farm::NORTH_BARRICADE);
    assert!(
        player
            .zone
            .load(1)
            .is_some_and(|load| load.placed.is_some())
    );
    assert_eq!(
        player.progress(quests::BARRICADES_AT_MILLBROOK),
        Some([1, 0, 0])
    );

    // The state survives recovery, then the east barricade rises the same way.
    player.restore(|_| {});
    player.carry_timber_to(farm::EAST_BARRICADE);
    player.carry_timber_to(farm::EAST_BARRICADE);
    assert_eq!(player.piece_health(farm::EAST_BARRICADE), 90);
    assert_eq!(
        player.progress(quests::BARRICADES_AT_MILLBROOK),
        Some([1, 1, 0])
    );
    let events = player.quest(quests::BARRICADES_AT_MILLBROOK, false);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert!(player.log().is_completed(quests::BARRICADES_AT_MILLBROOK));
    // With no open build the pile hands out no more timber.
    player.stand_at([x + 100, z + 100]);
    let events = player.send(ZoneCommand::Lift {
        source: Some(farm::TIMBER_PILE),
    });
    assert_eq!(refusal(&events), Some(ErrorCode::InvalidQuest));

    // The farmhands take a barricade down once it stood its time, so others raise it anew.
    let tick = player.zone.current_tick();
    player.restore(|state| {
        let site = &mut state.constructions[0];
        assert_eq!(site.piece, farm::NORTH_BARRICADE);
        site.raised = Some(tick - u64::from(CONSTRUCTION_STAND_TICKS));
    });
    player.zone.advance_tick().unwrap();
    assert_eq!(player.piece_health(farm::NORTH_BARRICADE), 0);
    assert_eq!(
        player
            .zone
            .construction(farm::NORTH_BARRICADE)
            .unwrap()
            .raised,
        None
    );
    assert_eq!(player.piece_health(farm::EAST_BARRICADE), 90);
}

/// A defender on *Hold Until Dawn*, its defence started at Osric.
fn defender() -> Player {
    let mut player = farmhand(quests::BARRICADES_AT_MILLBROOK);
    let events = player.quest(quests::HOLD_UNTIL_DAWN, true);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert!(player.zone.defence_ready());
    let events = player.send(ZoneCommand::QuestAction {
        npc: quests::OSRIC_MILL,
    });
    assert_eq!(refusal(&events), None, "{events:?}");
    let incident = player.zone.raid().active.clone().unwrap();
    assert_eq!(incident.civilians, vec![farm::CIVILIAN_HEALTH; 3]);
    // A second ask while it runs is refused.
    let events = player.send(ZoneCommand::QuestAction {
        npc: quests::OSRIC_MILL,
    });
    assert_eq!(refusal(&events), Some(ErrorCode::SiteNotReady));
    player
}

/// Brings the running defence to its last phase's end with the player as a participant and the
/// civilians at `health`.
fn end_defence(player: &mut Player, health: [u32; 3]) -> Vec<ZoneEvent> {
    let tick = player.zone.current_tick();
    player.restore(|state| {
        let incident = state.raid.active.as_mut().unwrap();
        incident.phase = RAID_PHASES;
        incident.scaled_for = vec![1; usize::from(RAID_PHASES)];
        incident.phase_started = tick + 1 - u64::from(PHASE_TICKS);
        incident.participants = vec![1];
        incident.civilians = health.to_vec();
    });
    player.zone.advance_tick().unwrap();
    player.zone.snapshot_for_player(1).unwrap().events
}

#[test]
fn hold_until_dawn_holds_when_the_farmfolk_stand() {
    let mut player = defender();
    // The farmfolk show what they have left.
    let marta = |player: &Player| {
        player
            .zone
            .snapshot_for_player(1)
            .unwrap()
            .entities
            .iter()
            .find(|entity| entity.kind == EntityKind::Npc && entity.id == farm::MARTA_MILL.get())
            .map(|entity| entity.health_percent)
    };
    assert_eq!(marta(&player), Some(100));
    // One of three down still holds: two stand.
    let events = end_defence(&mut player, [0, 120, farm::CIVILIAN_HEALTH]);
    assert_eq!(notices(&events), vec![RaidNotice::Stood]);
    let last = player.zone.raid().last.unwrap();
    assert!(last.defence);
    assert_eq!(last.outcome, RaidOutcome::Defended);
    assert_eq!(player.progress(quests::HOLD_UNTIL_DAWN), Some([1, 0, 0]));
    // The aftermath: the two struck farmfolk need tending; the barn took no damage.
    let tasks = player.zone.content().raid().unwrap().aid.len() as u32;
    assert_eq!(
        player.zone.raid().aftermath,
        [tasks, tasks + 1].into_iter().collect()
    );
    assert_eq!(marta(&player), Some(INJURED_HEALTH_PERCENT));
    let events = player.quest(quests::HOLD_UNTIL_DAWN, false);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert!(player.log().is_completed(quests::HOLD_UNTIL_DAWN));
}

#[test]
fn an_overrun_defence_completes_nothing_and_is_tried_again_once_the_farm_recovered() {
    let mut player = defender();
    let events = end_defence(&mut player, [0, 0, farm::CIVILIAN_HEALTH]);
    assert_eq!(notices(&events), vec![RaidNotice::Overrun]);
    assert_eq!(player.progress(quests::HOLD_UNTIL_DAWN), Some([0, 0, 0]));
    // Not again while the fallen lie wounded …
    player.beside(quests::OSRIC_MILL);
    let events = player.send(ZoneCommand::QuestAction {
        npc: quests::OSRIC_MILL,
    });
    assert_eq!(refusal(&events), Some(ErrorCode::SiteNotReady));
    // … but once they recovered by themselves.
    let at = player.zone.raid().last.unwrap().at;
    let tick = player.zone.current_tick();
    player.restore(|state| {
        state.raid.last = Some(RaidResolution {
            at: tick - u64::from(CIVILIAN_RECOVERY_TICKS),
            ..state.raid.last.unwrap()
        });
    });
    assert!(at > 0);
    player.zone.advance_tick().unwrap();
    assert!(player.zone.raid().aftermath.is_empty());
    let events = player.send(ZoneCommand::QuestAction {
        npc: quests::OSRIC_MILL,
    });
    assert_eq!(refusal(&events), None, "{events:?}");
    assert!(
        player
            .zone
            .raid()
            .active
            .as_ref()
            .is_some_and(|incident| incident.defence())
    );
}

#[test]
fn a_defences_raiders_strike_the_farmfolk_before_the_barn() {
    let mut player = defender();
    let site = player.zone.content().raid().unwrap().clone();
    // The north approach's first raider, arrived at the barn's west wall beside Marta.
    let raider = site.approaches[0].raiders[0];
    let end = *site.approaches[0].route.last().unwrap();
    let ground = player.zone.content().definition().ground_at(end[0], end[1]);
    // The defender keeps well away, so the raider does not turn on them.
    player.stand_at([2_900, 2_400]);
    player.restore(|state| {
        let record = state
            .creatures
            .iter_mut()
            .find(|record| record.creature_id == raider)
            .unwrap();
        assert!(record.raid.is_some(), "the first wave stood it up");
        record.position = [end[0] as f32, (ground + 90) as f32, end[1] as f32];
        record.velocity = [0.; 3];
        let member = record.raid.as_mut().unwrap();
        member.waypoint = u8::try_from(site.approaches[0].route.len()).unwrap();
        member.anchor = end;
    });
    for _ in 0..3 {
        player.zone.advance_tick().unwrap();
    }
    let incident = player.zone.raid().active.clone().unwrap();
    assert!(
        incident.civilians[0] < farm::CIVILIAN_HEALTH,
        "{incident:?}"
    );
    assert_eq!(
        player.zone.raid().damage,
        0,
        "the barn is spared while Marta stands"
    );
}

#[test]
fn after_the_raid_credits_the_aid_actually_given() {
    let mut player = farmhand(quests::HOLD_UNTIL_DAWN);
    let events = player.quest(quests::AFTER_THE_RAID, true);
    assert_eq!(refusal(&events), None, "{events:?}");
    let site = player.zone.content().raid().unwrap().clone();
    let tasks = site.tasks();
    let marta = u32::try_from(site.aid.len()).unwrap();
    // A defence left a breach in the north wall and Marta wounded.
    let tick = player.zone.current_tick();
    player.restore(|state| {
        state.raid.incidents = 1;
        state.raid.damage = 500;
        state.raid.last = Some(RaidResolution {
            incident: 1,
            at: tick - 1,
            outcome: RaidOutcome::Defended,
            defence: true,
        });
        state.raid.aftermath = [0, marta].into_iter().collect();
    });
    // Tending is no mending: each counts toward its own objective, once.
    let at = tasks[marta as usize].at;
    player.stand_at([at[0] + 150, at[1] + 150]);
    let events = player.send(ZoneCommand::RaidAid { task: marta });
    assert_eq!(refusal(&events), None, "{events:?}");
    assert_eq!(player.progress(quests::AFTER_THE_RAID), Some([0, 1, 0]));
    let events = player.send(ZoneCommand::RaidAid { task: marta });
    assert_eq!(refusal(&events), Some(ErrorCode::InvalidTarget));
    // A tended farmhand gives the barn nothing back; a mended breach does.
    assert_eq!(player.zone.raid().damage, 500);
    let at = tasks[0].at;
    player.stand_at([at[0], at[1] - 150]);
    let events = player.send(ZoneCommand::RaidAid { task: 0 });
    assert_eq!(refusal(&events), None, "{events:?}");
    assert_eq!(player.progress(quests::AFTER_THE_RAID), Some([1, 1, 0]));
    assert!(player.zone.raid().damage < 500);
    let events = player.quest(quests::AFTER_THE_RAID, false);
    assert_eq!(refusal(&events), None, "{events:?}");
    assert!(player.log().is_completed(quests::AFTER_THE_RAID));
}
