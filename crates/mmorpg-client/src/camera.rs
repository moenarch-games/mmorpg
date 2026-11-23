//! The native adapter of the shared third-person shoulder camera (`mmorpg-client-model`'s
//! [`camera`](mmorpg_client_model::camera), #1058): the field of view and `f32` render views
//! around the shared look, zoom, focus-following and ground rules. Presentation only: its heading
//! becomes movement intent, but never an authoritative position.
use mmorpg_client_model::camera as rules;
use mmorpg_core::EntityRef;

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

/// The shared orbit camera with the player's field of view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrbitCamera {
    field_of_view_degrees: f32,
    rules: rules::OrbitCamera,
}

impl Default for OrbitCamera {
    /// Starts behind a character facing yaw 0.
    fn default() -> Self {
        Self {
            field_of_view_degrees: 50.0,
            rules: rules::OrbitCamera::default(),
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
        self.rules
            .set_look(f64::from(look_sensitivity), invert_vertical_look);
    }

    /// Mouse drag: moving right turns the view right; moving down raises the eye.
    pub fn orbit(&mut self, dx_pixels: f64, dy_pixels: f64) {
        self.rules.orbit(dx_pixels, dy_pixels);
    }

    /// Positive wheel lines zoom in; the view follows smoothly in [`Self::update`].
    pub fn zoom(&mut self, lines: f32) {
        self.rules.zoom(f64::from(lines));
    }

    /// Advances smooth zoom by `dt` seconds; `instant` (reduced motion) jumps to the target.
    pub fn update(&mut self, dt: f64, instant: bool) {
        self.rules.update(dt, instant);
    }

    /// Adopt a presentation heading without changing user pitch or zoom.
    pub fn look_along(&mut self, heading: f32) {
        self.rules.look_along(f64::from(heading));
    }

    /// Follow the projected opponent in the shortest direction; blend out framing on clear.
    pub fn track(&mut self, self_position: [f32; 3], opponent: Option<[f32; 3]>, dt: f32) {
        self.rules.track(
            self_position.map(f64::from),
            opponent.map(|point| point.map(f64::from)),
            f64::from(dt),
        );
    }

    /// Raise the eye above the actual rendered terrain, preserving the target.
    #[must_use]
    pub fn view_above_ground(
        &self,
        focus: [f32; 3],
        ground_at: impl Fn(f32, f32) -> f32,
    ) -> CameraView {
        self.render(self.rules.view_above_ground(focus.map(f64::from), |x, z| {
            f64::from(ground_at(x as f32, z as f32))
        }))
    }

    /// The heading a character takes while moving under this camera.
    #[must_use]
    pub fn facing(&self) -> u16 {
        self.rules.facing()
    }

    /// The horizontal direction the camera looks in, in radians.
    #[must_use]
    pub fn yaw(&self) -> f32 {
        self.rules.yaw() as f32
    }

    #[must_use]
    pub fn distance(&self) -> f32 {
        self.rules.distance() as f32
    }

    #[must_use]
    pub fn view(&self, focus: [f32; 3]) -> CameraView {
        self.render(self.rules.view(focus.map(f64::from)))
    }

    fn render(&self, view: rules::View) -> CameraView {
        CameraView {
            field_of_view_degrees: self.field_of_view_degrees,
            eye: view.eye.map(|value| value as f32),
            target: view.target.map(|value| value as f32),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn wheel_zoom_eases_in_through_the_frame_update() {
        let mut camera = OrbitCamera::default();
        let start = camera.distance();
        camera.zoom(1.0);
        assert_eq!(camera.distance(), start, "the view eases, it does not jump");
        camera.update(1.0 / 60.0, false);
        assert!(camera.distance() < start);
        camera.zoom(1.0);
        camera.update(0.0, true);
        assert!((camera.distance() - start * 0.88_f32.powi(2)).abs() < 1e-4);
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
