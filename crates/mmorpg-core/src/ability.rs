//! The immutable ability catalog: every class and creature ability with a
//! global `u8` ID. Zone content binds the catalog to its identity (see
//! [`crate::ZoneContent::with_creature_abilities`]), so a recovered zone
//! always runs the abilities it was checkpointed with. Rules that use the
//! catalog live in `casting`; aura bookkeeping in `aura`.

use crate::class::PlayerClass;
use crate::ticks_from_ms;
use crate::unit::PLAYER_REACH_UNITS;

/// Revision of [`ABILITY_CATALOG`]; content identity hashes both.
pub const ABILITY_CATALOG_REVISION: u64 = 13;
/// Every player ability starts this global cooldown; haste never shortens it.
pub const GLOBAL_COOLDOWN_TICKS: u16 = ticks_from_ms(1_500);
/// At most this many auras rest on one unit.
pub const MAX_AURAS: usize = 8;
/// At most this many cooldowns run on one player: one per action-bar slot (its class abilities).
pub const MAX_COOLDOWNS: usize = 8;
/// Zone-RNG jitter added whenever a creature's ability timer starts.
pub const CREATURE_ABILITY_JITTER_TICKS: u16 = ticks_from_ms(1_000);

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
    /// A supportive ability (#303): it lands on the selected friendly (non-hostile, living)
    /// player within this XZ centre distance and line of sight, or on the caster when no other
    /// player is selected.
    Friendly(i32),
}

