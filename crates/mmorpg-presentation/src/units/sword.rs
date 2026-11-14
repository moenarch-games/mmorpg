//! The timed sword's bounded arm fit and brief blade ribbon (#679). Both skinning adapters
//! consume the same procedural layer over the authored body clips; no pose decides a hit.
use std::f64::consts::{FRAC_PI_2, TAU};

use mmorpg_core::{STRIKE_ARC_HALF_WIDTH, StrikeAim, StrikeHeight, StrikeSide, UNITS_PER_METRE};

use super::{
    imported::Layer,
    kit::{Frame, Q, qa, qm, qr},
};
use crate::geometry::{V, norm};

/// An accepted swing. Its clock, heading and aim come from `SwingBegan`, never local input.
#[derive(Clone, Copy, Debug)]
pub struct Swing {
    pub aim: StrikeAim,
    pub facing: u16,
    pub began: f64,
    pub startup: f64,
    pub active: f64,
}

impl Swing {
    pub fn yaw(self) -> f64 {
        f64::from(self.facing) / 65536. * TAU
    }
    pub fn until(self) -> f64 {
        self.began + self.startup + self.active + 0.25
    }

    pub fn pose(self, now: f64) -> Option<Layer> {
        let elapsed = now - self.began;
        let active = elapsed - self.startup;
        let weight = if elapsed < self.startup {
            (elapsed / self.startup.max(1e-6)).clamp(0., 1.)
        } else {
            (1. - (active - self.active).max(0.) / 0.12).max(0.)
        };
        (weight > 0.).then(|| {
            layer(
                self.aim,
                (active / self.active.max(1e-6)).clamp(0., 1.) as f32,
                weight as f32,
            )
        })
    }

    /// A short airborne afterimage of the blade, only behind the active sweep, fading for
    /// 60ms at its end. The outer edge denotes centre-hit reach (which includes the victim's
    /// body), not the length of an enlarged weapon. It is depth-tested ordinary presentation.
    pub fn trail(self, id: &str, feet: V, now: f64) -> Option<crate::Draw> {
        let elapsed = now - self.began - self.startup;
        let from = ((elapsed - 0.06) / self.active).clamp(0., 1.);
        let to = (elapsed / self.active).clamp(0., 1.);
        if from >= to {
            return None;
        }
        let [bottom, top] = self.aim.height.band();
        let height = f64::from(bottom + top) / (2. * f64::from(UNITS_PER_METRE));
        let reach = f64::from(mmorpg_core::unit::PLAYER_REACH_UNITS) / f64::from(UNITS_PER_METRE);
        let grip_height = height.clamp(0.8, 1.45);
        let slope = (height - grip_height) / (reach - 0.55);
        // A low grip cannot be lowered through the knees. Its blade tilts into the low
        // lane; only the portion inside that lane leaves an affected-area afterimage.
        let band = [bottom, top].map(|value| f64::from(value) / f64::from(UNITS_PER_METRE));
        let inner = if grip_height > band[1] {
            0.55 + (band[1] - grip_height) / slope
        } else if grip_height < band[0] {
            0.55 + (band[0] - grip_height) / slope
        } else {
            0.55
        };
        let mut geometry = crate::geometry::Geometry::default();
        for part in 0..8 {
            let at = |step: u32, radius: f64| {
                let progress = from + (to - from) * f64::from(step) / 8.;
                let angle = angle(self.aim, progress);
                [
                    radius * angle.sin(),
                    grip_height + (radius - 0.55) * slope,
                    radius * angle.cos(),
                ]
            };
            geometry.face(&[
                at(part, inner),
                at(part, reach),
                at(part + 1, reach),
                at(part + 1, inner),
            ]);
        }
        let fade = (1. - (elapsed - self.active).max(0.) / 0.06).clamp(0., 1.);
        Some(crate::Draw {
            id: format!("{id}-sword-trail"),
            resource_key: format!(
                "sword-trail:{}:{}:{:x}:{:x}",
                self.aim.height.code(),
                self.aim.side.code(),
                from.to_bits(),
                to.to_bits()
            ),
            geometry: std::sync::Arc::new(geometry),
            model: super::kit::model(
                Frame {
                    p: feet,
                    q: qa(1, self.yaw()),
                },
                [1.; 3],
            ),
            material: crate::Material {
                color: [0.75, 0.86, 1.],
                emissive: [0.1, 0.14, 0.2],
                opacity: (0.38 * fade) as f32,
                unlit: false,
                double_sided: true,
                ground: None,
            },
        })
    }
}

