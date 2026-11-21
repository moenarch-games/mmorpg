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
//! - **Coiled strike.** The marsh adder (#990) has one attack too: from its reach it holds
//!   still, coiled and hissing, through [`ADDER_WARN_TICKS`] (one `Warn_Hiss_Loop`), turning
//!   toward its target, strikes on its `Strike` clip's contact and holds still through the
//!   rest of the strike and its `Strike_Recover`, [`ADDER_RECOVER_TICKS`]. Backing off during
//!   the hiss dodges it.
//! - **Dive.** The cave bat (#995) flies ([`crate::flight`]) and has one attack, a dive: from
//!   [`BAT_DIVE_UNITS`] beyond its reach it flares and hisses, hovering through its
//!   `Dive_Windup`, then stoops along its facing and down to its target over [`BAT_DIVE_TICKS`]
//!   to `Dive_Contact`'s contact, and hangs low and labouring through [`BAT_RECOVER_TICKS`],
//!   within a sword's reach, before it climbs back out of it. Its bite lands only on a target
//!   still within reach and a step and within [`crate::flight::BITE_HEIGHT_UNITS`] of its
//!   height, so stepping away through the wind-up dodges it.
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

/// The marsh adder's coiled strike (#990), from the wildlife descriptor's
/// `greyhaven.wild.marsh-adder` clips: it hisses through one 1.333 s `Warn_Hiss_Loop`, then
/// its `Strike` (0.417 s) makes contact 0.208 s in; `Strike_Recover` lasts 0.833 s.
const ADDER_WARN_MS: u32 = 1_333;
const ADDER_CONTACT_MS: u32 = ADDER_WARN_MS + 208;
/// The adder's warning: the ticks it holds still, coiled and hissing, before it strikes.
pub const ADDER_WARN_TICKS: u16 = crate::ticks_from_ms(ADDER_WARN_MS);
/// Ticks after its contact the adder holds still, the rest of its `Strike` and its
/// `Strike_Recover`, open to a counter.
pub const ADDER_RECOVER_TICKS: u16 = crate::ticks_from_ms(417 - 208 + 833);
/// The adder slithers after a target at 1.2 m/s: a player who walks away leaves it behind.
pub const ADDER_SLITHER_SPEED_SUBUNITS_PER_TICK: i32 = crate::subunits_per_tick(120);

/// The cave bat's dive (#995), from the wildlife descriptor's `greyhaven.wild.cave-bat` clips:
/// its `Dive_Windup` lasts 0.5 s and its `Dive_Contact` makes contact 0.333 s in, its root
/// travelling 2.4 m along the dive line from the start to that contact, about 35° down;
/// `Dive_Contact` lasts 0.5 s and `Dive_Recover` 0.833 s.
const BAT_WINDUP_MS: u32 = 500;
const BAT_CONTACT_MS: u32 = BAT_WINDUP_MS + 333;
/// How far beyond its reach (units) the bat begins its dive: the dive line's 2 m across.
pub const BAT_DIVE_UNITS: i32 = 200;
/// The last ticks of the dive's wind-up, `Dive_Contact` up to its contact, in which the bat
/// stoops along its facing and down.
pub const BAT_DIVE_TICKS: u16 =
    crate::ticks_from_ms(BAT_CONTACT_MS) - crate::ticks_from_ms(BAT_WINDUP_MS);
/// The stoop's speed across, covering [`BAT_DIVE_UNITS`] in [`BAT_DIVE_TICKS`] (6 m/s).
pub const BAT_DIVE_SPEED_SUBUNITS_PER_TICK: i32 = crate::subunits_per_tick(600);
/// Ticks after its contact the bat hangs low, the rest of `Dive_Contact` and its
/// `Dive_Recover`, open to a counter.
pub const BAT_RECOVER_TICKS: u16 = crate::ticks_from_ms(500 - 333 + 833);
/// The bat flies after a target at 4.2 m/s: a running player outpaces it.
pub const BAT_FLIGHT_SPEED_SUBUNITS_PER_TICK: i32 = crate::subunits_per_tick(420);

/// A set-piece attack (#989, #990): one attack, begun from [`Self::leap_units`] beyond the
/// creature's reach, wound up standing still and turning toward its target, closed with a leap
/// over the wind-up's last [`Self::leap_ticks`], then a still recovery of
/// [`Self::recover_ticks`] after its contact. Between attacks it closes in at
/// [`Self::approach_speed`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Lunge {
    pub leap_units: i32,
    pub leap_ticks: u16,
    pub leap_speed: i32,
    pub recover_ticks: u16,
    pub approach_speed: i32,
}

/// The set-piece attack of `family`, if it has one: the lynx's pounce (#989), the marsh
/// adder's coiled strike (#990) and the cave bat's dive (#995).
#[must_use]
pub const fn lunge(family: CreatureFamily) -> Option<Lunge> {
    match family {
        CreatureFamily::Lynx => Some(Lunge {
            leap_units: POUNCE_LEAP_UNITS,
            leap_ticks: POUNCE_LEAP_TICKS,
            leap_speed: POUNCE_LEAP_SPEED_SUBUNITS_PER_TICK,
            recover_ticks: POUNCE_RECOVER_TICKS,
            approach_speed: crate::CREATURE_WALK_SPEED_SUBUNITS_PER_TICK,
        }),
        CreatureFamily::Adder => Some(Lunge {
            leap_units: 0,
            leap_ticks: 0,
            leap_speed: 0,
            recover_ticks: ADDER_RECOVER_TICKS,
            approach_speed: ADDER_SLITHER_SPEED_SUBUNITS_PER_TICK,
        }),
        CreatureFamily::Bat => Some(Lunge {
            leap_units: BAT_DIVE_UNITS,
            leap_ticks: BAT_DIVE_TICKS,
            leap_speed: BAT_DIVE_SPEED_SUBUNITS_PER_TICK,
            recover_ticks: BAT_RECOVER_TICKS,
            approach_speed: BAT_FLIGHT_SPEED_SUBUNITS_PER_TICK,
        }),
        _ => None,
    }
}

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
        // The adder (#990) strikes only: its hiss, then `Strike`'s contact.
        CreatureFamily::Adder => [ADDER_CONTACT_MS, ADDER_CONTACT_MS],
        // The bat (#995) dives only: `Dive_Windup`'s 0.5 s, then `Dive_Contact`'s contact.
        CreatureFamily::Bat => [BAT_CONTACT_MS, BAT_CONTACT_MS],
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
    /// Whether a `family` creature holding it is in its leap (#989): its [`Lunge`]'s last
    /// [`Lunge::leap_ticks`] of the wind-up.
    #[must_use]
    pub const fn leaping(self, family: CreatureFamily) -> bool {
        match lunge(family) {
            Some(lunge) => self.contact <= lunge.leap_ticks,
            None => false,
        }
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
