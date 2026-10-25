//! Cosmetic swim attitude (Atlas 6.6.41): how far a swimming body leans into its stroke and turns
//! toward its travel. It follows the zone's water mode and the projected motion only, pivots the
//! drawn model about its chest and never moves the canonical root, decides where a unit swims or
//! feeds back into gameplay.
use super::kit::{Frame, offset, qa, qm, qr};
use mmorpg_core::WaterMode;
use std::f64::consts::{FRAC_PI_2, FRAC_PI_4};

/// Forward lean of a surface swimmer at full stroke: chest down, head above the water.
pub const SURFACE_PITCH: f64 = 70. * std::f64::consts::PI / 180.;
/// Forward lean of a submerged swimmer at full stroke: level in the water.
pub const SUBMERGED_PITCH: f64 = FRAC_PI_2;
/// The steepest head-down dive and head-up ascent a submerged body tilts by, from level.
pub const DIVE_DOWN: f64 = 50. * std::f64::consts::PI / 180.;
pub const DIVE_UP: f64 = 80. * std::f64::consts::PI / 180.;
/// The farthest a swimming body turns from its facing toward its travel.
pub const MAX_HEADING: f64 = FRAC_PI_4;
/// The travel speed in metres per second at which a swimmer leans fully into its stroke.
pub const FULL_LEAN_SPEED: f64 = 1.2;
/// Seconds the attitude takes to settle most of the way toward a new target.
pub const SETTLE_SECONDS: f64 = 0.25;
/// Where the body pivots, as a share of its height from the feet: the chest.
pub const PIVOT: f64 = 0.65;

/// A body's cosmetic swim attitude in radians: `pitch` leans it forward (positive) about its
/// chest, `heading` turns it from the unit's facing (positive toward its left, like yaw).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Attitude {
    pub pitch: f64,
    pub heading: f64,
}

impl Attitude {
    /// Upright and facing the unit's yaw.
    pub const UPRIGHT: Self = Self {
        pitch: 0.,
        heading: 0.,
    };

    /// The attitude a body in `water` settles toward while it travels at `speed` m/s
    /// horizontally, `direction` radians from its facing (the yaw offset of the travel), and
    /// `vertical` m/s. Out of swimming depth, or treading in place, it stands upright; a backward
    /// paddle stays upright too, rather than swimming feet first.
    #[must_use]
    pub fn target(water: WaterMode, speed: f64, direction: f64, vertical: f64) -> Self {
        if !water.swims() {
            return Self::UPRIGHT;
        }
        let direction = (direction + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU)
            - std::f64::consts::PI;
        let backward = direction.abs() > FRAC_PI_2 + 0.2;
        let heading = if backward || speed < 0.25 {
            0.
        } else {
            direction.clamp(-MAX_HEADING, MAX_HEADING)
        };
        // The horizontal travel along the turned body.
        let along = if backward {
            0.
        } else {
            (speed * (direction - heading).cos()).max(0.)
        };
        let submerged = water == WaterMode::Submerged;
        let vertical = if submerged { vertical } else { 0. };
        let travel = along.hypot(vertical);
        let lean = (travel / FULL_LEAN_SPEED).clamp(0., 1.);
        if lean <= 0. {
            return Self::UPRIGHT;
        }
        let level = if submerged {
            SUBMERGED_PITCH
        } else {
            SURFACE_PITCH
        };
        // Head down into a dive, head up out of one, bounded short of vertical either way.
        let dive = (-vertical).atan2(along).clamp(-DIVE_UP, DIVE_DOWN);
        Self {
            pitch: lean * (level + dive),
            heading: heading * lean,
        }
    }

    /// This attitude eased toward `target` over `dt` seconds; reduced motion takes it at once.
    #[must_use]
    pub fn settle(self, target: Self, dt: f64, reduced: bool) -> Self {
        if reduced {
            return target;
        }
        let k = 1. - (-dt.max(0.) / SETTLE_SECONDS).exp();
        Self {
            pitch: self.pitch + (target.pitch - self.pitch) * k,
            heading: self.heading + (target.heading - self.heading) * k,
        }
    }
}

