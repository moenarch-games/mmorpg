//! Player classes and their resources: Warden rage, Ranger focus and
//! Arcanist mana. A player chooses a class once; until then it keeps the
//! class-agnostic baseline without a resource or abilities. All integer.

use crate::unit::percent_of;

/// Rage and focus share this maximum.
pub const RAGE_FOCUS_MAX: u16 = 100;
/// Warden rage gained per auto-attack hit dealt …
pub const RAGE_PER_HIT_DEALT: u16 = 6;
/// … per hit taken (a creature swing or ability that lands) …
pub const RAGE_PER_HIT_TAKEN: u16 = 4;
/// … and lost per resource interval out of combat.
pub const RAGE_DECAY: u16 = 2;
/// Ranger focus gained per resource interval.
pub const FOCUS_REGEN: u16 = 5;
/// Arcanist mana at level 1; each further level adds [`MANA_PER_LEVEL`].
pub const MANA_BASE: u16 = 110;
pub const MANA_PER_LEVEL: u16 = 22;
/// The five-second rule: no mana regeneration this long after spending.
pub const MANA_REGEN_DELAY_TICKS: u16 = 150;
/// Mana regenerates this percentage of its maximum per interval out of
/// combat, and [`MANA_REGEN_COMBAT_PERCENT`] in combat (rounded up).
pub const MANA_REGEN_PERCENT: u32 = 5;
pub const MANA_REGEN_COMBAT_PERCENT: u32 = 1;
/// Ticks between two resource steps (1 s).
pub const RESOURCE_INTERVAL_TICKS: u16 = 30;

/// A playable class; the wire value is its discriminant.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum PlayerClass {
    Warden,
    Ranger,
    Arcanist,
}

impl PlayerClass {
    pub const ALL: [Self; 3] = [Self::Warden, Self::Ranger, Self::Arcanist];

    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Warden),
            1 => Some(Self::Ranger),
            2 => Some(Self::Arcanist),
            _ => None,
        }
    }

    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Warden => 0,
            Self::Ranger => 1,
            Self::Arcanist => 2,
        }
    }

    /// Stable lower-case name for exports.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Warden => "warden",
            Self::Ranger => "ranger",
            Self::Arcanist => "arcanist",
        }
    }

    #[must_use]
    pub const fn resource(self) -> ResourceKind {
        match self {
            Self::Warden => ResourceKind::Rage,
            Self::Ranger => ResourceKind::Focus,
            Self::Arcanist => ResourceKind::Mana,
        }
    }
}

/// Presentation sex of a character; it has no gameplay effect.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Sex {
    Female,
    Male,
}

impl Sex {
    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Female),
            1 => Some(Self::Male),
            _ => None,
        }
    }

    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Female => 0,
            Self::Male => 1,
        }
    }
}

/// A player's one-time class choice.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ClassChoice {
    pub class: PlayerClass,
    pub sex: Sex,
}

impl ClassChoice {
    /// The appearance code of a player entity: `1 + class × 2 + sex`.
    /// Zero (no class) is reserved for players who have not chosen.
    #[must_use]
    pub const fn appearance(self) -> u16 {
        1 + self.class.code() as u16 * 2 + self.sex.code() as u16
    }

    /// Inverse of [`Self::appearance`]; `None` for 0 and unknown codes.
    #[must_use]
    pub const fn from_appearance(code: u16) -> Option<Self> {
        if code == 0 || code > 6 {
            return None;
        }
        let index = code - 1;
        match (
            PlayerClass::from_code((index / 2) as u8),
            Sex::from_code((index % 2) as u8),
        ) {
            (Some(class), Some(sex)) => Some(Self { class, sex }),
            _ => None,
        }
    }
}

/// The resource a class spends; the wire value is 1 + its discriminant.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ResourceKind {
    Rage,
    Focus,
    Mana,
}

impl ResourceKind {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Rage => "rage",
            Self::Focus => "focus",
            Self::Mana => "mana",
        }
    }

    /// The maximum at `level`: 100 rage or focus; mana 110 + 22 × (level − 1).
    #[must_use]
    pub const fn max(self, level: u8) -> u16 {
        match self {
            Self::Rage | Self::Focus => RAGE_FOCUS_MAX,
            Self::Mana => MANA_BASE + MANA_PER_LEVEL * level.saturating_sub(1) as u16,
        }
    }

    /// The value after choosing the class: rage starts empty, focus and
    /// mana full.
    #[must_use]
    pub const fn start(self, level: u8) -> u16 {
        match self {
            Self::Rage => 0,
            Self::Focus | Self::Mana => self.max(level),
        }
    }
}

/// The resource state machine of one player: value plus the two timers
/// that drive regeneration and decay.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ResourceClock {
    pub(crate) value: u16,
    /// Ticks counted toward the next resource step, below the interval.
    pub(crate) ticks: u16,
    /// Arcanist five-second rule: ticks left without mana regeneration.
    pub(crate) delay: u16,
}

