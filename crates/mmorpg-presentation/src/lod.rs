//! Levels of detail from an asset-tooling descriptor's `lodSelection` metadata (the
//! greyhaven-nature format, asset-tooling #449, which the fortress kit #445 shares): per asset a
//! bounding sphere and levels finest first, each used while the sphere's projected size
//! (radius over distance × tan(fovY / 2), the diameter's fraction of the viewport height) is at
//! least its `minScreenSize`.
use serde_json::Value;

/// The one selection metric the descriptors use.
pub const METRIC: &str = "projected-bounding-sphere-height-fraction";

/// One entry's levels of detail, from the descriptor's `lods`.
#[derive(Clone, Debug, PartialEq)]
pub struct EntryLods {
    pub entry: String,
    /// Bounding-sphere centre in the entry's frame and its radius, metres.
    pub centre: [f64; 3],
    pub radius: f64,
    /// `(variant, minScreenSize)` from the finest level to the coarsest.
    pub levels: Vec<(String, f64)>,
}

impl EntryLods {
    /// The projected size of the sphere seen from `distance` metres with vertical field of view
    /// `fov_y` (radians).
    #[must_use]
    pub fn screen_size(&self, distance: f64, fov_y: f64) -> f64 {
        self.radius / (distance.max(1e-3) * (fov_y / 2.).tan().max(1e-6))
    }

    /// The level to draw at `distance` metres: the first whose `minScreenSize` the projected
    /// size reaches.
    #[must_use]
    pub fn level(&self, distance: f64, fov_y: f64) -> usize {
        let size = self.screen_size(distance, fov_y);
        self.levels
            .iter()
            .position(|(_, min)| *min <= size)
            .unwrap_or(self.levels.len().saturating_sub(1))
    }

    /// The camera distance (metres, to the sphere centre) from which each level is drawn, for a
    /// distance-switched renderer (three.js `LOD`): 0 for the finest level.
    #[must_use]
    pub fn distances(&self, fov_y: f64) -> Vec<f64> {
        let tan = (fov_y / 2.).tan().max(1e-6);
        let mut previous = 0.;
        let mut out = Vec::with_capacity(self.levels.len());
        for (index, _) in self.levels.iter().enumerate() {
            // A level starts where the one before it ends.
            let start = match index.checked_sub(1).map(|before| self.levels[before].1) {
                Some(min) if min > 0. => self.radius / (min * tan),
                Some(_) => f64::INFINITY,
                None => 0.,
            };
            previous = start.max(previous);
            out.push(previous);
        }
        out
    }
}

/// Every asset's levels in a descriptor with `lodSelection`.
///
/// # Errors
///
/// For an unknown metric or an asset without a key, bounding sphere or levels.
pub fn descriptor_lods(descriptor: &Value) -> Result<Vec<EntryLods>, String> {
    let metric = descriptor["lodSelection"]["metric"].as_str();
    if metric != Some(METRIC) {
        return Err(format!("unknown LOD metric {metric:?}"));
    }
    let assets = descriptor["assets"]
        .as_array()
        .ok_or("the descriptor lists no assets")?;
    assets
        .iter()
        .map(|asset| {
            let entry = asset["key"].as_str().ok_or("an asset without key")?;
            let lods = &asset["lods"];
            let sphere = &lods["boundingSphere"];
            let centre = sphere["center"]
                .as_array()
                .filter(|c| c.len() == 3)
                .ok_or_else(|| format!("{entry} has no bounding sphere"))?;
            let centre = std::array::from_fn(|axis| centre[axis].as_f64().unwrap_or(0.));
            let radius = sphere["radius"]
                .as_f64()
                .ok_or_else(|| format!("{entry} has no bounding radius"))?;
            let levels = lods["levels"]
                .as_array()
                .ok_or_else(|| format!("{entry} has no levels"))?
                .iter()
                .map(|level| {
                    Some((
                        level["variant"].as_str()?.to_owned(),
                        level["minScreenSize"].as_f64()?,
                    ))
                })
                .collect::<Option<Vec<_>>>()
                .filter(|levels| !levels.is_empty())
                .ok_or_else(|| format!("{entry} has malformed levels"))?;
            Ok(EntryLods {
                entry: entry.to_owned(),
                centre,
                radius,
                levels,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wall() -> EntryLods {
        EntryLods {
            entry: "wall".into(),
            centre: [0., 4., 0.],
            radius: 40.,
            levels: vec![
                ("default".into(), 0.35),
                ("lod1".into(), 0.12),
                ("lod2".into(), 0.),
            ],
        }
    }

    #[test]
    fn levels_switch_where_the_projected_size_crosses_each_threshold() {
        let (wall, fov) = (wall(), 60_f64.to_radians());
        let distances = wall.distances(fov);
        assert_eq!(distances[0], 0.);
        // The reference distances asset-tooling states for a 40 m sphere at 60°.
        assert!((distances[1] - 40. / (0.35 * (fov / 2.).tan())).abs() < 1e-9);
        assert!((distances[2] - 40. / (0.12 * (fov / 2.).tan())).abs() < 1e-9);
        for (level, distance) in distances.iter().enumerate() {
            assert_eq!(wall.level(distance + 0.01, fov), level);
        }
        assert_eq!(wall.level(1., fov), 0);
        assert_eq!(wall.level(1e9, fov), 2);
    }

    #[test]
    fn the_descriptor_parses_its_lod_levels() {
        let json: Value = serde_json::from_str(
            r#"{"lodSelection":{"metric":"projected-bounding-sphere-height-fraction"},
          "assets":[{"key":"k","lods":{"boundingSphere":{"center":[0,1,0],"radius":2},
          "levels":[{"variant":"default","minScreenSize":0.3},{"variant":"lod1","minScreenSize":0}]}}]}"#,
        )
        .unwrap();
        let lods = descriptor_lods(&json).unwrap();
        assert_eq!(
            lods[0].levels,
            [("default".into(), 0.3), ("lod1".into(), 0.)]
        );
        assert_eq!(lods[0].centre, [0., 1., 0.]);
        assert!(descriptor_lods(&serde_json::json!({"lodSelection": {"metric": "x"}})).is_err());
    }
}
