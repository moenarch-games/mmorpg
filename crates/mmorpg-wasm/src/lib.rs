//! WebAssembly adapter for the GitHub Pages demo ([ADR 0002]).
//!
//! The browser runs the shared `mmorpg-core` zone simulation as a **local
//! single-player zone host**, like a listen server: [`LocalZone`] builds the
//! same `ZoneSimulation` and content revision as `mmorpg-zone-host`, accepts
//! `mmorpg-protocol` command bytes with increasing sequences, advances fixed
//! `TICK_HZ` ticks, and returns only encoded player-scoped projections, which the
//! browser decodes like a network source's, through [`wire`]: the
//! `mmorpg-protocol` decoder and command encoder exported to JavaScript.
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

pub use mmorpg_local_host::{
    action_lab, host, lab_diagnostics, lab_experiment, lab_promotion, scenario,
};
pub mod arena_presentation;
pub mod camera;
pub mod catalog;
pub mod client_model;
pub mod constants;
pub mod front_end;
pub mod party;
pub mod projections;
pub mod scenery;
pub mod shared_presentation;
pub mod static_scene;
pub mod unit_layout;
pub mod units;
pub mod wire;
pub mod world_map;

use mmorpg_characters::{CharacterStore, EntryBackend, EntryChange, LeaseHolder, StoreError};
use mmorpg_core::{CharacterId, ClassChoice, PlayerClass, Sex};
use tsify::Tsify as _;
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

/// The shared timed-sword arm fit, over the browser provider's already mixed pose. Five
/// frames (parent, upper arm, forearm, hand, weapon socket), each position + quaternion.
#[wasm_bindgen(js_name = swordArmPose)]
pub fn sword_arm_pose(
    name: &str,
    progress: f32,
    weight: f32,
    frames: &[f64],
    height: f64,
    floor: f64,
) -> Result<Vec<f64>, JsError> {
    use mmorpg_presentation::units::{
        imported::{Layer, Mask},
        kit::Frame,
        sword,
    };
    if frames.len() != 35
        || frames.iter().any(|value| !value.is_finite())
        || !height.is_finite()
        || height <= 0.
        || !floor.is_finite()
    {
        return Err(JsError::new(
            "a sword arm needs five finite frames and valid model bounds",
        ));
    }
    let frame = |i: usize| Frame {
        p: [frames[i], frames[i + 1], frames[i + 2]],
        q: [frames[i + 3], frames[i + 4], frames[i + 5], frames[i + 6]],
    };
    Ok(sword::fit(
        [frame(0), frame(7), frame(14), frame(21)],
        frame(28),
        height,
        floor,
        Layer {
            clip: sword::aim(name).map_or("", |aim| sword::layer(aim, 0., 0.).clip),
            time: progress,
            weight,
            mask: Mask::Upper,
        },
    )
    .map_or_else(Vec::new, |rotations| {
        rotations.into_iter().flatten().collect()
    }))
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
pub fn committed_area(entries: &CharacterEntries, character: &str) -> Result<Option<u32>, JsError> {
    let id = character_id(character)?;
    let committed = store(entries)
        .committed(&id)
        .map_err(|error| JsError::new(&error.to_string()))?;
    Ok(committed.and_then(|committed| {
        let [x, _, z] = committed.record.location.position;
        mmorpg_core::greyhaven_vale::area_at(x, z).map(|area| area.id().get())
    }))
}

/// Where and when the selected character's next local entry is expected to start (#855), so the
/// selection screen can prepare that view before Enter World: `[x, feetY, z, centreY]` in core
/// units, the facing in radians and the zone tick of its first projection. A character stored in
/// the local zone enters at its committed location, any other at the first spawn slot; the tick
/// is the saved world's (`checkpoint`), or a fresh zone's (0) without a usable one. Presentation
/// only: the zone decides where the character enters.
#[wasm_bindgen(js_name = entryPreview)]
pub fn entry_preview(
    entries: &CharacterEntries,
    character: &str,
    checkpoint: Option<Vec<u8>>,
) -> Result<Vec<f64>, JsError> {
    let id = character_id(character)?;
    let committed = store(entries)
        .committed(&id)
        .map_err(|error| JsError::new(&error.to_string()))?;
    let stored = committed
        .map(|committed| committed.record.location)
        .filter(|location| location.zone_id == LOCAL_ZONE_ID);
    let (feet, facing) = match stored {
        Some(location) => {
            let [x, y, z] = location.position;
            (
                [x, y - mmorpg_core::PLAYER_HALF_EXTENTS_UNITS[1], z],
                location.facing,
            )
        }
        None => (
            mmorpg_core::greyhaven_vale::SPAWN_GRID
                .feet(0)
                .unwrap_or([0, 0, 0]),
            0,
        ),
    };
    let tick = checkpoint
        .and_then(|bytes| mmorpg_protocol::decode_canonical_snapshot(&bytes).ok())
        .map_or(0, |snapshot| snapshot.tick);
    #[expect(
        clippy::cast_precision_loss,
        reason = "a zone tick stays far below 2^53"
    )]
    let tick = tick as f64;
    Ok(vec![
        f64::from(feet[0]),
        f64::from(feet[1]),
        f64::from(feet[2]),
        f64::from(feet[1] + mmorpg_core::PLAYER_HALF_EXTENTS_UNITS[1]),
        f64::from(facing) / 65_536.0 * std::f64::consts::TAU,
        tick,
    ])
}

/// Physique V1 (#633, #634) for the picker, as JSON: the fixed-point scale and neutral value, the
/// ±modifier bound in basis points, the eleven sliders in canonical order (`key`, `affectsStats`),
/// the attributes the modifier scales and the six presets' exact fixed-point vectors. All of it
/// is `mmorpg-core`'s; the page holds only labels.
#[wasm_bindgen(js_name = physiqueModel)]
#[must_use]
pub fn physique_model() -> String {
    use mmorpg_core::{PhysiqueParam, PhysiquePreset};
    serde_json::json!({
        "scale": mmorpg_core::PHYSIQUE_SCALE,
        "neutral": mmorpg_core::PHYSIQUE_NEUTRAL,
        "maxModifierBp": mmorpg_core::MAX_PHYSIQUE_MODIFIER_BP,
        "params": PhysiqueParam::ALL.iter().map(|param| serde_json::json!({
            "key": param.key(),
            "affectsStats": param.affects_stats(),
        })).collect::<Vec<_>>(),
        "attributes": ["maxHealth", "meleeDamage"],
        "presets": PhysiquePreset::ALL.iter().map(|preset| serde_json::json!({
            "name": preset.name(),
            "values": preset.physique().fixed(),
        })).collect::<Vec<_>>(),
    })
    .to_string()
}

fn physique(values: &[u16]) -> Result<mmorpg_core::Physique, JsError> {
    let values: [u16; mmorpg_core::PHYSIQUE_PARAMS] = values
        .try_into()
        .map_err(|_| JsError::new("a physique has exactly eleven values"))?;
    mmorpg_core::Physique::from_fixed(values).map_err(|error| JsError::new(&error.to_string()))
}

/// The combined, clamped modifiers of fixed-point physique `values`, in basis points, in
/// `physiqueModel().attributes` order: `mmorpg-core`'s own calculation (#634's live preview).
#[wasm_bindgen(js_name = physiqueModifiers)]
pub fn physique_modifiers(values: &[u16]) -> Result<Vec<i32>, JsError> {
    let physique = physique(values)?;
    Ok(mmorpg_core::PhysiqueAttribute::ALL
        .iter()
        .map(|&attribute| physique.modifier_bp(attribute))
        .collect())
}

/// The committed physique of a stored character, or `undefined` if it was never stored.
#[wasm_bindgen(js_name = committedPhysique)]
pub fn committed_physique(
    entries: &CharacterEntries,
    character: &str,
) -> Result<Option<Vec<u16>>, JsError> {
    let id = character_id(character)?;
    store(entries)
        .committed(&id)
        .map(|committed| committed.map(|committed| committed.record.physique.fixed().to_vec()))
        .map_err(|error| JsError::new(&error.to_string()))
}

/// Saves the picker's physique `values` to a stored character outside any zone, under its lease
/// at `zoneEpoch` (above any earlier page's); the page must hold the character for this tab.
/// Returns `false`, changing nothing, for a character never stored: its first entry creates it
/// with the values (`LocalZone.enter`'s `physique`).
#[wasm_bindgen(js_name = customizePhysique)]
pub fn customize_physique(
    entries: &CharacterEntries,
    character: &str,
    values: &[u16],
    #[wasm_bindgen(js_name = zoneEpoch)] zone_epoch: u64,
) -> Result<bool, JsError> {
    let id = character_id(character)?;
    let physique = physique(values)?;
    let holder = LeaseHolder {
        zone_id: LOCAL_ZONE_ID,
        zone_epoch,
    };
    host::customize_stored_physique(&mut store(entries), &id, physique, holder).map_err(js_error)
}

