//! Cosmetic locomotion and published viewer ability poses.
use std::f64::consts::{PI, TAU};
#[derive(Clone, Copy, Default)]
pub struct Leg {
    pub swing: f64,
    pub spread: f64,
    pub knee: f64,
    pub foot: f64,
}
#[derive(Clone, Copy, Default)]
pub struct Arm {
    pub swing: f64,
    pub spread: f64,
    pub elbow: f64,
}
#[derive(Clone, Copy, Default)]
pub struct Pose {
    pub bob: f64,
    pub lean: f64,
    pub twist: f64,
    pub hip_yaw: f64,
    pub breathe: f64,
    pub head_pitch: f64,
    pub cape: f64,
    pub legs: [Leg; 2],
    pub arms: [Arm; 2],
}
#[derive(Clone, Copy, Default)]
pub struct Locomotion {
    pub forward: f64,
    pub right: f64,
    pub vertical: f64,
    pub movement: f64,
    pub air: f64,
    pub turn: f64,
    pub stride: f64,
    pub shuffle: f64,
    pub time: f64,
}
#[derive(Clone, Copy, Debug)]
pub enum Stance {
    Sword,
    Bow,
    Staff,
}
#[derive(Default)]
pub struct Animator {
    last: Option<([f64; 2], f64)>,
    state: Locomotion,
}
impl Animator {
    pub fn update(
        &mut self,
        p: [f64; 2],
        yaw: f64,
        v: [f64; 3],
        dt: f64,
        reduced: bool,
    ) -> Locomotion {
        let s = &mut self.state;
        s.forward = v[0] * yaw.sin() + v[2] * yaw.cos();
        s.right = -v[0] * yaw.cos() + v[2] * yaw.sin();
        s.vertical = v[1];
        if let Some((last, facing)) = self.last {
            let travelled = (p[0] - last[0]).hypot(p[1] - last[1]);
            if travelled < 2. {
                s.stride += travelled / 2.6 * TAU;
            }
            let turned = (yaw - facing + PI).rem_euclid(TAU) - PI;
            s.turn = if dt > 0. { turned / dt } else { 0. };
            s.shuffle += turned.abs() * 3.;
        }
        self.last = Some((p, yaw));
        s.stride %= TAU * 1000.;
        s.shuffle %= TAU * 1000.;
        let ramp = |current: f64, target: f64| {
            if reduced {
                target
            } else if current < target {
                target.min(current + dt / 0.14)
            } else {
                target.max(current - dt / 0.14)
            }
        };
        s.movement = ramp(
            s.movement,
            if s.forward.hypot(s.right) > 0.3 {
                1.
            } else {
                0.
            },
        );
        s.air = ramp(s.air, if v[1].abs() >= 0.3 { 1. } else { 0. });
        s.time = if reduced { 0. } else { s.time + dt };
        *s
    }
}
pub fn pose(s: Locomotion, stance: Stance) -> Pose {
    let speed = s.forward.hypot(s.right);
    let mov = s.movement;
    let air = s.air;
    let ground = 1. - air;
    let phase = s.stride;
    let time = s.time;
    let run = (speed / 6.3).clamp(0., 1.);
    let direction = if speed > 0.05 {
        s.right.atan2(s.forward)
    } else {
        0.
    };
    let along = direction.cos();
    let across = direction.sin();
    let backward = along < -0.2;
    let hip = if backward {
        0.
    } else {
        direction.clamp(-0.7, 0.7) * (along.abs() * 2.).min(1.) * mov * ground
    };
    let stride = (0.45 + 0.4 * run) * mov * ground;
    let sin = phase.sin();
    let cos = phase.cos();
    let turned = hip.abs() > 0.05;
    let fs = if turned { 1. } else { along.abs() };
    let breathe = 0.015 + 0.012 * (time * 2.1).sin();
    let idle = 1. - mov;
    let turning = (s.turn.abs() / 2.5).min(1.) * idle * ground;
    let rising = (s.vertical / 3.).clamp(0., 1.);
    let tuck = air * (0.45 + 0.55 * rising);
    let legs = std::array::from_fn(|i| {
        let side = if i == 0 { 1. } else { -1. };
        let angle = side * stride * sin;
        let shuffle = (side * s.shuffle.sin()).max(0.) * turning;
        Leg {
            swing: (angle * fs * if backward { -0.8 } else { 1. }
                + tuck * if i == 0 { 0.55 } else { 0.15 })
            .clamp(-1.2, 1.3),
            spread: (angle * if turned { 0. } else { across } * -0.55 + side * shuffle * 0.12)
                .clamp(-0.45, 0.45),
            knee: ((0.15 * run + (0.35 + 0.9 * run) * (side * cos).max(0.)) * mov * ground
                + shuffle * 0.55
                + if i == 0 {
                    tuck * 1.1
                } else {
                    air * (0.35 + 0.3 * rising)
                })
            .clamp(0., 1.9),
            foot: (angle * 0.35).clamp(-0.6, 0.6),
        }
    });
    let arm_stride = (0.5 + 0.55 * run) * mov * ground * if backward { 0.6 } else { 1. };
    let arms = std::array::from_fn(|i| {
        let side = if i == 0 { 1. } else { -1. };
        let (held, elbow) = match (stance, i) {
            (Stance::Sword, 0) => (0.5, 0.55),
            (Stance::Sword, _) => (0.7, 0.45),
            (Stance::Bow, 0) => (0.6, 0.35),
            (Stance::Bow, _) => (0.85, 0.2),
            (Stance::Staff, 0) => (0.9, 0.2),
            (Stance::Staff, _) => (0.35, 0.9),
        };
        Arm {
            swing: (-side * sin * arm_stride * held * fs
                + air * 0.35
                + idle * 0.02 * (time * 1.3 + side).sin())
            .clamp(-1.2, 2.2),
            spread: (0.1
                + 0.05 * idle * (time * 2.1).sin()
                + air * (0.55 + 0.25 * (1. - rising))
                + turning * 0.1)
                .clamp(-0.2, 1.2),
            elbow: (elbow + (0.35 + 0.9 * run) * mov * ground + air * 0.3).clamp(0., 2.),
        }
    });
    Pose {
        bob: ((0.045 * run + 0.02) * ((2. * phase).cos() - 1.) / 2. * mov * ground
            - breathe * 0.3 * idle)
            .clamp(-0.12, 0.08),
        lean: ((if backward { -0.06 } else { 0.05 + 0.14 * run }) * mov * ground
            - air * 0.05 * (1. - rising))
            .clamp(-0.3, 0.35),
        twist: (0.14 * sin * stride - hip * 0.6).clamp(-0.4, 0.4),
        hip_yaw: hip.clamp(-0.8, 0.8),
        breathe: (breathe * idle).clamp(0., 0.03),
        head_pitch: (-0.08 * run * mov + 0.04 * idle * (time * 0.7).sin()).clamp(-0.3, 0.3),
        cape: (0.08
            + if backward { 0.05 } else { 0.45 * run } * mov
            + air * (0.2 + 0.3 * (1. - rising)))
            .clamp(0., 0.9),
        legs,
        arms,
    }
}
/// Whether a body stands, or lies dead with its fall `progress` (0 just died, 1 at rest).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Life {
    Alive,
    Dead { progress: f64 },
}

