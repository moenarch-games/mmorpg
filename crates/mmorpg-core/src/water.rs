//! Authoritative water (#274, Atlas 5.1.41–5.1.42): authored water volumes, the swim
//! modes they put a unit in, and the breath meter with drowning damage.
//!
//! A volume is an axis-aligned ellipse in XZ with a flat surface height; the ground under
//! it is the zone's terrain, so its depth is the terrain's. A player whose feet are more
//! than [`SWIM_DEPTH_UNITS`] below the surface swims: gravity no longer applies, held rise
//! intent ascends or descends, and the surface holds them with their feet
//! [`FLOAT_DEPTH_UNITS`] down, head above water. A swimmer whose head is under the surface
//! is submerged and spends breath; out of it, breath refills. At zero breath a submerged
//! player takes [`DROWN_DAMAGE_PERCENT`] of their maximum health every
//! [`DROWN_INTERVAL_TICKS`] through the environmental damage path.
//!
//! The Sunken Shrine (#274 part 2): a volume may hold authored [`AirPocket`] boxes; a head
//! inside one is not submerged, so breath refills there. No combat reaches into, out of or
//! between submerged units (Atlas 5.1.43), and aquatic creatures swim like players.

use crate::{EntityRef, PLAYER_HALF_EXTENTS_UNITS, PlayerId, ZoneError};

/// Feet this far below the surface swim instead of wading (1.3 m).
pub const SWIM_DEPTH_UNITS: i32 = 130;
/// A swimmer at the surface floats with its feet this far down (1.4 m): head and shoulders out.
pub const FLOAT_DEPTH_UNITS: i32 = 140;
/// Swimmers move at this share of their running speed (3.9 m/s forward).
pub const SWIM_SPEED_PERCENT: i32 = 62;
/// Vertical speed of held ascent or descent (2.4 m/s).
pub const SWIM_RISE_SUBUNITS_PER_TICK: i32 = crate::subunits_per_tick(240);
/// A full breath lasts 30 s submerged.
pub const BREATH_MAX_TICKS: u16 = crate::ticks_from_ms(30_000);
/// Out of the water, or with the head above it, breath refills in 5 s.
pub const BREATH_REFILL_PER_TICK: u16 = 6;
/// At zero breath a submerged player loses this share of maximum health …
pub const DROWN_DAMAGE_PERCENT: u32 = 10;
/// … every this many ticks (1 s).
pub const DROWN_INTERVAL_TICKS: u16 = crate::ticks_from_ms(1_000);
/// At most this many water volumes per zone.
pub const MAX_WATER_VOLUMES: usize = 8;
/// An idle aquatic creature notices a submerged player this close (8 m) …
pub const TRAIL_NOTICE_UNITS: i32 = 800;
/// … follows it for this long (6 s), then loses interest and swims home.
pub const TRAIL_TICKS: u16 = crate::ticks_from_ms(6_000);
/// A trailing creature keeps this far from the player (2 m) and never touches it.
pub const TRAIL_KEEP_UNITS: i32 = 200;
/// A trailing creature swims at 3.3 m/s, slower than a swimming player (3.9 m/s).
pub const TRAIL_SPEED_SUBUNITS_PER_TICK: i32 = crate::subunits_per_tick(330);

/// The vertical velocity (subunits per tick) of a swimming creature whose feet are `depth` units
/// below the surface, heading for feet `goal` units down: never above the floating depth.
#[must_use]
pub fn creature_swim_velocity(depth: i32, goal: i32) -> i32 {
    (depth - goal.max(FLOAT_DEPTH_UNITS))
        .saturating_mul(SUBUNITS)
        .clamp(-SWIM_RISE_SUBUNITS_PER_TICK, SWIM_RISE_SUBUNITS_PER_TICK)
}

const SUBUNITS: i32 = crate::physics::SUBUNITS_PER_UNIT;

/// At most this many air pockets per water volume.
pub const MAX_AIR_POCKETS: usize = 4;

