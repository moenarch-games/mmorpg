use mmorpg_client::session::{CharacterSelection, NetworkUpdate, PlayerInput};
use mmorpg_client::{
    ClientError,
    camera::{CameraView, OrbitCamera},
    controls::{ControlAction, NativeControls},
    gamepad::{
        PadAction, PadControls, PadMovement, PadState, camera_orbit, guard_lane, interface_key,
        read_pad,
    },
    graphics::WindowRenderer,
    graphics::project_to_viewport,
    hud::{CombatHud, class_kit},
    interaction::NpcContext,
    presentation::Presentation,
    roster::{Roster, config_dir},
    ui::{GameUi, UiFrame, UiIntent, overlay::EguiWindow},
    world::WorldScene,
    world_controls::{WorldAction, WorldControls},
};
use mmorpg_core::{EntityKind, EntityRef, PlayerClass, Sex, ZoneContent};
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
    characters: watch::Sender<Option<CharacterSelection>>,
    updates: watch::Receiver<NetworkUpdate>,
    frames: Option<u32>,
    character: Option<(PlayerClass, Sex)>,
) -> Result<(), ClientError> {
    let event_loop = EventLoop::new()?;
    let mut ui = GameUi::new(&scenery);
    if character.is_none() {
        // The local roster offers its characters; `--class` skips it.
        ui.set_roster(
            config_dir()
                .ok_or_else(|| "No configuration directory for the local roster.".to_owned())
                .and_then(|dir| Roster::load(&dir)),
        );
    }
    let mut presentation = Presentation::new(player_id, scenery, content, Instant::now())?;
    if let Some((class, _)) = character {
        presentation.set_default_appearance(class);
    }
    let mut app = App {
        runtime,
        window: None,
        renderer: None,
        world,
        presentation,
        title: String::new(),
        connection_epoch: None,
        last_refusal: None,
        input,
        characters,
        updates,
        camera: OrbitCamera::default(),
        orbit_buttons: HashSet::new(),
        cursor: None,
        guard_offset: None,
        pad_guard: None,
        error: None,
        frames,
        rendered: 0,
        next_frame: Instant::now(),
        abilities: character
            .map(|(class, _)| class_kit(class))
            .unwrap_or_default(),
        controls: NativeControls::new(),
        hud: None,
        world_controls: WorldControls::new(),
        npc_context: NpcContext::default(),
        pointer_origin: None,
        ui,
        egui: None,
        choosing: character.is_none(),
        chat_focused: false,
        gilrs: open_gamepads(),
        pad: PadControls::new(),
        pad_ui: Vec::new(),
        focused: true,
        last_pad_sample: Instant::now(),
        bindings_revision: 0,
        modifiers: winit::keyboard::ModifiersState::empty(),
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
    last_refusal: Option<(u32, mmorpg_client::session::RefusalReceipt)>,
    input: watch::Sender<PlayerInput>,
    characters: watch::Sender<Option<CharacterSelection>>,
    updates: watch::Receiver<NetworkUpdate>,
    camera: OrbitCamera,
    /// Held mouse buttons that orbit the camera while the cursor moves.
    orbit_buttons: HashSet<MouseButton>,
    cursor: Option<PhysicalPosition<f64>>,
    /// Vertical pointer travel since the guard key went down, while it is held (#156).
    guard_offset: Option<f64>,
    /// The lane the pad's held left trigger aims with the right stick (#156).
    pad_guard: Option<mmorpg_core::StrikeHeight>,
    error: Option<String>,
    frames: Option<u32>,
    rendered: u32,
    next_frame: Instant,
    /// Ability IDs on action-bar slots 1–8: the class kit in catalog order.
    abilities: Vec<u8>,
    /// Ability slots and cast cancelling through the shared input runtime.
    controls: NativeControls,
    /// The combat HUD of the latest projection.
    hud: Option<CombatHud>,
    world_controls: WorldControls,
    npc_context: NpcContext,
    /// A left press remains a click only within five physical pixels.
    pointer_origin: Option<PhysicalPosition<f64>>,
    /// The egui interface (#183): panels, frames and their intents.
    ui: GameUi,
    egui: Option<EguiWindow>,
    /// The character still needs a class and sex before abilities work.
    choosing: bool,
    /// The chat field owned the keyboard after the last interface frame.
    chat_focused: bool,
    /// Gamepad devices; `None` where the platform offers none.
    gilrs: Option<gilrs::Gilrs>,
    /// Gamepad actions and held movement through the shared input runtime.
    pad: PadControls,
    /// Pad focus navigation for the next interface pass.
    pad_ui: Vec<egui::Event>,
    /// Pads act only while the window has focus.
    focused: bool,
    last_pad_sample: Instant,
    bindings_revision: u64,
    modifiers: winit::keyboard::ModifiersState,
}

/// Opens the platform's gamepads; a platform without gamepad support keeps
/// a dummy backend, and a failure leaves keyboard and mouse only.
fn open_gamepads() -> Option<gilrs::Gilrs> {
    match gilrs::Gilrs::new() {
        Ok(gilrs) | Err(gilrs::Error::NotImplemented(gilrs)) => Some(gilrs),
        Err(error) => {
            eprintln!("gamepads unavailable: {error}");
            None
        }
    }
}

/// Movement intent from the held keys.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct HeldMovement {
    forward: i8,
    strafe: i8,
    /// Any movement key is held, so the character faces the camera.
    steering: bool,
}

