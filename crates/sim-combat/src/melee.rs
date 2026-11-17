//! Movement-state melee (#885): the basic attack's strikes by weapon family.
//!
//! One button, no directional input. The attacker's movement picks the strike:
//!
//! | Movement | Strike |
//! | --- | --- |
//! | Standing or walking | the **chain**: first, second and a heavier finisher, each continuing the chain when pressed again within [`CHAIN_WINDOW_TICKS`] of the previous strike's recovery |
//! | Running forward on the ground | the **running strike**: a lunge that carries the body forward through its wind-up, with a longer reach; it ends the chain |
//! | Airborne | the **jumping strike**: a downward blow that lands on touchdown in a small area ahead, then a landing recovery; it ends the chain |
//!
//! Every strike winds up and resolves on its contact tick, the authored contact frame of the
//! clip that shows it (asset-tooling #709/#711 `strike.contactSeconds`), so the blow and the
//! damage line up. Weapons differ only through their family's row ([`FamilyMelee::of`]):
//! daggers are fast and short, maces and staves slower and heavier, spears reach furthest with
//! the narrowest arc. Adding a weapon picks a family.

use crate::ticks_from_ms;

/// Strikes in a standing chain.
pub const CHAIN_LENGTH: u8 = 3;
/// After a chain strike's recovery, a further press within this window continues the chain
/// (0.6 s); later, the chain starts over.
pub const CHAIN_WINDOW_TICKS: u16 = ticks_from_ms(600);

/// A weapon's melee family: how its basic strikes time, reach and hit.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum WeaponFamily {
    Unarmed,
    Sword,
    /// Maces, axes and hammers.
    Mace,
    Dagger,
    /// Staves and wands.
    Staff,
    Spear,
    /// A bow swung in melee.
    Bow,
}

impl WeaponFamily {
    pub const ALL: [Self; 7] = [
        Self::Unarmed,
        Self::Sword,
        Self::Mace,
        Self::Dagger,
        Self::Staff,
        Self::Spear,
        Self::Bow,
    ];

    /// Wire code.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Unarmed => 0,
            Self::Sword => 1,
            Self::Mace => 2,
            Self::Dagger => 3,
            Self::Staff => 4,
            Self::Spear => 5,
            Self::Bow => 6,
        }
    }

    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Unarmed),
            1 => Some(Self::Sword),
            2 => Some(Self::Mace),
            3 => Some(Self::Dagger),
            4 => Some(Self::Staff),
            5 => Some(Self::Spear),
            6 => Some(Self::Bow),
            _ => None,
        }
    }

    /// The family's name, as its clips begin (`Sword_Run_Strike`).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Unarmed => "Unarmed",
            Self::Sword => "Sword",
            Self::Mace => "Mace",
            Self::Dagger => "Dagger",
            Self::Staff => "Staff",
            Self::Spear => "Spear",
            Self::Bow => "Bow",
        }
    }
}

/// Which basic strike a press became.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum StrikeVariant {
    /// The chain's first strike.
    First,
    Second,
    /// The chain's third, heavier strike.
    Finisher,
    Running,
    Jumping,
}

impl StrikeVariant {
    pub const ALL: [Self; 5] = [
        Self::First,
        Self::Second,
        Self::Finisher,
        Self::Running,
        Self::Jumping,
    ];

    /// Wire code; 0 is no strike.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::First => 1,
            Self::Second => 2,
            Self::Finisher => 3,
            Self::Running => 4,
            Self::Jumping => 5,
        }
    }

    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::First),
            2 => Some(Self::Second),
            3 => Some(Self::Finisher),
            4 => Some(Self::Running),
            5 => Some(Self::Jumping),
            _ => None,
        }
    }

    /// The chain strike at `index` (0-based, wrapping).
    #[must_use]
    pub const fn chain(index: u8) -> Self {
        match index % CHAIN_LENGTH {
            0 => Self::First,
            1 => Self::Second,
            _ => Self::Finisher,
        }
    }

    /// The strike a press makes: airborne the jumping strike, running the running strike,
    /// otherwise the chain strike at `chain`.
    #[must_use]
    pub const fn select(airborne: bool, running: bool, chain: u8) -> Self {
        if airborne {
            Self::Jumping
        } else if running {
            Self::Running
        } else {
            Self::chain(chain)
        }
    }

    /// The chain index the next standing strike begins with after this one: the chain
    /// advances, and the finisher, the running and the jumping strike start it over.
    #[must_use]
    pub const fn next_chain(self) -> u8 {
        match self {
            Self::First => 1,
            Self::Second => 2,
            Self::Finisher | Self::Running | Self::Jumping => 0,
        }
    }

    /// Whether the strike belongs to the standing chain.
    #[must_use]
    pub const fn is_chain(self) -> bool {
        matches!(self, Self::First | Self::Second | Self::Finisher)
    }
}

