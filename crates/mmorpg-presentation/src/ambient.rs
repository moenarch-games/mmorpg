//! Greyhaven's ambient animals (#419, #824): asset-tooling's `greyhaven-ambient-animals` cat, dog and
//! rodent, each pacing a small authored loop: a cat in front of the inn, a dog before the
//! Millbrook farmhouse and a brown rodent along the crates by the barn, and in the region's
//! settlements dogs, cats and a rodent of their own. Its `greyhaven-farm-animals` (#849) graze by
//! the barns of Millbrook Farm, Barrowdown, the farmland's three hamlets, the Ford Inn and
//! Westmere Lodge: cows, horses, a donkey, pigs, sheep, goats and chickens. Its `greyhaven-wildlife`
//! (#857) roams the wilder ground: deer by the hunters' camp, Westmere and the Delving Workings,
//! boar in the hunters' woods, foxes by the farmland and Greyfang Watch, rabbits on the meadows,
//! crows by the fields and a songbird by Barrowdown. Each stands on the drawn surface.
//!
//! Each animal rests (`Idle`, turning toward its next waypoint, then `Eat` once a grazer has
//! turned; a client whose model has no `Eat` keeps `Idle`) and walks (`Walk`, in place) round
//! its waypoints on the presentation clock alone, so a pose is a pure function of time: stable,
//! bounded and identical in both clients. Cosmetic only: no unit IDs, AI, collision, loot, combat
//! or ownership; the zone never hears of them.
use mmorpg_core::UNITS_PER_METRE;
use mmorpg_scenery::Scenery;

use crate::{
    batching::matrix,
    geometry::{compose, ry, translate},
};

/// The rolling releases the clients draw: the ambient animals' and the farm animals' (#849),
/// whose keys start [`FARM_PREFIX`].
pub const RELEASE: &str = "greyhaven-ambient-animals";
pub const FARM_RELEASE: &str = "greyhaven-farm-animals";
pub const FARM_PREFIX: &str = "greyhaven.farm.";
/// The wildlife's release (#857), whose keys start [`WILD_PREFIX`].
pub const WILD_RELEASE: &str = "greyhaven-wildlife";
pub const WILD_PREFIX: &str = "greyhaven.wild.";
/// The bundles' clips, in the index order of [`Pose::clip`].
pub const CLIPS: [&str; 3] = ["Idle", "Walk", "Eat"];
/// Seconds a rest spends turning toward the next waypoint.
const TURN_SECONDS: f64 = 0.6;
/// The cat's `Walk` (#799): metres its body travels per cycle with its planted paws held still
/// (the bundle descriptor's `walkStride`), and the cycle's seconds. Walked at exactly
/// [`CAT_PACE`], its paws track the ground instead of skating.
const CAT_STRIDE: f64 = 0.3469;
const CAT_WALK_SECONDS: f64 = 32. / 24.;
const CAT_PACE: f64 = CAT_STRIDE / CAT_WALK_SECONDS;

/// One animal's authored loop.
struct Beat {
    key: &'static str,
    /// Waypoints in units on the ground, walked in order and back to the first.
    path: [[i32; 2]; 3],
    /// Walking speed in metres per second.
    speed: f64,
    /// Seconds resting at each waypoint.
    rest: f64,
    /// Seconds into the loop at clock zero, so the animals never move in step.
    phase: f64,
    /// Whether it grazes (`Eat`) once it has turned at a rest.
    grazes: bool,
}

/// A loose animal's loop.
const fn roam(key: &'static str, path: [[i32; 2]; 3], speed: f64, rest: f64, phase: f64) -> Beat {
    Beat {
        key,
        path,
        speed,
        rest,
        phase,
        grazes: false,
    }
}

/// A farm animal `greyhaven.farm.<kind>` grazing round a small triangle about `[x, z]`, `reach`
/// units from it, in a quarter of its pasture.
const fn graze(
    key: &'static str,
    [x, z]: [i32; 2],
    reach: i32,
    speed: f64,
    rest: f64,
    phase: f64,
) -> Beat {
    Beat {
        key,
        path: [
            [x - reach, z - reach * 2 / 3],
            [x + reach, z - reach / 2],
            [x, z + reach],
        ],
        speed,
        rest,
        phase,
        grazes: true,
    }
}