const NAMES: [[&str; 3]; 3] = [
    ["TimedSword_Low_L", "TimedSword_Low_C", "TimedSword_Low_R"],
    ["TimedSword_Mid_L", "TimedSword_Mid_C", "TimedSword_Mid_R"],
    [
        "TimedSword_High_L",
        "TimedSword_High_C",
        "TimedSword_High_R",
    ],
];

/// A procedural layer, sampled by both existing skinning providers after their clip mix.
/// `time` is sweep progress, and `weight` eases the arm into/out of the authored pose.
pub fn layer(aim: StrikeAim, progress: f32, weight: f32) -> Layer {
    Layer {
        clip: NAMES[aim.height.code() as usize][aim.side.code() as usize],
        time: progress,
        weight,
        mask: super::imported::Mask::Upper,
    }
}

pub fn aim(name: &str) -> Option<StrikeAim> {
    for height in [StrikeHeight::Low, StrikeHeight::Middle, StrikeHeight::High] {
        for side in [StrikeSide::Left, StrikeSide::Centre, StrikeSide::Right] {
            let aim = StrikeAim::new(side, height);
            if name == layer(aim, 0., 0.).clip {
                return Some(aim);
            }
        }
    }
    None
}

/// Angle in the attacker's frame: the same side centre, width and direction as `Swing::slice`.
pub fn angle(aim: StrikeAim, progress: f64) -> f64 {
    let half = f64::from(STRIKE_ARC_HALF_WIDTH) / 65536. * TAU;
    let centre = match aim.side {
        StrikeSide::Left => TAU / 8.,
        StrikeSide::Centre => 0.,
        StrikeSide::Right => -TAU / 8.,
    };
    let direction = if aim.side == StrikeSide::Right {
        -1.
    } else {
        1.
    };
    centre + direction * (2. * progress.clamp(0., 1.) - 1.) * half
}

fn sub(a: V, b: V) -> V {
    std::array::from_fn(|i| a[i] - b[i])
}
fn add(a: V, b: V) -> V {
    std::array::from_fn(|i| a[i] + b[i])
}
fn dot(a: V, b: V) -> f64 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}
fn length(v: V) -> f64 {
    dot(v, v).sqrt()
}
fn conjugate([x, y, z, w]: Q) -> Q {
    [-x, -y, -z, w]
}

fn between(a: V, b: V) -> Q {
    let (a, b) = (norm(a), norm(b));
    let q = [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
        1. + dot(a, b),
    ];
    let n = q.iter().map(|v| v * v).sum::<f64>();
    if n < 1e-12 {
        let axis = if a[0].abs() < 0.9 {
            [0., a[2], -a[1]]
        } else {
            [-a[2], 0., a[0]]
        };
        let [x, y, z] = norm(axis);
        return [x, y, z, 0.];
    }
    q.map(|v| v / n.sqrt())
}

fn blend(a: Q, b: Q, weight: f64) -> Q {
    let sign = if a.iter().zip(b).map(|(a, b)| a * b).sum::<f64>() < 0. {
        -1.
    } else {
        1.
    };
    let q: Q = std::array::from_fn(|i| a[i] + (b[i] * sign - a[i]) * weight);
    let n = q.iter().map(|v| v * v).sum::<f64>().sqrt().max(1e-9);
    q.map(|v| v / n)
}

