//! Imported Quaternius glTF units: material colours baked into vertex colours, clips sampled and
//! skinned on the CPU, then submitted as ordinary scene draws so rendering and picking agree.
//! Presentation only; the authoritative body box still comes from content.
use super::kit::{Frame, model};
use crate::world::{DrawMaterial, SceneDraw};
use std::{
    collections::BTreeMap,
    sync::{Arc, OnceLock},
};
use three_d_core::{Color3, Mesh, Vec3, VertexAttributes};

type M4 = [f32; 16];
const IDENTITY: M4 = [
    1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
];

fn mul(a: &M4, b: &M4) -> M4 {
    std::array::from_fn(|i| {
        let (c, r) = (i / 4, i % 4);
        (0..4).map(|k| a[k * 4 + r] * b[c * 4 + k]).sum()
    })
}

fn trs(t: [f32; 3], q: [f32; 4], s: [f32; 3]) -> M4 {
    let [x, y, z, w] = q;
    [
        (1. - 2. * (y * y + z * z)) * s[0],
        2. * (x * y + z * w) * s[0],
        2. * (x * z - y * w) * s[0],
        0.,
        2. * (x * y - z * w) * s[1],
        (1. - 2. * (x * x + z * z)) * s[1],
        2. * (y * z + x * w) * s[1],
        0.,
        2. * (x * z + y * w) * s[2],
        2. * (y * z - x * w) * s[2],
        (1. - 2. * (x * x + y * y)) * s[2],
        0.,
        t[0],
        t[1],
        t[2],
        1.,
    ]
}

fn point(m: &M4, p: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|r| m[r] * p[0] + m[4 + r] * p[1] + m[8 + r] * p[2] + m[12 + r])
}

fn vector(m: &M4, v: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|r| m[r] * v[0] + m[4 + r] * v[1] + m[8 + r] * v[2])
}

/// glTF base colours are linear; the native mesh path expects sRGB vertex colours.
fn srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1. / 2.4) - 0.055
    }
}

#[derive(Clone, Copy)]
struct Rest {
    t: [f32; 3],
    r: [f32; 4],
    s: [f32; 3],
}

#[derive(Clone, Copy)]
enum Path {
    Translation,
    Rotation,
    Scale,
}

struct Channel {
    node: usize,
    path: Path,
    step: bool,
    times: Vec<f32>,
    values: Vec<[f32; 4]>,
}

struct Clip {
    duration: f32,
    channels: Vec<Channel>,
}

impl Channel {
    fn sample(&self, t: f32) -> [f32; 4] {
        let last = self.times.len() - 1;
        let k = self.times.partition_point(|&x| x <= t).saturating_sub(1);
        if k >= last || self.step {
            return self.values[k.min(last)];
        }
        let (a, b) = (self.values[k], self.values[k + 1]);
        let span = self.times[k + 1] - self.times[k];
        let u = if span > 0. {
            ((t - self.times[k]) / span).clamp(0., 1.)
        } else {
            0.
        };
        if let Path::Rotation = self.path {
            // Normalised lerp along the shorter arc.
            let sign = if a.iter().zip(&b).map(|(x, y)| x * y).sum::<f32>() < 0. {
                -1.
            } else {
                1.
            };
            let q: [f32; 4] = std::array::from_fn(|i| a[i] + (b[i] * sign - a[i]) * u);
            let len = q.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-6);
            q.map(|v| v / len)
        } else {
            std::array::from_fn(|i| a[i] + (b[i] - a[i]) * u)
        }
    }
}

struct Primitive {
    double_sided: bool,
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<Color3>,
    indices: Vec<u32>,
    /// Up to four (slot, weight) influences per vertex.
    influences: Vec<[(u16, f32); 4]>,
    /// Influence slots: driving node and its inverse bind matrix.
    slots: Vec<(usize, M4)>,
}

