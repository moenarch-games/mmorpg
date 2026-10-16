//! The immutable ability catalog: every class and creature ability with a
//! global `u8` ID. Zone content binds the catalog to its identity (see
//! [`crate::ZoneContent::with_creature_abilities`]), so a recovered zone
//! always runs the abilities it was checkpointed with. Rules that use the
//! catalog live in `casting`; aura bookkeeping in `aura`.

use crate::class::PlayerClass;
use crate::unit::PLAYER_REACH_UNITS;

/// Revision of [`ABILITY_CATALOG`]; content identity hashes both.
pub const ABILITY_CATALOG_REVISION: u64 = 1;
/// Every player ability starts this global cooldown; haste never shortens it.
pub const GLOBAL_COOLDOWN_TICKS: u16 = 45;
/// At most this many auras rest on one unit.
pub const MAX_AURAS: usize = 8;
/// At most this many cooldowns run on one player (its class abilities).
pub const MAX_COOLDOWNS: usize = 4;
/// Zone-RNG jitter added whenever a creature's ability timer starts.
pub const CREATURE_ABILITY_JITTER_TICKS: u16 = 30;

/// A global ability ID; 0 is never an ability.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AbilityId(u8);

impl AbilityId {
    #[must_use]
    pub const fn new(value: u8) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

/// Who may use an ability: a class from its unlock level, or a creature
/// whose template content binds it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AbilityUser {
    Class(PlayerClass),
    Creature,
}

/// Instant, a cast that resolves when complete, or a channel that pulses
/// while it runs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CastTime {
    Instant,
    Cast(u16),
    Channel(u16),
}

impl CastTime {
    /// Ticks the cast or channel takes; 0 for instants.
    #[must_use]
    pub const fn ticks(self) -> u16 {
        match self {
            Self::Instant => 0,
            Self::Cast(ticks) | Self::Channel(ticks) => ticks,
        }
    }

    #[must_use]
    pub const fn is_channel(self) -> bool {
        matches!(self, Self::Channel(_))
    }
}

/// How far the target may be: the caster itself, melee reach, or an XZ
/// centre distance in units.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AbilityRange {
    SelfCentred,
    Melee,
    Units(i32),
}

impl AbilityRange {
    /// The maximum XZ centre distance to the target, if the ability has one.
    #[must_use]
    pub const fn reach(self) -> Option<i32> {
        match self {
            Self::SelfCentred => None,
            Self::Melee => Some(PLAYER_REACH_UNITS),
            Self::Units(units) => Some(units),
        }
    }
}

/// What an aura does; the wire value is 1 + its discriminant.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum AuraKind {
    DamageOverTime,
    HealOverTime,
    Absorb,
    Root,
    Snare,
    Stun,
    Haste,
}

impl AuraKind {
    pub const ALL: [Self; 7] = [
        Self::DamageOverTime,
        Self::HealOverTime,
        Self::Absorb,
        Self::Root,
        Self::Snare,
        Self::Stun,
        Self::Haste,
    ];

    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::DamageOverTime => 1,
            Self::HealOverTime => 2,
            Self::Absorb => 3,
            Self::Root => 4,
            Self::Snare => 5,
            Self::Stun => 6,
            Self::Haste => 7,
        }
    }

    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::DamageOverTime),
            2 => Some(Self::HealOverTime),
            3 => Some(Self::Absorb),
            4 => Some(Self::Root),
            5 => Some(Self::Snare),
            6 => Some(Self::Stun),
            7 => Some(Self::Haste),
            _ => None,
        }
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::DamageOverTime => "damage_over_time",
            Self::HealOverTime => "heal_over_time",
            Self::Absorb => "absorb",
            Self::Root => "root",
            Self::Snare => "snare",
            Self::Stun => "stun",
            Self::Haste => "haste",
        }
    }
}

/// The aura an ability applies: kind, duration and pulse period (0 when
/// it does not pulse).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuraSpec {
    pub kind: AuraKind,
    pub duration: u16,
    pub period: u16,
}

