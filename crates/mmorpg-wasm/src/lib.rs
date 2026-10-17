//! WebAssembly adapter for the GitHub Pages demo ([ADR 0002]).
//!
//! The browser runs the shared `mmorpg-core` zone simulation as a **local
//! single-player zone host**, like a listen server: [`LocalZone`] builds the
//! same `ZoneSimulation` and content revision as `mmorpg-zone-host`, accepts
//! `mmorpg-protocol` command bytes with increasing sequences, advances fixed
//! 30 Hz ticks, and returns only encoded player-scoped projections, which the
//! browser decodes with the same TypeScript decoder a network source uses.
//! There is no handoff or shared world here. Characters persist through the
//! same `mmorpg-characters` record and lease contract as zone hosts, over
//! the page's [`CharacterEntries`] (the browser keeps them in `localStorage`).
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

use mmorpg_characters::{CharacterStore, EntryBackend, EntryChange, LeaseHolder, StoreError};
use mmorpg_core::{CharacterId, ClassChoice, PlayerClass, Sex};
use wasm_bindgen::prelude::{JsError, JsValue, wasm_bindgen};

use crate::host::{LOCAL_ZONE_ID, LocalZoneHost, SubmitOutcome};

#[wasm_bindgen]
extern "C" {
    /// The page's character entry storage:
    /// `{ read(key: string): Uint8Array | undefined; write(key: string, entry: Uint8Array): void }`.
    /// Calls are synchronous, so updates within the page never interleave;
    /// the page keeps other pages away from a character it plays.
    pub type CharacterEntries;

    #[wasm_bindgen(method, catch)]
    fn read(this: &CharacterEntries, key: &str) -> Result<Option<Vec<u8>>, JsValue>;

    #[wasm_bindgen(method, catch)]
    fn write(this: &CharacterEntries, key: &str, entry: &[u8]) -> Result<(), JsValue>;
}

struct PageEntries<'a>(&'a CharacterEntries);

impl EntryBackend for PageEntries<'_> {
    fn read(&self, id: &CharacterId) -> Result<Option<Vec<u8>>, StoreError> {
        self.0.read(id.as_str()).map_err(storage_error)
    }

    fn update(&mut self, id: &CharacterId, change: &mut EntryChange<'_>) -> Result<(), StoreError> {
        let current = EntryBackend::read(self, id)?;
        let next = change(current.as_deref())?;
        self.0.write(id.as_str(), &next).map_err(storage_error)
    }
}

fn storage_error(error: JsValue) -> StoreError {
    StoreError::Backend(
        error
            .as_string()
            .unwrap_or_else(|| "the page's character storage failed".to_owned()),
    )
}

fn store(entries: &CharacterEntries) -> CharacterStore<PageEntries<'_>> {
    CharacterStore::new(PageEntries(entries))
}

fn character_id(value: &str) -> Result<CharacterId, JsError> {
    CharacterId::new(value).map_err(|error| JsError::new(&error.to_string()))
}

/// The committed level of a stored character, or `undefined` if it was
/// never stored. Reads only the committed record, like a global profile query.
#[wasm_bindgen(js_name = committedLevel)]
pub fn committed_level(entries: &CharacterEntries, character: &str) -> Result<Option<u8>, JsError> {
    let id = character_id(character)?;
    store(entries)
        .committed(&id)
        .map(|committed| committed.map(|committed| committed.record.level))
        .map_err(|error| JsError::new(&error.to_string()))
}

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

    /// Enters the stored character `character` and returns its player ID.
    /// The page plays zone 1 at `zone_epoch`, which must exceed the epoch of
    /// any earlier page. A never-stored character is created with its class
    /// (0 Warden, 1 Ranger, 2 Arcanist) and sex (0 female, 1 male); a stored
    /// one keeps its own. Commands start at sequence 1. IDs are never reused.
    pub fn enter(
        &mut self,
        entries: &CharacterEntries,
        character: &str,
        class: u8,
        sex: u8,
        #[wasm_bindgen(js_name = zoneEpoch)] zone_epoch: u64,
    ) -> Result<u32, JsError> {
        let id = character_id(character)?;
        let class = PlayerClass::from_code(class)
            .zip(Sex::from_code(sex))
            .map(|(class, sex)| ClassChoice { class, sex })
            .ok_or_else(|| JsError::new("the class choice is invalid"))?;
        let holder = LeaseHolder {
            zone_id: LOCAL_ZONE_ID,
            zone_epoch,
        };
        self.host
            .enter(&mut store(entries), id, class, holder)
            .map_err(js_error)
    }

    /// Commits the player's character; returns the committed revision.
    pub fn save(&mut self, entries: &CharacterEntries, player: u32) -> Result<u64, JsError> {
        self.host
            .save(&mut store(entries), player)
            .map_err(js_error)
    }

    /// Commits and releases the player's character and removes its unit.
    pub fn exit(&mut self, entries: &CharacterEntries, player: u32) -> Result<u64, JsError> {
        self.host
            .exit(&mut store(entries), player)
            .map_err(js_error)
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