/// One strike's timing, damage and reach.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StrikeProfile {
    /// The wind-up from the press to the contact tick. A jumping strike lands on touchdown,
    /// but never before this.
    pub contact_ticks: u16,
    /// From the press until the next strike may begin; a jumping strike's landing recovery
    /// after its blow.
    pub interval_ticks: u16,
    /// Of the weapon damage range.
    pub damage_percent: u16,
    /// XZ centre distance; a jumping strike's landing radius.
    pub reach_units: i32,
    /// Half the arc either side of the facing (yaw units: a quarter turn is 16 384).
    pub half_width: u16,
}

impl StrikeProfile {
    const fn new(
        contact_ms: u32,
        interval_ms: u32,
        damage_percent: u16,
        reach_units: i32,
        half_width_degrees: u32,
    ) -> Self {
        Self {
            contact_ticks: ticks_from_ms(contact_ms),
            interval_ticks: ticks_from_ms(interval_ms),
            damage_percent,
            reach_units,
            half_width: ((half_width_degrees * 65_536 + 180) / 360) as u16,
        }
    }
}

/// A weapon family's basic strikes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FamilyMelee {
    pub chain: [StrikeProfile; CHAIN_LENGTH as usize],
    pub running: StrikeProfile,
    /// How far the running strike carries the body forward over its wind-up, in units.
    pub lunge_units: i32,
    pub jumping: StrikeProfile,
    /// The inclusive height band above the striker's feet the strikes cover, in units.
    pub band: [i32; 2],
}

impl FamilyMelee {
    /// The family's row. Contacts follow the clips' authored contact frames (asset-tooling
    /// #709/#711): running strikes contact 0.5 s into one jog cycle, jumping strikes touch down
    /// 0.35 s into theirs.
    #[must_use]
    pub const fn of(family: WeaponFamily) -> Self {
        type P = StrikeProfile;
        match family {
            // Balanced: two quick cuts and a full-body overhead.
            WeaponFamily::Sword => Self {
                chain: [
                    P::new(250, 600, 60, 255, 50),
                    P::new(250, 600, 60, 255, 50),
                    P::new(383, 1_000, 120, 270, 60),
                ],
                running: P::new(500, 900, 110, 300, 50),
                lunge_units: 325,
                jumping: P::new(350, 450, 140, 240, 75),
                band: [-100, 200],
            },
            // Slow and heavy, a wide swing.
            WeaponFamily::Mace => Self {
                chain: [
                    P::new(350, 750, 75, 245, 55),
                    P::new(350, 750, 75, 245, 55),
                    P::new(467, 1_100, 140, 255, 65),
                ],
                running: P::new(500, 1_000, 130, 285, 55),
                lunge_units: 275,
                jumping: P::new(350, 600, 170, 270, 90),
                band: [-100, 200],
            },
            // Fast and short.
            WeaponFamily::Dagger => Self {
                chain: [
                    P::new(217, 400, 40, 235, 40),
                    P::new(217, 400, 40, 235, 40),
                    P::new(183, 650, 80, 240, 45),
                ],
                running: P::new(500, 750, 85, 275, 40),
                lunge_units: 375,
                jumping: P::new(350, 350, 110, 235, 70),
                band: [-60, 180],
            },
            // Sweeping and a little longer, lighter than a mace.
            WeaponFamily::Staff => Self {
                chain: [
                    P::new(317, 700, 60, 275, 60),
                    P::new(350, 700, 60, 275, 60),
                    P::new(400, 1_000, 110, 285, 70),
                ],
                running: P::new(500, 950, 100, 315, 60),
                lunge_units: 300,
                jumping: P::new(350, 500, 130, 250, 80),
                band: [-100, 200],
            },
            // The longest reach and the narrowest arc.
            WeaponFamily::Spear => Self {
                chain: [
                    P::new(267, 650, 65, 330, 30),
                    P::new(267, 650, 65, 330, 30),
                    P::new(367, 1_000, 125, 340, 35),
                ],
                running: P::new(500, 950, 125, 380, 30),
                lunge_units: 350,
                jumping: P::new(350, 500, 140, 250, 60),
                band: [-60, 200],
            },
            // Jab, cross and hook.
            WeaponFamily::Unarmed => Self {
                chain: [
                    P::new(233, 500, 40, 235, 40),
                    P::new(367, 600, 45, 235, 45),
                    P::new(367, 800, 80, 240, 55),
                ],
                running: P::new(500, 800, 70, 270, 45),
                lunge_units: 300,
                jumping: P::new(350, 400, 90, 235, 70),
                band: [-80, 180],
            },
            // The bow's limb twice, then a push kick.
            WeaponFamily::Bow => Self {
                chain: [
                    P::new(400, 750, 55, 240, 45),
                    P::new(400, 750, 55, 240, 45),
                    P::new(433, 1_000, 100, 240, 50),
                ],
                running: P::new(500, 900, 80, 280, 45),
                lunge_units: 275,
                jumping: P::new(350, 450, 100, 235, 70),
                band: [-100, 200],
            },
        }
    }