/// One imported model: rest hierarchy, skinned primitives and named clips.
pub(super) struct Model {
    rest: Vec<Rest>,
    parents: Vec<Option<usize>>,
    /// Nodes ordered parents first.
    order: Vec<usize>,
    primitives: Vec<Primitive>,
    clips: BTreeMap<String, Clip>,
    /// Rest-clip bounds: lowest point and height along +Y, horizontal centre.
    pub floor: f32,
    pub height: f32,
    pub centre: [f32; 2],
    /// The `Head` joint, and the rest head centre's offset from it with the head's radius.
    head: Option<(usize, [f32; 3], f32)>,
}

impl Model {
    pub fn parse(bytes: &[u8], rest_clip: &str) -> Result<Self, gltf::Error> {
        let (document, buffers, _) = gltf::import_slice(bytes)?;
        let count = document.nodes().len();
        let mut rest = Vec::with_capacity(count);
        let mut parents = vec![None; count];
        for node in document.nodes() {
            let (t, r, s) = node.transform().decomposed();
            rest.push(Rest { t, r, s });
            for child in node.children() {
                parents[child.index()] = Some(node.index());
            }
        }
        let mut order = Vec::with_capacity(count);
        let mut stack: Vec<_> = (0..count).filter(|&n| parents[n].is_none()).collect();
        stack.reverse();
        while let Some(n) = stack.pop() {
            order.push(n);
            let node = document.nodes().nth(n).expect("node index in range");
            stack.extend(node.children().map(|c| c.index()));
        }
        let mut primitives = Vec::new();
        for node in document.nodes() {
            let Some(mesh) = node.mesh() else { continue };
            let slots: Vec<(usize, M4)> = match node.skin() {
                Some(skin) => {
                    let reader = skin.reader(|b| Some(&buffers[b.index()]));
                    let inverse: Vec<M4> = reader.read_inverse_bind_matrices().map_or_else(
                        || vec![IDENTITY; skin.joints().count()],
                        |m| {
                            m.map(|c| std::array::from_fn(|i| c[i / 4][i % 4]))
                                .collect()
                        },
                    );
                    skin.joints().map(|j| j.index()).zip(inverse).collect()
                }
                None => vec![(node.index(), IDENTITY)],
            };
            let skinned = node.skin().is_some();
            for primitive in mesh.primitives() {
                let reader = primitive.reader(|b| Some(&buffers[b.index()]));
                let Some(positions) = reader.read_positions() else {
                    continue;
                };
                let positions: Vec<[f32; 3]> = positions.collect();
                let normals: Vec<[f32; 3]> = reader
                    .read_normals()
                    .map_or_else(|| vec![[0., 1., 0.]; positions.len()], Iterator::collect);
                let base = primitive
                    .material()
                    .pbr_metallic_roughness()
                    .base_color_factor();
                let colors =
                    vec![Color3::new(srgb(base[0]), srgb(base[1]), srgb(base[2])); positions.len()];
                let indices: Vec<u32> = reader.read_indices().map_or_else(
                    || (0..u32::try_from(positions.len()).unwrap_or(0)).collect(),
                    |i| i.into_u32().collect(),
                );
                let influences = match (skinned, reader.read_joints(0), reader.read_weights(0)) {
                    (true, Some(joints), Some(weights)) => joints
                        .into_u16()
                        .zip(weights.into_f32())
                        .map(|(j, w)| std::array::from_fn(|i| (j[i], w[i])))
                        .collect(),
                    _ => vec![[(0, 1.), (0, 0.), (0, 0.), (0, 0.)]; positions.len()],
                };
                primitives.push(Primitive {
                    double_sided: primitive.material().double_sided(),
                    positions,
                    normals,
                    colors,
                    indices,
                    influences,
                    slots: slots.clone(),
                });
            }
        }
        let mut clips = BTreeMap::new();
        for animation in document.animations() {
            let mut channels = Vec::new();
            let mut duration = 0_f32;
            for channel in animation.channels() {
                let reader = channel.reader(|b| Some(&buffers[b.index()]));
                let (Some(times), Some(outputs)) = (reader.read_inputs(), reader.read_outputs())
                else {
                    continue;
                };
                let times: Vec<f32> = times.collect();
                use gltf::animation::util::ReadOutputs as O;
                let (path, mut values): (Path, Vec<[f32; 4]>) = match outputs {
                    O::Translations(v) => (
                        Path::Translation,
                        v.map(|[x, y, z]| [x, y, z, 0.]).collect(),
                    ),
                    O::Scales(v) => (Path::Scale, v.map(|[x, y, z]| [x, y, z, 0.]).collect()),
                    O::Rotations(v) => (Path::Rotation, v.into_f32().collect()),
                    O::MorphTargetWeights(_) => continue,
                };
                let interpolation = channel.sampler().interpolation();
                if interpolation == gltf::animation::Interpolation::CubicSpline {
                    // Keep each key's value; tangents are dropped for linear playback.
                    values = values.chunks_exact(3).map(|k| k[1]).collect();
                }
                if times.is_empty() || values.len() < times.len() {
                    continue;
                }
                duration = duration.max(*times.last().unwrap_or(&0.));
                channels.push(Channel {
                    node: channel.target().node().index(),
                    path,
                    step: interpolation == gltf::animation::Interpolation::Step,
                    times,
                    values,
                });
            }
            let name = animation.name().unwrap_or_default();
            // Blender prefixes clips with their armature ("AnimalArmature|Walk").
            let name = name.rsplit('|').next().unwrap_or(name).to_owned();
            clips.insert(name, Clip { duration, channels });
        }
        let mut model = Self {
            rest,
            parents,
            order,
            primitives,
            clips,
            floor: 0.,
            height: 1.,
            centre: [0.; 2],
            head: None,
        };
        let (mut min, mut max) = ([f32::MAX; 3], [f32::MIN; 3]);
        for p in model.skin(rest_clip, 0.).iter().flat_map(|(p, _)| p) {
            for i in 0..3 {
                min[i] = min[i].min(p[i]);
                max[i] = max[i].max(p[i]);
            }
        }
        if min[1] < max[1] {
            model.floor = min[1];
            model.height = max[1] - min[1];
            model.centre = [(min[0] + max[0]) * 0.5, (min[2] + max[2]) * 0.5];
        }
        model.head = model.rest_head(&document, rest_clip);
        Ok(model)
    }

