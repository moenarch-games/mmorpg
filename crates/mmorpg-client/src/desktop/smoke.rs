//! Bounded opt-in receipts for the existing native smoke path, from the actual window's
//! projections and imported presentation. Ordinary play never captures or changes camera.
use mmorpg_client::{
    camera::CameraView, presentation::Presentation, shared_assets::Receipt, world::WorldFrame,
};
use mmorpg_core::EntityKind;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Instant,
};

#[derive(Default)]
pub(super) struct Smoke {
    ready: bool,
    frontend: bool,
    resident: bool,
    positions: Option<BTreeMap<u32, [i32; 3]>>,
    settled: u8,
    capture: Option<(Receipt, PathBuf)>,
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
    pub fn capture(&mut self, receipt: Receipt) {
        let Some(path) = std::env::var_os("MMORPG_SMOKE_FRAME").filter(|path| !path.is_empty())
        else {
            return;
        };
        if !self.captured {
            self.capture = Some((receipt, path.into()));
        }
    }
    pub fn capture_view(&self, presentation: &Presentation, now: Instant) -> Option<CameraView> {
        self.capture.as_ref()?;
        let target = presentation.shared_detail_target(now)?;
        Some(CameraView {
            eye: [target[0] + 1.8, target[1] + 1.1, target[2] + 2.8],
            target,
            field_of_view_degrees: 50.,
        })
    }
    pub fn capture_path(&self) -> Option<&Path> {
        self.capture.as_ref().map(|(_, path)| path.as_path())
    }
    pub fn rendered(&mut self, frame: &WorldFrame, revision: u64, colors: usize) {
        let Some((receipt, _)) = self.capture.take() else {
            return;
        };
        self.captured = true;
        let cat_draws = frame
            .draws
            .iter()
            .filter(|draw| draw.id.starts_with(mmorpg_game_server::shared_assets::CAT))
            .count();
        println!(
            "{}",
            serde_json::json!({"event":"native_detail_rendered",
            "bytes_received":receipt.bytes_received,"cache_hits":receipt.cache_hits,"files_verified":receipt.files_verified,
            "manifest":receipt.manifest_identity,"snapshots_while_loading":receipt.snapshots_while_loading,
            "moved_while_loading":receipt.moved_while_loading,"content_revision":revision,
            "cat_draws":cat_draws,"rendered_colors":colors,"actual_window_surface":true})
        );
    }
}