/// The pastures (#849): open ground by each barn, clear of every structure, road, field and
/// shore; each animal keeps to a quarter of its pasture, 2.5 m from the middle.
const MILLBROOK: [i32; 2] = [7_500, 2_500];
const BARROWDOWN: [i32; 2] = [53_500, 68_200];
const WEST_HAMLET: [i32; 2] = [-9_500, 36_000];
const EAST_HAMLET: [i32; 2] = [14_000, 39_500];
const SOUTH_HAMLET: [i32; 2] = [-8_000, 57_500];
const FORD_INN: [i32; 2] = [104_900, 22_100];
const WESTMERE: [i32; 2] = [-64_500, 6_000];
/// The wildlife's haunts (#857): open ground near woods, meadows and fields, clear like the
/// pastures. The Vale meadow lies on the basin floor clear of the rim's rock band (#1195).
const HUNTERS_WOOD: [i32; 2] = [-46_300, 72_500];
const WESTMERE_GLADE: [i32; 2] = [-60_000, 12_000];
const BOAR_WOOD: [i32; 2] = [-51_600, 81_300];
const FARMLAND_EDGE: [i32; 2] = [1_700, 47_000];
const BARROW_MEADOW: [i32; 2] = [49_000, 64_000];
const FARMLAND_MEADOW: [i32; 2] = [-1_600, 39_600];
const VALE_MEADOW: [i32; 2] = [1_000, 8_400];
const FIELD_EDGE: [i32; 2] = [6_000, 50_000];
const MILLBROOK_FIELD: [i32; 2] = [10_500, 5_300];
const BARROW_HEDGE: [i32; 2] = [55_500, 72_500];
const DELVING_SLOPE: [i32; 2] = [100_000, -60_500];
const GREYFANG_HEATH: [i32; 2] = [16_000, -82_300];

/// `pasture` moved to its quarter `q` (0–3).
const fn quarter([x, z]: [i32; 2], q: u8) -> [i32; 2] {
    let (dx, dz) = match q {
        0 => (250, 250),
        1 => (-250, 250),
        2 => (-250, -250),
        _ => (250, -250),
    };
    [x + dx, z + dz]
}

