//! Physique V1 (#633): eleven continuous body and face sliders and the one authoritative
//! calculation of their stat effect.
//!
//! A [`Physique`] holds eleven normalized values in fixed point, `0..=PHYSIQUE_SCALE` standing
//! for `[0, 1]`; [`PHYSIQUE_NEUTRAL`] (0.5) is the neutral value of each, and characters saved
//! before physique existed load neutral. Only the first five parameters (frame, muscularity, body
//! fat, shoulder width and height) carry stat coefficients; the six cosmetic parameters have
//! zero coefficients for every attribute.
//!
//! The effect is a pure function of the current values: each stat parameter's signed
//! contribution is its coefficient times its deviation from neutral, the contributions to one
//! attribute are summed, and the sum is clamped to ±[`MAX_PHYSIQUE_MODIFIER_BP`] (±2 %). Nothing is
//! stored or accumulated, so edits, loads, equips, class changes and replay never stack.
//! Players have no base attributes (equipment alone adds stamina, strength, agility and intellect),
//! so the modifier scales the final attributes those feed: maximum health and melee damage.
//! The capsule, reach, hit geometry, timing and class behaviour are unchanged.

/// Fixed-point scale of a normalized slider value: `PHYSIQUE_SCALE` is 1.0.
pub const PHYSIQUE_SCALE: u16 = 10_000;
/// The neutral value of every slider (0.5).
pub const PHYSIQUE_NEUTRAL: u16 = PHYSIQUE_SCALE / 2;
/// Number of physique parameters.
pub const PHYSIQUE_PARAMS: usize = 11;
/// The parameters that may carry stat coefficients: the first five.
pub const PHYSIQUE_STAT_PARAMS: usize = 5;
/// The bound of each final attribute modifier, in basis points (±2 %).
pub const MAX_PHYSIQUE_MODIFIER_BP: i32 = 200;
const BP_SCALE: i64 = 10_000;

/// One physique slider, in canonical order.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum PhysiqueParam {
    Frame,
    Muscularity,
    BodyFat,
    ShoulderWidth,
    Height,
    BustFullness,
    WaistWidth,
    HipWidth,
    GluteFullness,
    JawWidth,
    JawProjection,
}

impl PhysiqueParam {
    /// Every parameter in canonical order.
    pub const ALL: [Self; PHYSIQUE_PARAMS] = [
        Self::Frame,
        Self::Muscularity,
        Self::BodyFat,
        Self::ShoulderWidth,
        Self::Height,
        Self::BustFullness,
        Self::WaistWidth,
        Self::HipWidth,
        Self::GluteFullness,
        Self::JawWidth,
        Self::JawProjection,
    ];

    /// Index in canonical order (and in the wire layout).
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The canonical camel-case key.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Frame => "frame",
            Self::Muscularity => "muscularity",
            Self::BodyFat => "bodyFat",
            Self::ShoulderWidth => "shoulderWidth",
            Self::Height => "height",
            Self::BustFullness => "bustFullness",
            Self::WaistWidth => "waistWidth",
            Self::HipWidth => "hipWidth",
            Self::GluteFullness => "gluteFullness",
            Self::JawWidth => "jawWidth",
            Self::JawProjection => "jawProjection",
        }
    }

    /// Whether the parameter may affect stats (the original five).
    #[must_use]
    pub const fn affects_stats(self) -> bool {
        self.index() < PHYSIQUE_STAT_PARAMS
    }
}

/// An attribute physique can adjust: the final values the existing attributes feed.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum PhysiqueAttribute {
    /// Level health plus the equipped stamina's bonus.
    MaxHealth,
    /// The melee damage range plus the equipped primary stat's bonus.
    MeleeDamage,
}

/// Number of [`PhysiqueAttribute`]s.
pub const PHYSIQUE_ATTRIBUTES: usize = 2;

impl PhysiqueAttribute {
    pub const ALL: [Self; PHYSIQUE_ATTRIBUTES] = [Self::MaxHealth, Self::MeleeDamage];
}

