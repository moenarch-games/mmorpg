//! Surface hazards (#294, Atlas 5.2.i294): patches of ground that change how a unit standing on
//! them moves: slippery ice and slowing tar.
//!
//! The effect contract is [`SurfaceKind::effect`] and the single lookup [`under_feet`]. Today a
//! zone's patches are authored content, in stable ID order, so they need no canonical state: a
//! unit's velocity already lives in canonical snapshots. An ability-placed patch (Frost Patch,
//! thrown tar) reuses the same [`SurfacePatch`] and effects; its zone state only has to add a
//! timed list consulted beside the content's.

use crate::ZoneError;

/// At most this many authored surface patches in one zone.
pub const MAX_SURFACE_PATCHES: usize = 32;

/// How much a unit's velocity can change per tick on ice (subunits per tick², about 1.5 s to run
/// up or stop from a full run).
pub const ICE_GRIP_SUBUNITS_PER_TICK: i32 = 30;
/// Tar leaves a unit this share of its running speed.
pub const TAR_SPEED_PERCENT: i32 = 45;

/// What a hazard patch is made of. The wire and identity code is stable.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, strum::VariantArray)]
pub enum SurfaceKind {
    /// Slippery: the velocity changes only slowly, so a unit slides.
    Ice,
    /// Sticky: a unit moves at [`TAR_SPEED_PERCENT`] of its speed.
    Tar,
}

impl SurfaceKind {
    /// Stable identity code.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Ice => 0,
            Self::Tar => 1,
        }
    }

    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Ice),
            1 => Some(Self::Tar),
            _ => None,
        }
    }

    /// What the surface does to a unit standing on it.
    #[must_use]
    pub const fn effect(self) -> SurfaceEffect {
        match self {
            Self::Ice => SurfaceEffect {
                speed_percent: 100,
                grip: Some(ICE_GRIP_SUBUNITS_PER_TICK),
            },
            Self::Tar => SurfaceEffect {
                speed_percent: TAR_SPEED_PERCENT,
                grip: None,
            },
        }
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Ice => "Ice",
            Self::Tar => "Tar",
        }
    }
}

/// The contract a surface hazard presents to movement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SurfaceEffect {
    /// Speed relative to normal movement; 100 leaves it unchanged.
    pub speed_percent: i32,
    /// When set, the velocity can change by at most this much per tick (subunits per tick²).
    pub grip: Option<i32>,
}

/// One patch of a hazard: an ellipse in XZ over a band of ground heights, with a stable ID.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SurfacePatch {
    pub id: u32,
    pub kind: SurfaceKind,
    /// Ellipse centre in XZ units.
    pub centre: [i32; 2],
    /// Ellipse radii in XZ units.
    pub radii: [i32; 2],
    /// Inclusive band of feet heights (units) it covers.
    pub height: [i32; 2],
}

impl SurfacePatch {
    /// Whether feet at `feet` stand on the patch.
    #[must_use]
    pub fn covers(&self, feet: [i32; 3]) -> bool {
        if feet[1] < self.height[0] || feet[1] > self.height[1] {
            return false;
        }
        let dx = i128::from(feet[0]) - i128::from(self.centre[0]);
        let dz = i128::from(feet[2]) - i128::from(self.centre[1]);
        let (rx, rz) = (i128::from(self.radii[0]), i128::from(self.radii[1]));
        dx * dx * rz * rz + dz * dz * rx * rx <= rx * rx * rz * rz
    }
}

/// The surface under feet at `feet`, if any: the lowest-ID patch that covers it.
#[must_use]
pub fn under_feet(patches: &[SurfacePatch], feet: [i32; 3]) -> Option<SurfaceKind> {
    patches
        .iter()
        .find(|patch| patch.covers(feet))
        .map(|patch| patch.kind)
}

/// The movement speed percent of a unit standing on `surface`.
#[must_use]
pub const fn speed_percent(surface: Option<SurfaceKind>) -> i32 {
    match surface {
        Some(kind) => kind.effect().speed_percent,
        None => 100,
    }
}