    /// Vertices mostly bound to the `Head` joint, measured in the rest pose.
    fn rest_head(&self, document: &gltf::Document, clip: &str) -> Option<(usize, [f32; 3], f32)> {
        let joint = document.nodes().find(|n| n.name() == Some("Head"))?.index();
        let (mut min, mut max) = ([f32::MAX; 3], [f32::MIN; 3]);
        for ((positions, _), p) in self.skin(clip, 0.).iter().zip(&self.primitives) {
            for (q, weights) in positions.iter().zip(&p.influences) {
                let main = weights.iter().max_by(|a, b| a.1.total_cmp(&b.1))?;
                if p.slots.get(usize::from(main.0)).map(|s| s.0) != Some(joint) {
                    continue;
                }
                for i in 0..3 {
                    min[i] = min[i].min(q[i]);
                    max[i] = max[i].max(q[i]);
                }
            }
        }
        if min[0] > max[0] {
            return None;
        }
        let at = self.globals(clip, 0.)[joint];
        let centre: [f32; 3] = std::array::from_fn(|i| (min[i] + max[i]) * 0.5);
        let radius = (0..3).map(|i| (max[i] - min[i]) * 0.5).fold(0., f32::max);
        Some((
            joint,
            std::array::from_fn(|i| centre[i] - at[12 + i]),
            radius,
        ))
    }

