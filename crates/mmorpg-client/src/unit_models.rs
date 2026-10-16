//! Low-poly unit models built from yaw-rotated [`SceneBox`] instances.
//!
//! The renderer only draws boxes (position, size, colour, yaw), so a model is
//! a list of parts in the unit's collision-box frame and animation moves parts
//! by translation alone. Every part is placed as a fraction of the unit's full
//! box (`-0.5..=0.5` per axis, +Z facing), so a model fits any template's box.
//! Models draw at most [`MODEL_BOX_BUDGET`] boxes and fit their box within 10%
//! at rest. Walk-cycle phase accumulates distance travelled (`Gait::step`, easing the swing); speed zero is the exact rest pose.
use crate::presentation::SceneBox;
use mmorpg_core::{CreatureFamily, EntityKind, NpcRole};
use std::f32::consts::{PI, TAU};

/// Upper bound of boxes one model draws.
pub const MODEL_BOX_BUDGET: usize = 16;
/// Nose marker in metres: small enough to read as facing, not as a collider.
const NOSE_SIZE: [f32; 3] = [0.14, 0.14, 0.2];
/// A corpse lies flat at this thickness in metres.
pub const CORPSE_HEIGHT: f32 = 0.16;
/// Corpses keep their colour at this brightness.
pub const CORPSE_SHADE: f32 = 0.45;
/// Horizontal speed in m/s below which a unit stands at rest.
const REST_SPEED: f32 = 0.05;
/// Horizontal speed in m/s at which the walk cycle reaches full swing.
const FULL_SWING_SPEED: f32 = 3.0;
/// Swing change per second: the full range takes 0.2 s.
pub const SWING_RATE: f32 = 5.0;
/// Ground covered per full leg cycle, in metres.
const STRIDE_METRES: f32 = 2.5;

const SKIN: [f32; 3] = [0.86, 0.68, 0.52];
const MIREFIN_SKIN: [f32; 3] = [0.38, 0.56, 0.5];
const HAIR: [f32; 3] = [0.25, 0.17, 0.12];
const FACE: [f32; 3] = [0.2, 0.15, 0.13];
const STEEL: [f32; 3] = [0.6, 0.62, 0.66];
const APRON: [f32; 3] = [0.85, 0.78, 0.6];
const SASH: [f32; 3] = [0.92, 0.76, 0.22];
const ROBE: [f32; 3] = [0.82, 0.86, 0.95];
const CLOAK: [f32; 3] = [0.16, 0.1, 0.1];
const TUSK: [f32; 3] = [0.92, 0.9, 0.82];
const BOOTS: f32 = 0.3;
const TROUSERS: f32 = 0.55;

/// Which humanoid is drawn: accessories and skin differ, the frame does not.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HumanoidKind {
    Player,
    Marauder,
    /// Gains a fin crest.
    Mirefin,
    Redbrand,
    /// The elite Redbrand: a darker cloak.
    Garrick,
    /// Helmeted guard.
    Guard,
    /// Apron.
    Vendor,
    /// Sash.
    QuestGiver,
    /// Robe.
    SpiritHealer,
}

/// Which quadruped is drawn.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AnimalKind {
    /// Bushy tail.
    Wolf,
    /// Tusks instead of a tail.
    Boar,
    /// Thin long tail.
    Vermin,
}

/// The model a unit is drawn with.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnitModel {
    Humanoid(HumanoidKind),
    Quadruped(AnimalKind),
    /// Unknown template: a single box with a nose.
    Placeholder,
}

/// Walk-cycle state: `phase` in radians, `swing` in `0..=1` (0 is the rest pose).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gait {
    pub phase: f32,
    pub swing: f32,
}

impl Gait {
    pub const IDLE: Self = Self {
        phase: 0.0,
        swing: 0.0,
    };

    /// Phase (radians) after covering `speed` m/s for `dt` seconds: it advances
    /// with distance travelled, so a speed change never makes it jump.
    /// Below the rest threshold the phase is held.
    #[must_use]
    pub fn advance(phase: f32, speed: f32, dt: f32) -> f32 {
        if speed.is_nan() || speed < REST_SPEED || dt.is_nan() || dt <= 0.0 {
            return phase;
        }
        (phase + dt * speed * TAU / STRIDE_METRES).rem_euclid(TAU)
    }

