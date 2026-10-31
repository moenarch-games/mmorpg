//! The client rules both clients share (#543): what the action bar, cast bar,
//! chat and quest windows show for a received projection, how a typed chat
//! line becomes a command, and which movement commands the current input
//! needs.
//!
//! Everything here is a plain function or struct over `mmorpg-core` types and
//! local input, with no I/O and no rendering. The native client calls it
//! directly; the browser calls it through thin `mmorpg-wasm` exports and only
//! renders the result. Nothing here decides an outcome: the zone validates
//! every command.
#![forbid(unsafe_code)]

pub mod chat;
pub mod hud;
pub mod outbox;
pub mod quest;

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
            raid: None,
        }
    }
}
