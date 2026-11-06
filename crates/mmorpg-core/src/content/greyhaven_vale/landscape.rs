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
    pub forests: Vec<Forest>,
    // Blender consumes this authoring profile; runtime collision comes from its exported mesh.
    #[serde(rename = "redbrandExterior")]
    _redbrand_exterior: serde::de::IgnoredAny,
    pub redbrand_camp: Camp,
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
    landscape
});
pub fn data() -> &'static Landscape {
    &LANDSCAPE
}
