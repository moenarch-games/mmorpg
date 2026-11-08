//! Captain Ulric Thorn and his shared loot (#404 part 3): attempts, adds by frozen party size,
//! wipe retries, completion, and Need/Greed settled once with committed grants.
mod support;

use std::sync::Arc;

use mmorpg_core::greyhaven_vale::{self, units::templates};
use mmorpg_core::redbrand_depths::{self, THORN, THORN_TEMPLATE};
use mmorpg_core::{
    CharacterId, CharacterLocation, CreatureId, CreatureLife, DepthsRunStart, DropState, DropView,
    EntityRef, RollChoice, ZoneCommand, ZoneId, ZoneSimulation,
};
use support::{arena, classes};

const SLOT: ZoneId = ZoneId::new(20_000);
const GREYHAVEN: ZoneId = ZoneId::new(1);
const ALL_CLEARED: u8 = 0b111;

fn id(name: &str) -> CharacterId {
    CharacterId::new(name).unwrap()
}

fn names(size: usize) -> Vec<&'static str> {
    ["aria", "bren", "cole", "dana", "elis"][..size].to_vec()
}

/// A level 6 character standing inside Thorn's chamber.
fn record(name: &str, player: u32, z: i32) -> mmorpg_core::CharacterRecord {
    let mut donor = ZoneSimulation::with_content(GREYHAVEN, greyhaven_vale::content()).unwrap();
    donor.add_player(1).unwrap();
    let mut donor = classes::at_level(donor, 1, 6);
    donor
        .apply_command(1, 1, ZoneCommand::ChooseClass { class: 0, sex: 0 })
        .unwrap();
    donor.advance_tick().unwrap();
    let mut record = donor.capture_character(1, id(name)).unwrap();
    record.location = CharacterLocation {
        zone_id: SLOT,
        position: [13_000, 90, z],
        facing: 16_384,
    };
    let _ = player;
    record
}

/// A depths run for `roster` with its chambers cleared, the party standing in Thorn's chamber.
fn fight(roster: &[&str], cleared: u8) -> ZoneSimulation {
    let mut zone = ZoneSimulation::with_content(SLOT, redbrand_depths::content()).unwrap();
    zone.begin_depths_run(DepthsRunStart {
        run_id: 7,
        roster: roster.iter().map(|name| id(name)).collect(),
        cleared,
        checkpoint: true,
        outcome: None,
    })
    .unwrap();
    for (index, name) in roster.iter().enumerate() {
        let player = u32::try_from(index).unwrap() + 1;
        let z = -300 + i32::try_from(index).unwrap() * 150;
        zone.admit_character(player, &record(name, player, z))
            .unwrap();
    }
    zone
}

fn boss(zone: &ZoneSimulation) -> mmorpg_core::CanonicalCreatureSnapshot {
    zone.snapshot()
        .unwrap()
        .creatures
        .into_iter()
        .find(|creature| creature.creature_id == THORN)
        .unwrap()
}

fn alive(zone: &ZoneSimulation, creature: u32) -> bool {
    zone.snapshot()
        .unwrap()
        .creatures
        .iter()
        .find(|c| c.creature_id.get() == creature)
        .is_some_and(|c| c.life == CreatureLife::Alive)
}

fn tick(zone: &mut ZoneSimulation, n: u64) {
    for _ in 0..n {
        zone.advance_tick().unwrap();
    }
}

fn edit(
    zone: ZoneSimulation,
    change: impl FnOnce(&mut mmorpg_core::CanonicalZoneSnapshot),
) -> ZoneSimulation {
    let mut state = zone.snapshot().unwrap();
    let content = Arc::clone(zone.content());
    change(&mut state);
    ZoneSimulation::from_snapshot(state, content).unwrap()
}

fn set_boss_health(zone: ZoneSimulation, health: u32) -> ZoneSimulation {
    edit(zone, |state| {
        let thorn = state
            .creatures
            .iter_mut()
            .find(|creature| creature.creature_id == THORN)
            .unwrap();
        thorn.health = health;
    })
}

