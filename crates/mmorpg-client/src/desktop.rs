use mmorpg_client::session::{CharacterSelection, NetworkUpdate, PartyClient, PlayerInput};
use mmorpg_client::{
    ClientError,
    camera::{CameraControl, CameraMode, CameraView, OrbitCamera, PointerOwner},
    controls::{ControlAction, MinionCommand, NativeControls},
    gamepad::{
        PadAction, PadControls, PadMovement, PadState, camera_orbit, guard_lane, interface_key,
        read_pad,
    },
    graphics::WindowRenderer,
    graphics::project_to_viewport,
    interaction::NpcContext,
    presentation::Presentation,
    roster::{Roster, config_dir},
    stage,
    ui::{GameUi, UiFrame, UiIntent, overlay::EguiWindow},
    world::WorldScene,
    world_controls::{WorldAction, WorldControls},
};
use mmorpg_client_model::hud::{CombatHud, class_kit};
use mmorpg_client_model::manual_aim;
use mmorpg_client_model::mouse_owner::{
    AimStyle, Begin, Gesture, Hold, HoldCamera, HoldRelease, LeftClick, MouseOwners, RightClick,
    WheelRelease,
};
use mmorpg_client_model::pie_menu::{MinionCall, Wheel, WheelAction, WheelKind, WheelUsage};
use mmorpg_client_model::power_draw::{DrawRelease, PowerDraw};
use mmorpg_core::{EntityKind, EntityRef, PlayerClass, Sex, ZoneContent, ZoneId};
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
    event::{DeviceEvent, ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, DeviceEvents, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowId},
};

/// A zone's scenery and static world built off the window thread for Enter World.
type Preparing = (ZoneId, std::thread::JoinHandle<(Scenery, WorldScene)>);

/// How long one stage frame may spend uploading the zone's first frame after entry (#726).
const UPLOAD_SHARE: Duration = Duration::from_millis(8);

/// The selection stage, shown instead of the zone from the window's start until the zone's
/// first frame is resident on the GPU.
struct Stage {
    presentation: Presentation,
    world: WorldScene,
    /// The zone is entered: its first frame uploads a share per frame while the stage shows.
    entered: bool,
}

/// Builds zone `zone`'s scenery and static world off the window thread for Enter World. Once
/// the zone's first projection arrives (or after 30 s without one) it also composes one frame
/// of the zone around the viewer (#726): that imports the trees, rocks, buildings and unit
/// models the frame draws and lowers the static scenery around the viewer into the returned
/// world, so the window thread's first zone frame only uploads.
fn prepare_zone(
    zone: ZoneId,
    updates: watch::Receiver<NetworkUpdate>,
    headwear: &'static str,
) -> (Scenery, WorldScene) {
    let scenery = mmorpg_scenery::scenery_for_zone(zone);
    let world = WorldScene::new(&scenery);
    // The trees, rocks and climbing kit and the ground textures, imported once per process.
    let _ = mmorpg_client::nature::library();
    let _ = mmorpg_client::ground::library();
    let waited = Instant::now() + Duration::from_secs(30);
    let snapshot = loop {
        if let NetworkUpdate::Snapshot { snapshot, .. } = &*updates.borrow()
            && snapshot.zone_id == zone
        {
            break Some(Arc::clone(snapshot));
        }
        if Instant::now() >= waited {
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let now = Instant::now();
    let content = mmorpg_core::content_for_zone(zone);
    if let Some(snapshot) = snapshot
        && let Ok(mut presentation) =
            Presentation::new(snapshot.viewer_id, scenery.clone(), content, now)
        && presentation.set_headwear(Some(headwear)).is_ok()
        && presentation.push((*snapshot).clone(), now).is_ok()
    {
        let mut camera = OrbitCamera::default();
        let _ = presentation.camera_view(&mut camera, &world, now);
        let view = camera.view_above_ground(presentation.camera_target(now), |x, z| {
            world.surface_y(x, z)
        });
        let _ = presentation.frame(&world, now, view);
    }
    (scenery, world)
}

/// Opens the window. `loaded` is the connected player's ID with its zone's scenery and world;
/// without it the window opens on the selection stage and builds zone `zone_id` at Enter World.
#[allow(clippy::too_many_arguments)]
pub fn run(
    runtime: Arc<tokio::runtime::Runtime>,
    zone_id: ZoneId,
    loaded: Option<(u32, Scenery, WorldScene)>,
    content: Arc<ZoneContent>,
    input: watch::Sender<PlayerInput>,
    characters: watch::Sender<Option<CharacterSelection>>,
    updates: watch::Receiver<NetworkUpdate>,
    party: PartyClient,
    frames: Option<u32>,
    character: Option<(PlayerClass, Sex)>,
) -> Result<(), ClientError> {
    let event_loop = EventLoop::new()?;
    let now = Instant::now();
    let (player_id, scenery, world, stage) = match loaded {
        Some((player_id, scenery, world)) => (player_id, scenery, world, None),
        None => {
            // The selection stage stands in for the zone until Enter World.
            let scenery = stage::scenery(&content);
            let world = stage::world(&scenery);
            let shown = stage::presentation(scenery.clone(), Arc::clone(&content), zone_id, now)?;
            let stage = Stage {
                presentation: shown,
                world: world.clone(),
                entered: false,
            };
            (0, scenery, world, Some(stage))
        }
    };
    let mut ui = GameUi::new(&scenery);
    if stage.is_some() {
        // The window opens on the logo, which dissolves into the selection stage.
        ui.play_splash();
    }
    if character.is_none() {
        // The local roster offers its characters; `--class` skips it.
        ui.set_roster(
            config_dir()
                .ok_or_else(|| "No configuration directory for the local roster.".to_owned())
                .and_then(|dir| Roster::load(&dir)),
        );
    }
    let mut presentation = Presentation::new(player_id, scenery, content, now)?;
    if let Some((class, _)) = character {
        presentation.set_default_appearance(class);
    }
    let mut app = App {
        runtime,
        window: None,
        renderer: None,
        world,
        presentation,
        stage,
        preparing: None,
        zone_id,
        title: String::new(),
        connection_epoch: None,
        zone: None,
        last_refusal: None,
        input,
        characters,
        updates,
        party,
        camera: OrbitCamera::default(),
        camera_control: CameraControl::default(),
        grabbed: false,
        last_look_frame: Instant::now(),
        orbit_buttons: HashSet::new(),
        cursor: None,
        mouse: MouseOwners::default(),
        wheels: Wheels::default(),
        quit: false,
        recapture_click: false,
        pad_guard: None,
        error: None,
        frames,
        rendered: 0,
        next_frame: Instant::now(),
        abilities: character
            .map(|(class, _)| class_kit(class))
            .unwrap_or_default(),
        controls: NativeControls::new(),
        power_draw: PowerDraw::default(),
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
        audio: mmorpg_client::audio::GameAudio::new(),
    };
    event_loop.run_app(&mut app)?;
    match app.error {
        Some(error) => Err(error.into()),
        None => Ok(()),
    }
}

/// Paints an open radial wheel at the screen centre (#653): one label per sector, north first and
/// clockwise, the pointed-at sector lit, gaps as a dash; the centre ring is the dead zone.
fn draw_wheel(ctx: &egui::Context, labels: &[Option<String>], highlight: Option<usize>) {
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::unique("pie-wheel"),
    ));
    let center = ctx.content_rect().center();
    let radius = 118.0;
    let gold = egui::Color32::from_rgb(255, 178, 74);
    let parchment = egui::Color32::from_rgb(245, 237, 211);
    painter.circle_filled(center, radius + 46.0, egui::Color32::from_black_alpha(150));
    painter.circle_stroke(center, radius + 46.0, egui::Stroke::new(1.5, gold));
    painter.circle_stroke(center, 24.0, egui::Stroke::new(1.0, parchment));
    #[allow(clippy::cast_precision_loss)]
    let step = std::f32::consts::TAU / labels.len().max(1) as f32;
    for (index, label) in labels.iter().enumerate() {
        #[allow(clippy::cast_precision_loss)]
        let turn = index as f32 * step;
        let at = center + egui::vec2(turn.sin(), -turn.cos()) * radius;
        let lit = highlight == Some(index);
        if lit {
            painter.circle_filled(at, 38.0, gold.gamma_multiply(0.35));
            painter.line_segment(
                [center + (at - center) * 0.22, center + (at - center) * 0.62],
                egui::Stroke::new(2.0, gold),
            );
        }
        painter.text(
            at,
            egui::Align2::CENTER_CENTER,
            label.as_deref().unwrap_or("—"),
            egui::FontId::proportional(if lit { 15.0 } else { 13.0 }),
            if lit { gold } else { parchment },
        );
    }
}

