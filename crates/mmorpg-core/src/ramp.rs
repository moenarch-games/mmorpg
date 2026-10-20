//! Ramps (#59): content-authored wedges that units walk up and down.
//!
//! A ramp is a right triangular prism filling the lower half of its bounding box along one
//! horizontal axis: a flat bottom, a vertical back face at the high end, and a sloped top that
//! rises from the ground at the low end to the box's full height at the high end. Its slope is
//! `atan(height / run)`. Physics owns everything a ramp does to a unit: the character
//! controller climbs slopes up to 45° and slides down steeper ones; gameplay only places them.

use crate::ZoneError;

/// Ramps a zone's content may author.
pub const MAX_RAMPS: usize = 32;

/// The horizontal direction a ramp's top rises toward.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum RampRise {
    PosX,
    NegX,
    PosZ,
    NegZ,
}

impl RampRise {
    /// Stable code for fingerprints and exports.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::PosX => 0,
            Self::NegX => 1,
            Self::PosZ => 2,
            Self::NegZ => 3,
        }
    }

    /// The unit direction `(x, z)` the top rises toward.
    #[must_use]
    pub const fn direction(self) -> [i32; 2] {
        match self {
            Self::PosX => [1, 0],
            Self::NegX => [-1, 0],
            Self::PosZ => [0, 1],
            Self::NegZ => [0, -1],
        }
    }
}

/// One authored ramp: its physics body (in the static collider namespace), its bounding box
/// and the direction its top rises toward.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Ramp {
    pub body: u32,
    /// Bounding box centre and half extents, in units.
    pub position: [i32; 3],
    pub half_extents: [i32; 3],
    pub rises: RampRise,
}

impl Ramp {
    /// The slope in degrees, rounded: the box height over its run.
    #[must_use]
    pub fn slope_degrees(&self) -> f64 {
        let run = match self.rises {
            RampRise::PosX | RampRise::NegX => self.half_extents[0],
            RampRise::PosZ | RampRise::NegZ => self.half_extents[2],
        };
        f64::from(self.half_extents[1])
            .atan2(f64::from(run))
            .to_degrees()
    }

    /// The top's height (units) above the ramp's bottom at horizontal `(x, z)`, or `None`
    /// outside its footprint.
    #[must_use]
    pub fn top_at(&self, x: i32, z: i32) -> Option<f64> {
        let [cx, _, cz] = self.position;
        let [hx, hy, hz] = self.half_extents;
        if (x - cx).abs() > hx || (z - cz).abs() > hz {
            return None;
        }
        let [dx, dz] = self.rises.direction();
        // Along the rise, from -1 at the low end to +1 at the high end.
        let along = if dx == 0 {
            f64::from((z - cz) * dz) / f64::from(hz)
        } else {
            f64::from((x - cx) * dx) / f64::from(hx)
        };
        Some(f64::from(hy) * (along + 1.0))
    }

    pub(crate) fn validate(&self) -> Result<(), ZoneError> {
        let in_range = (0..3).all(|axis| {
            (i64::from(self.position[axis]) - i64::from(self.half_extents[axis])).abs()
                <= i64::from(crate::MAX_CONTENT_COORDINATE_UNITS)
                && (i64::from(self.position[axis]) + i64::from(self.half_extents[axis])).abs()
                    <= i64::from(crate::MAX_CONTENT_COORDINATE_UNITS)
        });
        if self.half_extents.iter().any(|half| *half <= 0) || !in_range {
            return Err(ZoneError::new("ramp box is empty or out of range"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ramp_rises_across_its_box_toward_its_direction() {
        let ramp = Ramp {
            body: 1,
            position: [0, 50, 0],
            half_extents: [200, 50, 100],
            rises: RampRise::NegX,
        };
        assert!((ramp.slope_degrees() - 14.04).abs() < 0.01);
        assert_eq!(ramp.top_at(200, 0), Some(0.0));
        assert_eq!(ramp.top_at(0, 0), Some(50.0));
        assert_eq!(ramp.top_at(-200, 50), Some(100.0));
        assert_eq!(ramp.top_at(201, 0), None);
        let north = Ramp {
            rises: RampRise::PosZ,
            half_extents: [100, 50, 50],
            ..ramp
        };
        assert_eq!(north.top_at(0, -50), Some(0.0));
        assert_eq!(north.top_at(0, 50), Some(100.0));
        assert!((north.slope_degrees() - 45.0).abs() < 1e-9);
    }
}