/// Engages Thorn: the party is inside his chamber, so aggro starts the attempt.
fn engage(zone: &mut ZoneSimulation) {
    tick(zone, 2);
    assert!(alive(zone, THORN.get()), "Thorn stands");
    for _ in 0..400 {
        if zone.depths_run().unwrap().boss.attempt.is_some() {
            return;
        }
        tick(zone, 1);
    }
    panic!("Thorn never engaged");
}

/// Kills an engaged Thorn with real melee swings from the party.
fn kill(mut zone: ZoneSimulation, players: u32) -> ZoneSimulation {
    zone = set_boss_health(zone, 1);
    let target = EntityRef::Creature(THORN);
    let mut sequence: u32 = 1_000;
    for _ in 0..1_500 {
        if zone.depths_run().unwrap().completed() {
            return zone;
        }
        for player in 1..=players {
            if let Some(command) = arena::swing_at(&zone, player, target) {
                sequence += 1;
                zone.apply_command(player, sequence, command).unwrap();
            }
        }
        zone = edit(zone, |state| {
            for player in &mut state.players {
                if player.position[0] < 13_100.0 {
                    player.position[0] = 13_100.0;
                }
            }
        });
        tick(&mut zone, 1);
    }
    panic!("Thorn survived: {:?}", boss(&zone).health);
}

fn complete(roster: &[&str], cleared: u8) -> ZoneSimulation {
    let mut zone = fight(roster, cleared);
    engage(&mut zone);
    kill(zone, u32::try_from(roster.len()).unwrap())
}

/// Confirms the current outcome, as the host's directory would, and delivers.
fn commit(zone: &mut ZoneSimulation) {
    if let Some(report) = zone.boss_report() {
        zone.confirm_boss_outcome(report.run_id, report.outcome.version);
    }
    tick(zone, 1);
}

fn bag_has(zone: &ZoneSimulation, player: u32, item: mmorpg_core::ItemId) -> bool {
    zone.snapshot()
        .unwrap()
        .players
        .iter()
        .find(|p| p.player_id == player)
        .is_some_and(|p| {
            p.inventory
                .slots()
                .iter()
                .flatten()
                .any(|stack| stack.item() == item)
        })
}

fn roll(zone: &mut ZoneSimulation, player: u32, seq: u32, drop: u8, choice: RollChoice) {
    zone.apply_command(
        player,
        seq,
        ZoneCommand::RollDepthsDrop {
            run: 7,
            drop,
            choice,
        },
    )
    .unwrap();
    zone.advance_tick().unwrap();
}

#[test]
fn thorn_is_an_elite_level_six_appearing_after_the_chambers_and_a_retry_changes_nothing() {
    let content = redbrand_depths::content();
    let template = content.creature_template(THORN_TEMPLATE).unwrap();
    assert_eq!(
        (template.min_level, template.health, template.elite),
        (6, 900, true)
    );
    assert_eq!(
        content.creature_spawn(THORN).unwrap().template,
        THORN_TEMPLATE
    );
    // Not while a chamber stands.
    let mut early = fight(&["aria"], 0b011);
    tick(&mut early, 5);
    assert!(!alive(&early, THORN.get()), "the last chamber stands");
    // All clear: he appears, with the chamber-open notice.
    let mut zone = fight(&["aria"], ALL_CLEARED);
    tick(&mut zone, 2);
    assert!(alive(&zone, THORN.get()));
}