    /// The profile of `variant`.
    #[must_use]
    pub const fn strike(self, variant: StrikeVariant) -> StrikeProfile {
        match variant {
            StrikeVariant::First => self.chain[0],
            StrikeVariant::Second => self.chain[1],
            StrikeVariant::Finisher => self.chain[2],
            StrikeVariant::Running => self.running,
            StrikeVariant::Jumping => self.jumping,
        }
    }
}

/// The longest wind-up any strike has, which bounds a pending strike's contact countdown.
pub const MAX_CONTACT_TICKS: u16 = {
    let mut longest = 0;
    let mut family = 0;
    while family < WeaponFamily::ALL.len() {
        let row = FamilyMelee::of(WeaponFamily::ALL[family]);
        let mut variant = 0;
        while variant < StrikeVariant::ALL.len() {
            let ticks = row.strike(StrikeVariant::ALL[variant]).contact_ticks;
            if ticks > longest {
                longest = ticks;
            }
            variant += 1;
        }
        family += 1;
    }
    longest
};

/// The longest strike interval or landing recovery, which bounds the chain window.
pub const MAX_INTERVAL_TICKS: u16 = {
    let mut longest = 0;
    let mut family = 0;
    while family < WeaponFamily::ALL.len() {
        let row = FamilyMelee::of(WeaponFamily::ALL[family]);
        let mut variant = 0;
        while variant < StrikeVariant::ALL.len() {
            let ticks = row.strike(StrikeVariant::ALL[variant]).interval_ticks;
            if ticks > longest {
                longest = ticks;
            }
            variant += 1;
        }
        family += 1;
    }
    longest
};

/// `range` scaled by `percent`, each end rounded to the nearest whole point and at least 1.
#[must_use]
pub const fn scaled_damage(range: [u16; 2], percent: u16) -> [u16; 2] {
    [
        scaled_point(range[0], percent),
        scaled_point(range[1], percent),
    ]
}

