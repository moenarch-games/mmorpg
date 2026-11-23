//! The browser's handle on the shared orbit camera (`mmorpg-client-model`'s `camera`, #1058):
//! the page feeds it pointer, wheel and pinch input and the frame's focus, and renders the view
//! it returns. Views cross as `[eye x, y, z, target x, y, z]` in metres.
use mmorpg_client_model::camera::{self, SightBox, View};
use wasm_bindgen::prelude::wasm_bindgen;

fn view_of(values: &[f64]) -> Option<View> {
    let values: &[f64; 6] = values.try_into().ok()?;
    Some(View {
        eye: [values[0], values[1], values[2]],
        target: [values[3], values[4], values[5]],
    })
}

fn write(view: View, out: &mut [f64]) {
    if let Some(out) = out.get_mut(..6) {
        out[..3].copy_from_slice(&view.eye);
        out[3..].copy_from_slice(&view.target);
    }
}

/// The third-person shoulder camera around the local character.
#[wasm_bindgen(js_name = OrbitCamera)]
#[derive(Default)]
pub struct OrbitCameraHandle(camera::OrbitCamera);

#[wasm_bindgen(js_class = OrbitCamera)]
impl OrbitCameraHandle {
    #[wasm_bindgen(constructor)]
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Look sensitivity per pixel and vertical inversion.
    #[wasm_bindgen(js_name = setLook)]
    pub fn set_look(&mut self, sensitivity: f64, invert: bool) {
        self.0.set_look(sensitivity, invert);
    }

    /// Mouse drag: moving right turns the view right; moving down raises the eye.
    pub fn orbit(&mut self, dx: f64, dy: f64) {
        self.0.orbit(dx, dy);
    }

    /// Positive wheel lines zoom in.
    pub fn zoom(&mut self, lines: f64) {
        self.0.zoom(lines);
    }

    /// A pinch spreading the fingers by `scale`.
    #[wasm_bindgen(js_name = zoomBy)]
    pub fn zoom_by(&mut self, scale: f64) {
        self.0.zoom_by(scale);
    }

    /// Advances smooth zoom; `instant` (reduced motion) jumps to the target.
    pub fn update(&mut self, dt: f64, instant: bool) {
        self.0.update(dt, instant);
    }

    /// Follows the opponent at `opponent` (`[x, y, z]`, or empty once focus clears).
    pub fn track(&mut self, focus: &[f64], opponent: &[f64], dt: f64) {
        let point = |values: &[f64]| <[f64; 3]>::try_from(values).ok();
        if let Some(focus) = point(focus) {
            self.0.track(focus, point(opponent), dt);
        }
    }

    /// Turns the view to a heading in radians.
    #[wasm_bindgen(js_name = lookAlong)]
    pub fn look_along(&mut self, heading: f64) {
        self.0.look_along(heading);
    }

    /// The view's heading in radians: 0 toward +Z, increasing toward +X.
    #[wasm_bindgen(getter)]
    pub fn heading(&self) -> f64 {
        self.0.yaw()
    }

    /// The heading a character takes while moving under this camera, in yaw steps.
    pub fn facing(&self) -> u16 {
        self.0.facing()
    }

    #[wasm_bindgen(getter)]
    pub fn distance(&self) -> f64 {
        self.0.distance()
    }

    #[wasm_bindgen(getter = targetDistance)]
    pub fn target_distance(&self) -> f64 {
        self.0.target_distance()
    }

    /// Writes the view around a focus at the character's body centre into `out`.
    pub fn view(&self, x: f64, y: f64, z: f64, out: &mut [f64]) {
        write(self.0.view([x, y, z]), out);
    }
}

/// Raises a view's eye (`[eye, target]`) above `ground`, the surface height under it.
#[wasm_bindgen(js_name = raiseEye)]
pub fn raise_eye(view: &mut [f64], ground: f64) {
    if let Some(mut parsed) = view_of(view) {
        camera::raise_eye(&mut parsed, ground);
        write(parsed, view);
    }
}

/// Pulls a view's eye (`[eye, target]`) in until the sight line clears every box, given as
/// `[min x, y, z, max x, y, z]` each (#552).
#[wasm_bindgen(js_name = clearSightLine)]
pub fn clear_sight_line(view: &mut [f64], boxes: &[f64]) {
    let Some(parsed) = view_of(view) else {
        return;
    };
    let boxes: Vec<SightBox> = boxes
        .as_chunks::<6>()
        .0
        .iter()
        .map(|values| SightBox {
            min: [values[0], values[1], values[2]],
            max: [values[3], values[4], values[5]],
        })
        .collect();
    write(camera::clear_sight_line(parsed, &boxes), view);
}