/// The velocity change a slippery `surface` allows per tick.
#[must_use]
pub const fn grip(surface: Option<SurfaceKind>) -> Option<i32> {
    match surface {
        Some(kind) => kind.effect().grip,
        None => None,
    }
}

/// The velocity (subunits per tick) a unit keeps after one tick on a surface with `grip`: `wanted`
/// when within `grip` of `last`, otherwise `last` moved `grip` toward `wanted`.
#[must_use]
pub fn slip(last: [i32; 2], wanted: [i32; 2], grip: i32) -> [i32; 2] {
    let delta = [
        i64::from(wanted[0]) - i64::from(last[0]),
        i64::from(wanted[1]) - i64::from(last[1]),
    ];
    let length = (delta[0] * delta[0] + delta[1] * delta[1]).isqrt();
    if length <= i64::from(grip) || length == 0 {
        return wanted;
    }
    let scaled = |axis: i64, from: i32| {
        i32::try_from(i64::from(from) + axis * i64::from(grip) / length).unwrap_or(from)
    };
    [scaled(delta[0], last[0]), scaled(delta[1], last[1])]
}

/// A zone's patches: at most [`MAX_SURFACE_PATCHES`], ordered by ID, each sound.
pub fn validate(patches: &[SurfacePatch]) -> Result<(), ZoneError> {
    if patches.len() > MAX_SURFACE_PATCHES {
        return Err(ZoneError::new("zone surface patch capacity reached"));
    }
    if patches.windows(2).any(|pair| pair[0].id >= pair[1].id) {
        return Err(ZoneError::new("surface patches need ordered unique IDs"));
    }
    let limit = crate::MAX_CONTENT_COORDINATE_UNITS;
    for patch in patches {
        if patch.radii.iter().any(|&radius| radius <= 0)
            || patch.height[0] > patch.height[1]
            || patch
                .centre
                .iter()
                .chain(&patch.height)
                .any(|value| value.abs() > limit)
        {
            return Err(ZoneError::new("surface patch is invalid"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TAR: SurfacePatch = SurfacePatch {
        id: 1,
        kind: SurfaceKind::Tar,
        centre: [0, 0],
        radii: [100, 50],
        height: [-20, 20],
    };

    #[test]
    fn a_patch_is_an_ellipse_over_a_band_of_heights() {
        assert!(TAR.covers([100, 0, 0]));
        assert!(!TAR.covers([101, 0, 0]));
        assert!(TAR.covers([0, 20, 50]));
        assert!(!TAR.covers([0, 21, 0]));
        assert_eq!(under_feet(&[TAR], [0, 0, 0]), Some(SurfaceKind::Tar));
        assert_eq!(under_feet(&[TAR], [0, 0, 60]), None);
    }

    #[test]
    fn ice_limits_velocity_change_and_slowing_needs_only_the_percent() {
        assert_eq!(slip([0, 0], [2_688, 0], 30), [30, 0]);
        assert_eq!(slip([2_688, 0], [0, 0], 30), [2_658, 0]);
        assert_eq!(slip([10, 0], [20, 0], 30), [20, 0]);
        assert_eq!(speed_percent(Some(SurfaceKind::Tar)), TAR_SPEED_PERCENT);
        assert_eq!(speed_percent(None), 100);
        assert_eq!(
            grip(Some(SurfaceKind::Ice)),
            Some(ICE_GRIP_SUBUNITS_PER_TICK)
        );
        assert_eq!(grip(Some(SurfaceKind::Tar)), None);
    }

    #[test]
    fn validation_requires_ordered_ids_and_sound_shapes() {
        assert!(validate(&[TAR]).is_ok());
        assert!(validate(&[TAR, TAR]).is_err());
        let thin = SurfacePatch {
            radii: [0, 50],
            ..TAR
        };
        assert!(validate(&[thin]).is_err());
    }
}
