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
    // counters; these ceilings were recorded on the switch.
    let ceilings: [[usize; 4]; FIXTURES.len()] = [
        [21_240, 132_480, 190_080, 3_598_128],
        [21_240, 129_600, 70_005, 1_203_308],
        [21_240, 138_240, 367_057, 6_232_461],
        [20_285, 148_805, 1_926_294, 24_392_554],
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
            assert_eq!(report.ai_evaluations, 59 * TICKS);
        }
        if fixture.crowded {
            // Whole 14-byte events and 21-byte records beside the fixed part
            // and the door section, a 93-byte self sheet and an 18-byte copper-only
            // corpse sheet fill all 1,077 bytes.
            assert_eq!(report.max_projection_bytes, 1_077);
            assert!(report.candidates_tested > 1_000_000);
        }
    }
}
