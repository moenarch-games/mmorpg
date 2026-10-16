use mmorpg_client::session::{NetworkUpdate, PlayerInput};
use mmorpg_client::{
    ClientError,
    camera::OrbitCamera,
    controls::{ControlAction, NativeControls},
    graphics::WindowRenderer,
    hud::{CombatHud, class_kit},
    presentation::Presentation,
    world::WorldScene,
};
use mmorpg_core::{PlayerClass, ZoneContent};
use mmorpg_scenery::Scenery;
use std::{
    collections::HashSet,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::watch;
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalPosition,
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowId},
};

#[allow(clippy::too_many_arguments)]
pub fn run(
    runtime: Arc<tokio::runtime::Runtime>,
    player_id: u32,
    scenery: Scenery,
    content: Arc<ZoneContent>,
    world: WorldScene,
    input: watch::Sender<PlayerInput>,
    updates: watch::Receiver<NetworkUpdate>,
    frames: Option<u32>,
    class: PlayerClass,
) -> Result<(), ClientError> {
    let event_loop = EventLoop::new()?;
    let mut app = App {
        runtime,
        window: None,
        renderer: None,
        world,
        presentation: Presentation::new(player_id, scenery, content, Instant::now())?,
        title: String::new(),
        connection_epoch: None,
        input,
        updates,
        keys: HashSet::new(),
        camera: OrbitCamera::default(),
        orbit_buttons: HashSet::new(),
        cursor: None,
        error: None,
        frames,
        rendered: 0,
        next_frame: Instant::now(),
        abilities: class_kit(class),
        controls: NativeControls::new(),
        hud: None,
    };
    event_loop.run_app(&mut app)?;
    match app.error {
        Some(error) => Err(error.into()),
        None => Ok(()),
    }
}

struct App {
    runtime: Arc<tokio::runtime::Runtime>,
    window: Option<Arc<Window>>,
    renderer: Option<WindowRenderer>,
    /// Static geometry uploaded when the window's renderer is created.
    world: WorldScene,
    presentation: Presentation,
    /// The window title last set, so it changes only when its text does.
    title: String,
    connection_epoch: Option<u32>,
    input: watch::Sender<PlayerInput>,
    updates: watch::Receiver<NetworkUpdate>,
    keys: HashSet<KeyCode>,
    camera: OrbitCamera,
    /// Held mouse buttons that orbit the camera while the cursor moves.
    orbit_buttons: HashSet<MouseButton>,
    cursor: Option<PhysicalPosition<f64>>,
    error: Option<String>,
    frames: Option<u32>,
    rendered: u32,
    next_frame: Instant,
    /// Ability IDs on action-bar slots 1–4: the class kit in catalog order.
    abilities: Vec<u8>,
    /// Ability slots and cast cancelling through the shared input runtime.
    controls: NativeControls,
    /// The combat HUD of the latest projection.
    hud: Option<CombatHud>,
}

const FORWARD_KEYS: [KeyCode; 2] = [KeyCode::KeyW, KeyCode::ArrowUp];
const BACKWARD_KEYS: [KeyCode; 2] = [KeyCode::KeyS, KeyCode::ArrowDown];
const LEFT_KEYS: [KeyCode; 3] = [KeyCode::KeyA, KeyCode::KeyQ, KeyCode::ArrowLeft];
const RIGHT_KEYS: [KeyCode; 3] = [KeyCode::KeyD, KeyCode::KeyE, KeyCode::ArrowRight];

/// Movement intent from the held keys.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct HeldMovement {
    forward: i8,
    strafe: i8,
    /// Any movement key is held, so the character faces the camera.
    steering: bool,
}

/// W/S run and backpedal, A/D or Q/E strafe. Dead units cannot move, so the
/// dead hold no movement whatever keys are down.
fn held_movement(keys: &HashSet<KeyCode>, dead: bool) -> HeldMovement {
    let held = |group: &[KeyCode]| !dead && group.iter().any(|key| keys.contains(key));
    HeldMovement {
        forward: i8::from(held(&FORWARD_KEYS)) - i8::from(held(&BACKWARD_KEYS)),
        strafe: i8::from(held(&RIGHT_KEYS)) - i8::from(held(&LEFT_KEYS)),
        steering: held(&FORWARD_KEYS)
            || held(&BACKWARD_KEYS)
            || held(&LEFT_KEYS)
            || held(&RIGHT_KEYS),
    }
}

