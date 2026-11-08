//! The party-private Redbrand Depths (#404 part 2): content, composition by frozen party
//! size, the threshold request, chambers, the checkpoint, wipes and the way back out.
mod support;

use std::sync::Arc;

use mmorpg_core::greyhaven_vale::{
    self,
    cave::{self, THRESHOLD},
    quests,
    units::templates,
};
use mmorpg_core::redbrand_depths::{self, ARRIVAL, is_depths_zone};
use mmorpg_core::{
    CharacterId, CreatureId, CreatureLife, DepthsRunStart, ZoneCommand, ZoneId, ZoneSimulation,
    content_for_zone, depths_arrival,
};
use support::classes;

const SLOT: ZoneId = ZoneId::new(20_000);
const GREYHAVEN: ZoneId = ZoneId::new(1);

fn run(run_id: u64, roster: &[&str]) -> DepthsRunStart {
    DepthsRunStart {
        run_id,
        roster: roster
            .iter()
            .map(|name| CharacterId::new(*name).unwrap())
            .collect(),
        cleared: 0,
        checkpoint: false,
        outcome: None,
    }
}

fn depths_with(roster: &[&str]) -> ZoneSimulation {
    let mut zone = ZoneSimulation::with_content(SLOT, redbrand_depths::content()).unwrap();
    zone.begin_depths_run(run(1, roster)).unwrap();
    zone
}

fn party(size: usize) -> Vec<&'static str> {
    ["aria", "bren", "cole", "dana", "elis"][..size].to_vec()
}

/// `(template, alive at start)` of the run's standing creatures, in ID order.
fn standing(zone: &ZoneSimulation) -> Vec<CreatureId> {
    zone.snapshot()
        .unwrap()
        .creatures
        .iter()
        .filter(|creature| creature.life == CreatureLife::Alive)
        .map(|creature| creature.creature_id)
        .collect()
}

fn templates_of(ids: &[CreatureId]) -> Vec<mmorpg_core::CreatureTemplateId> {
    let content = redbrand_depths::content();
    ids.iter()
        .map(|id| content.creature_spawn(*id).unwrap().template)
        .collect()
}

#[test]
fn slots_all_host_the_same_valid_depths_and_only_redbrands_stand() {
    for id in [SLOT.get(), redbrand_depths::ZONE_IDS.end - 1] {
        assert!(is_depths_zone(ZoneId::new(id)));
        assert_eq!(
            content_for_zone(ZoneId::new(id)).revision(),
            redbrand_depths::content().revision()
        );
    }
    assert!(!is_depths_zone(ZoneId::new(19_999)));
    assert!(!is_depths_zone(GREYHAVEN));
    let content = redbrand_depths::content();
    for spawn in content.creature_spawns() {
        assert!(matches!(
            spawn.template,
            templates::REDBRAND_BANDIT
                | templates::REDBRAND_BRUTE
                | templates::REDBRAND_CROSSBOWMAN
                | redbrand_depths::THORN_TEMPLATE
        ));
    }
    // Before any run binds, nothing stands: creatures are reserved.
    let zone = ZoneSimulation::with_content(SLOT, redbrand_depths::content()).unwrap();
    assert!(standing(&zone).is_empty());
}

#[test]
fn the_composition_grows_and_changes_with_party_size_from_one_to_five() {
    let by_size: Vec<_> = (1..=5)
        .map(|size| standing(&depths_with(&party(size))))
        .collect();
    // Strictly more enemies for every party size: numbers, not multipliers.
    for pair in by_size.windows(2) {
        assert!(pair[0].len() < pair[1].len(), "{pair:?}");
    }
    let solo = &by_size[0];
    let five = &by_size[4];
    // Different composition, not the same creatures doubled: a party of five draws Crossbowmen a
    // solo player never faces in the first chamber, and the larger brute roster.
    let brutes = |ids: &Vec<CreatureId>| {
        templates_of(ids)
            .iter()
            .filter(|template| **template == templates::REDBRAND_BRUTE)
            .count()
    };
    assert!(brutes(five) > brutes(solo));
    assert!(five.len() >= 3 * solo.len().min(1));
    let first_chamber_template =
        |ids: &[CreatureId], id: u32| ids.iter().any(|creature| creature.get() == id);
    assert!(
        !first_chamber_template(solo, 4),
        "no solo crossbowman in the first chamber"
    );
    assert!(first_chamber_template(five, 4));
}

