//! Third-person orbit camera around the local character. Presentation only:
//! its heading becomes movement intent, but never an authoritative position.
use crate::presentation::yaw_from_radians;

/// The camera pivots around the character's upper body, not its feet.
const PIVOT_HEIGHT_METRES: f32 = 0.6;
const MIN_DISTANCE_METRES: f32 = 2.0;
const MAX_DISTANCE_METRES: f32 = 30.0;
/// Keeps the eye above the pivot's horizon and short of straight down.
const MIN_PITCH_RADIANS: f32 = 0.05;
const MAX_PITCH_RADIANS: f32 = 1.45;
const ORBIT_RADIANS_PER_PIXEL: f32 = 0.006;
/// Each wheel line changes the distance by this factor.
const ZOOM_STEP: f32 = 0.88;

/// Eye and look-at point for one rendered frame, in metres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraView {
    pub eye: [f32; 3],
    pub target: [f32; 3],
}

/// `yaw` is the horizontal direction the camera looks in the world yaw
/// convention (0 looks toward +Z, increasing toward +X). `pitch` raises the
/// eye above the pivot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrbitCamera {
    yaw: f32,
    pitch: f32,
    distance: f32,
}

impl Default for OrbitCamera {
    /// Starts behind a character facing yaw 0.
    fn default() -> Self {
        Self {
            yaw: 0.0,
            pitch: 0.36,
            distance: 9.0,
        }
    }
}

impl OrbitCamera {
    /// Mouse drag: moving right turns the view right; moving down raises the eye.
    pub fn orbit(&mut self, dx_pixels: f64, dy_pixels: f64) {
        let turn = (dx_pixels as f32) * ORBIT_RADIANS_PER_PIXEL;
        self.yaw = (self.yaw - turn).rem_euclid(std::f32::consts::TAU);
        self.pitch = (self.pitch + (dy_pixels as f32) * ORBIT_RADIANS_PER_PIXEL)
            .clamp(MIN_PITCH_RADIANS, MAX_PITCH_RADIANS);
    }

    /// Positive wheel lines zoom in.
    pub fn zoom(&mut self, lines: f32) {
        if lines.is_finite() {
            self.distance = (self.distance * ZOOM_STEP.powf(lines))
                .clamp(MIN_DISTANCE_METRES, MAX_DISTANCE_METRES);
        }
    }

    /// Adopt a presentation heading without changing user pitch or zoom.
    pub fn look_along(&mut self, heading: f32) {
        if heading.is_finite() {
            self.yaw = heading.rem_euclid(std::f32::consts::TAU);
        }
    }

    /// Raise the eye above the actual rendered terrain, preserving the target.
    #[must_use]
    pub fn view_above_ground(
        &self,
        focus: [f32; 3],
        ground_at: impl Fn(f32, f32) -> f32,
    ) -> CameraView {
        let mut view = self.view(focus);
        let floor = ground_at(view.eye[0], view.eye[2]) + 0.35;
        if floor.is_finite() && view.eye[1] < floor {
            view.eye[1] = floor;
        }
        view
    }

    /// The heading a character takes while moving under this camera.
    #[must_use]
    pub fn facing(&self) -> u16 {
        yaw_from_radians(self.yaw)
    }

    #[must_use]
    pub fn distance(&self) -> f32 {
        self.distance
    }

    #[must_use]
    pub fn view(&self, focus: [f32; 3]) -> CameraView {
        let target = [focus[0], focus[1] + PIVOT_HEIGHT_METRES, focus[2]];
        let horizontal = self.distance * self.pitch.cos();
        CameraView {
            eye: [
                target[0] - self.yaw.sin() * horizontal,
                target[1] + self.distance * self.pitch.sin(),
                target[2] - self.yaw.cos() * horizontal,
            ],
            target,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(left: [f32; 3], right: [f32; 3]) -> bool {
        left.iter().zip(right).all(|(a, b)| (a - b).abs() < 1e-4)
    }

    #[test]
    fn default_view_looks_over_the_shoulder_toward_positive_z() {
        let camera = OrbitCamera::default();
        let view = camera.view([1.0, 0.9, -2.0]);
        assert!(close(view.target, [1.0, 1.5, -2.0]));
        assert!(
            view.eye[2] < view.target[2],
            "the eye is behind the character"
        );
        assert!(view.eye[1] > view.target[1], "and above it");
        assert!((view.eye[0] - view.target[0]).abs() < 1e-4);
        let offset: [f32; 3] = std::array::from_fn(|axis| view.eye[axis] - view.target[axis]);
        let length = offset.iter().map(|value| value * value).sum::<f32>().sqrt();
        assert!((length - camera.distance()).abs() < 1e-4);
        assert_eq!(camera.facing(), 0);
    }

    #[test]
    fn dragging_right_turns_the_heading_toward_the_characters_right() {
        let mut camera = OrbitCamera::default();
        // A quarter turn to the right: facing +Z turns to face -X.
        camera.orbit(
            f64::from(std::f32::consts::FRAC_PI_2 / ORBIT_RADIANS_PER_PIXEL),
            0.0,
        );
        assert!(camera.facing().abs_diff(49_152) <= 2, "{}", camera.facing());
        let view = camera.view([0.0; 3]);
        assert!(view.eye[0] > 0.5, "the eye moved behind a -X heading");
        camera.orbit(
            -f64::from(std::f32::consts::PI / ORBIT_RADIANS_PER_PIXEL),
            0.0,
        );
        assert!(camera.facing().abs_diff(16_384) <= 2, "{}", camera.facing());
    }

    #[test]
    fn pitch_and_zoom_are_clamped() {
        let mut camera = OrbitCamera::default();
        camera.orbit(0.0, 1e9);
        let top = camera.view([0.0; 3]);
        assert!(top.eye[1] < top.target[1] + camera.distance());
        camera.orbit(0.0, -1e9);
        let low = camera.view([0.0; 3]);
        assert!(
            low.eye[1] > low.target[1],
            "the eye never drops below the pivot"
        );
        camera.zoom(1e3);
        assert_eq!(camera.distance(), MIN_DISTANCE_METRES);
        camera.zoom(-1e3);
        assert_eq!(camera.distance(), MAX_DISTANCE_METRES);
        camera.zoom(f32::NAN);
        assert_eq!(camera.distance(), MAX_DISTANCE_METRES);
    }
}
