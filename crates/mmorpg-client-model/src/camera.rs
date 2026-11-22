//! The third-person shoulder camera both clients orbit around the local character (#1058): look
//! and zoom limits, smooth zoom, enemy-focus following, the ground floor under the eye and the
//! sight line's pull-in. Presentation only: its heading becomes movement intent, never an
//! authoritative position. Each client keeps its input and render adapters.
//!
//! Angles are radians in the world yaw convention (0 looks toward +Z, increasing toward +X);
//! lengths are metres.

use std::f64::consts::TAU;

/// The camera pivots around the character's upper body, not its feet.
pub const PIVOT_HEIGHT_METRES: f64 = 0.6;
/// The eye stays at least this far above the ground under it.
pub const GROUND_CLEARANCE_METRES: f64 = 0.35;
pub const MIN_DISTANCE_METRES: f64 = 2.;
pub const MAX_DISTANCE_METRES: f64 = 8.;
/// Allows looking up and down while staying short of the vertical singularity.
pub const MIN_PITCH_RADIANS: f64 = -1.;
pub const MAX_PITCH_RADIANS: f64 = 1.2;
pub const ORBIT_RADIANS_PER_PIXEL: f64 = 0.006;
/// Each wheel line changes the distance by this factor.
pub const ZOOM_STEP: f64 = 0.88;
/// Smooth zoom covers about 1 − e^(−rate × seconds) of the remaining distance.
pub const ZOOM_RATE_PER_SECOND: f64 = 12.;
pub const SHOULDER_OFFSET_METRES: f64 = 0.65;
const FOCUS_TURN_RATE: f64 = 10.;
const FOCUS_FRAMING_RATE: f64 = 7.;
const FOCUS_FRAMING_METRES: f64 = 3.;
/// How far in front of the first blocking surface a pulled-in eye stays.
pub const SIGHT_CLEARANCE_METRES: f64 = 0.3;
const START_DISTANCE_METRES: f64 = 3.5;

/// Eye and look-at point of one view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub eye: [f64; 3],
    pub target: [f64; 3],
}

/// An axis-aligned box the sight line may not pass through.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SightBox {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