/// Basis points each stat parameter contributes per attribute at its extreme (value 0 gives the
/// negated row, 1 the row, neutral nothing), indexed by [`PhysiqueParam`] then
/// [`PhysiqueAttribute`]. The cosmetic rows are zero. Sums past ±2 % are clamped.
pub const PHYSIQUE_COEFFICIENTS: [[i32; PHYSIQUE_ATTRIBUTES]; PHYSIQUE_PARAMS] = [
    // frame: a larger frame is sturdier.
    [100, 0],
    // muscularity: stronger blows, a little sturdier.
    [50, 150],
    // bodyFat: sturdier, a little slower to hit hard.
    [100, -50],
    // shoulderWidth: leverage.
    [0, 50],
    // height: reach is unchanged; a little sturdier and heavier blows.
    [50, 50],
    // Cosmetic only: exactly zero.
    [0, 0],
    [0, 0],
    [0, 0],
    [0, 0],
    [0, 0],
    [0, 0],
];

/// The eleven slider values of one character, `0..=PHYSIQUE_SCALE` each.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Physique {
    values: [u16; PHYSIQUE_PARAMS],
}

impl Default for Physique {
    fn default() -> Self {
        Self::NEUTRAL
    }
}

/// A refused physique: a value that is not finite or lies outside `[0, 1]`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidPhysique;

impl std::fmt::Display for InvalidPhysique {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("physique values must be finite and within [0, 1]")
    }
}

impl std::error::Error for InvalidPhysique {}

impl Physique {
    /// Every slider neutral: no stat change.
    pub const NEUTRAL: Self = Self {
        values: [PHYSIQUE_NEUTRAL; PHYSIQUE_PARAMS],
    };

    /// From fixed-point values; each must be at most [`PHYSIQUE_SCALE`].
    pub fn from_fixed(values: [u16; PHYSIQUE_PARAMS]) -> Result<Self, InvalidPhysique> {
        if values.iter().all(|&value| value <= PHYSIQUE_SCALE) {
            Ok(Self { values })
        } else {
            Err(InvalidPhysique)
        }
    }

    /// From normalized values in canonical order: each must be finite and within `[0, 1]`, and
    /// is rounded to the nearest fixed-point step.
    pub fn from_normalized(values: [f64; PHYSIQUE_PARAMS]) -> Result<Self, InvalidPhysique> {
        let mut fixed = [0; PHYSIQUE_PARAMS];
        for (slot, value) in fixed.iter_mut().zip(values) {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(InvalidPhysique);
            }
            // In range by the check above.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            {
                *slot = (value * f64::from(PHYSIQUE_SCALE)).round() as u16;
            }
        }
        Self::from_fixed(fixed)
    }

    /// Fixed-point values in canonical order.
    #[must_use]
    pub const fn fixed(&self) -> [u16; PHYSIQUE_PARAMS] {
        self.values
    }

    #[must_use]
    pub const fn get(&self, param: PhysiqueParam) -> u16 {
        self.values[param.index()]
    }

    /// `self` with `param` set; the value must be at most [`PHYSIQUE_SCALE`].
    pub fn with(mut self, param: PhysiqueParam, value: u16) -> Result<Self, InvalidPhysique> {
        if value > PHYSIQUE_SCALE {
            return Err(InvalidPhysique);
        }
        self.values[param.index()] = value;
        Ok(self)
    }

    /// The normalized value of `param`.
    #[must_use]
    pub fn normalized(&self, param: PhysiqueParam) -> f64 {
        f64::from(self.get(param)) / f64::from(PHYSIQUE_SCALE)
    }

    /// The combined, clamped modifier of `attribute` in basis points, within
    /// ±[`MAX_PHYSIQUE_MODIFIER_BP`].
    #[must_use]
    pub fn modifier_bp(&self, attribute: PhysiqueAttribute) -> i32 {
        let column = attribute as usize;
        let numerator: i64 = PhysiqueParam::ALL
            .iter()
            .map(|&param| {
                let deviation = i64::from(self.get(param)) - i64::from(PHYSIQUE_NEUTRAL);
                i64::from(PHYSIQUE_COEFFICIENTS[param.index()][column]) * deviation
            })
            .sum();
        let combined = numerator / i64::from(PHYSIQUE_NEUTRAL);
        let bound = i64::from(MAX_PHYSIQUE_MODIFIER_BP);
        // Clamped to ±200 above.
        #[allow(clippy::cast_possible_truncation)]
        {
            combined.clamp(-bound, bound) as i32
        }
    }

    /// `value` adjusted by this physique's `attribute` modifier, rounded to nearest.
    #[must_use]
    pub fn apply(&self, attribute: PhysiqueAttribute, value: u32) -> u32 {
        scale(value, self.modifier_bp(attribute))
    }
}

