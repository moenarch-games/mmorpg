//! Client-only presentation of Battle Royale's flat arena; no match simulation lives here.
//! The mode host supplies every physical/combat projection. This empty template catalog
//! supplies shared character/ability/item descriptions and no Greyhaven props or NPCs.
use std::sync::{Arc, OnceLock};

use mmorpg_core::{
    ABILITY_CATALOG_REVISION, ClassChoice, EphemeralCharacterBuild, MAX_PLAYER_LEVEL, PlayerClass,
    Sex, ZoneContent, ZoneDefinition,
};
use mmorpg_presentation::StaticScene;
use mmorpg_scenery::Scenery;
use wasm_bindgen::prelude::{JsError, wasm_bindgen};

const ARENA_CONTENT_REVISION: u64 = 2;
type SceneBuffers = (String, Vec<f32>, Vec<u32>, Vec<f32>);

pub(crate) fn content() -> Arc<ZoneContent> {
    static CONTENT: OnceLock<Arc<ZoneContent>> = OnceLock::new();
    Arc::clone(CONTENT.get_or_init(|| {
        let definition = ZoneDefinition::new(ARENA_CONTENT_REVISION, [0, -1, 0], Vec::new())
            .expect("empty client presentation definition is valid");
        Arc::new(
            ZoneContent::from_definition(definition)
                .with_creature_abilities(ABILITY_CATALOG_REVISION, Vec::new())
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