/// `end` scaled by `percent`, rounded to the nearest point, between 1 and `u16::MAX`.
const fn scaled_point(end: u16, percent: u16) -> u16 {
    let scaled = (end as u32 * percent as u32 + 50) / 100;
    if scaled == 0 {
        1
    } else if scaled > u16::MAX as u32 {
        u16::MAX
    } else {
        scaled as u16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mean damage per second over a full chain at `range`, before the hit table.
    fn chain_dps(family: WeaponFamily, range: [u16; 2]) -> f64 {
        let row = FamilyMelee::of(family);
        let (mut damage, mut ticks) = (0.0, 0.0);
        for strike in row.chain {
            let [low, high] = scaled_damage(range, strike.damage_percent);
            damage += f64::from(low + high) / 2.;
            ticks += f64::from(strike.interval_ticks);
        }
        damage / (ticks / f64::from(sim_foundation::TICK_HZ))
    }

    #[test]
    fn variants_follow_the_movement_and_the_chain() {
        assert_eq!(StrikeVariant::select(false, false, 0), StrikeVariant::First);
        assert_eq!(
            StrikeVariant::select(false, false, 1),
            StrikeVariant::Second
        );
        assert_eq!(
            StrikeVariant::select(false, false, 2),
            StrikeVariant::Finisher
        );
        assert_eq!(
            StrikeVariant::select(false, true, 2),
            StrikeVariant::Running
        );
        assert_eq!(StrikeVariant::select(true, true, 1), StrikeVariant::Jumping);
        let advanced: Vec<_> = StrikeVariant::ALL.map(StrikeVariant::next_chain).into();
        assert_eq!(advanced, [1, 2, 0, 0, 0]);
        for variant in StrikeVariant::ALL {
            assert_eq!(StrikeVariant::from_code(variant.code()), Some(variant));
        }
        for family in WeaponFamily::ALL {
            assert_eq!(WeaponFamily::from_code(family.code()), Some(family));
        }
    }

    #[test]
    fn every_strike_winds_up_before_its_recovery_ends() {
        for family in WeaponFamily::ALL {
            let row = FamilyMelee::of(family);
            for variant in StrikeVariant::ALL {
                let strike = row.strike(variant);
                assert!(strike.contact_ticks > 0, "{family:?} {variant:?}");
                if variant != StrikeVariant::Jumping {
                    assert!(
                        strike.contact_ticks < strike.interval_ticks,
                        "{family:?} {variant:?}"
                    );
                }
            }
            // The finisher hits hardest of the chain; a running and a jumping strike hit
            // harder than an ordinary chain strike.
            let [first, second, finisher] = row.chain.map(|strike| strike.damage_percent);
            assert!(finisher > first && finisher > second, "{family:?}");
            assert!(row.running.damage_percent > first, "{family:?}");
            assert!(
                row.jumping.damage_percent > row.running.damage_percent,
                "{family:?}"
            );
            assert!(
                row.running.reach_units > row.chain[0].reach_units,
                "{family:?}"
            );
            // Every strike reaches a creature that reaches the striker: its reach plus a
            // player's half width (30 units).
            for variant in StrikeVariant::ALL {
                let reach = row.strike(variant).reach_units;
                assert!(
                    reach >= crate::unit::CREATURE_REACH_UNITS + 30,
                    "{family:?} {variant:?}"
                );
            }
        }
        assert_eq!(MAX_CONTACT_TICKS, ticks_from_ms(500));
        assert_eq!(MAX_INTERVAL_TICKS, ticks_from_ms(1_100));
    }

    #[test]
    fn families_feel_different() {
        let sword = FamilyMelee::of(WeaponFamily::Sword);
        let dagger = FamilyMelee::of(WeaponFamily::Dagger);
        let mace = FamilyMelee::of(WeaponFamily::Mace);
        let spear = FamilyMelee::of(WeaponFamily::Spear);
        let staff = FamilyMelee::of(WeaponFamily::Staff);
        // Daggers are fast and short, maces slow and heavy, spears long and narrow.
        assert!(dagger.chain[0].interval_ticks < sword.chain[0].interval_ticks);
        assert!(dagger.chain[0].reach_units < sword.chain[0].reach_units);
        assert!(mace.chain[0].interval_ticks > sword.chain[0].interval_ticks);
        assert!(mace.chain[0].damage_percent > sword.chain[0].damage_percent);
        assert!(staff.chain[0].interval_ticks > sword.chain[0].interval_ticks);
        assert!(spear.chain[0].reach_units > staff.chain[0].reach_units);
        assert!(spear.chain[0].half_width < sword.chain[0].half_width);
        assert!(spear.lunge_units > mace.lunge_units);
    }

    #[test]
    fn a_level_one_chain_kills_a_timber_wolf_in_about_nine_seconds() {
        // 42 health against the level 1 range, 3–6, ignoring misses and critical hits.
        for family in [
            WeaponFamily::Sword,
            WeaponFamily::Mace,
            WeaponFamily::Dagger,
            WeaponFamily::Staff,
            WeaponFamily::Spear,
        ] {
            let seconds = 42. / chain_dps(family, crate::unit::player_damage(1));
            assert!((7.5..=10.5).contains(&seconds), "{family:?}: {seconds}");
        }
    }

    #[test]
    fn scaled_damage_rounds_and_keeps_a_point() {
        assert_eq!(scaled_damage([3, 6], 60), [2, 4]);
        assert_eq!(scaled_damage([1, 1], 40), [1, 1]);
        assert_eq!(scaled_damage([10, 20], 150), [15, 30]);
    }
}