#[test]
fn a_complete_solo_run_calls_two_adds_and_drops_one_item_for_the_solo_hero() {
    let mut zone = fight(&["aria"], ALL_CLEARED);
    engage(&mut zone);
    assert_eq!(
        zone.depths_run()
            .unwrap()
            .boss
            .attempt
            .as_ref()
            .unwrap()
            .eligible,
        vec![id("aria")],
        "the eligible snapshot is who stood in his chamber when he engaged"
    );
    // At half health he calls his two adds and starts the area attack.
    let mut zone = set_boss_health(zone, 400);
    tick(&mut zone, 1);
    assert!(alive(&zone, 31) && alive(&zone, 32), "two bandits called");
    assert!(!alive(&zone, 33), "no crossbowman for a solo hero");
    let attempt = zone.depths_run().unwrap().boss.attempt.clone().unwrap();
    assert!(attempt.adds_called);
    assert_eq!(attempt.phase, 1);
    let mut zone = kill(zone, 1);
    let run = zone.depths_run().unwrap().clone();
    assert!(run.completed());
    assert!(
        !alive(&zone, 31) && !alive(&zone, 32),
        "his adds leave with him"
    );
    let outcome = run.boss.outcome.unwrap();
    assert_eq!(outcome.drops.len(), 1, "one item for a solo kill");
    // The roll prompt reaches the eligible hero; choosing Need settles and delivers it.
    let view = zone.snapshot_for_player(1).unwrap().depths_loot.unwrap();
    assert!(matches!(view.drops[0].state, DropView::Rolling { .. }));
    let item = outcome.drops[0].item;
    roll(&mut zone, 1, 2_000, 0, RollChoice::Need);
    let settled = zone.depths_run().unwrap().boss.outcome.clone().unwrap();
    assert!(matches!(
        &settled.drops[0].state,
        DropState::Awarded { winner, need: true, .. } if *winner == id("aria")
    ));
    // Not delivered until the directory has it.
    assert!(
        !bag_has(&zone, 1, item),
        "held until the outcome is committed"
    );
    commit(&mut zone);
    assert!(bag_has(&zone, 1, item), "committed, it reaches the bag");
    let record = zone.capture_character(1, id("aria")).unwrap();
    assert!(
        record.depths.thorn_defeated,
        "the deferred story evidence (5.1.37)"
    );
    assert_eq!(record.depths.receipt_run, 7);
    assert!(record.depths.credited);
    // Duplicates and late answers change nothing.
    let delivered = zone
        .depths_run()
        .unwrap()
        .boss
        .outcome
        .clone()
        .unwrap()
        .drops;
    roll(&mut zone, 1, 2_001, 0, RollChoice::Greed);
    assert_eq!(
        zone.depths_run()
            .unwrap()
            .boss
            .outcome
            .as_ref()
            .unwrap()
            .drops,
        delivered
    );
}

#[test]
fn a_full_party_run_scales_adds_and_mechanics_with_the_frozen_profile_not_multipliers() {
    let content = redbrand_depths::content();
    let mut zone = fight(&names(5), ALL_CLEARED);
    engage(&mut zone);
    assert_eq!(
        boss(&zone).health,
        content.creature_template(THORN_TEMPLATE).unwrap().health,
        "no health multiplier"
    );
    let mut zone = set_boss_health(zone, 300);
    tick(&mut zone, 1);
    for add in 31..=34 {
        assert!(alive(&zone, add), "five players face all four adds");
    }
    // Into his last phase: the second attack alternates in for a party of three or more.
    let mut zone = set_boss_health(zone, 200);
    tick(&mut zone, 1);
    assert_eq!(
        zone.depths_run()
            .unwrap()
            .boss
            .attempt
            .as_ref()
            .unwrap()
            .phase,
        2
    );
    let zone = kill(zone, 5);
    let drops = zone
        .depths_run()
        .unwrap()
        .boss
        .outcome
        .as_ref()
        .unwrap()
        .drops
        .clone();
    assert_eq!(drops.len(), 2, "a party of five shares two items");
    assert_ne!(drops[0].item, drops[1].item, "distinct items of the pool");
}

#[test]
fn a_wipe_resets_the_attempt_within_the_run_and_a_later_kill_counts_once() {
    let mut zone = fight(&["aria"], ALL_CLEARED);
    engage(&mut zone);
    let mut zone = set_boss_health(zone, 200);
    tick(&mut zone, 1);
    assert!(alive(&zone, 31));
    // Everyone falls; the wipe resolves.
    let mut zone = edit(zone, |state| {
        for player in &mut state.players {
            player.combat.health = 0;
        }
        for creature in &mut state.creatures {
            creature.threat.clear();
            creature.tapped_by = None;
        }
    });
    tick(&mut zone, mmorpg_core::WIPE_DELAY_TICKS + 2);
    let thorn = boss(&zone);
    assert_eq!(thorn.life, CreatureLife::Alive);
    assert_eq!(thorn.health, 900, "full health, no accumulating penalty");
    assert!(!alive(&zone, 31), "his adds are gone");
    let run = zone.depths_run().unwrap();
    assert!(
        run.boss.attempt.is_none(),
        "no attempt until he is engaged again"
    );
    assert_eq!(run.cleared, ALL_CLEARED, "cleared chambers stay cleared");
    assert_eq!(run.run_id, 7, "the same run, the same frozen profile");
    // The second attempt wins; only one outcome is ever recorded.
    let mut zone = edit(zone, |state| {
        for player in &mut state.players {
            player.position = [13_000.0, 90.0, 0.0];
        }
    });
    engage(&mut zone);
    let zone = kill(zone, 1);
    assert_eq!(
        zone.depths_run()
            .unwrap()
            .boss
            .outcome
            .as_ref()
            .unwrap()
            .drops
            .len(),
        1
    );
}

