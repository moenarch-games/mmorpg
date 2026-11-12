//! Third-person shoulder camera around the local character. Presentation only:
//! its heading becomes movement intent, but never an authoritative position.
use crate::presentation::yaw_from_radians;
use mmorpg_core::EntityRef;

/// The camera pivots around the character's upper body, not its feet.
const PIVOT_HEIGHT_METRES: f32 = 0.6;
const MIN_DISTANCE_METRES: f32 = 2.0;
const MAX_DISTANCE_METRES: f32 = 8.0;
/// Allows looking up and down while staying short of the vertical singularity.
const MIN_PITCH_RADIANS: f32 = -1.0;
const MAX_PITCH_RADIANS: f32 = 1.2;
const ORBIT_RADIANS_PER_PIXEL: f32 = 0.006;
/// Each wheel line changes the distance by this factor.
const ZOOM_STEP: f32 = 0.88;
const SHOULDER_OFFSET_METRES: f32 = 0.65;
const FOCUS_TURN_RATE: f32 = 10.0;
const FOCUS_FRAMING_RATE: f32 = 7.0;
const FOCUS_FRAMING_METRES: f32 = 3.0;

/// A selected NPC is still a selection, but not an enemy camera focus.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CameraMode {
    #[default]
    FreeLook,
    Focus(EntityRef),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerOwner {
    Camera,
    Interface,
    Gesture,
}

/// Small client-only transition model: the input runtime still owns bindings and held keys.
#[derive(Clone, Copy, Debug, Default)]
pub struct CameraControl {
    mode: CameraMode,
    cursor_released: bool,
}

impl CameraControl {
    pub fn mode(&self) -> CameraMode {
        self.mode
    }

    pub fn set_focus(&mut self, target: Option<EntityRef>) {
        let next = target.map_or(CameraMode::FreeLook, CameraMode::Focus);
        if self.mode != next {
            self.mode = next;
            // Native can recapture on returning to free look, without a second click.
            self.cursor_released = false;
        }
    }

    pub fn release_cursor(&mut self) {
        self.cursor_released = true;
    }

    pub fn request_look(&mut self) {
        self.cursor_released = false;
    }

    pub fn pointer_owner(&self, gesture: bool, interface: bool) -> PointerOwner {
        if gesture {
            PointerOwner::Gesture
        } else if interface || self.cursor_released || self.mode != CameraMode::FreeLook {
            PointerOwner::Interface
        } else {
            PointerOwner::Camera
        }
    }
}

/// Eye and look-at point for one rendered frame, in metres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraView {
    pub field_of_view_degrees: f32,
    pub eye: [f32; 3],
    pub target: [f32; 3],
}

/// `yaw` is the horizontal direction the camera looks in the world yaw
/// convention (0 looks toward +Z, increasing toward +X). `pitch` raises the
/// eye above the pivot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrbitCamera {
    field_of_view_degrees: f32,
    look_sensitivity: f32,
    invert_vertical_look: bool,
    yaw: f32,
    pitch: f32,
    distance: f32,
    framing: [f32; 3],
}

impl Default for OrbitCamera {
    /// Starts behind a character facing yaw 0.
    fn default() -> Self {
        Self {
            field_of_view_degrees: 50.0,
            look_sensitivity: 1.0,
            invert_vertical_look: false,
            yaw: 0.0,
            pitch: 0.12,
            distance: 3.5,
            framing: [0.0; 3],
        }
    }
}

impl OrbitCamera {
    /// Apply already-validated client preferences to camera presentation.
    pub fn set_preferences(
        &mut self,
        field_of_view_degrees: f32,
        look_sensitivity: f32,
        invert_vertical_look: bool,
    ) {
        self.field_of_view_degrees = field_of_view_degrees;
        self.look_sensitivity = look_sensitivity;
        self.invert_vertical_look = invert_vertical_look;
    }