    /// The posed head centre and radius in model space, following the head joint's position.
    pub fn head(&self, clip: &str, time: f32) -> Option<([f32; 3], f32)> {
        let (joint, offset, radius) = self.head?;
        let at = self.globals(clip, time)[joint];
        Some((std::array::from_fn(|i| at[12 + i] + offset[i]), radius))
    }

    /// Model-to-world placement: scaled to `height` metres, lowest rest point on `root`.
    pub fn placement(&self, root: Frame, height: f64) -> three_d_animation::Mat4 {
        let scale = height / f64::from(self.height);
        let mut placement = model(root, [scale; 3]);
        let offset = [-self.centre[0], -self.floor, -self.centre[1]].map(f64::from);
        let m = &mut placement.elements;
        for r in 0..3 {
            let shift: f64 = (0..3).map(|k| f64::from(m[k * 4 + r]) * offset[k]).sum();
            m[12 + r] += shift as f32;
        }
        placement
    }

    pub fn duration(&self, clip: &str) -> f32 {
        self.clips.get(clip).map_or(0., |c| c.duration)
    }

    fn globals(&self, clip: &str, time: f32) -> Vec<M4> {
        let mut local = self.rest.clone();
        if let Some(clip) = self.clips.get(clip) {
            for channel in &clip.channels {
                let v = channel.sample(time);
                let r = &mut local[channel.node];
                match channel.path {
                    Path::Translation => r.t = [v[0], v[1], v[2]],
                    Path::Rotation => r.r = v,
                    Path::Scale => r.s = [v[0], v[1], v[2]],
                }
            }
        }
        let mut globals = vec![IDENTITY; self.rest.len()];
        for &n in &self.order {
            let l = trs(local[n].t, local[n].r, local[n].s);
            globals[n] = self.parents[n].map_or(l, |p| mul(&globals[p], &l));
        }
        globals
    }

    /// Skinned positions and normals per primitive in model space.
    fn skin(&self, clip: &str, time: f32) -> Vec<(Vec<[f32; 3]>, Vec<[f32; 3]>)> {
        let globals = self.globals(clip, time);
        self.primitives
            .iter()
            .map(|p| {
                let slots: Vec<M4> = p
                    .slots
                    .iter()
                    .map(|(node, inverse)| mul(&globals[*node], inverse))
                    .collect();
                p.positions
                    .iter()
                    .zip(&p.normals)
                    .zip(&p.influences)
                    .map(|((pos, normal), weights)| {
                        let (mut q, mut n) = ([0.; 3], [0.; 3]);
                        for &(slot, w) in weights {
                            if w <= 0. {
                                continue;
                            }
                            let m = &slots[usize::from(slot).min(slots.len() - 1)];
                            let (a, b) = (point(m, *pos), vector(m, *normal));
                            for i in 0..3 {
                                q[i] += a[i] * w;
                                n[i] += b[i] * w;
                            }
                        }
                        let len = n.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-6);
                        (q, n.map(|v| v / len))
                    })
                    .unzip()
            })
            .collect()
    }

    /// Draws for one posed instance standing at `root`, scaled to `height` metres.
    pub fn draws(
        &self,
        id: &str,
        root: Frame,
        height: f64,
        clip: &str,
        time: f32,
        tint: Option<f32>,
    ) -> Vec<SceneDraw> {
        let placement = self.placement(root, height);
        self.skin(clip, time)
            .into_iter()
            .zip(&self.primitives)
            .enumerate()
            .filter_map(|(index, ((positions, normals), p))| {
                let colors = match tint {
                    Some(f) => p
                        .colors
                        .iter()
                        .map(|c| Color3::new(c.r * f, c.g * f, c.b * f))
                        .collect(),
                    None => p.colors.clone(),
                };
                let mesh = Mesh::with_attributes(
                    positions
                        .iter()
                        .map(|v| Vec3::new(v[0], v[1], v[2]))
                        .collect(),
                    p.indices.clone(),
                    VertexAttributes {
                        normals: Some(
                            normals
                                .iter()
                                .map(|v| Vec3::new(v[0], v[1], v[2]))
                                .collect(),
                        ),
                        colors: Some(colors),
                        ..Default::default()
                    },
                )
                .ok()?;
                Some(SceneDraw {
                    id: format!("{id}-mesh-{index}"),
                    resource_key: format!("skinned:{id}:{index}"),
                    mesh: Arc::new(mesh),
                    model: placement,
                    material: DrawMaterial {
                        color: [1.; 3],
                        emissive: [0.; 3],
                        opacity: 1.,
                        unlit: false,
                        double_sided: p.double_sided,
                    },
                })
            })
            .collect()
    }
}