#[rustfmt::skip]
const BEATS: [Beat; 52] = [
    // East of the inn door, between the front wall and the square.
    roam("greyhaven.ambient.cat", [[1_800, 600], [2_080, 640], [1_950, 820]], CAT_PACE, 4.5, 0.),
    // Before the Millbrook farmhouse, inside the field fence.
    roam("greyhaven.ambient.dog", [[5_900, 5_200], [6_500, 5_220], [6_250, 5_420]], 0.9, 3., 2.),
    // Along the west face of the crates by the barn.
    roam("greyhaven.ambient.rodent", [[7_110, 3_240], [7_120, 3_580], [7_170, 3_420]], 0.35, 2.5, 5.),
    // The settlements (#824): a dog by Barrowdown's barn and one at the hunters' camp, cats at
    // the Ford Inn and the Westmere Lodge, a rodent by the east hamlet's barn.
    roam("greyhaven.ambient.dog", [[53_000, 68_400], [53_800, 68_600], [53_400, 69_400]], 0.9, 3.5, 1.),
    roam("greyhaven.ambient.cat", [[105_000, 20_900], [105_400, 21_200], [104_900, 21_300]], CAT_PACE, 5., 3.),
    roam("greyhaven.ambient.cat", [[-65_200, 4_600], [-64_700, 4_900], [-65_300, 5_100]], CAT_PACE, 4., 1.5),
    roam("greyhaven.ambient.dog", [[-48_600, 78_900], [-47_800, 78_700], [-48_000, 79_100]], 0.9, 3., 4.),
    roam("greyhaven.ambient.rodent", [[13_600, 41_800], [14_200, 41_900], [13_900, 42_100]], 0.35, 2.5, 2.5),
    // The farm animals (#849), by the barns.
    graze("greyhaven.farm.cow", quarter(MILLBROOK, 0), 120, 0.6, 9., 0.),
    graze("greyhaven.farm.horse", quarter(MILLBROOK, 1), 120, 0.8, 8., 3.),
    graze("greyhaven.farm.pig", quarter(MILLBROOK, 2), 110, 0.5, 6., 1.),
    graze("greyhaven.farm.chicken", quarter(MILLBROOK, 3), 70, 0.4, 3., 0.5),
    graze("greyhaven.farm.sheep", quarter(BARROWDOWN, 0), 110, 0.5, 8., 2.),
    graze("greyhaven.farm.sheep", quarter(BARROWDOWN, 1), 110, 0.5, 7., 6.),
    graze("greyhaven.farm.goat", quarter(BARROWDOWN, 2), 110, 0.6, 6., 4.),
    graze("greyhaven.farm.chicken", quarter(BARROWDOWN, 3), 70, 0.4, 3., 1.5),
    graze("greyhaven.farm.cow", quarter(WEST_HAMLET, 0), 120, 0.6, 10., 5.),
    graze("greyhaven.farm.sheep", quarter(WEST_HAMLET, 2), 110, 0.5, 8., 0.),
    graze("greyhaven.farm.chicken", quarter(WEST_HAMLET, 3), 70, 0.4, 3.5, 2.),
    graze("greyhaven.farm.pig", quarter(EAST_HAMLET, 0), 110, 0.5, 6., 3.),
    graze("greyhaven.farm.horse", quarter(EAST_HAMLET, 1), 120, 0.8, 9., 1.),
    graze("greyhaven.farm.chicken", quarter(EAST_HAMLET, 3), 70, 0.4, 3., 0.),
    graze("greyhaven.farm.cow", quarter(SOUTH_HAMLET, 1), 120, 0.6, 9., 2.),
    graze("greyhaven.farm.goat", quarter(SOUTH_HAMLET, 2), 110, 0.6, 6., 5.),
    graze("greyhaven.farm.chicken", quarter(SOUTH_HAMLET, 0), 70, 0.4, 3., 1.),
    graze("greyhaven.farm.chicken", quarter(SOUTH_HAMLET, 3), 70, 0.4, 4., 2.5),
    graze("greyhaven.farm.donkey", quarter(FORD_INN, 0), 110, 0.6, 9., 0.),
    graze("greyhaven.farm.chicken", quarter(FORD_INN, 2), 70, 0.4, 3., 1.),
    graze("greyhaven.farm.horse", quarter(WESTMERE, 0), 120, 0.8, 8., 2.),
    graze("greyhaven.farm.donkey", quarter(WESTMERE, 2), 110, 0.6, 9., 4.),
    // The wildlife (#857).
    graze("greyhaven.wild.stag", quarter(HUNTERS_WOOD, 0), 120, 0.7, 8., 1.),
    graze("greyhaven.wild.doe", quarter(HUNTERS_WOOD, 1), 120, 0.7, 7., 4.),
    graze("greyhaven.wild.doe", quarter(HUNTERS_WOOD, 2), 120, 0.7, 9., 2.),
    graze("greyhaven.wild.doe", quarter(WESTMERE_GLADE, 0), 120, 0.7, 8., 0.),
    graze("greyhaven.wild.doe", quarter(WESTMERE_GLADE, 3), 120, 0.7, 7., 3.),
    graze("greyhaven.wild.boar", quarter(BOAR_WOOD, 0), 110, 0.6, 6., 1.),
    graze("greyhaven.wild.boar", quarter(BOAR_WOOD, 2), 110, 0.6, 7., 4.),
    graze("greyhaven.wild.fox", quarter(FARMLAND_EDGE, 0), 120, 0.9, 4., 2.),
    graze("greyhaven.wild.rabbit", quarter(BARROW_MEADOW, 0), 70, 0.6, 3., 0.),
    graze("greyhaven.wild.rabbit", quarter(BARROW_MEADOW, 2), 70, 0.6, 4., 1.5),
    graze("greyhaven.wild.rabbit", quarter(FARMLAND_MEADOW, 1), 70, 0.6, 3., 0.5),
    graze("greyhaven.wild.rabbit", quarter(FARMLAND_MEADOW, 3), 70, 0.6, 3.5, 2.),
    graze("greyhaven.wild.rabbit", quarter(VALE_MEADOW, 0), 70, 0.6, 3., 1.),
    graze("greyhaven.wild.crow", quarter(FIELD_EDGE, 0), 70, 0.5, 3., 0.),
    graze("greyhaven.wild.crow", quarter(FIELD_EDGE, 1), 70, 0.5, 4., 1.),
    graze("greyhaven.wild.crow", quarter(MILLBROOK_FIELD, 2), 70, 0.5, 3., 2.),
    graze("greyhaven.wild.songbird", quarter(BARROW_HEDGE, 0), 60, 0.4, 3., 0.5),
    graze("greyhaven.wild.stag", quarter(DELVING_SLOPE, 0), 120, 0.7, 9., 3.),
    graze("greyhaven.wild.doe", quarter(DELVING_SLOPE, 2), 120, 0.7, 8., 0.),
    graze("greyhaven.wild.fox", quarter(GREYFANG_HEATH, 0), 120, 0.9, 4., 1.),
    graze("greyhaven.wild.boar", quarter(BOAR_WOOD, 1), 110, 0.6, 5., 2.5),
    graze("greyhaven.wild.fox", quarter(BARROW_MEADOW, 1), 120, 0.9, 5., 3.),
];

