//! The ability catalogue's canonical byte encoding for content identity (#1231).
use super::*;

/// A canonical byte encoding of the whole catalog for content identity.
pub(crate) fn catalog_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    for ability in &ABILITY_CATALOG {
        bytes.extend_from_slice(&ability.id.get().to_be_bytes());
        bytes.extend_from_slice(&u32::try_from(ability.name.len()).unwrap_or(0).to_be_bytes());
        bytes.extend_from_slice(ability.name.as_bytes());
        bytes.push(match ability.user {
            AbilityUser::Class(class) => class.code(),
            AbilityUser::Creature => 0xff,
        });
        bytes.push(ability.level);
        bytes.extend_from_slice(&ability.cost.to_be_bytes());
        let (cast, ticks) = match ability.cast {
            CastTime::Instant => (0, 0),
            CastTime::Cast(ticks) => (1, ticks),
            CastTime::Channel(ticks) => (2, ticks),
        };
        bytes.push(cast);
        bytes.extend_from_slice(&ticks.to_be_bytes());
        bytes.extend_from_slice(&ability.cooldown.to_be_bytes());
        let range = match ability.range {
            AbilityRange::SelfCentred => -1,
            AbilityRange::Melee => 0,
            AbilityRange::Units(units) => units,
            // Friendly reach is encoded below every hostile one (#303).
            AbilityRange::Friendly(units) => i32::MIN + units,
        };
        bytes.extend_from_slice(&range.to_be_bytes());
        let fields: Vec<i32> = match ability.effect {
            AbilityEffect::WeaponStrike {
                numerator,
                denominator,
                bonus,
            } => vec![1, numerator.into(), denominator.into(), bonus.into()],
            AbilityEffect::Bash {
                stun_ticks,
                lockout_ticks,
                knockback,
            } => vec![2, stun_ticks.into(), lockout_ticks.into(), knockback],
            AbilityEffect::HealOverTime {
                percent,
                duration,
                period,
            } => vec![3, percent.into(), duration.into(), period.into()],
            AbilityEffect::Cleave { extra, radius } => vec![4, extra.into(), radius],
            AbilityEffect::DamageOverTime {
                base,
                per_level,
                duration,
                period,
            } => vec![
                5,
                base.into(),
                per_level.into(),
                duration.into(),
                period.into(),
            ],
            AbilityEffect::Snare { percent, duration } => vec![6, percent.into(), duration.into()],
            AbilityEffect::Haste { percent, duration } => vec![7, percent.into(), duration.into()],
            AbilityEffect::Bolt { damage, per_level } => {
                vec![8, damage[0].into(), damage[1].into(), per_level.into()]
            }
            AbilityEffect::Nova {
                damage,
                per_level,
                radius,
                root_ticks,
                unbreakable_ticks,
            } => vec![
                9,
                damage[0].into(),
                damage[1].into(),
                per_level.into(),
                radius,
                root_ticks.into(),
                unbreakable_ticks.into(),
            ],
            AbilityEffect::Absorb {
                base,
                per_level,
                duration,
            } => vec![10, base.into(), per_level.into(), duration.into()],
            AbilityEffect::Blizzard {
                damage,
                per_level,
                radius,
                period,
            } => vec![
                11,
                damage[0].into(),
                damage[1].into(),
                per_level.into(),
                radius,
                period.into(),
            ],
            AbilityEffect::Bandage {
                percent,
                below_percent,
            } => vec![12, percent.into(), below_percent.into()],
            AbilityEffect::Blast {
                damage,
                per_level,
                radius,
                knockback,
            } => vec![
                13,
                damage[0].into(),
                damage[1].into(),
                per_level.into(),
                radius,
                knockback,
            ],
            AbilityEffect::Charge { knockback } => vec![14, knockback],
            AbilityEffect::Stomp {
                damage,
                per_level,
                radius,
                knockback,
            } => vec![
                15,
                damage[0].into(),
                damage[1].into(),
                per_level.into(),
                radius,
                knockback,
            ],
            AbilityEffect::Smash {
                damage,
                per_level,
                range,
                half_angle,
                knockback,
            } => vec![
                19,
                damage[0].into(),
                damage[1].into(),
                per_level.into(),
                range,
                half_angle.into(),
                knockback,
            ],
            AbilityEffect::Cone {
                damage,
                per_level,
                range,
                half_angle,
                percent,
                duration,
            } => vec![
                16,
                damage[0].into(),
                damage[1].into(),
                per_level.into(),
                range,
                half_angle.into(),
                percent.into(),
                duration.into(),
            ],
            AbilityEffect::Beam {
                damage,
                per_level,
                length,
                period,
            } => vec![
                17,
                damage[0].into(),
                damage[1].into(),
                per_level.into(),
                length,
                period.into(),
            ],
            AbilityEffect::Chain {
                damage,
                per_level,
                jumps,
                radius,
            } => vec![
                18,
                damage[0].into(),
                damage[1].into(),
                per_level.into(),
                jumps.into(),
                radius,
            ],
            AbilityEffect::Drain {
                damage,
                per_level,
                period,
            } => vec![
                20,
                damage[0].into(),
                damage[1].into(),
                per_level.into(),
                period.into(),
            ],
            AbilityEffect::Root {
                root_ticks,
                unbreakable_ticks,
            } => vec![21, root_ticks.into(), unbreakable_ticks.into()],
            AbilityEffect::Splash {
                base,
                per_level,
                radius,
                duration,
                period,
            } => vec![
                22,
                base.into(),
                per_level.into(),
                radius,
                duration.into(),
                period.into(),
            ],
            AbilityEffect::Summon {
                health,
                health_per_level,
                damage,
                damage_per_level,
                swing_ticks,
                duration,
            } => vec![
                23,
                health.into(),
                health_per_level.into(),
                damage[0].into(),
                damage[1].into(),
                damage_per_level.into(),
                swing_ticks.into(),
                duration.into(),
            ],
            AbilityEffect::Companion {
                health,
                health_per_level,
                damage,
                damage_per_level,
                swing_ticks,
            } => vec![
                24,
                health.into(),
                health_per_level.into(),
                damage[0].into(),
                damage[1].into(),
                damage_per_level.into(),
                swing_ticks.into(),
            ],
            AbilityEffect::SummonTerrain { kind } => vec![25, i32::from(kind.code())],
            AbilityEffect::Leap {
                distance,
                damage,
                per_level,
                radius,
                knockback,
            } => vec![
                26,
                distance,
                damage[0].into(),
                damage[1].into(),
                per_level.into(),
                radius,
                knockback,
            ],
            AbilityEffect::Hook { damage, per_level } => {
                vec![27, damage[0].into(), damage[1].into(), per_level.into()]
            }
            AbilityEffect::Blink { distance } => vec![28, distance],
            AbilityEffect::RaiseDead {
                health,
                health_per_level,
                damage,
                damage_per_level,
                swing_ticks,
                duration,
            } => vec![
                29,
                health.into(),
                health_per_level.into(),
                damage[0].into(),
                damage[1].into(),
                damage_per_level.into(),
                swing_ticks.into(),
                duration.into(),
            ],
            AbilityEffect::CorpseBurst {
                damage,
                per_level,
                radius,
            } => vec![
                30,
                damage[0].into(),
                damage[1].into(),
                per_level.into(),
                radius,
            ],
            AbilityEffect::Reckoning { percent, pool } => vec![31, percent.into(), pool.into()],
            AbilityEffect::SignalCall => vec![32],
            AbilityEffect::Shapeshift { speed, duration } => {
                vec![33, speed.into(), duration.into()]
            }
        };
        bytes.extend_from_slice(&u32::try_from(fields.len()).unwrap_or(0).to_be_bytes());
        for field in fields {
            bytes.extend_from_slice(&field.to_be_bytes());
        }
        bytes.push(ability.projectile.map_or(0, crate::ProjectileKind::code));
    }
    bytes
}