/// W/S run and backpedal, A/D or Q/E strafe, combined with the pad's left
/// stick. Dead units cannot move, so the dead hold no movement whatever keys
/// are down.
fn held_movement(keyboard: PadMovement, pad: PadMovement, dead: bool) -> HeldMovement {
    if dead {
        return HeldMovement {
            forward: 0,
            strafe: 0,
            steering: false,
        };
    }
    HeldMovement {
        forward: (keyboard.forward + pad.forward).clamp(-1, 1),
        strafe: (keyboard.strafe + pad.strafe).clamp(-1, 1),
        steering: keyboard.steering || pad.steering,
    }
}

/// Browser-style pixel scrolling: this many pixels count as one wheel line.
/// Pointer travel (physical pixels) from the middle lane before the guard moves high or low.
const GUARD_LANE_THRESHOLD_PX: f64 = 24.;
const PIXELS_PER_WHEEL_LINE: f64 = 40.0;
const CONTROL_HINTS: &str = "W/S move · A/D or Q/E strafe · Space jumps (hold to swim up) · X swims down · click selects · N cycles NPC · Enter talks to the selected NPC, opens or closes a door in reach, or chats · click a door to open or close it · B bags · C character · L quests · Tab target · F attack · 1-8 abilities · drag to orbit · wheel zooms · Esc cancels a cast, closes panels or quits · gamepad: sticks move and look, A jump (hold to swim up), R3 swim down, X attack, RB target, d-pad abilities, LB NPC, Start talk or door, B back, hold LT to guard (right stick up/down aims), L3 character, RT quests";

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
        let scale = self.window.as_ref().map_or(1., |w| w.scale_factor());
        let margin = mmorpg_client::picking::NEAR_MISS_PIXELS * scale;
        let result = self
            .renderer
            .as_ref()
            .map(|r| r.pick([cursor.x, cursor.y], margin));
        let picked = match result {
            Some(Ok(picked)) => picked,
            Some(Err(error)) => {
                self.fail(event_loop, error);
                return;
            }
            None => None,
        };
        // A click on the door leaf asks the zone to swing it; the zone checks reach (#158).
        if let Some(mmorpg_client::picking::Picked::Door(door)) = picked {
            let command = self.presentation.toggle_door(door);
            self.send_interaction(command);
            return;
        }
        let target = match picked {
            Some(mmorpg_client::picking::Picked::Unit(target)) => Some(target),
            _ => None,
        };
        if let Some(target) = target {
            // Only a current visible creature, NPC or other player can become a requested
            // target; another player may be a friend to heal or shield (#303).
            if self.presentation.latest().is_some_and(|s| {
                s.entities.iter().any(|e| {
                    e.entity() == target
                        && (matches!(e.kind, EntityKind::Creature | EntityKind::Npc)
                            || (e.kind == EntityKind::Player && e.id != s.viewer_id))
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
                    self.update_movement();
                } else if let Some(command) = self.presentation.reachable_door() {
                    // Without a selected NPC, interact swings the door in reach (#158).
                    self.send_interaction(command);
                    return true;
                } else if let Some(command) = self.presentation.reachable_repair() {
                    // Or repairs a damaged structure piece in reach (#305).
                    self.send_interaction(command);
                    return true;
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
        } = held_movement(self.controls.movement(), self.pad.movement(), self.dead());
        let facing = steering.then(|| self.camera.facing());
        // Held Jump ascends and held Descend descends while swimming (#274); the zone ignores
        // the rise out of the water.
        let rise = if self.dead() {
            0
        } else {
            (self.controls.rise() + self.pad.rise()).clamp(-1, 1)
        };
        self.input.send_if_modified(|input| {
            let next = PlayerInput {
                forward,
                strafe,
                rise,
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
    fn control(&mut self, action: ControlAction) {
        match action {
            ControlAction::MoveForward
            | ControlAction::MoveBackward
            | ControlAction::MoveLeft
            | ControlAction::MoveRight => {}
            ControlAction::Jump => self.jump(),
            ControlAction::Descend => {}
            ControlAction::TargetNext => self.combat_key(KeyCode::Tab),
            ControlAction::ToggleAttack => self.combat_key(KeyCode::KeyF),
            ControlAction::ReleaseSpirit => self.combat_key(KeyCode::KeyR),
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
            ControlAction::TogglePanel(panel) => self.ui.toggle(panel),
            ControlAction::OpenChat => self.ui.open_chat(),
            ControlAction::Guard => self.guard_offset = Some(0.),
        }
    }

    /// The held guard lane follows the pointer's vertical travel since the
    /// press, as in the browser: up guards high, down low; a release, focus
    /// loss or a viewer that cannot guard lowers it. The zone decides whether
    /// it protects.
    fn update_guard(&mut self) {
        let can_guard = self.presentation.latest().is_some_and(|latest| {
            !latest.viewer.dead
                && latest
                    .viewer
                    .class
                    .is_some_and(|choice| choice.class == mmorpg_core::PlayerClass::Warden)
        });
        if !self.controls.is_held(ControlAction::Guard) || !can_guard {
            self.guard_offset = None;
        }
        let pointer = self.guard_offset.map(|offset| {
            if offset <= -GUARD_LANE_THRESHOLD_PX {
                mmorpg_core::StrikeHeight::High
            } else if offset >= GUARD_LANE_THRESHOLD_PX {
                mmorpg_core::StrikeHeight::Low
            } else {
                mmorpg_core::StrikeHeight::Middle
            }
        });
        // The key's pointer aim wins over a pad guard held at the same time.
        let lane = pointer.or(self.pad_guard.filter(|_| can_guard));
        if self.input.borrow().guard != lane {
            self.input.send_modify(|input| input.guard = lane);
        }
    }

    /// Samples connected pads once per frame: held movement, the analog
    /// camera stick and one-shot actions. Pads act only while the window is
    /// focused; a disconnect retires everything they held.
    fn poll_pads(&mut self, now: Instant) {
        let seconds = now
            .saturating_duration_since(self.last_pad_sample)
            .as_secs_f32();
        self.last_pad_sample = now;
        let Some(gilrs) = self.gilrs.as_mut() else {
            return;
        };
        let mut disconnected = false;
        while let Some(event) = gilrs.next_event() {
            disconnected |= event.event == gilrs::EventType::Disconnected;
        }
        if disconnected {
            self.pad.retire("gamepadDisconnected");
        }
        let pads: Vec<PadState> = gilrs.gamepads().map(|(_, pad)| read_pad(&pad)).collect();
        let snapshot = self.presentation.latest();
        self.pad.set_input_context(
            self.ui.settings.open
                || self.choosing
                || self.npc_context.npc.is_some()
                || self.ui.windows_open(snapshot),
            self.ui.settings.open,
            !self.focused || self.ui.chat_focused(),
        );
        for action in self.pad.sample(&pads) {
            self.pad_action(action);
        }
        self.pad_guard = self.pad.guarding().then(|| guard_lane(&pads));
        let [dx, mut dy] = camera_orbit(&pads, seconds);
        // While the pad guards, the right stick's vertical aims the shield.
        if self.pad_guard.is_some() {
            dy = 0.0;
        }
        if !self.ui.settings.open
            && self.focused
            && !self.ui.chat_focused()
            && (dx != 0.0 || dy != 0.0)
        {
            self.camera.orbit(dx, dy);
        }
        self.update_movement();
    }

    /// A pad press becomes the same intent as its key or interface control.
    fn pad_action(&mut self, action: PadAction) {
        // Focus moves, presses and Escape (which also drops the focus) are
        // keys for the next interface pass.
        self.pad_ui.extend(interface_key(action));
        match action {
            PadAction::Jump => self.jump(),
            PadAction::TargetNext => self.combat_key(KeyCode::Tab),
            PadAction::ToggleAutoAttack => self.combat_key(KeyCode::KeyF),
            PadAction::ReleaseSpirit => self.combat_key(KeyCode::KeyR),
            PadAction::AbilitySlot(slot) => self.control(ControlAction::AbilitySlot(slot)),
            PadAction::CancelCast => self.control(ControlAction::CancelCast),
            PadAction::TogglePanel(panel) => self.control(ControlAction::TogglePanel(panel)),
            PadAction::NextNpc => {
                self.world_action(WorldAction::NextNpc);
            }
            PadAction::Talk => {
                self.world_action(WorldAction::Interact);
            }
            PadAction::ClosePanel => {
                if self.npc_context.npc.is_some() {
                    self.npc_context.close();
                    self.world_controls.set_interaction(false);
                } else {
                    self.ui.close_window(self.presentation.latest());
                }
            }
            PadAction::FocusPrevious | PadAction::FocusNext | PadAction::Activate => {}
        }
    }

    fn send_interaction(&mut self, command: mmorpg_core::ZoneCommand) {
        self.input.send_modify(|input| {
            input.interaction = Some(command);
            input.interactions = input.interactions.wrapping_add(1);
        });
    }

    /// Held keys, armed clicks and held semantic actions let go, e.g. when
    /// the chat field takes the keyboard.
    fn retire_held_input(&mut self) {
        self.controls.focus_lost();
        self.world_controls.reset();
        self.orbit_buttons.clear();
        self.guard_offset = None;
        self.pad_guard = None;
        self.pad.retire("inputContextChanged");
        self.update_guard();
        self.update_movement();
    }

    /// An interface control's intent becomes the same command as its key.
    fn ui_intent(&mut self, intent: UiIntent) {
        match intent {
            UiIntent::Command(command) => {
                self.input.send_modify(|input| {
                    input.interaction = Some(command);
                    input.interactions = input.interactions.wrapping_add(1);
                });
            }
            UiIntent::AbilitySlot(slot) => self.control(ControlAction::AbilitySlot(slot)),
            UiIntent::ToggleAttack => self.combat_key(KeyCode::KeyF),
            UiIntent::ReleaseSpirit => self.combat_key(KeyCode::KeyR),
            UiIntent::Select(target) => {
                self.input.send_modify(|input| {
                    input.target = target;
                    input.selections = input.selections.wrapping_add(1);
                });
            }
            UiIntent::Talk => {
                self.world_action(WorldAction::Interact);
            }
            UiIntent::CloseContext => {
                self.npc_context.close();
                self.world_controls.set_interaction(false);
            }
            UiIntent::ChooseCharacter(id, class, sex, headwear) => {
                self.retire_held_input();
                self.ui.settings.bindings.select_character(Some(&id));
                self.sync_bindings();
                self.choosing = false;
                self.presentation.set_preview(None);
                self.presentation.set_default_appearance(class);
                // The roster's headwear is a known style, so this cannot fail.
                let _ = self.presentation.set_headwear(Some(headwear.id()));
                self.abilities = class_kit(class);
                self.input.send_modify(|input| {
                    input.class_choice = Some((class.code(), sex.code()));
                });
                self.characters.send_replace(Some(CharacterSelection {
                    local_id: id,
                    class: mmorpg_core::ClassChoice { class, sex },
                    global_id: None,
                }));
            }
        }
    }

    fn binding_modifiers(&self) -> input_bindings_core::Modifiers {
        input_bindings_core::Modifiers {
            ctrl: self.modifiers.control_key(),
            alt: self.modifiers.alt_key(),
            shift: self.modifiers.shift_key(),
            meta: self.modifiers.super_key(),
            alt_graph: false,
        }
    }

    fn sync_bindings(&mut self) {
        let revision = self.ui.settings.bindings.revision();
        if revision == self.bindings_revision {
            return;
        }
        let profile = self.ui.settings.bindings.profile().cloned();
        self.retire_held_input();
        if let Some(profile) = profile {
            if let Err(error) = self
                .controls
                .set_profile(Some(profile.clone()))
                .and_then(|()| self.world_controls.set_profile(profile.clone()))
                .and_then(|()| self.pad.set_profile(profile))
            {
                self.ui.settings.bindings.problem(error);
                return;
            }
        }
        self.bindings_revision = revision;
    }

    /// Runs one egui pass over the latest projection and applies its intents.
    fn interface(&mut self, now: Instant, view: CameraView) {
        let (Some(egui), Some(window)) = (self.egui.as_mut(), self.window.as_ref()) else {
            return;
        };
        let connection = match &*self.updates.borrow() {
            NetworkUpdate::Waiting => Some("Connecting to the world…"),
            NetworkUpdate::Reconnecting => Some("Reconnecting to the world…"),
            _ if self.presentation.is_stalled(now) => Some("Connection stalled…"),
            _ => None,
        };
        let forward = [view.target[0] - view.eye[0], view.target[2] - view.eye[2]];
        let presentation = &self.presentation;
        let names = |entity| presentation.unit_name(entity);
        let viewport = self.renderer.as_ref().map_or([0, 0], |r| r.viewport());
        let points_per_pixel = 1.0 / egui.context.pixels_per_point().max(0.1);
        let project = |point| {
            project_to_viewport(view, viewport, point)
                .map(|[x, y]| egui::pos2(x * points_per_pixel, y * points_per_pixel))
        };
        let frame = UiFrame {
            snapshot: presentation.latest(),
            content: presentation.content(),
            hud: self.hud.as_ref(),
            names: &names,
            forward,
            connection,
            choosing: self.choosing,
            labels: presentation.labels(),
            project: &project,
            eye: view.eye,
        };
        let mut raw = egui.state.take_egui_input(window);
        raw.events.append(&mut self.pad_ui);
        let mut intents = Vec::new();
        let settings_was_open = self.ui.settings.open;
        let output = egui.context.run_pass(raw, |ctx| {
            intents = self.ui.show(ctx, &frame, &mut self.npc_context, now);
        });
        let platform = egui.paint.prepare(&egui.context, output);
        egui.state.handle_platform_output(window, platform);
        for intent in intents {
            self.ui_intent(intent);
        }
        self.sync_bindings();
        if settings_was_open != self.ui.settings.open {
            self.retire_held_input();
        }
        let preferences = &self.ui.settings.preferences;
        if let (
            Ok(settings_core::SettingValue::Integer(fov)),
            Ok(settings_core::SettingValue::Number(sensitivity)),
        ) = (
            preferences.value(mmorpg_client::settings::FIELD_OF_VIEW),
            preferences.value(mmorpg_client::settings::LOOK_SENSITIVITY),
        ) {
            self.camera.set_preferences(
                *fov as f32,
                *sensitivity as f32,
                matches!(
                    preferences.value(mmorpg_client::settings::INVERT_VERTICAL_LOOK),
                    Ok(settings_core::SettingValue::Bool(true))
                ),
            );
        }
        self.presentation.set_reduced_motion(matches!(preferences.value(mmorpg_client::settings::REDUCE_MOTION), Ok(settings_core::SettingValue::Choice(value)) if value == "reduce"));
        let focused = self.ui.chat_focused();
        if focused && !self.chat_focused {
            self.retire_held_input();
        }
        self.chat_focused = focused;
    }

    /// The HUD and the `casting` control layer follow the latest projection.
    fn follow_projection(&mut self, now: Instant) {
        self.update_guard();
        let latest = self.presentation.latest();
        let casting = latest.is_some_and(|latest| latest.viewer.cast.is_some());
        self.controls.set_casting(casting);
        self.pad.set_casting(casting);
        self.hud = latest.map(CombatHud::from_projection);
        if let Some(choice) = latest.and_then(|latest| latest.viewer.class) {
            // The zone keeps the class; the bar follows the projected kit.
            self.choosing = false;
            self.abilities = class_kit(choice.class);
        }
        if let Some(snapshot) = latest {
            let names = |entity| self.presentation.unit_name(entity);
            self.ui
                .observe(snapshot, self.presentation.content(), &names, now);
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
                self.egui = Some(EguiWindow::new(&window));
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
        let binding_modifiers = self.binding_modifiers();
        if self.ui.settings.open
            && let WindowEvent::KeyboardInput { event, .. } = &event
            && event.state == ElementState::Pressed
            && let PhysicalKey::Code(code) = event.physical_key
            && self.ui.settings.bindings.capture_key_with_modifiers(
                code,
                event.repeat,
                binding_modifiers.clone(),
            )
        {
            return;
        }
        if matches!(event, WindowEvent::Focused(false)) {
            self.ui.settings.bindings.cancel_capture();
        }
        // The interface sees pointer and window events; the keyboard only while
        // the chat field owns it, so focused buttons never steal movement keys.
        let keyboard = matches!(
            event,
            WindowEvent::KeyboardInput { .. } | WindowEvent::Ime(_)
        );
        let chat_owns_keyboard =
            self.chat_focused || self.ui.chat_focused() || self.ui.settings.open;
        let ui_consumed = match (self.egui.as_mut(), self.window.as_ref()) {
            (Some(egui), Some(window)) if !keyboard || chat_owns_keyboard => {
                egui.state.on_window_event(window, &event).consumed
            }
            _ => false,
        };
        if keyboard && chat_owns_keyboard {
            if let WindowEvent::KeyboardInput { event, .. } = &event
                && let PhysicalKey::Code(code) = event.physical_key
                && event.state == ElementState::Released
            {
                self.controls.key_up(code);
            }
            return;
        }
        match event {
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::CloseRequested => {
                self.stop();
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(size.width, size.height);
                }
            }
            WindowEvent::Focused(focused) if focused => self.focused = true,
            WindowEvent::Focused(_) => {
                self.focused = false;
                self.pad.retire("focusLost");
                self.controls.focus_lost();
                self.retire_pointer();
                self.update_movement();
            }
            // A press on the interface stays there; releases always reach the
            // world so a drag that ends over a panel still lets go.
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                ..
            } if ui_consumed => {}
            WindowEvent::MouseWheel { .. } if ui_consumed => {}
            WindowEvent::MouseInput { state, button, .. } => {
                let index = match button {
                    MouseButton::Left => 0,
                    MouseButton::Middle => 1,
                    MouseButton::Right => 2,
                    MouseButton::Back => 3,
                    MouseButton::Forward => 4,
                    MouseButton::Other(index) => index,
                };
                let selecting = self.world_controls.selecting();
                let click = self.world_controls.pointer_button(
                    index,
                    state == ElementState::Pressed,
                    binding_modifiers.clone(),
                );
                if state == ElementState::Pressed {
                    if !selecting && self.world_controls.selecting() {
                        self.pointer_origin = self.cursor;
                    }
                    if matches!(button, MouseButton::Left | MouseButton::Right) {
                        self.orbit_buttons.insert(button);
                    }
                } else {
                    if click && self.pointer_origin.take().is_some() {
                        self.select_pointer(event_loop);
                    }
                    self.orbit_buttons.remove(&button);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let (Some(previous), Some(offset)) = (self.cursor, self.guard_offset.as_mut())
                    && self.orbit_buttons.is_empty()
                {
                    let limit = 2. * GUARD_LANE_THRESHOLD_PX;
                    *offset = (*offset + position.y - previous.y).clamp(-limit, limit);
                }
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
                    for action in self.world_controls.key_with_modifiers(
                        code,
                        event.state == ElementState::Pressed,
                        event.repeat,
                        binding_modifiers.clone(),
                    ) {
                        let handled = self.world_action(action);
                        // A handled cancel or door use consumes the key (Enter does not chat).
                        cancel_handled |=
                            matches!(action, WorldAction::Cancel | WorldAction::Interact)
                                && handled;
                    }
                    // Enter that opened a selected NPC's context does not also chat.
                    let context_opened = !context_was_open && self.npc_context.npc.is_some();
                    self.show_title(Instant::now());
                    if context_was_open || context_opened || cancel_handled {
                        if event.state == ElementState::Released {
                            self.controls.key_up(code);
                            self.update_movement();
                        }
                        return;
                    }

                    if event.state == ElementState::Pressed {
                        let actions = self.controls.key_down_with_modifiers(
                            code,
                            event.repeat,
                            binding_modifiers.clone(),
                        );
                        // Escape cancels a cast through the controls, otherwise a
                        // fresh press closes; a held Escape's repeats never close.
                        if code == KeyCode::Escape && actions.is_empty() {
                            if !event.repeat && self.ui.close_panels() {
                                return;
                            }
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
                        refusal,
                    } => {
                        if self.connection_epoch != Some(connection_epoch) {
                            self.presentation.reset(now);
                            self.ui.reset();
                            self.npc_context = NpcContext::default();
                            self.world_controls.set_interaction(false);
                            self.retire_pointer();
                            self.connection_epoch = Some(connection_epoch);
                        }
                        if let Err(error) = self.presentation.push((*snapshot).clone(), now) {
                            self.fail(event_loop, error);
                            return;
                        }
                        self.follow_projection(now);
                        if let Some(refusal) = refusal {
                            let receipt = (connection_epoch, refusal);
                            if self.last_refusal != Some(receipt) {
                                self.last_refusal = Some(receipt);
                                self.npc_context.feedback = format!("refused: {:?}", refusal.code);
                            }
                        }
                        // Dying lets go of held movement; living again resumes it.
                        self.update_movement();
                        self.show_title(now);
                    }
                    NetworkUpdate::Failed(error) => {
                        self.fail(event_loop, error);
                        return;
                    }
                }
                self.poll_pads(now);
                // While choosing, the viewer turns on the turntable facing the camera.
                let preview = self.choosing.then(|| {
                    let preview = self.ui.preview();
                    let _ = self.presentation.set_headwear(Some(preview.headwear.id()));
                    let yaw =
                        self.camera.yaw() + std::f32::consts::PI + preview.degrees.to_radians();
                    (preview.choice, yaw.rem_euclid(std::f32::consts::TAU))
                });
                self.presentation.set_preview(preview);
                let view = self
                    .presentation
                    .camera_view(&mut self.camera, &self.world, now);
                let frame = self.presentation.frame(&self.world, now, view);
                self.interface(now, view);
                // The egui action bar replaces the flat combat HUD rectangles.
                let overlay = self
                    .egui
                    .as_mut()
                    .map(|egui| &mut egui.paint as &mut dyn mmorpg_client::graphics::Overlay);
                if let Some(renderer) = &mut self.renderer
                    && let Err(error) = renderer.render(
                        &frame,
                        &[],
                        view,
                        self.presentation.picked_draws(),
                        overlay,
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

    fn keyboard(keys: &[KeyCode]) -> PadMovement {
        let mut controls = NativeControls::new();
        for key in keys {
            controls.key_down(*key, false);
        }
        controls.movement()
    }

    #[test]
    fn the_pad_stick_combines_with_held_keys() {
        let stick = PadMovement {
            forward: 1,
            strafe: -1,
            steering: true,
        };
        let back = keyboard(&[KeyCode::KeyS]);
        assert_eq!(
            held_movement(back, stick, false),
            HeldMovement {
                forward: 0,
                strafe: -1,
                steering: true
            }
        );
        assert_eq!(
            held_movement(PadMovement::default(), stick, false),
            HeldMovement {
                forward: 1,
                strafe: -1,
                steering: true
            }
        );
        assert!(!held_movement(PadMovement::default(), stick, true).steering);
    }

    #[test]
    fn the_dead_hold_no_movement() {
        let keys = keyboard(&[KeyCode::KeyW, KeyCode::KeyA]);
        assert_eq!(
            held_movement(keys, PadMovement::default(), false),
            HeldMovement {
                forward: 1,
                strafe: -1,
                steering: true
            }
        );
        assert_eq!(
            held_movement(keys, PadMovement::default(), true),
            HeldMovement {
                forward: 0,
                strafe: 0,
                steering: false
            }
        );
        let opposed = keyboard(&[KeyCode::KeyS, KeyCode::ArrowUp, KeyCode::KeyE]);
        assert_eq!(
            held_movement(opposed, PadMovement::default(), false),
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