/// The drawn model's root for a body standing at `root` (feet, turned by its yaw) and `height`
/// metres tall, leaned and turned by `attitude` about its chest. Only the drawn model moves; the
/// unit's projected position stays where the zone put it.
#[must_use]
pub fn lean(root: Frame, attitude: Attitude, height: f64) -> Frame {
    if attitude == Attitude::UPRIGHT {
        return root;
    }
    let chest = offset(root, [0., height * PIVOT, 0.]).p;
    let q = qm(root.q, qm(qa(1, attitude.heading), qa(0, attitude.pitch)));
    let lowered = qr(q, [0., -height * PIVOT, 0.]);
    Frame {
        p: std::array::from_fn(|i| chest[i] + lowered[i]),
        q,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dry_and_wading_bodies_stand_upright() {
        for water in [WaterMode::Dry, WaterMode::Wading] {
            assert_eq!(Attitude::target(water, 3., 0.5, -2.), Attitude::UPRIGHT);
        }
    }

    #[test]
    fn treading_and_paddling_backward_stay_upright() {
        for water in [WaterMode::Swimming, WaterMode::Submerged] {
            assert_eq!(Attitude::target(water, 0., 0., 0.), Attitude::UPRIGHT);
            assert_eq!(
                Attitude::target(water, 2., std::f64::consts::PI, 0.),
                Attitude::UPRIGHT
            );
        }
    }

    #[test]
    fn a_stroke_leans_in_by_depth_and_turns_toward_its_travel_within_bounds() {
        let surface = Attitude::target(WaterMode::Swimming, 2., 0., 0.);
        assert!((surface.pitch - SURFACE_PITCH).abs() < 1e-9);
        let level = Attitude::target(WaterMode::Submerged, 2., 0., 0.);
        assert!((level.pitch - SUBMERGED_PITCH).abs() < 1e-9);
        // A slow stroke leans less.
        assert!(Attitude::target(WaterMode::Swimming, 0.6, 0., 0.).pitch < surface.pitch);
        // Swimming sideways turns the body toward the travel, at most a quarter turn.
        let sideways = Attitude::target(WaterMode::Swimming, 2., FRAC_PI_2, 0.);
        assert!((sideways.heading - MAX_HEADING).abs() < 1e-9);
        let right = Attitude::target(WaterMode::Swimming, 2., -0.3, 0.);
        assert!((right.heading + 0.3).abs() < 1e-9);
        // Surface swimming ignores vertical motion; the zone keeps the swimmer at the surface.
        assert_eq!(Attitude::target(WaterMode::Swimming, 2., 0., -1.), surface);
    }

    #[test]
    fn diving_and_surfacing_tilt_the_head_down_and_up_without_flipping() {
        let down = Attitude::target(WaterMode::Submerged, 0., 0., -2.);
        assert!((down.pitch - (SUBMERGED_PITCH + DIVE_DOWN)).abs() < 1e-9);
        let up = Attitude::target(WaterMode::Submerged, 0., 0., 2.);
        assert!((up.pitch - (SUBMERGED_PITCH - DIVE_UP)).abs() < 1e-9);
        assert!(up.pitch > 0.);
        let angled = Attitude::target(WaterMode::Submerged, 2., 0., -2.);
        assert!(angled.pitch > SUBMERGED_PITCH && angled.pitch < down.pitch + 1e-9);
    }

    #[test]
    fn the_lean_pivots_the_drawn_model_about_its_chest() {
        let root = Frame {
            p: [3., 1., -2.],
            q: qa(1, 0.7),
        };
        assert_eq!(lean(root, Attitude::UPRIGHT, 1.8).p, root.p);
        let leaned = lean(
            root,
            Attitude {
                pitch: SURFACE_PITCH,
                heading: 0.3,
            },
            1.8,
        );
        let chest = |frame: Frame| offset(frame, [0., 1.8 * PIVOT, 0.]).p;
        for (a, b) in chest(leaned).iter().zip(chest(root)) {
            assert!((a - b).abs() < 1e-9);
        }
        // Leaning forward lifts the feet behind the chest and lowers the head ahead of it.
        let head = offset(leaned, [0., 1.8, 0.]).p;
        assert!(head[1] < 1. + 1.8 && leaned.p[1] > root.p[1]);
    }

    #[test]
    fn the_attitude_eases_toward_its_target() {
        let target = Attitude::target(WaterMode::Swimming, 2., 0., 0.);
        let mut attitude = Attitude::UPRIGHT;
        let mut previous = 0.;
        for _ in 0..30 {
            attitude = attitude.settle(target, 0.05, false);
            assert!(attitude.pitch > previous && attitude.pitch <= target.pitch);
            previous = attitude.pitch;
        }
        assert!((attitude.pitch - target.pitch).abs() < 0.01);
        assert_eq!(Attitude::UPRIGHT.settle(target, 0.05, true), target);
    }
}
