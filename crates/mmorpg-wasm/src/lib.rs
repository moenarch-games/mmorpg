//! WebAssembly adapter for the GitHub Pages demo ([ADR 0002]).
//!
//! The browser runs the shared `mmorpg-core` zone simulation as a **local
//! single-player zone host**, like a listen server: [`LocalZone`] builds the
//! same `ZoneSimulation` and content revision as `mmorpg-zone-host`, accepts
//! `mmorpg-protocol` command bytes with increasing sequences, advances fixed
//! 30 Hz ticks, and returns only encoded player-scoped projections, which the
//! browser decodes with the same TypeScript decoder a network source uses.
//! There is no fencing, lease, handoff, persistence or shared world here.
//!
//! This crate is a thin adapter: gameplay rules live in `mmorpg-core`, wire
//! formats in `mmorpg-protocol`. The static scenery export lives in
//! [`scenery`], the content catalog (creature, NPC and area names by ID) in
//! [`catalog`]. Everything testable is plain Rust in [`host`], [`scenery`]
//! and [`catalog`]; the `#[wasm_bindgen]` items below only translate types.
//!
//! The workspace `unsafe_code = "forbid"` lint applies unchanged: the code
//! `wasm-bindgen` 0.2.129 generates compiles under it, so this crate needs no
//! lint exception.
//!
//! [ADR 0002]: https://github.com/moritzbrantner/mmorpg/blob/main/docs/adr/0002-browser-embeds-zone-simulation.md
#![forbid(unsafe_code)]

pub mod catalog;
pub mod host;
pub mod scenery;

use wasm_bindgen::prelude::{JsError, wasm_bindgen};

use crate::host::{LocalZoneHost, SubmitOutcome};

/// The local zone host as seen from JavaScript. Errors surface as exceptions
/// only for malformed input or misuse (unknown player, sequence 0); a stale
/// sequence is a normal `false` result.
#[wasm_bindgen]
pub struct LocalZone {
    host: LocalZoneHost,
}

#[wasm_bindgen]
impl LocalZone {
    /// A fresh zone at tick 0, hosting zone 1 with the hosted content revision.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<LocalZone, JsError> {
        Ok(Self {
            host: LocalZoneHost::new().map_err(js_error)?,
        })
    }

    /// Spawns a new player with its class (0 Warden, 1 Ranger, 2 Arcanist)
    /// and sex (0 female, 1 male) and returns its ID. The class choice is the
    /// player's command sequence 1. IDs are never reused.
    pub fn join(&mut self, class: u8, sex: u8) -> Result<u32, JsError> {
        self.host.join_as(class, sex).map_err(js_error)
    }

    /// Removes the player's unit. Returns whether it was present.
    pub fn leave(&mut self, player: u32) -> bool {
        self.host.leave(player)
    }

    /// Applies one command-wire-v4 payload. Returns `true` when applied and
    /// `false` when the sequence is stale; throws for malformed bytes.
    pub fn submit(&mut self, player: u32, sequence: u32, command: &[u8]) -> Result<bool, JsError> {
        self.host
            .submit(player, sequence, command)
            .map(|outcome| outcome == SubmitOutcome::Applied)
            .map_err(js_error)
    }

    /// Advances one 30 Hz tick and returns the new tick.
    pub fn tick(&mut self) -> Result<u64, JsError> {
        self.host.tick().map_err(js_error)
    }

    /// The encoded player-visible snapshot addressed to `player`.
    pub fn projection(&self, player: u32) -> Result<Vec<u8>, JsError> {
        self.host.projection(player).map_err(js_error)
    }

    #[wasm_bindgen(js_name = contentRevision)]
    pub fn content_revision(&self) -> u64 {
        self.host.content_revision()
    }

    #[wasm_bindgen(js_name = zoneId)]
    pub fn zone_id(&self) -> u32 {
        self.host.zone_id().get()
    }

    #[wasm_bindgen(js_name = currentTick)]
    pub fn current_tick(&self) -> u64 {
        self.host.current_tick()
    }
}

/// The versioned JSON scenery export of the hosted content revision.
#[wasm_bindgen]
pub fn scenery() -> String {
    scenery::hosted_scenery_json().to_owned()
}

/// The versioned JSON content catalog of the hosted content revision:
/// creature templates, NPCs and areas by ID.
#[wasm_bindgen(js_name = catalog)]
pub fn content_catalog() -> String {
    catalog::hosted_catalog_json().to_owned()
}

/// The ID of the named core area containing `(x, z)` in units, if any.
#[wasm_bindgen(js_name = areaAt)]
pub fn area_at(x: i32, z: i32) -> Option<u16> {
    scenery::hosted_areas()
        .area_at(x, z)
        .map(|area| area.id().get())
}

/// Presentation relief at `(x, z)` in units; clients raise units by it.
#[wasm_bindgen(js_name = reliefAt)]
pub fn relief_at(x: i32, z: i32) -> i32 {
    scenery::relief_at(x, z)
}

fn js_error(error: host::LocalZoneError) -> JsError {
    JsError::new(&error.to_string())
}