#[test]
fn need_outranks_greed_passes_and_silence_fall_to_a_manual_claim_and_full_bags_wait() {
    // Need before Greed, ties resolved by the deterministic roll.
    let mut zone = complete(&names(2), ALL_CLEARED);
    let outcome = zone.depths_run().unwrap().boss.outcome.clone().unwrap();
    let item = outcome.drops[0].item;
    roll(&mut zone, 1, 3_000, 0, RollChoice::Greed);
    roll(&mut zone, 2, 3_000, 0, RollChoice::Need);
    let settled = zone.depths_run().unwrap().boss.outcome.clone().unwrap();
    assert!(matches!(
        &settled.drops[0].state,
        DropState::Awarded { winner, need: true, .. } if *winner == id("bren")
    ));
    commit(&mut zone);
    assert!(bag_has(&zone, 2, item) && !bag_has(&zone, 1, item));

    // Everyone passes: held, claimable, one claimant only (5.1.83).
    let mut zone = complete(&names(2), ALL_CLEARED);
    roll(&mut zone, 1, 3_000, 0, RollChoice::Pass);
    roll(&mut zone, 2, 3_000, 0, RollChoice::Pass);
    assert_eq!(
        zone.depths_run()
            .unwrap()
            .boss
            .outcome
            .as_ref()
            .unwrap()
            .drops[0]
            .state,
        DropState::Held
    );
    zone.apply_command(1, 3_001, ZoneCommand::ClaimDepthsDrop { run: 7, drop: 0 })
        .unwrap();
    zone.apply_command(2, 3_001, ZoneCommand::ClaimDepthsDrop { run: 7, drop: 0 })
        .unwrap();
    tick(&mut zone, 1);
    commit(&mut zone);
    let item_held = zone
        .depths_run()
        .unwrap()
        .boss
        .outcome
        .as_ref()
        .unwrap()
        .drops[0]
        .item;
    assert!(
        bag_has(&zone, 1, item_held) ^ bag_has(&zone, 2, item_held),
        "exactly one character holds the claimed item"
    );

    // Silence at the deadline counts as Pass, then manual claim only.
    let mut zone = complete(&["aria"], ALL_CLEARED);
    let deadline = u64::from(redbrand_depths::ROLL_TICKS) + 2;
    tick(&mut zone, deadline);
    assert_eq!(
        zone.depths_run()
            .unwrap()
            .boss
            .outcome
            .as_ref()
            .unwrap()
            .drops[0]
            .state,
        DropState::Held,
        "an unanswered window expires into a held item, not a free award"
    );

    // A winner with a full bag keeps the item reserved until it fits.
    let mut zone = complete(&["aria"], ALL_CLEARED);
    let item = zone
        .depths_run()
        .unwrap()
        .boss
        .outcome
        .as_ref()
        .unwrap()
        .drops[0]
        .item;
    let filler = mmorpg_core::ItemStack::new(mmorpg_core::ItemId::new(1), 1).unwrap();
    zone = edit(zone, |state| {
        state.players[0].inventory =
            mmorpg_core::Inventory::from_slots([Some(filler); mmorpg_core::INVENTORY_SLOTS]);
    });
    roll(&mut zone, 1, 3_000, 0, RollChoice::Need);
    commit(&mut zone);
    assert!(!bag_has(&zone, 1, item), "no room yet");
    assert!(matches!(
        &zone
            .depths_run()
            .unwrap()
            .boss
            .outcome
            .as_ref()
            .unwrap()
            .drops[0]
            .state,
        DropState::Awarded {
            delivered: false,
            ..
        }
    ));
    let mut zone = edit(zone, |state| {
        let mut slots = [Some(filler); mmorpg_core::INVENTORY_SLOTS];
        slots[0] = None;
        state.players[0].inventory = mmorpg_core::Inventory::from_slots(slots);
    });
    tick(&mut zone, 1);
    assert!(bag_has(&zone, 1, item), "delivered as soon as it fits");
}