    /// Next gait state: the swing eases toward its target for `speed` at
    /// `SWING_RATE`, and the phase keeps advancing while any swing remains,
    /// so limbs settle instead of snapping to or from the rest pose.
    #[must_use]
    pub fn step(self, speed: f32, dt: f32) -> Self {
        let speed = if speed.is_nan() || speed < REST_SPEED {
            0.0
        } else {
            speed
        };
        let dt = if dt.is_nan() { 0.0 } else { dt.max(0.0) };
        let target = (speed / FULL_SWING_SPEED).clamp(0.0, 1.0);
        let max_delta = SWING_RATE * dt;
        let swing = self.swing + (target - self.swing).clamp(-max_delta, max_delta);
        let phase = if swing > 0.0 {
            let effective = speed.max(swing * FULL_SWING_SPEED);
            (self.phase + dt * effective * TAU / STRIDE_METRES).rem_euclid(TAU)
        } else {
            self.phase
        };
        Self { phase, swing }
    }

    /// Gait from an accumulated phase and the current horizontal speed (m/s);
    /// the swing grows with speed and speed zero is the exact rest pose.
    #[must_use]
    pub fn new(speed: f32, phase: f32) -> Self {
        if speed.is_nan() || speed < REST_SPEED {
            return Self::IDLE;
        }
        Self {
            phase,
            swing: (speed / FULL_SWING_SPEED).clamp(0.0, 1.0),
        }
    }
}

/// Where and how large a unit stands, in metres. `centre` is the body centre
/// and `half` the collision box half extents.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub centre: [f32; 3],
    pub half: [f32; 3],
    pub yaw: f32,
}

#[derive(Clone, Copy)]
enum Motion {
    Still,
    /// Leg swing; the sign picks the diagonal pair.
    Leg(f32),
    /// Arm swing; the sign picks the side.
    Arm(f32),
    /// Torso and head bob twice per cycle.
    Bob,
}

#[derive(Clone, Copy)]
struct Part {
    centre: [f32; 3],
    size: [f32; 3],
    color: [f32; 3],
    motion: Motion,
}

const fn part(centre: [f32; 3], size: [f32; 3], color: [f32; 3], motion: Motion) -> Part {
    Part {
        centre,
        size,
        color,
        motion,
    }
}

fn shade(color: [f32; 3], factor: f32) -> [f32; 3] {
    color.map(|channel| channel * factor)
}

impl UnitModel {
    /// Chooses the model of a unit. `family` and `elite` describe a creature's
    /// template (`None` when the template is unknown), `role` an NPC's.
    #[must_use]
    pub fn choose(
        kind: EntityKind,
        family: Option<CreatureFamily>,
        elite: bool,
        role: Option<NpcRole>,
    ) -> Self {
        match kind {
            EntityKind::Player => Self::Humanoid(HumanoidKind::Player),
            EntityKind::Npc => Self::Humanoid(match role {
                Some(NpcRole::Guard) => HumanoidKind::Guard,
                Some(NpcRole::Vendor) => HumanoidKind::Vendor,
                Some(NpcRole::SpiritHealer) => HumanoidKind::SpiritHealer,
                Some(NpcRole::QuestGiver) | None => HumanoidKind::QuestGiver,
            }),
            EntityKind::Creature => match family {
                None => Self::Placeholder,
                Some(CreatureFamily::Wolf) => Self::Quadruped(AnimalKind::Wolf),
                Some(CreatureFamily::Boar) => Self::Quadruped(AnimalKind::Boar),
                Some(CreatureFamily::Vermin) => Self::Quadruped(AnimalKind::Vermin),
                Some(CreatureFamily::Marauder) => Self::Humanoid(HumanoidKind::Marauder),
                Some(CreatureFamily::Mirefin) => Self::Humanoid(HumanoidKind::Mirefin),
                Some(CreatureFamily::Redbrand) if elite => Self::Humanoid(HumanoidKind::Garrick),
                Some(CreatureFamily::Redbrand) => Self::Humanoid(HumanoidKind::Redbrand),
            },
        }
    }

    fn parts(self, color: [f32; 3]) -> Vec<Part> {
        match self {
            Self::Humanoid(kind) => humanoid_parts(kind, color),
            Self::Quadruped(kind) => quadruped_parts(kind, color),
            Self::Placeholder => Vec::new(),
        }
    }

    fn is_humanoid(self) -> bool {
        matches!(self, Self::Humanoid(_))
    }
}

