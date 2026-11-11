//! Physique V1 (#633): the zone applies the bounded physique modifier to the authoritative
//! maximum health and melee damage, cosmetic sliders change nothing, and customization recomputes
//! from the current values without stacking.

use std::sync::Arc;

use mmorpg_core::{
    CharacterId, ClassChoice, PHYSIQUE_PARAMS, PHYSIQUE_SCALE, Physique, PhysiqueParam,
    PhysiquePreset, PlayerClass, Sex, ZoneId, ZoneSimulation, greyhaven_vale,
};

const PLAYER: u32 = 7;

/// A level-10 Warden at the level cap: large enough health for a 2 % change to show.
fn veteran() -> ZoneSimulation {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), greyhaven_vale::content()).unwrap();
    zone.create_character(
        PLAYER,
        ClassChoice {
            class: PlayerClass::Warden,
            sex: Sex::Male,
        },
    )
    .unwrap();
    let mut state = zone.snapshot().unwrap();
    state.players[0].combat.level = 10;
    state.players[0].combat.experience = 0;
    state.players[0].combat.health = 185;
    ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap()
}

/// Maximum health and the melee range the zone uses.
fn stats(zone: &ZoneSimulation) -> (u32, [u16; 2]) {
    let viewer = zone.project_for_player(PLAYER).unwrap().snapshot.viewer;
    (viewer.max_health, viewer.damage)
}

fn with_stat_sliders(value: u16) -> Physique {
    let mut physique = Physique::NEUTRAL;
    for param in PhysiqueParam::ALL.into_iter().filter(|param| param.affects_stats()) {
        physique = physique.with(param, value).unwrap();
    }
    physique
}

#[test]
fn neutral_physique_keeps_the_original_numbers_and_extremes_stay_within_two_percent() {
    let mut zone = veteran();
    let (health, damage) = stats(&zone);
    assert_eq!(health, 185);

    zone.customize_physique(PLAYER, with_stat_sliders(PHYSIQUE_SCALE))
        .unwrap();
    let (high_health, high_damage) = stats(&zone);
    assert_eq!(high_health, 189, "+2 %, rounded");
    // 12–15 moves by less than half a point at 2 %: rounded, it stays.
    assert_eq!(high_damage, damage);
    // Health followed the raised maximum.
    assert_eq!(zone.snapshot().unwrap().players[0].combat.health, 189);

    zone.customize_physique(PLAYER, with_stat_sliders(0)).unwrap();
    let (low_health, _) = stats(&zone);
    assert_eq!(low_health, 181, "-2 %, rounded");

    // Back to neutral: exactly the original numbers, nothing accumulated.
    zone.customize_physique(PLAYER, Physique::NEUTRAL).unwrap();
    assert_eq!(stats(&zone), (health, damage));
    assert_eq!(zone.snapshot().unwrap().players[0].combat.health, 185);
}

#[test]
fn cosmetic_sliders_at_their_limits_leave_authoritative_stats_unchanged() {
    for preset in PhysiquePreset::ALL {
        let mut zone = veteran();
        zone.customize_physique(PLAYER, preset.physique()).unwrap();
        let expected = stats(&zone);
        for corner in 0..1u32 << (PHYSIQUE_PARAMS - 5) {
            let mut physique = preset.physique();
            for (bit, param) in PhysiqueParam::ALL[5..].iter().enumerate() {
                let value = if corner >> bit & 1 == 1 {
                    PHYSIQUE_SCALE
                } else {
                    0
                };
                physique = physique.with(*param, value).unwrap();
            }
            zone.customize_physique(PLAYER, physique).unwrap();
            assert_eq!(stats(&zone), expected, "{preset:?} {corner:b}");
        }
    }
}

#[test]
fn presets_round_trip_to_average_and_through_snapshots_and_records() {
    let mut zone = veteran();
    let original = stats(&zone);
    for preset in PhysiquePreset::ALL {
        zone.customize_physique(PLAYER, preset.physique()).unwrap();
        assert_eq!(zone.player_physique(PLAYER), Some(preset.physique()));
        // Snapshot recovery and character capture keep the sliders and the derived stats.
        let restored =
            ZoneSimulation::from_snapshot(zone.snapshot().unwrap(), Arc::clone(zone.content()))
                .unwrap();
        assert_eq!(stats(&restored), stats(&zone));
        let record = zone
            .capture_character(PLAYER, CharacterId::new("physique").unwrap())
            .unwrap();
        assert_eq!(record.physique, preset.physique());
        let mut admitted =
            ZoneSimulation::with_content(ZoneId::new(1), greyhaven_vale::content()).unwrap();
        admitted.admit_character(PLAYER, &record).unwrap();
        assert_eq!(stats(&admitted), stats(&zone));
    }
    zone.customize_physique(PLAYER, PhysiquePreset::Average.physique())
        .unwrap();
    assert_eq!(stats(&zone), original);
}

#[test]
fn a_stored_record_is_customized_with_the_same_rule() {
    let mut zone = veteran();
    let mut record = zone
        .capture_character(PLAYER, CharacterId::new("offline").unwrap())
        .unwrap();
    record.customize_physique(PhysiquePreset::Heavyset.physique());
    zone.customize_physique(PLAYER, PhysiquePreset::Heavyset.physique())
        .unwrap();
    assert_eq!(
        record,
        zone.capture_character(PLAYER, CharacterId::new("offline").unwrap())
            .unwrap()
    );
}