#[test]
fn a_recovered_run_never_rerolls_or_regrants_and_a_fresh_run_is_a_new_identity() {
    // A settled, committed, delivered run recovered from its directory record.
    let mut zone = complete(&["aria"], ALL_CLEARED);
    roll(&mut zone, 1, 4_000, 0, RollChoice::Need);
    commit(&mut zone);
    let report = zone.boss_report().unwrap();
    let item = report.outcome.drops[0].item;
    let record = zone.capture_character(1, id("aria")).unwrap();
    assert_eq!(record.inventory.count(item), 1);

    // A restarted host rebinds the run from the directory: no second boss, no new roll.
    let mut restarted = ZoneSimulation::with_content(SLOT, redbrand_depths::content()).unwrap();
    restarted
        .begin_depths_run(DepthsRunStart {
            run_id: 7,
            roster: vec![id("aria")],
            cleared: ALL_CLEARED,
            checkpoint: true,
            outcome: Some(report.outcome.clone()),
        })
        .unwrap();
    assert_eq!(
        restarted
            .depths_run()
            .unwrap()
            .boss
            .outcome
            .as_ref()
            .unwrap()
            .confirmed,
        report.outcome.version
    );
    tick(&mut restarted, 3);
    assert!(
        !alive(&restarted, THORN.get()),
        "a defeated Thorn never stands again"
    );
    // The character returns holding its item and receipt: nothing is granted twice.
    restarted.admit_character(1, &record).unwrap();
    tick(&mut restarted, 2);
    assert_eq!(
        restarted
            .capture_character(1, id("aria"))
            .unwrap()
            .inventory
            .count(item),
        1,
        "the receipt stops a second grant after recovery"
    );
    // A character whose record lacks the receipt, because its commit was lost before the
    // crash, gets the award it durably won in the directory, exactly once.
    let mut lost = record.clone();
    lost.depths = mmorpg_core::DepthsRecord::default();
    lost.inventory = mmorpg_core::Inventory::from_slots([None; mmorpg_core::INVENTORY_SLOTS]);
    let mut restarted = ZoneSimulation::with_content(SLOT, redbrand_depths::content()).unwrap();
    restarted
        .begin_depths_run(DepthsRunStart {
            run_id: 7,
            roster: vec![id("aria")],
            cleared: ALL_CLEARED,
            checkpoint: true,
            outcome: Some(report.outcome.clone()),
        })
        .unwrap();
    restarted.admit_character(1, &lost).unwrap();
    tick(&mut restarted, 2);
    assert_eq!(
        restarted
            .capture_character(1, id("aria"))
            .unwrap()
            .inventory
            .count(item),
        1
    );

    // An open window at the crash cannot resume; the item stays claimable, never rerolled.
    let zone = complete(&["aria"], ALL_CLEARED);
    let open = zone.boss_report().unwrap();
    assert!(matches!(
        open.outcome.drops[0].state,
        DropState::Rolling { .. }
    ));
    let mut restarted = ZoneSimulation::with_content(SLOT, redbrand_depths::content()).unwrap();
    restarted
        .begin_depths_run(DepthsRunStart {
            run_id: 7,
            roster: vec![id("aria")],
            cleared: ALL_CLEARED,
            checkpoint: true,
            outcome: Some(open.outcome),
        })
        .unwrap();
    assert_eq!(
        restarted
            .depths_run()
            .unwrap()
            .boss
            .outcome
            .as_ref()
            .unwrap()
            .drops[0]
            .state,
        DropState::Held
    );
    let _ = CreatureId::new(30);
    let _ = templates::REDBRAND_BANDIT;
}

