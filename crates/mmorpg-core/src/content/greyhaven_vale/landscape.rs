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
