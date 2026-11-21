//! Creature attacks with a wind-up (#889, Atlas 5.2.i889).
//!
//! The beasts whose bodies the wildlife release animates (wolves, boars, rats, bears, cave
//! spiders and marsh lurkers) bite like a player's basic strike (#885) instead of hitting the
//! moment their swing is ready:
//!
//! - **Wind-up.** When the swing timer allows and its target stands in reach, the creature
//!   begins one of its two attacks, drawn from its own random stream (#861): the quick
//!   [`CreatureAttack::Quick`] two times in three, else the slower, harder
//!   [`CreatureAttack::Heavy`]. Observers see [`ZoneEvent::CreatureAttackBegan`] and the swing
//!   timer starts its interval, so the bite comes on screen before it lands.
//! - **Contact.** On its contact tick, the family's authored contact frame ([`contact_ticks`]),
//!   the attack resolves against the unit it began on, through the ordinary hit table, if that
//!   unit still stands within reach and a step. A target that got away is missed; a guard
//!   blocks it as any creature blow. The heavy attack deals half again.
//! - **Interruption.** Death, a stun, a cast, an evade or the loss of its fight drops a
//!   pending attack.
//! - **Pounce.** The lynx (#989) has one attack, a pounce: it begins it from
//!   [`POUNCE_LEAP_UNITS`] beyond its reach, crouches through the wind-up turning toward its
//!   target at a bounded rate, leaps along its facing for the last [`POUNCE_LEAP_TICKS`] and,
//!   after the contact, holds still for [`POUNCE_RECOVER_TICKS`]. Stepping aside during the
//!   wind-up dodges it; its recovery is the opening to hit back.
//!
//! A pending attack is canonical creature state, so replay and recovery continue mid-bite.
//! Outcomes stay health only: no knockback or hitstun.

use crate::{CreatureFamily, EntityRef};

/// How far past its reach (units) a target may have stepped by the contact tick and still be
/// bitten: half a metre, so only a real step away dodges.
pub const CREATURE_ATTACK_SLACK_UNITS: i32 = 50;
/// One attack in this many is the heavy one.
pub const HEAVY_ATTACK_ODDS: u32 = 3;

/// The lynx's pounce (#989), from the wildlife descriptor's `greyhaven.wild.lynx` clips: its
/// `Pounce_Windup` lasts 0.5 s and its `Pounce_Lunge` makes contact 0.333 s in, its root
/// travelling 1.37 m between 0.125 s and that contact; `Pounce_Recover` lasts 0.833 s.
const POUNCE_WINDUP_MS: u32 = 500;
const POUNCE_CONTACT_MS: u32 = POUNCE_WINDUP_MS + 333;
const POUNCE_LEAP_START_MS: u32 = POUNCE_WINDUP_MS + 125;
/// How far beyond its reach (units) a pouncer begins its pounce: the lunge's 1.37 m.
pub const POUNCE_LEAP_UNITS: i32 = 137;
/// The last ticks of a pounce's wind-up, from the lunge's take-off to its contact, in which
/// the pouncer leaps along its facing.
pub const POUNCE_LEAP_TICKS: u16 =
    crate::ticks_from_ms(POUNCE_CONTACT_MS) - crate::ticks_from_ms(POUNCE_LEAP_START_MS);
/// The leap's speed, covering [`POUNCE_LEAP_UNITS`] in [`POUNCE_LEAP_TICKS`] (6.9 m/s).
pub const POUNCE_LEAP_SPEED_SUBUNITS_PER_TICK: i32 = crate::subunits_per_tick(690);
/// Ticks after its contact a pouncer holds still in its `Pounce_Recover`, open to a counter.
pub const POUNCE_RECOVER_TICKS: u16 = crate::ticks_from_ms(833);

/// One of a beast's two attacks, its `Attack_A` and `Attack_B` clips.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, strum::VariantArray)]
pub enum CreatureAttack {
    /// The quick bite or swipe.
    Quick,
    /// The slower lunge with a longer wind-up, dealing half again.
    Heavy,
}

impl CreatureAttack {
    /// Stable wire and canonical code.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Quick => 0,
            Self::Heavy => 1,
        }
    }

    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Quick),
            1 => Some(Self::Heavy),
            _ => None,
        }
    }

    /// Its damage from a rolled `amount`.
    #[must_use]
    pub const fn damage(self, amount: u32) -> u32 {
        match self {
            Self::Quick => amount,
            Self::Heavy => amount * 3 / 2,
        }
    }
}

/// Ticks from the start of `family`'s `attack` to its bite, the wildlife descriptor's
/// `contactSeconds` (asset-tooling #716, #723); `None` for families that swing at once.
#[must_use]
pub const fn contact_ticks(family: CreatureFamily, attack: CreatureAttack) -> Option<u16> {
    let [quick, heavy] = match family {
        CreatureFamily::Wolf => [375, 583],
        CreatureFamily::Boar => [375, 625],
        CreatureFamily::Vermin => [417, 500],
        CreatureFamily::Bear => [458, 833],
        CreatureFamily::Spider => [375, 583],
        CreatureFamily::Lurker => [333, 583],
        // The lynx (#989) pounces only: `Pounce_Windup`'s 0.5 s, then `Pounce_Lunge`'s contact.
        CreatureFamily::Lynx => [POUNCE_CONTACT_MS, POUNCE_CONTACT_MS],
        CreatureFamily::Marauder
        | CreatureFamily::Mirefin
        | CreatureFamily::Redbrand
        | CreatureFamily::Prop
        | CreatureFamily::Ogre => return None,
    };
    Some(crate::ticks_from_ms(match attack {
        CreatureAttack::Quick => quick,
        CreatureAttack::Heavy => heavy,
    }))
}

/// A creature attack between its start and its bite.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PendingAttack {
    pub attack: CreatureAttack,
    /// The unit it began on, which it bites if still in reach.
    pub target: EntityRef,
    /// Ticks of wind-up left.
    pub contact: u16,
}

impl PendingAttack {
    /// Whether a pouncer holding it is in its leap (#989): the wind-up's last
    /// [`POUNCE_LEAP_TICKS`].
    #[must_use]
    pub const fn leaping(self) -> bool {
        self.contact <= POUNCE_LEAP_TICKS
    }

    /// Whether a `family` creature may hold it: a family that winds up, no further from its
    /// bite than the attack's whole wind-up.
    #[must_use]
    pub const fn is_valid(self, family: CreatureFamily) -> bool {
        match contact_ticks(family, self.attack) {
            Some(ticks) => self.contact > 0 && self.contact <= ticks,
            None => false,
        }
    }
}
