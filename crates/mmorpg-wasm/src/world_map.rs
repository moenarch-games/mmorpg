//! The browser's thin wrappers over `mmorpg_client_model::world_map`: the map both
//! clients open with M, where a zone position lies on it and what lies under a point,
//! so the browser and the native client agree on all three.
use mmorpg_client_model::world_map as model;
use serde::{Deserialize, Serialize};
use tsify::Tsify;
use wasm_bindgen::prelude::{JsError, JsValue, wasm_bindgen};

/// The world map's GeoJSON layers in WGS84 (`content/world/export/world-map.json`).
#[wasm_bindgen(js_name = worldMapBundle)]
#[must_use]
pub fn world_map_bundle() -> String {
    model::BUNDLE.to_owned()
}

/// `[longitude, latitude]` of a zone position in metres (`x` east, `z` south).
#[wasm_bindgen(js_name = worldMapZoneToMap)]
#[must_use]
pub fn world_map_zone_to_map(x: f64, z: f64) -> Vec<f64> {
    model::zone_to_map(x, z).to_vec()
}

/// A facing in radians (0 faces +z, south; a quarter turn +x, east) as a compass bearing.
#[wasm_bindgen(js_name = worldMapBearing)]
#[must_use]
pub fn world_map_bearing(yaw: f64) -> f64 {
    model::bearing(yaw)
}

/// What the map says about a point, for the hover label.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "camelCase")]
#[tsify(missing_as_null)]
pub struct WorldMapRegion {
    pub id: String,
    pub name: Option<String>,
    pub kingdom: String,
    pub biome: String,
    pub zone: Option<String>,
}

/// The region under `[longitude, latitude]`, or `null` over the sea.
#[wasm_bindgen(js_name = worldMapRegionAt, unchecked_return_type = "WorldMapRegion | null")]
pub fn world_map_region_at(longitude: f64, latitude: f64) -> Result<JsValue, JsError> {
    let found = model::world_map()
        .region_at([longitude, latitude])
        .map(region);
    crate::client_model::to_js(&found)
}

/// The region with this id (the hovered map feature), or `null`.
#[wasm_bindgen(js_name = worldMapRegion, unchecked_return_type = "WorldMapRegion | null")]
pub fn world_map_region(id: &str) -> Result<JsValue, JsError> {
    crate::client_model::to_js(&model::world_map().region(id).map(region))
}

fn region(r: model::RegionInfo) -> WorldMapRegion {
    WorldMapRegion {
        id: r.id,
        name: r.name,
        kingdom: r.kingdom,
        biome: r.biome,
        zone: r.zone,
    }
}