/// Clip names for one model's presentation states.
pub(super) struct Clips {
    pub idle: &'static str,
    pub walk: &'static str,
    pub run: &'static str,
    pub attack: &'static str,
    pub death: &'static str,
}

pub(super) struct Asset {
    pub model: Model,
    pub clips: Clips,
}

/// Every bundled Quaternius model (CC0); see `assets/quaternius/PROVENANCE.md`.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum Kind {
    KnightMale,
    KnightFemale,
    Elf,
    NinjaFemale,
    Wizard,
    Witch,
    VikingMale,
    VikingFemale,
    GoblinMale,
    GoblinFemale,
    PirateMale,
    PirateFemale,
    SoldierMale,
    SoldierFemale,
    WorkerMale,
    WorkerFemale,
    CasualMale,
    CasualFemale,
    Wolf,
    Rat,
    Pig,
}

macro_rules! bundled {
    ($file:literal) => {
        include_bytes!(concat!("../../../../assets/quaternius/", $file)).as_slice()
    };
}

/// Shared clips of the Ultimate Animated Character pack.
const fn humanoid(attack: &'static str) -> Clips {
    Clips {
        idle: "Idle",
        walk: "Walk",
        run: "Run",
        attack,
        death: "Death",
    }
}

impl Kind {
    const ALL: [Self; 21] = [
        Self::KnightMale,
        Self::KnightFemale,
        Self::Elf,
        Self::NinjaFemale,
        Self::Wizard,
        Self::Witch,
        Self::VikingMale,
        Self::VikingFemale,
        Self::GoblinMale,
        Self::GoblinFemale,
        Self::PirateMale,
        Self::PirateFemale,
        Self::SoldierMale,
        Self::SoldierFemale,
        Self::WorkerMale,
        Self::WorkerFemale,
        Self::CasualMale,
        Self::CasualFemale,
        Self::Wolf,
        Self::Rat,
        Self::Pig,
    ];