/// Browser-style pixel scrolling: this many pixels count as one wheel line.
const PIXELS_PER_WHEEL_LINE: f64 = 40.0;
const CONTROL_HINTS: &str = "W/S move · A/D or Q/E strafe · Space jumps · Tab target · F attack · 1-4 abilities · drag to orbit · wheel zooms · Esc cancels a cast or closes";

impl App {
    fn stop(&self) {
        self.input.send_if_modified(|input| {
            let moving = input.forward != 0 || input.strafe != 0;
            input.forward = 0;
            input.strafe = 0;
            moving
        });
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, error: impl ToString) {
        self.stop();
        self.error = Some(error.to_string());
        event_loop.exit();
    }

    /// Whether the latest projection shows the player dead.
    fn dead(&self) -> bool {
        self.presentation
            .latest()
            .is_some_and(|latest| latest.viewer.dead)
    }

    /// While any movement key is held, the character faces the camera's yaw.
    /// The dead send no held movement; it resumes with the next projection
    /// that shows them alive.
    fn update_movement(&self) {
        let HeldMovement {
            forward,
            strafe,
            steering,
        } = held_movement(&self.keys, self.dead());
        let facing = steering.then(|| self.camera.facing());
        self.input.send_if_modified(|input| {
            let next = PlayerInput {
                forward,
                strafe,
                facing: facing.unwrap_or(input.facing),
                ..*input
            };
            let changed = *input != next;
            *input = next;
            changed
        });
    }

    /// Dead units cannot jump, so a press while dead sends nothing.
    fn jump(&self) {
        if !self.dead() {
            self.input
                .send_modify(|input| input.jumps = input.jumps.wrapping_add(1));
        }
    }

    /// Tab, F and R are intents derived from the latest projection; the zone
    /// decides what happens.
    fn combat_key(&self, code: KeyCode) {
        let latest = self.presentation.latest();
        match code {
            KeyCode::Tab => {
                if let Some(target) = self.presentation.next_tab_target() {
                    self.input.send_modify(|input| {
                        input.target = Some(target);
                        input.selections = input.selections.wrapping_add(1);
                    });
                }
            }
            KeyCode::KeyF => {
                let start = !latest.is_some_and(|latest| latest.viewer.auto_attacking);
                self.input.send_modify(|input| {
                    input.attack = start;
                    input.attack_requests = input.attack_requests.wrapping_add(1);
                });
            }
            KeyCode::KeyR if latest.is_some_and(|latest| latest.viewer.dead) => {
                self.input
                    .send_modify(|input| input.releases = input.releases.wrapping_add(1));
            }
            _ => {}
        }
    }

    /// A slot use or cast cancel is an intent; the zone decides what happens.
    fn control(&self, action: ControlAction) {
        match action {
            ControlAction::AbilitySlot(slot) => {
                let ability = usize::from(slot)
                    .checked_sub(1)
                    .and_then(|index| self.abilities.get(index));
                if let Some(&ability) = ability {
                    self.input.send_modify(|input| {
                        input.ability = ability;
                        input.ability_uses = input.ability_uses.wrapping_add(1);
                    });
                }
            }
            ControlAction::CancelCast => {
                self.input.send_modify(|input| {
                    input.cancels = input.cancels.wrapping_add(1);
                });
            }
        }
    }

    /// The HUD and the `casting` control layer follow the latest projection.
    fn follow_projection(&mut self) {
        let latest = self.presentation.latest();
        self.controls
            .set_casting(latest.is_some_and(|latest| latest.viewer.cast.is_some()));
        self.hud = latest.map(CombatHud::from_projection);
    }