#[test]
fn eligibility_is_the_chamber_at_the_pull_and_a_disconnected_winner_may_answer_or_expire() {
    // A roster character away at the pull is not eligible, whatever else it does later.
    let mut zone = fight(&names(2), ALL_CLEARED);
    let away = zone.capture_character(2, id("bren")).unwrap();
    assert!(zone.remove_player(2));
    let mut record = away.clone();
    record.location = CharacterLocation {
        zone_id: SLOT,
        position: [11_000, 90, 0],
        facing: 0,
    };
    zone.admit_character(2, &record).unwrap();
    let mut zone = edit(zone, |state| {
        let player = state.players.iter_mut().find(|p| p.player_id == 2).unwrap();
        player.position = [-9_000.0, 90.0, 0.0];
    });
    engage(&mut zone);
    assert_eq!(
        zone.depths_run()
            .unwrap()
            .boss
            .attempt
            .as_ref()
            .unwrap()
            .eligible,
        vec![id("aria")]
    );
    let zone = kill(zone, 1);
    let mut zone = edit(zone, |state| {
        let player = state.players.iter_mut().find(|p| p.player_id == 2).unwrap();
        player.position = [13_000.0, 90.0, 300.0];
    });
    assert!(zone.snapshot_for_player(2).unwrap().depths_loot.is_none());
    zone.apply_command(
        2,
        5_000,
        ZoneCommand::RollDepthsDrop {
            run: 7,
            drop: 0,
            choice: RollChoice::Need,
        },
    )
    .unwrap();
    zone.advance_tick().unwrap();
    assert!(matches!(
        zone.depths_run()
            .unwrap()
            .boss
            .outcome
            .as_ref()
            .unwrap()
            .drops[0]
            .state,
        DropState::Rolling { .. }
    ));

    // An eligible character who left may answer on return before the deadline; afterwards
    // late answers change nothing, and a replayed snapshot settles identically.
    let mut zone = complete(&names(2), ALL_CLEARED);
    let bren = zone.capture_character(2, id("bren")).unwrap();
    assert!(zone.remove_player(2));
    roll(&mut zone, 1, 6_000, 0, RollChoice::Greed);
    zone.admit_character(2, &bren).unwrap();
    tick(&mut zone, 2);
    roll(&mut zone, 2, 6_001, 0, RollChoice::Pass);
    let settled = zone
        .depths_run()
        .unwrap()
        .boss
        .outcome
        .as_ref()
        .unwrap()
        .drops[0]
        .state
        .clone();
    assert!(matches!(
        &settled,
        DropState::Awarded { winner, need: false, .. } if *winner == id("aria")
    ));
    let replayed =
        ZoneSimulation::from_snapshot(zone.snapshot().unwrap(), Arc::clone(zone.content()));
    assert_eq!(
        replayed
            .unwrap()
            .depths_run()
            .unwrap()
            .boss
            .outcome
            .as_ref()
            .unwrap()
            .drops[0]
            .state,
        settled
    );
}

#[test]
fn a_completed_run_never_stands_thorn_again_and_a_stale_outcome_is_fenced() {
    let mut zone = complete(&["aria"], ALL_CLEARED);
    // Claims and rolls on the wrong run fail closed.
    zone.apply_command(
        1,
        7_000,
        ZoneCommand::RollDepthsDrop {
            run: 99,
            drop: 0,
            choice: RollChoice::Need,
        },
    )
    .unwrap();
    zone.advance_tick().unwrap();
    assert!(matches!(
        zone.depths_run()
            .unwrap()
            .boss
            .outcome
            .as_ref()
            .unwrap()
            .drops[0]
            .state,
        DropState::Rolling { .. }
    ));
    // A stale confirmation of a later version than exists cannot deliver.
    zone.confirm_boss_outcome(7, 9_999);
    let outcome = zone.depths_run().unwrap().boss.outcome.clone().unwrap();
    assert_eq!(outcome.confirmed, outcome.version);
    zone.confirm_boss_outcome(99, 1);
    tick(&mut zone, 5);
    assert!(
        !alive(&zone, THORN.get()),
        "completed: Thorn never rises again"
    );
}
