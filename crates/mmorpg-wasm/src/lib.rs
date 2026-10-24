//! WebAssembly adapter for the GitHub Pages demo ([ADR 0002]).
//!
//! The browser runs the shared `mmorpg-core` zone simulation as a **local
//! single-player zone host**, like a listen server: [`LocalZone`] builds the
//! same `ZoneSimulation` and content revision as `mmorpg-zone-host`, accepts
//! `mmorpg-protocol` command bytes with increasing sequences, advances fixed
//! `TICK_HZ` ticks, and returns only encoded player-scoped projections, which the
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
//! [ADR 0002]: https://github.com/moenarch-games/mmorpg/blob/main/docs/adr/0002-browser-embeds-zone-simulation.md
#![forbid(unsafe_code)]

pub mod action_lab;
pub mod arena_presentation;
pub mod catalog;
pub mod host;
pub mod scenario;
pub mod scenery;
pub mod static_scene;
pub mod units;

use mmorpg_characters::{CharacterStore, EntryBackend, EntryChange, LeaseHolder, StoreError};
use mmorpg_core::{CharacterId, ClassChoice, PlayerClass, Sex};
use wasm_bindgen::prelude::{JsError, JsValue, wasm_bindgen};

use crate::host::{LOCAL_ZONE_ID, LocalZoneHost, SubmitOutcome};
use crate::scenario::Scenario;

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

