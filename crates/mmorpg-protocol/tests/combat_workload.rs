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
    // Revision 24 (#274) adds the Sunken Shrine's 28 boxes to every step's world, and its
    // lurkers swim, which shifts the zone RNG and so the fights' creatures. Revision 25 (#160)
    // garrisons the Greyfang cave with six more creatures for AI and physics every tick and
    // adds its sealed threshold box, which shifts the zone RNG and the fights again. Revision 26
    // (#431) rebuilds the shrine on the asset-tooling kit with 15 more boxes in every step.
    // Revision 28 (#423) lifts the lake onto the basin terrace, which shifts the zone RNG and
    // keeps the spread and crowded fights' creatures thinking and testing a little more. The
    // 60 Hz zone (#379) covers half the time in the same ticks; its creatures run on subunit
    // steering, which moves the crowded fights a little. Farmland and Hamlets (#424) adds its
    // buildings, fences and trees to every step's world, and `i32` positions grow every
    // entity record by six bytes. The Bandit Highlands (#425) add their stronghold, crags and
    // pines to every step's world.
    let ceilings: [[usize; 4]; FIXTURES.len()] = [
        [29_160, 692_640, 224_640, 5_202_864],
        [29_160, 689_760, 80_140, 1_431_435],
        [29_160, 698_400, 389_620, 7_150_844],
        [29_041, 709_801, 2_248_200, 24_496_050],
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
            assert_eq!(report.ai_evaluations, 81 * TICKS);
        }
        if fixture.crowded {
            // Whole 14-byte events and 27-byte records beside the fixed part
            // and the door section, a 93-byte self sheet, an 18-byte copper-only
            // corpse sheet, the projectile, debris and piece counts fill the 1,077 bytes
            // (#424: `i32` positions): no further 27-byte record fits.
            assert!(report.max_projection_bytes + 27 > MAX_PLAYER_PROJECTION_BYTES);
            assert!(report.candidates_tested > 1_000_000);
        }
    }
}