fn humanoid_parts(kind: HumanoidKind, clothing: [f32; 3]) -> Vec<Part> {
    use HumanoidKind as K;
    use Motion::{Arm, Bob, Leg};
    let skin = if kind == K::Mirefin {
        MIREFIN_SKIN
    } else {
        SKIN
    };
    let trousers = shade(clothing, TROUSERS);
    let mut parts = Vec::with_capacity(MODEL_BOX_BUDGET);
    for (x, side) in [(-0.2, 1.0), (0.2, -1.0)] {
        parts.push(part(
            [x, -0.28, 0.0],
            [0.3, 0.44, 0.34],
            trousers,
            Leg(side),
        ));
        parts.push(part(
            [x, -0.46, 0.03],
            [0.32, 0.08, 0.4],
            shade(clothing, BOOTS),
            Leg(side),
        ));
    }
    parts.push(part([0.0, 0.08, 0.0], [0.7, 0.4, 0.5], clothing, Bob));
    parts.push(if kind == K::SpiritHealer {
        // The robe hangs over the legs.
        part([0.0, -0.2, 0.0], [0.72, 0.56, 0.54], ROBE, Bob)
    } else {
        part(
            [0.0, -0.04, 0.0],
            [0.74, 0.07, 0.54],
            shade(clothing, 0.4),
            Bob,
        )
    });
    for (x, side) in [(-0.42, -1.0), (0.42, 1.0)] {
        parts.push(part([x, 0.22, 0.0], [0.16, 0.12, 0.3], clothing, Arm(side)));
        parts.push(part([x, 0.02, 0.0], [0.14, 0.4, 0.24], clothing, Arm(side)));
        parts.push(part([x, -0.22, 0.0], [0.15, 0.1, 0.26], skin, Arm(side)));
    }
    parts.push(part([0.0, 0.39, 0.0], [0.5, 0.22, 0.5], skin, Bob));
    // The face plate shows the facing instead of a nose.
    parts.push(part([0.0, 0.39, 0.26], [0.34, 0.14, 0.04], FACE, Bob));
    parts.push(match kind {
        K::Guard => part([0.0, 0.45, 0.0], [0.56, 0.14, 0.56], STEEL, Bob),
        K::Mirefin => part(
            [0.0, 0.46, -0.04],
            [0.08, 0.16, 0.4],
            shade(MIREFIN_SKIN, 0.7),
            Bob,
        ),
        _ => part([0.0, 0.47, -0.02], [0.54, 0.08, 0.54], HAIR, Bob),
    });
    parts.push(match kind {
        K::Vendor => part([0.0, 0.0, 0.27], [0.5, 0.5, 0.03], APRON, Bob),
        K::QuestGiver => part([0.0, 0.12, 0.0], [0.76, 0.09, 0.54], SASH, Bob),
        K::Garrick => part([0.0, 0.05, -0.27], [0.7, 0.7, 0.04], CLOAK, Bob),
        _ => part(
            [0.0, 0.12, 0.26],
            [0.5, 0.2, 0.03],
            shade(clothing, 1.25),
            Bob,
        ),
    });
    parts
}

fn quadruped_parts(kind: AnimalKind, body: [f32; 3]) -> Vec<Part> {
    use Motion::{Bob, Leg, Still};
    let legs = shade(body, 0.7);
    let mut parts = Vec::with_capacity(MODEL_BOX_BUDGET);
    // Diagonal pairs trot together.
    for (x, z, pair) in [
        (-0.18, 0.28, 1.0),
        (0.18, 0.28, -1.0),
        (-0.18, -0.3, -1.0),
        (0.18, -0.3, 1.0),
    ] {
        parts.push(part([x, -0.31, z], [0.16, 0.38, 0.16], legs, Leg(pair)));
    }
    parts.push(part([0.0, 0.08, -0.05], [0.55, 0.4, 0.6], body, Bob));
    parts.push(part(
        [0.0, 0.18, 0.33],
        [0.4, 0.34, 0.3],
        shade(body, 0.9),
        Bob,
    ));
    parts.push(part(
        [0.0, 0.1, 0.47],
        [0.2, 0.14, 0.14],
        shade(body, 0.55),
        Bob,
    ));
    for x in [-0.14, 0.14] {
        parts.push(part(
            [x, 0.38, 0.28],
            [0.1, 0.12, 0.08],
            shade(body, 0.8),
            Bob,
        ));
    }
    match kind {
        AnimalKind::Wolf => parts.push(part(
            [0.0, 0.12, -0.45],
            [0.12, 0.12, 0.2],
            shade(body, 0.8),
            Bob,
        )),
        AnimalKind::Boar => {
            for x in [-0.1, 0.1] {
                parts.push(part([x, 0.04, 0.48], [0.04, 0.04, 0.12], TUSK, Bob));
            }
        }
        AnimalKind::Vermin => parts.push(part(
            [0.0, -0.2, -0.38],
            [0.05, 0.05, 0.34],
            shade(body, 0.6),
            Still,
        )),
    }
    parts
}

