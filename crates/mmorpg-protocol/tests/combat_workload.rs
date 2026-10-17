#[path = "support/combat_workload.rs"]
mod combat_workload;

use combat_workload::{FIXTURES, TICKS, measure};
use mmorpg_protocol::MAX_PLAYER_PROJECTION_BYTES;

#[test]
fn greyhaven_combat_work_and_wire_budgets_are_ratcheted() {
    // Reviewed v1 ceilings in fixture order: AI, admitted physics bodies,
    // broad-phase pairs, TOI tests, contact resolutions, projection candidates,
    // and total encoded player-projection bytes. A decrease is welcome; raising
    // a ceiling requires a deliberate workload/behavior/performance review.
    // Content revision 6 raised the spread fights' pair checks, contacts and
    // projection candidates: lurkers stop to cast Muck Bolt and bandits to
    // cast Crude Bandage, which moves bodies differently. Snapshot v15 raised
    // the spread 32-player fight's bytes by 35,693 for the 14-byte
    // AttackPerformed/DamageObserved records visible bystanders now receive.
    // Content revision 10 replaced the inn block with the walk-in inn's 32 wall
    // and furniture bodies and its door (+32 bodies × 360 ticks of visits) and
    // raised the broad-phase pairs, and candidates slightly, near the inn (#158).
    let ceilings: [[usize; 7]; FIXTURES.len()] = [
        [21_240, 132_480, 174_870, 4, 27_222, 190_080, 3_204_720],
        [21_240, 129_600, 262_455, 20, 24_540, 70_367, 981_271],
        [21_240, 138_240, 377_262, 41, 32_010, 367_679, 5_442_236],
        [20_106, 148_626, 469_181, 68, 39_928, 1_938_628, 24_499_402],
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
        assert!(report.staged_bodies <= ceiling[1]);
        assert!(report.max_projection_bytes <= MAX_PLAYER_PROJECTION_BYTES);
        let actual = [
            report.ai_evaluations,
            report.physics_body_visits,
            report.pair_checks,
            report.toi_tests,
            report.contact_resolutions,
            report.candidates_tested,
            report.projection_bytes,
        ];
        for (index, (actual, ceiling)) in actual.into_iter().zip(ceiling).enumerate() {
            // v6 adds eight XP bytes per projection. v7 adds nine bag metadata
            // bytes per projection and 64 bag bytes every ten ticks. These
            // fixtures never move bags. v8 adds five repeated copper/presence bytes
            // and at most 21 bytes per measured complete corpse sheet. v9
            // adds 23 fixed class/resource/cast/list-count bytes per
            // projection; these fixtures never choose a class, so no
            // cooldown or aura records follow. v10 adds the four-byte melee
            // damage range per projection and 20 equipment and stat bytes per
            // self sheet. v12 adds the one-byte chat count per projection;
            // these fixtures never chat. v14 adds a nine-byte quest part
            // per self sheet (mask, counts and the Marshal's `!` marker;
            // these fixtures never accept a quest) and the one-byte quest
            // item presence per corpse sheet. v18 adds the four-byte viewer
            // guard per projection. v19 adds the one-byte melee stance and
            // v20 the one-byte combo phase per projection. v22 adds the
            // door count and the inn door's two-byte record per projection.
            let ceiling = if index == 6 {
                ceiling
                    + (22 + 23 + 4 + 1 + 4 + 1 + 1 + 3) * fixture.players * TICKS
                    + (64 + 20 + 9) * fixture.players * (TICKS / 10)
                    + 22 * report.loot_sheets
            } else {
                ceiling
            };
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
