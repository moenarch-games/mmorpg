//! Browser creature families and NPC role rigs, fitted to their real content boxes.
use super::{animation::*, kit::*};
use crate::{Draw, geometry::V};
use mmorpg_core::{CreatureFamily, EntitySnapshot, NpcRole};
use std::f64::consts::PI;
struct Parts<'a> {
    id: &'a str,
    r: &'a mut Resources,
    draws: Vec<Draw>,
    dead: bool,
}
impl Parts<'_> {
    fn add(&mut self, n: &str, p: Primitive, c: u32, f: Frame) {
        self.draws.push(self.r.primitive(
            format!("{}-{n}", self.id),
            p,
            if self.dead { shade(c, 0.45) } else { c },
            f,
        ));
    }
    fn bx(&mut self, n: &str, s: V, c: u32, f: Frame) {
        self.add(n, Primitive::Box(s), c, f);
    }
}
pub fn accent(e: &EntitySnapshot, f: CreatureFamily) -> u32 {
    if e.flags.tapped_by_other {
        return 0x8b8b8b;
    }
    let i = match f {
        CreatureFamily::Wolf => 0,
        CreatureFamily::Boar => 1,
        CreatureFamily::Vermin => 2,
        CreatureFamily::Marauder => 3,
        CreatureFamily::Mirefin => 4,
        CreatureFamily::Redbrand => 5,
        CreatureFamily::Ogre => 6,
        // Props (#288) keep their own wood colours.
        CreatureFamily::Prop => return 0x8a6a45,
    };
    if e.flags.hostile {
        [
            0xb3533f, 0xb8452f, 0xa84a3a, 0xc23b2e, 0xa55a4c, 0x992d27, 0x8f4a32,
        ][i]
    } else {
        [
            0xc9ab45, 0xc79f2f, 0xbfae5a, 0xd1b04a, 0xb7a553, 0xc2a03c, 0xb39a4a,
        ][i]
    }
}
struct Animal {
    fur: u32,
    dark: u32,
    torso: (V, V),
    legs: [f64; 7],
    head: (V, V),
    snout: (V, V, u32),
    ear: (V, V),
    eye: (V, V),
    collar: (V, V),
    tail: (V, V, u32),
    lift: f64,
}
fn animal_spec(f: CreatureFamily) -> Animal {
    match f {
        CreatureFamily::Boar => Animal {
            fur: 0x6e4d33,
            dark: 0x4b3220,
            torso: ([1.5, 0.5, 1.15], [0., 0.6, -0.15]),
            legs: [0.4, 0.4, 0.35, 0.55, 0.4, -0.55, 0.],
            head: ([0.9, 0.32, 0.5], [0., 0.5, 0.65]),
            snout: ([0.5, 0.18, 0.24], [0., 0.44, 0.88], 0x8d6a5a),
            ear: ([0.2, 0.1, 0.1], [0.35, 0.7, 0.5]),
            eye: ([0.12, 0.06, 0.06], [0.35, 0.57, 0.89]),
            collar: ([1.52, 0.5, 0.1], [0., 0.6, 0.4]),
            tail: ([0.08, 0.08, 0.2], [0., 0.75, -0.72], 0x4b3220),
            lift: 0.8,
        },
        CreatureFamily::Vermin => Animal {
            fur: 0x8a7f73,
            dark: 0x675f56,
            torso: ([1.1, 0.5, 0.9], [0., 0.5, -0.1]),
            legs: [0.3, 0.3, 0.25, 0.4, 0.3, -0.45, 0.],
            head: ([0.7, 0.4, 0.45], [0., 0.58, 0.62]),
            snout: ([0.3, 0.2, 0.25], [0., 0.52, 0.9], 0xc79e94),
            ear: ([0.3, 0.3, 0.1], [0.3, 0.85, 0.5]),
            eye: ([0.1, 0.07, 0.06], [0.28, 0.62, 0.83]),
            collar: ([1., 0.4, 0.1], [0., 0.52, 0.38]),
            tail: ([0.1, 0.1, 0.45], [0., 0.5, -0.55], 0xc79e94),
            lift: 0.6,
        },
        _ => Animal {
            fur: 0x85766a,
            dark: 0x5f544a,
            torso: ([1.15, 0.32, 1.1], [0., 0.62, -0.25]),
            legs: [0.52, 0.52, 0.3, 0.45, 0.35, -0.65, 0.],
            head: ([0.8, 0.28, 0.6], [0., 0.74, 0.55]),
            snout: ([0.4, 0.12, 0.35], [0., 0.68, 0.88], 0x4a4038),
            ear: ([0.2, 0.12, 0.1], [0.28, 0.94, 0.42]),
            eye: ([0.14, 0.06, 0.06], [0.3, 0.77, 0.84]),
            collar: ([0.95, 0.3, 0.12], [0., 0.66, 0.3]),
            tail: ([0.18, 0.12, 0.3], [0., 0.7, -0.7], 0x6e6156),
            lift: 0.65,
        },
    }
}
pub fn animal(
    r: &mut Resources,
    id: &str,
    placement: Frame,
    half: V,
    family: CreatureFamily,
    accent: u32,
    s: Locomotion,
    action: Option<Action>,
    life: Life,
) -> Vec<Draw> {
    let dead = life.dead();
    let fall = life.fall();
    let spec = animal_spec(family);
    let [hx, hy, hz] = half;
    let h = hy * 2.;
    let pivot = spec.legs[0] * h;
    let root = root(
        placement.p,
        2. * placement.q[1].atan2(placement.q[3]),
        hy,
        dead.then(|| {
            (
                tilt(0., PI / 2. * fall, 0.),
                hy + (spec.lift * hx - hy) * fall,
            )
        }),
    );
    let speed = s.forward.hypot(s.right);
    let mov = if dead { 0. } else { s.movement };
    let idle = 1. - mov;
    let run = (speed / 6.3).clamp(0., 1.);
    let phase = s.stride * 2.2;
    let time = if dead { 0. } else { s.time };
    let diagonal = (0.3 + 0.45 * run) * mov * phase.sin();
    let bob = -(0.012 + 0.03 * run) * mov * (1. - (2. * phase).cos()) / 2.;
    let pitch = 0.05 * run * mov * (2. * phase).sin();
    let breathe = (0.02 + 0.015 * (time * 2.1).sin()) * idle;
    let head_pitch = -0.12 * run * mov + 0.05 * idle * (time * 0.7).sin();
    let sway = 0.3 * idle * (time * 1.7).sin() + 0.2 * mov * phase.sin();
    let lift = if dead { 0. } else { 0.15 + 0.45 * run * mov };
    // An attempt lunges: the body pitches in, the head drops and the forelegs reach.
    let lunge = action.map_or(0., |a| weight(a));
    let pitch = pitch + lunge * 0.22;
    let head_pitch = head_pitch - lunge * 0.45;
    let body = child(root, [0., pivot + bob, 0.], tilt(pitch, 0., 0.));
    let at = |p: V, side: f64| [p[0] * side * hx, p[1] * h - pivot, p[2] * hz];
    let size = |s: V| [s[0] * hx, s[1] * h, s[2] * hz];
    let mut p = Parts {
        id,
        r,
        draws: Vec::new(),
        dead,
    };
    let ts = spec.torso.0;
    p.bx(
        "torso",
        size([ts[0] * (1. + breathe), ts[1], ts[2] * (1. + breathe)]),
        spec.fur,
        offset(body, at(spec.torso.1, 1.)),
    );
    let swings = if dead {
        [0.25, 0.1, -0.2, -0.1]
    } else {
        [
            diagonal + lunge * 0.4,
            -diagonal + lunge * 0.4,
            -diagonal,
            diagonal,
        ]
    };
    for (i, name, side, z) in [
        (0, "front-left", 1., spec.legs[4]),
        (1, "front-right", -1., spec.legs[4]),
        (2, "back-left", 1., spec.legs[5]),
        (3, "back-right", -1., spec.legs[5]),
    ] {
        let hip = child(
            body,
            at([spec.legs[3], spec.legs[0], z], side),
            tilt(-swings[i], 0., 0.),
        );
        let length = spec.legs[1] * h;
        p.bx(
            &format!("{name}-leg"),
            [spec.legs[2] * hx, length, spec.legs[2] * hz],
            spec.dark,
            offset(hip, [0., -length / 2., 0.]),
        );
    }
    let head = child(body, at(spec.head.1, 1.), tilt(head_pitch, 0., 0.));
    let relative = |pos: V, side: f64| {
        let a = at(pos, side);
        let b = at(spec.head.1, 1.);
        offset(head, std::array::from_fn(|i| a[i] - b[i]))
    };
    p.bx("head", size(spec.head.0), spec.fur, head);
    p.bx(
        "snout",
        size(spec.snout.0),
        spec.snout.2,
        relative(spec.snout.1, 1.),
    );
    for (name, side) in [("left", 1.), ("right", -1.)] {
        p.bx(
            &format!("ear-{name}"),
            size(spec.ear.0),
            spec.dark,
            relative(spec.ear.1, side),
        );
        p.bx(
            &format!("eye-{name}"),
            size(spec.eye.0),
            accent,
            relative(spec.eye.1, side),
        );
    }
    p.bx(
        "collar",
        size(spec.collar.0),
        accent,
        offset(body, at(spec.collar.1, 1.)),
    );
    let tail = child(body, at(spec.tail.1, 1.), tilt(lift - 0.4, 0., sway));
    p.bx(
        "tail",
        size(spec.tail.0),
        spec.tail.2,
        offset(tail, [0., 0., -spec.tail.0[2] * hz / 2.]),
    );
    if family == CreatureFamily::Boar {
        for (n, s, a, c) in [
            ("bristles", [0.6, 0.14, 0.8], [0., 0.9, -0.25], 0x3a2819),
            ("tusk-left", [0.07, 0.07, 0.18], [0.32, 0.4, 0.95], 0xe6dcc0),
            (
                "tusk-right",
                [0.07, 0.07, 0.18],
                [-0.32, 0.4, 0.95],
                0xe6dcc0,
            ),
        ] {
            p.bx(n, size(s), c, offset(body, at(a, 1.)));
        }
    }
    p.draws
}

