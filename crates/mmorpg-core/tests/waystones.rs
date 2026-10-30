//! Exploration-unlocked waystones (#145): discovery within reach, durable
//! unlocks, travel between stones of one zone and the refusals, plus the Seam
//! Test zone pair.
mod support;

use std::sync::Arc;

use mmorpg_core::{
    CharacterId, ClassChoice, ErrorCode, GREYHAVEN_OUTPOST, PlayerClass, SEAM_TEST_EAST,
    SEAM_TEST_WEST, Sex, WaystoneId, WaystoneSet, ZoneCommand, ZoneContent, ZoneEvent, ZoneId,
    ZoneSimulation, seam_test, waystone,
};
use support::arena;

const ZONE: ZoneId = ZoneId::new(5);

fn warden() -> ClassChoice {
    ClassChoice {
        class: PlayerClass::Warden,
        sex: Sex::Female,
    }
}

/// Flat test ground binding two network stones in one zone: Greyhaven Outpost's and Seam Test
/// West's, so travel between them stays inside the zone.
fn two_stone_content() -> Arc<ZoneContent> {
    Arc::new(
        ZoneContent::clone(&arena::arena(Vec::new(), Vec::new(), Vec::new()))
            .with_creature_abilities(Vec::new())
            .unwrap()
            .with_waystones(vec![SEAM_TEST_WEST, GREYHAVEN_OUTPOST])
            .unwrap(),
    )
}

/// `zone` with player 1's body centre at `(x, z)` and, optionally, its combat timer running.
fn placed(zone: &ZoneSimulation, x: f32, z: f32, combat_timer: u16) -> ZoneSimulation {
    let mut state = zone.snapshot().unwrap();
    let record = state
        .players
        .iter_mut()
        .find(|record| record.player_id == 1)
        .unwrap();
    record.position = [x, 90.0, z];
    record.combat.combat_timer = combat_timer;
    ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap()
}

fn stone_centre(id: WaystoneId) -> (f32, f32) {
    let site = waystone(id).unwrap();
    (site.stone[0] as f32, site.stone[1] as f32)
}

/// A warden standing 3 m from `at`'s stone.
fn beside(content: Arc<ZoneContent>, at: WaystoneId, combat_timer: u16) -> ZoneSimulation {
    let mut zone = ZoneSimulation::with_content(ZONE, content).unwrap();
    zone.create_character(1, warden()).unwrap();
    let (x, z) = stone_centre(at);
    placed(&zone, x, z - 300.0, combat_timer)
}

fn travel(zone: &mut ZoneSimulation, sequence: u32, to: WaystoneId) -> Vec<ErrorCode> {
    zone.apply_command(1, sequence, ZoneCommand::UseWaystone { waystone: to })
        .unwrap();
    zone.advance_tick().unwrap();
    zone.snapshot_for_player(1)
        .unwrap()
        .events
        .into_iter()
        .filter_map(|event| match event {
            ZoneEvent::Error { code, .. } => Some(code),
            _ => None,
        })
        .collect()
}

#[test]
fn reaching_a_stone_unlocks_it_durably() {
    let mut zone = ZoneSimulation::with_content(ZONE, two_stone_content()).unwrap();
    zone.create_character(1, warden()).unwrap();
    zone.advance_tick().unwrap();
    assert_eq!(zone.unlocked_waystones(1).unwrap(), WaystoneSet::EMPTY);

    let (x, z) = stone_centre(GREYHAVEN_OUTPOST);
    let mut zone = placed(&zone, x + 700.0, z, 0);
    zone.advance_tick().unwrap();
    let unlocked = zone.unlocked_waystones(1).unwrap();
    assert_eq!(unlocked.iter().collect::<Vec<_>>(), [GREYHAVEN_OUTPOST]);
    assert_eq!(
        zone.snapshot_for_player(1).unwrap().viewer.waystones,
        unlocked
    );

    let record = zone
        .capture_character(1, CharacterId::new("explorer").unwrap())
        .unwrap();
    assert_eq!(record.waystones, unlocked);
    let mut elsewhere = ZoneSimulation::with_content(ZoneId::new(6), seam_test::west()).unwrap();
    elsewhere.admit_character(7, &record).unwrap();
    assert_eq!(elsewhere.unlocked_waystones(7).unwrap(), unlocked);
}