/// `value × (1 + bp / 10 000)`, rounded half up; never negative.
#[must_use]
pub(crate) fn scale(value: u32, bp: i32) -> u32 {
    let scaled = (i64::from(value) * (BP_SCALE + i64::from(bp)) + BP_SCALE / 2) / BP_SCALE;
    u32::try_from(scaled.max(0)).unwrap_or(u32::MAX)
}

/// The largest value any physique gives `value`: the +2 % bound. Bounds recovered amounts.
#[must_use]
pub(crate) fn most(value: u32) -> u32 {
    scale(value, MAX_PHYSIQUE_MODIFIER_BP)
}

/// A player's maximum health: level health plus the equipped stamina's bonus, adjusted by
/// physique. The one definition live players, records and snapshots all use.
#[must_use]
pub(crate) fn max_health(level: u8, equipment: &crate::Equipment, physique: &Physique) -> u32 {
    physique.apply(
        PhysiqueAttribute::MaxHealth,
        crate::unit::player_max_health(level) + equipment.totals().bonus_health(),
    )
}

impl crate::ZoneSimulation {
    /// The explicit physique customization (#633): replaces `player_id`'s eleven sliders with
    /// `physique`, already validated by construction. Stat effects are recomputed from the new
    /// values exactly once (the previous sliders leave nothing behind); current health follows the
    /// change of maximum as an equipment change does, and a dead player stays dead. V1 has no cost
    /// or cooldown, and collision, reach and timing are unchanged.
    pub fn customize_physique(
        &mut self,
        player_id: crate::PlayerId,
        physique: Physique,
    ) -> Result<(), crate::ZoneError> {
        let player = self
            .players
            .get_mut(&player_id)
            .ok_or_else(|| crate::ZoneError::new("unknown player"))?;
        let old_max = player.max_health();
        player.physique = physique;
        if player.is_alive() {
            player.health =
                crate::equipment::follow_max_health(player.health, old_max, player.max_health());
        }
        Ok(())
    }

    /// `player_id`'s physique sliders.
    #[must_use]
    pub fn player_physique(&self, player_id: crate::PlayerId) -> Option<Physique> {
        self.players.get(&player_id).map(|player| player.physique)
    }
}

impl crate::CharacterRecord {
    /// The same customization for a stored character that is not in a zone, such as one edited
    /// at character selection; the caller commits the record under the character's lease.
    pub fn customize_physique(&mut self, physique: Physique) {
        let old_max = max_health(self.level, &self.equipment, &self.physique);
        self.physique = physique;
        if self.health > 0 {
            let new_max = max_health(self.level, &self.equipment, &self.physique);
            self.health = crate::equipment::follow_max_health(self.health, old_max, new_max);
        }
    }
}

/// The six physique presets. A preset is only an exact slider vector: choosing one sets those
/// values, and nothing records which preset was chosen.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum PhysiquePreset {
    Average,
    Lean,
    Athletic,
    Muscular,
    Stocky,
    Heavyset,
}