/// The `combat-sounds` bundle's SoundManifest JSON with mmorpg's cues and distance curve
/// added, the same manifest the native client plays (#447).
#[wasm_bindgen(js_name = soundManifest)]
pub fn sound_manifest(bundle: &str) -> Result<String, JsError> {
    mmorpg_presentation::audio::sound_manifest(bundle).map_err(|error| JsError::new(&error))
}

/// The sway chains (Atlas 6.6.39) an appearance JSON declares on its hair, beards and outfits:
/// a JSON array of `{ joints, limit }` with `limit` in radians, shared with the native client.
#[wasm_bindgen(js_name = declaredChains)]
pub fn declared_chains(appearance: &str) -> Result<String, JsError> {
    let appearance: serde_json::Value = serde_json::from_str(appearance)?;
    let chains: Vec<_> = mmorpg_presentation::units::sway::chains_of(&[&appearance])
        .into_iter()
        .map(|chain| serde_json::json!({ "joints": chain.joints, "limit": chain.limit }))
        .collect();
    Ok(serde_json::Value::Array(chains).to_string())
}

/// The `humanoid-appearance` JSON the Ranger figure `character` draws from (#39, #493): the
/// bundle's own, with the game's humanoid enemy presets beside the male figure's, the presets
/// the native client draws too. `kit` is the Redbrand kit's descriptor JSON when it was fetched
/// (#675): its outfit joins the male's and dresses the bandits. `hair_kit` is the hair kit's
/// descriptor JSON when it was fetched (#677): its pieces are the male's hair and beards.
#[wasm_bindgen(js_name = figureAppearance)]
pub fn figure_appearance(
    character: &str,
    appearance: &str,
    kit: Option<String>,
    hair_kit: Option<String>,
) -> Result<String, JsError> {
    let figure = mmorpg_presentation::units::imported::Figure::of_character(character)
        .ok_or_else(|| JsError::new(&format!("no Ranger figure {character}")))?;
    let appearance: serde_json::Value = serde_json::from_str(appearance)?;
    let kit: Option<serde_json::Value> = kit.as_deref().map(serde_json::from_str).transpose()?;
    let hair_kit: Option<serde_json::Value> =
        hair_kit.as_deref().map(serde_json::from_str).transpose()?;
    Ok(mmorpg_presentation::units::presets::figure_appearance(
        figure,
        &appearance,
        kit.as_ref(),
        hair_kit.as_ref(),
    )
    .to_string())
}

/// A unit's own seeded variation of the preset `id` (asset-tooling #380) over `appearance`
/// (with the enemy presets), as preset JSON: the same man the native client draws for that seed.
#[wasm_bindgen(js_name = varyPreset)]
pub fn vary_preset(appearance: &str, id: &str, seed: u32) -> Result<String, JsError> {
    let appearance: serde_json::Value = serde_json::from_str(appearance)?;
    mmorpg_presentation::units::presets::vary_preset(&appearance, id, seed)
        .map(|preset| preset.to_string())
        .ok_or_else(|| JsError::new(&format!("the Ranger bundle has no preset {id}")))
}

/// Per shared loadout name, the bundle items it wears and their sockets (`Loadout::attachments`,
/// #1012), as JSON `{"sword-and-shield": [{"key": "equipment.sword.arming", "socket":
/// "hand_r.weapon"}, …], …}`: the one table both clients attach from.
#[wasm_bindgen(js_name = loadoutAttachments)]
#[must_use]
pub fn loadout_attachments() -> String {
    use mmorpg_presentation::units::imported::Loadout;
    use strum::VariantArray;
    let table: serde_json::Map<_, _> = Loadout::VARIANTS
        .iter()
        .map(|loadout| {
            let items = loadout
                .attachments()
                .iter()
                .map(|at| serde_json::json!({ "key": at.item, "socket": at.socket }))
                .collect();
            (loadout.name().to_owned(), serde_json::Value::Array(items))
        })
        .collect();
    serde_json::Value::Object(table).to_string()
}