/// A Necromancer's summoned skeleton (#295): a deliberately plain, bone-white placeholder
/// humanoid with a sword, slightly hunched, until the owner's undead art exists.
pub fn skeleton(
    r: &mut Resources,
    id: &str,
    placement: Frame,
    half: V,
    loc: Locomotion,
    action: Option<Action>,
    life: Life,
) -> Vec<Draw> {
    const BONE: u32 = 0xd8d0b4;
    const SHADE: u32 = 0x8f8a78;
    humanoid_in(
        r,
        id,
        placement,
        half,
        (BONE, SHADE, BONE, SHADE, Stance::Sword, 0.12),
        (None, None),
        (false, false),
        0x9cff7a,
        loc,
        action,
        life,
    )
}

pub fn humanoid(
    r: &mut Resources,
    id: &str,
    placement: Frame,
    half: V,
    family: Option<CreatureFamily>,
    role: Option<NpcRole>,
    elite: bool,
    shielded: bool,
    accent: u32,
    loc: Locomotion,
    action: Option<Action>,
    life: Life,
) -> Vec<Draw> {
    let palette = match (family, role) {
        (Some(CreatureFamily::Marauder), _) => {
            (0xa07a52, accent, 0x4a4036, 0x2f2923, Stance::Sword, 0.08)
        }
        (Some(CreatureFamily::Mirefin), _) => {
            (0x4d8e88, 0x4d8e88, 0x3a6f6b, 0x2f5a57, Stance::Sword, 0.25)
        }
        (Some(CreatureFamily::Redbrand), _) => {
            (0xd0a27c, accent, 0x3a3330, 0x2a221c, Stance::Sword, 0.02)
        }
        // Large creatures (#297): a hunched, grey-green brute with a club.
        (Some(CreatureFamily::Ogre), _) => {
            (0x7d8a5c, accent, 0x4f4436, 0x34291f, Stance::Sword, 0.18)
        }
        (_, Some(NpcRole::Guard)) => (0xd7a582, 0x7d878c, 0x4a5256, 0x2f2923, Stance::Sword, 0.),
        (_, Some(NpcRole::Vendor)) => (0xd7a582, 0xd9c9a3, 0x5a4632, 0x3a2a1e, Stance::Staff, 0.),
        (_, Some(NpcRole::QuestGiver)) => {
            (0xd7a582, 0x3f66b0, 0x2f3f63, 0x2f2923, Stance::Staff, 0.)
        }
        _ => (0xe2cdb8, 0xf1f1f4, 0xf1f1f4, 0xd8d8de, Stance::Staff, 0.),
    };
    humanoid_in(
        r,
        id,
        placement,
        half,
        palette,
        (family, role),
        (elite, shielded),
        accent,
        loc,
        action,
        life,
    )
}