/// What an ability does when it resolves. "Per level" amounts scale with
/// the caster's levels above 1, like the player damage curve.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AbilityEffect {
    /// Weapon damage × `numerator` / `denominator` + `bonus` to the target.
    WeaponStrike {
        numerator: u16,
        denominator: u16,
        bonus: u16,
    },
    /// Interrupts the target's cast, locking that ability for
    /// `lockout_ticks`, and stuns it.
    Bash { stun_ticks: u16, lockout_ticks: u16 },
    /// Heals `percent` of the caster's maximum health over the aura.
    HealOverTime {
        percent: u16,
        duration: u16,
        period: u16,
    },
    /// Weapon damage to the target and up to `extra` more living creatures
    /// within `radius` of it, in `EntityRef` order.
    Cleave { extra: u8, radius: i32 },
    /// `base` + `per_level` damage spread over the aura's pulses.
    DamageOverTime {
        base: u16,
        per_level: u16,
        duration: u16,
        period: u16,
    },
    /// Slows the target's movement by `percent`.
    Snare { percent: u16, duration: u16 },
    /// Speeds the caster's swings by `percent`.
    Haste { percent: u16, duration: u16 },
    /// Direct damage in `damage` (inclusive) + `per_level`.
    Bolt { damage: [u16; 2], per_level: u16 },
    /// Damage to every living creature within `radius` of the caster, then a
    /// root that damage breaks once `unbreakable_ticks` have passed.
    Nova {
        damage: [u16; 2],
        radius: i32,
        root_ticks: u16,
        unbreakable_ticks: u16,
    },
    /// A shield absorbing `base` + `per_level` damage.
    Absorb {
        base: u16,
        per_level: u16,
        duration: u16,
    },
    /// Every `period` ticks of the channel, damage to every living creature
    /// within `radius` of the target point fixed at the start.
    Blizzard {
        damage: [u16; 2],
        per_level: u16,
        radius: i32,
        period: u16,
    },
    /// Heals `percent` of maximum health; usable only below `below_percent`.
    Bandage { percent: u16, below_percent: u16 },
}

/// One catalog row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Ability {
    pub id: AbilityId,
    pub name: &'static str,
    pub user: AbilityUser,
    /// Unlock level for classes; creatures ignore it.
    pub level: u8,
    pub cost: u16,
    pub cast: CastTime,
    pub cooldown: u16,
    pub range: AbilityRange,
    pub effect: AbilityEffect,
}

impl Ability {
    /// The aura this ability applies, if any.
    #[must_use]
    pub const fn aura(&self) -> Option<AuraSpec> {
        let (kind, duration, period) = match self.effect {
            AbilityEffect::Bash { stun_ticks, .. } => (AuraKind::Stun, stun_ticks, 0),
            AbilityEffect::HealOverTime {
                duration, period, ..
            } => (AuraKind::HealOverTime, duration, period),
            AbilityEffect::DamageOverTime {
                duration, period, ..
            } => (AuraKind::DamageOverTime, duration, period),
            AbilityEffect::Snare { duration, .. } => (AuraKind::Snare, duration, 0),
            AbilityEffect::Haste { duration, .. } => (AuraKind::Haste, duration, 0),
            AbilityEffect::Nova { root_ticks, .. } => (AuraKind::Root, root_ticks, 0),
            AbilityEffect::Absorb { duration, .. } => (AuraKind::Absorb, duration, 0),
            AbilityEffect::WeaponStrike { .. }
            | AbilityEffect::Cleave { .. }
            | AbilityEffect::Bolt { .. }
            | AbilityEffect::Blizzard { .. }
            | AbilityEffect::Bandage { .. } => return None,
        };
        Some(AuraSpec {
            kind,
            duration,
            period,
        })
    }

    /// Whether its aura rests on the caster (heal over time, haste, absorb)
    /// rather than on the creatures it hits.
    #[must_use]
    pub const fn aura_on_caster(&self) -> bool {
        matches!(
            self.effect,
            AbilityEffect::HealOverTime { .. }
                | AbilityEffect::Haste { .. }
                | AbilityEffect::Absorb { .. }
        )
    }

    /// Whether the ability needs a hostile unit target (otherwise it is
    /// centred on the caster and ignores any target).
    #[must_use]
    pub const fn needs_target(&self) -> bool {
        !matches!(self.range, AbilityRange::SelfCentred)
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "one catalog row per call keeps the table readable"
)]
const fn ability(
    id: u8,
    name: &'static str,
    user: AbilityUser,
    level: u8,
    cost: u16,
    cast: CastTime,
    cooldown: u16,
    range: AbilityRange,
    effect: AbilityEffect,
) -> Ability {
    Ability {
        id: AbilityId(id),
        name,
        user,
        level,
        cost,
        cast,
        cooldown,
        range,
        effect,
    }
}