/// Radial wheels (#653, Atlas `5.2.i653-2`): an option, off by default.
#[derive(Default)]
struct Wheels {
    /// `controls.wheels`.
    enabled: bool,
    /// The fixed wheel's sector choices, north first, clockwise.
    sectors: Vec<String>,
    /// When the open wheel's key went down: a quick tap keeps it open for a click.
    pressed_at: Option<Instant>,
    /// The opt-in local counters (`controls.wheel_usage`), saved to `wheel-usage.json` in the
    /// configuration directory; never sent anywhere.
    usage: Option<WheelUsage>,
    /// When the counters last advanced and were last written.
    counted_at: Option<Instant>,
    saved_at: Option<Instant>,
}

impl Wheels {
    fn usage_path() -> Option<std::path::PathBuf> {
        config_dir().map(|dir| dir.join("wheel-usage.json"))
    }

    /// Turns the counters on (reading the earlier file) or off (writing them a last time).
    fn set_counting(&mut self, on: bool) {
        match (on, self.usage.is_some()) {
            (true, false) => {
                let text = Self::usage_path()
                    .and_then(|path| std::fs::read_to_string(path).ok())
                    .unwrap_or_default();
                self.usage = Some(WheelUsage::from_json(&text));
                self.counted_at = None;
            }
            (false, true) => {
                self.save();
                self.usage = None;
            }
            _ => {}
        }
    }

    /// Adds the frame's time under the current owner and the wheel events; saves every 10 s.
    fn count(&mut self, mouse: &mut MouseOwners, now: Instant) {
        let tally = mouse.take_tally();
        let Some(usage) = self.usage.as_mut() else {
            return;
        };
        usage.add(tally);
        if let Some(at) = self.counted_at {
            let ms = now.saturating_duration_since(at).as_secs_f64() * 1000.0;
            // A stalled frame (a debugger, a suspended laptop) is not play time.
            usage.spend(mouse.owner(), ms.min(1000.0));
        }
        self.counted_at = Some(now);
        if self
            .saved_at
            .is_none_or(|at| now.saturating_duration_since(at) >= Duration::from_secs(10))
        {
            self.saved_at = Some(now);
            self.save();
        }
    }

    fn save(&self) {
        if let (Some(usage), Some(path)) = (&self.usage, Self::usage_path()) {
            let _ = mmorpg_client::settings::atomic_write(&path, usage.to_json().as_bytes());
        }
    }
}

struct App {
    runtime: Arc<tokio::runtime::Runtime>,
    window: Option<Arc<Window>>,
    renderer: Option<WindowRenderer>,
    /// Static geometry uploaded when the window's renderer is created.
    world: WorldScene,
    presentation: Presentation,
    /// The selection stage, shown instead of the zone until the zone's first frame is resident
    /// after Enter World; `world` and `presentation` are placeholders until the zone is entered.
    stage: Option<Stage>,
    /// The zone's scenery and world, building since Enter World.
    preparing: Option<Preparing>,
    /// The zone the window connects to.
    zone_id: ZoneId,
    /// The window title last set, so it changes only when its text does.
    title: String,
    connection_epoch: Option<u32>,
    /// The zone of the projections presented, once one arrived.
    zone: Option<mmorpg_core::ZoneId>,
    last_refusal: Option<(u32, mmorpg_client::session::RefusalReceipt)>,
    input: watch::Sender<PlayerInput>,
    characters: watch::Sender<Option<CharacterSelection>>,
    updates: watch::Receiver<NetworkUpdate>,
    party: PartyClient,
    camera: OrbitCamera,
    camera_control: CameraControl,
    /// Actual OS grab, not an assumption about the window manager.
    grabbed: bool,
    last_look_frame: Instant,
    /// Held mouse buttons that orbit the camera while the cursor moves.
    orbit_buttons: HashSet<MouseButton>,
    cursor: Option<PhysicalPosition<f64>>,
    /// One mouse owner at a time, the shared rule the browser also runs (#636): free look,
    /// focus, or a held strike or guard aimed with mouse motion (#154, #156).
    mouse: MouseOwners,
    /// Radial wheels (#653): their settings, the held key's press time and the usage counters.
    wheels: Wheels,
    /// A left press in released free look: its click selects under the cursor, or recaptures.
    recapture_click: bool,
    /// The game menu's Quit was chosen; the event loop exits on its next wait.
    quit: bool,
    /// The lane the pad's held left trigger aims with the right stick (#156).
    pad_guard: Option<mmorpg_core::StrikeHeight>,
    error: Option<String>,
    frames: Option<u32>,
    rendered: u32,
    next_frame: Instant,
    /// Ability IDs on action-bar slots 1–10: the class kit in catalog order.
    abilities: Vec<u8>,
    /// Ability slots and cast cancelling through the shared input runtime.
    controls: NativeControls,
    /// The held Aimed or Piercing Shot draw, the shared rule the browser also runs (#607).
    power_draw: PowerDraw,
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
    /// Combat sounds (#447); `None` when the bundle manifest is unusable.
    audio: Option<mmorpg_client::audio::GameAudio>,
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

/// W/S run and backpedal, A/D strafe, combined with the pad's left
/// stick. The session's movement outbox sends nothing held while the player
/// is dead (#543).
fn held_movement(keyboard: PadMovement, pad: PadMovement) -> HeldMovement {
    HeldMovement {
        forward: (keyboard.forward + pad.forward).clamp(-1, 1),
        strafe: (keyboard.strafe + pad.strafe).clamp(-1, 1),
        steering: keyboard.steering || pad.steering,
    }
}

/// Browser-style pixel scrolling: this many pixels count as one wheel line.
const PIXELS_PER_WHEEL_LINE: f64 = 40.0;

impl App {
    /// Whether an interface (a window, the menu, chat, Settings, an NPC context) or a lost
    /// focus or connection owns the pointer.
    fn interface_owns_pointer(&self) -> bool {
        !self.focused
            || !self.connected()
            || self.choosing
            || self.chat_focused
            || self.ui.settings.open
            || self.npc_context.npc.is_some()
            || self.ui.windows_open(self.presentation.latest())
    }

