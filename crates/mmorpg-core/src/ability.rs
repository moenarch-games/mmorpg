//! The immutable ability catalog: every class and creature ability with a
//! global `u8` ID. Zone content binds the catalog to its identity (see
//! [`crate::ZoneContent::with_creature_abilities`]), so a recovered zone
//! always runs the abilities it was checkpointed with. Rules that use the
//! catalog live in `casting`; aura bookkeeping in `aura`.

use crate::class::PlayerClass;
use crate::ticks_from_ms;
use crate::unit::PLAYER_REACH_UNITS;

/// Every player ability starts this global cooldown, the action graph's
/// `GlobalCooldown` recovery (1.5 s); haste shortens it only for a shot (#884).
pub const GLOBAL_COOLDOWN_TICKS: u16 =
    crate::action::recovery_ticks(crate::action::ActionId::GlobalCooldown);
/// At most this many auras rest on one unit.
pub const MAX_AURAS: usize = 8;
/// At most this many cooldowns run on one player: one per slot of both action-bar rows.
pub const MAX_COOLDOWNS: usize = 20;
/// Zone-RNG jitter added whenever a creature's ability timer starts.
pub const CREATURE_ABILITY_JITTER_TICKS: u16 = ticks_from_ms(1_000);

/// A global ability ID; 0 is never an ability.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AbilityId(u32);

impl AbilityId {
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u32 {
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
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, strum::VariantArray)]
pub enum AuraKind {
    DamageOverTime,
    HealOverTime,
    Absorb,
    Root,
    Snare,
    Stun,
    Haste,
    /// A fresh corpse (#935) the bearer may spend with Raise Dead or Corpse Burst; the
    /// aura's caster is the corpse.
    Corpse,
    /// The Druid's wolf form (#957): `amount` is the movement speed bonus in percent, and
    /// only the form's own abilities may be used while it rests on the bearer.
    Shapeshift,
}

