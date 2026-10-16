use mmorpg_core::{MAX_PLAYER_LEVEL, experience_to_next_level, kill_experience};

#[test]
fn starter_curve_uses_current_level_xp_and_an_explicit_cap() {
    let expected = [100, 200, 300, 400, 500, 600, 700, 800, 900, 0];
    for (level, threshold) in (1..=10).zip(expected) {
        assert_eq!(experience_to_next_level(level), Some(threshold));
    }
    assert_eq!(MAX_PLAYER_LEVEL, 10);
    assert_eq!(expected.iter().sum::<u32>(), 4_500);
    for level in [0, 11, 60, u8::MAX] {
        assert_eq!(experience_to_next_level(level), None);
    }
}

#[test]
fn fixed_rewards_cover_equal_lower_gray_higher_and_bonus_cap_levels() {
    // Independently tabulated design examples, including integer rounding.
    for (player, creature, xp) in [
        (1, 1, 50),
        (2, 2, 55),
        (5, 5, 70),
        (5, 4, 52),
        (5, 3, 36),
        (5, 2, 22),
        (5, 1, 10),
        (6, 1, 0),
        (9, 1, 0),
        (5, 6, 82),
        (5, 7, 96),
        (5, 8, 110),
        (5, 9, 126),
        (5, 10, 142),
        (5, 11, 150),
        (1, 60, 517),
        (10, 1, 0),
        (10, 10, 0),
        (10, 60, 0),
    ] {
        assert_eq!(
            kill_experience(player, creature),
            Some(xp),
            "{player}/{creature}"
        );
    }
    assert_eq!(
        kill_experience(1, 1).unwrap() * 2,
        experience_to_next_level(1).unwrap()
    );
}

#[test]
fn invalid_levels_never_receive_a_fallback_reward() {
    for (player, creature) in [
        (0, 1),
        (11, 1),
        (60, 1),
        (u8::MAX, 1),
        (1, 0),
        (1, 61),
        (1, u8::MAX),
        (10, 0),
    ] {
        assert_eq!(kill_experience(player, creature), None);
    }
    for player in 1..=MAX_PLAYER_LEVEL {
        for creature in 1..=60 {
            assert!(kill_experience(player, creature).unwrap() <= 517);
        }
    }
}