/// An authored breathable box inside a water volume (#274): a head inside it is not
/// submerged. Bounds are inclusive, in units.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AirPocket {
    pub min: [i32; 3],
    pub max: [i32; 3],
}

impl AirPocket {
    #[must_use]
    pub fn contains(&self, point: [i32; 3]) -> bool {
        (0..3).all(|axis| (self.min[axis]..=self.max[axis]).contains(&point[axis]))
    }
}

/// An authored body of water: an axis-aligned ellipse with a flat surface.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WaterVolume {
    pub name: String,
    /// Ellipse centre and radii in XZ units.
    pub centre: [i32; 2],
    pub radii: [i32; 2],
    /// Surface height in units.
    pub surface: i32,
    /// Breathable boxes under the surface (#274).
    pub air_pockets: Vec<AirPocket>,
}

impl WaterVolume {
    /// Whether `(x, z)` lies inside the ellipse (its boundary included).
    #[must_use]
    pub fn contains(&self, x: i32, z: i32) -> bool {
        let dx = i128::from(x) - i128::from(self.centre[0]);
        let dz = i128::from(z) - i128::from(self.centre[1]);
        let (rx, rz) = (i128::from(self.radii[0]), i128::from(self.radii[1]));
        dx * dx * rz * rz + dz * dz * rx * rx <= rx * rx * rz * rz
    }

    pub(crate) fn validate(&self) -> Result<(), ZoneError> {
        if self.name.is_empty() || self.radii.iter().any(|&radius| radius <= 0) {
            return Err(ZoneError::new(
                "water volumes need a name and positive radii",
            ));
        }
        if self.air_pockets.len() > MAX_AIR_POCKETS
            || self
                .air_pockets
                .iter()
                .any(|pocket| (0..3).any(|axis| pocket.min[axis] > pocket.max[axis]))
        {
            return Err(ZoneError::new("water volume air pockets are invalid"));
        }
        if self
            .centre
            .iter()
            .chain(&[self.surface])
            .any(|value| value.unsigned_abs() > crate::MAX_CONTENT_COORDINATE_UNITS.unsigned_abs())
        {
            return Err(ZoneError::new(
                "water volume lies outside the content coordinate range",
            ));
        }
        Ok(())
    }
}

/// Where a unit stands with respect to water.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, PartialOrd, Ord, Hash)]
pub enum WaterMode {
    /// Not in water.
    #[default]
    Dry,
    /// Feet in water, walking on the ground.
    Wading,
    /// Swimming with the head above the surface.
    Swimming,
    /// Swimming with the head under the surface: breath runs down.
    Submerged,
}

impl WaterMode {
    /// The mode of a player-sized unit whose feet are at `feet` (units).
    #[must_use]
    pub fn of_feet(waters: &[WaterVolume], feet: [i32; 3]) -> Self {
        Self::of_body(waters, feet, 2 * PLAYER_HALF_EXTENTS_UNITS[1])
    }

    /// The mode of a unit `height` units tall whose feet are at `feet` (units): the first
    /// volume over its feet decides, and a head inside any air pocket (#274) is not submerged.
    #[must_use]
    pub fn of_body(waters: &[WaterVolume], feet: [i32; 3], height: i32) -> Self {
        let Some(water) = waters.iter().find(|water| water.contains(feet[0], feet[2])) else {
            return Self::Dry;
        };
        let depth = water.surface - feet[1];
        let head = [feet[0], feet[1] + height, feet[2]];
        if depth <= 0 {
            Self::Dry
        } else if depth <= SWIM_DEPTH_UNITS {
            Self::Wading
        } else if depth > height
            && !waters
                .iter()
                .flat_map(|water| &water.air_pockets)
                .any(|pocket| pocket.contains(head))
        {
            Self::Submerged
        } else {
            Self::Swimming
        }
    }

