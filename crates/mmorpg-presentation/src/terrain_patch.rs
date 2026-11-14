//! Southmeadow's saved asset-tooling recipe masks (#782). Heights remain owned by core.
use mmorpg_scenery::Scenery;
use serde::Deserialize;
use std::sync::LazyLock;

/// Existing grass and flower art, loaded by the clients' ordinary placed-kit loader.
pub const MEADOW_ENTRIES: [&str; 8] = [
    "greyhaven-meadow.kit.grass-short",
    "greyhaven-meadow.kit.grass-tall",
    "greyhaven-meadow.kit.grass-clump",
    "greyhaven-meadow.kit.grass-dry",
    "greyhaven-meadow.kit.flowers-yellow",
    "greyhaven-meadow.kit.flowers-white",
    "greyhaven-meadow.kit.flowers-red",
    "greyhaven-meadow.kit.flowers-mixed",
];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Masks {
    version: u32,
    producer_revision: String,
    origin: [i32; 2],
    step: i32,
    columns: usize,
    rows: usize,
    slope: Vec<u8>,
    exclusion: Vec<u8>,
    meadow: Vec<u8>,
    gravel: Vec<u8>,
}
static MASKS: LazyLock<Masks> = LazyLock::new(|| {
    let masks: Masks = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../content/greyhaven/terrain-patches/southmeadow/masks.json"
    )))
    .expect("saved terrain recipe masks");
    assert_eq!(masks.version, 1);
    assert!(!masks.producer_revision.is_empty());
    assert!(masks.step > 0 && masks.columns > 1 && masks.rows > 1);
    for field in [&masks.slope, &masks.exclusion, &masks.meadow, &masks.gravel] {
        assert_eq!(field.len(), masks.columns * masks.rows);
    }
    masks
});

/// Recipe paint and Q8 slope drive gravel and exposed rock on the existing ground mesh.
/// Linear scalar bilinear sampling; outside the bounded patch the old weights are unchanged.
#[must_use]
pub fn weights(s: &Scenery, x: i32, z: i32) -> [f64; 2] {
    if !s.greyhaven() {
        return [0.; 2];
    }
    let m = &*MASKS;
    let [u, v] = [x - m.origin[0], z - m.origin[1]].map(|v| f64::from(v) / f64::from(m.step));
    if u < 0.
        || v < 0.
        || u >= (m.columns - 1) as f64
        || v >= (m.rows - 1) as f64
        || s.road_distance([x, z]) <= 250
        || s.farmland().iter().any(|f| f.distance([x, z]) <= 200)
    {
        return [0.; 2];
    }
    let sample = |field: &[u8]| {
        let (c, r) = (u.floor() as usize, v.floor() as usize);
        let (a, b) = (u.fract(), v.fract());
        let at = |c, r| f64::from(field[r * m.columns + c]) / 255.;
        (at(c, r) * (1. - a) + at(c + 1, r) * a) * (1. - b)
            + (at(c, r + 1) * (1. - a) + at(c + 1, r + 1) * a) * b
    };
    let coverage = 1. - sample(&m.exclusion);
    let slope = sample(&m.slope);
    let paint = sample(&m.gravel);
    let meadow = sample(&m.meadow);
    [
        coverage * paint * (0.82 + 0.18 * slope) * (1. - 0.25 * meadow),
        coverage * paint * (slope * 3.).clamp(0., 1.),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_recipe_drives_existing_layers_and_real_ground_dressing() {
        let s = mmorpg_scenery::greyhaven_vale_scenery();
        assert_eq!(weights(&s, 7900, 18000), [0.; 2]);
        assert!(weights(&s, 10400, 17200)[0] > 0.5);
        let grid = s.terrain_grid(crate::TERRAIN_STEP_UNITS);
        for entries in [&MEADOW_ENTRIES[..], &crate::river::ENTRIES[..]] {
            let placements = crate::world_dressing::ground_details(&s, entries);
            assert!(!placements.is_empty());
            for p in placements {
                assert!(entries.contains(&p.entry));
                assert_eq!(p.placed().max_distance, Some(60.));
                let x = f64::from(p.model[12]);
                let z = f64::from(p.model[14]);
                assert!(
                    (f64::from(p.model[13]) - crate::terrain::surface(&grid, x, z)).abs() < 0.0001
                );
                assert!((80. ..112.).contains(&x) && (164. ..196.).contains(&z));
                assert!(
                    s.road_distance([(x * 100.).round() as i32, (z * 100.).round() as i32]) > 250
                );
            }
        }
        let detail = crate::world_dressing::placed(&s)
            .iter()
            .filter(|p| p.ground_detail);
        for p in detail {
            assert!(p.id.starts_with("southmeadow:"));
            assert!(p.collider.is_none() && p.fallback_collider.is_none());
        }
    }
}
