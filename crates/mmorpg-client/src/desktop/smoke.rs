//! Bounded opt-in receipts for the existing native smoke path, from the actual window's
//! projections and imported presentation. Ordinary play never captures or changes camera.
use mmorpg_client::{
    camera::CameraView, presentation::Presentation, shared_assets::Receipt, world::WorldScene,
};
use mmorpg_core::EntityKind;
use std::{collections::BTreeMap, time::Instant};

#[derive(Default)]
pub(super) struct Smoke {
    ready: bool,
    frontend: bool,
    resident: bool,
    positions: Option<BTreeMap<u32, [i32; 3]>>,
    settled: u8,
    captured: bool,
    failed: bool,
}
impl Smoke {
    pub fn enabled() -> Option<Self> {
        std::env::var_os("MMORPG_SMOKE_FRAME").map(|_| Self::default())
    }
    pub fn frontend(&mut self) {
        if !self.frontend {
            self.frontend = true;
            println!("{}", serde_json::json!({"event":"native_frontend_frame"}));
        }
    }
    pub fn resident(&mut self) {
        if !self.resident {
            self.resident = true;
            println!("{}", serde_json::json!({"event":"native_world_resident"}));
        }
    }
    pub fn observe(&mut self, presentation: &Presentation) {
        let Some(snapshot) = presentation.latest() else {
            return;
        };
        let players: Vec<_> = snapshot
            .entities
            .iter()
            .filter(|entity| entity.kind == EntityKind::Player)
            .collect();
        let positions = players
            .iter()
            .map(|entity| (entity.id, entity.position))
            .collect::<BTreeMap<_, _>>();
        let stable = players
            .iter()
            .all(|entity| entity.velocity[0] == 0 && entity.velocity[2] == 0);
        if self.ready
            && (!stable || self.positions.as_ref() == Some(&positions) || self.settled >= 8)
        {
            return;
        }
        self.positions = Some(positions);
        let event = if self.ready {
            self.settled += 1;
            "native_movement_settled"
        } else {
            self.ready = true;
            "native_world_ready"
        };
        println!(
            "{}",
            serde_json::json!({"event":event, "viewer":snapshot.viewer_id, "tick":snapshot.tick,
            "players":players.iter().map(|entity| serde_json::json!({"id":entity.id,"position":entity.position})).collect::<Vec<_>>() })
        );
    }
    pub fn failed(&mut self, event: &str, error: &str) {
        if self.failed {
            return;
        }
        self.failed = true;
        println!("{}", serde_json::json!({"event":event, "error":error}));
    }
    pub fn capture(
        &mut self,
        presentation: &mut Presentation,
        world: &WorldScene,
        receipt: Receipt,
        runtime: tokio::runtime::Handle,
    ) {
        // An empty opt-in path requests bounded window receipts without another image.
        if std::env::var_os("MMORPG_SMOKE_FRAME").is_none_or(|path| path.is_empty()) {
            return;
        }
        if self.captured {
            return;
        }
        self.captured = true;
        let now = Instant::now();
        let Some(target) = presentation.shared_detail_target(now) else {
            return;
        };
        let view = CameraView {
            eye: [target[0] + 1.8, target[1] + 1.1, target[2] + 2.8],
            target,
            field_of_view_degrees: 50.,
        };
        let frame = presentation.frame(world, now, view);
        let cat_draws = frame
            .draws
            .iter()
            .filter(|draw| draw.id.starts_with(mmorpg_game_server::shared_assets::CAT))
            .count();
        let revision = presentation.content().revision();
        runtime.clone().spawn_blocking(move || {
            match runtime.block_on(mmorpg_client::graphics::render_offscreen_frame(
                &frame,
                &[],
                view,
            )) {
                Ok(colors) if cat_draws > 0 => println!(
                    "{}",
                    serde_json::json!({"event":"native_detail_rendered",
                    "bytes_received":receipt.bytes_received,"cache_hits":receipt.cache_hits,"files_verified":receipt.files_verified,
                    "manifest":receipt.manifest_identity,"snapshots_while_loading":receipt.snapshots_while_loading,"moved_while_loading":receipt.moved_while_loading,"content_revision":revision,"cat_draws":cat_draws,"rendered_colors":colors})
                ),
                result => {
                    eprintln!("Native detail frame failed: {result:?}; cat_draws={cat_draws}")
                }
            }
        });
    }
}