const WARDEN: AbilityUser = AbilityUser::Class(PlayerClass::Warden);
const RANGER: AbilityUser = AbilityUser::Class(PlayerClass::Ranger);
const ARCANIST: AbilityUser = AbilityUser::Class(PlayerClass::Arcanist);
const RANGED: AbilityRange = AbilityRange::Units(3_000);

pub mod ids {
    use super::AbilityId;

    pub const HEROIC_STRIKE: AbilityId = AbilityId::new(1);
    pub const SHIELD_BASH: AbilityId = AbilityId::new(2);
    pub const RALLYING_CRY: AbilityId = AbilityId::new(3);
    pub const CLEAVE: AbilityId = AbilityId::new(4);
    pub const AIMED_SHOT: AbilityId = AbilityId::new(5);
    pub const SERPENT_STING: AbilityId = AbilityId::new(6);
    pub const CONCUSSIVE_SHOT: AbilityId = AbilityId::new(7);
    pub const RAPID_FIRE: AbilityId = AbilityId::new(8);
    pub const FIREBOLT: AbilityId = AbilityId::new(9);
    pub const FROST_NOVA: AbilityId = AbilityId::new(10);
    pub const ARCANE_BARRIER: AbilityId = AbilityId::new(11);
    pub const BLIZZARD: AbilityId = AbilityId::new(12);
    pub const MUCK_BOLT: AbilityId = AbilityId::new(13);
    pub const CRUDE_BANDAGE: AbilityId = AbilityId::new(14);
}

/// Every ability, ordered by ID; row `i` has ID `i + 1`.
pub const ABILITY_CATALOG: [Ability; 14] = [
    ability(
        1,
        "Heroic Strike",
        WARDEN,
        1,
        15,
        CastTime::Instant,
        0,
        AbilityRange::Melee,
        AbilityEffect::WeaponStrike {
            numerator: 3,
            denominator: 2,
            bonus: 6,
        },
    ),
    ability(
        2,
        "Shield Bash",
        WARDEN,
        2,
        10,
        CastTime::Instant,
        360,
        AbilityRange::Melee,
        AbilityEffect::Bash {
            stun_ticks: 60,
            lockout_ticks: 120,
        },
    ),
    ability(
        3,
        "Rallying Cry",
        WARDEN,
        4,
        20,
        CastTime::Instant,
        900,
        AbilityRange::SelfCentred,
        AbilityEffect::HealOverTime {
            percent: 30,
            duration: 300,
            period: 30,
        },
    ),
    ability(
        4,
        "Cleave",
        WARDEN,
        6,
        20,
        CastTime::Instant,
        180,
        AbilityRange::Melee,
        AbilityEffect::Cleave {
            extra: 2,
            radius: 300,
        },
    ),
    ability(
        5,
        "Aimed Shot",
        RANGER,
        1,
        30,
        CastTime::Instant,
        180,
        RANGED,
        AbilityEffect::WeaponStrike {
            numerator: 2,
            denominator: 1,
            bonus: 8,
        },
    ),
    ability(
        6,
        "Serpent Sting",
        RANGER,
        2,
        20,
        CastTime::Instant,
        0,
        RANGED,
        AbilityEffect::DamageOverTime {
            base: 30,
            per_level: 4,
            duration: 450,
            period: 90,
        },
    ),
    ability(
        7,
        "Concussive Shot",
        RANGER,
        4,
        15,
        CastTime::Instant,
        360,
        RANGED,
        AbilityEffect::Snare {
            percent: 50,
            duration: 120,
        },
    ),
    ability(
        8,
        "Rapid Fire",
        RANGER,
        6,
        20,
        CastTime::Instant,
        900,
        AbilityRange::SelfCentred,
        AbilityEffect::Haste {
            percent: 40,
            duration: 300,
        },
    ),
    ability(
        9,
        "Firebolt",
        ARCANIST,
        1,
        25,
        CastTime::Cast(60),
        0,
        RANGED,
        AbilityEffect::Bolt {
            damage: [10, 14],
            per_level: 3,
        },
    ),
    ability(
        10,
        "Frost Nova",
        ARCANIST,
        2,
        30,
        CastTime::Instant,
        600,
        AbilityRange::SelfCentred,
        AbilityEffect::Nova {
            damage: [3, 5],
            radius: 800,
            root_ticks: 180,
            unbreakable_ticks: 30,
        },
    ),
    ability(
        11,
        "Arcane Barrier",
        ARCANIST,
        4,
        40,
        CastTime::Instant,
        900,
        AbilityRange::SelfCentred,
        AbilityEffect::Absorb {
            base: 20,
            per_level: 8,
            duration: 600,
        },
    ),
    ability(
        12,
        "Blizzard",
        ARCANIST,
        6,
        80,
        CastTime::Channel(180),
        0,
        RANGED,
        AbilityEffect::Blizzard {
            damage: [4, 6],
            per_level: 1,
            radius: 600,
            period: 30,
        },
    ),
    ability(
        13,
        "Muck Bolt",
        AbilityUser::Creature,
        1,
        0,
        CastTime::Cast(45),
        240,
        AbilityRange::Units(2_000),
        AbilityEffect::Bolt {
            damage: [5, 8],
            per_level: 0,
        },
    ),
    ability(
        14,
        "Crude Bandage",
        AbilityUser::Creature,
        1,
        0,
        CastTime::Cast(90),
        600,
        AbilityRange::SelfCentred,
        AbilityEffect::Bandage {
            percent: 20,
            below_percent: 50,
        },
    ),
];

