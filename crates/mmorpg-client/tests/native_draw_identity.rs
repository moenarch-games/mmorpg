#[path = "support/draw_identity.rs"]
mod identity;
use identity::{canonical_draw_id, unique_index};
const BROWSER: &str =
    "instances:80m-1_0:near:scenery:greyhaven:9:0123456789abcdef:instance-geometry:3:12";
const NATIVE: &str =
    "instances:80m-1_0:near:native-scenery:9:fedcba9876543210:instance-geometry:3:12";
#[test]
fn only_exporter_cache_envelope_is_normalized() {
    assert_eq!(canonical_draw_id(BROWSER), canonical_draw_id(NATIVE));
    assert_eq!(
        canonical_draw_id(BROWSER),
        "instances:80m-1_0:near:instance-geometry:3:12"
    );
    for changed in [
        NATIVE.replace("80m-1_0", "80m0_0"),
        NATIVE.replace(":near:", ":far:"),
        NATIVE.replace("geometry:3:", "geometry:4:"),
        NATIVE.replace(":3:12", ":3:13"),
    ] {
        assert_ne!(canonical_draw_id(BROWSER), canonical_draw_id(&changed));
    }
}
#[test]
fn unrelated_and_malformed_ids_are_preserved() {
    for id in [
        "unit-creature-100-head",
        "static-all-terrain:always:ffffff",
        "campfire-flame-1",
        "asset-tooling:0123456789abcdef:body",
        "instances:80m-1_0:near:scenery:greyhaven:9:bad:instance-geometry:3:12",
        "instances:80m-1_0:near:native-scenery:9:fedcba9876543210:instance-geometry:3:12:13",
        "instances:80m-1_0:unknown:native-scenery:9:fedcba9876543210:instance-geometry:3:12",
    ] {
        assert_eq!(canonical_draw_id(id), id);
    }
}
#[test]
#[should_panic(expected = "duplicate actual canonical draw ID")]
fn actual_canonical_collisions_fail_closed() {
    unique_index([(BROWSER, 1), (NATIVE, 2)], "actual");
}
#[test]
#[should_panic(expected = "duplicate expected canonical draw ID")]
fn expected_canonical_collisions_fail_closed() {
    unique_index([(BROWSER, 1), (NATIVE, 2)], "expected");
}