/// Marks creatures fallen in a run, as if a party killed them, keeping the content.
fn with_fallen(zone: ZoneSimulation, ids: &[u32], tick: u64) -> ZoneSimulation {
    let mut state = zone.snapshot().unwrap();
    for id in ids {
        let creature = state
            .creatures
            .iter_mut()
            .find(|creature| creature.creature_id.get() == *id)
            .expect("a site creature");
        creature.life = CreatureLife::Despawned { died_at: tick };
        creature.health = 0;
        creature.position = [0.0; 3];
        creature.velocity = [0.0; 3];
        creature.threat.clear();
        creature.ai = mmorpg_core::CreatureAi::Idle {
            timer: 0,
            destination: None,
        };
        creature.abilities = Default::default();
    }
    ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap()
}

#[test]
fn a_run_starts_at_most_once_and_a_second_start_changes_nothing() {
    let mut zone = depths_with(&party(1));
    let before = zone.snapshot().unwrap();
    assert!(!zone.begin_depths_run(run(1, &party(1))).unwrap());
    assert_eq!(zone.snapshot().unwrap(), before);
    // A character not on the roster is not admitted, whatever the host asks.
    assert!(!zone.depths_admits(&CharacterId::new("dana").unwrap()));
    assert!(zone.depths_admits(&CharacterId::new("aria").unwrap()));
    // The roster length is the frozen party size.
    assert_eq!(zone.depths_run().unwrap().profile(), 1);
}

#[test]
fn clearing_the_first_chamber_clears_it_and_unlocks_the_checkpoint() {
    // A party of two fights the first chamber's two bandits and one more.
    let mut zone = depths_with(&party(2));
    zone.advance_tick().unwrap();
    assert_eq!(zone.depths_run().unwrap().cleared, 0);
    assert!(!zone.depths_run().unwrap().checkpoint);

    let chamber: Vec<u32> = standing(&zone)
        .iter()
        .map(|id| id.get())
        .filter(|id| *id <= 6)
        .collect();
    let mut zone = with_fallen(zone, &chamber, 1);
    zone.advance_tick().unwrap();
    let progress = zone.depths_run().unwrap();
    assert_eq!(progress.cleared & 1, 1);
    assert!(progress.checkpoint);
    // The checkpoint survives recovery.
    let state = zone.snapshot().unwrap();
    let restored =
        ZoneSimulation::from_snapshot(state.clone(), Arc::clone(zone.content())).unwrap();
    assert!(restored.depths_run().unwrap().checkpoint);
    assert_eq!(restored.snapshot().unwrap(), state);
}

#[test]
fn a_larger_party_draws_reinforcements_once_defenders_fall() {
    let mut zone = depths_with(&party(4));
    zone.advance_tick().unwrap();
    assert!(!standing(&zone).iter().any(|id| id.get() == 13));
    // Two of the guard room's defenders fall: its reinforcement arrives.
    let mut zone = with_fallen(zone, &[7, 8], 1);
    zone.advance_tick().unwrap();
    assert!(standing(&zone).iter().any(|id| id.get() == 13));
}

#[test]
fn a_cleared_chambers_creatures_never_stand_again_and_a_rebound_run_keeps_its_progress() {
    let mut zone = depths_with(&party(1));
    zone.advance_tick().unwrap();
    let chamber: Vec<u32> = standing(&zone)
        .iter()
        .map(|id| id.get())
        .filter(|id| *id <= 6)
        .collect();
    let mut zone = with_fallen(zone, &chamber, 1);
    // A long gap passes: fallen creatures would normally respawn, but these never do.
    for _ in 0..500 {
        zone.advance_tick().unwrap();
    }
    assert!(!standing(&zone).iter().any(|id| chamber.contains(&id.get())));

    // A restarted host rebinds the same run from the directory's recorded progress.
    let run_state = zone.depths_run().unwrap().clone();
    let mut rebound = ZoneSimulation::with_content(SLOT, redbrand_depths::content()).unwrap();
    rebound
        .begin_depths_run(DepthsRunStart {
            run_id: run_state.run_id,
            roster: run_state.roster,
            cleared: run_state.cleared,
            checkpoint: run_state.checkpoint,
            outcome: None,
        })
        .unwrap();
    assert!(!standing(&rebound).iter().any(|id| id.get() <= 6));
    assert!(!standing(&rebound).is_empty());
}