/// The catalog row of `id`, if any.
#[must_use]
pub fn ability_by_id(id: AbilityId) -> Option<&'static Ability> {
    ABILITY_CATALOG.get(usize::from(id.get().checked_sub(1)?))
}

/// The class ability `id` that `class` has learned at `level`.
pub(crate) fn learned(class: PlayerClass, level: u8, id: AbilityId) -> Option<&'static Ability> {
    ability_by_id(id)
        .filter(|ability| ability.user == AbilityUser::Class(class) && ability.level <= level)
}

/// `value` + `per_level` × (level − 1), saturating.
pub(crate) const fn scaled(value: u16, per_level: u16, level: u8) -> u16 {
    value.saturating_add(per_level.saturating_mul(level.saturating_sub(1) as u16))
}

/// A canonical byte encoding of the whole catalog for content identity.
pub(crate) fn catalog_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&ABILITY_CATALOG_REVISION.to_be_bytes());
    for ability in &ABILITY_CATALOG {
        bytes.push(ability.id.get());
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
            } => vec![2, stun_ticks.into(), lockout_ticks.into()],
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
                radius,
                root_ticks,
                unbreakable_ticks,
            } => vec![
                9,
                damage[0].into(),
                damage[1].into(),
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
        };
        bytes.extend_from_slice(&u32::try_from(fields.len()).unwrap_or(0).to_be_bytes());
        for field in fields {
            bytes.extend_from_slice(&field.to_be_bytes());
        }
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalog_is_ordered_bounded_and_consistent() {
        for (index, ability) in ABILITY_CATALOG.iter().enumerate() {
            assert_eq!(usize::from(ability.id.get()), index + 1, "{}", ability.name);
            assert_eq!(ability_by_id(ability.id), Some(ability));
            if let Some(aura) = ability.aura() {
                assert!(aura.duration > 0);
                assert!(aura.period == 0 || aura.duration % aura.period == 0);
            }
            if let CastTime::Channel(ticks) = ability.cast {
                let AbilityEffect::Blizzard { period, .. } = ability.effect else {
                    panic!("only Blizzard channels");
                };
                assert_eq!(ticks % period, 0);
            }
        }
        assert_eq!(ability_by_id(AbilityId::new(0)), None);
        assert_eq!(ability_by_id(AbilityId::new(15)), None);
        // Each class has at most MAX_COOLDOWNS abilities with a cooldown.
        for class in PlayerClass::ALL {
            let cooldowns = ABILITY_CATALOG
                .iter()
                .filter(|ability| ability.user == AbilityUser::Class(class) && ability.cooldown > 0)
                .count();
            assert!(cooldowns <= MAX_COOLDOWNS);
        }
    }

    #[test]
    fn learning_follows_class_and_level() {
        assert!(learned(PlayerClass::Warden, 1, ids::HEROIC_STRIKE).is_some());
        assert!(learned(PlayerClass::Warden, 1, ids::SHIELD_BASH).is_none());
        assert!(learned(PlayerClass::Warden, 2, ids::SHIELD_BASH).is_some());
        assert!(learned(PlayerClass::Ranger, 10, ids::HEROIC_STRIKE).is_none());
        assert!(learned(PlayerClass::Arcanist, 10, ids::MUCK_BOLT).is_none());
        assert!(learned(PlayerClass::Arcanist, 10, AbilityId::new(99)).is_none());
        assert_eq!(scaled(30, 4, 1), 30);
        assert_eq!(scaled(30, 4, 3), 38);
    }
}