    fn source(self) -> (&'static [u8], Clips) {
        const SLASH: &str = "SwordSlash";
        const SHOOT: &str = "Shoot_OneHanded";
        const PUNCH: &str = "Punch";
        match self {
            Self::KnightMale => (bundled!("Knight_Male.glb"), humanoid(SLASH)),
            Self::KnightFemale => (bundled!("Knight_Golden_Female.glb"), humanoid(SLASH)),
            Self::Elf => (bundled!("Elf.glb"), humanoid(SHOOT)),
            Self::NinjaFemale => (bundled!("Ninja_Female.glb"), humanoid(SHOOT)),
            Self::Wizard => (bundled!("Wizard.glb"), humanoid(SHOOT)),
            Self::Witch => (bundled!("Witch.glb"), humanoid(SHOOT)),
            Self::VikingMale => (bundled!("Viking_Male.glb"), humanoid(SLASH)),
            Self::VikingFemale => (bundled!("Viking_Female.glb"), humanoid(SLASH)),
            Self::GoblinMale => (bundled!("Goblin_Male.glb"), humanoid(PUNCH)),
            Self::GoblinFemale => (bundled!("Goblin_Female.glb"), humanoid(PUNCH)),
            Self::PirateMale => (bundled!("Pirate_Male.glb"), humanoid(SLASH)),
            Self::PirateFemale => (bundled!("Pirate_Female.glb"), humanoid(SLASH)),
            Self::SoldierMale => (bundled!("Soldier_Male.glb"), humanoid(SLASH)),
            Self::SoldierFemale => (bundled!("Soldier_Female.glb"), humanoid(SLASH)),
            Self::WorkerMale => (bundled!("Worker_Male.glb"), humanoid(PUNCH)),
            Self::WorkerFemale => (bundled!("Worker_Female.glb"), humanoid(PUNCH)),
            Self::CasualMale => (bundled!("Casual_Male.glb"), humanoid(PUNCH)),
            Self::CasualFemale => (bundled!("Casual_Female.glb"), humanoid(PUNCH)),
            Self::Wolf => (
                bundled!("Wolf.glb"),
                Clips {
                    idle: "Idle",
                    walk: "Walk",
                    run: "Gallop",
                    attack: "Attack",
                    death: "Death",
                },
            ),
            Self::Rat => (
                bundled!("Rat.glb"),
                Clips {
                    idle: "Rat_Idle",
                    walk: "Rat_Walk",
                    run: "Rat_Run",
                    attack: "Rat_Attack",
                    death: "Rat_Death",
                },
            ),
            // The farm pig has no attack clip; its jump reads as a charge.
            Self::Pig => (
                bundled!("Pig.glb"),
                Clips {
                    idle: "Idle",
                    walk: "Walk",
                    run: "Run",
                    attack: "Jump",
                    death: "Death",
                },
            ),
        }
    }

    /// The parsed model, imported once per process on first use.
    pub fn asset(self) -> &'static Asset {
        static ASSETS: OnceLock<Vec<OnceLock<Asset>>> = OnceLock::new();
        let slots = ASSETS.get_or_init(|| Self::ALL.iter().map(|_| OnceLock::new()).collect());
        let index = Self::ALL
            .iter()
            .position(|kind| *kind == self)
            .expect("every kind is listed");
        slots[index].get_or_init(|| {
            let (bytes, clips) = self.source();
            Asset {
                model: Model::parse(bytes, clips.idle)
                    .expect("bundled Quaternius model is valid glTF"),
                clips,
            }
        })
    }
}

/// Per-unit clip choice and clock. Presentation-owned and cosmetic.
#[derive(Default)]
pub(super) struct Playback {
    clip: &'static str,
    time: f32,
    attack_until: f64,
}

impl Playback {
    /// Starts an observed attack clip at `now`.
    pub fn attack(&mut self, asset: &Asset, now: f64) {
        self.attack_until = now + f64::from(asset.model.duration(asset.clips.attack).max(0.3));
    }

    /// Advances and returns the clip and time to pose this frame.
    pub fn advance(
        &mut self,
        asset: &Asset,
        speed: f64,
        dead: bool,
        now: f64,
        dt: f64,
        reduced: bool,
    ) -> (&'static str, f32) {
        let c = &asset.clips;
        let (clip, rate, once) = if dead {
            (c.death, 1., true)
        } else if now < self.attack_until && !reduced {
            (c.attack, 1., true)
        } else if speed > 3.2 {
            (c.run, (speed / 5.).clamp(0.7, 1.5), false)
        } else if speed > 0.25 {
            (c.walk, (speed / 1.8).clamp(0.6, 1.6), false)
        } else {
            (c.idle, 1., false)
        };
        if clip != self.clip {
            self.clip = clip;
            self.time = 0.;
        } else if !reduced {
            self.time += (dt * rate) as f32;
        }
        let duration = asset.model.duration(clip);
        let time = if duration <= 0. {
            0.
        } else if once {
            self.time.min(duration)
        } else {
            self.time % duration
        };
        (clip, time)
    }
}