/// One posed animal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub key: &'static str,
    /// Column-major model-to-world matrix, metres: origin on the ground, head along the heading.
    pub model: [f32; 16],
    /// Index into [`CLIPS`].
    pub clip: usize,
    /// Seconds into the clip since it started; clients wrap it by the clip's length.
    pub time: f64,
}

/// The bundle entries the clients load, in [`poses`] order.
#[must_use]
pub fn entries() -> Vec<&'static str> {
    BEATS.iter().map(|beat| beat.key).collect()
}

/// Each animal's first waypoint in units, in [`poses`] order: where its art is needed.
#[must_use]
pub fn anchors() -> Vec<[i32; 2]> {
    BEATS.iter().map(|beat| beat.path[0]).collect()
}

fn metres(units: f64) -> f64 {
    units / f64::from(UNITS_PER_METRE)
}

fn heading(from: [i32; 2], to: [i32; 2]) -> f64 {
    f64::from(to[0] - from[0]).atan2(f64::from(to[1] - from[1]))
}

fn length(from: [i32; 2], to: [i32; 2]) -> f64 {
    metres(f64::from(to[0] - from[0]).hypot(f64::from(to[1] - from[1])))
}

/// Where `beat` stands at `seconds`: feet in units, heading, clip and clip time.
fn sample(beat: &Beat, seconds: f64) -> ([f64; 2], f64, usize, f64) {
    let n = beat.path.len();
    let leg = |i: usize| (beat.path[i], beat.path[(i + 1) % n]);
    let period: f64 = (0..n)
        .map(|i| {
            let (a, b) = leg(i);
            beat.rest + length(a, b) / beat.speed
        })
        .sum();
    let mut t = (seconds + beat.phase).rem_euclid(period);
    for i in 0..n {
        let (a, b) = leg(i);
        let at = beat.path[i].map(f64::from);
        if t < beat.rest {
            // Turn from the arriving heading to the leaving one, the short way round.
            let arrive = heading(beat.path[(i + n - 1) % n], a);
            let leave = heading(a, b);
            let turn = (leave - arrive + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU)
                - std::f64::consts::PI;
            let u = (t / TURN_SECONDS).min(1.);
            let eased = u * u * (3. - 2. * u);
            // A grazer eats between turning and the last moment before it walks on.
            if beat.grazes && t >= TURN_SECONDS && t < beat.rest - 0.5 {
                return (at, leave, 2, t - TURN_SECONDS);
            }
            return (at, arrive + turn * eased, 0, t);
        }
        t -= beat.rest;
        let walk = length(a, b) / beat.speed;
        if t < walk {
            let u = t / walk;
            let to = b.map(f64::from);
            return (
                [at[0] + (to[0] - at[0]) * u, at[1] + (to[1] - at[1]) * u],
                heading(a, b),
                1,
                t,
            );
        }
        t -= walk;
    }
    (
        beat.path[0].map(f64::from),
        heading(leg(0).0, leg(0).1),
        0,
        0.,
    )
}