    /// The mode of a player body centred at `centre` (units). Clients derive the modes of
    /// projected players this way from their authoritative positions and the content's
    /// volumes.
    #[must_use]
    pub fn of_player(waters: &[WaterVolume], centre: [i32; 3]) -> Self {
        Self::of_feet(
            waters,
            [
                centre[0],
                centre[1] - PLAYER_HALF_EXTENTS_UNITS[1],
                centre[2],
            ],
        )
    }

    /// Swimming, at the surface or under it.
    #[must_use]
    pub const fn swims(self) -> bool {
        matches!(self, Self::Swimming | Self::Submerged)
    }

    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Dry => 0,
            Self::Wading => 1,
            Self::Swimming => 2,
            Self::Submerged => 3,
        }
    }

    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Dry),
            1 => Some(Self::Wading),
            2 => Some(Self::Swimming),
            3 => Some(Self::Submerged),
            _ => None,
        }
    }
}

/// A player's breath and drowning clock (canonical).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Breath {
    /// Ticks of breath left, at most [`BREATH_MAX_TICKS`].
    pub remaining: u16,
    /// Ticks since the last drowning damage while out of breath and submerged.
    pub drowning: u16,
}

impl Default for Breath {
    fn default() -> Self {
        Self::FULL
    }
}

impl Breath {
    pub const FULL: Self = Self {
        remaining: BREATH_MAX_TICKS,
        drowning: 0,
    };

    #[must_use]
    pub const fn is_valid(self) -> bool {
        self.remaining <= BREATH_MAX_TICKS
            && self.drowning < DROWN_INTERVAL_TICKS
            && (self.drowning == 0 || self.remaining == 0)
    }

    /// One tick in `mode`; returns whether drowning damage is due this tick.
    pub(crate) fn advance(&mut self, mode: WaterMode) -> bool {
        if mode != WaterMode::Submerged {
            self.remaining = (self.remaining + BREATH_REFILL_PER_TICK).min(BREATH_MAX_TICKS);
            self.drowning = 0;
            return false;
        }
        if self.remaining > 0 {
            self.remaining -= 1;
            return false;
        }
        self.drowning += 1;
        if self.drowning >= DROWN_INTERVAL_TICKS {
            self.drowning = 0;
            return true;
        }
        false
    }
}

/// How far (units) below the surface the feet at `feet` are, if a player-sized unit there
/// swims.
#[must_use]
pub fn swim_depth(waters: &[WaterVolume], feet: [i32; 3]) -> Option<i32> {
    if !WaterMode::of_feet(waters, feet).swims() {
        return None;
    }
    let water = waters
        .iter()
        .find(|water| water.contains(feet[0], feet[2]))?;
    Some(water.surface - feet[1])
}

/// The vertical velocity (subunits per tick) of a swimmer whose feet are `depth` units below the
/// surface, holding `rise` (-1 down, 0 level, 1 up): never above the floating depth.
#[must_use]
pub fn swim_vertical_velocity(depth: i32, rise: i8) -> i32 {
    let toward_float = (depth - FLOAT_DEPTH_UNITS).saturating_mul(SUBUNITS);
    match rise {
        r if r > 0 => SWIM_RISE_SUBUNITS_PER_TICK.min(toward_float.max(0)),
        r if r < 0 => -SWIM_RISE_SUBUNITS_PER_TICK,
        // Level: hold depth, except that feet above the floating depth sink to it.
        _ if toward_float < 0 => toward_float.max(-SWIM_RISE_SUBUNITS_PER_TICK),
        _ => 0,
    }
}

impl crate::ZoneSimulation {
    /// How deep (units) the water at `(x, z)` is over the ground, if any volume covers it.
    pub(crate) fn water_depth_at(&self, x: i32, z: i32) -> Option<i32> {
        let water = self
            .content()
            .waters()
            .iter()
            .find(|water| water.contains(x, z))?;
        Some(water.surface - self.ground_y(x, z))
    }

