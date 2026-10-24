//! Greyhaven's ambient animals (#419): asset-tooling's `greyhaven-ambient-animals` cat, dog and
//! rodent, each pacing a small authored loop: a cat in front of the inn, a dog before the
//! Millbrook farmhouse and a brown rodent along the crates by the barn.
//!
//! Each animal rests (`Idle`, turning toward its next waypoint) and walks (`Walk`, in place) round
//! its waypoints on the presentation clock alone, so a pose is a pure function of time: stable,
//! bounded and identical in both clients. Cosmetic only: no unit IDs, AI, collision, loot, combat
//! or ownership; the zone never hears of them.
use mmorpg_core::UNITS_PER_METRE;
use mmorpg_scenery::Scenery;

use crate::{
    batching::matrix,
    geometry::{compose, ry, translate},
};

/// The rolling release the clients draw.
pub const RELEASE: &str = "greyhaven-ambient-animals";
/// The bundle's clips, in the index order of [`Pose::clip`].
pub const CLIPS: [&str; 2] = ["Idle", "Walk"];
/// Seconds a rest spends turning toward the next waypoint.
const TURN_SECONDS: f64 = 0.6;

/// One animal's authored loop.
struct Beat {
    key: &'static str,
    /// Waypoints in units on the ground, walked in order and back to the first.
    path: &'static [[i32; 2]],
    /// Walking speed in metres per second.
    speed: f64,
    /// Seconds resting at each waypoint.
    rest: f64,
    /// Seconds into the loop at clock zero, so the animals never move in step.
    phase: f64,
}

#[rustfmt::skip]
const BEATS: [Beat; 3] = [
    // East of the inn door, between the front wall and the square.
    Beat { key: "greyhaven.ambient.cat", path: &[[1_800, 600], [2_080, 640], [1_950, 820]], speed: 0.45, rest: 4.5, phase: 0. },
    // Before the Millbrook farmhouse, inside the field fence.
    Beat { key: "greyhaven.ambient.dog", path: &[[5_900, 5_200], [6_500, 5_220], [6_250, 5_420]], speed: 0.9, rest: 3., phase: 2. },
    // Along the west face of the crates by the barn.
    Beat { key: "greyhaven.ambient.rodent", path: &[[7_110, 3_240], [7_120, 3_580], [7_170, 3_420]], speed: 0.35, rest: 2.5, phase: 5. },
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
            let lift = scenery.height_at(feet[0].round() as i32, feet[1].round() as i32);
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
    fn every_animal_stays_on_its_loop_and_cycles_both_clips() {
        let scenery = greyhaven_vale_scenery();
        let mut clips = [[false; 2]; 3];
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
                let ground = f64::from(scenery.height_at(x.round() as i32, z.round() as i32));
                assert!((f64::from(pose.model[13]) * 100. - ground).abs() < 1.);
            }
        }
        assert_eq!(clips, [[true; 2]; 3]);
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
    fn reduced_motion_holds_each_animal_still() {
        let scenery = greyhaven_vale_scenery();
        assert_eq!(poses(&scenery, 3., false), poses(&scenery, 40., false));
    }
}