/// Per area, the imported unit models its creature spawns use, and `"residents"` where a named
/// resident (#418) or a humanoid enemy (#39) with an appearance preset stands, as JSON
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
            let enemies = content.creature_spawns().iter().any(|spawn| {
                inside(spawn.position)
                    && content
                        .creature_template(spawn.template)
                        .is_some_and(|template| Resident::of_creature(&template.name).is_some())
            });
            if enemies
                || content
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

    /// A fresh world like [`LocalZone::new`] whose rolls also derive from `world_seed`
    /// (#1111), which the page draws for each new world; its checkpoints keep it.
    #[wasm_bindgen(js_name = withWorldSeed)]
    pub fn with_world_seed(world_seed: u64) -> Result<LocalZone, JsError> {
        Ok(Self {
            host: LocalZoneHost::with_world_seed(world_seed).map_err(js_error)?,
            scenario_player: None,
        })
    }

    /// The content revision a fresh zone hosts, without building one and its physics (#855).
    #[wasm_bindgen(js_name = hostedContentRevision)]
    #[must_use]
    pub fn hosted_content_revision() -> u64 {
        host::hosted_content().revision()
    }

    /// The hosted content's costly base parts, for another instance of this build (#855).
    #[wasm_bindgen(js_name = hostedContentSeed)]
    #[must_use]
    pub fn hosted_content_seed() -> Vec<i32> {
        host::hosted_content_seed()
    }

    /// Seeds the hosted content's base parts from [`LocalZone::hosted_content_seed`] of this same
    /// build, before anything built them; false when it came too late or does not fit (#855).
    #[wasm_bindgen(js_name = seedHostedContent)]
    #[must_use]
    pub fn seed_hosted_content(seed: &[i32]) -> bool {
        host::seed_hosted_content(seed)
    }

    /// Prepares the hosted content's terrain heightfield now (#855), so the local zone joined
    /// later reuses it rather than preparing it on the way into the world.
    #[wasm_bindgen(js_name = prepareHostedPhysics)]
    pub fn prepare_hosted_physics() {
        if let Some(terrain) = host::hosted_content().definition().terrain() {
            mmorpg_core::physics::prepare_terrain(terrain);
        }
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

    /// A fresh Battle Royale practice arena (#550) with a maximum-level ephemeral character of
    /// `class` and `sex` (as in [`LocalZone::enter`]) wearing `equipment` (six catalog item IDs,
    /// 0 for an empty slot). Throws for an illegal build. The character is
    /// [`LocalZone::scenario_player`]; nothing is stored.
    #[wasm_bindgen(js_name = forPracticeArena)]
    pub fn for_practice_arena(class: u8, sex: u8, equipment: &[u32]) -> Result<LocalZone, JsError> {
        let build = arena_build(class, sex, equipment)?;
        let (host, player) = LocalZoneHost::practice_arena(&build).map_err(js_error)?;
        Ok(Self {
            host,
            scenario_player: Some(player),
        })
    }

    /// A fresh practice arena for a netcode trial (#680): the build of
    /// [`LocalZone::for_practice_arena`] as [`LocalZone::scenario_player`], players may fight, a
    /// sparring partner ([`LocalZone::sparring_partner`]) stands in, and `rewind` turns on the
    /// match's capped hit rewind. Throws for an illegal build; nothing is stored.
    #[wasm_bindgen(js_name = forArenaTrial)]
    pub fn for_arena_trial(
        class: u8,
        sex: u8,
        equipment: &[u32],
        rewind: bool,
    ) -> Result<LocalZone, JsError> {
        let build = arena_build(class, sex, equipment)?;
        let (host, player) = LocalZoneHost::arena_trial(&build, rewind).map_err(js_error)?;
        Ok(Self {
            host,
            scenario_player: Some(player),
        })
    }

    /// The netcode trial's sparring partner (#680), which the page drives; undefined otherwise.
    #[wasm_bindgen(getter, js_name = sparringPartner)]
    pub fn sparring_partner(&self) -> Option<u32> {
        self.host.sparring_partner()
    }

    /// A fresh Action Lab world (#381) with a configured ephemeral character
    /// of `class` at `level` wearing `gear` (`starter`, `standard` or
    /// `none`), `main_hand` replacing its weapon unless 0, and `resource`
    /// (clamped to its maximum) unless undefined. `config`, unless undefined,
    /// is an experimental action graph (#382, `actionLabBaseline()`'s
    /// format) the world runs instead of the production rules. Throws for an
    /// invalid setup or configuration. The character is
    /// [`LocalZone::scenario_player`]; nothing is stored.
    #[wasm_bindgen(js_name = forActionLab)]
    pub fn for_action_lab(
        class: &str,
        level: u8,
        gear: &str,
        #[wasm_bindgen(js_name = mainHand)] main_hand: u32,
        resource: Option<u16>,
        seed: u64,
        config: Option<String>,
    ) -> Result<LocalZone, JsError> {
        let setup = action_lab::LabSetup::parse(class, level, gear, main_hand, resource)
            .map_err(|error| JsError::new(&error))?;
        let (mut host, player) = LocalZoneHost::action_lab(setup, seed).map_err(js_error)?;
        if let Some(config) = config {
            let graph = lab_experiment::Config::parse(&config)
                .and_then(|config| config.graph())
                .map_err(|error| JsError::new(&error))?;
            host.set_action_graph(graph)
                .map_err(|error| JsError::new(&format!("{error:?}")))?;
        }
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
    /// one keeps its own. A never-stored character is created with the optional fixed-point
    /// `physique` (#634), else neutral. Commands start at sequence 1. IDs are never reused.
    pub fn enter(
        &mut self,
        entries: &CharacterEntries,
        character: &str,
        class: u8,
        sex: u8,
        #[wasm_bindgen(js_name = zoneEpoch)] zone_epoch: u64,
        physique: Option<Vec<u16>>,
    ) -> Result<u32, JsError> {
        let id = character_id(character)?;
        let physique = match physique {
            Some(values) => self::physique(&values)?,
            None => mmorpg_core::Physique::NEUTRAL,
        };
        let class = PlayerClass::from_code(class)
            .zip(Sex::from_code(sex))
            .map(|(class, sex)| ClassChoice { class, sex })
            .ok_or_else(|| JsError::new("the class choice is invalid"))?;
        let holder = LeaseHolder {
            zone_id: LOCAL_ZONE_ID,
            zone_epoch,
        };
        self.host
            .enter_with_physique(&mut store(entries), id, class, physique, holder)
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

    /// The canonical-snapshot digest of the world as 16 hex digits (#382):
    /// a replay that reaches the same digest reproduced the recording.
    #[wasm_bindgen(js_name = canonicalDigest)]
    pub fn canonical_digest(&self) -> Result<String, JsError> {
        lab_experiment::digest(&self.host)
            .map(|digest| format!("{digest:016x}"))
            .map_err(|error| JsError::new(&error))
    }
}

/// The production action graph as an editable experiment configuration (#382).
#[wasm_bindgen(js_name = actionLabBaseline)]
#[must_use]
pub fn action_lab_baseline() -> String {
    serde_json::to_string(&lab_experiment::Config::baseline()).unwrap_or_default()
}

/// Validates an experiment configuration: `{identity, baseline, error}` JSON.
#[wasm_bindgen(js_name = actionLabCheck)]
#[must_use]
pub fn action_lab_check(config: &str) -> String {
    lab_experiment::check(config).to_string()
}

/// The rule values that differ between two configurations, as a JSON list of
/// `path: old → new` lines (#383); throws for a malformed configuration.
#[wasm_bindgen(js_name = actionLabChanges)]
pub fn action_lab_changes(from: &str, to: &str) -> Result<String, JsError> {
    let from = lab_experiment::Config::parse(from).map_err(|error| JsError::new(&error))?;
    let to = lab_experiment::Config::parse(to).map_err(|error| JsError::new(&error))?;
    serde_json::to_string(&lab_promotion::changes(&from, &to))
        .map_err(|error| JsError::new(&error.to_string()))
}

/// Replays a recorded lab scenario under a configuration (#382) across a
/// simulated network (#415) and returns the run as JSON; throws for a
/// malformed or stale scenario, configuration or network.
#[wasm_bindgen(js_name = actionLabRun)]
pub fn action_lab_run(scenario: &str, config: &str, network: &str) -> Result<String, JsError> {
    let scenario =
        lab_experiment::ScenarioFile::parse(scenario).map_err(|error| JsError::new(&error))?;
    let config = lab_experiment::Config::parse(config).map_err(|error| JsError::new(&error))?;
    let network = lab_experiment::Network::parse(network).map_err(|error| JsError::new(&error))?;
    let run =
        lab_experiment::run(&scenario, &config, &network).map_err(|error| JsError::new(&error))?;
    serde_json::to_string(&run).map_err(|error| JsError::new(&error.to_string()))
}

/// Replays a recorded lab scenario under configurations `a` and `b` across
/// one simulated network and reports where they diverge, as JSON (#382, #415).
#[wasm_bindgen(js_name = actionLabCompare)]
pub fn action_lab_compare(
    scenario: &str,
    a: &str,
    b: &str,
    network: &str,
) -> Result<String, JsError> {
    let scenario =
        lab_experiment::ScenarioFile::parse(scenario).map_err(|error| JsError::new(&error))?;
    let a = lab_experiment::Config::parse(a).map_err(|error| JsError::new(&error))?;
    let b = lab_experiment::Config::parse(b).map_err(|error| JsError::new(&error))?;
    let network = lab_experiment::Network::parse(network).map_err(|error| JsError::new(&error))?;
    let comparison = lab_experiment::compare(&scenario, &a, &b, &network)
        .map_err(|error| JsError::new(&error))?;
    serde_json::to_string(&comparison).map_err(|error| JsError::new(&error.to_string()))
}

/// Presents zone `zone` from now on (#145): an online session followed a redirect notice into
/// it. Every later scenery, static-scene, kit and unit export describes that zone's content,
/// whose revision this returns. The local zone host is unaffected.
#[wasm_bindgen(js_name = presentZone)]
#[must_use]
pub fn present_zone(zone: u32) -> u64 {
    scenery::present_zone(mmorpg_core::ZoneId::new(zone));
    scenery::hosted_scenery().content_revision
}

/// The versioned JSON scenery export of the presented content revision.
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

/// The hosted content's waystone (#145) within reach of `(x, z)` units, 0 for none.
#[wasm_bindgen(js_name = waystoneAt)]
#[must_use]
pub fn waystone_at(x: i32, z: i32) -> u32 {
    mmorpg_core::waystone_within_reach(&host::hosted_content(), [x, z]).map_or(0, |id| id.get())
}

/// Names of the waystone network (#145) in ID order: waystone `index + 1`.
#[wasm_bindgen(js_name = waystoneNames)]
#[must_use]
pub fn waystone_names() -> Vec<String> {
    mmorpg_core::WAYSTONES
        .iter()
        .map(|site| site.name.to_owned())
        .collect()
}

/// The destructible pieces (#305) feet at `(x, z)` units can repair, in ID order.
#[wasm_bindgen(js_name = reachablePieces)]
#[must_use]
pub fn reachable_pieces(x: i32, z: i32) -> Vec<u32> {
    host::hosted_content()
        .pieces()
        .iter()
        .filter(|piece| piece.within_repair_reach([x, 0, z]))
        .map(|piece| piece.id.get())
        .collect()
}

/// The night raid target's approximate disturbance circle (#530) as `[x, z, radius]` in units,
/// or empty without a raid target.
#[wasm_bindgen(js_name = raidArea)]
#[must_use]
pub fn raid_area() -> Vec<i32> {
    host::hosted_content()
        .raid()
        .map(|site| {
            let ([x, z], radius) = site.area();
            vec![x, z, radius]
        })
        .unwrap_or_default()
}

/// The night raid target's aftermath aid tasks (#531) as `[x, z, npc]` triples in units, in
/// task order: `npc` is the injured guard to tend, or 0 for a breach to mend.
#[wasm_bindgen(js_name = raidAid)]
#[must_use]
pub fn raid_aid() -> Vec<i32> {
    host::hosted_content()
        .raid()
        .map(|site| {
            site.aid
                .iter()
                .flat_map(|task| {
                    let npc = match task.kind {
                        mmorpg_core::AidKind::TendGuard(npc) => {
                            i32::try_from(npc.get()).unwrap_or(0)
                        }
                        mmorpg_core::AidKind::MendBreach => 0,
                    };
                    [task.at[0], task.at[1], npc]
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The hosted zone's ore veins (#917) as `[x, z]` centres in units, in wire order.
#[wasm_bindgen(js_name = gatherNodes)]
#[must_use]
pub fn gather_nodes() -> Vec<i32> {
    host::hosted_content()
        .gather_nodes()
        .iter()
        .flat_map(|node| node.at)
        .collect()
}

/// For each ore vein (#917) in wire order, the `gather` kit's placement index of its full and
/// of its spent look, as `[full, spent]` pairs.
#[wasm_bindgen(js_name = veinSlots)]
#[must_use]
pub fn vein_slots() -> Vec<u32> {
    mmorpg_presentation::world_dressing::vein_slots(scenery::hosted_scenery())
        .into_iter()
        .flatten()
        .filter_map(|index| u32::try_from(index).ok())
        .collect()
}

/// The night raid target's forewarning spots (#532) in units: the tracks' `[x, z]`, the lookout
/// guard's NPC ID, then each brace spot's `[x, z]`; empty without a raid target.
#[wasm_bindgen(js_name = raidPrep)]
#[must_use]
pub fn raid_prep() -> Vec<i32> {
    host::hosted_content()
        .raid()
        .map(|site| {
            let mut spots = vec![
                site.tracks[0],
                site.tracks[1],
                i32::try_from(site.lookout.get()).unwrap_or(0),
            ];
            spots.extend(site.braces.iter().flatten());
            spots
        })
        .unwrap_or_default()
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
pub fn action_lab_area() -> Option<u32> {
    let ([x, z], _) = action_lab::LAB_LOCATION;
    area_at(x, z)
}

/// The ID of the named core area containing `(x, z)` in units, if any.
#[wasm_bindgen(js_name = areaAt)]
pub fn area_at(x: i32, z: i32) -> Option<u32> {
    scenery::hosted_areas()
        .area_at(x, z)
        .map(|area| area.id().get())
}

/// The area a review scenario (#371) starts in, so the page can load its art
/// first; undefined for an unknown name or a start outside every area.
#[wasm_bindgen(js_name = scenarioArea)]
pub fn scenario_area(name: &str) -> Option<u32> {
    let ([x, z], _) = Scenario::from_name(name)?.location();
    area_at(x, z)
}

/// Presentation relief at `(x, z)` in units; clients raise units by it.
#[wasm_bindgen(js_name = reliefAt)]
pub fn relief_at(x: i32, z: i32) -> i32 {
    scenery::relief_at(x, z)
}

/// The drawn terrain surface at `(x, z)` in units: the physical ground plus the relief, where
/// the ambient animals stand.
#[wasm_bindgen(js_name = surfaceAt)]
pub fn surface_at(x: i32, z: i32) -> i32 {
    scenery::hosted_scenery().surface_at(x, z)
}

/// The JSON layout of the shared static world both clients draw (#184).
#[wasm_bindgen(js_name = staticScene)]
pub fn static_scene() -> String {
    static_scene::layout().to_owned()
}

/// [`static_scene`] as UTF-8 bytes, for [`seed_static_scene`] in another instance of this module
/// (#855): the worker transfers them and the page copies them in, with no 20 MB string to decode
/// and encode again.
#[wasm_bindgen(js_name = staticSceneLayout)]
#[must_use]
pub fn static_scene_layout() -> Vec<u8> {
    static_scene::layout().as_bytes().to_vec()
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
/// `viewer` is empty without one; or a kit's procedural fallback while the client draws that
/// kit: the Outpost kit buildings, #420, or the market square's settlement props, #421).
#[wasm_bindgen(js_name = staticSceneShown)]
pub fn static_scene_shown(viewer: &[f64], outpost_kit: bool, settlement_props: bool) -> Vec<u8> {
    static_scene::shown(
        viewer,
        mmorpg_presentation::Kits {
            outpost: outpost_kit,
            settlement_props,
            // Nature and climbing fallbacks are whole static groups (#49, #621), which the browser
            // hides itself.
            nature: false,
            limestone: false,
            climbing: false,
            crag: false,
            dressed: &[],
        },
    )
}

/// Presentation-only cave adaptation at the projected viewer position, shared with native.
#[wasm_bindgen(js_name = staticSceneInterior)]
pub fn static_scene_interior(viewer: &[f64]) -> f32 {
    if !scenery::hosted_scenery().greyhaven() {
        return 0.;
    }
    mmorpg_presentation::lights::cave_interior(viewer.try_into().ok())
}

/// The scenery's campfire and lamp lights at a cosmetic clock, eight floats each (#600).
#[wasm_bindgen(js_name = staticScenePointLights)]
pub fn static_scene_point_lights(seconds: f64, animate: bool) -> Vec<f32> {
    static_scene::point_lights(seconds, animate)
}

/// Rendered terrain height in metres at `(x, z)` metres.
/// A fresh stream of the hosted zone's terrain tiles (#526) for one renderer.
#[wasm_bindgen(js_name = terrainStream)]
#[must_use]
pub fn terrain_stream() -> static_scene::TerrainStream {
    static_scene::TerrainStream::new(false)
}

/// The presented scenery's derived terrain grids as bytes, for the browser's derived-data cache
/// (#526).
#[wasm_bindgen(js_name = terrainGridCache)]
#[must_use]
pub fn terrain_grid_cache() -> Vec<u8> {
    scenery::terrain_grid_cache()
}

/// Reuses terrain grids from an earlier [`terrain_grid_cache`]: false (nothing seeded) when the
/// bytes are damaged or from another content revision, so the grids are derived as usual.
#[wasm_bindgen(js_name = seedTerrainGrids)]
#[must_use]
pub fn seed_terrain_grids(bytes: &[u8]) -> bool {
    scenery::seed_terrain_grids(bytes)
}

/// Reuses the [`scenery`] export another instance of this module derived (#855: the page's
/// derivation worker): false, nothing seeded, for another content revision's or once built.
#[wasm_bindgen(js_name = seedScenery)]
#[must_use]
pub fn seed_scenery(json: String) -> bool {
    scenery::seed_scenery_json(json)
}

/// The hosted static scene without its geometry (`StaticScene::skeleton`, #855), for
/// [`seed_static_scene`] in another instance of this module.
#[wasm_bindgen(js_name = staticSceneSkeleton)]
#[must_use]
pub fn static_scene_skeleton() -> Vec<u8> {
    static_scene::skeleton()
}

/// Reuses the static scene another instance of this module derived (#855): its
/// [`static_scene_skeleton`], its [`static_scene_layout`] and its buffers, so this instance
/// builds only the terrain tiles. False, nothing seeded, for another content revision's, a
/// layout that is not UTF-8, or once the scene was built.
#[wasm_bindgen(js_name = seedStaticScene)]
#[must_use]
pub fn seed_static_scene(
    skeleton: Vec<u8>,
    layout: Vec<u8>,
    vertices: Vec<f32>,
    indices: Vec<u32>,
    models: Vec<f32>,
) -> bool {
    String::from_utf8(layout)
        .is_ok_and(|layout| static_scene::seed(skeleton, (layout, vertices, indices, models)))
}

/// The hosted static scene's prefix (`StaticScene::prefix`), which [`seed_terrain_far_levels`]
/// checks.
#[wasm_bindgen(js_name = staticScenePrefix)]
#[must_use]
pub fn static_scene_prefix() -> String {
    static_scene::prefix()
}

/// Every terrain tile's far level of the hosted scene (#855), packed as [`static_scene::TerrainStream`]'s `far`
/// packs one, in tile order, for [`seed_terrain_far_levels`] in another instance of this module.
#[wasm_bindgen(js_name = terrainFarLevels)]
#[must_use]
pub fn terrain_far_levels() -> Vec<f32> {
    static_scene::far_levels()
}

/// Reuses the far levels another instance of this module derived (#855) for the scene with
/// `prefix` ([`static_scene_prefix`]), so this instance's terrain streams hand them out instead
/// of building them. False, nothing seeded, for truncated levels or once seeded; another scene's
/// are ignored.
#[wasm_bindgen(js_name = seedTerrainFarLevels)]
#[must_use]
pub fn seed_terrain_far_levels(prefix: String, levels: Vec<f32>) -> bool {
    static_scene::seed_far_levels(prefix, levels)
}

/// The hosted grass's per-sample density and tint (#855), for [`seed_grass`] in another
/// instance of this module; derives the grass now.
#[wasm_bindgen(js_name = grassSeed)]
#[must_use]
pub fn grass_seed() -> Vec<u8> {
    static_scene::grass_seed()
}

/// Reuses the grass another instance of this module derived (#855), so this instance only
/// places its blocked ground on first use. False, nothing seeded, once a seed was given.
#[wasm_bindgen(js_name = seedGrass)]
#[must_use]
pub fn seed_grass(seed: Vec<u8>) -> bool {
    static_scene::seed_grass(seed)
}

/// A fresh stream of the hosted zone's static groups (#526) for one renderer.
#[wasm_bindgen(js_name = groupStream)]
#[must_use]
pub fn group_stream() -> static_scene::GroupStream {
    static_scene::GroupStream::new(false)
}

#[wasm_bindgen(js_name = surfaceY)]
pub fn surface_y(x: f64, z: f64) -> f64 {
    static_scene::surface_y(x, z)
}

/// The grass clumps of the hosted scene's cell `(column, row)`, each `mmorpg_presentation::grass::CLUMP_FLOATS`
/// floats (position in metres, yaw, height, linear RGB, wildflower), in a random order: any prefix thins evenly.
#[wasm_bindgen(js_name = grassCell)]
#[must_use]
pub fn grass_cell(column: i32, row: i32) -> Vec<f32> {
    static_scene::hosted_grass().cell(column, row)
}

/// The side of a grass cell, in metres.
#[wasm_bindgen(js_name = grassCellMetres)]
#[must_use]
pub fn grass_cell_metres() -> f64 {
    mmorpg_presentation::grass::CELL_METRES
}

/// The sparse far clumps of the hosted scene's far cell `(column, row)`, laid out as [`grass_cell`]'s.
#[wasm_bindgen(js_name = grassFarCell)]
#[must_use]
pub fn grass_far_cell(column: i32, row: i32) -> Vec<f32> {
    static_scene::hosted_grass().far_cell(column, row)
}

/// The side of a far grass cell, in metres.
#[wasm_bindgen(js_name = grassFarCellMetres)]
#[must_use]
pub fn grass_far_cell_metres() -> f64 {
    mmorpg_presentation::grass::FAR_CELL_METRES
}

/// The rest (idle) clip of an imported unit model, which the page measures it in.
#[wasm_bindgen(js_name = importedRestClip)]
pub fn imported_rest_clip(kind: &str) -> Option<String> {
    mmorpg_presentation::units::imported::Kind::from_name(kind)
        .map(|kind| kind.clips().idle.to_owned())
}

/// One session's decoded projections (#1009); see [`projections`]. Every projection the page
/// receives is ingested here once: the page gets a view whose unchanged sheets are left out,
/// and the unit presenter reads the decoded projection by id. Every command the session sends
/// goes through [`ProjectionStore::acknowledge`], which echoes the sheet revisions held (#544).
#[wasm_bindgen]
#[derive(Default)]
pub struct ProjectionStore(projections::ProjectionStore);

#[wasm_bindgen]
impl ProjectionStore {
    #[wasm_bindgen(constructor)]
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Ingests a received projection payload strictly; a malformed one changes nothing.
    pub fn ingest(
        &mut self,
        payload: &[u8],
    ) -> Result<tsify::Ts<wire::IngestedProjection>, JsError> {
        let ingested = self
            .0
            .ingest(payload)
            .map_err(|error| JsError::new(&error))?;
        Ok(wire::IngestedProjection::from(ingested).into_ts()?)
    }

    /// Ingests the local zone's projection for `player` without its bytes crossing to the page.
    #[wasm_bindgen(js_name = ingestLocal)]
    pub fn ingest_local(
        &mut self,
        zone: &LocalZone,
        player: u32,
    ) -> Result<tsify::Ts<wire::IngestedProjection>, JsError> {
        let payload = zone.host.projection(player).map_err(js_error)?;
        let ingested = self
            .0
            .ingest_local(&payload)
            .map_err(|error| JsError::new(&error))?;
        Ok(wire::IngestedProjection::from(ingested).into_ts()?)
    }

    /// Encoded command bytes wrapped with the sheet revisions held.
    #[must_use]
    pub fn acknowledge(&self, command: &[u8]) -> Vec<u8> {
        mmorpg_protocol::encode_acknowledged_command(self.0.revisions(), command)
    }

    /// The store's work so far: `[projections, decodes, boundary bytes, sheet views]`.
    #[must_use]
    pub fn work(&self) -> Vec<f64> {
        let work = self.0.work();
        #[allow(clippy::cast_precision_loss)]
        [
            work.projections,
            work.decodes,
            work.boundary_bytes,
            work.sheet_views,
        ]
        .map(|count| count as f64)
        .to_vec()
    }
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

    /// An imported model's `humanoid-clips` descriptor JSON (#500): its traversal clips' speeds,
    /// ledge heights and authored root paths.
    #[wasm_bindgen(js_name = modelMotions)]
    pub fn model_motions(&mut self, kind: &str, descriptor: &str) -> Result<(), JsError> {
        self.0
            .model_motions(kind, descriptor)
            .map_err(|error| JsError::new(&error))
    }

    /// The residents' appearance presets loaded (#418): named residents become Rangers.
    #[wasm_bindgen(js_name = residentsReady)]
    pub fn residents_ready(&mut self) {
        self.0.residents_ready();
    }

    /// Tunes the imported models' layered blend (the Action Lab, #384): crossfade seconds for
    /// the legs and the upper body, and the weights of a leg overlay and an upper-body action.
    /// Values are clamped to 0–1; presentation only.
    #[wasm_bindgen(js_name = setBlend)]
    pub fn set_blend(
        &mut self,
        lower_seconds: f32,
        upper_seconds: f32,
        lower_weight: f32,
        upper_weight: f32,
    ) {
        self.0
            .set_blend(mmorpg_presentation::units::imported::Blend {
                lower_seconds,
                upper_seconds,
                lower_weight,
                upper_weight,
            });
    }

    /// The crag kit's `crag-descriptor` JSON once the page draws the kit, or `""` while it
    /// does not (#668): climbers on its climbable faces are drawn out to the modules' rock.
    ///
    /// # Errors
    ///
    /// For a malformed descriptor.
    #[wasm_bindgen(js_name = setCragDescriptor)]
    pub fn set_crag_descriptor(&mut self, json: &str) -> Result<(), JsError> {
        self.0
            .set_crag_descriptor(json)
            .map_err(|error| JsError::new(&error))
    }

    /// Whether the page draws the `strike-effects` and `spell-effects` bundles (#894): each
    /// frame's `effects` then lists what to play, and their primitive stand-ins are not drawn.
    #[wasm_bindgen(js_name = setEffectKits)]
    pub fn set_effect_kits(&mut self, drawn: bool) {
        self.0.set_effect_kits(drawn);
    }

    /// The platforms (ids) the page's kit draws instead of their procedural pillar, the
    /// fortress kit's lifts (#591): each frame's `platforms` says where each stands.
    #[wasm_bindgen(js_name = setKitPlatforms)]
    pub fn set_kit_platforms(&mut self, platforms: &[u32]) {
        self.0.set_kit_platforms(platforms);
    }

    /// Where each caster's spell starts (#612): `[kind, id, x, y, z]` per unit in metres, its
    /// imported `channel` point as the page posed it; the next frames' beams start there.
    #[wasm_bindgen(js_name = setChannelOrigins)]
    pub fn set_channel_origins(&mut self, origins: &[f64]) -> Result<(), JsError> {
        self.0
            .set_channel_origins(origins)
            .map_err(|error| JsError::new(&error))
    }

    /// Shows or hides the development body overlay (#39): each unit's authoritative collision
    /// body drawn translucent over its model. Presentation only.
    #[wasm_bindgen(js_name = setBodyOverlay)]
    pub fn set_body_overlay(&mut self, shown: bool) {
        self.0.set_body_overlay(shown);
    }

    /// What the page's windows are open toward this frame (#619). Presentation only.
    #[wasm_bindgen(js_name = setInteraction)]
    pub fn set_interaction(&mut self, loot_window: bool, npc: Option<u32>) {
        self.0.set_interaction(loot_window, npc);
    }

    /// Observes the session's projection `id` from `store` (#1009), decoded once when it was
    /// ingested; `false`, changing nothing, once the store no longer retains it.
    pub fn push(&mut self, store: &ProjectionStore, id: u32, seconds: f64) -> bool {
        store
            .0
            .get(id)
            .map(|snapshot| self.0.push(snapshot, seconds))
            .is_some()
    }

    pub fn reset(&mut self) {
        self.0.reset();
    }

    /// The local player pressed or began aiming an attack with `ability` at `seconds`: the
    /// viewer winds up until the zone answers (Atlas 6.6.10). Presentation only.
    pub fn prepare(&mut self, ability: u32, seconds: f64) {
        self.0
            .presenter_mut()
            .prepare(mmorpg_core::AbilityId::new(ability), seconds);
    }

    /// The directed strike being prepared aims at `aim` (#154): its wind-up shows that slash
    /// (#582). Presentation only.
    #[wasm_bindgen(js_name = aimPreparation)]
    pub fn aim_preparation(&mut self, aim: tsify::Ts<wire::StrikeAim>) -> Result<(), JsError> {
        let aim: mmorpg_core::StrikeAim = aim.to_rust()?.into();
        self.0.presenter_mut().aim_preparation(aim);
        Ok(())
    }

    /// The prepared input went to the zone at `seconds`; an unanswered wind-up lets go later.
    #[wasm_bindgen(js_name = preparedSent)]
    pub fn prepared_sent(&mut self, seconds: f64) {
        self.0.presenter_mut().prepared_sent(seconds);
    }

    /// Blur, menus, a retired input or a superseding gesture: the wind-up fades without a swing.
    #[wasm_bindgen(js_name = abandonPreparation)]
    pub fn abandon_preparation(&mut self) {
        self.0.presenter_mut().abandon_preparation();
    }

    /// Sound cues and fades of the projections pushed since the last call (#447), as JSON.
    #[wasm_bindgen(js_name = takeAudio)]
    pub fn take_audio(&mut self) -> String {
        self.0.take_audio()
    }

    /// The confirmed impacts of the projections pushed since the last call (#52, #1058):
    /// `[kind (0 hit, 1 block), unit kind code, unit id]` per struck unit, in event order.
    #[wasm_bindgen(js_name = takeImpacts)]
    pub fn take_impacts(&mut self) -> Vec<u32> {
        self.0.take_impacts()
    }

    /// One frame, laid out in WASM memory (#1057, [`unit_layout`]): the units sampled from the
    /// pushed projections 2 ticks behind the newest, `elapsedTicks` past it (#1058), with the
    /// viewer at `ownBody` (`[x, y, z, facing, vx, vy, vz]`) while the page predicts it. Read it
    /// through [`Self::directory`] and [`Self::memory`], with [`Self::fresh_strings`] when its
    /// header counts new strings.
    pub fn frame(
        &mut self,
        #[wasm_bindgen(js_name = elapsedTicks)] elapsed_ticks: f64,
        #[wasm_bindgen(js_name = ownBody)] own_body: &[f64],
        seconds: f64,
        dt: f64,
        #[wasm_bindgen(js_name = reducedMotion)] reduced_motion: bool,
        #[wasm_bindgen(js_name = barYaw)] bar_yaw: f32,
        choice: u32,
        headwear: &str,
        #[wasm_bindgen(js_name = previewYaw)] preview_yaw: f32,
    ) -> Result<(), JsError> {
        self.0
            .frame(
                elapsed_ticks,
                own_body,
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

    /// A frame of exactly `units` (`[kind, id, x, y, z, facing]` each), a scrubbed recording's,
    /// laid out like [`Self::frame`].
    #[wasm_bindgen(js_name = frameAt)]
    pub fn frame_at(
        &mut self,
        units: &[f64],
        seconds: f64,
        dt: f64,
        #[wasm_bindgen(js_name = reducedMotion)] reduced_motion: bool,
        #[wasm_bindgen(js_name = barYaw)] bar_yaw: f32,
        choice: u32,
        headwear: &str,
        #[wasm_bindgen(js_name = previewYaw)] preview_yaw: f32,
    ) -> Result<(), JsError> {
        self.0
            .frame_at(
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

    /// The address of the last layout's buffer directory, [`units::Directory`]: pointer,
    /// capacity and length of its words, numbers, models, vertices and indices.
    pub fn directory(&self) -> usize {
        std::ptr::from_ref(self.0.directory()) as usize
    }

    /// The module's memory, which the directory's pointers address.
    pub fn memory(&self) -> JsValue {
        wasm_bindgen::memory()
    }

    /// The last layout's new strings, each followed by a newline, in interning order.
    #[wasm_bindgen(js_name = freshStrings)]
    pub fn fresh_strings(&self) -> String {
        self.0.layout().fresh_strings().to_owned()
    }
}

/// One walk-in building's presented door and interior look (#158): the inn or Elden's house,
/// shared with the native client; the page picks the door against the drawn leaf mesh.
#[wasm_bindgen]
pub struct BuildingPresenter(mmorpg_presentation::buildings::WalkIn);

#[wasm_bindgen]
impl BuildingPresenter {
    /// Building `index` of [`Self::count`].
    #[wasm_bindgen(constructor)]
    pub fn new(index: usize) -> Result<Self, JsError> {
        let spec = mmorpg_presentation::buildings::specs(scenery::hosted_scenery())
            .into_iter()
            .nth(index)
            .ok_or_else(|| JsError::new("no such walk-in building"))?;
        Ok(Self(mmorpg_presentation::buildings::WalkIn::new(
            spec,
            scenery::hosted_scenery(),
        )))
    }

    /// A mausoleum at a caller-owned content placement (#683); descriptor is the kit attachment.
    pub fn mausoleum(
        descriptor: &str,
        x: i32,
        y: i32,
        z: i32,
        quarters: u8,
    ) -> Result<Self, JsError> {
        let spec = mmorpg_presentation::buildings::Spec::mausoleum(descriptor, [x, y, z], quarters)
            .map_err(|e| JsError::new(&e))?;
        Ok(Self(mmorpg_presentation::buildings::WalkIn::new(
            spec,
            scenery::hosted_scenery(),
        )))
    }

    /// Same adaptive indoor boom as the native presenter. Vectors are metres.
    pub fn camera(
        &mut self,
        viewer: &[f64],
        eye: &[f64],
        target: &[f64],
        dt: f64,
        reduced: bool,
    ) -> Vec<f64> {
        let (Ok(viewer), Ok(eye), Ok(target)) =
            (viewer.try_into(), eye.try_into(), target.try_into())
        else {
            return eye.to_vec();
        };
        self.0.camera(viewer, eye, target, dt, reduced).to_vec()
    }

    pub fn lighting(&self, viewer: &[f64]) -> f32 {
        viewer.try_into().map_or(0., |p| self.0.lighting(p))
    }

    /// Eight floats, matching static scene lights: position, sRGB, lumens and range.
    #[wasm_bindgen(js_name = pointLight)]
    pub fn point_light(&self) -> Vec<f64> {
        self.0.light().map_or_else(Vec::new, |light| {
            vec![
                light.position[0],
                light.position[1],
                light.position[2],
                f64::from((light.color >> 16) & 255) / 255.,
                f64::from((light.color >> 8) & 255) / 255.,
                f64::from(light.color & 255) / 255.,
                light.lumens,
                light.range,
            ]
        })
    }

    /// How many walk-in buildings there are.
    #[must_use]
    pub fn count() -> usize {
        mmorpg_presentation::buildings::specs(scenery::hosted_scenery()).len()
    }

    /// The walk-in cottage (#843) this building is, by its house collider, or 0.
    #[must_use]
    pub fn cottage(&self) -> u32 {
        self.0.spec().cottage.unwrap_or(0)
    }

    /// Whether feet at `viewer` (metres) stand in the room.
    #[must_use]
    pub fn inside(&self, viewer: &[f64]) -> bool {
        viewer.try_into().is_ok_and(|p| self.0.inside(p))
    }

    /// The bundle directory under `buildings/`.
    #[must_use]
    pub fn bundle(&self) -> String {
        self.0.spec().bundle.to_owned()
    }

    /// The content door the building presents.
    #[must_use]
    pub fn door(&self) -> u32 {
        self.0.spec().door.map_or(0, |door| door.get())
    }

    /// The door leaf's bundle entry.
    #[wasm_bindgen(js_name = doorEntry)]
    #[must_use]
    pub fn door_entry(&self) -> String {
        self.0.spec().door_entry().unwrap_or("").to_owned()
    }

    /// The asset-tooling bundle entries to draw, in [`Self::frame`] order.
    #[must_use]
    pub fn entries(&self) -> Vec<String> {
        self.0
            .spec()
            .entries
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
    pub fn reachable_door(x: i32, z: i32) -> u32 {
        host::hosted_content()
            .doors()
            .iter()
            .find(|door| door.within_reach([x, 0, z]))
            .map_or(0, |door| door.id.get())
    }

    /// The seat feet at `(x, z)` units reach, 0 for none (#619); the zone decides.
    #[wasm_bindgen(js_name = reachableSeat)]
    #[must_use]
    pub fn reachable_seat(x: i32, z: i32) -> u32 {
        host::hosted_content()
            .seat_in_reach([x, z])
            .map_or(0, |seat| seat.id.get())
    }
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

/// Each placed Outpost kit module's building wear in [0, 1], in [`outpost_kit_models`] order: the
/// browser blends it from the kit's wear masks.
#[wasm_bindgen(js_name = outpostKitWear)]
#[must_use]
pub fn outpost_kit_wear() -> Vec<f32> {
    mmorpg_presentation::outpost_kit::parts(scenery::hosted_scenery())
        .iter()
        .map(|part| part.wear)
        .collect()
}

/// The market square's asset-tooling settlement-prop entries (#421), placed by the
/// presentation the native client shares.
#[wasm_bindgen(js_name = settlementPropEntries)]
#[must_use]
pub fn settlement_prop_entries() -> Vec<String> {
    mmorpg_presentation::settlement_props::entries()
        .into_iter()
        .map(str::to_owned)
        .collect()
}

/// The entry of each placed settlement prop, in [`settlement_prop_models`] order.
#[wasm_bindgen(js_name = settlementPropKeys)]
#[must_use]
pub fn settlement_prop_keys() -> Vec<String> {
    mmorpg_presentation::settlement_props::parts(scenery::hosted_scenery())
        .iter()
        .map(|part| part.entry.to_owned())
        .collect()
}

/// 16 column-major model floats per placed settlement prop.
#[wasm_bindgen(js_name = settlementPropModels)]
#[must_use]
pub fn settlement_prop_models() -> Vec<f32> {
    mmorpg_presentation::settlement_props::parts(scenery::hosted_scenery())
        .iter()
        .flat_map(|part| part.model)
        .collect()
}

/// A bundle drawn at fixed placements through the shared level selection: `"nature"`, the
/// `greyhaven-nature` trees and bushes (#49), `"climbing"`, the climbing kit's ladders, ropes
/// and ivy (#621), `"crag"`, the crag kit's rock modules (#668), `"gather"`, the ore veins
/// (#917), or any ordinary placed kit of the shared inventory ([`placed_kits`]): the limestone
/// blocks, river stones, meadow, Wolfrun Den, discoveries, Sunken Shrine and any kit registered
/// after them.
fn placed_kit(
    kit: &str,
) -> Result<
    (
        &'static [&'static str],
        &'static [mmorpg_presentation::lod::Placement],
    ),
    JsError,
> {
    use mmorpg_presentation::{climbing, crag, lod::Placement, nature};
    static NATURE: scenery::PerZone<Vec<Placement>> = scenery::PerZone::new();
    static CLIMBING: scenery::PerZone<Vec<Placement>> = scenery::PerZone::new();
    static CRAG: scenery::PerZone<Vec<Placement>> = scenery::PerZone::new();
    static GATHER: scenery::PerZone<Vec<Placement>> = scenery::PerZone::new();
    // Every ordinary placed kit's placements, in inventory order (#683).
    static ORDINARY: scenery::PerZone<Vec<Vec<Placement>>> = scenery::PerZone::new();
    match kit {
        // The placement lists mix bundles, so the nature kit and the inventory's mixed kits
        // take the placements of their own entries from the zone's one derivation (#1010).
        "nature" => Ok((
            &nature::ENTRIES,
            NATURE.get(|_| mixed_placements().of(&nature::ENTRIES)),
        )),
        // The ore veins (#917), in the order [`vein_slots`] indexes.
        "gather" => {
            let entries = &mmorpg_presentation::world_dressing::GATHERABLE_ENTRIES;
            Ok((
                entries,
                GATHER.get(|_| {
                    mmorpg_presentation::world_dressing::kit_placements(
                        scenery::hosted_scenery(),
                        entries,
                    )
                }),
            ))
        }
        "climbing" => Ok((
            &climbing::ENTRIES,
            CLIMBING.get(|_| climbing::placements(scenery::hosted_scenery())),
        )),
        "crag" => Ok((
            &crag::KIT_ENTRIES,
            CRAG.get(|_| crag::placements(scenery::hosted_scenery())),
        )),
        other => {
            // An ordinary placed kit from the shared inventory (#683).
            let kits = mmorpg_presentation::world_dressing::placed_kits();
            let index = kits
                .iter()
                .position(|placed| placed.name == other)
                .ok_or_else(|| JsError::new(&format!("no placed kit {other:?}")))?;
            let placements = ORDINARY.get(|_| {
                kits.iter()
                    .map(|placed| placed.placements(scenery::hosted_scenery(), mixed_placements()))
                    .collect()
            });
            Ok((&kits[index].entries, &placements[index]))
        }
    }
}

/// The ordinary placed kits (#683), in inventory order, as JSON: each kit's `name` (for
/// [`kit_entries`], [`kit_keys`], [`kit_models`] and [`KitLods`]), its `bundle` directory under
/// the site root, its `descriptor` attachment, its `label` and, for a kit that replaces a
/// static-scene stand-in (#1011), that fallback's `standIn` name as the static scene's groups
/// name it. The browser registers every one generically.
#[wasm_bindgen(js_name = placedKits)]
#[must_use]
pub fn placed_kits() -> String {
    serde_json::Value::Array(
        mmorpg_presentation::world_dressing::placed_kits()
            .iter()
            .map(|kit| {
                serde_json::json!({
                    "name": kit.name,
                    "bundle": kit.bundle,
                    "descriptor": kit.descriptor,
                    "label": kit.label(),
                    "standIn": kit.stand_in.map(static_scene::fallback_name),
                })
            })
            .collect(),
    )
    .to_string()
}

/// The presented zone's mixed placement inventory, derived once per zone and shared by the
/// nature, limestone, river and meadow kits (#1010).
fn mixed_placements() -> &'static mmorpg_presentation::world_dressing::MixedPlacements {
    static MIXED: scenery::PerZone<mmorpg_presentation::world_dressing::MixedPlacements> =
        scenery::PerZone::new();
    MIXED.get(|_| {
        #[cfg(test)]
        mixed_derivations::count();
        mmorpg_presentation::world_dressing::MixedPlacements::new(scenery::hosted_scenery())
    })
}

/// How often each zone's mixed inventory was derived, for the test that it is derived once.
#[cfg(test)]
mod mixed_derivations {
    use std::sync::atomic::{AtomicUsize, Ordering};

    static DERIVED: crate::scenery::PerZone<AtomicUsize> = crate::scenery::PerZone::new();

    pub(crate) fn count() {
        DERIVED
            .get(|_| AtomicUsize::new(0))
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn of_presented_zone() -> usize {
        DERIVED
            .peek()
            .map_or(0, |derived| derived.load(Ordering::Relaxed))
    }
}

/// The bundle entries `kit` (`"nature"` or `"climbing"`) is drawn from (#49, #621).
///
/// # Errors
///
/// For an unknown kit.
#[wasm_bindgen(js_name = kitEntries)]
pub fn kit_entries(kit: &str) -> Result<Vec<String>, JsError> {
    Ok(placed_kit(kit)?
        .0
        .iter()
        .map(|entry| (*entry).to_owned())
        .collect())
}

/// The entry of each of `kit`'s placements, in [`kit_models`] order.
///
/// # Errors
///
/// For an unknown kit.
#[wasm_bindgen(js_name = kitKeys)]
pub fn kit_keys(kit: &str) -> Result<Vec<String>, JsError> {
    Ok(placed_kit(kit)?
        .1
        .iter()
        .map(|placement| placement.entry.to_owned())
        .collect())
}

/// 16 column-major model floats per placement of `kit`.
///
/// # Errors
///
/// For an unknown kit.
#[wasm_bindgen(js_name = kitModels)]
pub fn kit_models(kit: &str) -> Result<Vec<f32>, JsError> {
    Ok(placed_kit(kit)?
        .1
        .iter()
        .flat_map(|placement| placement.model)
        .collect())
}

/// A placed kit's layered surface (#732) from its bundle's descriptor, as JSON: the mask
/// attribute, the pieces' material, the library entry and its layers, and the blend's numbers
/// at Greyhaven's weathering (`mmorpg_presentation::surface`); `undefined` for a baked kit.
///
/// # Errors
///
/// For a malformed descriptor.
#[wasm_bindgen(js_name = kitSurface)]
pub fn kit_surface(descriptor: &str) -> Result<Option<String>, JsError> {
    use mmorpg_presentation::surface::{
        CAVITY_COLOUR, GREYHAVEN_WEATHERING, LayeredSurface, MOSS_COVER, MOSS_RAMP,
    };
    let Some(surface) =
        LayeredSurface::from_descriptor(descriptor).map_err(|error| JsError::new(&error))?
    else {
        return Ok(None);
    };
    let thresholds = surface.thresholds(GREYHAVEN_WEATHERING);
    Ok(Some(
        serde_json::json!({
            "maskAttribute": surface.mask_attribute,
            "material": surface.material,
            "library": surface.library,
            "layers": surface.layers,
            "weathering": GREYHAVEN_WEATHERING,
            "moss": thresholds.moss,
            "cavity": thresholds.cavity,
            "mossRamp": MOSS_RAMP,
            "mossCover": MOSS_COVER,
            "cavityColour": CAVITY_COLOUR,
        })
        .to_string(),
    ))
}

/// A placed kit's levels of detail (#49, #621), from its bundle's descriptor, for the
/// placements [`kit_keys`] lists: the same selection and cross-fade the native client runs.
#[wasm_bindgen]
pub struct KitLods(mmorpg_presentation::lod::Lods);

#[wasm_bindgen]
impl KitLods {
    #[wasm_bindgen(constructor)]
    pub fn new(kit: &str, descriptor: &str) -> Result<Self, JsError> {
        let placements = placed_kit(kit)?.1;
        // The shared parse: every kit's levels, with the nature trees' own switch points.
        // The crag kit's descriptor lists its outcrops apart and a macro texture, which is not
        // placed (#668, #683).
        let entries = if kit == "crag" {
            mmorpg_presentation::crag::descriptor_lods(descriptor)
        } else {
            mmorpg_presentation::nature::descriptor_lods(descriptor)
        }
        .map_err(|error| JsError::new(&format!("{kit}: {error}")))?;
        Ok(Self(mmorpg_presentation::lod::Lods::new(
            entries,
            placements
                .iter()
                .map(mmorpg_presentation::lod::Placement::placed),
        )))
    }

    /// Hides placement `index`, or shows it again (#917: a vein's full or spent look).
    #[wasm_bindgen(js_name = setHidden)]
    pub fn set_hidden(&mut self, index: usize, hidden: bool) {
        self.0.set_hidden(index, hidden);
    }

    /// Each entry's variants from the finest level to the coarsest, as `entry/variant`.
    pub fn variants(&self, entry: &str) -> Vec<String> {
        self.0
            .entries()
            .iter()
            .find(|lods| lods.entry == entry)
            .map(|lods| {
                lods.levels
                    .iter()
                    .map(|(variant, _)| format!("{entry}/{variant}"))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Advances the selection for a camera at `(x, y, z)` metres with vertical field of view
    /// `fov_y` radians and column-major `clip_from_world` (16 floats, or empty), `seconds` after
    /// the previous update: per drawn level its placement, level and fade, three floats each
    /// (see `mmorpg_presentation::nature::Shown`), or `undefined` when that is unchanged since
    /// the previous update (the camera rests and nothing fades).
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        x: f64,
        y: f64,
        z: f64,
        fov_y: f64,
        clip_from_world: &[f32],
        seconds: f64,
        bias: f64,
        fade: bool,
    ) -> Option<Vec<f32>> {
        let frustum = <[f32; 16]>::try_from(clip_from_world).ok();
        self.0
            .update([x, y, z], fov_y, seconds, bias, fade, frustum);
        self.0.changed().then(|| shown_floats(self.0.shown()))
    }
}

/// Per drawn level its placement, level and fade, three floats each.
pub(crate) fn shown_floats(shown: &[mmorpg_presentation::lod::Shown]) -> Vec<f32> {
    shown
        .iter()
        .flat_map(|shown| {
            #[allow(clippy::cast_precision_loss)]
            [shown.placement as f32, shown.level as f32, shown.fade]
        })
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
    if !scenery::hosted_scenery().greyhaven() {
        return Vec::new();
    }
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
    if !scenery::hosted_scenery().greyhaven() {
        return Vec::new();
    }
    mmorpg_presentation::ambient::poses(scenery::hosted_scenery(), seconds, animate)
        .iter()
        .flat_map(|pose| {
            #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
            let tail = [pose.clip as f32, pose.time as f32];
            pose.model.into_iter().chain(tail)
        })
        .collect()
}

fn js_error(error: host::LocalZoneError) -> JsError {
    JsError::new(&error.to_string())
}

/// A practice arena build of `class` and `sex` wearing `equipment` (six item IDs, 0 empty).
fn arena_build(
    class: u8,
    sex: u8,
    equipment: &[u32],
) -> Result<mmorpg_core::EphemeralCharacterBuild, JsError> {
    let class = PlayerClass::from_code(class)
        .zip(Sex::from_code(sex))
        .map(|(class, sex)| ClassChoice { class, sex })
        .ok_or_else(|| JsError::new("the class choice is invalid"))?;
    let slots: [u32; mmorpg_core::EQUIPMENT_SLOTS] = equipment
        .try_into()
        .map_err(|_| JsError::new("an arena build has one item per equipment slot"))?;
    mmorpg_core::EphemeralCharacterBuild::new(
        class,
        slots.map(|id| (id != 0).then(|| mmorpg_core::ItemId::new(id))),
    )
    .map_err(|error| JsError::new(&error.to_string()))
}

/// Predicted own movement for the arena netcode trial (#680): `mmorpg-client-model`'s
/// reconciliation over the zone's own movement rules, for the practice arena's content or a
/// hosted match's fortress.
#[wasm_bindgen]
pub struct ArenaMovementPrediction {
    inner: mmorpg_client_model::prediction::PredictedMovement,
}

#[wasm_bindgen]
impl ArenaMovementPrediction {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<ArenaMovementPrediction, JsError> {
        Self::over(arena_presentation::trial_content(false))
    }

    /// A prediction over the fortress a hosted match runs: the same bodies, lifts and movement
    /// rules whether or not the match rewinds hits.
    #[wasm_bindgen(js_name = forHostedArena)]
    pub fn for_hosted_arena() -> Result<ArenaMovementPrediction, JsError> {
        Self::over(arena_presentation::fortress())
    }

    fn over(
        content: std::sync::Arc<mmorpg_core::ZoneContent>,
    ) -> Result<ArenaMovementPrediction, JsError> {
        Ok(Self {
            inner: mmorpg_client_model::prediction::PredictedMovement::new(
                host::ARENA_ZONE_ID,
                content,
            )
            .map_err(|error| JsError::new(&error.to_string()))?,
        })
    }

    /// The page sent `command` (plain or observed wire bytes) under `sequence`.
    pub fn sent(&mut self, sequence: u32, command: &[u8]) -> Result<(), JsError> {
        let (command, _) = mmorpg_protocol::decode_observed_command(command)
            .map_err(|error| JsError::new(&error.to_string()))?;
        self.inner
            .sent(sequence, command)
            .map_err(|error| JsError::new(&error.to_string()))
    }

    /// One local tick.
    pub fn step(&mut self) -> Result<(), JsError> {
        self.inner
            .step()
            .map_err(|error| JsError::new(&error.to_string()))
    }

    /// Every projection's tick and acknowledged sequence, in order.
    pub fn acknowledge(&mut self, tick: u64, acknowledged: u32) {
        self.inner.acknowledge(tick, acknowledged);
    }

    /// Restarts from the newest projection's own body (centre in units, velocity in whole
    /// units per tick) and replays what was sent since.
    #[allow(clippy::too_many_arguments)]
    pub fn reconcile(
        &mut self,
        tick: u64,
        x: i32,
        y: i32,
        z: i32,
        vx: i32,
        vy: i32,
        vz: i32,
        facing: u16,
    ) -> Result<(), JsError> {
        self.inner
            .reconcile(tick, [x, y, z], [vx, vy, vz], facing)
            .map_err(|error| JsError::new(&error.to_string()))
    }

    /// The predicted centre in units and facing: `[x, y, z, facing]`.
    pub fn position(&self) -> Vec<f32> {
        self.inner
            .position()
            .map_or_else(Vec::new, |([x, y, z], facing)| {
                vec![x, y, z, f32::from(facing)]
            })
    }
}

/// The walk-in cottages' roof parts among the Outpost kit placements ([`outpost_kit_keys`]):
/// per part, its placement index then its cottage's house collider, flat (#843).
#[wasm_bindgen(js_name = cottageRoofs)]
#[must_use]
pub fn cottage_roofs() -> Vec<u32> {
    mmorpg_presentation::outpost_kit::cottage_roofs(scenery::hosted_scenery())
        .into_iter()
        .flat_map(|(index, house)| [u32::try_from(index).unwrap_or(u32::MAX), house])
        .collect()
}

#[cfg(test)]
mod placed_kit_tests {
    use super::*;

    /// The nature kit and the inventory's limestone, river and meadow kits share one derivation
    /// of the presented zone's mixed inventory whatever order they are first asked in (#1010),
    /// and each keeps its former placements.
    #[test]
    fn mixed_kits_share_one_derivation_per_zone() {
        use mmorpg_presentation::{nature, river, terrain_patch::MEADOW_ENTRIES, world_dressing};
        let kits = ["river", "nature", "meadow", "limestone"];
        for kit in kits.iter().chain(kits.iter().rev()) {
            placed_kit(kit).unwrap();
        }
        assert_eq!(mixed_derivations::of_presented_zone(), 1);
        let scenery = scenery::hosted_scenery();
        let mut all = nature::placements(scenery);
        all.extend(river::placements(scenery));
        all.extend(world_dressing::ground_details(scenery, &MEADOW_ENTRIES));
        all.extend(world_dressing::ground_details(scenery, &river::ENTRIES));
        for kit in kits {
            let (entries, placements) = placed_kit(kit).unwrap();
            let before: Vec<_> = all
                .iter()
                .filter(|placement| entries.contains(&placement.entry))
                .copied()
                .collect();
            assert_eq!(placements, &before[..], "{kit}");
        }
        assert_eq!(mixed_derivations::of_presented_zone(), 1);
    }
}