impl Life {
    #[must_use]
    pub const fn dead(self) -> bool {
        matches!(self, Self::Dead { .. })
    }

    /// Fall progress, 1 when alive or settled.
    #[must_use]
    pub fn fall(self) -> f64 {
        match self {
            Self::Alive => 1.,
            Self::Dead { progress } => progress.clamp(0., 1.),
        }
    }
}

/// How strongly an action's pose applies: a cast ramps in and stays; an instant ability swells
/// and releases.
#[must_use]
pub fn weight(a: Action) -> f64 {
    let t = a.progress.clamp(0., 1.);
    if a.hold {
        (t * 5.).min(1.)
    } else {
        (PI * t).sin()
    }
}

#[derive(Clone, Copy)]
pub struct Action {
    pub stance: Stance,
    pub progress: f64,
    pub hold: bool,
}
pub fn action(mut p: Pose, a: Option<Action>) -> Pose {
    let Some(a) = a else { return p };
    let t = a.progress.clamp(0., 1.);
    let w = if a.hold {
        (t * 5.).min(1.)
    } else {
        (PI * t).sin()
    };
    let mix = |x: f64, y: f64| x + (y - x) * w;
    let arm = |base: Arm, target: Arm, weight: f64| Arm {
        swing: (base.swing + (target.swing - base.swing) * weight).clamp(-1.2, 2.2),
        spread: (base.spread + (target.spread - base.spread) * weight).clamp(-0.2, 1.2),
        elbow: (base.elbow + (target.elbow - base.elbow) * weight).clamp(0., 2.),
    };
    match a.stance {
        Stance::Staff => {
            p.lean = mix(p.lean, 0.06).clamp(-0.3, 0.35);
            p.head_pitch = mix(p.head_pitch, -0.12).clamp(-0.3, 0.3);
            p.arms[0] = arm(
                p.arms[0],
                Arm {
                    swing: 1.15,
                    spread: 0.25,
                    elbow: 0.7,
                },
                w,
            );
            p.arms[1] = arm(
                p.arms[1],
                Arm {
                    swing: 1.7,
                    spread: 0.2,
                    elbow: 0.45,
                },
                w,
            );
        }
        Stance::Bow => {
            p.twist = mix(p.twist, 0.3).clamp(-0.4, 0.4);
            p.arms[0] = arm(
                p.arms[0],
                Arm {
                    swing: 1.55,
                    spread: 0.1,
                    elbow: 0.05,
                },
                w,
            );
            p.arms[1] = arm(
                p.arms[1],
                Arm {
                    swing: 0.95,
                    spread: 0.3,
                    elbow: 1.8,
                },
                w,
            );
        }
        Stance::Sword => {
            p.lean = mix(p.lean, 0.05 + 0.2 * t).clamp(-0.3, 0.35);
            p.twist = mix(p.twist, -0.3 + 0.6 * t).clamp(-0.4, 0.4);
            p.arms[1] = arm(
                p.arms[1],
                Arm {
                    swing: 2.1 - 1.9 * t,
                    spread: 0.15,
                    elbow: 0.5 + 0.7 * t,
                },
                (w * 1.4).min(1.),
            );
        }
    }
    p
}
