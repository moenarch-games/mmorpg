#[path = "support/combat_workload.rs"]
mod combat_workload;
#[path = "../../mmorpg-core/tests/support/greyhaven_workload.rs"]
mod greyhaven_workload;

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
    // the players after every other unit, and snapshot v26 adds a debris count byte. Content
    // revision 17 (#297) adds the hill ogre: one more creature for AI and physics every tick,
    // and the spread fights' players stand by shifted spawns of the longer spawn table. Content
    // revision 18 (#56) adds the mill hoist's platform body to every step's world, and
    // revision 19 (#59) the mill's loading dock and its two ramps. Revision 20 (#305) adds the
    // Redbrand watch post's three pieces to every step's world, and snapshot v27 a piece count
    // byte to every projection. Creature navigation (#307) routes blocked chases around
    // walls, which moves the spread and crowded fights. Revision 21 (#58) stands two heavy crates
    // in every step's world, and the spread fights push and see them. Revision 22 (#276) gives
    // Garrick his telegraphed Redbrand Whirl, whose cast events the camp fight's players see.
    // Revision 23 (#274) deepens Stillwater Lake, so its creatures stop at the basin, and
    // snapshot v32 adds the viewer's two breath bytes to every projection; the basin's edge
    // keeps its flat cells in the heightfield, which moves the crowded fight a little.
    let ceilings: [[usize; 4]; FIXTURES.len()] = [
        [27_000, 442_800, 220_128, 4_231_728],
        [27_000, 439_920, 79_320, 1_335_287],
        [26_941, 448_501, 385_876, 6_368_958],
        [26_045, 459_125, 2_216_266, 24_492_501],
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
            assert_eq!(report.ai_evaluations, 75 * TICKS);
        }
        if fixture.crowded {
            // Whole 14-byte events and 21-byte records beside the fixed part
            // and the door section, a 93-byte self sheet, an 18-byte copper-only
            // corpse sheet, the projectile, debris and piece counts fill 1,074 of the 1,077
            // bytes: no further 21-byte record fits.
            assert_eq!(report.max_projection_bytes, 1_074);
            assert!(report.candidates_tested > 1_000_000);
        }
    }
}