fn in_greyhaven(class: u8, at: [i32; 2], qualified: bool) -> (ZoneSimulation, u32) {
    let mut zone = ZoneSimulation::with_content(GREYHAVEN, greyhaven_vale::content()).unwrap();
    zone.add_player(1).unwrap();
    let mut zone = classes::at_level(zone, 1, 6);
    zone.apply_command(1, 1, ZoneCommand::ChooseClass { class, sex: 0 })
        .unwrap();
    zone.advance_tick().unwrap();
    let mut state = zone.snapshot().unwrap();
    state.players[0].position = [
        at[0] as f32,
        (cave::GREYFANG.floor_y + mmorpg_core::PLAYER_HALF_EXTENTS_UNITS[1]) as f32,
        at[1] as f32,
    ];
    if qualified {
        // The whole Garrick chain turned in, as a log must be.
        state.players[0].quests.completed = (1..=quests::GARRICK_REDBRAND_QUEST.get())
            .filter_map(|id| mmorpg_core::QuestId::new(id).bit())
            .fold(0, |mask, bit| mask | bit);
    }
    let mut zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
    zone.enable_depths_entry();
    (zone, 1)
}

/// Inside the threshold strip, at its corner farthest from the door's guard (three metres off).
const STRIP: [i32; 2] = [7_890, -13_620];

#[test]
fn standing_in_the_threshold_strip_reports_whether_a_character_qualifies() {
    let (mut zone, player) = in_greyhaven(0, STRIP, false);
    zone.advance_tick().unwrap();
    let requests = zone.threshold_requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].player, player);
    assert_eq!(requests[0].candidates, vec![(player, false)]);

    let (mut qualified, player) = in_greyhaven(0, STRIP, true);
    qualified.advance_tick().unwrap();
    assert_eq!(
        qualified.threshold_requests()[0].candidates,
        vec![(player, true)]
    );

    // Elsewhere, no request; the public cavern itself has no gate.
    let (mut away, _) = in_greyhaven(0, [THRESHOLD.0[0] - 800, THRESHOLD.0[1]], true);
    away.advance_tick().unwrap();
    assert!(away.threshold_requests().is_empty());
}

#[test]
fn a_transfer_arrives_in_the_entrance_hall_and_walking_out_returns_to_the_public_cave() {
    let zone = depths_with(&party(1));
    assert_eq!(depths_arrival(SLOT).position, ARRIVAL);
    assert_eq!(depths_arrival(SLOT).zone_id, SLOT);
    // The arrival stands clear of every wall, inside the entrance hall.
    assert!(zone.stands_clear(ARRIVAL, mmorpg_core::PLAYER_HALF_EXTENTS_UNITS));

    // The exit strip's far side lands in the public cave's mouth, on clear ground.
    let greyhaven = ZoneSimulation::with_content(GREYHAVEN, greyhaven_vale::content()).unwrap();
    let exit = redbrand_depths::content().handoff_corridors()[0];
    assert_eq!(exit.neighbor, GREYHAVEN);
    for (x, z) in [
        (exit.crossing.min[0], exit.crossing.min[1]),
        (exit.crossing.min[0], exit.crossing.max[1] - 1),
        (exit.crossing.max[0] - 1, exit.crossing.min[1]),
        (exit.crossing.max[0] - 1, exit.crossing.max[1] - 1),
    ] {
        let landed = [x + exit.offset[0], 90 + exit.offset[1], z + exit.offset[2]];
        assert!(cave::GREYFANG.flattens(landed[0], landed[2]), "{landed:?}");
        assert!(
            greyhaven.stands_clear(landed, mmorpg_core::PLAYER_HALF_EXTENTS_UNITS),
            "{landed:?}"
        );
    }
}

#[test]
fn a_character_of_a_retired_run_returns_at_the_public_entrance() {
    let (mut zone, player) = in_greyhaven(0, STRIP, true);
    let character = CharacterId::new("aria").unwrap();
    let mut record = zone.capture_character(player, character).unwrap();
    assert!(zone.remove_player(player));
    record.location = mmorpg_core::CharacterLocation {
        zone_id: SLOT,
        position: ARRIVAL,
        facing: 0,
    };
    zone.admit_character(2, &record).unwrap();
    assert_eq!(
        zone.snapshot().unwrap().players[0]
            .position
            .map(|v| v as i32),
        cave::depths_exit_entrance()
    );
}