/// The nearest yaw step (`u16`, 65 536 a turn) for an angle in radians.
#[must_use]
pub fn yaw_from_radians(radians: f64) -> u16 {
    let turns = (radians / TAU).rem_euclid(1.);
    // A full turn rounds to 65 536, which wraps to yaw 0.
    ((turns * 65_536.).round() as u32 % 65_536) as u16
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrbitCamera {
    look_sensitivity: f64,
    invert_vertical_look: bool,
    yaw: f64,
    pitch: f64,
    distance: f64,
    target_distance: f64,
    framing: [f64; 3],
}

impl Default for OrbitCamera {
    /// Starts behind a character facing yaw 0.
    fn default() -> Self {
        Self {
            look_sensitivity: 1.,
            invert_vertical_look: false,
            yaw: 0.,
            pitch: 0.12,
            distance: START_DISTANCE_METRES,
            target_distance: START_DISTANCE_METRES,
            framing: [0.; 3],
        }
    }
}

impl OrbitCamera {
    /// Already validated look preferences: pixel sensitivity and vertical inversion.
    pub fn set_look(&mut self, sensitivity: f64, invert_vertical_look: bool) {
        if sensitivity.is_finite() && sensitivity > 0. {
            self.look_sensitivity = sensitivity;
        }
        self.invert_vertical_look = invert_vertical_look;
    }

    /// Mouse drag: moving right turns the view right; moving down raises the eye.
    pub fn orbit(&mut self, dx_pixels: f64, dy_pixels: f64) {
        if !dx_pixels.is_finite() || !dy_pixels.is_finite() {
            return;
        }
        let scale = ORBIT_RADIANS_PER_PIXEL * self.look_sensitivity;
        let dy_pixels = if self.invert_vertical_look {
            -dy_pixels
        } else {
            dy_pixels
        };
        self.yaw = (self.yaw - dx_pixels * scale).rem_euclid(TAU);
        self.pitch = (self.pitch + dy_pixels * scale).clamp(MIN_PITCH_RADIANS, MAX_PITCH_RADIANS);
    }

    /// Positive wheel lines zoom in; the view follows smoothly in [`Self::update`].
    pub fn zoom(&mut self, lines: f64) {
        if lines.is_finite() {
            self.target_distance = (self.target_distance * ZOOM_STEP.powf(lines))
                .clamp(MIN_DISTANCE_METRES, MAX_DISTANCE_METRES);
        }
    }

    /// A pinch: spreading the fingers by `scale` brings the camera that much closer.
    pub fn zoom_by(&mut self, scale: f64) {
        if scale.is_finite() && scale > 0. {
            self.target_distance =
                (self.target_distance / scale).clamp(MIN_DISTANCE_METRES, MAX_DISTANCE_METRES);
        }
    }

    /// Advances smooth zoom by `dt` seconds; `instant` (reduced motion) jumps to the target.
    pub fn update(&mut self, dt: f64, instant: bool) {
        let blend = if instant || !dt.is_finite() {
            1.
        } else {
            1. - (-ZOOM_RATE_PER_SECOND * dt.max(0.)).exp()
        };
        self.distance += (self.target_distance - self.distance) * blend;
        if (self.target_distance - self.distance).abs() < 1e-3 {
            self.distance = self.target_distance;
        }
    }

    /// Follows a projected opponent along the shorter turn, framing it beside the character,
    /// and blends the framing out once focus clears; never snaps when focus is acquired,
    /// switched or cleared.
    pub fn track(&mut self, focus: [f64; 3], opponent: Option<[f64; 3]>, dt: f64) {
        let dt = if dt.is_finite() { dt.max(0.) } else { 0. };
        if let Some(opponent) = opponent {
            let desired = (opponent[0] - focus[0]).atan2(opponent[2] - focus[2]);
            let shorter = (desired - self.yaw).sin().atan2((desired - self.yaw).cos());
            self.look_along(self.yaw + shorter * (1. - (-FOCUS_TURN_RATE * dt).exp()));
        }
        let delta = opponent.map_or([0.; 3], |point| {
            std::array::from_fn(|axis| point[axis] - focus[axis])
        });
        let scale = (FOCUS_FRAMING_METRES / delta[0].hypot(delta[2]).max(1e-6)).min(1.);
        let desired = [
            delta[0] * scale * 0.35,
            delta[1] * 0.2,
            delta[2] * scale * 0.35,
        ];
        let blend = 1. - (-FOCUS_FRAMING_RATE * dt).exp();
        for (current, target) in self.framing.iter_mut().zip(desired) {
            *current += (target - *current) * blend;
        }
    }

    /// Adopts a heading without changing pitch or zoom.
    pub fn look_along(&mut self, heading: f64) {
        if heading.is_finite() {
            self.yaw = heading.rem_euclid(TAU);
        }
    }

    /// The heading the camera looks along.
    #[must_use]
    pub const fn yaw(&self) -> f64 {
        self.yaw
    }

    /// The heading a character takes while moving under this camera.
    #[must_use]
    pub fn facing(&self) -> u16 {
        yaw_from_radians(self.yaw)
    }

    /// The distance the view uses now; it approaches [`Self::target_distance`].
    #[must_use]
    pub const fn distance(&self) -> f64 {
        self.distance
    }

    #[must_use]
    pub const fn target_distance(&self) -> f64 {
        self.target_distance
    }

    /// Eye and look-at point for a focus at the character's body centre.
    #[must_use]
    pub fn view(&self, focus: [f64; 3]) -> View {
        // Shift the whole aim line to screen-right, leaving the character left of the reticle;
        // offsetting only the eye would still centre the character and skew the heading.
        let shoulder_x = -self.yaw.cos() * SHOULDER_OFFSET_METRES;
        let shoulder_z = self.yaw.sin() * SHOULDER_OFFSET_METRES;
        let horizontal = self.distance * self.pitch.cos();
        View {
            eye: [
                focus[0] - self.yaw.sin() * horizontal + shoulder_x,
                focus[1] + PIVOT_HEIGHT_METRES + self.distance * self.pitch.sin(),
                focus[2] - self.yaw.cos() * horizontal + shoulder_z,
            ],
            target: [
                focus[0] + shoulder_x + self.framing[0],
                focus[1] + PIVOT_HEIGHT_METRES + self.framing[1],
                focus[2] + shoulder_z + self.framing[2],
            ],
        }
    }

    /// [`Self::view`] with the eye raised above the ground under it; nothing else collides.
    #[must_use]
    pub fn view_above_ground(&self, focus: [f64; 3], ground_at: impl Fn(f64, f64) -> f64) -> View {
        let mut view = self.view(focus);
        let ground = ground_at(view.eye[0], view.eye[2]);
        raise_eye(&mut view, ground);
        view
    }
}

/// Keeps the eye [`GROUND_CLEARANCE_METRES`] above `ground`, the surface height under it.
pub fn raise_eye(view: &mut View, ground: f64) {
    let floor = ground + GROUND_CLEARANCE_METRES;
    if floor.is_finite() && view.eye[1] < floor {
        view.eye[1] = floor;
    }
}

/// The view with its eye pulled in toward the target until the sight line clears every box (a
/// camera boom, #552): indoors the eye stays under roofs and in front of walls instead of
/// looking at their backs. Boxes that contain the target are ignored.
#[must_use]
pub fn clear_sight_line(view: View, boxes: &[SightBox]) -> View {
    let View { eye, target } = view;
    let ray: [f64; 3] = std::array::from_fn(|axis| eye[axis] - target[axis]);
    let length = ray[0].hypot(ray[1]).hypot(ray[2]);
    if length < 1e-6 {
        return view;
    }
    let mut nearest: f64 = 1.;
    for sight in boxes {
        let (mut enter, mut exit) = (0_f64, 1_f64);
        for axis in 0..3 {
            if enter > exit {
                break;
            }
            let (origin, step) = (target[axis], ray[axis]);
            let (low, high) = (sight.min[axis], sight.max[axis]);
            if step.abs() < 1e-9 {
                if origin < low || origin > high {
                    exit = -1.;
                }
                continue;
            }
            let (a, b) = ((low - origin) / step, (high - origin) / step);
            enter = enter.max(a.min(b));
            exit = exit.min(a.max(b));
        }
        let contains_target =
            (0..3).all(|axis| target[axis] >= sight.min[axis] && target[axis] <= sight.max[axis]);
        if enter <= exit && !contains_target && enter < nearest {
            nearest = enter;
        }
    }
    if nearest >= 1. {
        return view;
    }
    let t = (nearest - SIGHT_CLEARANCE_METRES / length).max(0.);
    View {
        eye: std::array::from_fn(|axis| target[axis] + ray[axis] * t),
        target,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::{FRAC_PI_2, PI};

    fn close(left: [f64; 3], right: [f64; 3]) -> bool {
        left.iter().zip(right).all(|(a, b)| (a - b).abs() < 1e-9)
    }

    #[test]
    fn the_default_view_looks_over_the_right_shoulder_toward_positive_z() {
        let camera = OrbitCamera::default();
        let view = camera.view([1., 0.9, -2.]);
        assert!(close(view.target, [0.35, 1.5, -2.]));
        assert!(
            view.eye[2] < view.target[2],
            "the eye is behind the character"
        );
        assert!(view.eye[1] > view.target[1], "and above it");
        assert!(view.eye[0] < 1. && (view.eye[0] - view.target[0]).abs() < 1e-9);
        let offset: [f64; 3] = std::array::from_fn(|axis| view.eye[axis] - view.target[axis]);
        let length = offset[0].hypot(offset[1]).hypot(offset[2]);
        assert!((length - camera.distance()).abs() < 1e-9);
        assert_eq!(camera.facing(), 0);
    }

    #[test]
    fn dragging_right_turns_the_heading_toward_the_characters_right() {
        let mut camera = OrbitCamera::default();
        // A quarter turn to the right: facing +Z turns to face -X.
        camera.orbit(FRAC_PI_2 / ORBIT_RADIANS_PER_PIXEL, 0.);
        assert!(camera.facing().abs_diff(49_152) <= 2, "{}", camera.facing());
        assert!(
            camera.view([0.; 3]).eye[0] > 0.5,
            "the eye moved behind a -X heading"
        );
        camera.orbit(-PI / ORBIT_RADIANS_PER_PIXEL, 0.);
        assert!(camera.facing().abs_diff(16_384) <= 2, "{}", camera.facing());
        camera.look_along(PI);
        assert_eq!(camera.facing(), 32_768);
    }

    #[test]
    fn sensitivity_scales_look_and_inversion_reverses_only_vertical_motion() {
        let mut normal = OrbitCamera::default();
        let mut sensitive = normal;
        sensitive.set_look(2., false);
        normal.orbit(10., 4.);
        sensitive.orbit(5., 2.);
        assert_eq!(normal.view([0.; 3]), sensitive.view([0.; 3]));
        let mut inverted = OrbitCamera::default();
        inverted.set_look(1., true);
        let mut upright = OrbitCamera::default();
        upright.orbit(8., 5.);
        inverted.orbit(8., -5.);
        assert_eq!(upright.view([0.; 3]), inverted.view([0.; 3]));
    }

    #[test]
    fn pitch_and_zoom_are_clamped_and_ignore_non_finite_input() {
        let mut camera = OrbitCamera::default();
        camera.orbit(0., 1e9);
        let top = camera.view([0.; 3]);
        assert!(top.eye[1] < top.target[1] + camera.distance());
        camera.orbit(0., -1e9);
        let low = camera.view([0.; 3]);
        assert!(
            low.eye[1] < low.target[1],
            "looking up places the eye below the pivot"
        );
        camera.zoom(1e3);
        assert_eq!(camera.target_distance(), MIN_DISTANCE_METRES);
        camera.update(0., true);
        assert_eq!(camera.distance(), MIN_DISTANCE_METRES);
        camera.zoom(-1e3);
        camera.update(0., true);
        assert_eq!(camera.distance(), MAX_DISTANCE_METRES);
        camera.zoom(f64::NAN);
        camera.zoom_by(0.);
        camera.orbit(f64::NAN, 0.);
        camera.update(f64::NAN, false);
        assert_eq!(camera.distance(), MAX_DISTANCE_METRES);
        camera.zoom_by(2.);
        assert_eq!(camera.target_distance(), MAX_DISTANCE_METRES / 2.);
    }

    #[test]
    fn zoom_eases_toward_its_target_and_settles_exactly() {
        let mut camera = OrbitCamera::default();
        let start = camera.distance();
        camera.zoom(3.);
        let target = camera.target_distance();
        assert!(target < start);
        assert_eq!(camera.distance(), start);
        camera.update(1. / 60., false);
        assert!(camera.distance() < start && camera.distance() > target);
        for _ in 0..120 {
            camera.update(1. / 60., false);
        }
        assert_eq!(camera.distance(), target);
    }

    #[test]
    fn the_eye_never_sinks_below_the_ground_under_it() {
        let camera = OrbitCamera::default();
        let flat = camera.view_above_ground([0., 1., 0.], |_, _| 0.);
        assert_eq!(flat, camera.view([0., 1., 0.]));
        let hill = camera.view_above_ground([0., 1., 0.], |_, _| 12.);
        assert!((hill.eye[1] - (12. + GROUND_CLEARANCE_METRES)).abs() < 1e-9);
        assert_eq!([hill.eye[0], hill.eye[2]], [flat.eye[0], flat.eye[2]]);
        assert_eq!(hill.target, flat.target);
    }

    #[test]
    fn enemy_focus_switches_and_releases_without_a_camera_snap() {
        let mut camera = OrbitCamera::default();
        let focus = [0., 1., 0.];
        let before = camera.view(focus);
        camera.track(focus, Some([8., 1., 0.]), 1. / 60.);
        assert!(camera.yaw() > 0. && camera.yaw() < FRAC_PI_2);
        assert!(camera.view(focus).target[0] > before.target[0]);
        for _ in 0..90 {
            camera.track(focus, Some([8., 1., 0.]), 1. / 60.);
        }
        assert!((camera.yaw() - FRAC_PI_2).abs() < 0.001);
        camera.track(focus, Some([-8., 1., 0.]), 1. / 60.);
        assert!(camera.yaw() < FRAC_PI_2 + 0.5);
        let released = camera.yaw();
        camera.track(focus, None, 1. / 60.);
        assert_eq!(camera.yaw(), released);
        for _ in 0..90 {
            camera.track(focus, None, 1. / 60.);
        }
        let target = camera.view(focus).target[0];
        assert!((target + camera.yaw().cos() * SHOULDER_OFFSET_METRES).abs() < 0.001);
    }

    #[test]
    fn the_sight_line_pulls_the_eye_in_front_of_walls_and_under_roofs() {
        let view = View {
            target: [0., 1., 0.],
            eye: [0., 5., -10.],
        };
        let aside = SightBox {
            min: [5., 0., -5.],
            max: [6., 8., 5.],
        };
        assert_eq!(clear_sight_line(view, &[aside]), view);
        let wall = SightBox {
            min: [-5., 0., -6.],
            max: [5., 8., -5.],
        };
        let pulled = clear_sight_line(view, &[wall]);
        assert_eq!(pulled.target, view.target);
        assert!(pulled.eye[2] > -5. && pulled.eye[2] < 0.);
        let roof = SightBox {
            min: [-10., 3., -10.],
            max: [10., 3.5, 10.],
        };
        assert!(clear_sight_line(view, &[roof]).eye[1] < 3.);
        // A box around the character, such as the floor it stands on, never pulls the eye.
        let floor = SightBox {
            min: [-10., -1., -10.],
            max: [10., 1.4, 10.],
        };
        assert_eq!(clear_sight_line(view, &[floor]), view);
    }

    #[test]
    fn yaw_steps_round_to_the_nearest_and_wrap_a_full_turn() {
        assert_eq!(yaw_from_radians(0.), 0);
        assert_eq!(yaw_from_radians(FRAC_PI_2), 16_384);
        assert_eq!(yaw_from_radians(-FRAC_PI_2), 49_152);
        assert_eq!(yaw_from_radians(TAU - 1e-9), 0);
    }
}
