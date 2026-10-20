//! Independent #28 contract, authored on 28ce451 before implementation.
//! Included as a desktop child module so events go through the SAME App,
//! WindowRenderer, watch input and run_session used by the executable.
//! Pointer/focus/leave WindowEvents dispatch through the actual live App handler.
//! Keyboard events use XTest; its delivered-event assertion fails closed.
//! Direct dispatch is not a claim of OS-pointer or hardware coverage.
//! See native_pointer_contract.md for fixtures, controls and remaining evidence.
use super::*;
use game_server::{GameSimulation, SimulationError, SimulationSnapshot, SnapshotScope};
use mmorpg_client::{network::ClientSession, session::run_session};
use mmorpg_core::{
    CanonicalZoneSnapshot, CreatureAi, CreatureBehaviour, CreatureId, EntityRef, NpcId,
    PLAYER_HALF_EXTENTS_UNITS, ZoneCommand, ZoneSimulation,
};
use std::{process::Command, sync::Mutex};
use winit::{event::DeviceId, platform::pump_events::EventLoopExtPumpEvents};

#[path = "native_pointer_host.rs"]
mod host;

struct ObservedZone {
    adapter: mmorpg_game_server::ZoneGameServerAdapter,
    commands: Vec<ZoneCommand>,
    frozen: Option<SimulationSnapshot>,
}
#[derive(Clone)]
struct SharedZone(Arc<Mutex<ObservedZone>>);
impl GameSimulation for SharedZone {
    fn tick_hz(&self) -> u16 {
        self.0.lock().unwrap().adapter.tick_hz()
    }
    fn max_players(&self) -> usize {
        self.0.lock().unwrap().adapter.max_players()
    }
    fn current_tick(&self) -> u64 {
        self.0.lock().unwrap().adapter.current_tick()
    }
    fn add_player(&mut self, id: u32) -> Result<(), SimulationError> {
        self.0.lock().unwrap().adapter.add_player(id)
    }
    fn remove_player(&mut self, id: u32) -> bool {
        self.0.lock().unwrap().adapter.remove_player(id)
    }
    fn apply_command(
        &mut self,
        id: u32,
        sequence: u32,
        payload: &[u8],
    ) -> Result<(), SimulationError> {
        let mut zone = self.0.lock().unwrap();
        zone.commands
            .push(mmorpg_protocol::decode_command(payload).unwrap());
        zone.adapter.apply_command(id, sequence, payload)
    }
    fn advance_tick(&mut self) -> Result<(), SimulationError> {
        self.0.lock().unwrap().adapter.advance_tick()
    }
    fn snapshot_scope(&self) -> SnapshotScope {
        SnapshotScope::PlayerScoped
    }
    fn snapshot(&self) -> Result<SimulationSnapshot, SimulationError> {
        self.0.lock().unwrap().adapter.snapshot()
    }
    fn snapshot_for(&self, id: u32) -> Result<SimulationSnapshot, SimulationError> {
        let zone = self.0.lock().unwrap();
        match &zone.frozen {
            Some(snapshot) => Ok(snapshot.clone()),
            None => zone.adapter.snapshot_for(id),
        }
    }
}

/// Only schedules events; the real handler owns all interpretation and output.
struct WindowDriver {
    app: App,
    pending: Vec<WindowEvent>,
    closed: bool,
    os_pointer_presses: usize,
    os_pointer_releases: usize,
    os_keys: usize,
}
impl ApplicationHandler for WindowDriver {
    fn resumed(&mut self, active: &ActiveEventLoop) {
        self.app.resumed(active);
    }
    fn window_event(&mut self, active: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        match &event {
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                ..
            } => self.os_pointer_presses += 1,
            WindowEvent::MouseInput {
                state: ElementState::Released,
                ..
            } => self.os_pointer_releases += 1,
            WindowEvent::KeyboardInput { .. } => self.os_keys += 1,
            _ => {}
        }
        self.app.window_event(active, id, event);
    }
    fn about_to_wait(&mut self, active: &ActiveEventLoop) {
        if let Some(window) = &self.app.window {
            let id = window.id();
            for event in self.pending.drain(..) {
                self.app.window_event(active, id, event);
            }
        }
        self.app.about_to_wait(active);
        self.closed |= active.exiting();
    }
}