#[test]
fn travel_moves_the_player_to_an_unlocked_stone_of_the_zone() {
    let mut zone = beside(two_stone_content(), SEAM_TEST_WEST, 0);
    zone.advance_tick().unwrap();
    // Greyhaven's stone is still locked.
    assert_eq!(
        travel(&mut zone, 1, GREYHAVEN_OUTPOST),
        [ErrorCode::InvalidWaystone]
    );

    let (x, z) = stone_centre(GREYHAVEN_OUTPOST);
    let mut zone = placed(&zone, x, z - 300.0, 0);
    zone.advance_tick().unwrap();
    assert_eq!(
        travel(&mut zone, 2, GREYHAVEN_OUTPOST),
        [ErrorCode::InvalidWaystone]
    );
    assert!(travel(&mut zone, 3, SEAM_TEST_WEST).is_empty());
    let arrival = waystone(SEAM_TEST_WEST).unwrap().arrival;
    let (position, velocity) = zone.player_motion(1).unwrap();
    assert!(
        (position[0] - arrival[0] as f32).abs() < 30.0,
        "{position:?}"
    );
    assert!(
        (position[2] - arrival[1] as f32).abs() < 30.0,
        "{position:?}"
    );
    assert!(velocity[0].abs() < 1.0 && velocity[2].abs() < 1.0);
}

#[test]
fn travel_is_refused_in_combat_away_from_stones_and_across_zones() {
    let content = two_stone_content();
    let (x, z) = stone_centre(GREYHAVEN_OUTPOST);

    // Both unlocked, then in combat beside one of them.
    let mut zone = beside(Arc::clone(&content), SEAM_TEST_WEST, 0);
    zone.advance_tick().unwrap();
    let mut zone = placed(&zone, x, z - 300.0, 0);
    zone.advance_tick().unwrap();
    let mut fighting = placed(&zone, x, z - 300.0, 100);
    assert_eq!(
        travel(&mut fighting, 1, SEAM_TEST_WEST),
        [ErrorCode::InCombat]
    );

    // Twenty metres from any stone.
    let mut away = placed(&zone, x + 2_000.0, z, 0);
    assert_eq!(
        travel(&mut away, 1, SEAM_TEST_WEST),
        [ErrorCode::InvalidWaystone]
    );

    // Seam Test East stands in another logical zone.
    let mut west = ZoneSimulation::with_content(seam_test::WEST_ZONE, seam_test::west()).unwrap();
    west.create_character(1, warden()).unwrap();
    let (x, z) = stone_centre(SEAM_TEST_WEST);
    let west = placed(&west, x, z + 300.0, 0);
    let mut record = west
        .capture_character(1, CharacterId::new("traveller").unwrap())
        .unwrap();
    record.waystones = WaystoneSet::from_bits(0b111).unwrap();
    let mut west = ZoneSimulation::with_content(seam_test::WEST_ZONE, seam_test::west()).unwrap();
    west.admit_character(1, &record).unwrap();
    west.advance_tick().unwrap();
    assert!(west.departures().is_empty());
    // A host that moves nobody between zones refuses the travel.
    assert_eq!(
        travel(&mut west, 1, SEAM_TEST_EAST),
        [ErrorCode::InvalidWaystone]
    );
    // Otherwise travel there departs through the fenced transfer: the zone
    // only marks the departure and the host freezes the traveller.
    west.enable_waystone_departures();
    assert_eq!(travel(&mut west, 2, SEAM_TEST_EAST), []);
    let departure = west.departure(1).expect("the traveller departs");
    let site = waystone(SEAM_TEST_EAST).unwrap();
    assert_eq!(departure.destination.zone_id, seam_test::EAST_ZONE);
    assert_eq!(
        [
            departure.destination.position[0],
            departure.destination.position[2]
        ],
        site.arrival
    );
    let (position, _) = west.player_motion(1).unwrap();
    assert!(
        (position[2] - (z + 300.0)).abs() < 30.0,
        "the traveller stays put until its host moves it"
    );
    assert!(west.cancel_departure(1));
    assert!(west.departures().is_empty());
    assert_eq!(travel(&mut west, 3, GREYHAVEN_OUTPOST), []);
    assert_eq!(
        west.departure(1).unwrap().destination.zone_id,
        mmorpg_core::GREYHAVEN_ZONE
    );
    west.freeze_for_transfer(
        1,
        CharacterId::new("traveller").unwrap(),
        mmorpg_core::TransferId::new(1).unwrap(),
        west.departure(1).unwrap().destination,
    )
    .unwrap();
    assert!(west.departures().is_empty(), "freezing takes the departure");
}