#[test]
fn two_runs_keep_their_own_state() {
    let mut first = depths_with(&party(1));
    let mut second =
        ZoneSimulation::with_content(ZoneId::new(20_001), redbrand_depths::content()).unwrap();
    second.begin_depths_run(run(2, &["tess"])).unwrap();
    first.advance_tick().unwrap();
    second.advance_tick().unwrap();
    let chamber: Vec<u32> = standing(&first)
        .iter()
        .map(|id| id.get())
        .filter(|id| *id <= 6)
        .collect();
    let mut first = with_fallen(first, &chamber, 1);
    first.advance_tick().unwrap();
    assert_eq!(first.depths_run().unwrap().cleared & 1, 1);
    assert_eq!(second.depths_run().unwrap().cleared, 0);
    assert!(first.depths_admits(&CharacterId::new("aria").unwrap()));
    assert!(!second.depths_admits(&CharacterId::new("aria").unwrap()));
}

#[test]
fn a_gathered_party_enters_together_with_each_members_qualification() {
    use mmorpg_core::{PartyMember, PartyRoster, PartyState};
    let (zone, player) = in_greyhaven(0, STRIP, true);
    let mut state = zone.snapshot().unwrap();
    let mut mate = state.players[0].clone();
    mate.player_id = 2;
    mate.spawn_slot = 1;
    mate.quests.completed = 0;
    // A step along the strip: gathered, unqualified, and clear of the first character.
    mate.position[2] -= 200.0;
    state.players.push(mate);
    let mut zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
    zone.enable_depths_entry();
    for (revision, id) in [(1, player), (2, 2)] {
        zone.apply_party_state(
            id,
            PartyState {
                revision,
                roster: Some(PartyRoster {
                    party: 7,
                    version: 1,
                    members: [player, 2]
                        .map(|member| PartyMember {
                            name: format!("Player {member}"),
                            player: Some(member),
                            online: true,
                        })
                        .to_vec(),
                }),
                invite: None,
            },
        )
        .unwrap();
    }
    zone.advance_tick().unwrap();
    let requests = zone.threshold_requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].player, player);
    assert_eq!(requests[0].candidates, vec![(player, true), (2, false)]);
}

#[test]
fn a_wipe_stands_the_party_up_at_the_entrance_and_leaves_the_fallen_fallen() {
    let (donor, player) = in_greyhaven(0, STRIP, true);
    let mut record = donor
        .capture_character(player, CharacterId::new("aria").unwrap())
        .unwrap();
    record.location = depths_arrival(SLOT);
    record.health = 0; // Dead on arrival, as if the fight went badly.
    let mut zone = depths_with(&party(1));
    zone.admit_character(player, &record).unwrap();
    zone.advance_tick().unwrap();
    let survivor = standing(&zone)[1];
    // One creature of the first chamber has fallen, and a survivor is left wounded and far.
    let mut state = zone.snapshot().unwrap();
    let fallen = state
        .creatures
        .iter_mut()
        .find(|c| c.creature_id.get() == 1)
        .unwrap();
    fallen.life = CreatureLife::Despawned { died_at: 1 };
    fallen.health = 0;
    fallen.position = [0.0; 3];
    fallen.velocity = [0.0; 3];
    fallen.threat.clear();
    fallen.abilities = Default::default();
    fallen.ai = mmorpg_core::CreatureAi::Idle {
        timer: 0,
        destination: None,
    };
    let wounded = state
        .creatures
        .iter_mut()
        .find(|c| c.creature_id == survivor)
        .unwrap();
    wounded.health = 1;
    let mut zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();

    for _ in 0..mmorpg_core::WIPE_DELAY_TICKS + 2 {
        zone.advance_tick().unwrap();
    }
    let state = zone.snapshot().unwrap();
    assert!(state.players[0].combat.health > 0, "the party stands up");
    assert!(
        state.players[0].position[0] < -8_000.0,
        "at the entrance: {:?}",
        state.players[0].position
    );
    let creature = |id: u32| {
        state
            .creatures
            .iter()
            .find(|c| c.creature_id.get() == id)
            .unwrap()
    };
    assert_ne!(
        creature(1).life,
        CreatureLife::Alive,
        "the fallen stay fallen"
    );
    let healed = creature(survivor.get());
    assert_eq!(healed.life, CreatureLife::Alive);
    assert!(healed.health > 1, "survivors go home healed");
    assert_eq!(zone.depths_run().unwrap().cleared, 0);
}
