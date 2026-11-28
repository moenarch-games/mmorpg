//! The Greyhaven Vale region beyond its hand-authored parts (#810): the outline the boundary
//! wall follows and the base terrain inside it, exported once from the continent atlas
//! (`atlas.py zone-export greyhaven`). The committed JSON is the zone's content, parsed once.
use serde::Deserialize;
use std::sync::LazyLock;

use crate::Terrain;

const SOURCE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../content/greyhaven/region.json"
));

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    version: u32,
    #[serde(rename = "note")]
    _note: serde::de::IgnoredAny,
    outline: Vec<[i32; 2]>,
    terrain: Grid,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Grid {
    origin: [i32; 2],
    step: i32,
    columns: u16,
    rows: u16,
    /// Rows along +Z, each a row of samples along +X.
    heights: Vec<Vec<i32>>,
}

/// The parsed region: its outline, the outline's edges bucketed by Z band for point tests, and
/// the base terrain.
pub struct Region {
    pub outline: Vec<[i32; 2]>,
    bands: Vec<Vec<usize>>,
    band_origin: i32,
    pub terrain: Terrain,
}

/// Z extent of one band of outline edges.
const BAND_UNITS: i32 = 2_000;

fn parse(source: &str) -> Result<Region, String> {
    let source: Source = serde_json::from_str(source).map_err(|error| error.to_string())?;
    if source.version != 1 {
        return Err(format!("unknown region version {}", source.version));
    }
    let outline = source.outline;
    if outline.len() < 3 {
        return Err("the region outline needs three points".to_owned());
    }
    let grid = source.terrain;
    if grid.heights.len() != usize::from(grid.rows)
        || grid
            .heights
            .iter()
            .any(|row| row.len() != usize::from(grid.columns))
    {
        return Err("region terrain rows do not match its size".to_owned());
    }
    let terrain = Terrain {
        origin: grid.origin,
        step: grid.step,
        columns: grid.columns,
        rows: grid.rows,
        heights: grid.heights.into_iter().flatten().collect(),
    };
    terrain.validate().map_err(|error| error.to_string())?;
    let band_origin = outline.iter().map(|point| point[1]).min().unwrap_or(0);
    let band = |z: i32| usize::try_from((z - band_origin) / BAND_UNITS).unwrap_or(0);
    let mut bands = vec![Vec::new(); band(outline.iter().map(|p| p[1]).max().unwrap_or(0)) + 1];
    for index in 0..outline.len() {
        let (a, b) = (outline[index], outline[(index + 1) % outline.len()]);
        for edges in &mut bands[band(a[1].min(b[1]))..=band(a[1].max(b[1]))] {
            edges.push(index);
        }
    }
    Ok(Region {
        outline,
        bands,
        band_origin,
        terrain,
    })
}

static REGION: LazyLock<Region> =
    LazyLock::new(|| parse(SOURCE).expect("committed Greyhaven region is valid"));

/// The committed region.
#[must_use]
pub fn data() -> &'static Region {
    &REGION
}

impl Region {
    /// Whether `(x, z)` lies inside the outline (even–odd rule; a point on a horizontal edge
    /// counts on its lower side, as half-open rectangles do).
    #[must_use]
    pub fn contains(&self, x: i32, z: i32) -> bool {
        if z < self.band_origin {
            return false;
        }
        let Ok(band) = usize::try_from((z - self.band_origin) / BAND_UNITS) else {
            return false;
        };
        let Some(edges) = self.bands.get(band) else {
            return false;
        };
        let mut inside = false;
        for &index in edges {
            let a = self.outline[index];
            let b = self.outline[(index + 1) % self.outline.len()];
            inside ^= crate::areas::crosses(a, b, x, z);
        }
        inside
    }

    /// The base terrain's height at `(x, z)` in units: bilinear between the atlas samples.
    #[must_use]
    pub fn base_height(&self, x: i32, z: i32) -> i32 {
        self.terrain.height_at(x, z)
    }
}

#[cfg(test)]
mod tests {
    use super::{data, parse};
    use crate::greyhaven_vale::{AUTHORED, PLAYABLE_BOUNDS};

    #[test]
    fn the_outline_encloses_the_authored_parts_within_the_playable_bounds() {
        let region = data();
        for point in &region.outline {
            assert!(
                PLAYABLE_BOUNDS.min[0] <= point[0] && point[0] <= PLAYABLE_BOUNDS.max[0],
                "{point:?}"
            );
            assert!(
                PLAYABLE_BOUNDS.min[1] <= point[1] && point[1] <= PLAYABLE_BOUNDS.max[1],
                "{point:?}"
            );
        }
        for bounds in AUTHORED {
            for x in (bounds.min[0]..bounds.max[0]).step_by(400) {
                for z in (bounds.min[1]..bounds.max[1]).step_by(400) {
                    assert!(region.contains(x, z), "({x}, {z})");
                }
            }
        }
        assert!(!region.contains(PLAYABLE_BOUNDS.min[0], PLAYABLE_BOUNDS.min[1]));
        assert!(!region.contains(PLAYABLE_BOUNDS.max[0] + 1, 0));
    }

    #[test]
    fn rejects_a_region_whose_rows_do_not_match() {
        let bad = r#"{"version":1,"note":"","outline":[[0,0],[10,0],[0,10]],
            "terrain":{"origin":[0,0],"step":10,"columns":2,"rows":2,"heights":[[0,0]]}}"#;
        assert!(parse(bad).is_err());
    }
}