/// Every animal at `seconds` on `scenery`'s relief; without `animate` (reduced motion) each holds
/// still at its first waypoint.
#[must_use]
pub fn poses(scenery: &Scenery, seconds: f64, animate: bool) -> Vec<Pose> {
    BEATS
        .iter()
        .map(|beat| {
            let (feet, yaw, clip, time) = if animate {
                sample(beat, seconds)
            } else {
                (
                    beat.path[0].map(f64::from),
                    heading(beat.path[0], beat.path[1]),
                    0,
                    0.,
                )
            };
            #[allow(clippy::cast_possible_truncation)]
            let lift = scenery.surface_at(feet[0].round() as i32, feet[1].round() as i32);
            Pose {
                key: beat.key,
                model: matrix(compose(&[
                    translate(metres(feet[0]), metres(f64::from(lift)), metres(feet[1])),
                    ry(yaw),
                ])),
                clip,
                time,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmorpg_scenery::greyhaven_vale_scenery;

    #[test]
    fn every_animal_stays_on_its_loop_and_cycles_its_clips() {
        let scenery = greyhaven_vale_scenery();
        let mut clips = [[false; 3]; BEATS.len()];
        for step in 0..1_200 {
            let seconds = f64::from(step) * 0.1;
            for (index, pose) in poses(&scenery, seconds, true).iter().enumerate() {
                clips[index][pose.clip] = true;
                let [x, z] = [pose.model[12], pose.model[14]].map(|m| f64::from(m) * 100.);
                let path = BEATS[index].path;
                let (lo, hi) = path
                    .iter()
                    .fold(([f64::MAX; 2], [f64::MIN; 2]), |(lo, hi), p| {
                        let p = p.map(f64::from);
                        (
                            [lo[0].min(p[0]), lo[1].min(p[1])],
                            [hi[0].max(p[0]), hi[1].max(p[1])],
                        )
                    });
                assert!(x >= lo[0] - 1. && x <= hi[0] + 1. && z >= lo[1] - 1. && z <= hi[1] + 1.);
                let ground = f64::from(scenery.surface_at(x.round() as i32, z.round() as i32));
                assert!((f64::from(pose.model[13]) * 100. - ground).abs() < 1.);
            }
        }
        // Every animal rests and walks; the grazers also eat, the rest never do.
        for (beat, seen) in BEATS.iter().zip(clips) {
            assert_eq!(seen, [true, true, beat.grazes], "{}", beat.key);
        }
    }

    #[test]
    fn no_loop_crosses_a_structure() {
        let scenery = greyhaven_vale_scenery();
        let blocked: Vec<_> = scenery
            .props
            .iter()
            .filter(|prop| prop.collider.is_some())
            .map(mmorpg_scenery::Prop::footprint)
            .collect();
        for step in 0..2_400 {
            for pose in poses(&scenery, f64::from(step) * 0.05, true) {
                let [x, z] = [pose.model[12], pose.model[14]].map(|m| (m * 100.).round() as i32);
                let clear = 30;
                assert!(
                    !blocked.iter().any(|r| x > r.min[0] - clear
                        && x < r.max[0] + clear
                        && z > r.min[1] - clear
                        && z < r.max[1] + clear),
                    "{} at {x}, {z}",
                    pose.key
                );
            }
        }
    }

    #[test]
    fn cats_cover_one_stride_per_walk_cycle_and_rest_in_place() {
        // #799: while a cat walks, its ground travel tracks its Walk clip at the stride its paws
        // plant at; resting, or under reduced motion, it holds still in Idle.
        let scenery = greyhaven_vale_scenery();
        let cats: Vec<_> = (0..BEATS.len())
            .filter(|&i| BEATS[i].key == "greyhaven.ambient.cat")
            .collect();
        assert_eq!(cats.len(), 3);
        let at = |pose: &Pose| [pose.model[12], pose.model[14]].map(f64::from);
        let step = 0.25;
        let (mut walked, mut rested) = (0, 0);
        for n in 0..600 {
            let seconds = f64::from(n) * step;
            let (now, next) = (
                poses(&scenery, seconds, true),
                poses(&scenery, seconds + step, true),
            );
            for &i in &cats {
                let (a, b) = (now[i], next[i]);
                let [dx, dz] = [at(&b)[0] - at(&a)[0], at(&b)[1] - at(&a)[1]];
                let moved = dx.hypot(dz);
                if a.clip == 1 && b.clip == 1 && a.model[0] == b.model[0] {
                    // One leg: the clip advances CAT_WALK_SECONDS per CAT_STRIDE travelled.
                    let cycles = (b.time - a.time) / CAT_WALK_SECONDS;
                    assert!((moved - cycles * CAT_STRIDE).abs() < 1e-3, "{moved} m");
                    walked += 1;
                } else if a.clip == 0 && b.clip == 0 {
                    assert!(moved < 1e-6, "a resting cat moved {moved} m");
                    rested += 1;
                }
            }
        }
        assert!(walked > 100 && rested > 100);
        for &i in &cats {
            let (early, late) = (
                poses(&scenery, 3., false)[i],
                poses(&scenery, 40., false)[i],
            );
            assert_eq!((early.clip, early.time), (0, 0.));
            assert_eq!(early, late);
        }
    }

    #[test]
    fn reduced_motion_holds_each_animal_still() {
        let scenery = greyhaven_vale_scenery();
        assert_eq!(poses(&scenery, 3., false), poses(&scenery, 40., false));
    }
}