impl ResourceClock {
    /// One tick of resource upkeep. Counters only run while the step can
    /// change the value, so a resting player's state stays unchanged.
    pub(crate) fn advance(&mut self, kind: ResourceKind, level: u8, in_combat: bool) {
        let max = kind.max(level);
        let step: Option<u16> = match kind {
            ResourceKind::Rage => (!in_combat && self.value > 0).then_some(0),
            ResourceKind::Focus => (self.value < max).then_some(FOCUS_REGEN),
            ResourceKind::Mana => {
                if self.delay > 0 {
                    self.delay -= 1;
                    self.ticks = 0;
                    return;
                }
                let percent = if in_combat {
                    MANA_REGEN_COMBAT_PERCENT
                } else {
                    MANA_REGEN_PERCENT
                };
                (self.value < max)
                    .then(|| u16::try_from(percent_of(u32::from(max), percent)).unwrap_or(u16::MAX))
            }
        };
        let Some(gain) = step else {
            self.ticks = 0;
            return;
        };
        self.ticks += 1;
        if self.ticks < RESOURCE_INTERVAL_TICKS {
            return;
        }
        self.ticks = 0;
        self.value = match kind {
            ResourceKind::Rage => self.value.saturating_sub(RAGE_DECAY),
            ResourceKind::Focus | ResourceKind::Mana => self.value.saturating_add(gain).min(max),
        };
    }

    /// Spends `cost`; mana spending restarts the five-second rule.
    pub(crate) fn spend(&mut self, kind: ResourceKind, cost: u16) {
        self.value = self.value.saturating_sub(cost);
        if kind == ResourceKind::Mana && cost > 0 {
            self.delay = MANA_REGEN_DELAY_TICKS;
            self.ticks = 0;
        }
    }

    pub(crate) fn gain(&mut self, kind: ResourceKind, level: u8, amount: u16) {
        self.value = self.value.saturating_add(amount).min(kind.max(level));
    }

    /// Whether a recovered clock is reachable for `kind` at `level`.
    pub(crate) fn is_valid(self, kind: ResourceKind, level: u8) -> bool {
        self.value <= kind.max(level)
            && self.ticks < RESOURCE_INTERVAL_TICKS
            && self.delay <= MANA_REGEN_DELAY_TICKS
            && (kind == ResourceKind::Mana || self.delay == 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appearance_codes_round_trip_and_reserve_zero() {
        let mut codes = Vec::new();
        for class in PlayerClass::ALL {
            for sex in [Sex::Female, Sex::Male] {
                let choice = ClassChoice { class, sex };
                assert_eq!(
                    ClassChoice::from_appearance(choice.appearance()),
                    Some(choice)
                );
                codes.push(choice.appearance());
            }
        }
        assert_eq!(codes, [1, 2, 3, 4, 5, 6]);
        assert_eq!(ClassChoice::from_appearance(0), None);
        assert_eq!(ClassChoice::from_appearance(7), None);
        assert_eq!(PlayerClass::from_code(3), None);
        assert_eq!(Sex::from_code(2), None);
    }

    #[test]
    fn resources_follow_the_class_contract() {
        assert_eq!(ResourceKind::Mana.max(1), 110);
        assert_eq!(ResourceKind::Mana.max(10), 308);
        assert_eq!(ResourceKind::Rage.start(5), 0);
        assert_eq!(ResourceKind::Focus.start(5), 100);

        let mut rage = ResourceClock {
            value: 5,
            ..ResourceClock::default()
        };
        for _ in 0..29 {
            rage.advance(ResourceKind::Rage, 1, false);
        }
        assert_eq!(rage.value, 5);
        rage.advance(ResourceKind::Rage, 1, false);
        assert_eq!((rage.value, rage.ticks), (3, 0));
        rage.advance(ResourceKind::Rage, 1, true);
        assert_eq!((rage.value, rage.ticks), (3, 0), "no decay in combat");

        let mut mana = ResourceClock {
            value: 50,
            ..ResourceClock::default()
        };
        mana.spend(ResourceKind::Mana, 25);
        assert_eq!((mana.value, mana.delay), (25, 150));
        for _ in 0..150 + 29 {
            mana.advance(ResourceKind::Mana, 1, false);
        }
        assert_eq!(mana.value, 25);
        mana.advance(ResourceKind::Mana, 1, false);
        assert_eq!(mana.value, 31, "5 % of 110, rounded up");
        for _ in 0..30 {
            mana.advance(ResourceKind::Mana, 1, true);
        }
        assert_eq!(mana.value, 33, "1 % of 110, rounded up");

        let mut focus = ResourceClock {
            value: 98,
            ..ResourceClock::default()
        };
        for _ in 0..30 {
            focus.advance(ResourceKind::Focus, 1, true);
        }
        assert_eq!(focus.value, 100);
        focus.advance(ResourceKind::Focus, 1, true);
        assert_eq!(focus.ticks, 0, "a full resource stops counting");
    }
}