    /// Shows the connection state or the player's health and target.
    fn show_title(&mut self, now: Instant) {
        let title = if self.presentation.is_stalled(now) {
            "MMORPG — connection stalled".to_owned()
        } else {
            match self.presentation.status() {
                Some(status) => format!("MMORPG — {status} — {CONTROL_HINTS}"),
                None => format!("MMORPG — {CONTROL_HINTS}"),
            }
        };
        if title != self.title
            && let Some(window) = &self.window
        {
            window.set_title(&title);
            self.title = title;
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let window = match event_loop.create_window(
            Window::default_attributes()
                .with_title("MMORPG — connecting to world")
                .with_inner_size(winit::dpi::LogicalSize::new(1280, 800)),
        ) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                self.fail(event_loop, error);
                return;
            }
        };
        match self
            .runtime
            .block_on(WindowRenderer::new(Arc::clone(&window), &self.world))
        {
            Ok(renderer) => {
                self.renderer = Some(renderer);
                self.window = Some(window);
            }
            Err(error) => self.fail(event_loop, error),
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                self.stop();
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(size.width, size.height);
                }
            }
            WindowEvent::Focused(false) => {
                self.controls.focus_lost();
                self.keys.clear();
                self.orbit_buttons.clear();
                self.update_movement();
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if matches!(button, MouseButton::Left | MouseButton::Right) {
                    match state {
                        ElementState::Pressed => self.orbit_buttons.insert(button),
                        ElementState::Released => self.orbit_buttons.remove(&button),
                    };
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(previous) = self.cursor.replace(position)
                    && !self.orbit_buttons.is_empty()
                {
                    self.camera
                        .orbit(position.x - previous.x, position.y - previous.y);
                    // A held movement key keeps the character facing the camera.
                    self.update_movement();
                }
            }
            WindowEvent::CursorLeft { .. } => self.cursor = None,
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, lines) => f64::from(lines),
                    MouseScrollDelta::PixelDelta(position) => position.y / PIXELS_PER_WHEEL_LINE,
                };
                self.camera.zoom(lines as f32);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    if event.state == ElementState::Pressed {
                        let actions = self.controls.key_down(code, event.repeat);
                        // Escape cancels a cast through the controls, otherwise a
                        // fresh press closes; a held Escape's repeats never close.
                        if code == KeyCode::Escape && actions.is_empty() {
                            if !event.repeat {
                                self.stop();
                                event_loop.exit();
                            }
                            return;
                        }
                        for action in actions {
                            self.control(action);
                        }
                    } else {
                        self.controls.key_up(code);
                    }
                    // Jump is edge-triggered: key repeat never queues more jumps.
                    if code == KeyCode::Space
                        && event.state == ElementState::Pressed
                        && !event.repeat
                    {
                        self.jump();
                    }
                    if event.state == ElementState::Pressed && !event.repeat {
                        self.combat_key(code);
                    }
                    if event.state == ElementState::Pressed {
                        self.keys.insert(code);
                    } else {
                        self.keys.remove(&code);
                    }
                    self.update_movement();
                }
            }
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let update = self.updates.borrow_and_update().clone();
                match update {
                    NetworkUpdate::Waiting => {}
                    NetworkUpdate::Reconnecting => {
                        if let Some(window) = &self.window {
                            window.set_title("MMORPG — reconnecting to world");
                        }
                        self.title.clear();
                    }
                    NetworkUpdate::Snapshot {
                        connection_epoch,
                        snapshot,
                    } => {
                        if self.connection_epoch != Some(connection_epoch) {
                            self.presentation.reset(now);
                            self.connection_epoch = Some(connection_epoch);
                        }
                        if let Err(error) = self.presentation.push((*snapshot).clone(), now) {
                            self.fail(event_loop, error);
                            return;
                        }
                        self.follow_projection();
                        // Dying lets go of held movement; living again resumes it.
                        self.update_movement();
                        self.show_title(now);
                    }
                    NetworkUpdate::Failed(error) => {
                        self.fail(event_loop, error);
                        return;
                    }
                }
                let view = self.camera.view(self.presentation.camera_target(now));
                if let Some(renderer) = &mut self.renderer
                    && let Err(error) = renderer.render(
                        &self.presentation.scene(now, view),
                        &self.hud.as_ref().map(CombatHud::rects).unwrap_or_default(),
                        view,
                    )
                {
                    self.fail(event_loop, error);
                    return;
                }
                self.rendered = self.rendered.saturating_add(1);
                if self.frames.is_some_and(|limit| self.rendered >= limit) {
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        if now >= self.next_frame {
            if let Some(window) = &self.window {
                window.request_redraw();
            }
            self.next_frame = now + Duration::from_secs_f64(1.0 / 60.0);
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dead_hold_no_movement() {
        let keys: HashSet<_> = [KeyCode::KeyW, KeyCode::KeyA].into_iter().collect();
        assert_eq!(
            held_movement(&keys, false),
            HeldMovement {
                forward: 1,
                strafe: -1,
                steering: true
            }
        );
        assert_eq!(
            held_movement(&keys, true),
            HeldMovement {
                forward: 0,
                strafe: 0,
                steering: false
            }
        );
        let opposed: HashSet<_> = [KeyCode::KeyS, KeyCode::ArrowUp, KeyCode::KeyE]
            .into_iter()
            .collect();
        assert_eq!(
            held_movement(&opposed, false),
            HeldMovement {
                forward: 0,
                strafe: 1,
                steering: true
            }
        );
    }
}

// Independent #28 application acceptance; no runtime behavior is added here.
#[cfg(all(test, target_os = "linux"))]
#[path = "../tests/support/native_pointer_application.rs"]
mod native_pointer_application;