/// The authoritative simulation rate (`mmorpg_core::TICK_HZ`) the local host
/// advances at; the browser's `TICK_HZ` must equal it.
#[wasm_bindgen(js_name = tickHz)]
#[must_use]
pub fn tick_hz() -> u16 {
    mmorpg_core::TICK_HZ
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

/// The area of a stored character's committed location, or `undefined` if it was never stored
/// or stands outside every area (#240: the page loads that region's art first).
#[wasm_bindgen(js_name = committedArea)]
pub fn committed_area(entries: &CharacterEntries, character: &str) -> Result<Option<u16>, JsError> {
    let id = character_id(character)?;
    let committed = store(entries)
        .committed(&id)
        .map_err(|error| JsError::new(&error.to_string()))?;
    Ok(committed.and_then(|committed| {
        let [x, _, z] = committed.record.location.position;
        mmorpg_core::greyhaven_vale::area_at(x, z).map(|area| area.id().get())
    }))
}

/// Per area, the imported unit models its creature spawns use, and `"residents"` where a named
/// resident with an appearance preset stands (#418), as JSON
/// `[{"area": id, "models": ["Wolf", …]}]` in area order (#240).
#[wasm_bindgen(js_name = regionModels)]
pub fn region_models() -> String {
    use mmorpg_presentation::units::imported::{Kind, Resident};
    let content = mmorpg_core::greyhaven_vale::content();
    let regions: Vec<_> = mmorpg_core::greyhaven_vale::areas()
        .areas()
        .iter()
        .map(|area| {
            let inside = |[x, z]: [i32; 2]| {
                mmorpg_core::greyhaven_vale::area_at(x, z).is_some_and(|at| at.id() == area.id())
            };
            let mut models: Vec<_> = content
                .creature_spawns()
                .iter()
                .filter(|spawn| inside(spawn.position))
                .filter_map(|spawn| {
                    let family = content.creature_template(spawn.template)?.family;
                    Kind::of_family(family)
                })
                .collect();
            models.sort();
            models.dedup();
            let mut names: Vec<_> = models.iter().map(|model| model.name()).collect();
            if content
                .npcs()
                .iter()
                .any(|npc| inside(npc.position) && Resident::of_name(&npc.name).is_some())
            {
                names.push("residents");
            }
            serde_json::json!({
                "area": area.id().get(),
                "models": names,
            })
        })
        .collect();
    serde_json::Value::Array(regions).to_string()
}

/// The local zone host as seen from JavaScript. Errors surface as exceptions
/// only for malformed input or misuse (unknown player, sequence 0); a stale
/// sequence is a normal `false` result.
#[wasm_bindgen]
pub struct LocalZone {
    host: LocalZoneHost,
    scenario_player: Option<u32>,
}

#[wasm_bindgen]
impl LocalZone {
    /// A fresh zone at tick 0, hosting zone 1 with the hosted content revision.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<LocalZone, JsError> {
        Ok(Self {
            host: LocalZoneHost::new().map_err(js_error)?,
            scenario_player: None,
        })
    }

    /// The saved world of [`LocalZone::checkpoint`], resumed at its tick
    /// without players; throws for bytes of another version or content.
    #[wasm_bindgen(js_name = fromCheckpoint)]
    pub fn from_checkpoint(bytes: &[u8]) -> Result<LocalZone, JsError> {
        Ok(Self {
            host: LocalZoneHost::from_world_checkpoint(bytes).map_err(js_error)?,
            scenario_player: None,
        })
    }

    /// A fresh world in the named review scenario (#371), `seed` mixed into
    /// its RNG; throws for an unknown name. Its ephemeral character is
    /// already in: [`LocalZone::scenario_player`]. Nothing is stored.
    #[wasm_bindgen(js_name = forScenario)]
    pub fn for_scenario(name: &str, seed: u64) -> Result<LocalZone, JsError> {
        let scenario = Scenario::from_name(name)
            .ok_or_else(|| JsError::new(&format!("unknown scenario {name:?}")))?;
        let (host, player) = LocalZoneHost::scenario(scenario, seed).map_err(js_error)?;
        Ok(Self {
            host,
            scenario_player: Some(player),
        })
    }

    /// A fresh Action Lab world (#381) with a configured ephemeral character
    /// of `class` at `level` wearing `gear` (`starter`, `standard` or
    /// `none`), `main_hand` replacing its weapon unless 0, and `resource`
    /// (clamped to its maximum) unless undefined. Throws for an invalid
    /// setup. The character is [`LocalZone::scenario_player`]; nothing is stored.
    #[wasm_bindgen(js_name = forActionLab)]
    pub fn for_action_lab(
        class: &str,
        level: u8,
        gear: &str,
        #[wasm_bindgen(js_name = mainHand)] main_hand: u16,
        resource: Option<u16>,
        seed: u64,
    ) -> Result<LocalZone, JsError> {
        let setup = action_lab::LabSetup::parse(class, level, gear, main_hand, resource)
            .map_err(|error| JsError::new(&error))?;
        let (host, player) = LocalZoneHost::action_lab(setup, seed).map_err(js_error)?;
        Ok(Self {
            host,
            scenario_player: Some(player),
        })
    }

    /// The player's action graph after the last completed tick, with each
    /// input's arbitration at the next tick, as JSON (#381); undefined for an
    /// unknown player.
    #[wasm_bindgen(js_name = actionInspection)]
    pub fn action_inspection(&self, player: u32) -> Option<String> {
        action_lab::inspection_json(self.host.zone(), player)
    }

    /// The scenario character's player ID; undefined outside a scenario.
    #[wasm_bindgen(getter, js_name = scenarioPlayer)]
    pub fn scenario_player(&self) -> Option<u32> {
        self.scenario_player
    }

    /// The world without its players, to save beside the character records.
    pub fn checkpoint(&self) -> Result<Vec<u8>, JsError> {
        self.host.world_checkpoint().map_err(js_error)
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

    /// Advances one [`TICK_HZ`](mmorpg_core::TICK_HZ) tick and returns the new tick.
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

/// The destructible pieces (#305) feet at `(x, z)` units can repair, in ID order.
#[wasm_bindgen(js_name = reachablePieces)]
#[must_use]
pub fn reachable_pieces(x: i32, z: i32) -> Vec<u16> {
    host::hosted_content()
        .pieces()
        .iter()
        .filter(|piece| piece.within_repair_reach([x, 0, z]))
        .map(|piece| piece.id.get())
        .collect()
}

/// The authored action graph as JSON (#380, #381): actions by region, edges
/// with windows in ms and ticks, recovery modifiers and compatibility rules.
#[wasm_bindgen(js_name = actionGraph)]
#[must_use]
pub fn action_graph() -> String {
    action_lab::graph_json()
}

/// The Action Lab's setup choices as JSON: `{maxLevel, mainHand: [{id, name}]}`.
#[wasm_bindgen(js_name = actionLabOptions)]
#[must_use]
pub fn action_lab_options() -> String {
    serde_json::json!({
        "maxLevel": mmorpg_core::MAX_PLAYER_LEVEL,
        "mainHand": action_lab::main_hand_items(),
    })
    .to_string()
}

/// The area the Action Lab starts in.
#[wasm_bindgen(js_name = actionLabArea)]
#[must_use]
pub fn action_lab_area() -> Option<u16> {
    let ([x, z], _) = action_lab::LAB_LOCATION;
    area_at(x, z)
}

/// The ID of the named core area containing `(x, z)` in units, if any.
#[wasm_bindgen(js_name = areaAt)]
pub fn area_at(x: i32, z: i32) -> Option<u16> {
    scenery::hosted_areas()
        .area_at(x, z)
        .map(|area| area.id().get())
}

/// The area a review scenario (#371) starts in, so the page can load its art
/// first; undefined for an unknown name or a start outside every area.
#[wasm_bindgen(js_name = scenarioArea)]
pub fn scenario_area(name: &str) -> Option<u16> {
    let ([x, z], _) = Scenario::from_name(name)?.location();
    area_at(x, z)
}

/// Presentation relief at `(x, z)` in units; clients raise units by it.
#[wasm_bindgen(js_name = reliefAt)]
pub fn relief_at(x: i32, z: i32) -> i32 {
    scenery::relief_at(x, z)
}

/// The JSON layout of the shared static world both clients draw (#184).
#[wasm_bindgen(js_name = staticScene)]
pub fn static_scene() -> String {
    static_scene::layout().to_owned()
}

/// Review-only static presentation with one oak omitted before spatial batching.
/// The hosted scenery and simulation remain untouched. No animation belongs to an oak.
#[wasm_bindgen(js_name = reviewStaticSceneWithoutOak)]
pub fn review_static_scene_without_oak(index: usize) -> Result<String, JsValue> {
    let mut scenery = scenery::hosted_scenery().clone();
    if !scenery.props.get(index).is_some_and(|prop| {
        prop.kind == mmorpg_scenery::PropKind::Tree(mmorpg_scenery::TreeVariant::Oak)
    }) {
        return Err(JsValue::from_str(
            "The review placement must be an existing oak",
        ));
    }
    scenery.props.remove(index);
    let scene = mmorpg_presentation::StaticScene::new(&scenery);
    let (layout, vertices, indices, models) = static_scene::encode(&scene);
    serde_json::to_string(&serde_json::json!({
        "layout": layout, "vertices": vertices, "indices": indices, "models": models,
    }))
    .map_err(|error| JsValue::from_str(&error.to_string()))
}

/// Positions, normals and optional colours of every static-scene geometry.
#[wasm_bindgen(js_name = staticSceneVertices)]
pub fn static_scene_vertices() -> Vec<f32> {
    static_scene::vertices().to_vec()
}

#[wasm_bindgen(js_name = staticSceneIndices)]
pub fn static_scene_indices() -> Vec<u32> {
    static_scene::indices().to_vec()
}

/// Column-major model matrices of every static-scene draw, 16 floats each.
#[wasm_bindgen(js_name = staticSceneModels)]
pub fn static_scene_models() -> Vec<f32> {
    static_scene::models().to_vec()
}

/// Animated-part model matrices at a cosmetic clock, 16 floats per part.
#[wasm_bindgen(js_name = staticSceneMotion)]
pub fn static_scene_motion(seconds: f64, animate: bool) -> Vec<f32> {
    static_scene::motion(seconds, animate)
}

/// Per animated part 1 (drawn) or 0 (a cave roof cut away over the viewer's feet, metres;
/// `viewer` is empty without one; or an Outpost kit building's blockout while `outpost_kit`
/// is drawn, #420).
#[wasm_bindgen(js_name = staticSceneShown)]
pub fn static_scene_shown(viewer: &[f64], outpost_kit: bool) -> Vec<u8> {
    static_scene::shown(viewer, outpost_kit)
}

/// Rendered terrain height in metres at `(x, z)` metres.
#[wasm_bindgen(js_name = surfaceY)]
pub fn surface_y(x: f64, z: f64) -> f64 {
    static_scene::surface_y(x, z)
}

/// The rest (idle) clip of an imported unit model, which the page measures it in.
#[wasm_bindgen(js_name = importedRestClip)]
pub fn imported_rest_clip(kind: &str) -> Option<String> {
    mmorpg_presentation::units::imported::Kind::from_name(kind)
        .map(|kind| kind.clips().idle.to_owned())
}

/// The shared unit presentation of one joined player (#184): push received projections,
/// then ask for each frame's unit draws; see [`units`].
#[wasm_bindgen]
pub struct UnitPresenter(units::Units);

#[wasm_bindgen]
impl UnitPresenter {
    #[wasm_bindgen(constructor)]
    pub fn new(player: u32) -> Self {
        Self(units::Units::new(player))
    }

    /// The same character presenter on the flat Battle Royale presentation.
    #[wasm_bindgen(js_name = forArena)]
    pub fn for_arena(player: u32) -> Self {
        Self(units::Units::with_content(
            player,
            arena_presentation::content(),
            arena_presentation::shared_scenery(),
        ))
    }

    #[wasm_bindgen(js_name = modelReady)]
    pub fn model_ready(&mut self, kind: &str, clips: &str) -> Result<(), JsError> {
        self.0
            .model_ready(kind, clips)
            .map_err(|error| JsError::new(&error))
    }

    /// The residents' appearance presets loaded (#418): named residents become Rangers.
    #[wasm_bindgen(js_name = residentsReady)]
    pub fn residents_ready(&mut self) {
        self.0.residents_ready();
    }

    pub fn push(&mut self, projection: &[u8], seconds: f64) -> Result<(), JsError> {
        self.0
            .push(projection, seconds)
            .map_err(|error| JsError::new(&error))
    }

    pub fn reset(&mut self) {
        self.0.reset();
    }

    #[allow(clippy::too_many_arguments)]
    pub fn frame(
        &mut self,
        units: &[f64],
        seconds: f64,
        dt: f64,
        #[wasm_bindgen(js_name = reducedMotion)] reduced_motion: bool,
        #[wasm_bindgen(js_name = barYaw)] bar_yaw: f32,
        choice: u16,
        headwear: &str,
        #[wasm_bindgen(js_name = previewYaw)] preview_yaw: f32,
    ) -> Result<String, JsError> {
        self.0
            .frame(
                units,
                seconds,
                dt,
                reduced_motion,
                bar_yaw,
                choice,
                headwear,
                preview_yaw,
            )
            .map_err(|error| JsError::new(&error))
    }

    /// Model matrices of the last call's draws, 16 floats each.
    pub fn models(&self) -> Vec<f32> {
        self.0.buffers.models.clone()
    }

    /// Positions, normals and colours of the last call's new geometries.
    pub fn vertices(&self) -> Vec<f32> {
        self.0.buffers.vertices.clone()
    }

    pub fn indices(&self) -> Vec<u32> {
        self.0.buffers.indices.clone()
    }
}

/// The walk-in inn's presented door and interior look (#158), shared with the native client;
/// the page picks the door against the drawn leaf mesh.
#[wasm_bindgen]
pub struct InnPresenter(mmorpg_presentation::buildings::Inn);

#[wasm_bindgen]
impl InnPresenter {
    #[wasm_bindgen(constructor)]
    #[must_use]
    pub fn new() -> Self {
        Self(mmorpg_presentation::buildings::Inn::new(
            scenery::hosted_scenery(),
        ))
    }

    /// The asset-tooling bundle entries to draw, in [`Self::frame`] order.
    #[must_use]
    pub fn entries() -> Vec<String> {
        mmorpg_presentation::buildings::INN_ENTRIES
            .iter()
            .map(|&key| key.to_owned())
            .collect()
    }

    /// Per entry 16 column-major model floats then 1 (shown) or 0 (hidden). `viewer` holds the
    /// viewer's feet in metres, or is empty without one.
    pub fn frame(
        &mut self,
        open: bool,
        viewer: &[f64],
        dt: f64,
        #[wasm_bindgen(js_name = reducedMotion)] reduced_motion: bool,
    ) -> Vec<f32> {
        let viewer = (viewer.len() == 3).then(|| [viewer[0], viewer[1], viewer[2]]);
        self.0
            .frame(open, viewer, dt, reduced_motion)
            .iter()
            .flat_map(|part| {
                part.model
                    .into_iter()
                    .chain([if part.visible { 1. } else { 0. }])
            })
            .collect()
    }

    /// The content door the zone would let feet at `(x, z)` units use, or 0.
    #[wasm_bindgen(js_name = reachableDoor)]
    #[must_use]
    pub fn reachable_door(x: i32, z: i32) -> u8 {
        host::hosted_content()
            .doors()
            .iter()
            .find(|door| door.within_reach([x, 0, z]))
            .map_or(0, |door| door.id.get())
    }
}

/// The Sunken Shrine's asset-tooling kit modules (#431), placed by the presentation the native
/// client shares.
#[wasm_bindgen(js_name = shrineKitEntries)]
#[must_use]
pub fn shrine_kit_entries() -> Vec<String> {
    mmorpg_presentation::shrine::entries()
        .into_iter()
        .map(str::to_owned)
        .collect()
}

/// The kit entry of each placed shrine module, in [`shrine_kit_models`] order.
#[wasm_bindgen(js_name = shrineKitKeys)]
#[must_use]
pub fn shrine_kit_keys() -> Vec<String> {
    mmorpg_presentation::shrine::parts(scenery::hosted_scenery())
        .iter()
        .map(|part| part.key.to_owned())
        .collect()
}

/// 16 column-major model floats per placed shrine module.
#[wasm_bindgen(js_name = shrineKitModels)]
#[must_use]
pub fn shrine_kit_models() -> Vec<f32> {
    mmorpg_presentation::shrine::parts(scenery::hosted_scenery())
        .iter()
        .flat_map(|part| part.model)
        .collect()
}

/// The Outpost kit buildings' asset-tooling entries (#420), `outpost.kit.<part>` with
/// `/<variant>` unless it is `default`, placed by the presentation the native client shares.
#[wasm_bindgen(js_name = outpostKitEntries)]
#[must_use]
pub fn outpost_kit_entries() -> Vec<String> {
    mmorpg_presentation::outpost_kit::entries(scenery::hosted_scenery())
        .into_iter()
        .map(str::to_owned)
        .collect()
}

/// The kit entry of each placed Outpost kit module, in [`outpost_kit_models`] order.
#[wasm_bindgen(js_name = outpostKitKeys)]
#[must_use]
pub fn outpost_kit_keys() -> Vec<String> {
    mmorpg_presentation::outpost_kit::parts(scenery::hosted_scenery())
        .iter()
        .map(|part| part.entry.to_owned())
        .collect()
}

/// 16 column-major model floats per placed Outpost kit module.
#[wasm_bindgen(js_name = outpostKitModels)]
#[must_use]
pub fn outpost_kit_models() -> Vec<f32> {
    mmorpg_presentation::outpost_kit::parts(scenery::hosted_scenery())
        .iter()
        .flat_map(|part| part.model)
        .collect()
}

/// Greyhaven's ambient animal entries (#419), in [`ambient_animal_frame`] order.
#[wasm_bindgen(js_name = ambientAnimalEntries)]
#[must_use]
pub fn ambient_animal_entries() -> Vec<String> {
    mmorpg_presentation::ambient::entries()
        .into_iter()
        .map(str::to_owned)
        .collect()
}

/// The ambient animals' clip names, indexed by each pose's clip float.
#[wasm_bindgen(js_name = ambientAnimalClips)]
#[must_use]
pub fn ambient_animal_clips() -> Vec<String> {
    mmorpg_presentation::ambient::CLIPS
        .into_iter()
        .map(str::to_owned)
        .collect()
}

/// Feet of each ambient animal's first waypoint in units, `x, z` pairs.
#[wasm_bindgen(js_name = ambientAnimalAnchors)]
#[must_use]
pub fn ambient_animal_anchors() -> Vec<i32> {
    mmorpg_presentation::ambient::anchors()
        .into_iter()
        .flatten()
        .collect()
}

/// Per ambient animal at `seconds`: 16 column-major model floats, the clip index and the clip
/// time. Without `animate` (reduced motion) each holds still.
#[wasm_bindgen(js_name = ambientAnimalFrame)]
#[must_use]
pub fn ambient_animal_frame(seconds: f64, animate: bool) -> Vec<f32> {
    mmorpg_presentation::ambient::poses(scenery::hosted_scenery(), seconds, animate)
        .iter()
        .flat_map(|pose| {
            #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
            let tail = [pose.clip as f32, pose.time as f32];
            pose.model.into_iter().chain(tail)
        })
        .collect()
}

impl Default for InnPresenter {
    fn default() -> Self {
        Self::new()
    }
}

fn js_error(error: host::LocalZoneError) -> JsError {
    JsError::new(&error.to_string())
}