/// Appends the boxes of a unit: its model standing or, when `corpse`, lying
/// flat. Corpses are darkened and at most [`CORPSE_HEIGHT`] tall.
pub fn append_unit(
    boxes: &mut Vec<SceneBox>,
    model: UnitModel,
    color: [f32; 3],
    placement: Placement,
    gait: Gait,
    corpse: bool,
) {
    let Placement { centre, half, yaw } = placement;
    let full = half.map(|value| value * 2.0);
    let feet = centre[1] - half[1];
    let (sin, cos) = yaw.sin_cos();
    // Local metres (right, up from the body centre, forward) to the world.
    let place = |local: [f32; 3]| {
        [
            centre[0] + local[0] * cos + local[2] * sin,
            centre[1] + local[1],
            centre[2] + local[2] * cos - local[0] * sin,
        ]
    };
    if model == UnitModel::Placeholder {
        if corpse {
            boxes.push(SceneBox {
                position: [centre[0], feet + CORPSE_HEIGHT / 2.0, centre[2]],
                size: [full[0], CORPSE_HEIGHT, full[1]],
                color: shade(color, CORPSE_SHADE),
                yaw,
            });
            return;
        }
        boxes.push(SceneBox {
            position: centre,
            size: full,
            color,
            yaw,
        });
        // The nose touches the body's front face: half depth plus half nose depth.
        let reach = (full[2] + NOSE_SIZE[2]) / 2.0;
        boxes.push(SceneBox {
            position: place([0.0, half[1] * 0.5, reach]),
            size: NOSE_SIZE,
            color: shade(color, 0.45),
            yaw,
        });
        return;
    }
    for part in model.parts(color) {
        let (local, size, tint) = if corpse {
            let (lying, thickness) = lay_flat(part, model.is_humanoid(), full);
            (
                [lying[0], lying[1] - half[1], lying[2]],
                thickness,
                shade(part.color, CORPSE_SHADE),
            )
        } else {
            let (shift, lift) = motion_offsets(part.motion, gait);
            (
                [
                    part.centre[0] * full[0],
                    (part.centre[1] + lift) * full[1],
                    (part.centre[2] + shift) * full[2],
                ],
                [
                    part.size[0] * full[0],
                    part.size[1] * full[1],
                    part.size[2] * full[2],
                ],
                part.color,
            )
        };
        boxes.push(SceneBox {
            position: place(local),
            size,
            color: tint,
            yaw,
        });
    }
}

/// Fractional (forward shift of depth, lift of height) of a moving part.
fn motion_offsets(motion: Motion, gait: Gait) -> (f32, f32) {
    let Gait { phase, swing } = gait;
    match motion {
        Motion::Still => (0.0, 0.0),
        Motion::Leg(side) => {
            let p = if side < 0.0 { phase + PI } else { phase };
            (p.sin() * 0.16 * swing, p.cos().max(0.0) * 0.05 * swing)
        }
        Motion::Arm(side) => {
            let p = if side < 0.0 { phase } else { phase + PI };
            (p.sin() * 0.1 * swing, 0.0)
        }
        Motion::Bob => (0.0, (0.5 + 0.5 * (2.0 * phase).cos()) * 0.015 * swing),
    }
}

/// A part laid flat on the ground: (centre with y measured from the feet,
/// size). Humanoids lie on their back with the head forward, so their height
/// becomes length; animals already extend along the facing and only flatten.
fn lay_flat(part: Part, humanoid: bool, full: [f32; 3]) -> ([f32; 3], [f32; 3]) {
    let [cx, cy, cz] = part.centre;
    let [sx, sy, sz] = part.size;
    let (forward, length, thin_centre, thin_size) = if humanoid {
        (cy * full[1], sy * full[1], cz, sz)
    } else {
        (cz * full[2], sz * full[2], cy, sy)
    };
    let low =
        ((thin_centre + 0.5 - thin_size / 2.0) * CORPSE_HEIGHT).clamp(0.0, CORPSE_HEIGHT - 0.02);
    let high =
        ((thin_centre + 0.5 + thin_size / 2.0) * CORPSE_HEIGHT).clamp(low + 0.02, CORPSE_HEIGHT);
    (
        [cx * full[0], (low + high) / 2.0, forward],
        [sx * full[0], high - low, length],
    )
}