    /// `velocity` (per tick, XZ), unless its step leads a non-swimming creature into deeper
    /// water than it can wade: then it stands. Moving toward shallower water stays allowed.
    pub(crate) fn keep_out_of_deep_water(&self, here: [i32; 2], velocity: [i32; 2]) -> [i32; 2] {
        if velocity == [0, 0] || self.content().waters().is_empty() {
            return velocity;
        }
        // Where the next tick's step (subunits per tick) leads, in whole units.
        let to = [
            here[0] + velocity[0] / SUBUNITS,
            here[1] + velocity[1] / SUBUNITS,
        ];
        match self.water_depth_at(to[0], to[1]) {
            Some(depth)
                if depth > SWIM_DEPTH_UNITS
                    && depth > self.water_depth_at(here[0], here[1]).unwrap_or(0) =>
            {
                [0, 0]
            }
            _ => velocity,
        }
    }

    /// The vertical velocity of an aquatic creature `height` tall with its feet at `feet`, if
    /// it swims there (#274): toward feet at `goal` (units, absolute) when it follows a
    /// player, else up to the floating depth.
    pub(crate) fn creature_swim(
        &self,
        feet: [i32; 3],
        height: i32,
        goal: Option<i32>,
    ) -> Option<i32> {
        let waters = self.content().waters();
        if !WaterMode::of_body(waters, feet, height).swims() {
            return None;
        }
        let surface = waters
            .iter()
            .find(|water| water.contains(feet[0], feet[2]))?
            .surface;
        let goal = goal.map_or(FLOAT_DEPTH_UNITS, |goal| surface - goal);
        Some(creature_swim_velocity(surface - feet[1], goal))
    }

    /// The water mode of a player body centred at `centre`.
    pub(crate) fn player_water_mode(&self, centre: crate::physics::Vec3i) -> WaterMode {
        WaterMode::of_player(self.content().waters(), [centre.x, centre.y, centre.z])
    }

    /// The water mode of a unit's body; NPCs and missing bodies are dry.
    pub(crate) fn unit_water_mode(&self, unit: EntityRef) -> WaterMode {
        if self.content().waters().is_empty() || matches!(unit, EntityRef::Npc(_)) {
            return WaterMode::Dry;
        }
        let Some(body) = self.world.body(crate::entity::body_id(unit)) else {
            return WaterMode::Dry;
        };
        let (centre, half) = (body.position(), body.half_extents());
        WaterMode::of_body(
            self.content().waters(),
            [centre.x, centre.y - half.y, centre.z],
            2 * half.y,
        )
    }

    /// Whether a unit is under water (#274).
    pub(crate) fn submerged(&self, unit: EntityRef) -> bool {
        self.unit_water_mode(unit) == WaterMode::Submerged
    }

    /// No combat into, out of or between submerged units (#274, Atlas 5.1.43).
    pub(crate) fn combat_blocked(&self, source: EntityRef, target: EntityRef) -> bool {
        self.submerged(source) || self.submerged(target)
    }

    /// The nearest submerged living player within [`TRAIL_NOTICE_UNITS`] of `here` (3D),
    /// ties to the lowest ID: what an idle aquatic creature trails (#274).
    pub(crate) fn trail_candidate(&self, here: [i32; 3]) -> Result<Option<PlayerId>, ZoneError> {
        let mut best: Option<(i64, PlayerId)> = None;
        for entity in self.interest.near(here[0], here[2], TRAIL_NOTICE_UNITS) {
            let EntityRef::Player(player_id) = entity else {
                continue;
            };
            if !self
                .players
                .get(&player_id)
                .is_some_and(|player| player.is_alive())
                || !self.submerged(entity)
            {
                continue;
            }
            let at = self.player_position(player_id)?;
            let distance = [at.x - here[0], at.y - here[1], at.z - here[2]]
                .iter()
                .map(|&axis| i64::from(axis).pow(2))
                .sum::<i64>();
            if distance <= i64::from(TRAIL_NOTICE_UNITS).pow(2)
                && best.is_none_or(|best| (distance, player_id) < best)
            {
                best = Some((distance, player_id));
            }
        }
        Ok(best.map(|(_, player_id)| player_id))
    }

