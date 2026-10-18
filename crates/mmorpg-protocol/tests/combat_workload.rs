#[path = "support/combat_workload.rs"]
mod combat_workload;

use combat_workload::{FIXTURES, TICKS, measure};
use mmorpg_protocol::MAX_PLAYER_PROJECTION_BYTES;

#[test]
fn greyhaven_combat_work_and_wire_budgets_are_ratcheted() {
    // Reviewed ceilings in fixture order: AI, physics body visits, projection
    // candidates and total encoded player-projection bytes. A decrease is welcome;
    // raising a ceiling requires a deliberate workload/behavior/performance review.
    // Rapier physics (#271) replaced the first-party engine's pair, TOI and contact
    // counters; these ceilings were recorded on the switch. Sliding around other
    // characters (#273) moved the spread 32-player fight slightly. The outer valley
    // (content revision 12, #159) adds 15 creatures and about 700 trunks, so every
    // step's Rapier world holds more bodies. Its hills (revision 13) moved the spread
    // 32-player fight's creatures slightly, and so did sliding around off-axis contacts.
    // The Greyfang cave (revision 14) adds eight rock boxes to every step's world. Snapshot v25
    // (#286) adds a projectile count byte to every projection. Content revision 15 (#288) stands 17
    // breakable props on fixed bodies in every step's world; projections show the props near
    // the players after every other unit, and snapshot v26 adds a debris count byte.
    let ceilings: [[usize; 4]; FIXTURES.len()] = [
        [26_640, 439_200, 220_128, 4_214_448],
        [26_581, 436_261, 78_385, 1_275_848],
        [26_640, 444_960, 358_196, 5_945_191],
        [25_745, 455_585, 2_162_794, 24_434_854],
    ];
    for (fixture, ceiling) in FIXTURES.into_iter().zip(ceilings) {
        let report = measure(fixture).unwrap();
        assert_eq!(
            report,
            measure(fixture).unwrap(),
            "{} cold replay",
            fixture.name
        );
        assert_eq!(report.physics_steps, TICKS);
        assert_eq!(report.projections, fixture.players * TICKS);
        assert_eq!(
            report.dynamic_body_visits,
            fixture.players * TICKS + report.ai_evaluations
        );
        assert_eq!(report.maintenance_inspections, report.dynamic_body_visits);
        assert!(report.max_projection_bytes <= MAX_PLAYER_PROJECTION_BYTES);
        let actual = [
            report.ai_evaluations,
            report.physics_body_visits,
            report.candidates_tested,
            report.projection_bytes,
        ];
        for (index, (actual, ceiling)) in actual.into_iter().zip(ceiling).enumerate() {
            assert!(
                actual <= ceiling,
                "{} counter {index}: {actual} exceeds {ceiling}",
                fixture.name
            );
        }
        if fixture.fighting {
            // A cheap or truncated script with no real combat cannot pass.
            assert!(report.damage_records >= fixture.players);
            assert!(report.death_records > 0);
        } else {
            assert_eq!(report.event_records, 0);
            assert_eq!(report.ai_evaluations, 74 * TICKS);
        }
        if fixture.crowded {
            // Whole 14-byte events and 21-byte records beside the fixed part
            // and the door section, a 93-byte self sheet, an 18-byte copper-only
            // corpse sheet and the projectile and debris counts fill all 1,077 bytes.
            assert_eq!(report.max_projection_bytes, 1_077);
            assert!(report.candidates_tested > 1_000_000);
        }
    }
}
