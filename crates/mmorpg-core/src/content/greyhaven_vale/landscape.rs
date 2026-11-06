//! Small declarative landscape inputs over Greyhaven's existing terrain generator.
//! Committed JSON is parsed once; physics and presentation use the same authored points.
use serde::Deserialize;
use std::sync::LazyLock;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Landscape {
    pub watercourses: Watercourses,
    pub paths: Vec<Path>,
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
    landscape
});
pub fn data() -> &'static Landscape {
    &LANDSCAPE
}