struct NativeApplication {
    event_loop: EventLoop<()>,
    driver: WindowDriver,
    zone: SharedZone,
    content: Arc<ZoneContent>,
    _host: host::LocalHost,
    worker: tokio::task::JoinHandle<()>,
}
impl Drop for NativeApplication {
    fn drop(&mut self) {
        self.worker.abort();
    }
}
impl NativeApplication {
    fn new() -> Self {
        use winit::platform::x11::EventLoopBuilderExtX11;
        let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
        let original = mmorpg_core::greyhaven_vale::content();
        // Freeze wandering/aggro in the fixture, preserving actual names,
        // bodies, NPC identities, supported roles, quests and vendor catalog.
        let templates = original
            .creature_templates()
            .iter()
            .cloned()
            .map(|mut template| {
                template.behaviour = CreatureBehaviour::Neutral;
                template
            })
            .collect();
        let spawns = original
            .creature_spawns()
            .iter()
            .cloned()
            .map(|mut spawn| {
                spawn.wander_radius = 0;
                spawn
            })
            .collect();
        let content = Arc::new(
            ZoneContent::new(
                original.definition().clone(),
                original.areas().clone(),
                templates,
                spawns,
                original.npcs().to_vec(),
                original.graveyard(),
            )
            .unwrap()
            .with_loot_tables(original.loot_revision(), original.loot_tables().to_vec())
            .unwrap()
            .with_creature_abilities(
                original.ability_revision(),
                original.creature_abilities().to_vec(),
            )
            .unwrap()
            .with_vendors(original.vendor_revision(), original.vendors().to_vec())
            .unwrap()
            .with_quests(original.quest_revision(), original.quests().to_vec())
            .unwrap(),
        );
        let zone_id = mmorpg_core::ZoneId::new(1);
        let zone = SharedZone(Arc::new(Mutex::new(ObservedZone {
            adapter: mmorpg_game_server::ZoneGameServerAdapter::with_content(
                zone_id,
                Arc::clone(&content),
            )
            .unwrap(),
            commands: Vec::new(),
            frozen: None,
        })));
        let local = {
            let _entered = runtime.enter();
            host::LocalHost::start(zone.clone(), zone_id)
        };
        let session = runtime
            .block_on(ClientSession::connect(
                &local.url,
                Some(&local.certificate),
                zone_id,
                content.revision(),
            ))
            .unwrap();
        let player_id = session.player_id();
        let (input, receiver) = watch::channel(PlayerInput {
            class_choice: Some((PlayerClass::Warden.code(), mmorpg_core::Sex::Male.code())),
            ..PlayerInput::default()
        });
        let (sender, updates) = watch::channel(NetworkUpdate::Waiting);
        let (shutdown, stop) = tokio::sync::oneshot::channel();
        let worker = runtime.spawn(async move {
            let _shutdown = shutdown; // keep the real worker alive until fixture drop
            if let Err(error) = run_session(session, receiver, &sender, stop).await {
                sender.send_replace(NetworkUpdate::Failed(error.to_string()));
            }
        });
        let scenery = mmorpg_scenery::greyhaven_vale_scenery();
        let (characters, _selected_character) = watch::channel(None);
        let app = App {
            characters,
            runtime,
            window: None,
            renderer: None,
            world: WorldScene::new(&scenery),
            presentation: {
                let mut presentation =
                    Presentation::new(player_id, scenery, Arc::clone(&content), Instant::now())
                        .unwrap();
                presentation.set_default_appearance(PlayerClass::Warden);
                presentation
            },
            title: String::new(),
            connection_epoch: None,
            last_refusal: None,
            input,
            updates,
            camera: OrbitCamera::default(),
            orbit_buttons: HashSet::new(),
            cursor: None,
            guard_offset: None,
            pad_guard: None,
            error: None,
            frames: None,
            rendered: 0,
            next_frame: Instant::now(),
            abilities: class_kit(PlayerClass::Warden),
            controls: NativeControls::new(),
            hud: None,
            world_controls: WorldControls::new(),
            npc_context: NpcContext::default(),
            pointer_origin: None,
            ui: GameUi::new(&mmorpg_scenery::greyhaven_vale_scenery()),
            gilrs: None,
            pad: mmorpg_client::gamepad::PadControls::new(),
            pad_ui: Vec::new(),
            focused: true,
            last_pad_sample: Instant::now(),
            bindings_revision: 0,
            egui: None,
            choosing: false,
            chat_focused: false,
        };
        let event_loop = EventLoop::builder()
            .with_x11()
            .with_any_thread(true)
            .build()
            .unwrap();
        let mut application = Self {
            event_loop,
            driver: WindowDriver {
                app,
                pending: Vec::new(),
                closed: false,
                os_pointer_presses: 0,
                os_pointer_releases: 0,
                os_keys: 0,
            },
            zone,
            content,
            _host: local,
            worker,
        };
        application.until("connected window and projected class", |app| {
            app.driver.app.rendered > 2
                && app
                    .driver
                    .app
                    .presentation
                    .latest()
                    .is_some_and(|snapshot| snapshot.viewer.class.is_some())
        });
        // Observe the adapter contract for this actual window using the same
        // default compatible-surface request as WindowRenderer::new.
        let instance = wgpu::Instance::default();
        let surface = instance
            .create_surface(Arc::clone(application.driver.app.window.as_ref().unwrap()))
            .unwrap();
        let adapter = application
            .driver
            .app
            .runtime
            .block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            }))
            .unwrap();
        let info = adapter.get_info();
        eprintln!("native acceptance window-compatible GPU adapter: {info:?}");
        assert_ne!(
            info.device_type,
            wgpu::DeviceType::Cpu,
            "actual native acceptance requires a hardware GPU adapter"
        );
        drop(surface);
        application
            .driver
            .app
            .window
            .as_ref()
            .unwrap()
            .focus_window();
        application.pump(Duration::from_millis(200));
        eprintln!(
            "native acceptance connected real window/host: frames={}, title={:?}",
            application.driver.app.rendered, application.driver.app.title
        );
        application
    }
    fn pump(&mut self, duration: Duration) {
        let deadline = Instant::now() + duration;
        while Instant::now() < deadline {
            self.event_loop
                .pump_app_events(Some(Duration::from_millis(10)), &mut self.driver);
            assert!(
                !self.driver.closed,
                "native window closed during acceptance"
            );
            assert!(
                self.driver.app.error.is_none(),
                "native app error: {:?}",
                self.driver.app.error
            );
        }
    }
    fn until(&mut self, message: &str, predicate: impl Fn(&Self) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(12);
        while !predicate(self) && Instant::now() < deadline {
            self.pump(Duration::from_millis(20));
        }
        if !predicate(self) {
            let canonical = self.snapshot();
            let canonical_player = canonical.players.first().map(|player| player.position);
            let projected_player = self
                .driver
                .app
                .presentation
                .latest()
                .and_then(|projection| {
                    projection
                        .entities
                        .iter()
                        .find(|entity| {
                            entity.kind == mmorpg_core::EntityKind::Player
                                && entity.id == projection.viewer_id
                        })
                        .map(|entity| (projection.tick, entity.position))
                });
            let commands = self.zone.0.lock().unwrap().commands.clone();
            let tail = &commands[commands.len().saturating_sub(12)..];
            panic!(
                "{message}; title={:?}; canonical_player_at_tick{}={canonical_player:?}; projected_player={projected_player:?}; last_commands={tail:?}",
                self.driver.app.title, canonical.tick
            );
        }
        eprintln!(
            "native acceptance reached: {message}; frames={}; title={:?}",
            self.driver.app.rendered, self.driver.app.title
        );
    }
    fn dispatch(&mut self, events: impl IntoIterator<Item = WindowEvent>) {
        self.driver.pending.extend(events);
        self.pump(Duration::from_millis(80));
    }
    fn os_input(&mut self, arguments: &[String]) {
        assert!(
            Command::new("python3")
                .arg(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/tests/support/native_input.py"
                ))
                .args(arguments)
                .status()
                .unwrap()
                .success(),
            "XTest event injection failed"
        );
        self.pump(Duration::from_millis(80));
    }
    fn os_move_pointer(&mut self, point: PhysicalPosition<f64>) {
        let window = u64::from(self.driver.app.window.as_ref().unwrap().id());
        self.os_input(&[
            "move".into(),
            window.to_string(),
            point.x.round().to_string(),
            point.y.round().to_string(),
        ]);
    }
    fn os_pointer(&mut self, point: PhysicalPosition<f64>, end: Option<WindowEvent>) {
        let before = (
            self.driver.os_pointer_presses,
            self.driver.os_pointer_releases,
        );
        let window = u64::from(self.driver.app.window.as_ref().unwrap().id());
        self.os_input(&["focus".into(), window.to_string()]);
        self.os_move_pointer(point);
        self.os_input(&["press".into()]);
        if let Some(event) = end {
            match event {
                WindowEvent::CursorMoved { position, .. } => self.os_move_pointer(position),
                event => self.dispatch([event]),
            }
        }
        self.os_input(&["release".into()]);
        self.until(
            "OS pointer events arrive through ordinary winit handler",
            |app| {
                app.driver.os_pointer_presses > before.0
                    && app.driver.os_pointer_releases > before.1
            },
        );
    }
    fn pointer(&mut self, point: PhysicalPosition<f64>, end: Option<WindowEvent>) {
        if std::env::var("MMORPG_NATIVE_POINTER_OS").as_deref() == Ok("1") {
            self.os_pointer(point, end);
            return;
        }
        let device_id = DeviceId::dummy();
        self.dispatch([
            WindowEvent::CursorMoved {
                device_id,
                position: point,
            },
            WindowEvent::MouseInput {
                device_id,
                state: ElementState::Pressed,
                button: MouseButton::Left,
            },
        ]);
        if let Some(event) = end {
            self.dispatch([event]);
        }
        self.dispatch([WindowEvent::MouseInput {
            device_id,
            state: ElementState::Released,
            button: MouseButton::Left,
        }]);
        self.until(
            "pointer WindowEvents dispatched through actual App handler",
            |app| app.driver.pending.is_empty(),
        );
    }
    fn key(&mut self, keysym: &str) {
        let before = self.driver.os_keys;
        self.os_input(&["key".into(), keysym.into()]);
        self.pump(Duration::from_millis(200));
        self.until(
            "XTest keyboard event actually arrives through winit",
            |app| app.driver.os_keys > before,
        );
    }
    fn snapshot(&self) -> CanonicalZoneSnapshot {
        self.zone
            .0
            .lock()
            .unwrap()
            .adapter
            .zone()
            .snapshot()
            .unwrap()
    }
    fn restore(&mut self, snapshot: CanonicalZoneSnapshot) {
        *self.zone.0.lock().unwrap().adapter.zone_mut() =
            ZoneSimulation::from_snapshot(snapshot, Arc::clone(&self.content)).unwrap();
        self.pump(Duration::from_millis(400));
    }
    fn setup(&mut self, feet: [i32; 2], creatures: &[(u32, [i32; 2])]) {
        let mut snapshot = self.snapshot();
        let player = &mut snapshot.players[0];
        player.position =
            mmorpg_core::physics::from_units([feet[0], PLAYER_HALF_EXTENTS_UNITS[1], feet[1]]);
        player.velocity = [0.0; 3];
        player.forward = 0;
        player.strafe = 0;
        player.combat.target = None;
        player.combat.auto_attack = false;
        player.combat.intents.clear();
        player.copper = 20;
        player.quests = mmorpg_core::QuestLog::default();
        player.quests_changed_at = snapshot.tick;
        for &(id, position) in creatures {
            let creature = snapshot
                .creatures
                .iter_mut()
                .find(|creature| creature.creature_id.get() == id)
                .unwrap();
            creature.position[0] = position[0] as f32;
            creature.position[2] = position[1] as f32;
            creature.velocity = [0.0; 3];
            creature.ai = CreatureAi::Idle {
                timer: 60_000,
                destination: None,
            };
        }
        let mut zone = self.zone.0.lock().unwrap();
        *zone.adapter.zone_mut() =
            ZoneSimulation::from_snapshot(snapshot, Arc::clone(&self.content)).unwrap();
        zone.commands.clear();
        drop(zone);
        self.driver.app.camera = OrbitCamera::default();
        self.driver.app.presentation.reset(Instant::now());
        self.until(
            &format!(
                "canonical fixture reaches actual application projection at requested feet {feet:?}"
            ),
            |app| {
                app.driver
                    .app
                    .presentation
                    .latest()
                    .is_some_and(|projection| {
                        projection.viewer.copper == 20
                            && projection.entities.iter().any(|entity| {
                                entity.kind == mmorpg_core::EntityKind::Player
                                    && entity.id == projection.viewer_id
                                    && entity.position[0] == feet[0]
                                    && entity.position[2] == feet[1]
                            })
                            && creatures.iter().all(|(id, _)| {
                                projection.entities.iter().any(|entity| {
                                    entity.kind == mmorpg_core::EntityKind::Creature
                                        && entity.id == *id
                                })
                            })
                    })
            },
        );
        // Startup framing is exercised before setup(). This authored selection
        // fixture then applies an explicit user camera, after reset has framed
        // the first valid snapshot, preserving its original +Z composition.
        self.driver.app.camera = OrbitCamera::default();
        self.pump(Duration::from_millis(150));
    }
    fn point(&mut self, entity: EntityRef) -> PhysicalPosition<f64> {
        use three_d_camera::PerspectiveCamera;
        use three_d_core::Vec3;
        let canonical = self.snapshot();
        let app = &mut self.driver.app;
        let now = Instant::now();
        let pose = app
            .presentation
            .units(now)
            .get(&entity)
            .unwrap_or_else(|| {
                panic!(
                    "fixture entity {entity:?} absent; projection={:?}; canonical={:?}",
                    app.presentation.latest(),
                    canonical
                )
            })
            .pose
            .position;
        // Independent projection of the displayed torso, not production picking.
        let view = app
            .presentation
            .camera_view(&mut app.camera, &app.world, now);
        let size = app.window.as_ref().unwrap().inner_size();
        let camera = PerspectiveCamera::new(
            Vec3::new(view.eye[0], view.eye[1], view.eye[2]),
            Vec3::new(view.target[0], view.target[1], view.target[2]),
            Vec3::new(0.0, 1.0, 0.0),
            50_f32.to_radians(),
            size.width as f32 / size.height as f32,
            0.1,
            800.0,
        )
        .unwrap();
        let matrix = camera.view_projection_matrix().elements;
        let point = [pose[0], pose[1] + 0.25, pose[2], 1.0];
        let clip: [f32; 4] = std::array::from_fn(|row| {
            (0..4)
                .map(|column| matrix[column * 4 + row] * point[column])
                .sum()
        });
        assert!(clip[3] > 0.0);
        let x = (clip[0] / clip[3] + 1.0) * 0.5 * size.width as f32;
        let y = (1.0 - clip[1] / clip[3]) * 0.5 * size.height as f32;
        assert!(
            x > 0.0 && x < size.width as f32 && y > 0.0 && y < size.height as f32,
            "fixture target {entity:?} must be visible: world={pose:?}, view={view:?}, pixel=({x},{y}), window={size:?}"
        );
        PhysicalPosition::new(f64::from(x), f64::from(y))
    }
    fn select(&mut self, entity: EntityRef) {
        let point = self.point(entity);
        let rendered = self.driver.app.rendered;
        self.pointer(point, None);
        self.until("exact clicked identity must be confirmed by zone", |app| {
            app.driver
                .app
                .presentation
                .latest()
                .is_some_and(|snapshot| snapshot.viewer.target == Some(entity))
                && app.driver.app.rendered > rendered
        });
        assert!(
            !self
                .driver
                .app
                .presentation
                .latest()
                .unwrap()
                .viewer
                .auto_attacking,
            "a click selects without implicitly attacking"
        );
        let name = match entity {
            EntityRef::Npc(id) => &self.content.npc(id).unwrap().name,
            EntityRef::Creature(id) => {
                &self
                    .content
                    .creature_template(self.content.creature_spawn(id).unwrap().template)
                    .unwrap()
                    .name
            }
            EntityRef::Player(_) => unreachable!(),
        };
        assert!(
            self.driver.app.title.contains(name),
            "native visible target identity must name {name}"
        );
    }
    fn no_selection_command(&mut self, operation: impl FnOnce(&mut Self)) {
        let count = self.selection_count();
        operation(self);
        self.pump(Duration::from_millis(200));
        assert_eq!(
            self.selection_count(),
            count,
            "retired/dragged/occluded input must not select"
        );
    }
    fn selection_count(&self) -> usize {
        self.zone
            .0
            .lock()
            .unwrap()
            .commands
            .iter()
            .filter(|command| matches!(command, ZoneCommand::SelectTarget(_)))
            .count()
    }
    fn role_action_count(&self) -> usize {
        self.zone
            .0
            .lock()
            .unwrap()
            .commands
            .iter()
            .filter(|command| {
                matches!(
                    command,
                    ZoneCommand::AcceptQuest { .. }
                        | ZoneCommand::CompleteQuest { .. }
                        | ZoneCommand::BuyItem { .. }
                        | ZoneCommand::SellItem { .. }
                )
            })
            .count()
    }
    fn role(&mut self, npc: u32, distance: i32) {
        let position = self.content.npc(NpcId::new(npc)).unwrap().position;
        // The keep/inn and south palisade overlap these NPCs' south approaches.
        // Keep the exact 2.5 m / 8 m distances on a proven clear side instead.
        let north_approach = matches!(npc, 1 | 3 | 6);
        let offset = if north_approach { distance } else { -distance };
        self.setup([position[0], position[1] + offset], &[]);
        if north_approach {
            let rendered = self.driver.app.rendered;
            self.driver
                .app
                .camera
                .orbit(f64::from(std::f32::consts::PI / 0.006), 0.0);
            self.until("approach camera renders toward the actual NPC", |app| {
                app.driver.app.rendered > rendered
            });
        }
        self.select(EntityRef::Npc(NpcId::new(npc)));
        assert_eq!(
            self.role_action_count(),
            0,
            "selection must never autoaccept or autobuy"
        );
        self.key("Return");
    }
}

