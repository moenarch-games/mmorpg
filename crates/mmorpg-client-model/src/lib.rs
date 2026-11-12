//! The client rules both clients share (#543): what the action bar, cast bar,
//! chat and quest windows show for a received projection, how a typed chat
//! line becomes a command, and which movement commands the current input
//! needs, when a held power shot fires, who owns the mouse, and the front end
//! (selection modes, game menu, class cards), theme and logo both clients draw.
//!
//! Everything here is a plain function or struct over `mmorpg-core` types and
//! local input, with no I/O and no rendering. The native client calls it
//! directly; the browser calls it through thin `mmorpg-wasm` exports and only
//! renders the result. Nothing here decides an outcome: the zone validates
//! every command.
#![forbid(unsafe_code)]

pub mod chat;
pub mod front_end;
pub mod hud;
pub mod logo;
pub mod manual_aim;
pub mod mouse_owner;
pub mod outbox;
pub mod party;
pub mod pie_menu;
pub mod power_draw;
pub mod quest;
pub mod theme;
pub mod world_map;

#[cfg(test)]
pub(crate) mod test_support {
    use mmorpg_core::{TargetDetail, ViewerState, ZoneId, ZoneSnapshot};

    /// An empty projection for `viewer` at `tick`.
    pub fn snapshot(viewer: u32, tick: u64) -> ZoneSnapshot {
        ZoneSnapshot {
            content_revision: 3,
            acknowledged_sequence: 0,
            viewer_id: viewer,
            zone_id: ZoneId::new(1),
            tick,
            viewer: ViewerState::default(),
            cooldowns: Vec::new(),
            auras: Vec::new(),
            target_of_target: None,
            target_detail: TargetDetail::default(),
            inventory_revision: 0,
            inventory: None,
            equipment: None,
            quests: None,
            loot: None,
            events: Vec::new(),
            chat: Vec::new(),
            entities: Vec::new(),
            doors: Vec::new(),
            projectiles: Vec::new(),
            debris: Vec::new(),
            pieces: Vec::new(),
            summoned: Vec::new(),
            traps: Vec::new(),
            raid: None,
            party: None,
            depths_loot: None,
        }
    }
}