impl AuraKind {
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
            Self::Corpse => 8,
            Self::Shapeshift => 9,
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
            8 => Some(Self::Corpse),
            9 => Some(Self::Shapeshift),
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
            Self::Corpse => "corpse",
            Self::Shapeshift => "shapeshift",
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
    /// Damage in `damage` (inclusive) + `per_level` to every living creature within `radius`
    /// of the caster, then a root that damage breaks once `unbreakable_ticks` have passed.
    Nova {
        damage: [u16; 2],
        per_level: u16,
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
    /// A thrown flask's splash (#290): where it shatters, every hostile unit within `radius`
    /// that the point can see takes a damage over time of `base` + `per_level`.
    Splash {
        base: u16,
        per_level: u16,
        radius: i32,
        duration: u16,
        period: u16,
    },
    /// Raises a minion beside the caster (#295) with `health` + `health_per_level` health that
    /// hits for `damage` (inclusive) + `damage_per_level` every `swing_ticks` and crumbles after
    /// `duration` ticks. The oldest of the caster's minions crumbles when a raise would exceed
    /// [`crate::MAX_MINIONS_PER_SUMMONER`].
    Summon {
        health: u16,
        health_per_level: u16,
        damage: [u16; 2],
        damage_per_level: u16,
        swing_ticks: u16,
        duration: u16,
    },
    /// Raises summoned terrain (#293) a fixed distance in front of the caster: a temporary,
    /// destroyable obstacle that blocks movement and line of sight.
    SummonTerrain { kind: crate::SummonedTerrainKind },
    /// Heroic Leap (#294): the caster jumps `distance` units along its facing; walls stop the
    /// flight, gravity brings it down, and on landing every hostile unit within `radius` that
    /// the landing point can see takes `damage` + `per_level` and a `knockback`, both falling
    /// off linearly to half at the edge.
    Leap {
        distance: i32,
        damage: [u16; 2],
        per_level: u16,
        radius: i32,
        knockback: i32,
    },
    /// Grappling Hook (#294): the selected unit takes `damage` + `per_level` and is pulled
    /// toward the caster, stopping at melee reach; walls stop it and the forced-move immunity
    /// applies.
    Hook { damage: [u16; 2], per_level: u16 },
    /// Blink (#294): the caster teleports `distance` units along its facing, or as far as the
    /// way is free; see `mobility.rs`.
    Blink { distance: i32 },
    /// Calls the caster's one persistent companion (#296) beside it with `health` +
    /// `health_per_level` health that bites for `damage` (inclusive) + `damage_per_level` every
    /// `swing_ticks`. It is not timed and outlives its owner's death; it goes when killed or
    /// when its owner leaves the zone. Calling it while it stands brings it back to the owner's
    /// side with the health it has.
    Companion {
        health: u16,
        health_per_level: u16,
        damage: [u16; 2],
        damage_per_level: u16,
        swing_ticks: u16,
    },
    /// Raise Dead (#935): spends the caster's fresh corpse to raise one helper like
    /// [`AbilityEffect::Summon`]. At most [`crate::RAISE_DEAD_LIMIT`] stand at once; a raise
    /// past it is refused and keeps the corpse. Its aura is the corpse mark itself
    /// ([`AuraKind::Corpse`]), which lasts [`CORPSE_WINDOW_TICKS`].
    RaiseDead {
        health: u16,
        health_per_level: u16,
        damage: [u16; 2],
        damage_per_level: u16,
        swing_ticks: u16,
        duration: u16,
    },
    /// Corpse Burst (#935): spends the caster's fresh corpse, which bursts like a blast without
    /// knockback: every hostile unit within `radius` that the corpse can see takes `damage` +
    /// `per_level`, falling off linearly to half at the edge.
    CorpseBurst {
        damage: [u16; 2],
        per_level: u16,
        radius: i32,
    },
    /// Divine Reckoning (#936): a holy strike of `percent` of the harm the target dealt within the
    /// caster's reach this encounter, which the cast consumes; each enemy's stored harm is capped
    /// at `pool`. See `reckoning.rs`.
    Reckoning { percent: u16, pool: u16 },
    /// Wolf Form (#957): the caster takes the wolf's shape for `duration` ticks, moving
    /// `speed` percent faster; using it again in that shape returns the caster to caster form.
    /// See [`AuraKind::Shapeshift`] for what the shape may use.
    Shapeshift { speed: u16, duration: u16 },
    /// A Redbrand signaler's horn call for help (#952): once the cast completes, the one authored
    /// partner content binds to the caller ([`crate::ZoneContent::with_signal_calls`]) engages the
    /// caller's threat leader. Any interruption puts the call off for its whole cooldown.
    SignalCall,
}

/// How long a corrupted victim's corpse stays spendable for its Necromancer (#935): 10 s.
pub const CORPSE_WINDOW_TICKS: u16 = ticks_from_ms(10_000);

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
            }
            | AbilityEffect::Splash {
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
            AbilityEffect::RaiseDead { .. } => (AuraKind::Corpse, CORPSE_WINDOW_TICKS, 0),
            AbilityEffect::Shapeshift { duration, .. } => (AuraKind::Shapeshift, duration, 0),
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
            | AbilityEffect::Drain { .. }
            | AbilityEffect::Summon { .. }
            | AbilityEffect::SummonTerrain { .. }
            | AbilityEffect::Companion { .. }
            | AbilityEffect::Leap { .. }
            | AbilityEffect::Hook { .. }
            | AbilityEffect::Blink { .. }
            | AbilityEffect::CorpseBurst { .. }
            | AbilityEffect::Reckoning { .. }
            | AbilityEffect::SignalCall => return None,
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
                | AbilityEffect::Shapeshift { .. }
        )
    }

    /// Whether the ability flies freely along the facing without a target: every shot (#517;
    /// earlier Aimed Shot #286, Firebolt #289, Throwing Knife #290 and the aimed specials).
    #[must_use]
    pub const fn aims_freely(&self) -> bool {
        self.projectile.is_some()
    }

    /// How its projectile treats what it meets (#291). Aimed specials ignore any selected target
    /// and always fly along the facing.
    #[must_use]
    pub const fn shot(&self) -> Shot {
        match self.id.0 {
            19 => Shot::Pierce,
            20 => Shot::Ricochet,
            43 => Shot::Shatter,
            44 => Shot::Bounce,
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
    /// A thrown flask (#290): lobbed in an arc, it shatters on the first thing it meets and its
    /// effect splashes around that point.
    Shatter,
    /// A thrown bomb (#290): lobbed in an arc, it bounces off surfaces and rolls to rest, then
    /// bursts when it touches a unit or its fuse ([`crate::BOMB_FUSE_TICKS`]) runs out.
    Bounce,
}

mod catalog;
mod identity;
pub use catalog::{ABILITY_CATALOG, ids};
pub(crate) use identity::catalog_bytes;

/// The catalog row of `id`, if any.
#[must_use]
pub fn ability_by_id(id: AbilityId) -> Option<&'static Ability> {
    ABILITY_CATALOG.get(usize::try_from(id.get().checked_sub(1)?).ok()?)
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

#[cfg(test)]
mod tests {
    use super::*;
    use strum::VariantArray;

    #[test]
    fn the_catalog_is_ordered_bounded_and_consistent() {
        for (index, ability) in ABILITY_CATALOG.iter().enumerate() {
            assert_eq!(ability.id.get() as usize, index + 1, "{}", ability.name);
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
        assert_eq!(ability_by_id(AbilityId::new(60)), None);
        // Each class has at most MAX_COOLDOWNS abilities with a cooldown.
        for &class in PlayerClass::VARIANTS {
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