/// Right-arm local rotations, parent/upperarm/lowerarm/hand in model space and the weapon's
/// socket in hand space. Bone lengths and the existing sword stay unchanged. The wrist aims
/// toward the lane inside the effective centre-hit reach; it never stretches the arm to it.
pub fn fit(
    frames: [Frame; 4],
    socket: Frame,
    height: f64,
    floor: f64,
    sample: Layer,
) -> Option<[Q; 3]> {
    let aim = aim(sample.clip)?;
    let [parent, upper, lower, hand] = frames;
    let scale = height / 1.8;
    let yaw = angle(aim, f64::from(sample.time));
    let [bottom, top] = aim.height.band();
    let lane = f64::from(bottom + top) / (2. * f64::from(UNITS_PER_METRE));
    // Keep the grip within the existing arm's range, tilting the blade into low/high lanes.
    let grip_height = lane.clamp(0.8, 1.45);
    let grip = [
        0.55 * yaw.sin() * scale,
        floor + grip_height * scale,
        0.55 * yaw.cos() * scale,
    ];
    let reach = f64::from(mmorpg_core::unit::PLAYER_REACH_UNITS) / f64::from(UNITS_PER_METRE);
    let pitch = ((lane - grip_height) / (reach - 0.55)).atan();
    let socket_q = qm(qa(1, yaw), qa(0, FRAC_PI_2 - pitch));
    let hand_q = qm(socket_q, conjugate(socket.q));
    let wanted = sub(grip, qr(hand_q, socket.p));
    let (a, b) = (length(sub(lower.p, upper.p)), length(sub(hand.p, lower.p)));
    if a < 1e-5 || b < 1e-5 {
        return None;
    }
    let direction = norm(sub(wanted, upper.p));
    let distance = length(sub(wanted, upper.p)).clamp((a - b).abs() + 1e-5, a + b - 1e-5);
    let wrist = add(upper.p, direction.map(|v| v * distance));
    let along = (a * a - b * b + distance * distance) / (2. * distance);
    let pole = [-1., -0.35, 0.];
    let bend = norm(sub(pole, direction.map(|v| v * dot(pole, direction))));
    let elbow = add(
        add(upper.p, direction.map(|v| v * along)),
        bend.map(|v| v * (a * a - along * along).max(0.).sqrt()),
    );
    let upper_q = qm(between(sub(lower.p, upper.p), sub(elbow, upper.p)), upper.q);
    let lower_q = qm(between(sub(hand.p, lower.p), sub(wrist, elbow)), lower.q);
    let original = [
        qm(conjugate(parent.q), upper.q),
        qm(conjugate(upper.q), lower.q),
        qm(conjugate(lower.q), hand.q),
    ];
    let fitted = [
        qm(conjugate(parent.q), upper_q),
        qm(conjugate(upper_q), lower_q),
        qm(conjugate(lower_q), hand_q),
    ];
    Some(std::array::from_fn(|i| {
        blend(
            original[i],
            fitted[i],
            f64::from(sample.weight.clamp(0., 1.)),
        )
    }))
}

#[cfg(test)]
mod tests {
    use super::super::kit::IQ;
    use super::*;

    #[test]
    fn sword_motion_and_trail_share_the_accepted_sweep() {
        let aim = StrikeAim::new(StrikeSide::Centre, StrikeHeight::Middle);
        let swing = Swing {
            aim,
            facing: 16384,
            began: 0.,
            startup: 0.3,
            active: 0.15,
        };
        assert!(swing.trail("test", [0.; 3], 0.29).is_none());
        let draw = swing.trail("test", [0.; 3], 0.4).unwrap();
        assert!(
            draw.geometry
                .p
                .iter()
                .all(|p| (p[1] - 0.95).abs() < 1e-9 && p[0].hypot(p[2]) <= 2.5 + 1e-9)
        );
        assert!(swing.trail("test", [0.; 3], 0.52).is_none());
        let frames = [
            Frame {
                p: [0., 1.4, 0.],
                q: IQ,
            },
            Frame {
                p: [-0.2, 1.4, 0.],
                q: IQ,
            },
            Frame {
                p: [-0.45, 1.25, 0.],
                q: IQ,
            },
            Frame {
                p: [-0.55, 1., 0.],
                q: IQ,
            },
        ];
        let original_upper = sub(frames[2].p, frames[1].p);
        let original_lower = sub(frames[3].p, frames[2].p);
        for side in [StrikeSide::Left, StrikeSide::Centre, StrikeSide::Right] {
            for height in [StrikeHeight::Low, StrikeHeight::Middle, StrikeHeight::High] {
                let aim = StrikeAim::new(side, height);
                let [bottom, top] = height
                    .band()
                    .map(|v| f64::from(v) / f64::from(UNITS_PER_METRE));
                let trail = Swing { aim, ..swing }.trail("lane", [0.; 3], 0.4).unwrap();
                assert!(
                    trail
                        .geometry
                        .p
                        .iter()
                        .all(|p| p[1] >= bottom - 1e-9 && p[1] <= top + 1e-9)
                );
                for progress in [0., 0.5, 1.] {
                    let rotations = fit(
                        frames,
                        Frame { p: [0.; 3], q: IQ },
                        1.8,
                        0.,
                        layer(aim, progress, 1.),
                    )
                    .unwrap();
                    let upper = qm(frames[0].q, rotations[0]);
                    let lower = qm(upper, rotations[1]);
                    let hand = qm(lower, rotations[2]);
                    assert!(
                        (length(qr(upper, original_upper)) - length(original_upper)).abs() < 1e-9
                    );
                    assert!(
                        (length(qr(lower, original_lower)) - length(original_lower)).abs() < 1e-9
                    );
                    let blade = qr(hand, [0., 1., 0.]);
                    assert!(
                        (blade[0].atan2(blade[2]) - angle(aim, f64::from(progress))).abs() < 1e-9
                    );
                }
            }
        }
    }
}
