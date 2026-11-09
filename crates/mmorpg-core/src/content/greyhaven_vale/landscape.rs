//! Small declarative landscape inputs over Greyhaven's existing terrain generator.
//! Committed JSON is parsed once; physics and presentation use the same authored points.
use serde::Deserialize;
use std::sync::LazyLock;

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Landscape {
    pub watercourses: Watercourses,
    pub paths: Vec<Path>,
    pub basin: Basin,
    pub northern_ridge: NorthernRidge,
    pub highland_relief: HighlandRelief,
    pub forests: Vec<Forest>,
    // Blender consumes this authoring profile; runtime collision comes from its exported mesh.
    #[serde(rename = "redbrandExterior")]
    _redbrand_exterior: serde::de::IgnoredAny,
    pub redbrand_camp: Camp,
    pub brook_ground: Vec<GroundMound>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Watercourses {
    pub village: Vec<[i32; 2]>,
    pub inflow: Vec<[i32; 2]>,
    pub outlet: Vec<[i32; 2]>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Path {
    pub name: String,
    pub points: Vec<[i32; 2]>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Basin {
    pub lobes: Vec<BasinLobe>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct BasinLobe {
    pub center: [i32; 2],
    pub radii: [i32; 2],
    pub shoulder_q10: i32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct NorthernRidge {
    pub protected_approach: [[i32; 2]; 2],
    pub sections: Vec<RidgeSection>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
/// One cross-section: X, then eight [Z, height] points in cm, south to north.
pub struct RidgeSection {
    pub x: i32,
    pub profile: [[i32; 2]; 8],
}
/// The sculpted landforms on the Bandit Highlands' high ground (#528), in centimetres.
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct HighlandRelief {
    /// Rising height bands, cycled through for the terraces' stratification.
    pub strata_bands: Vec<i32>,
    /// Ridges rising above the high ground, the union of their flanks.
    pub spines: Vec<ReliefFeature>,
    /// Dry gullies cut below the high ground.
    pub gullies: Vec<ReliefFeature>,
    /// Overlooks levelled into the flanks at their centre's own height.
    pub ledges: Vec<Ledge>,
}
/// One authored landform along `[x, z, half width, crown or depth]` points.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReliefFeature {
    pub name: String,
    pub points: Vec<[i32; 4]>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ledge {
    pub name: String,
    pub center: [i32; 2],
    pub radius: i32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Forest {
    pub name: String,
    pub origin: [i32; 2],
    pub cells: [i32; 2],
    pub seed: u64,
    pub percent: u64,
    pub elevation: [i32; 2],
}
/// Placement is baked by Blender; the clearing also keeps runtime tree roots off the approach.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Camp {
    pub clearing: [[i32; 2]; 2],
    #[serde(rename = "base")]
    _base: serde::de::IgnoredAny,
    #[serde(rename = "palisades")]
    _palisades: serde::de::IgnoredAny,
    #[serde(rename = "shelters")]
    _shelters: serde::de::IgnoredAny,
    #[serde(rename = "fire")]
    _fire: serde::de::IgnoredAny,
}
/// Low terrain rises in centimetres, independent of decorative ground cover.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroundMound {
    pub center: [i32; 2],
    pub radius: i32,
    pub height: i32,
}
static LANDSCAPE: LazyLock<Landscape> = LazyLock::new(|| {
    let landscape: Landscape = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../content/greyhaven/landscape.json"
    )))
    .expect("committed Greyhaven landscape is valid");
    for points in [
        &landscape.watercourses.village,
        &landscape.watercourses.inflow,
        &landscape.watercourses.outlet,
    ] {
        assert!(
            points.len() >= 2 && points.windows(2).all(|pair| pair[0] != pair[1]),
            "authored watercourses need connected, nonzero segments"
        );
    }
    for path in &landscape.paths {
        assert!(
            !path.name.is_empty()
                && path.points.len() >= 2
                && path.points.windows(2).all(|p| p[0] != p[1]),
            "authored paths need a name and nonzero segments"
        );
    }
    assert!(
        !landscape.basin.lobes.is_empty()
            && landscape
                .basin
                .lobes
                .iter()
                .all(|lobe| lobe.radii.iter().all(|r| *r > 0) && lobe.shoulder_q10 > 1024),
        "valid basin shoulders"
    );
    assert!(
        landscape.forests.iter().all(|forest| {
            !forest.name.is_empty()
                && forest.cells.iter().all(|n| *n > 0)
                && forest.percent <= 100
                && forest.elevation[0] < forest.elevation[1]
        }),
        "valid authored forest patches"
    );
    assert!(
        landscape
            .brook_ground
            .iter()
            .all(|m| m.radius >= 400 && (0..=100).contains(&m.height)),
        "low, supported brook ground profiles"
    );
    let sections = &landscape.northern_ridge.sections;
    assert!(
        sections.len() >= 2 && sections.windows(2).all(|pair| pair[0].x < pair[1].x),
        "ridge sections are ordered west to east"
    );
    assert!(
        sections.iter().all(
            |section| section.profile.windows(2).all(|p| p[0][0] > p[1][0])
                && section.profile[0][1] == 0
                && section.profile[7][1] == super::highlands::PLATEAU_Y
        ),
        "ridge profiles join the valley and plateau"
    );
    let relief = &landscape.highland_relief;
    assert!(
        !relief.strata_bands.is_empty() && relief.strata_bands.iter().all(|band| *band >= 400),
        "strata bands span at least 4 m"
    );
    assert!(
        relief.spines.iter().chain(&relief.gullies).all(|feature| {
            !feature.name.is_empty()
                && feature.points.len() >= 2
                && feature.points.windows(2).all(|p| p[0][..2] != p[1][..2])
                // No flank is steeper than about 20° before its terraces: walkable after them.
                && feature.points.iter().all(|p| p[3] > 0 && p[3] * 100 <= p[2] * 36)
        }),
        "relief features are connected, positive and leave walkable flanks"
    );
    assert!(
        relief
            .ledges
            .iter()
            .all(|ledge| !ledge.name.is_empty() && ledge.radius >= 400),
        "ledges are named and at least 8 m across"
    );
    landscape
});
pub fn data() -> &'static Landscape {
    &LANDSCAPE
}

/// Authored low relief, tapered away from the existing movement corridors.
pub(super) fn brook_relief(x: i32, z: i32) -> i32 {
    let rise = data()
        .brook_ground
        .iter()
        .map(|m| {
            let distance = i64::from(x - m.center[0]).pow(2) + i64::from(z - m.center[1]).pow(2);
            let radius = i64::from(m.radius).pow(2);
            let weight = ((radius - distance).max(0) * 1024 / radius).min(1024);
            (i64::from(m.height) * weight * weight / (1024 * 1024)) as i32
        })
        .max()
        .unwrap_or(0);
    if rise == 0 {
        return 0;
    }
    let mut clear = 0;
    for margin in [0, 50, 100, 150, 200, 250] {
        let probe = crate::StaticCollider {
            id: 0,
            position: [x, 0, z],
            half_extents: [margin, 1, margin],
        };
        clear += i32::from(super::clear_of_roads(&probe));
    }
    rise * clear / 6
}

#[cfg(test)]
mod relief_tests {
    use super::*;
    #[test]
    fn brook_rises_leave_the_bridge_and_main_walkway_flat() {
        assert!(brook_relief(-1100, 4400) > 20);
        assert_eq!(brook_relief(0, 3200), 0);
        assert_eq!(brook_relief(0, 4400), 0);
        assert_eq!(brook_relief(-3000, 4400), 0);
    }
}
