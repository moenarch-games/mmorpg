//! Client-only Battle Royale arena: its flat presentation, and the practice arena (#550) the
//! page hosts locally. A hosted match supplies every physical/combat projection itself; the
//! practice arena is the same flat ground with stationary training targets, simulated by the
//! shared `mmorpg-core` rules in [`crate::host::LocalZoneHost::practice_arena`]. Both share this
//! content's character, ability, item and target descriptions; no Greyhaven props or NPCs.
use std::sync::{Arc, OnceLock};

use mmorpg_core::{
    CharacterPersistence, ClassChoice, CreatureBehaviour, CreatureFamily, CreatureId,
    CreatureSpawn, CreatureTemplate, CreatureTemplateId, EphemeralCharacterBuild, MAX_PLAYER_LEVEL,
    PlayerClass, PropKind, Sex, SpawnGrid, StaticCollider, ZoneAreas, ZoneContent, ZoneDefinition,
};
use mmorpg_presentation::StaticScene;
use mmorpg_scenery::Scenery;
use wasm_bindgen::prelude::{JsError, wasm_bindgen};

/// The content revision the pinned battle-royale host stamps on its projections
/// (`web/src/world/battle-royale/protocol.ts`); a cross-repository pin, updated on re-pin.
const ARENA_CONTENT_REVISION: u64 = 2;
use crate::static_scene::SceneBuffers;

/// The practice arena's training target (#550): a breakable prop at the maximum player level,
/// so it never moves or fights and keeps no threat, but takes real damage and breaks.
const TRAINING_TARGET: CreatureTemplateId = CreatureTemplateId::new(1);
/// Health of a training target: about twenty max-level weapon hits, a few ability rotations.
const TRAINING_TARGET_HEALTH: u32 = 300;
/// Feet XZ of the training targets: three 8–9 m ahead of spawn slot 0 (which faces +Z) for melee,
/// two 20 m ahead for ranged abilities.
const TRAINING_TARGETS: [[i32; 2]; 5] = [
    [-300, 800],
    [0, 900],
    [300, 800],
    [-600, 2_000],
    [600, 2_000],
];
/// Spawn slots run along +X in two rows, so nothing stands in front of slot 0 but the targets.
const PRACTICE_SPAWN_GRID: SpawnGrid = SpawnGrid {
    origin: [0, 0],
    columns: 256,
    spacing: 200,
};

pub(crate) fn content() -> Arc<ZoneContent> {
    static CONTENT: OnceLock<Arc<ZoneContent>> = OnceLock::new();
    Arc::clone(CONTENT.get_or_init(|| {
        let definition = ZoneDefinition::with_spawn_grid(
            mmorpg_core::STANDARD_GRAVITY,
            PRACTICE_SPAWN_GRID,
            // The flat ground under the spawn grid: the zone makes it a floor half-space.
            vec![StaticCollider {
                id: 1,
                position: [0, -50, 0],
                half_extents: [10_000, 50, 10_000],
            }],
        )
        .expect("flat arena definition is valid");
        let target = CreatureTemplate {
            id: TRAINING_TARGET,
            name: "Training Target".to_owned(),
            family: CreatureFamily::Prop,
            behaviour: CreatureBehaviour::Prop(PropKind::Barrel),
            min_level: MAX_PLAYER_LEVEL,
            max_level: MAX_PLAYER_LEVEL,
            elite: false,
            health: TRAINING_TARGET_HEALTH,
            health_per_level: 0,
            damage: [1, 1],
            damage_per_level: 0,
            swing_ticks: mmorpg_core::ticks_from_ms(2_000),
            respawn_ticks: mmorpg_core::unit::CORPSE_TICKS,
            half_extents: [30, 45, 30],
        };
        let spawns = (1..)
            .zip(TRAINING_TARGETS)
            .map(|(id, position)| CreatureSpawn {
                id: CreatureId::new(id),
                template: TRAINING_TARGET,
                position,
                facing: 32_768,
                wander_radius: 0,
                levels: None,
            })
            .collect();
        let content = ZoneContent::new(
            definition,
            ZoneAreas::default(),
            vec![target],
            spawns,
            Vec::new(),
            [0, 0],
        )
        .expect("practice arena content is valid");
        Arc::new(
            content
                .with_character_persistence(CharacterPersistence::Ephemeral)
                .with_creature_abilities(Vec::new())
                .expect("shared ability catalog is valid"),
        )
    }))
}
pub(crate) fn shared_scenery() -> Arc<Scenery> {
    static SCENERY: OnceLock<Arc<Scenery>> = OnceLock::new();
    Arc::clone(SCENERY.get_or_init(|| Arc::new(Scenery::flat(ARENA_CONTENT_REVISION))))
}
fn buffers() -> &'static SceneBuffers {
    static BUFFERS: OnceLock<SceneBuffers> = OnceLock::new();
    BUFFERS.get_or_init(|| crate::static_scene::encode(&StaticScene::new(&shared_scenery())))
}

/// Reuses the same Rust scenery meshes and character catalog as the ordinary client.
#[derive(Default)]
#[wasm_bindgen]
pub struct ArenaPresentation;

#[wasm_bindgen]
impl ArenaPresentation {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self
    }
    pub fn scenery(&self) -> String {
        let export =
            crate::scenery::export(&shared_scenery(), &[], &mmorpg_core::ZoneAreas::default());
        serde_json::to_string(&export).expect("scenery export is serializable data")
    }
    pub fn catalog(&self) -> String {
        serde_json::to_string(&crate::catalog::catalog(&content()))
            .expect("character catalog is serializable data")
    }
    #[wasm_bindgen(js_name = maximumLevel)]
    pub fn maximum_level(&self) -> u8 {
        MAX_PLAYER_LEVEL
    }
    /// Default choices come from the actual admission API; the UI can choose other legal slots.
    #[wasm_bindgen(js_name = standardEquipment)]
    pub fn standard_equipment(&self, class: u8) -> Result<Vec<u16>, JsError> {
        let class = PlayerClass::from_code(class).ok_or_else(|| JsError::new("unknown class"))?;
        let build = EphemeralCharacterBuild::standard(ClassChoice {
            class,
            sex: Sex::Female,
        })
        .map_err(|error| JsError::new(&error.to_string()))?;
        Ok(build
            .equipment()
            .slots()
            .iter()
            .map(|slot| slot.map_or(0, |id| id.get()))
            .collect())
    }
    #[wasm_bindgen(js_name = staticScene)]
    pub fn static_scene(&self) -> String {
        buffers().0.clone()
    }
    #[wasm_bindgen(js_name = staticSceneVertices)]
    pub fn static_scene_vertices(&self) -> Vec<f32> {
        buffers().1.clone()
    }
    #[wasm_bindgen(js_name = staticSceneIndices)]
    pub fn static_scene_indices(&self) -> Vec<u32> {
        buffers().2.clone()
    }
    #[wasm_bindgen(js_name = staticSceneModels)]
    pub fn static_scene_models(&self) -> Vec<f32> {
        buffers().3.clone()
    }
}