impl AbilityRange {
    /// The maximum XZ centre distance to the target, if the ability has one.
    #[must_use]
    pub const fn reach(self) -> Option<i32> {
        match self {
            Self::SelfCentred => None,
            Self::Melee => Some(PLAYER_REACH_UNITS),
            Self::Units(units) | Self::Friendly(units) => Some(units),
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
    /// `lockout_ticks`, stuns it and knocks it `knockback` units away (#287).
    Bash {
        stun_ticks: u16,
        lockout_ticks: u16,
        knockback: i32,
    },
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
    /// An explosion at the target (#287): `damage` + `per_level` and a `knockback` (units) away
    /// from the centre for every living creature within `radius` that the centre can see. Both
    /// fall off linearly to half at the edge; walls between centre and creature shield it.
    Blast {
        damage: [u16; 2],
        per_level: u16,
        radius: i32,
        knockback: i32,
    },
    /// The caster rushes at the target (#287) and knocks it `knockback` units back on arrival.
    Charge { knockback: i32 },
    /// A large creature's stomp (#297), centred on the caster: `damage` + `per_level` and a
    /// `knockback` (units) away from it for every living player within `radius` that it can
    /// see, both falling off linearly to half at the edge; walls shield.
    Stomp {
        damage: [u16; 2],
        per_level: u16,
        radius: i32,
        knockback: i32,
    },
    /// A creature's overhead smash (#160): `damage` + `per_level` to every living player it can
    /// see within `range` and `half_angle` degrees of its facing, each knocked `knockback`
    /// units away from it. The caster keeps its facing through the wind-up.
    Smash {
        damage: [u16; 2],
        per_level: u16,
        range: i32,
        half_angle: u16,
        knockback: i32,
    },
    /// A breath in front of the caster (#292): `damage` + `per_level` to every hostile unit it
    /// can see within `range` and `half_angle` degrees of its facing, then a snare of `percent`.
    Cone {
        damage: [u16; 2],
        per_level: u16,
        range: i32,
        half_angle: u16,
        percent: u16,
        duration: u16,
    },
    /// A channelled beam along the caster's current facing (#292): every `period` ticks,
    /// `damage` + `per_level` to every hostile unit on the line up to `length`, which the first
    /// wall or a raised shield turned toward it stops.
    Beam {
        damage: [u16; 2],
        per_level: u16,
        length: i32,
        period: u16,
    },
    /// Lightning at the target that jumps `jumps` times to the nearest unhit hostile unit within
    /// `radius` that the last victim can see (#292), at full `damage` + `per_level` each.
    Chain {
        damage: [u16; 2],
        per_level: u16,
        jumps: u8,
        radius: i32,
    },
    /// A channelled drain on the target (#301): every `period` ticks, `damage` + `per_level` to
    /// it, and the caster heals by the health it lost. The channel breaks when the target dies,
    /// leaves line of sight or moves out of range.
    Drain {
        damage: [u16; 2],
        per_level: u16,
        period: u16,
    },
    /// Roots the target for `root_ticks` (#304); damage breaks it once `unbreakable_ticks` have
    /// passed, like a Frost Nova root.
    Root {
        root_ticks: u16,
        unbreakable_ticks: u16,
    },
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
    /// The projectile that carries the effect to the target (#286); `None` lands at once.
    pub projectile: Option<crate::ProjectileKind>,
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
            AbilityEffect::Snare { duration, .. } | AbilityEffect::Cone { duration, .. } => {
                (AuraKind::Snare, duration, 0)
            }
            AbilityEffect::Haste { duration, .. } => (AuraKind::Haste, duration, 0),
            AbilityEffect::Nova { root_ticks, .. } | AbilityEffect::Root { root_ticks, .. } => {
                (AuraKind::Root, root_ticks, 0)
            }
            AbilityEffect::Absorb { duration, .. } => (AuraKind::Absorb, duration, 0),
            AbilityEffect::WeaponStrike { .. }
            | AbilityEffect::Cleave { .. }
            | AbilityEffect::Bolt { .. }
            | AbilityEffect::Blizzard { .. }
            | AbilityEffect::Bandage { .. }
            | AbilityEffect::Blast { .. }
            | AbilityEffect::Charge { .. }
            | AbilityEffect::Stomp { .. }
            | AbilityEffect::Smash { .. }
            | AbilityEffect::Beam { .. }
            | AbilityEffect::Chain { .. }
            | AbilityEffect::Drain { .. } => return None,
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

    /// Whether the ability may also fly freely along the facing without a target: Aimed Shot
    /// (#286), Firebolt (#289) and the aimed specials.
    #[must_use]
    pub const fn aims_freely(&self) -> bool {
        self.id.0 == ids::AIMED_SHOT.0
            || self.id.0 == ids::FIREBOLT.0
            || !matches!(self.shot(), Shot::Plain)
    }

    /// How its projectile treats what it meets (#291). Aimed specials ignore any selected target
    /// and always fly along the facing.
    #[must_use]
    pub const fn shot(&self) -> Shot {
        match self.id.0 {
            19 => Shot::Pierce,
            20 => Shot::Ricochet,
            _ => Shot::Plain,
        }
    }

    /// Whether the ability needs a hostile unit target (otherwise it is
    /// centred on the caster or lands on a friendly player or the caster).
    #[must_use]
    pub const fn needs_target(&self) -> bool {
        !matches!(
            self.range,
            AbilityRange::SelfCentred | AbilityRange::Friendly(_)
        )
    }

    /// Whether it lands on a selected friendly player or the caster (#303).
    #[must_use]
    pub const fn is_friendly(&self) -> bool {
        matches!(self.range, AbilityRange::Friendly(_))
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
        projectile: None,
    }
}

/// `ability` carried by a `kind` projectile.
const fn shot(ability: Ability, kind: crate::ProjectileKind) -> Ability {
    Ability {
        projectile: Some(kind),
        ..ability
    }
}

const WARDEN: AbilityUser = AbilityUser::Class(PlayerClass::Warden);
const RANGER: AbilityUser = AbilityUser::Class(PlayerClass::Ranger);
const ARCANIST: AbilityUser = AbilityUser::Class(PlayerClass::Arcanist);
const NECROMANCER: AbilityUser = AbilityUser::Class(PlayerClass::Necromancer);
const ROGUE: AbilityUser = AbilityUser::Class(PlayerClass::Rogue);
const CLERIC: AbilityUser = AbilityUser::Class(PlayerClass::Cleric);
const DRUID: AbilityUser = AbilityUser::Class(PlayerClass::Druid);
/// Heals and shields reach a friendly player as far as ranged spells reach a hostile one.
const FRIENDLY: AbilityRange = AbilityRange::Friendly(3_000);
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
    pub const CROSSBOW_BOLT: AbilityId = AbilityId::new(15);
    pub const FIRE_BLAST: AbilityId = AbilityId::new(16);
    pub const CHARGE: AbilityId = AbilityId::new(17);
    pub const STOMP: AbilityId = AbilityId::new(18);
    pub const PIERCING_SHOT: AbilityId = AbilityId::new(19);
    pub const RICOCHET_SHOT: AbilityId = AbilityId::new(20);
    pub const FROST_BREATH: AbilityId = AbilityId::new(21);
    pub const LIGHTNING_BEAM: AbilityId = AbilityId::new(22);
    pub const CHAIN_LIGHTNING: AbilityId = AbilityId::new(23);
    pub const REDBRAND_WHIRL: AbilityId = AbilityId::new(24);
    /// The Redbrand Brute's slow, telegraphed overhead smash (#160).
    pub const OVERHEAD_SMASH: AbilityId = AbilityId::new(25);
    pub const SHADOW_BOLT: AbilityId = AbilityId::new(26);
    pub const CORRUPTION: AbilityId = AbilityId::new(27);
    pub const DRAIN_LIFE: AbilityId = AbilityId::new(28);
    pub const BONE_SHIELD: AbilityId = AbilityId::new(29);
    pub const SINISTER_STRIKE: AbilityId = AbilityId::new(30);
    pub const POISONED_BLADE: AbilityId = AbilityId::new(31);
    pub const GOUGE: AbilityId = AbilityId::new(32);
    pub const ADRENALINE_RUSH: AbilityId = AbilityId::new(33);
    pub const SMITE: AbilityId = AbilityId::new(34);
    pub const RENEW: AbilityId = AbilityId::new(35);
    pub const HOLY_BEAM: AbilityId = AbilityId::new(36);
    pub const DIVINE_SHIELD: AbilityId = AbilityId::new(37);
    pub const WRATH: AbilityId = AbilityId::new(38);
    pub const ENTANGLING_ROOTS: AbilityId = AbilityId::new(39);
    pub const REJUVENATION: AbilityId = AbilityId::new(40);
    pub const BARKSKIN: AbilityId = AbilityId::new(41);
}

/// How a projectile treats what it meets (#291).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Shot {
    /// Stops at the first thing it meets.
    Plain,
    /// Passes through up to [`crate::PIERCE_HITS`] enemies in a line, each later hit weaker;
    /// walls and raised shields stop it.
    Pierce,
    /// Mirrors off walls, props and terrain up to [`crate::RICOCHET_BOUNCES`] times and stops
    /// at the first unit.
    Ricochet,
}