    fn pointer_owner(&self) -> PointerOwner {
        self.camera_control.pointer_owner(
            self.mouse.gesture().is_some(),
            self.interface_owns_pointer(),
        )
    }

    fn sync_cursor(&mut self) {
        // An interface ends a held gesture without firing (#636).
        let interface = self.interface_owns_pointer();
        if interface && self.mouse.gesture().is_some() {
            self.interrupt_gesture();
        }
        // A gesture keeps the capture free look had: its motion stays relative and the cursor
        // never jumps (#636).
        // A manual aim (#649) captures the pointer even from focus: its motion steers the
        // shoulder camera, and the focus's free cursor returns when it ends.
        let wants_grab = self.camera_control.pointer_owner(false, interface)
            == PointerOwner::Camera
            || (!interface && self.mouse.gesture() == Some(Gesture::Aim));
        if wants_grab == self.grabbed {
            return;
        }
        let Some(window) = &self.window else { return };
        if wants_grab {
            // Locked pointer supplies raw DeviceEvent mouse motion even at the window edge.
            if window
                .set_cursor_grab(winit::window::CursorGrabMode::Locked)
                .or_else(|_| window.set_cursor_grab(winit::window::CursorGrabMode::Confined))
                .is_ok()
            {
                self.grabbed = true;
                window.set_cursor_visible(false);
            }
        } else {
            let _ = window.set_cursor_grab(winit::window::CursorGrabMode::None);
            window.set_cursor_visible(true);
            self.grabbed = false;
            self.cursor = None;
            self.retire_pointer();
        }
    }

    /// A projected target drives the view, never selection authority or hit checks.
    fn track_focus(&mut self, now: Instant) {
        let dt = now
            .saturating_duration_since(self.last_look_frame)
            .as_secs_f32()
            .min(1.0 / 15.0);
        self.last_look_frame = now;
        let latest = self.presentation.latest();
        let selected = latest.and_then(|snapshot| snapshot.viewer.target);
        let opponent = latest.and_then(|snapshot| {
            if snapshot.viewer.dead {
                return None;
            }
            let entity = snapshot
                .entities
                .iter()
                .find(|entity| Some(entity.entity()) == selected)?;
            (entity.flags.attackable
                && !entity.flags.dead
                && matches!(entity.kind, EntityKind::Creature | EntityKind::Player))
            .then_some(entity)
        });
        let target = opponent.map(|entity| entity.entity());
        if let CameraMode::Focus(previous) = self.camera_control.mode()
            && target.is_none()
            && selected == Some(previous)
        {
            // The zone's target can outlive the projected/dead body; clear it once.
            self.input.send_modify(|input| {
                input.target = None;
                input.selections = input.selections.wrapping_add(1);
            });
        }
        self.camera_control.set_focus(target);
        self.mouse.set_focus(target.is_some());
        // A manual aim (#649) hands the camera to the mouse; it keeps the focus and turns back to
        // it smoothly when the aim ends.
        let aiming = self.mouse.gesture() == Some(Gesture::Aim);
        let opponent = opponent
            .filter(|_| !aiming)
            .map(|entity| entity.position.map(|cm| cm as f32 / 100.0));
        self.camera
            .track(self.presentation.camera_target(now), opponent, dt);
        self.sync_cursor();
        self.update_movement();
    }

    fn retire_pointer(&mut self) {
        self.pointer_origin = None;
        self.orbit_buttons.clear();
        self.world_controls.reset();
    }

    fn connected(&self) -> bool {
        matches!(*self.updates.borrow(), NetworkUpdate::Snapshot { .. })
            && !self.presentation.is_stalled(Instant::now())
    }