#[test]
#[ignore = "requires X11, libXtst and an actual wgpu adapter; run explicitly for #28"]
fn native_pointer_application_contract() {
    let mut app = NativeApplication::new();
    // Real redraw/session initialization must adopt the same unobstructed view
    // as the gallery, rather than leaving native yaw zero inside the keep.
    assert_ne!(
        app.driver.app.camera.facing(),
        0,
        "actual App did not frame the first session view"
    );
    app.driver.app.camera.orbit(100.0, 0.0);
    let user_heading = app.driver.app.camera.facing();
    app.pump(Duration::from_millis(100));
    assert_eq!(
        app.driver.app.camera.facing(),
        user_heading,
        "App reset user orbit on later frames"
    );
    let front = EntityRef::Creature(CreatureId::new(220));
    let second = EntityRef::Creature(CreatureId::new(221));
    app.setup([0, 1_500], &[(220, [-180, 2_100]), (221, [180, 2_300])]);
    app.select(second); // deliberately choose farther body first
    app.select(front);
    let point = app.point(front);
    let camera = app.driver.app.camera;
    app.no_selection_command(|app| {
        app.pointer(
            point,
            Some(WindowEvent::CursorMoved {
                device_id: DeviceId::dummy(),
                position: PhysicalPosition::new(point.x + 90.0, point.y + 20.0),
            }),
        )
    });
    assert_ne!(
        app.driver.app.camera, camera,
        "drag must orbit the actual rendered camera"
    );
    app.driver.app.camera = camera;
    for end in [
        WindowEvent::Focused(false),
        WindowEvent::CursorLeft {
            device_id: DeviceId::dummy(),
        },
    ] {
        app.no_selection_command(|app| app.pointer(point, Some(end)));
        app.dispatch([WindowEvent::Focused(true)]);
    }
    app.pointer(PhysicalPosition::new(5.0, 5.0), None);
    assert!(
        app.driver
            .app
            .presentation
            .latest()
            .unwrap()
            .viewer
            .target
            .is_none_or(|target| target == front),
        "empty space must not invent a unit or nearest substitute"
    );
    app.driver.app.window.as_ref().unwrap().focus_window();
    app.key("Tab");
    assert!(
        app.driver
            .app
            .presentation
            .latest()
            .unwrap()
            .viewer
            .target
            .is_some(),
        "Tab remains a physical keyboard alternative"
    );

    // Collinear bodies: the front humanoid occludes the rear torso sample.
    app.setup([0, 1_500], &[(220, [0, 2_000]), (221, [0, 2_180])]);
    let behind = app.point(second);
    app.pointer(behind, None);
    app.until(
        "front rendered body must win overlapping silhouettes",
        |app| app.driver.app.presentation.latest().unwrap().viewer.target == Some(front),
    );

    // Draw the occluder through the actual renderer; the same hidden body
    // must remain unselectable for a wall, roof slab and a terrain triangle.
    for kind in 0..3 {
        let roof = kind == 1;
        app.setup([0, 1_500], &[(220, [0, 2_100])]);
        let original = app.driver.app.world.clone();
        let point = app.point(front);
        use mmorpg_client::world::{DrawMaterial, SceneDraw};
        let (mesh, size, at) = if kind == 2 {
            (
                three_d_core::Mesh::new(
                    vec![
                        three_d_core::Vec3::new(-10.0, -5.0, 18.0),
                        three_d_core::Vec3::new(10.0, -5.0, 18.0),
                        three_d_core::Vec3::new(0.0, 15.0, 18.0),
                    ],
                    vec![0, 2, 1],
                )
                .unwrap(),
                [1.0; 3],
                [0.0; 3],
            )
        } else {
            (
                three_d_core::Mesh::unit_cube(),
                if roof {
                    [10.0, 0.5, 12.0]
                } else {
                    [10.0, 8.0, 0.5]
                },
                if roof {
                    [0.0, 3.0, 20.0]
                } else {
                    [0.0, 2.0, 18.0]
                },
            )
        };
        app.driver.app.world.additional_draws.push(SceneDraw {
            id: format!("acceptance-occluder-{kind}"),
            resource_key: format!("acceptance-occluder-{kind}"),
            mesh: Arc::new(mesh),
            model: three_d_animation::Mat4 {
                elements: [
                    size[0], 0.0, 0.0, 0.0, 0.0, size[1], 0.0, 0.0, 0.0, 0.0, size[2], 0.0, at[0],
                    at[1], at[2], 1.0,
                ],
            },
            material: DrawMaterial {
                color: [0.3; 3],
                emissive: [0.0; 3],
                opacity: 1.0,
                unlit: false,
                double_sided: false,
            },
        });
        let window = Arc::clone(app.driver.app.window.as_ref().unwrap());
        // Retire the configured surface before creating another for this window.
        drop(app.driver.app.renderer.take());
        app.driver.app.renderer = Some(
            app.driver
                .app
                .runtime
                .block_on(WindowRenderer::new(window, &app.driver.app.world))
                .unwrap(),
        );
        app.pump(Duration::from_millis(100));
        app.no_selection_command(|app| app.pointer(point, None));
        app.driver.app.world = original;
        let window = Arc::clone(app.driver.app.window.as_ref().unwrap());
        // Retire the configured surface before creating another for this window.
        drop(app.driver.app.renderer.take());
        app.driver.app.renderer = Some(
            app.driver
                .app
                .runtime
                .block_on(WindowRenderer::new(window, &app.driver.app.world))
                .unwrap(),
        );
    }

    // Actual Greyhaven content identities and role availability, never nearest.
    app.role(1, 250);
    assert!(
        app.driver.app.title.contains("Trouble in the Woods"),
        "Marshal must expose his supported quest context"
    );
    app.key("1");
    app.until("native quest choice must reach the authority", |app| {
        app.snapshot().players[0]
            .quests
            .entries
            .iter()
            .any(|entry| entry.quest.get() == 1)
    });
    // Fixture only supplies earned objective progress; the native action
    // must still request completion and the real core settles the reward.
    let mut ready = app.snapshot();
    ready.players[0].quests.entries[0].progress[0] = 6;
    ready.players[0].quests_changed_at = ready.tick;
    app.restore(ready);
    let before = app.snapshot().players[0].clone();
    app.key("1");
    app.until(
        "Marshal native completion credits his actual reward",
        |app| {
            let after = &app.snapshot().players[0];
            after.quests.is_completed(mmorpg_core::QuestId::new(1))
                && after.copper == before.copper + 10
        },
    );
    app.role(2, 250);
    let title = app.driver.app.title.to_lowercase();
    assert!(
        title.contains("tanner")
            && (title.contains("pelts") || title.contains("available") || title.contains("locked")),
        "Tanner must have her own honest prerequisite/quest state"
    );
    // Tanner's reward-choice context: her collect prerequisites are setup,
    // never an application-authored acceptance/completion result.
    let mut ready = app.snapshot();
    let player = &mut ready.players[0];
    player.quests.completed = 1;
    player.quests.entries = vec![mmorpg_core::QuestEntry {
        quest: mmorpg_core::QuestId::new(2),
        progress: [0; 3],
    }];
    player
        .inventory
        .insert(mmorpg_core::ItemId::new(10), 5)
        .unwrap();
    player.inventory_revision += 1;
    player.inventory_changed_at = ready.tick;
    player.quests_changed_at = ready.tick;
    app.restore(ready);
    app.until(
        "Tanner restored self sheets reach actual application context",
        |app| {
            let context = &app.driver.app.npc_context;
            context
                .inventory
                .as_ref()
                .is_some_and(|inventory| inventory.count(mmorpg_core::ItemId::new(10)) == 5)
                && context.quests.as_ref().is_some_and(|quests| {
                    quests.completed == 1
                        && quests.entries.iter().any(|entry| {
                            entry.quest == mmorpg_core::QuestId::new(2) && entry.progress[0] == 5
                        })
                })
        },
    );
    eprintln!(
        "TANNER_BOUNDARY title={:?} expected={:?} canonicalQuests={:?} receivedQuests={:?} contextQuests={:?} inventoryRevision={:?}",
        app.driver.app.title,
        [9, 6].map(
            |id| mmorpg_core::item_template(mmorpg_core::ItemId::new(id))
                .unwrap()
                .name
        ),
        app.snapshot().players[0].quests,
        app.driver.app.presentation.latest().unwrap().quests,
        app.driver.app.npc_context.quests,
        app.driver
            .app
            .presentation
            .latest()
            .unwrap()
            .inventory_revision
    );
    for item in [9, 6] {
        assert!(
            app.driver.app.title.contains(
                mmorpg_core::item_template(mmorpg_core::ItemId::new(item))
                    .unwrap()
                    .name
            ),
            "native completion must expose actual reward choices"
        );
    }
    let before = app.snapshot().players[0].clone();
    app.key("Right"); // choose the second advertised reward
    app.key("1");
    app.until(
        "Tanner completion settles the chosen reward exactly once",
        |app| {
            let after = &app.snapshot().players[0];
            after.quests.is_completed(mmorpg_core::QuestId::new(2))
                && after.copper == before.copper + 15
                && after.inventory.count(mmorpg_core::ItemId::new(6))
                    == before.inventory.count(mmorpg_core::ItemId::new(6)) + 1
                && after.inventory.count(mmorpg_core::ItemId::new(9))
                    == before.inventory.count(mmorpg_core::ItemId::new(9))
                && after.inventory.count(mmorpg_core::ItemId::new(10))
                    == before.inventory.count(mmorpg_core::ItemId::new(10)) - 5
        },
    );
    app.role(6, 250);
    let title = app.driver.app.title.to_lowercase();
    assert!(
        title.contains("guard")
            && (title.contains("unsupported") || title.contains("no interaction")),
        "guard must not inherit a nearby quest/vendor role or invented dialogue"
    );
    let count = app.role_action_count();
    app.key("1");
    assert_eq!(app.role_action_count(), count);
    app.role(3, 250);
    assert!(
        app.driver.app.title.to_lowercase().contains("buy"),
        "Bram must expose real vendor actions"
    );
    let before = app.snapshot().players[0].clone();
    app.key("1");
    app.until(
        "native vendor purchase must settle authoritative item and copper",
        |app| {
            let after = &app.snapshot().players[0];
            after.inventory_revision == before.inventory_revision + 1
                && after.copper == before.copper - 5
                && after.inventory.count(mmorpg_core::ItemId::new(2))
                    == before.inventory.count(mmorpg_core::ItemId::new(2)) + 1
        },
    );

    let before = app.snapshot().players[0].clone();
    let sold = before.inventory.slots()[0].unwrap().item();
    app.key("2"); // one unit from the advertised first bag slot
    app.until(
        "Bram native sale settles inventory and copper together",
        |app| {
            let after = &app.snapshot().players[0];
            after.inventory_revision == before.inventory_revision + 1
                && after.copper == before.copper + mmorpg_core::sell_price(sold)
                && after.inventory.count(sold) + 1 == before.inventory.count(sold)
        },
    );

    // Both role paths refuse a visible selected NPC beyond authoritative reach.
    for npc in [1, 3] {
        app.role(npc, 800);
        let before = app.snapshot().players[0].clone();
        app.key("1");
        eprintln!(
            "RANGE_BOUNDARY npc={npc} title={:?} feedback={:?} canonicalTarget={:?} latestAck={:?} events={:?}",
            app.driver.app.title,
            app.driver.app.npc_context.feedback,
            app.snapshot().players[0].combat.target,
            app.driver
                .app
                .presentation
                .latest()
                .map(|p| p.acknowledged_sequence),
            app.driver.app.presentation.latest().map(|p| &p.events)
        );
        app.until(
            "range refusal reaches visible selected NPC context",
            |app| {
                let title = app.driver.app.title.to_lowercase();
                title.contains("range") || title.contains("reach") || title.contains("far")
            },
        );
        let after = app.snapshot().players[0].clone();
        assert_eq!(
            (
                after.inventory,
                after.copper,
                after.inventory_revision,
                after.quests
            ),
            (
                before.inventory,
                before.copper,
                before.inventory_revision,
                before.quests
            )
        );
        let title = app.driver.app.title.to_lowercase();
        assert!(
            title.contains("range") || title.contains("reach") || title.contains("far"),
            "native UI must explain range refusal for the selected identity"
        );
    }

    // Close only the context on Escape; an armed pointer is retired too.
    app.role(3, 250);
    let point = app.point(EntityRef::Npc(NpcId::new(3)));
    let device_id = DeviceId::dummy();
    app.dispatch([
        WindowEvent::CursorMoved {
            device_id,
            position: point,
        },
        WindowEvent::MouseInput {
            device_id,
            state: ElementState::Pressed,
            button: MouseButton::Left,
        },
    ]);
    let selections = app.selection_count();
    app.key("Escape");
    app.dispatch([WindowEvent::MouseInput {
        device_id,
        state: ElementState::Released,
        button: MouseButton::Left,
    }]);
    assert_eq!(
        app.selection_count(),
        selections,
        "cancel must retire an armed pointer"
    );

    app.key("Return");
    let before_purchase = app.snapshot().players[0].inventory_revision;
    app.key("1");
    app.until(
        "pre-outage purchase must settle once before reconnect",
        |app| app.snapshot().players[0].inventory_revision == before_purchase + 1,
    );
    let before_resume = app.snapshot().players[0].clone();

    // Force the real receive loop through its stall/resume path. A duplicate
    // projection is never synthesized into an advancing application snapshot.
    let identity = app.snapshot().players[0].player_id;
    let epoch = app.driver.app.connection_epoch.unwrap();
    let previous = app
        .zone
        .0
        .lock()
        .unwrap()
        .adapter
        .snapshot_for(identity)
        .unwrap();
    app.zone.0.lock().unwrap().frozen = Some(previous);
    app.until("real stale transport must enter reconnect", |app| {
        matches!(
            *app.driver.app.updates.borrow(),
            NetworkUpdate::Reconnecting
        )
    });
    let role_count = app.role_action_count();
    let selections = app.selection_count();
    app.pointer(point, None);
    app.key("Return");
    app.key("1");
    app.zone.0.lock().unwrap().frozen = None;
    app.until(
        "same player must resume on a later connection epoch",
        |app| {
            app.driver
                .app
                .connection_epoch
                .is_some_and(|next| next > epoch)
        },
    );
    app.pump(Duration::from_millis(700));
    assert_eq!(app.snapshot().players[0].player_id, identity);
    assert_eq!(
        app.role_action_count(),
        role_count,
        "outage role presses must not replay"
    );
    assert_eq!(
        app.selection_count(),
        selections,
        "outage selection must not replay"
    );
    let after_resume = app.snapshot().players[0].clone();
    assert_eq!(
        (
            after_resume.inventory,
            after_resume.copper,
            after_resume.inventory_revision,
            after_resume.quests
        ),
        (
            before_resume.inventory,
            before_resume.copper,
            before_resume.inventory_revision,
            before_resume.quests
        ),
        "resume must not settle any prior or outage role action twice"
    );
    assert!(
        app.driver.app.rendered > 100,
        "journey must render in the real GPU application"
    );
}
