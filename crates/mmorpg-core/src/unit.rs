//! Unit rules shared by players and creatures: the class-agnostic player
//! baseline, the hit table, combat and regeneration timing. All integer.

use crate::rng::ZoneRng;

/// Highest level any unit can have.
pub const MAX_UNIT_LEVEL: u8 = 60;
/// New characters start here until progression lands.
pub const PLAYER_START_LEVEL: u8 = 1;
/// Player maximum health at level 1; each further level adds [`PLAYER_HEALTH_PER_LEVEL`].
pub const PLAYER_BASE_HEALTH: u32 = 50;
pub const PLAYER_HEALTH_PER_LEVEL: u32 = 15;
/// Player melee damage at level 1; each further level adds 1 to both ends.
pub const PLAYER_BASE_DAMAGE: [u16; 2] = [3, 6];
/// Ticks between two player swings (2 s).
pub const PLAYER_SWING_TICKS: u16 = crate::ticks_from_ms(2_000);
/// Player melee reach: XZ centre distance in units.
pub const PLAYER_REACH_UNITS: i32 = 250;
/// Creature melee reach before adding the target's half-width.
pub const CREATURE_REACH_UNITS: i32 = 200;
/// Hit table: this many percent of swings miss …
pub const MISS_PERCENT: u32 = 5;
/// … and this many percent of swings are critical hits for ×1.5 damage.
pub const CRITICAL_PERCENT: u32 = 5;
/// No swing deals more than this, so a critical hit fits `u16`.
pub const MAX_SWING_DAMAGE: u16 = 10_000;
/// Dealing or taking damage keeps a unit in combat for this many ticks (5 s).
pub const COMBAT_LINGER_TICKS: u16 = crate::ticks_from_ms(5_000);
/// Players regenerate after this many ticks out of combat (6 s) …
pub const REGEN_DELAY_TICKS: u16 = crate::ticks_from_ms(6_000);
/// … every this many ticks (1 s) …
pub const REGEN_INTERVAL_TICKS: u16 = crate::ticks_from_ms(1_000);
/// … this percentage of their maximum health, rounded up.
pub const REGEN_PERCENT: u32 = 3;
/// A creature corpse stays for 30 s without a physics body.
pub const CORPSE_TICKS: u32 = crate::long_ticks_from_ms(30_000);
/// A released spirit returns with half of its maximum health, rounded up.
pub const RELEASE_HEALTH_PERCENT: u32 = 50;
/// A player receives at most one out-of-range error per second.
pub const OUT_OF_RANGE_ERROR_INTERVAL_TICKS: u16 = crate::ticks_from_ms(1_000);

/// Player maximum health: 50 + 15 · (level − 1).
#[must_use]
pub const fn player_max_health(level: u8) -> u32 {
    PLAYER_BASE_HEALTH + PLAYER_HEALTH_PER_LEVEL * (level.saturating_sub(1) as u32)
}

/// Player melee damage range: 3–6, plus 1 per level above 1.
#[must_use]
pub const fn player_damage(level: u8) -> [u16; 2] {
    let bonus = level.saturating_sub(1) as u16;
    [PLAYER_BASE_DAMAGE[0] + bonus, PLAYER_BASE_DAMAGE[1] + bonus]
}

/// Health as a percentage, rounded up so a living unit never shows 0.
#[must_use]
pub const fn health_percent(health: u32, max_health: u32) -> u8 {
    if max_health == 0 || health == 0 {
        return 0;
    }
    let percent = (health as u64 * 100).div_ceil(max_health as u64);
    if percent > 100 { 100 } else { percent as u8 }
}

/// `percent` of `max_health`, rounded up.
#[must_use]
pub const fn percent_of(max_health: u32, percent: u32) -> u32 {
    ((max_health as u64 * percent as u64).div_ceil(100)) as u32
}

/// The outcome of one swing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Swing {
    Miss,
    Hit { amount: u16, critical: bool },
}

/// Rolls the hit table, then damage uniformly in `damage` (inclusive). A miss
/// draws once, a hit twice; critical hits deal ×1.5, rounded down.
pub(crate) fn roll_swing(rng: &mut ZoneRng, damage: [u16; 2]) -> Swing {
    let roll = rng.below(100);
    if roll < MISS_PERCENT {
        return Swing::Miss;
    }
    let base = rng.inclusive(u32::from(damage[0]), u32::from(damage[1]));
    let critical = roll < MISS_PERCENT + CRITICAL_PERCENT;
    let amount = if critical { base * 3 / 2 } else { base };
    Swing::Hit {
        // Content bounds damage by MAX_SWING_DAMAGE, so ×1.5 fits u16.
        amount: u16::try_from(amount).unwrap_or(u16::MAX),
        critical,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ZoneId;

    #[test]
    fn player_baseline_follows_the_contract() {
        assert_eq!(player_max_health(1), 50);
        assert_eq!(player_max_health(10), 185);
        assert_eq!(player_damage(1), [3, 6]);
        assert_eq!(player_damage(4), [6, 9]);
    }

    #[test]
    fn health_percent_rounds_up_for_the_living() {
        assert_eq!(health_percent(0, 50), 0);
        assert_eq!(health_percent(1, 420), 1);
        assert_eq!(health_percent(49, 50), 98);
        assert_eq!(health_percent(419, 420), 100);
        assert_eq!(health_percent(420, 420), 100);
        assert_eq!(health_percent(u32::MAX, u32::MAX), 100);
        assert_eq!(percent_of(50, 3), 2);
        assert_eq!(percent_of(50, 50), 25);
        assert_eq!(percent_of(185, 50), 93);
    }

    #[test]
    fn the_hit_table_is_five_percent_misses_and_crits() {
        let mut rng = ZoneRng::seeded(1, ZoneId::new(1));
        let (mut misses, mut crits, mut hits) = (0, 0, 0);
        for _ in 0..20_000 {
            match roll_swing(&mut rng, [3, 6]) {
                Swing::Miss => misses += 1,
                Swing::Hit {
                    amount,
                    critical: true,
                } => {
                    assert!((4..=9).contains(&amount));
                    crits += 1;
                }
                Swing::Hit {
                    amount,
                    critical: false,
                } => {
                    assert!((3..=6).contains(&amount));
                    hits += 1;
                }
            }
        }
        assert!((800..1_200).contains(&misses), "{misses}");
        assert!((800..1_200).contains(&crits), "{crits}");
        assert_eq!(misses + crits + hits, 20_000);
    }
}
