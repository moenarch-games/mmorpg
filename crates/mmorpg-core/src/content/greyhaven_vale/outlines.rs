//! Greyhaven's named areas as traced from its geography (#1063): `content/greyhaven/areas.json`,
//! made by `content/greyhaven/authoring/areas.py` from the zone's outline, terrain and design.
//! The polygons tile the zone's outline; the committed JSON is the zone's content, parsed once.
use serde::Deserialize;

use crate::{Area, AreaId};

const SOURCE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../content/greyhaven/areas.json"
));

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    version: u32,
    #[serde(rename = "note")]
    _note: serde::de::IgnoredAny,
    areas: Vec<Outline>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Outline {
    id: u32,
    name: String,
    centre: [i32; 2],
    radius: i32,
    rings: Vec<Vec<[i32; 2]>>,
}

/// The traced areas, in file order.
pub(super) fn areas() -> Result<Vec<Area>, String> {
    let source: Source = serde_json::from_str(SOURCE).map_err(|error| error.to_string())?;
    if source.version != 1 {
        return Err(format!("unknown areas version {}", source.version));
    }
    source
        .areas
        .into_iter()
        .map(|outline| {
            Area::polygon(
                AreaId::new(outline.id),
                outline.name,
                outline.rings,
                outline.centre,
                outline.radius,
            )
            .map_err(|error| error.to_string())
        })
        .collect()
}
