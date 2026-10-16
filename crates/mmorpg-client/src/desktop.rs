use mmorpg_client::session::{NetworkUpdate, PlayerInput};
use mmorpg_client::{
    ClientError,
    camera::OrbitCamera,
    controls::{ControlAction, NativeControls},
    graphics::WindowRenderer,
    hud::{CombatHud, class_kit},
    interaction::NpcContext,
    presentation::Presentation,
    world::WorldScene,
    world_controls::{WorldAction, WorldControls},
};
use mmorpg_core::{EntityKind, EntityRef, PlayerClass, ZoneContent};
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
    let mut presentation = Presentation::new(player_id, scenery, content, Instant::now())?;
    presentation.set_default_appearance(class);
    let mut app = App {
        runtime,
        window: None,
        renderer: None,
        world,
        presentation,
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
        world_controls: WorldControls::new(),
        npc_context: NpcContext::default(),
        pointer_origin: None,
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
    world_controls: WorldControls,
    npc_context: NpcContext,
    /// A left press remains a click only within five physical pixels.
    pointer_origin: Option<PhysicalPosition<f64>>,
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
const CONTROL_HINTS: &str = "W/S move · A/D or Q/E strafe · Space jumps · click selects · N cycles NPC · Enter interact · Tab target · F attack · 1-4 abilities · drag to orbit · wheel zooms · Esc cancels a cast or closes";

impl App {
    fn retire_pointer(&mut self) {
        self.pointer_origin = None;
        self.orbit_buttons.clear();
        self.world_controls.reset();
    }

    fn connected(&self) -> bool {
        matches!(*self.updates.borrow(), NetworkUpdate::Snapshot { .. })
            && !self.presentation.is_stalled(Instant::now())
    }

    fn select_pointer(&mut self, event_loop: &ActiveEventLoop) {
        if !self.connected() {
            return;
        }
        let Some(cursor) = self.cursor else { return };
        let result = self.renderer.as_ref().map(|r| r.pick([cursor.x, cursor.y]));
        let target = match result {
            Some(Ok(target)) => target,
            Some(Err(error)) => {
                self.fail(event_loop, error);
                return;
            }
            None => None,
        };
        if let Some(target) = target {
            // Only a current visible creature/NPC can become a requested target.
            if self.presentation.latest().is_some_and(|s| {
                s.entities.iter().any(|e| {
                    e.entity() == target && matches!(e.kind, EntityKind::Creature | EntityKind::Npc)
                })
            }) {
                self.npc_context.close();
                self.world_controls.set_interaction(false);
                self.input.send_modify(|input| {
                    input.target = Some(target);
                    input.selections = input.selections.wrapping_add(1);
                });
            }
        }
    }

    fn world_action(&mut self, action: WorldAction) -> bool {
        match action {
            WorldAction::Cancel => {
                if self.npc_context.npc.is_some() || !self.orbit_buttons.is_empty() {
                    self.npc_context.close();
                    self.world_controls.set_interaction(false);
                    self.retire_pointer();
                    return true;
                }
                return false;
            }
            WorldAction::Interact if self.connected() => {
                if let Some(EntityRef::Npc(npc)) =
                    self.presentation.latest().and_then(|s| s.viewer.target)
                {
                    self.npc_context.open(npc);
                    self.world_controls.set_interaction(true);
                    // A context owns the number keys until closed.
                    self.controls.focus_lost();
                    self.keys.clear();
                    self.update_movement();
                }
            }
            WorldAction::NextNpc if self.connected() => {
                if let Some(snapshot) = self.presentation.latest() {
                    let mut npcs: Vec<_> = snapshot
                        .entities
                        .iter()
                        .filter(|e| e.kind == EntityKind::Npc)
                        .map(|e| e.entity())
                        .collect();
                    npcs.sort_unstable();
                    let index = snapshot
                        .viewer
                        .target
                        .and_then(|t| npcs.iter().position(|e| *e == t))
                        .map_or(0, |i| i + 1);
                    if !npcs.is_empty() {
                        let target = npcs[index % npcs.len()];
                        self.input.send_modify(|i| {
                            i.target = Some(target);
                            i.selections = i.selections.wrapping_add(1);
                        });
                        self.npc_context.close();
                        self.world_controls.set_interaction(false);
                    }
                }
            }
            WorldAction::Primary | WorldAction::Sell if self.connected() => {
                let selected = self.presentation.latest().and_then(|s| s.viewer.target);
                if self.npc_context.npc.map(EntityRef::Npc) == selected
                    && let Some(command) = self
                        .npc_context
                        .action(self.presentation.content(), action == WorldAction::Sell)
                {
                    self.npc_context.feedback = "request sent".into();
                    self.input.send_modify(|input| {
                        input.interaction = Some(command);
                        input.interactions = input.interactions.wrapping_add(1);
                    });
                }
            }
            WorldAction::Details => self.npc_context.next_detail(self.presentation.content()),
            WorldAction::Previous
            | WorldAction::Next
            | WorldAction::RewardPrevious
            | WorldAction::RewardNext => {
                let delta = if matches!(action, WorldAction::Previous | WorldAction::RewardPrevious)
                {
                    -1
                } else {
                    1
                };
                self.npc_context.navigate(
                    self.presentation.content(),
                    delta,
                    matches!(
                        action,
                        WorldAction::RewardPrevious | WorldAction::RewardNext
                    ),
                );
            }
            WorldAction::Select
            | WorldAction::Interact
            | WorldAction::NextNpc
            | WorldAction::Primary
            | WorldAction::Sell => {}
        }
        true
    }

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
        if let Some(snapshot) = latest {
            self.npc_context.observe(snapshot);
            if self.npc_context.npc.is_some()
                && self.npc_context.npc.map(EntityRef::Npc) != snapshot.viewer.target
            {
                self.npc_context.close();
                self.world_controls.set_interaction(false);
            }
        }
    }

    /// Shows the connection state or the player's health and target.
    fn show_title(&mut self, now: Instant) {
        let title = if self.presentation.is_stalled(now) {
            "MMORPG — connection stalled".to_owned()
        } else {
            if self.npc_context.npc.is_some() {
                format!(
                    "MMORPG — {}",
                    self.npc_context.text(self.presentation.content())
                )
            } else {
                match self.presentation.status() {
                    Some(status) => format!("MMORPG — {status} — {CONTROL_HINTS}"),
                    None => format!("MMORPG — {CONTROL_HINTS}"),
                }
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
                self.retire_pointer();
                self.update_movement();
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if matches!(button, MouseButton::Left | MouseButton::Right) {
                    if state == ElementState::Pressed {
                        self.orbit_buttons.insert(button);
                        if button == MouseButton::Left {
                            self.world_controls.pointer(true);
                            self.pointer_origin = self.cursor;
                        }
                    } else {
                        if button == MouseButton::Left {
                            let click = self.world_controls.pointer(false)
                                && self.pointer_origin.take().is_some();
                            if click {
                                self.select_pointer(event_loop);
                            }
                        }
                        self.orbit_buttons.remove(&button);
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(previous) = self.cursor.replace(position)
                    && !self.orbit_buttons.is_empty()
                {
                    if self.pointer_origin.is_some_and(|start| {
                        (position.x - start.x).hypot(position.y - start.y) > 5.0
                    }) {
                        self.pointer_origin = None;
                    }
                    if self.pointer_origin.is_none()
                        || self.orbit_buttons.contains(&MouseButton::Right)
                    {
                        self.camera
                            .orbit(position.x - previous.x, position.y - previous.y);
                        self.update_movement();
                    }
                }
            }
            WindowEvent::CursorLeft { .. } => {
                self.cursor = None;
                self.retire_pointer();
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, lines) => f64::from(lines),
                    MouseScrollDelta::PixelDelta(position) => position.y / PIXELS_PER_WHEEL_LINE,
                };
                self.camera.zoom(lines as f32);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    let context_was_open = self.npc_context.npc.is_some();
                    let mut cancel_handled = false;
                    for action in self.world_controls.key(
                        code,
                        event.state == ElementState::Pressed,
                        event.repeat,
                    ) {
                        let handled = self.world_action(action);
                        cancel_handled |= action == WorldAction::Cancel && handled;
                    }
                    self.show_title(Instant::now());
                    if context_was_open || cancel_handled {
                        if event.state == ElementState::Released {
                            self.controls.key_up(code);
                            self.keys.remove(&code);
                            self.update_movement();
                        }
                        return;
                    }

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
                        self.retire_pointer();
                        self.npc_context.close();
                        self.world_controls.set_interaction(false);
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
                            self.npc_context = NpcContext::default();
                            self.world_controls.set_interaction(false);
                            self.retire_pointer();
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
                let view = self
                    .presentation
                    .camera_view(&mut self.camera, &self.world, now);
                let frame = self.presentation.frame(&self.world, now, view);
                if let Some(renderer) = &mut self.renderer
                    && let Err(error) = renderer.render(
                        &frame,
                        &self.hud.as_ref().map(CombatHud::rects).unwrap_or_default(),
                        view,
                        self.presentation.picked_draws(),
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