#[test]
fn seam_test_walkers_cross_once_into_the_neighbour() {
    let west = ZoneSimulation::with_content(seam_test::WEST_ZONE, seam_test::west()).unwrap();
    let mut zone = west;
    zone.create_character(1, warden()).unwrap();
    let zone = placed(&zone, 5_200.0, 0.0, 0);
    let crossings = zone.seam_crossings();
    assert_eq!(crossings.len(), 1);
    let destination = crossings[0].destination;
    assert_eq!(destination.zone_id, seam_test::EAST_ZONE);
    assert_eq!(destination.position[0], 5_200 - seam_test::SHIFT);

    let mut east = ZoneSimulation::with_content(seam_test::EAST_ZONE, seam_test::east()).unwrap();
    east.create_character(1, warden()).unwrap();
    let east = placed(&east, destination.position[0] as f32, 0.0, 0);
    assert!(
        east.seam_crossings().is_empty(),
        "arrivals are not handed straight back"
    );
    assert!(
        east.handoff_corridor_at(destination.position[0], 0)
            .is_some()
    );
    assert!(Arc::ptr_eq(
        &seam_test::content_for_zone(seam_test::EAST_ZONE),
        &seam_test::east()
    ));
}

#[test]
fn every_bound_stone_arrival_is_clear_of_colliders() {
    let half = mmorpg_core::PLAYER_HALF_EXTENTS_UNITS;
    for content in [
        mmorpg_core::greyhaven_vale::content(),
        seam_test::west(),
        seam_test::east(),
    ] {
        assert_eq!(content.waystones().len(), 1);
        for id in content.waystones() {
            let site = waystone(*id).unwrap();
            let [x, z] = site.arrival;
            let y = content.definition().ground_at(x, z) + half[1];
            for collider in content.definition().colliders() {
                let overlaps = (0..3).all(|axis| {
                    let centre = [x, y, z][axis];
                    (centre - collider.position[axis]).abs()
                        < half[axis] + collider.half_extents[axis]
                });
                assert!(
                    !overlaps,
                    "{} arrival overlaps collider {}",
                    site.name, collider.id
                );
            }
        }
    }
}

#[test]
fn greyhaven_quest_logs_cross_into_the_seam_test_zones_unchanged() {
    let quest = mmorpg_core::greyhaven_vale::quests::quests()
        .into_iter()
        .find(|quest| quest.prerequisite.is_none())
        .unwrap();
    let mut west = ZoneSimulation::with_content(seam_test::WEST_ZONE, seam_test::west()).unwrap();
    west.create_character(1, warden()).unwrap();
    let mut record = west
        .capture_character(1, CharacterId::new("quester").unwrap())
        .unwrap();
    record.quests.entries = vec![mmorpg_core::QuestEntry {
        quest: quest.id,
        progress: [0; mmorpg_core::MAX_QUEST_OBJECTIVES],
    }];
    let mut east = ZoneSimulation::with_content(seam_test::EAST_ZONE, seam_test::east()).unwrap();
    east.admit_character(1, &record).unwrap();
    let carried = east
        .capture_character(1, CharacterId::new("quester").unwrap())
        .unwrap();
    assert_eq!(carried.quests, record.quests, "the log crosses unchanged");

    // Content that carries no other zone's quests still holds no such log.
    let mut plain = ZoneSimulation::with_content(ZONE, two_stone_content()).unwrap();
    assert!(plain.admit_character(1, &record).is_err());
}