    /// Mouse drag: moving right turns the view right; moving down raises the eye.
    pub fn orbit(&mut self, dx_pixels: f64, dy_pixels: f64) {
        let dy_pixels = if self.invert_vertical_look {
            -dy_pixels
        } else {
            dy_pixels
        };
        let turn = (dx_pixels as f32) * ORBIT_RADIANS_PER_PIXEL * self.look_sensitivity;
        self.yaw = (self.yaw - turn).rem_euclid(std::f32::consts::TAU);
        self.pitch = (self.pitch
            + (dy_pixels as f32) * ORBIT_RADIANS_PER_PIXEL * self.look_sensitivity)
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

    /// Follow the projected opponent in the shortest direction; blend out framing on clear.
    pub fn track(&mut self, self_position: [f32; 3], opponent: Option<[f32; 3]>, dt: f32) {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        if let Some(opponent) = opponent {
            let desired = (opponent[0] - self_position[0]).atan2(opponent[2] - self_position[2]);
            let difference = (desired - self.yaw).sin().atan2((desired - self.yaw).cos());
            self.look_along(self.yaw + difference * (1.0 - (-FOCUS_TURN_RATE * dt).exp()));
        }
        let delta = opponent.map_or([0.0; 3], |point| {
            std::array::from_fn(|axis| point[axis] - self_position[axis])
        });
        let scale = (FOCUS_FRAMING_METRES / delta[0].hypot(delta[2]).max(1e-6)).min(1.0);
        let desired = [
            delta[0] * scale * 0.35,
            delta[1] * 0.2,
            delta[2] * scale * 0.35,
        ];
        let blend = 1.0 - (-FOCUS_FRAMING_RATE * dt).exp();
        for (current, target) in self.framing.iter_mut().zip(desired) {
            *current += (target - *current) * blend;
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

    /// The horizontal direction the camera looks in, in radians.
    #[must_use]
    pub fn yaw(&self) -> f32 {
        self.yaw
    }

    #[must_use]
    pub fn distance(&self) -> f32 {
        self.distance
    }

    #[must_use]
    pub fn view(&self, focus: [f32; 3]) -> CameraView {
        // Shift the whole aim line to screen-right so the character stays left of the reticle.
        let shoulder_x = -self.yaw.cos() * SHOULDER_OFFSET_METRES;
        let shoulder_z = self.yaw.sin() * SHOULDER_OFFSET_METRES;
        let target = [
            focus[0] + shoulder_x + self.framing[0],
            focus[1] + PIVOT_HEIGHT_METRES + self.framing[1],
            focus[2] + shoulder_z + self.framing[2],
        ];
        let horizontal = self.distance * self.pitch.cos();
        CameraView {
            field_of_view_degrees: self.field_of_view_degrees,
            eye: [
                focus[0] - self.yaw.sin() * horizontal + shoulder_x,
                focus[1] + PIVOT_HEIGHT_METRES + self.distance * self.pitch.sin(),
                focus[2] - self.yaw.cos() * horizontal + shoulder_z,
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
        assert!(close(view.target, [0.35, 1.5, -2.0]));
        assert!(
            view.eye[2] < view.target[2],
            "the eye is behind the character"
        );
        assert!(view.eye[1] > view.target[1], "and above it");
        assert!(
            view.eye[0] < 1.0 && (view.eye[0] - view.target[0]).abs() < 1e-4,
            "the eye sits over the right shoulder"
        );
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
    fn preferences_change_projection_and_look_without_moving_the_focus() {
        let mut normal = OrbitCamera::default();
        let mut sensitive = normal;
        sensitive.set_preferences(70.0, 2.0, false);
        normal.orbit(10.0, 0.0);
        sensitive.orbit(5.0, 0.0);
        assert_eq!(normal.view([0.0; 3]).eye, sensitive.view([0.0; 3]).eye);
        assert_eq!(sensitive.view([0.0; 3]).field_of_view_degrees, 70.0);
        assert_eq!(
            normal.view([0.0; 3]).target,
            sensitive.view([0.0; 3]).target
        );
    }

    #[test]
    fn inverted_look_reverses_vertical_motion_without_reversing_heading() {
        let mut normal = OrbitCamera::default();
        let mut inverted = normal;
        inverted.set_preferences(50.0, 1.0, true);
        normal.orbit(8.0, 5.0);
        inverted.orbit(8.0, -5.0);
        assert_eq!(normal.view([0.0; 3]), inverted.view([0.0; 3]));
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
            low.eye[1] < low.target[1],
            "looking up can place the eye below the pivot"
        );
        camera.zoom(1e3);
        assert_eq!(camera.distance(), MIN_DISTANCE_METRES);
        camera.zoom(-1e3);
        assert_eq!(camera.distance(), MAX_DISTANCE_METRES);
        camera.zoom(f32::NAN);
        assert_eq!(camera.distance(), MAX_DISTANCE_METRES);
    }

    #[test]
    fn enemy_focus_switches_and_releases_without_a_camera_snap() {
        let mut camera = OrbitCamera::default();
        let before = camera.view([0.0, 1.0, 0.0]);
        camera.track([0.0, 1.0, 0.0], Some([8.0, 1.0, 0.0]), 1.0 / 60.0);
        assert!(camera.yaw() > 0.0 && camera.yaw() < std::f32::consts::FRAC_PI_2);
        assert!(camera.view([0.0, 1.0, 0.0]).target[0] > before.target[0]);
        for _ in 0..90 {
            camera.track([0.0, 1.0, 0.0], Some([8.0, 1.0, 0.0]), 1.0 / 60.0);
        }
        assert!((camera.yaw() - std::f32::consts::FRAC_PI_2).abs() < 0.001);
        camera.track([0.0, 1.0, 0.0], Some([-8.0, 1.0, 0.0]), 1.0 / 60.0);
        assert!(camera.yaw() < std::f32::consts::FRAC_PI_2 + 0.5);
        let released_heading = camera.yaw();
        camera.track([0.0, 1.0, 0.0], None, 1.0 / 60.0);
        assert_eq!(camera.yaw(), released_heading);
        for _ in 0..90 {
            camera.track([0.0, 1.0, 0.0], None, 1.0 / 60.0);
        }
        assert!(
            (camera.view([0.0, 1.0, 0.0]).target[0] + camera.yaw().cos() * SHOULDER_OFFSET_METRES)
                .abs()
                < 0.001
        );
    }

    #[test]
    fn pointer_ownership_tracks_free_look_focus_gesture_and_ui_retirement() {
        let first = EntityRef::Creature(mmorpg_core::CreatureId::new(108));
        let second = EntityRef::Creature(mmorpg_core::CreatureId::new(300));
        let mut state = CameraControl::default();
        assert_eq!(state.pointer_owner(false, false), PointerOwner::Camera);
        state.set_focus(Some(first));
        assert_eq!(state.mode(), CameraMode::Focus(first));
        assert_eq!(state.pointer_owner(false, false), PointerOwner::Interface);
        assert_eq!(state.pointer_owner(true, false), PointerOwner::Gesture);
        state.set_focus(Some(second));
        assert_eq!(state.mode(), CameraMode::Focus(second));
        state.set_focus(None);
        assert_eq!(state.mode(), CameraMode::FreeLook);
        assert_eq!(state.pointer_owner(false, false), PointerOwner::Camera);
        state.release_cursor();
        assert_eq!(state.pointer_owner(false, false), PointerOwner::Interface);
        state.request_look();
        assert_eq!(state.pointer_owner(false, true), PointerOwner::Interface);
        assert_eq!(state.pointer_owner(false, false), PointerOwner::Camera);
    }
}