impl PhysiquePreset {
    pub const ALL: [Self; 6] = [
        Self::Average,
        Self::Lean,
        Self::Athletic,
        Self::Muscular,
        Self::Stocky,
        Self::Heavyset,
    ];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Average => "Average",
            Self::Lean => "Lean",
            Self::Athletic => "Athletic",
            Self::Muscular => "Muscular",
            Self::Stocky => "Stocky",
            Self::Heavyset => "Heavyset",
        }
    }

    /// The preset's exact values: frame, muscularity, body fat, shoulder width and height; the
    /// six cosmetic sliders stay neutral.
    #[must_use]
    pub const fn physique(self) -> Physique {
        let stats: [u16; PHYSIQUE_STAT_PARAMS] = match self {
            Self::Average => [5_000, 5_000, 5_000, 5_000, 5_000],
            Self::Lean => [3_500, 4_000, 2_000, 4_500, 5_500],
            Self::Athletic => [5_000, 6_500, 3_500, 6_000, 5_500],
            Self::Muscular => [6_000, 8_500, 4_000, 7_500, 5_500],
            Self::Stocky => [7_000, 6_000, 6_000, 6_500, 3_500],
            Self::Heavyset => [7_500, 5_000, 8_500, 6_000, 5_000],
        };
        let mut values = [PHYSIQUE_NEUTRAL; PHYSIQUE_PARAMS];
        let mut index = 0;
        while index < PHYSIQUE_STAT_PARAMS {
            values[index] = stats[index];
            index += 1;
        }
        Physique { values }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extremes() -> impl Iterator<Item = [u16; PHYSIQUE_PARAMS]> {
        // Every corner of the five stat sliders, with the cosmetic six at their own corners too.
        (0..1u32 << PHYSIQUE_PARAMS).map(|bits| {
            let mut values = [0; PHYSIQUE_PARAMS];
            for (index, value) in values.iter_mut().enumerate() {
                *value = if bits >> index & 1 == 1 {
                    PHYSIQUE_SCALE
                } else {
                    0
                };
            }
            values
        })
    }

    #[test]
    fn neutral_changes_nothing() {
        for attribute in PhysiqueAttribute::ALL {
            assert_eq!(Physique::NEUTRAL.modifier_bp(attribute), 0);
            for value in [0, 1, 50, 937, 1_000_000] {
                assert_eq!(Physique::NEUTRAL.apply(attribute, value), value);
            }
        }
        assert_eq!(Physique::default(), Physique::NEUTRAL);
    }

    #[test]
    fn extreme_and_combined_sliders_stay_within_two_percent() {
        let mut reached = [false; 2];
        for values in extremes() {
            let physique = Physique::from_fixed(values).unwrap();
            for attribute in PhysiqueAttribute::ALL {
                let bp = physique.modifier_bp(attribute);
                assert!(bp.abs() <= MAX_PHYSIQUE_MODIFIER_BP, "{values:?} {bp}");
                reached[0] |= bp == MAX_PHYSIQUE_MODIFIER_BP;
                reached[1] |= bp == -MAX_PHYSIQUE_MODIFIER_BP;
                let health = physique.apply(attribute, 1_000);
                assert!((980..=1_020).contains(&health), "{health}");
            }
        }
        assert_eq!(reached, [true, true], "the clamp is exercised");
    }

    #[test]
    fn cosmetic_sliders_never_change_stats() {
        for preset in PhysiquePreset::ALL {
            let base = preset.physique();
            for values in extremes() {
                let mut fixed = base.fixed();
                fixed[PHYSIQUE_STAT_PARAMS..].copy_from_slice(&values[PHYSIQUE_STAT_PARAMS..]);
                let varied = Physique::from_fixed(fixed).unwrap();
                for attribute in PhysiqueAttribute::ALL {
                    assert_eq!(varied.modifier_bp(attribute), base.modifier_bp(attribute));
                }
            }
        }
        for param in PhysiqueParam::ALL {
            if !param.affects_stats() {
                assert_eq!(
                    PHYSIQUE_COEFFICIENTS[param.index()],
                    [0; PHYSIQUE_ATTRIBUTES]
                );
            }
        }
    }

    #[test]
    fn presets_are_exact_vectors_and_average_is_neutral() {
        assert_eq!(PhysiquePreset::Average.physique(), Physique::NEUTRAL);
        assert_eq!(
            PhysiquePreset::Muscular.physique().fixed(),
            [
                6_000, 8_500, 4_000, 7_500, 5_500, 5_000, 5_000, 5_000, 5_000, 5_000, 5_000
            ]
        );
        // Choosing presets in turn keeps nothing: back at Average the numbers are the original.
        let mut current = Physique::NEUTRAL;
        for preset in PhysiquePreset::ALL.iter().chain(&[PhysiquePreset::Average]) {
            current = preset.physique();
        }
        assert_eq!(current.apply(PhysiqueAttribute::MaxHealth, 935), 935);
    }

    #[test]
    fn normalized_values_are_validated() {
        let mut values = [0.5; PHYSIQUE_PARAMS];
        assert_eq!(Physique::from_normalized(values), Ok(Physique::NEUTRAL));
        values[3] = 1.0;
        values[9] = 0.0;
        let physique = Physique::from_normalized(values).unwrap();
        assert_eq!(physique.get(PhysiqueParam::ShoulderWidth), PHYSIQUE_SCALE);
        assert_eq!(physique.normalized(PhysiqueParam::JawWidth), 0.0);
        for bad in [f64::NAN, f64::INFINITY, -0.01, 1.01] {
            values[10] = bad;
            assert_eq!(Physique::from_normalized(values), Err(InvalidPhysique));
        }
        assert!(Physique::from_fixed([PHYSIQUE_SCALE + 1; PHYSIQUE_PARAMS]).is_err());
        assert_eq!(PhysiqueParam::JawProjection.key(), "jawProjection");
    }

    #[test]
    fn scaling_rounds_to_nearest() {
        assert_eq!(scale(50, 200), 51);
        assert_eq!(scale(50, -200), 49);
        assert_eq!(scale(6, 200), 6);
        assert_eq!(most(935), 954);
    }
}