/// Every ability, ordered by ID; row `i` has ID `i + 1`.
pub const ABILITY_CATALOG: [Ability; 41] = [
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
        ticks_from_ms(12_000),
        AbilityRange::Melee,
        AbilityEffect::Bash {
            stun_ticks: ticks_from_ms(2_000),
            lockout_ticks: ticks_from_ms(4_000),
            knockback: 300,
        },
    ),
    ability(
        3,
        "Rallying Cry",
        WARDEN,
        4,
        20,
        CastTime::Instant,
        ticks_from_ms(30_000),
        AbilityRange::SelfCentred,
        AbilityEffect::HealOverTime {
            percent: 30,
            duration: ticks_from_ms(10_000),
            period: ticks_from_ms(1_000),
        },
    ),
    ability(
        4,
        "Cleave",
        WARDEN,
        6,
        20,
        CastTime::Instant,
        ticks_from_ms(6_000),
        AbilityRange::Melee,
        AbilityEffect::Cleave {
            extra: 2,
            radius: 300,
        },
    ),
    shot(
        ability(
            5,
            "Aimed Shot",
            RANGER,
            1,
            30,
            CastTime::Instant,
            ticks_from_ms(6_000),
            RANGED,
            AbilityEffect::WeaponStrike {
                numerator: 2,
                denominator: 1,
                bonus: 8,
            },
        ),
        crate::ProjectileKind::Arrow,
    ),
    shot(
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
                duration: ticks_from_ms(15_000),
                period: ticks_from_ms(3_000),
            },
        ),
        crate::ProjectileKind::Arrow,
    ),
    shot(
        ability(
            7,
            "Concussive Shot",
            RANGER,
            4,
            15,
            CastTime::Instant,
            ticks_from_ms(12_000),
            RANGED,
            AbilityEffect::Snare {
                percent: 50,
                duration: ticks_from_ms(4_000),
            },
        ),
        crate::ProjectileKind::Arrow,
    ),
    ability(
        8,
        "Rapid Fire",
        RANGER,
        6,
        20,
        CastTime::Instant,
        ticks_from_ms(30_000),
        AbilityRange::SelfCentred,
        AbilityEffect::Haste {
            percent: 40,
            duration: ticks_from_ms(10_000),
        },
    ),
    // A magic missile (#289): it homes on its target, or flies straight along the facing.
    shot(
        ability(
            9,
            "Firebolt",
            ARCANIST,
            1,
            25,
            CastTime::Cast(ticks_from_ms(2_000)),
            0,
            RANGED,
            AbilityEffect::Bolt {
                damage: [10, 14],
                per_level: 3,
            },
        ),
        crate::ProjectileKind::Missile,
    ),
    ability(
        10,
        "Frost Nova",
        ARCANIST,
        2,
        30,
        CastTime::Instant,
        ticks_from_ms(20_000),
        AbilityRange::SelfCentred,
        AbilityEffect::Nova {
            damage: [3, 5],
            radius: 800,
            root_ticks: ticks_from_ms(6_000),
            unbreakable_ticks: ticks_from_ms(1_000),
        },
    ),
    ability(
        11,
        "Arcane Barrier",
        ARCANIST,
        4,
        40,
        CastTime::Instant,
        ticks_from_ms(30_000),
        AbilityRange::SelfCentred,
        AbilityEffect::Absorb {
            base: 20,
            per_level: 8,
            duration: ticks_from_ms(20_000),
        },
    ),
    ability(
        12,
        "Blizzard",
        ARCANIST,
        6,
        80,
        CastTime::Channel(ticks_from_ms(6_000)),
        0,
        RANGED,
        AbilityEffect::Blizzard {
            damage: [4, 6],
            per_level: 1,
            radius: 600,
            period: ticks_from_ms(1_000),
        },
    ),
    ability(
        13,
        "Muck Bolt",
        AbilityUser::Creature,
        1,
        0,
        CastTime::Cast(ticks_from_ms(1_500)),
        ticks_from_ms(8_000),
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
        CastTime::Cast(ticks_from_ms(3_000)),
        ticks_from_ms(20_000),
        AbilityRange::SelfCentred,
        AbilityEffect::Bandage {
            percent: 20,
            below_percent: 50,
        },
    ),
    shot(
        ability(
            15,
            "Crossbow Bolt",
            AbilityUser::Creature,
            1,
            0,
            CastTime::Cast(ticks_from_ms(1_000)),
            ticks_from_ms(8_000),
            AbilityRange::Units(2_000),
            AbilityEffect::Bolt {
                damage: [5, 8],
                per_level: 0,
            },
        ),
        crate::ProjectileKind::Bolt,
    ),
    ability(
        16,
        "Fire Blast",
        ARCANIST,
        3,
        45,
        CastTime::Cast(ticks_from_ms(1_500)),
        ticks_from_ms(10_000),
        RANGED,
        AbilityEffect::Blast {
            damage: [12, 16],
            per_level: 3,
            radius: 500,
            knockback: 600,
        },
    ),
    ability(
        17,
        "Charge",
        WARDEN,
        3,
        0,
        CastTime::Instant,
        ticks_from_ms(15_000),
        // A charge covers CHARGE_MAX_TICKS × CHARGE_SPEED_UNITS plus melee reach.
        AbilityRange::Units(1_500),
        AbilityEffect::Charge { knockback: 300 },
    ),
    // A one-second wind-up gives players time to step out (#297).
    ability(
        18,
        "Stomp",
        AbilityUser::Creature,
        1,
        0,
        CastTime::Cast(ticks_from_ms(1_000)),
        ticks_from_ms(12_000),
        AbilityRange::SelfCentred,
        AbilityEffect::Stomp {
            damage: [8, 12],
            per_level: 2,
            radius: 350,
            knockback: 300,
        },
    ),
    // Aimed specials (#291): always loosed along the facing, they fly physically and can miss.
    shot(
        ability(
            19,
            "Piercing Shot",
            RANGER,
            3,
            35,
            CastTime::Instant,
            ticks_from_ms(10_000),
            RANGED,
            AbilityEffect::WeaponStrike {
                numerator: 2,
                denominator: 1,
                bonus: 10,
            },
        ),
        crate::ProjectileKind::Arrow,
    ),
    shot(
        ability(
            20,
            "Ricochet Shot",
            RANGER,
            5,
            30,
            CastTime::Instant,
            ticks_from_ms(8_000),
            RANGED,
            AbilityEffect::WeaponStrike {
                numerator: 3,
                denominator: 2,
                bonus: 8,
            },
        ),
        crate::ProjectileKind::Arrow,
    ),
    // Cones, beams and chains (#292).
    ability(
        21,
        "Frost Breath",
        ARCANIST,
        5,
        35,
        CastTime::Instant,
        ticks_from_ms(10_000),
        AbilityRange::SelfCentred,
        AbilityEffect::Cone {
            damage: [6, 9],
            per_level: 2,
            range: 800,
            half_angle: 30,
            percent: 40,
            duration: ticks_from_ms(3_000),
        },
    ),
    ability(
        22,
        "Lightning Beam",
        ARCANIST,
        8,
        50,
        CastTime::Channel(ticks_from_ms(2_000)),
        ticks_from_ms(20_000),
        AbilityRange::SelfCentred,
        AbilityEffect::Beam {
            damage: [4, 6],
            per_level: 1,
            length: 2_000,
            period: ticks_from_ms(500),
        },
    ),
    ability(
        23,
        "Chain Lightning",
        ARCANIST,
        7,
        45,
        CastTime::Cast(ticks_from_ms(1_500)),
        ticks_from_ms(12_000),
        RANGED,
        AbilityEffect::Chain {
            damage: [10, 14],
            per_level: 3,
            jumps: 2,
            radius: 800,
        },
    ),
    // Garrick Redbrand's one telegraphed special (#276, Atlas 5.2.14): a 1.5 s wind-up, then a
    // spinning sweep around him; stepping out of its ring avoids it.
    ability(
        24,
        "Redbrand Whirl",
        AbilityUser::Creature,
        1,
        0,
        CastTime::Cast(ticks_from_ms(1_500)),
        ticks_from_ms(15_000),
        AbilityRange::SelfCentred,
        AbilityEffect::Stomp {
            damage: [16, 22],
            per_level: 0,
            radius: 400,
            knockback: 200,
        },
    ),
    // The Redbrand Brute's overhead smash (#160): a 2 s wind-up with its facing locked, then a
    // heavy blow in a 3.5 m arc ahead of it that knocks its victims back 2 m. Stepping aside,
    // behind it or out of reach avoids it.
    ability(
        25,
        "Overhead Smash",
        AbilityUser::Creature,
        1,
        0,
        CastTime::Cast(ticks_from_ms(2_000)),
        ticks_from_ms(15_000),
        AbilityRange::Melee,
        AbilityEffect::Smash {
            damage: [16, 22],
            per_level: 0,
            range: 350,
            half_angle: 50,
            knockback: 200,
        },
    ),
    // The Necromancer (#301): a shadow missile that tracks its target, a curse, a life drain
    // and a bone ward, in line with the Arcanist's mana costs. Summons follow with #295.
    shot(
        ability(
            26,
            "Shadow Bolt",
            NECROMANCER,
            1,
            25,
            CastTime::Cast(ticks_from_ms(2_000)),
            0,
            RANGED,
            AbilityEffect::Bolt {
                damage: [10, 14],
                per_level: 3,
            },
        ),
        crate::ProjectileKind::Shadow,
    ),
    shot(
        ability(
            27,
            "Corruption",
            NECROMANCER,
            2,
            30,
            CastTime::Instant,
            0,
            RANGED,
            AbilityEffect::DamageOverTime {
                base: 30,
                per_level: 4,
                duration: ticks_from_ms(15_000),
                period: ticks_from_ms(3_000),
            },
        ),
        crate::ProjectileKind::Shadow,
    ),
    ability(
        28,
        "Drain Life",
        NECROMANCER,
        4,
        45,
        CastTime::Channel(ticks_from_ms(3_000)),
        ticks_from_ms(10_000),
        RANGED,
        AbilityEffect::Drain {
            damage: [5, 7],
            per_level: 1,
            period: ticks_from_ms(1_000),
        },
    ),
    ability(
        29,
        "Bone Shield",
        NECROMANCER,
        6,
        40,
        CastTime::Instant,
        ticks_from_ms(30_000),
        AbilityRange::SelfCentred,
        AbilityEffect::Absorb {
            base: 20,
            per_level: 8,
            duration: ticks_from_ms(20_000),
        },
    ),
    // The Rogue (#302): a dagger strike, a poisoned blade, a stunning gouge and a burst of
    // speed, on focus like the Ranger. Thrown weapons, traps and dashes follow in #290, #300
    // and #294.
    ability(
        30,
        "Sinister Strike",
        ROGUE,
        1,
        25,
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
        31,
        "Poisoned Blade",
        ROGUE,
        2,
        20,
        CastTime::Instant,
        0,
        AbilityRange::Melee,
        AbilityEffect::DamageOverTime {
            base: 30,
            per_level: 4,
            duration: ticks_from_ms(15_000),
            period: ticks_from_ms(3_000),
        },
    ),
    ability(
        32,
        "Gouge",
        ROGUE,
        4,
        15,
        CastTime::Instant,
        ticks_from_ms(12_000),
        AbilityRange::Melee,
        AbilityEffect::Bash {
            stun_ticks: ticks_from_ms(2_000),
            lockout_ticks: ticks_from_ms(4_000),
            knockback: 0,
        },
    ),
    ability(
        33,
        "Adrenaline Rush",
        ROGUE,
        6,
        20,
        CastTime::Instant,
        ticks_from_ms(30_000),
        AbilityRange::SelfCentred,
        AbilityEffect::Haste {
            percent: 40,
            duration: ticks_from_ms(10_000),
        },
    ),
    // The Cleric (#303) on faith: Smite costs nothing and builds faith, so a fresh Cleric can
    // always act; Renew and Divine Shield land on a selected friendly player or the caster.
    // Numbers follow the Arcanist and Necromancer. Knockback blasts follow later.
    shot(
        ability(
            34,
            "Smite",
            CLERIC,
            1,
            0,
            CastTime::Cast(ticks_from_ms(2_000)),
            0,
            RANGED,
            AbilityEffect::Bolt {
                damage: [10, 14],
                per_level: 3,
            },
        ),
        crate::ProjectileKind::Holy,
    ),
    ability(
        35,
        "Renew",
        CLERIC,
        2,
        25,
        CastTime::Instant,
        0,
        FRIENDLY,
        AbilityEffect::HealOverTime {
            percent: 25,
            duration: ticks_from_ms(15_000),
            period: ticks_from_ms(3_000),
        },
    ),
    ability(
        36,
        "Holy Beam",
        CLERIC,
        4,
        40,
        CastTime::Channel(ticks_from_ms(2_000)),
        ticks_from_ms(20_000),
        AbilityRange::SelfCentred,
        AbilityEffect::Beam {
            damage: [4, 6],
            per_level: 1,
            length: 2_000,
            period: ticks_from_ms(500),
        },
    ),
    ability(
        37,
        "Divine Shield",
        CLERIC,
        6,
        35,
        CastTime::Instant,
        ticks_from_ms(20_000),
        FRIENDLY,
        AbilityEffect::Absorb {
            base: 20,
            per_level: 8,
            duration: ticks_from_ms(20_000),
        },
    ),
    // The Druid (#304) on mana like the Arcanist: a nature missile, a single-target root,
    // a heal over time on a selected friendly player or the caster, and a bark shield. The pet
    // companion follows in #296 and stone walls in #293.
    shot(
        ability(
            38,
            "Wrath",
            DRUID,
            1,
            25,
            CastTime::Cast(ticks_from_ms(2_000)),
            0,
            RANGED,
            AbilityEffect::Bolt {
                damage: [10, 14],
                per_level: 3,
            },
        ),
        crate::ProjectileKind::Nature,
    ),
    ability(
        39,
        "Entangling Roots",
        DRUID,
        2,
        30,
        CastTime::Instant,
        ticks_from_ms(20_000),
        RANGED,
        AbilityEffect::Root {
            root_ticks: ticks_from_ms(6_000),
            unbreakable_ticks: ticks_from_ms(1_000),
        },
    ),
    ability(
        40,
        "Rejuvenation",
        DRUID,
        4,
        35,
        CastTime::Instant,
        0,
        FRIENDLY,
        AbilityEffect::HealOverTime {
            percent: 25,
            duration: ticks_from_ms(15_000),
            period: ticks_from_ms(3_000),
        },
    ),
    ability(
        41,
        "Barkskin",
        DRUID,
        6,
        40,
        CastTime::Instant,
        ticks_from_ms(30_000),
        AbilityRange::SelfCentred,
        AbilityEffect::Absorb {
            base: 20,
            per_level: 8,
            duration: ticks_from_ms(20_000),
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
        };
        bytes.extend_from_slice(&u32::try_from(fields.len()).unwrap_or(0).to_be_bytes());
        for field in fields {
            bytes.extend_from_slice(&field.to_be_bytes());
        }
        bytes.push(ability.projectile.map_or(0, crate::ProjectileKind::code));
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
                let (AbilityEffect::Blizzard { period, .. }
                | AbilityEffect::Beam { period, .. }
                | AbilityEffect::Drain { period, .. }) = ability.effect
                else {
                    panic!("only Blizzard, Lightning Beam and Drain Life channel");
                };
                assert_eq!(ticks % period, 0);
            }
        }
        assert_eq!(ability_by_id(AbilityId::new(0)), None);
        assert_eq!(ability_by_id(AbilityId::new(42)), None);
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
