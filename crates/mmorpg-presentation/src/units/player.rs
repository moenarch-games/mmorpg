//! Current published player silhouettes and class/sex proportions.
use super::{animation::*, kit::*};
use crate::{Draw, geometry::*};
use std::f64::consts::PI;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Hat {
    Helm,
    Cap,
    Hood,
}
#[derive(Clone, Copy, Debug)]
pub struct Look {
    pub female: bool,
    pub stance: Stance,
    pub body: u32,
    pub chest: u32,
    pub shoulder: u32,
    pub cloak: u32,
    pub weapon: u32,
    /// The glowing orb atop a staff.
    pub orb: u32,
    /// A Rogue's dagger (#302): the sword stance with a short blade and no shield.
    pub dagger: bool,
    pub hat: Option<Hat>,
}
impl Look {
    pub fn new(class: u8, female: bool, hat: Option<Hat>, other: bool) -> Self {
        const ARCANE_ORB: u32 = 0x9fe8ff;
        let (stance, body, chest, shoulder, cloak, weapon, orb) = match class {
            1 => (
                Stance::Bow,
                0x748465,
                0x617653,
                0x788868,
                0x465a42,
                0xa88355,
                ARCANE_ORB,
            ),
            2 => (
                Stance::Staff,
                0x777895,
                0x62657e,
                0x8580a0,
                0x4d4d69,
                0xbd895f,
                ARCANE_ORB,
            ),
            // The Necromancer (#301): violet-black robes, a bone-white staff and a green orb.
            3 => (
                Stance::Staff,
                0x3d3548,
                0x2c2536,
                0x4b3e5e,
                0x1f1a28,
                0xe6dfc8,
                0x7dff8a,
            ),
            // The Rogue (#302): charcoal and dark leather, a steel dagger.
            4 => (
                Stance::Sword,
                0x45413d,
                0x3a2e26,
                0x4f4038,
                0x2a2826,
                0xc4c9cc,
                ARCANE_ORB,
            ),
            _ => (
                Stance::Sword,
                0x718d84,
                0x58746b,
                0x6e8f84,
                0x425c56,
                0xd3d6cf,
                ARCANE_ORB,
            ),
        };
        if other {
            Self {
                female: false,
                stance: Stance::Sword,
                body: 0x8b9094,
                chest: 0x767c80,
                shoulder: 0x9aa0a4,
                cloak: 0x5d6468,
                weapon: 0xd3d6cf,
                orb: ARCANE_ORB,
                dagger: false,
                hat: None,
            }
        } else {
            Self {
                female,
                stance,
                body,
                chest,
                shoulder,
                cloak,
                weapon,
                orb,
                dagger: class == 4,
                hat,
            }
        }
    }
}
struct Build {
    pelvis: f64,
    thigh: f64,
    shin: f64,
    torso: f64,
    upper: f64,
    fore: f64,
    shoulder: f64,
    hip: f64,
    waist: f64,
    chest: f64,
    depth: f64,
    head: f64,
    limb: f64,
}
fn build(l: Look) -> Build {
    let (height, span, hip_ratio, cx, cz, hr, arm) = if l.female {
        (1.66, 0.47, 0.94, 0.79, 0.48, 0.33, 0.15)
    } else {
        (1.7, 0.52, 0.72, 0.86, 0.5, 0.34, 0.16)
    };
    let st = height / 1.7;
    let shoulder = 0.23 * (span / 0.52);
    Build {
        pelvis: 0.9 * st,
        thigh: 0.4 * st,
        shin: 0.4 * st,
        torso: 0.46 * st,
        upper: 0.27 * st,
        fore: 0.24 * st,
        shoulder,
        hip: shoulder * hip_ratio * 0.6,
        waist: 0.14 * (cx / 0.86) * (0.82 + 0.2 * hip_ratio),
        chest: shoulder * 1.02,
        depth: 0.12 * (cz / 0.5),
        head: 0.13 * (hr / 0.34),
        limb: arm / 0.16,
    }
}
struct Parts<'a> {
    id: &'a str,
    resources: &'a mut Resources,
    draws: Vec<Draw>,
}
impl Parts<'_> {
    fn part(&mut self, n: &str, s: &str, c: u32, f: Frame, scale: V) {
        self.draws.push(
            self.resources
                .draw(format!("{}-{n}", self.id), s, c, f, scale),
        );
    }
}
const SKIN: u32 = 0xd7a582;
const METAL: u32 = 0xb8bec4;
const DARK_METAL: u32 = 0x6d7378;
const LEATHER: u32 = 0x6a4a30;
const BOOTS: u32 = 0x4c3828;
fn hat(p: &mut Parts, l: Look, head: Frame, r: f64) {
    let hair = if l.female { 0x8a5632 } else { 0x4a3222 };
    match l.hat {
        None => p.part(
            "hair",
            "blob",
            hair,
            offset(head, [0., r * 0.22, -r * 0.12]),
            [r * 1.02, r * 0.9, r * 1.02],
        ),
        Some(Hat::Helm) => {
            p.part(
                "helm",
                "blob",
                METAL,
                offset(head, [0., r * 0.3, -0.005]),
                [r * 1.1, r * 0.95, r * 1.12],
            );
            p.part(
                "helm-rim",
                "post",
                DARK_METAL,
                offset(head, [0., r * 0.12, 0.]),
                [r * 1.12, 0.035, r * 1.14],
            );
            p.part(
                "helm-crest",
                "box",
                0xa8453a,
                offset(head, [0., r * 1.18, -0.01]),
                [0.018, r * 0.4, r * 0.9],
            );
            p.part(
                "helm-nasal",
                "box",
                METAL,
                offset(head, [0., r * 0.05, r * 1.08]),
                [0.014, r * 0.4, 0.012],
            );
        }
        Some(Hat::Cap) => {
            p.part(
                "hair",
                "blob",
                hair,
                offset(head, [0., r * 0.1, -r * 0.2]),
                [r * 1.02, r * 0.85, r],
            );
            p.part(
                "cap-brim",
                "post",
                0x6f875f,
                offset(head, [0., r * 0.45, 0.01]),
                [r * 1.45, 0.02, r * 1.45],
            );
            p.part(
                "cap-crown",
                "cone",
                0x5d7351,
                child(head, [0., r * 0.47, -0.005], tilt(-0.15, 0., 0.)),
                [r * 1.02, r * 1.15, r * 1.02],
            );
            p.part(
                "cap-feather",
                "box",
                0xc8483a,
                child(head, [r * 0.8, r * 0.95, -r * 0.3], tilt(-0.6, 0.4, 0.)),
                [0.01, r * 0.7, 0.02],
            );
        }
        Some(Hat::Hood) => {
            p.part(
                "hood",
                "blob",
                0xb7c6bd,
                offset(head, [0., r * 0.18, -r * 0.14]),
                [r * 1.18, r * 1.12, r * 1.18],
            );
            p.part(
                "hood-peak",
                "cone",
                0xa7b8ae,
                child(head, [0., r * 0.7, -r * 0.45], tilt(-0.9, 0., 0.)),
                [r * 0.6, r, r * 0.6],
            );
            p.part(
                "hood-face",
                "blob",
                shade(SKIN, 0.55),
                offset(head, [0., -r * 0.02, r * 0.6]),
                [r * 0.72, r * 0.8, r * 0.45],
            );
            p.part(
                "hood-face-light",
                "blob",
                SKIN,
                offset(head, [0., -r * 0.08, r * 0.72]),
                [r * 0.6, r * 0.66, r * 0.4],
            );
        }
    }
    if l.female {
        p.part(
            "braid",
            "limb",
            hair,
            child(head, [0., -r * 0.2, -r * 0.85], tilt(0.25, 0., 0.)),
            [0.045, 0.3, 0.04],
        );
        p.part(
            "braid-tie",
            "blob",
            LEATHER,
            offset(head, [0., -r * 0.2 - 0.29, -r * 0.85 - 0.075]),
            [0.035, 0.03, 0.035],
        );
    }
}
pub fn nodes(
    resources: &mut Resources,
    id: &str,
    root: Frame,
    l: Look,
    pose: Pose,
    bow_turn: f64,
) -> Vec<Draw> {
    let b = build(l);
    let mut p = Parts {
        id,
        resources,
        draws: Vec::new(),
    };
    let robe = matches!(l.stance, Stance::Staff);
    let pants = shade(
        if robe { l.chest } else { l.body },
        if robe { 0.8 } else { 0.62 },
    );
    let pelvis = child(root, [0., b.pelvis + pose.bob, 0.], qa(1, pose.hip_yaw));
    let spine = child(pelvis, [0., 0.06, 0.], tilt(pose.lean, 0., pose.twist));
    let neck = child(
        spine,
        [0., b.torso + 0.02, 0.],
        tilt(pose.head_pitch, 0., 0.),
    );
    let head = offset(neck, [0., b.head + 0.05, 0.01]);
    if matches!(l.stance, Stance::Bow) {
        p.draws = super::archer::nodes(
            p.resources,
            id,
            Frame {
                q: qm(root.q, qa(1, bow_turn)),
                ..root
            },
            l,
            pose.bob,
        );
        hat(&mut p, l, head, b.head);
        return p.draws;
    }
    p.part(
        "hips",
        "box",
        pants,
        offset(pelvis, [0., 0.02, 0.]),
        [b.hip + 0.05, 0.1, b.depth * 0.95],
    );
    for (i, side, sign) in [(0, "left", 1.), (1, "right", -1.)] {
        let leg = pose.legs[i];
        let hip = child(
            pelvis,
            [sign * b.hip, -0.02, 0.],
            tilt(-leg.swing, sign * leg.spread, 0.),
        );
        p.part(
            &format!("{side}-thigh"),
            "limb",
            pants,
            hip,
            [0.098 * b.limb, b.thigh, 0.1 * b.limb],
        );
        let knee = child(hip, [0., -b.thigh, 0.], tilt(leg.knee, 0., 0.));
        p.part(
            &format!("{side}-shin"),
            "limb",
            pants,
            knee,
            [0.074 * b.limb, b.shin, 0.078 * b.limb],
        );
        p.part(
            &format!("{side}-boot"),
            "limb",
            BOOTS,
            offset(knee, [0., -b.shin * 0.42, 0.]),
            [0.084 * b.limb, b.shin * 0.58, 0.088 * b.limb],
        );
        let ankle = child(
            knee,
            [0., -b.shin, 0.],
            tilt(leg.foot - leg.knee * 0.25 + leg.swing * 0.2, 0., 0.),
        );
        p.part(
            &format!("{side}-foot"),
            "box",
            BOOTS,
            offset(ankle, [0., -0.04, 0.06]),
            [0.066, 0.045, 0.14],
        );
    }
    let breathe = 1. + pose.breathe;
    p.part(
        "torso",
        &format!("torso-{}", (b.chest / b.waist * 100.).round()),
        l.body,
        spine,
        [
            b.waist / 0.707 * breathe,
            b.torso,
            b.depth / 0.707 * breathe,
        ],
    );
    let depth = b.depth * (1. + (b.chest / b.waist - 1.) * 0.62) * breathe;
    p.part(
        "chest-plate",
        "box",
        l.chest,
        offset(spine, [0., b.torso * 0.62, depth]),
        [b.chest * 0.6, b.torso * 0.24, 0.02],
    );
    p.part(
        "belt",
        "box",
        LEATHER,
        offset(spine, [0., 0.03, 0.]),
        [b.waist + 0.012, 0.035, b.depth + 0.012],
    );
    p.part(
        "buckle",
        "box",
        METAL,
        offset(spine, [0., 0.03, b.depth + 0.012]),
        [0.03, 0.028, 0.01],
    );
    p.part(
        "neck",
        "post",
        SKIN,
        offset(neck, [0., -0.02, 0.]),
        [0.045, 0.08, 0.045],
    );
    p.part(
        "head",
        "blob",
        SKIN,
        head,
        [b.head * 0.92, b.head * 1.05, b.head],
    );
    p.part(
        "nose",
        "box",
        shade(SKIN, 0.9),
        offset(head, [0., -0.01, b.head * 0.98]),
        [0.016, 0.026, 0.022],
    );
    for (side, sign) in [("left", 1.), ("right", -1.)] {
        p.part(
            &format!("eye-{side}"),
            "box",
            0x2a2320,
            offset(head, [sign * b.head * 0.38, b.head * 0.18, b.head * 0.88]),
            [0.014, 0.016, 0.01],
        );
    }
    hat(&mut p, l, head, b.head);
    let mut hands = [root; 2];
    let mut forearms = [root; 2];
    for (i, side, sign) in [(0, "left", 1.), (1, "right", -1.)] {
        let arm = pose.arms[i];
        let shoulder = child(
            spine,
            [sign * b.shoulder, b.torso - 0.06, 0.],
            tilt(-arm.swing, sign * arm.spread, 0.),
        );
        p.part(
            &format!("{side}-upper-arm"),
            "limb",
            l.body,
            shoulder,
            [0.068 * b.limb, b.upper, 0.068 * b.limb],
        );
        let elbow = child(shoulder, [0., -b.upper, 0.], tilt(-arm.elbow, 0., 0.));
        forearms[i] = elbow;
        p.part(
            &format!("{side}-forearm"),
            "limb",
            if robe { l.chest } else { l.body },
            elbow,
            [0.058 * b.limb, b.fore, 0.058 * b.limb],
        );
        p.part(
            &format!("{side}-glove"),
            "limb",
            LEATHER,
            offset(elbow, [0., -b.fore * 0.52, 0.]),
            [0.064 * b.limb, b.fore * 0.48, 0.064 * b.limb],
        );
        let hand = offset(elbow, [0., -b.fore - 0.045, 0.]);
        hands[i] = hand;
        p.part(
            &format!("{side}-hand"),
            "blob",
            SKIN,
            hand,
            [0.052, 0.064, 0.058],
        );
        p.part(
            &format!("{side}-shoulder"),
            "blob",
            l.shoulder,
            offset(spine, [sign * b.shoulder, b.torso - 0.02, 0.]),
            if matches!(l.stance, Stance::Sword) {
                [0.12, 0.085, 0.125]
            } else {
                [0.09, 0.068, 0.095]
            },
        );
    }
    if robe {
        let skirt = child(pelvis, [0., 0.06, 0.], tilt(pose.cape * 0.35, 0., 0.));
        let length = 0.72 * (b.thigh + b.shin) / 0.8;
        p.part(
            "robe",
            "skirt",
            l.chest,
            skirt,
            [b.hip + 0.07, length, b.depth + 0.04],
        );
        p.part(
            "robe-hem",
            "post",
            0xc9a85a,
            offset(skirt, [0., -length, 0.]),
            [(b.hip + 0.07) * 1.46, 0.05, (b.depth + 0.04) * 1.46],
        );
        p.part(
            "sash",
            "box",
            0xc9a85a,
            child(spine, [0.05, b.torso * 0.5, 0.], tilt(0., 0.55, 0.)),
            [0.03, b.torso * 0.62, b.depth * 1.22],
        );
    }
    let cloak = child(
        spine,
        [0., b.torso - 0.02, -b.depth - 0.03],
        tilt(pose.cape, 0., 0.),
    );
    p.part(
        "cloak",
        "box",
        l.cloak,
        offset(cloak, [0., -0.38, 0.]),
        [b.shoulder * 0.9, 0.38, 0.015],
    );
    gear(&mut p, l, root, hands, forearms);
    p.draws
}
fn gear(p: &mut Parts, l: Look, root: Frame, hands: [Frame; 2], fore: [Frame; 2]) {
    match l.stance {
        Stance::Sword => {
            let grip = child(hands[1], [0., -0.01, 0.01], tilt(0.75, 0., 0.));
            p.part("sword-grip", "box", LEATHER, grip, [0.018, 0.018, 0.07]);
            p.part(
                "sword-pommel",
                "blob",
                0xc7b66d,
                offset(grip, [0., 0., -0.08]),
                [0.03; 3],
            );
            p.part(
                "sword-guard",
                "box",
                0xc7b66d,
                offset(grip, [0., 0., 0.075]),
                [0.11, 0.02, 0.022],
            );
            let (reach, half) = if l.dagger { (0.25, 0.17) } else { (0.5, 0.42) };
            p.part(
                "sword-blade",
                "box",
                l.weapon,
                offset(grip, [0., 0., reach]),
                [0.032, 0.007, half],
            );
            if l.dagger {
                return;
            }
            let shield = child(fore[0], [0.07, -0.15, 0.02], tilt(0., -PI / 2., 0.));
            p.part(
                "shield",
                "disc",
                mix(l.chest, 0x2f5f8f, 0.55),
                shield,
                [0.29, 0.04, 0.29],
            );
            p.part(
                "shield-rim",
                "disc",
                DARK_METAL,
                offset(shield, [0., -0.004, 0.]),
                [0.31, 0.03, 0.31],
            );
            p.part(
                "shield-boss",
                "blob",
                METAL,
                offset(shield, [0., 0.045, 0.]),
                [0.07, 0.04, 0.07],
            );
            p.part(
                "shield-band",
                "box",
                0xd8c27a,
                offset(shield, [0., 0.042, 0.]),
                [0.035, 0.004, 0.25],
            );
        }
        Stance::Staff => {
            let staff = Frame {
                p: hands[1].p,
                q: qm(root.q, tilt(0.12, -0.05, 0.)),
            };
            p.part(
                "staff",
                "post",
                l.weapon,
                offset(staff, [0., -0.8, 0.]),
                [0.024, 1.85, 0.024],
            );
            p.part(
                "staff-fitting",
                "post",
                0xc7b66d,
                offset(staff, [0., 0.95, 0.]),
                [0.034, 0.07, 0.034],
            );
            for (n, c, r) in [
                ("staff-orb", l.orb, 0.075),
                ("staff-orb-core", 0xffffff, 0.04),
            ] {
                p.part(n, "blob", c, offset(staff, [0., 1.12, 0.]), [r; 3]);
                if let Some(draw) = p.draws.last_mut() {
                    draw.material.emissive = rgb(c);
                }
            }
        }
        Stance::Bow => {}
    }
}