    /// After the physics step: each living player's breath advances in its water mode, and
    /// an exhausted submerged player takes drowning damage (Atlas 5.1.42).
    pub(crate) fn update_breath(&mut self) -> Result<(), ZoneError> {
        if self.content().waters().is_empty() {
            return Ok(());
        }
        let ids: Vec<_> = self.players.keys().copied().collect();
        for player_id in ids {
            let centre = self.player_position(player_id)?;
            let mode = self.player_water_mode(centre);
            let Some(player) = self.players.get_mut(&player_id) else {
                continue;
            };
            if !player.is_alive() {
                player.breath = Breath::FULL;
                continue;
            }
            if player.breath.advance(mode) {
                let damage = (player.max_health() * DROWN_DAMAGE_PERCENT / 100).max(1);
                self.apply_environmental_damage(player_id, damage)?;
                if let Some(player) = self.players.get_mut(&player_id)
                    && !player.is_alive()
                {
                    player.breath = Breath::FULL;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lake() -> Vec<WaterVolume> {
        vec![WaterVolume {
            name: "lake".into(),
            centre: [0, 0],
            radii: [1_000, 500],
            surface: 15,
            air_pockets: vec![AirPocket {
                min: [300, -400, -100],
                max: [500, -200, 100],
            }],
        }]
    }

    #[test]
    fn modes_follow_feet_depth() {
        let waters = lake();
        assert_eq!(WaterMode::of_feet(&waters, [2_000, 0, 0]), WaterMode::Dry);
        assert_eq!(WaterMode::of_feet(&waters, [0, 0, 0]), WaterMode::Wading);
        assert_eq!(
            WaterMode::of_feet(&waters, [0, -150, 0]),
            WaterMode::Swimming
        );
        assert_eq!(
            WaterMode::of_feet(&waters, [0, 15 - 140, 0]),
            WaterMode::Swimming
        );
        assert_eq!(
            WaterMode::of_feet(&waters, [0, -400, 0]),
            WaterMode::Submerged
        );
        assert!(!waters[0].contains(0, 501));
        // A head inside the air pocket breathes.
        assert_eq!(
            WaterMode::of_feet(&waters, [400, -500, 0]),
            WaterMode::Swimming
        );
        assert_eq!(
            WaterMode::of_feet(&waters, [700, -500, 0]),
            WaterMode::Submerged
        );
    }

    #[test]
    fn breath_drains_refills_and_drowns() {
        let mut breath = Breath::FULL;
        for _ in 0..BREATH_MAX_TICKS {
            assert!(!breath.advance(WaterMode::Submerged));
        }
        assert_eq!(breath.remaining, 0);
        let due = (0..DROWN_INTERVAL_TICKS)
            .filter(|_| breath.advance(WaterMode::Submerged))
            .count();
        assert_eq!(due, 1);
        assert!(breath.is_valid());
        assert!(!breath.advance(WaterMode::Swimming));
        assert_eq!(
            breath,
            Breath {
                remaining: BREATH_REFILL_PER_TICK,
                drowning: 0
            }
        );
    }

    #[test]
    fn swimmers_float_and_never_rise_above_the_surface_hold() {
        assert_eq!(swim_vertical_velocity(FLOAT_DEPTH_UNITS, 1), 0);
        assert_eq!(
            swim_vertical_velocity(FLOAT_DEPTH_UNITS + 3, 1),
            3 * SUBUNITS
        );
        assert_eq!(swim_vertical_velocity(400, 1), SWIM_RISE_SUBUNITS_PER_TICK);
        assert_eq!(
            swim_vertical_velocity(400, -1),
            -SWIM_RISE_SUBUNITS_PER_TICK
        );
        assert_eq!(swim_vertical_velocity(400, 0), 0);
        assert_eq!(
            swim_vertical_velocity(FLOAT_DEPTH_UNITS - 1, 0),
            -SUBUNITS,
            "feet above the floating depth sink to it"
        );
        assert_eq!(swim_vertical_velocity(0, 0), -SWIM_RISE_SUBUNITS_PER_TICK);
    }
}