    /// Selects (or swings the door) under the cursor or crosshair; `true` if anything was picked.
    fn select_pointer(&mut self, event_loop: &ActiveEventLoop) -> bool {
        if !self.connected() {
            return false;
        }
        let cursor = if self.grabbed {
            let Some(size) = self.window.as_ref().map(|window| window.inner_size()) else {
                return false;
            };
            PhysicalPosition::new(f64::from(size.width) / 2.0, f64::from(size.height) / 2.0)
        } else {
            let Some(cursor) = self.cursor else {
                return false;
            };
            cursor
        };
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
                return false;
            }
            None => None,
        };
        // A click on the door leaf asks the zone to swing it; the zone checks reach (#158).
        if let Some(mmorpg_client::picking::Picked::Door(door)) = picked {
            let command = self.presentation.toggle_door(door);
            self.send_interaction(command);
            return true;
        }
        let target = match picked {
            Some(mmorpg_client::picking::Picked::Unit(target)) => Some(target),
            _ => None,
        };
        if picked.is_none() && matches!(self.camera_control.mode(), CameraMode::Focus(_)) {
            // With Escape now the game menu (#636), a focused click on empty world clears it.
            self.input.send_modify(|input| {
                input.target = None;
                input.selections = input.selections.wrapping_add(1);
            });
        }
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
        picked.is_some()
    }

    fn world_action(&mut self, action: WorldAction) -> bool {
        match action {
            WorldAction::Cancel => {
                // Escape is the game menu (#636): an NPC context or an open window closes first,
                // otherwise the menu opens. Right click, not Escape, cancels gestures and casts.
                if self.npc_context.npc.is_some() {
                    self.npc_context.close();
                    self.world_controls.set_interaction(false);
                    self.retire_pointer();
                } else if !self.ui.close_window(self.presentation.latest()) {
                    self.open_game_menu();
                }
                self.sync_cursor();
                return true;
            }
            WorldAction::Interact if self.connected() => {
                if let Some(command) = self.presentation.reachable_raid_prep() {
                    // On a forewarned evening, reads the tracks, reports them to the lookout
                    // or braces the barn in reach (#532), even with the lookout selected.
                    self.send_interaction(command);
                    return true;
                } else if let Some(EntityRef::Npc(npc)) =
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
                } else if let Some(command) = self.presentation.reachable_seat() {
                    // Or takes (or leaves) the seat in reach (#619).
                    self.send_interaction(command);
                    return true;
                } else if let Some(command) = self.presentation.reachable_repair() {
                    // Or repairs a damaged structure piece in reach (#305).
                    self.send_interaction(command);
                    return true;
                } else if let Some(command) = self.presentation.reachable_raid_aid() {
                    // Or helps the night raid's aftermath in reach (#531).
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
            // A dismiss rebound to a key; the mouse button dismisses from its pointer edge.
            WorldAction::Dismiss => self.dismiss(),
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

    /// Free-look or focus always steers facing; A/D remain character-local strafing.
    fn update_movement(&self) {
        let HeldMovement {
            forward,
            strafe,
            steering,
        } = held_movement(self.controls.movement(), self.pad.movement());
        let facing = (steering
            || self.grabbed
            || matches!(self.camera_control.mode(), CameraMode::Focus(_)))
        .then(|| self.camera.facing());
        // Held Jump ascends and held Descend descends while swimming (#274); the zone ignores
        // the rise out of the water.
        let rise = (self.controls.rise() + self.pad.rise()).clamp(-1, 1);
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

    /// One jump; the session drops presses made while dead.
    fn jump(&self) {
        self.input
            .send_modify(|input| input.jumps = input.jumps.wrapping_add(1));
    }

    /// A dodge roll (#516), dropped like a jump while dead; the zone decides whether it starts.
    fn roll(&self) {
        self.input
            .send_modify(|input| input.rolls = input.rolls.wrapping_add(1));
    }

    /// One weapon swing where the character faces (#517); the zone resolves what it hits, and
    /// a press while dead sends nothing.
    fn swing(&mut self) {
        if self.connected() && !self.dead() {
            self.drop_power_draw();
            self.input
                .send_modify(|input| input.swings = input.swings.wrapping_add(1));
        }
    }

    /// Lets go of a held draw without a shot; `true` if there was one (#607).
    fn drop_power_draw(&mut self) -> bool {
        let dropped = self.power_draw.cancel();
        if dropped {
            self.presentation.abandon_attack_preparation();
        }
        dropped
    }

    /// A gesture ended without firing (#636): a strike's wind-up goes, a guard lowers.
    fn gesture_ended(&mut self, ended: Option<Gesture>) {
        match ended {
            Some(Gesture::Hold(Hold::Strike)) => self.presentation.abandon_attack_preparation(),
            Some(Gesture::Hold(Hold::Guard)) => self.update_guard(),
            // A manual aim (#649) lowers its wind-up; the camera goes back to the focus.
            Some(Gesture::Aim) => {
                self.presentation.abandon_attack_preparation();
                self.sync_cursor();
            }
            Some(Gesture::Wheel) | None => {}
        }
    }

    /// Throws the aimed `ability` along the aim (#649): the character faces the camera and its
    /// slot is used as usual; the zone decides the flight, landing and hits.
    fn throw_aimed(&mut self, ability: u8) {
        self.update_movement();
        let slot = self
            .abilities
            .iter()
            .position(|&held| held == ability)
            .and_then(|index| u8::try_from(index + 1).ok());
        if let Some(slot) = slot {
            self.control(ControlAction::AbilitySlot(slot));
        }
        self.sync_cursor();
    }

    /// Ends whatever gesture owns the mouse without firing: focus loss, a menu or window,
    /// a retired key.
    fn interrupt_gesture(&mut self) {
        let ended = self.mouse.interrupt();
        self.gesture_ended(ended);
    }

    /// Right click (#636): cancels a held gesture without firing, otherwise the cast; it also
    /// drops a held draw (#607).
    fn dismiss(&mut self) {
        self.drop_power_draw();
        match self.mouse.right_click() {
            RightClick::Cancel(gesture) => self.gesture_ended(Some(gesture)),
            RightClick::CancelCast => self.control(ControlAction::CancelCast),
        }
    }

    /// A wheel key went down (#653): with wheels on, opens the fixed or context wheel from free
    /// look or focus; never over a window, a hold or an aim. The camera holds still meanwhile.
    fn open_wheel(&mut self, kind: WheelKind) {
        if !self.wheels.enabled || !self.connected() {
            return;
        }
        let latest = self.presentation.latest();
        if self.ui.windows_open(latest) {
            return;
        }
        let wheel = match kind {
            WheelKind::Fixed => {
                let choices: Vec<&str> = self.wheels.sectors.iter().map(String::as_str).collect();
                Wheel::fixed(&choices)
            }
            WheelKind::Context => match latest {
                Some(latest) => Wheel::context(latest, self.presentation.content()),
                None => return,
            },
        };
        if self.mouse.open_wheel(wheel) {
            self.drop_power_draw();
            self.wheels.pressed_at = Some(Instant::now());
        }
    }

    /// The wheel keys that went up: a flick picks, a quick tap keeps the wheel open for a left
    /// click, anything else ends it without an action.
    fn follow_wheel_releases(&mut self) {
        for retired in self.controls.drain_wheel_releases() {
            let held_ms = self
                .wheels
                .pressed_at
                .map_or(f64::INFINITY, |at| at.elapsed().as_secs_f64() * 1000.0);
            if let WheelRelease::Pick(action) = self.mouse.release_wheel(held_ms, retired) {
                self.run_wheel_action(action);
            }
        }
    }

    /// Runs a wheel's chosen action exactly as its own key would; the zone decides the outcome.
    fn run_wheel_action(&mut self, action: WheelAction) {
        match action {
            WheelAction::Slot(slot) => self.control(ControlAction::AbilitySlot(slot)),
            WheelAction::Swing => self.control(ControlAction::Swing),
            WheelAction::Roll => self.control(ControlAction::Roll),
            WheelAction::TargetNext => self.control(ControlAction::TargetNext),
            WheelAction::ClearTarget => self.input.send_modify(|input| {
                input.target = None;
                input.selections = input.selections.wrapping_add(1);
            }),
            // Native shows a vendor's wares in its NPC context, which interact opens.
            WheelAction::Interact | WheelAction::Trade => {
                self.world_action(WorldAction::Interact);
            }
            WheelAction::Minions(call) => self.control(ControlAction::Minions(match call {
                MinionCall::Attack => MinionCommand::Attack,
                MinionCall::Defend => MinionCommand::Defend,
                MinionCall::Follow => MinionCommand::Follow,
                MinionCall::Retreat => MinionCommand::Retreat,
            })),
            WheelAction::Emote(emote) => {
                self.send_interaction(mmorpg_core::ZoneCommand::Emote(emote));
            }
        }
        self.sync_cursor();
    }

    /// Escape with nothing to close, or the menu toggle: the game menu (#636).
    fn open_game_menu(&mut self) {
        self.retire_held_input();
        self.ui.game_menu_open = true;
    }

    /// The Warden's directed Heroic Strike key went down (#154): a living Warden starts aiming
    /// with the mouse; an aim or wheel keeps the mouse and nothing starts.
    fn begin_strike(&mut self) {
        let can_strike = !self.dead()
            && self
                .abilities
                .contains(&mmorpg_core::ability::ids::HEROIC_STRIKE.get());
        if !can_strike || !self.connected() {
            return;
        }
        if let Begin::Started { preempted } = self.mouse.begin_hold(Hold::Strike) {
            // The most recent press owns the mouse (5.2.37): a held guard lowers.
            self.gesture_ended(preempted.map(Gesture::Hold));
            self.power_draw.cancel();
            self.presentation.hold_attack_preparation(
                mmorpg_core::ability::ids::HEROIC_STRIKE.get(),
                Instant::now(),
            );
        }
    }

    /// The strike and guard keys that went up: a deliberate release of the owning hold strikes
    /// or lowers; a retirement or a stale key ends it without firing.
    fn follow_hold_releases(&mut self) {
        for (hold, retired) in self.controls.drain_hold_releases() {
            if self.mouse.gesture() != Some(Gesture::Hold(hold)) {
                continue;
            }
            if retired {
                self.interrupt_gesture();
                continue;
            }
            match self.mouse.release_hold(hold) {
                Some(HoldRelease::Strike(aim)) => {
                    let facing = self.camera.facing();
                    self.presentation.prepare_attack(
                        mmorpg_core::ability::ids::HEROIC_STRIKE.get(),
                        Instant::now(),
                    );
                    self.presentation.aim_attack_preparation(aim);
                    self.send_interaction(mmorpg_core::ZoneCommand::DirectedStrike {
                        ability: mmorpg_core::ability::ids::HEROIC_STRIKE.get(),
                        facing,
                        aim,
                    });
                }
                Some(HoldRelease::LowerGuard) | None => self.update_guard(),
            }
        }
    }

    /// The ability on `slot`, 1–20.
    fn slot_ability(&self, slot: u8) -> Option<u8> {
        usize::from(slot)
            .checked_sub(1)
            .and_then(|index| self.abilities.get(index))
            .copied()
    }

    /// A key or pad button (anything with a release) pressed `slot`: a power shot draws and
    /// waits for the release (#607); everything else goes straight to the slot's control.
    fn press_slot(&mut self, slot: u8) {
        let Some(ability) = self.slot_ability(slot) else {
            return;
        };
        let latest = self.presentation.latest();
        let interface_open = self.ui.windows_open(latest);
        // The bomb takes a temporary shoulder-camera aim (#649); a press while another gesture
        // owns the mouse does nothing.
        if latest.is_some_and(|latest| manual_aim::can_aim(latest, slot, ability, interface_open)) {
            if self.mouse.begin_aim(slot, ability) {
                self.drop_power_draw();
                self.presentation
                    .hold_attack_preparation(ability, Instant::now());
                self.sync_cursor();
            }
            return;
        }
        if self.power_draw.press(slot, ability, latest, interface_open) {
            self.presentation
                .hold_attack_preparation(ability, Instant::now());
        } else {
            self.control(ControlAction::AbilitySlot(slot));
        }
    }

    /// Tab and R are intents derived from the latest projection; the zone
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
            ControlAction::Roll => self.roll(),
            ControlAction::Descend => {}
            ControlAction::TargetNext => self.combat_key(KeyCode::Tab),
            ControlAction::Swing => self.swing(),
            ControlAction::ReleaseSpirit => self.combat_key(KeyCode::KeyR),
            // The one place a slot use is sent: a press that cannot draw, an action-bar click or
            // a held power shot's deliberate release (#607).
            ControlAction::AbilitySlot(slot) => {
                if let Some(ability) = self.slot_ability(slot) {
                    self.power_draw.cancel();
                    self.input.send_modify(|input| {
                        input.ability = ability;
                        input.ability_uses = input.ability_uses.wrapping_add(1);
                    });
                    // A reversible local wind-up until the zone answers (Atlas 6.6.10); a held
                    // draw's clip carries on unbroken.
                    self.presentation.prepare_attack(ability, Instant::now());
                }
            }
            ControlAction::CancelCast => {
                self.input.send_modify(|input| {
                    input.cancels = input.cancels.wrapping_add(1);
                });
            }
            ControlAction::TogglePanel(panel) => self.ui.toggle(panel),
            ControlAction::OpenChat => self.ui.open_chat(),
            ControlAction::DirectedStrike => self.begin_strike(),
            ControlAction::Guard => {
                // The raised shield takes the mouse (5.2.37) and supersedes an unanswered
                // wind-up or a held draw; an aim or wheel keeps the mouse and nothing rises.
                if let Begin::Started { .. } = self.mouse.begin_hold(Hold::Guard) {
                    self.power_draw.cancel();
                    self.presentation.abandon_attack_preparation();
                }
                self.update_guard();
            }
            ControlAction::Minions(command) => {
                if let Some(order) = self.minion_order(command) {
                    self.send_interaction(mmorpg_core::ZoneCommand::CommandMinions(order));
                }
            }
            ControlAction::FixedWheel => self.open_wheel(WheelKind::Fixed),
            ControlAction::ContextWheel => self.open_wheel(WheelKind::Context),
        }
    }

    /// A minion group order (#295) from the latest projection: attack the selected unit or
    /// defend where the viewer stands. The zone checks it.
    fn minion_order(&self, command: MinionCommand) -> Option<mmorpg_core::MinionOrder> {
        use mmorpg_core::MinionOrder;
        let latest = self.presentation.latest()?;
        Some(match command {
            MinionCommand::Attack => MinionOrder::Attack(latest.viewer.target?),
            MinionCommand::Defend => {
                let me = latest.entities.iter().find(|unit| {
                    unit.entity() == mmorpg_core::EntityRef::Player(latest.viewer_id)
                })?;
                MinionOrder::Defend([me.position[0], me.position[2]])
            }
            MinionCommand::Follow => MinionOrder::Follow,
            MinionCommand::Retreat => MinionOrder::Retreat,
        })
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
        if !can_guard && self.mouse.gesture() == Some(Gesture::Hold(Hold::Guard)) {
            self.mouse.interrupt();
        }
        // The key's mouse aim (the shared mouse-owner rule, #636) wins over a pad guard held at
        // the same time.
        let lane = self
            .mouse
            .guard_lane()
            .or(self.pad_guard.filter(|_| can_guard));
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
            && self.camera_control.mode() == CameraMode::FreeLook
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
            PadAction::Roll => self.roll(),
            PadAction::TargetNext => self.combat_key(KeyCode::Tab),
            PadAction::Swing => self.swing(),
            PadAction::ReleaseSpirit => self.combat_key(KeyCode::KeyR),
            PadAction::AbilitySlot(slot) => self.press_slot(slot),
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

    /// A held draw whose ability became unusable, or that an opened window interrupts, drops.
    fn follow_power_draw(&mut self) {
        if self.power_draw.drawing().is_none() {
            return;
        }
        if let Some(latest) = self.presentation.latest() {
            let interface_open = self.ui.windows_open(Some(latest));
            if self.power_draw.observe(latest, interface_open) {
                self.presentation.abandon_attack_preparation();
            }
        }
    }

    /// Sends each held slot's key or button release (#485), as the browser does on key-up:
    /// the zone reads it only for an experimental graph's hold-to-charge and chord triggers.
    /// A retirement (focus loss, a context or profile change) is sent as retired, so a held
    /// charge never acts on it.
    fn send_slot_releases(&mut self) {
        let releases = self.controls.drain_releases();
        let pad_releases = self.pad.drain_releases();
        for release in releases.into_iter().chain(pad_releases) {
            // A held aim throws on its deliberate release (#649); a retirement ends it unthrown.
            let aimed = self.mouse.gesture() == Some(Gesture::Aim);
            match self.mouse.release_aim(release.slot, release.retired) {
                Some(ability) => self.throw_aimed(ability),
                None if aimed && self.mouse.gesture().is_none() => {
                    self.gesture_ended(Some(Gesture::Aim));
                }
                None => {}
            }
            // A held power shot fires on its deliberate release, ahead of the release itself.
            match self.power_draw.release(release.slot, release.retired) {
                Some(DrawRelease::Fire(_)) => {
                    self.control(ControlAction::AbilitySlot(release.slot));
                }
                Some(DrawRelease::Drop) => self.presentation.abandon_attack_preparation(),
                None => {}
            }
            let ability = self.slot_ability(release.slot);
            if let Some(ability) = ability {
                self.input.send_modify(|input| {
                    input.ability_release = (ability, release.retired);
                    input.ability_releases = input.ability_releases.wrapping_add(1);
                });
            }
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
        self.mouse.interrupt();
        self.controls.drain_hold_releases();
        self.pad_guard = None;
        self.pad.retire("inputContextChanged");
        self.power_draw.cancel();
        self.presentation.abandon_attack_preparation();
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
            UiIntent::Party(request) => self.party.send(request),
            // A click has no release to wait for: it fires at once, power shots too (#607).
            UiIntent::AbilitySlot(slot) => self.control(ControlAction::AbilitySlot(slot)),
            UiIntent::ReleaseSpirit => self.combat_key(KeyCode::KeyR),
            UiIntent::Quit => self.quit = true,
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
                if self.stage.as_ref().is_some_and(|stage| !stage.entered)
                    && self.preparing.is_none()
                {
                    // The zone's world builds while the session connects and claims.
                    let zone = self.zone_id;
                    let updates = self.updates.clone();
                    let headwear = headwear.id();
                    self.preparing = Some((
                        zone,
                        std::thread::spawn(move || prepare_zone(zone, updates, headwear)),
                    ));
                }
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
                    name: self.ui.character_name.clone(),
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
        if let Some(profile) = profile
            && let Err(error) = self
                .controls
                .set_profile(Some(profile.clone()))
                .and_then(|()| self.world_controls.set_profile(profile.clone()))
                .and_then(|()| self.pad.set_profile(profile))
        {
            self.ui.settings.bindings.problem(error);
            return;
        }
        self.bindings_revision = revision;
    }

    /// Composes the entered zone's frame from its camera without drawing it and uploads up to
    /// [`UPLOAD_SHARE`] of it (#726); true once all of it is resident.
    fn preload_zone(&mut self, now: Instant) -> bool {
        // First-view collision framing runs once, here, for the zone's first drawn frame.
        let _ = self
            .presentation
            .camera_view(&mut self.camera, &self.world, now);
        let view = self
            .camera
            .view_above_ground(self.presentation.camera_target(now), |x, z| {
                self.world.surface_y(x, z)
            });
        let frame = self.presentation.frame(&self.world, now, view);
        self.renderer
            .as_mut()
            .is_none_or(|renderer| renderer.preload(&frame, Instant::now() + UPLOAD_SHARE))
    }

    /// Runs one egui pass over the latest projection and applies its intents.
    fn interface(&mut self, now: Instant, view: CameraView) {
        let (Some(egui), Some(window)) = (self.egui.as_mut(), self.window.as_ref()) else {
            return;
        };
        let connection = match &*self.updates.borrow() {
            _ if self.stage.is_some() && self.choosing => None,
            _ if self.stage.is_some() => Some("Entering the world…"),
            NetworkUpdate::Waiting => Some("Connecting to the world…"),
            NetworkUpdate::Reconnecting => Some("Reconnecting to the world…"),
            NetworkUpdate::Travelling(_) => Some("Travelling…"),
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
        // A manual aim's expected landing (#649): a non-authoritative aid on the ground ahead.
        let landing = (self.mouse.gesture() == Some(Gesture::Aim))
            .then(|| presentation.latest())
            .flatten()
            .and_then(|latest| manual_aim::landing(latest, self.camera.facing()))
            .and_then(|point| project(presentation.rendered(point)));
        // An open radial wheel (#653): each sector's label, north first and clockwise, and the
        // sector the mouse points at.
        let class = presentation
            .latest()
            .and_then(|latest| latest.viewer.class)
            .map(|choice| choice.class);
        let wheel = self.mouse.wheel().map(|wheel| {
            let labels: Vec<Option<String>> = wheel
                .sectors()
                .iter()
                .map(|sector| sector.map(|action| action.label(class)))
                .collect();
            (labels, self.mouse.wheel_sector())
        });
        let mut raw = egui.state.take_egui_input(window);
        raw.events.append(&mut self.pad_ui);
        let mut intents = Vec::new();
        let settings_was_open = self.ui.settings.open;
        let output = egui.context.run_pass(raw, |ctx| {
            intents = self.ui.show(ctx, &frame, &mut self.npc_context, now);
            if self.grabbed {
                let painter = ctx.layer_painter(egui::LayerId::new(
                    egui::Order::Foreground,
                    egui::Id::unique("camera-reticle"),
                ));
                let center = ctx.content_rect().center();
                painter.circle_stroke(
                    center,
                    6.0,
                    egui::Stroke::new(1.2, egui::Color32::from_rgb(245, 237, 211)),
                );
                painter.circle_filled(center, 1.0, egui::Color32::WHITE);
                if let Some(at) = landing {
                    let stroke = egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 178, 74));
                    painter.circle_stroke(at, 14.0, stroke);
                    painter.circle_filled(at, 2.5, stroke.color);
                }
            }
            if let Some((labels, highlight)) = &wheel {
                draw_wheel(ctx, labels, *highlight);
            }
        });
        let platform = egui.paint.prepare(&egui.context, output);
        egui.state.handle_platform_output(window, platform);
        for intent in intents {
            self.ui_intent(intent);
        }
        for note in self.party.notes() {
            self.ui
                .logs
                .note(mmorpg_client_model::chat::LineKind::System, note);
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
        self.mouse.set_hold_camera(
            match preferences.value(mmorpg_client::settings::HOLD_CAMERA) {
                Ok(settings_core::SettingValue::Choice(value)) => HoldCamera::from_setting(value),
                _ => HoldCamera::default(),
            },
        );
        self.mouse.set_aim_style(
            match preferences.value(mmorpg_client::settings::AIM_STYLE) {
                Ok(settings_core::SettingValue::Choice(value)) => AimStyle::from_setting(value),
                _ => AimStyle::default(),
            },
        );
        let on = |id| {
            matches!(
                preferences.value(id),
                Ok(settings_core::SettingValue::Bool(true))
            )
        };
        self.wheels.enabled = on(mmorpg_client::settings::WHEELS);
        let counting = on(mmorpg_client::settings::WHEEL_USAGE);
        self.wheels.sectors = mmorpg_client::settings::WHEEL_SECTORS
            .iter()
            .map(|id| match preferences.value(id) {
                Ok(settings_core::SettingValue::Choice(value)) => value.clone(),
                _ => String::new(),
            })
            .collect();
        self.wheels.set_counting(counting);
        if !self.wheels.enabled && self.mouse.gesture() == Some(Gesture::Wheel) {
            self.mouse.interrupt();
        }
        if let Some(audio) = &mut self.audio {
            audio.follow(preferences);
        }
        let focused = self.ui.chat_focused();
        if focused && !self.chat_focused {
            self.retire_held_input();
        }
        self.chat_focused = focused;
        self.sync_cursor();
    }

    /// The HUD and the `casting` control layer follow the latest projection.
    fn follow_projection(&mut self, now: Instant) {
        if let Some(latest) = self.presentation.latest() {
            // Death or a stun ends a held gesture without firing (#636).
            let ended = self.mouse.observe(latest);
            self.gesture_ended(ended);
        }
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
                    Some(status) => format!("MMORPG — {status}"),
                    None => "MMORPG".to_owned(),
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
    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.wheels.save();
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        event_loop.listen_device_events(DeviceEvents::WhenFocused);
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
                self.sync_cursor();
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
                // A minimised window reports a zero size on some platforms.
                if let Some(audio) = &mut self.audio {
                    audio.set_minimised(size.width == 0 || size.height == 0);
                }
            }
            // Sound suspends while the window is hidden or loses focus (#447).
            WindowEvent::Occluded(occluded) => {
                if let Some(audio) = &mut self.audio {
                    audio.set_minimised(occluded);
                }
            }
            WindowEvent::Focused(focused) if focused => {
                self.focused = true;
                if let Some(audio) = &mut self.audio {
                    audio.set_focused(true);
                }
                self.sync_cursor();
            }
            WindowEvent::Focused(_) => {
                self.focused = false;
                self.camera_control.release_cursor();
                self.sync_cursor();
                if let Some(audio) = &mut self.audio {
                    audio.set_focused(false);
                }
                self.retire_held_input();
                self.send_slot_releases();
                self.follow_hold_releases();
                self.follow_wheel_releases();
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
                let pressed = state == ElementState::Pressed;
                if pressed
                    && button == MouseButton::Left
                    && !self.grabbed
                    && self.camera_control.mode() == CameraMode::FreeLook
                    && self.mouse.gesture().is_none()
                    && self.focused
                    && self.connected()
                    && !self.ui.settings.open
                    && !self.chat_focused
                    && self.npc_context.npc.is_none()
                    && !self.ui.windows_open(self.presentation.latest())
                {
                    // A click on a unit or door under the free cursor selects it on release, as
                    // in the browser; any other click recaptures mouse-look then (#636).
                    self.recapture_click = self.cursor.is_some();
                    if !self.recapture_click {
                        self.camera_control.request_look();
                        self.sync_cursor();
                        if self.grabbed {
                            return; // Recapture is not an unintended first selection.
                        }
                    }
                } else if pressed {
                    self.recapture_click = false;
                }
                let index = match button {
                    MouseButton::Left => 0,
                    MouseButton::Middle => 1,
                    MouseButton::Right => 2,
                    MouseButton::Back => 3,
                    MouseButton::Forward => 4,
                    MouseButton::Other(index) => index,
                };
                let selecting = self.world_controls.selecting();
                let edge =
                    self.world_controls
                        .pointer_button(index, pressed, binding_modifiers.clone());
                // Right click cancels a held gesture without firing, otherwise the cast (#636).
                if edge.dismissed {
                    self.dismiss();
                    self.sync_cursor();
                    return;
                }
                if pressed {
                    if !selecting && self.world_controls.selecting() {
                        // A click drops a held draw (#607) and is nothing else; a held strike
                        // or guard ignores the left button (#636); an armed aim throws (#649).
                        let click = if self.drop_power_draw() {
                            LeftClick::Nothing
                        } else {
                            self.mouse.left_click()
                        };
                        if let LeftClick::Throw(ability) = click {
                            self.throw_aimed(ability);
                            return;
                        }
                        // A tapped-open wheel (#653) runs the pointed-at sector, or nothing.
                        if let LeftClick::Pick(action) = click {
                            if let Some(action) = action {
                                self.run_wheel_action(action);
                            }
                            return;
                        }
                        let selects = click == LeftClick::Select;
                        self.pointer_origin = selects
                            .then(|| {
                                self.cursor.or_else(|| {
                                    self.grabbed.then(|| PhysicalPosition::new(0.0, 0.0))
                                })
                            })
                            .flatten();
                    }
                    if button == MouseButton::Left && self.mouse.gesture().is_none() {
                        self.orbit_buttons.insert(button);
                    }
                } else {
                    if edge.selected
                        && self.pointer_origin.take().is_some()
                        && self.mouse.gesture().is_none()
                    {
                        let picked = self.select_pointer(event_loop);
                        if !picked && std::mem::take(&mut self.recapture_click) {
                            self.camera_control.request_look();
                            self.sync_cursor();
                        }
                    }
                    if button == MouseButton::Left {
                        self.recapture_click = false;
                    }
                    self.orbit_buttons.remove(&button);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(previous) = self.cursor.replace(position)
                    && !self.orbit_buttons.is_empty()
                    && !self.grabbed
                    && self.camera_control.mode() == CameraMode::FreeLook
                {
                    if self.pointer_origin.is_some_and(|start| {
                        (position.x - start.x).hypot(position.y - start.y) > 5.0
                    }) {
                        self.pointer_origin = None;
                    }
                    if self.pointer_origin.is_none() {
                        self.camera
                            .orbit(position.x - previous.x, position.y - previous.y);
                        self.update_movement();
                    }
                }
            }
            WindowEvent::CursorLeft { .. } => {
                if !self.grabbed {
                    self.cursor = None;
                    self.retire_pointer();
                }
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
                    // The game menu blocks game keys; Escape (world.cancel) closes it (#636).
                    if self.ui.game_menu_open
                        && event.state == ElementState::Pressed
                        && code != KeyCode::Escape
                    {
                        return;
                    }
                    let context_was_open = self.npc_context.npc.is_some();
                    let mut cancel_handled = false;
                    for action in self.world_controls.key_with_modifiers(
                        code,
                        event.state == ElementState::Pressed,
                        event.repeat,
                        binding_modifiers.clone(),
                    ) {
                        let handled = self.world_action(action);
                        // A handled cancel or interaction consumes the key.
                        cancel_handled |=
                            matches!(action, WorldAction::Cancel | WorldAction::Interact)
                                && handled;
                    }
                    // The E that opened a selected NPC's context reaches no other control.
                    let context_opened = !context_was_open && self.npc_context.npc.is_some();
                    self.show_title(Instant::now());
                    if context_was_open || context_opened || cancel_handled {
                        if event.state == ElementState::Released {
                            self.controls.key_up(code);
                            self.update_movement();
                        }
                        return;
                    }

                    // Debug builds: F4 draws every unit's authoritative body over its model (#39).
                    if cfg!(debug_assertions)
                        && code == KeyCode::F4
                        && event.state == ElementState::Pressed
                        && !event.repeat
                    {
                        self.presentation.toggle_body_overlay();
                        return;
                    }
                    if event.state == ElementState::Pressed {
                        let actions = self.controls.key_down_with_modifiers(
                            code,
                            event.repeat,
                            binding_modifiers.clone(),
                        );
                        for action in actions {
                            match action {
                                ControlAction::AbilitySlot(slot) => self.press_slot(slot),
                                action => self.control(action),
                            }
                        }
                    } else {
                        self.controls.key_up(code);
                    }
                    self.update_guard();
                    self.sync_cursor();
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
                    NetworkUpdate::Travelling(zone) => {
                        self.retire_pointer();
                        self.npc_context.close();
                        self.world_controls.set_interaction(false);
                        if let Some(window) = &self.window {
                            window
                                .set_title(&format!("MMORPG — travelling to zone {}", zone.get()));
                        }
                        self.title.clear();
                    }
                    NetworkUpdate::Snapshot { .. }
                        if self.stage.is_some()
                            && self
                                .preparing
                                .as_ref()
                                .is_some_and(|(_, build)| !build.is_finished()) =>
                    {
                        // The zone's world is still building: the stage stays and the window
                        // keeps drawing; the latest projection is taken once the world is ready.
                    }
                    NetworkUpdate::Snapshot {
                        connection_epoch,
                        snapshot,
                        refusal,
                    } => {
                        // The first projection after Enter World enters the zone behind the
                        // stage; a later one from another zone (#145) travels.
                        let entering = self.stage.as_ref().is_some_and(|stage| !stage.entered)
                            || self.zone.is_some_and(|zone| zone != snapshot.zone_id);
                        self.zone = Some(snapshot.zone_id);
                        if entering {
                            // The zone's scenery, content and player ID.
                            let zone = snapshot.zone_id;
                            let prepared = self
                                .preparing
                                .take()
                                .filter(|(prepared, _)| *prepared == zone)
                                .and_then(|(_, build)| build.join().ok());
                            let (scenery, world) = prepared.unwrap_or_else(|| {
                                let scenery = mmorpg_scenery::scenery_for_zone(zone);
                                let world = WorldScene::new(&scenery);
                                (scenery, world)
                            });
                            let content = mmorpg_core::content_for_zone(zone);
                            // The stage keeps showing until the zone's first frame is resident.
                            if let Some(stage) = &mut self.stage {
                                stage.entered = true;
                            }
                            self.world = world;
                            self.ui.set_scenery(&scenery);
                            if let Err(error) = self.presentation.enter_zone(
                                snapshot.viewer_id,
                                scenery,
                                content,
                                now,
                            ) {
                                self.fail(event_loop, error);
                                return;
                            }
                            self.connection_epoch = None;
                        }
                        if self.connection_epoch != Some(connection_epoch) {
                            self.power_draw.cancel();
                            self.presentation.reset(now);
                            self.ui.reset();
                            self.npc_context = NpcContext::default();
                            self.world_controls.set_interaction(false);
                            self.retire_pointer();
                            self.connection_epoch = Some(connection_epoch);
                            if let Some(audio) = &mut self.audio {
                                audio.reset();
                            }
                        }
                        if let Err(error) = self.presentation.push((*snapshot).clone(), now) {
                            self.fail(event_loop, error);
                            return;
                        }
                        let sounds = self.presentation.take_audio();
                        if let Some(audio) = &mut self.audio {
                            audio.apply(sounds, now);
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
                self.send_slot_releases();
                self.follow_hold_releases();
                self.follow_wheel_releases();
                self.wheels.count(&mut self.mouse, now);
                self.follow_power_draw();
                // On the stage, the viewer turns on the turntable facing the camera, also while
                // Enter World waits for the zone.
                let staged = self.choosing || self.stage.is_some();
                // Until the zone's first frame is resident the stage shows instead of the zone.
                let shown = self
                    .stage
                    .as_mut()
                    .map_or(&mut self.presentation, |stage| &mut stage.presentation);
                let preview = staged.then(|| {
                    let preview = self.ui.preview();
                    let _ = shown.set_headwear(Some(preview.headwear.id()));
                    let yaw = stage::FACING_CAMERA + preview.degrees.to_radians();
                    (preview.choice, yaw.rem_euclid(std::f32::consts::TAU))
                });
                shown.set_preview(preview);
                let view = if self.stage.is_some() {
                    // The stage has its own close camera; the orbit camera waits for the zone.
                    stage::camera(self.camera.view([0.0; 3]).field_of_view_degrees)
                } else {
                    // First-view collision framing runs once; focus then gently adjusts it.
                    let _ = self
                        .presentation
                        .camera_view(&mut self.camera, &self.world, now);
                    self.track_focus(now);
                    self.camera
                        .view_above_ground(self.presentation.camera_target(now), |x, z| {
                            self.world.surface_y(x, z)
                        })
                };
                if let Some(audio) = &mut self.audio {
                    audio.listen(view);
                    audio.pump();
                }
                let loot_window = self.ui.loot_window_open(self.presentation.latest());
                self.presentation
                    .set_interaction(loot_window, self.npc_context.npc);
                // Once entered, the zone's first frame uploads a share per stage frame (#726);
                // the stage leaves after the frame on which all of it became resident.
                let resident = self.stage.as_ref().is_some_and(|stage| stage.entered)
                    && self.preload_zone(now);
                let frame = match &mut self.stage {
                    Some(stage) => stage.presentation.frame(&stage.world, now, view),
                    None => self.presentation.frame(&self.world, now, view),
                };
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
                        self.stage
                            .as_ref()
                            .map_or(&self.presentation, |stage| &stage.presentation)
                            .picked_draws(),
                        overlay,
                    )
                {
                    self.fail(event_loop, error);
                    return;
                }
                if resident {
                    self.stage = None;
                }
                self.rendered = self.rendered.saturating_add(1);
                if self.frames.is_some_and(|limit| self.rendered >= limit) {
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }

    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _device_id: winit::event::DeviceId,
        event: DeviceEvent,
    ) {
        let DeviceEvent::MouseMotion { delta: (dx, dy) } = event else {
            return;
        };
        if self.mouse.gesture().is_some() {
            // The gesture owns the motion (#636), captured or not; a locked camera holds still.
            let turn = self.mouse.motion(dx, dy);
            if self.mouse.gesture() == Some(Gesture::Hold(Hold::Strike))
                && let Some(aim) = self.mouse.strike_aim()
            {
                self.presentation.aim_attack_preparation(aim);
            }
            self.update_guard();
            if turn && self.grabbed {
                self.camera.orbit(dx, dy);
                self.update_movement();
            }
            return;
        }
        if self.grabbed && self.pointer_owner() == PointerOwner::Camera {
            // A captured left press stays a selection click only within a few pixels of travel.
            if let Some(travel) = self.pointer_origin.as_mut() {
                travel.x += dx;
                travel.y += dy;
                if travel.x.hypot(travel.y) > 5.0 {
                    self.pointer_origin = None;
                }
            }
            self.camera.orbit(dx, dy);
            self.update_movement();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.quit {
            self.stop();
            event_loop.exit();
            return;
        }
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
            held_movement(back, stick),
            HeldMovement {
                forward: 0,
                strafe: -1,
                steering: true
            }
        );
        assert_eq!(
            held_movement(PadMovement::default(), stick),
            HeldMovement {
                forward: 1,
                strafe: -1,
                steering: true
            }
        );
    }

    #[test]
    fn held_keys_combine_and_opposed_keys_cancel() {
        let keys = keyboard(&[KeyCode::KeyW, KeyCode::KeyA]);
        assert_eq!(
            held_movement(keys, PadMovement::default()),
            HeldMovement {
                forward: 1,
                strafe: -1,
                steering: true
            }
        );
        let opposed = keyboard(&[KeyCode::KeyS, KeyCode::ArrowUp, KeyCode::KeyD]);
        assert_eq!(
            held_movement(opposed, PadMovement::default()),
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