/// A humanoid in `palette` (skin, tunic, trousers, boots, stance, hunch).
#[allow(clippy::too_many_arguments)]
fn humanoid_in(
    r: &mut Resources,
    id: &str,
    placement: Frame,
    half: V,
    palette: (u32, u32, u32, u32, Stance, f64),
    (family, role): (Option<CreatureFamily>, Option<NpcRole>),
    (elite, shielded): (bool, bool),
    accent: u32,
    loc: Locomotion,
    action: Option<Action>,
    life: Life,
) -> Vec<Draw> {
    let dead = life.dead();
    let fall = life.fall();
    let (skin, tunic, trousers, boots, stance, hunch) = palette;
    let [sx, sy, sz] = [half[0] / 0.3, half[1] * 2. / 1.8, half[2] / 0.3];
    let pose = super::animation::action(
        pose(if dead { Locomotion::default() } else { loc }, stance),
        if dead { None } else { action },
    );
    let rt = root(
        placement.p,
        2. * placement.q[1].atan2(placement.q[3]),
        half[1],
        dead.then(|| {
            (
                tilt(-PI / 2. * fall, 0., 0.),
                half[1] + (0.16 * sz - half[1]) * fall,
            )
        }),
    );
    let mut p = Parts {
        id,
        r,
        draws: Vec::new(),
        dead,
    };
    let pelvis = offset(rt, [0., 0.9 * sy + pose.bob, 0.]);
    for (i, name, side) in [(0, "left", 1.), (1, "right", -1.)] {
        let leg = pose.legs[i];
        let hip = child(
            pelvis,
            [side * 0.1 * sx, 0., 0.],
            tilt(-leg.swing, side * leg.spread, 0.),
        );
        p.bx(
            &format!("{name}-leg"),
            [0.15 * sx, 0.78 * sy, 0.17 * sz],
            trousers,
            offset(hip, [0., -0.39 * sy, 0.]),
        );
        p.bx(
            &format!("{name}-boot"),
            [0.16 * sx, 0.1 * sy, 0.27 * sz],
            boots,
            offset(hip, [0., -0.83 * sy, 0.04 * sz]),
        );
    }
    let spine = child(
        pelvis,
        [0., 0.04 * sy, 0.],
        tilt(pose.lean + hunch, 0., pose.twist),
    );
    let breathe = 1. + pose.breathe;
    p.bx(
        "torso",
        [0.42 * sx * breathe, 0.56 * sy, 0.24 * sz * breathe],
        tunic,
        offset(spine, [0., 0.28 * sy, 0.]),
    );
    let neck = child(spine, [0., 0.58 * sy, 0.], tilt(pose.head_pitch, 0., 0.));
    let head = offset(neck, [0., 0.12 * sy, 0.01 * sz]);
    p.add("head", Primitive::Ball(0.125 * sy), skin, head);
    let mut hands = [rt; 2];
    let mut fore = [rt; 2];
    for (i, name, side) in [(0, "left", 1.), (1, "right", -1.)] {
        let a = pose.arms[i];
        let shoulder = child(
            spine,
            [side * 0.22 * sx, 0.5 * sy, 0.],
            tilt(-a.swing, side * a.spread * 0.4, 0.),
        );
        p.bx(
            &format!("{name}-arm"),
            [0.085 * sx, 0.28 * sy, 0.09 * sz],
            tunic,
            offset(shoulder, [0., -0.14 * sy, 0.]),
        );
        let elbow = child(shoulder, [0., -0.28 * sy, 0.], tilt(-a.elbow, 0., 0.));
        fore[i] = elbow;
        p.bx(
            &format!("{name}-forearm"),
            [0.075 * sx, 0.27 * sy, 0.08 * sz],
            skin,
            offset(elbow, [0., -0.135 * sy, 0.]),
        );
        hands[i] = offset(elbow, [0., -0.29 * sy, 0.]);
        p.add(
            &format!("{name}-hand"),
            Primitive::Ball(0.045 * sy),
            skin,
            hands[i],
        );
    }
    let sword = |p: &mut Parts| {
        p.bx(
            "sword",
            [0.04 * sx, 0.46 * sy, 0.03 * sz],
            0xc3c8cc,
            child(hands[1], [0., -0.2 * sy, 0.05 * sz], tilt(0.35, 0., 0.)),
        )
    };
    let shield = |p: &mut Parts, c| {
        p.add(
            "shield",
            Primitive::Post(0.1 * sx, 0.04),
            c,
            child(
                fore[0],
                [-0.07 * sx, -0.13 * sy, 0.07 * sz],
                tilt(PI / 2., 0., 0.),
            ),
        )
    };
    match (family, role) {
        (Some(CreatureFamily::Marauder), _) => {
            p.bx(
                "snout",
                [0.1 * sx, 0.08 * sy, 0.13 * sz],
                0x6e5236,
                offset(head, [0., -0.04 * sy, 0.13 * sz]),
            );
            for (name, side) in [("left", 1.), ("right", -1.)] {
                p.bx(
                    &format!("ear-{name}"),
                    [0.05 * sx, 0.1 * sy, 0.04 * sz],
                    0x6e5236,
                    child(
                        head,
                        [side * 0.09 * sx, 0.12 * sy, -0.02 * sz],
                        tilt(0., -side * 0.3, 0.),
                    ),
                );
            }
            p.bx(
                "rags",
                [0.3 * sx, 0.3 * sy, 0.03 * sz],
                0x5a4a3a,
                offset(pelvis, [0., -0.17 * sy, 0.1 * sz]),
            );
            p.bx(
                "club",
                [0.06 * sx, 0.5 * sy, 0.06 * sz],
                0x6a4a30,
                child(hands[1], [0., -0.2 * sy, 0.05 * sz], tilt(0.35, 0., 0.)),
            );
            shield(&mut p, 0x6b5a45);
        }
        (Some(CreatureFamily::Ogre), _) => {
            p.bx(
                "club",
                [0.09 * sx, 0.55 * sy, 0.09 * sz],
                0x5a3e28,
                child(hands[1], [0., -0.22 * sy, 0.05 * sz], tilt(0.35, 0., 0.)),
            );
        }
        (Some(CreatureFamily::Mirefin), _) => {
            for (i, (z, h)) in [(0.06, 0.14), (-0.02, 0.19), (-0.1, 0.13)]
                .into_iter()
                .enumerate()
            {
                p.bx(
                    &format!("fin-{i}"),
                    [0.03 * sx, h * sy, 0.07 * sz],
                    0x2f9a8f,
                    child(
                        head,
                        [0., (0.11 + h / 2.) * sy, z * sz],
                        tilt(-0.35, 0., 0.),
                    ),
                );
            }
            for (i, name, side) in [(0, "left", 1.), (1, "right", -1.)] {
                p.bx(
                    &format!("eye-{name}"),
                    [0.045 * sx, 0.045 * sy, 0.04 * sz],
                    accent,
                    offset(head, [side * 0.07 * sx, 0.05 * sy, 0.1 * sz]),
                );
                p.bx(
                    &format!("fin-{name}"),
                    [0.02 * sx, 0.18 * sy, 0.12 * sz],
                    0x2f9a8f,
                    offset(fore[i], [side * 0.03 * sx, -0.1 * sy, 0.]),
                );
            }
            p.bx(
                "harness",
                [0.45 * sx, 0.24 * sy, 0.27 * sz],
                accent,
                offset(spine, [0., 0.36 * sy, 0.]),
            );
            p.bx(
                "loincloth",
                [0.3 * sx, 0.24 * sy, 0.03 * sz],
                accent,
                offset(pelvis, [0., -0.14 * sy, 0.1 * sz]),
            );
        }
        (Some(CreatureFamily::Redbrand), _) => {
            p.bx(
                "bandana",
                [0.27 * sx, 0.07 * sy, 0.27 * sz],
                0xe04a3a,
                offset(head, [0., 0.07 * sy, 0.]),
            );
            p.bx(
                "bandana-knot",
                [0.05 * sx, 0.07 * sy, 0.06 * sz],
                0xe04a3a,
                child(head, [0., 0.06 * sy, -0.15 * sz], tilt(0.4, 0., 0.)),
            );
            p.bx(
                "belt",
                [0.44 * sx, 0.05 * sy, 0.26 * sz],
                0x2a221c,
                offset(spine, [0., 0.02 * sy, 0.]),
            );
            sword(&mut p);
            if shielded {
                // The Redbrand Brute (#160), recoloured: an iron helm and breastplate and a
                // tall iron-rimmed shield held across its front.
                p.bx(
                    "helm",
                    [0.3 * sx, 0.12 * sy, 0.3 * sz],
                    0x5b6067,
                    offset(head, [0., 0.1 * sy, 0.]),
                );
                p.bx(
                    "breastplate",
                    [0.46 * sx, 0.3 * sy, 0.08 * sz],
                    0x5b6067,
                    offset(spine, [0., 0.4 * sy, 0.13 * sz]),
                );
                p.bx(
                    "tower-shield",
                    [0.36 * sx, 0.62 * sy, 0.05 * sz],
                    0x4a2a22,
                    child(
                        fore[0],
                        [-0.05 * sx, -0.1 * sy, 0.14 * sz],
                        tilt(0., 0., 0.),
                    ),
                );
                p.bx(
                    "shield-rim",
                    [0.4 * sx, 0.06 * sy, 0.06 * sz],
                    0x8a9097,
                    child(fore[0], [-0.05 * sx, 0.2 * sy, 0.15 * sz], tilt(0., 0., 0.)),
                );
            } else {
                shield(&mut p, 0x7a2a22);
            }
            if elite {
                p.bx(
                    "cloak",
                    [0.44 * sx, 0.8 * sy, 0.03 * sz],
                    0x5d1717,
                    child(spine, [0., 0.2 * sy, -0.14 * sz], tilt(pose.cape, 0., 0.)),
                );
                for (name, side) in [("left", 1.), ("right", -1.)] {
                    p.bx(
                        &format!("pauldron-{name}"),
                        [0.13 * sx, 0.07 * sy, 0.14 * sz],
                        0x3a3330,
                        offset(spine, [side * 0.25 * sx, 0.57 * sy, 0.]),
                    );
                }
            }
        }
        (_, Some(NpcRole::Guard)) => {
            p.add(
                "helmet",
                Primitive::Ball(0.14 * sy),
                0xa9b1b6,
                offset(head, [0., 0.035 * sy, -0.005 * sz]),
            );
            p.bx(
                "helmet-nasal",
                [0.025 * sx, 0.1 * sy, 0.02 * sz],
                0xa9b1b6,
                offset(head, [0., -0.02 * sy, 0.135 * sz]),
            );
            for (n, z) in [("front", 0.125), ("back", -0.125)] {
                p.bx(
                    &format!("tabard-{n}"),
                    [0.3 * sx, 0.5 * sy, 0.03 * sz],
                    0x3d8a4c,
                    offset(spine, [0., 0.27 * sy, z * sz]),
                );
            }
            for (name, side) in [("left", 1.), ("right", -1.)] {
                p.bx(
                    &format!("pauldron-{name}"),
                    [0.12 * sx, 0.06 * sy, 0.14 * sz],
                    0xa9b1b6,
                    offset(spine, [side * 0.245 * sx, 0.56 * sy, 0.]),
                );
            }
            sword(&mut p);
            shield(&mut p, 0x3d8a4c);
        }
        (_, Some(NpcRole::Vendor)) => {
            p.add(
                "hair",
                Primitive::Ball(0.13 * sy),
                0x5a3b24,
                offset(head, [0., 0.03 * sy, -0.025 * sz]),
            );
            p.bx(
                "apron",
                [0.32 * sx, 0.42 * sy, 0.03 * sz],
                0x7a4f2c,
                offset(spine, [0., 0.22 * sy, 0.125 * sz]),
            );
            p.bx(
                "satchel",
                [0.06 * sx, 0.16 * sy, 0.2 * sz],
                0x6a4a30,
                offset(spine, [-0.24 * sx, 0.1 * sy, 0.]),
            );
            p.bx(
                "strap",
                [0.04 * sx, 0.6 * sy, 0.28 * sz],
                0x6a4a30,
                child(spine, [0., 0.3 * sy, 0.], tilt(0., -0.5, 0.)),
            );
        }
        (_, Some(NpcRole::QuestGiver)) => {
            p.add(
                "hair",
                Primitive::Ball(0.13 * sy),
                0x8a8a8a,
                offset(head, [0., 0.03 * sy, -0.025 * sz]),
            );
            p.bx(
                "cloak",
                [0.44 * sx, 0.7 * sy, 0.03 * sz],
                0x2d4a86,
                child(spine, [0., 0.25 * sy, -0.14 * sz], tilt(pose.cape, 0., 0.)),
            );
            p.bx(
                "sash",
                [0.44 * sx, 0.06 * sy, 0.27 * sz],
                0xd6b45a,
                offset(spine, [0., 0.08 * sy, 0.]),
            );
            p.add(
                "scroll",
                Primitive::Post(0.03 * sx, 0.2 * sy),
                0xe8ddb8,
                child(hands[1], [0., 0., 0.02 * sz], tilt(PI / 2., 0., 0.)),
            );
        }
        _ => {
            p.bx(
                "robe",
                [0.4 * sx, 0.8 * sy, 0.28 * sz],
                0xf7f7fa,
                offset(pelvis, [0., -0.4 * sy, 0.]),
            );
            p.add(
                "hood",
                Primitive::Ball(0.145 * sy),
                0xe4e6ee,
                offset(head, [0., 0.015 * sy, -0.035 * sz]),
            );
            p.bx(
                "sash",
                [0.44 * sx, 0.06 * sy, 0.27 * sz],
                0x9fc4e8,
                offset(spine, [0., 0.08 * sy, 0.]),
            );
            p.add(
                "orb",
                Primitive::Ball(0.05 * sy),
                0xbfe8ff,
                offset(hands[1], [0., -0.03 * sy, 0.01 * sz]),
            );
        }
    }
    p.draws
}
