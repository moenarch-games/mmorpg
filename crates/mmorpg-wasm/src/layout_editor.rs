//! Separate authoring surface; never mutates an active zone.
use mmorpg_core::greyhaven_vale;
use mmorpg_presentation::StaticScene;
use serde_json::json;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(js_name = layoutSource)]
pub fn layout_source() -> String {
    greyhaven_vale::layout::SOURCE.to_owned()
}

#[wasm_bindgen]
pub struct LayoutPreview {
    scenery: String,
    objects: String,
    scene: StaticScene,
    buffers: (String, Vec<f32>, Vec<u32>, Vec<f32>),
}

#[wasm_bindgen]
impl LayoutPreview {
    #[wasm_bindgen(constructor)]
    pub fn new(source: &str) -> Result<LayoutPreview, JsError> {
        let definition = greyhaven_vale::layout::compile(source)
            .map_err(|error| JsError::new(&error.to_string()))?;
        // Includes creature/NPC/graveyard, quest, door and routine validation.
        greyhaven_vale::content_with_definition(definition.clone())
            .map_err(|error| JsError::new(&error.to_string()))?;
        let scenery = mmorpg_scenery::layout::compose(source, &definition)
            .map_err(|error| JsError::new(&error))?;
        let mut objects = Vec::new();
        for collider in definition.colliders().iter().filter(|collider| {
            (103..=106).contains(&collider.id) || (60_000..65_000).contains(&collider.id)
        }) {
            objects.push(json!({"id": collider.id.to_string(), "kind": "house", "position": [collider.position[0], collider.position[2]], "half": [collider.half_extents[0], collider.half_extents[2]], "yaw": 0, "authority": "collision"}));
        }
        for prop in
            mmorpg_scenery::layout::decorations(source).map_err(|error| JsError::new(&error))?
        {
            objects.push(json!({"id": prop.id, "kind": prop.kind, "position": prop.position, "half": [30, 30], "yaw": prop.yaw, "authority": "decoration"}));
        }
        let export =
            crate::scenery::export(&scenery, definition.colliders(), greyhaven_vale::areas());
        let scene = StaticScene::new(&scenery);
        let buffers = crate::static_scene::encode(&scene);
        Ok(Self {
            scenery: serde_json::to_string(&export)
                .map_err(|error| JsError::new(&error.to_string()))?,
            objects: serde_json::to_string(&objects)
                .map_err(|error| JsError::new(&error.to_string()))?,
            scene,
            buffers,
        })
    }

    pub fn objects(&self) -> String {
        self.objects.clone()
    }
    pub fn scenery(&self) -> String {
        self.scenery.clone()
    }
    #[wasm_bindgen(js_name = staticScene)]
    pub fn static_scene(&self) -> String {
        self.buffers.0.clone()
    }
    #[wasm_bindgen(js_name = staticSceneVertices)]
    pub fn static_scene_vertices(&self) -> Vec<f32> {
        self.buffers.1.clone()
    }
    #[wasm_bindgen(js_name = staticSceneIndices)]
    pub fn static_scene_indices(&self) -> Vec<u32> {
        self.buffers.2.clone()
    }
    #[wasm_bindgen(js_name = staticSceneModels)]
    pub fn static_scene_models(&self) -> Vec<f32> {
        self.buffers.3.clone()
    }
    #[wasm_bindgen(js_name = staticSceneMotion)]
    pub fn static_scene_motion(&self, seconds: f64, animate: bool) -> Vec<f32> {
        self.scene
            .motion(seconds, animate)
            .into_iter()
            .flatten()
            .collect()
    }
    #[wasm_bindgen(js_name = staticSceneShown)]
    pub fn static_scene_shown(&self, _viewer: &[f64]) -> Vec<u8> {
        self.scene.shown(None).into_iter().map(u8::from).collect()
    }
    #[wasm_bindgen(js_name = surfaceY)]
    pub fn surface_y(&self, x: f64, z: f64) -> f64 {
        self.scene.surface_y(x, z)
    }
}
