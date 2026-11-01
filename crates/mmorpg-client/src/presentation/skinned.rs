//! Imported Quaternius glTF units: material colours baked into vertex colours and clips sampled
//! into joint palettes on the CPU. Each figure draws its model's shared bind-pose meshes with its
//! palette, which the renderer skins on the GPU (#595); picking poses a clicked body itself.
//! Presentation only; the authoritative body box still comes from content.
use crate::world::{DrawMaterial, DrawSkin, Influences, SceneDraw};
use mmorpg_presentation::units::{
    imported::{
        BOW_DRAW_AXIS, BOW_HAND_SOCKET, BOW_NOCK_JOINT, BOW_STOWED_SOCKET, BowShot,
        DROP_ARMOR_OUTFIT, Figure, Joint, Kind, Layer, Loadout, Mask, Outfit, UPPER_BODY_ROOT,
        draw_meters, gripped,
    },
    kit::{Frame, model},
    player::Hat,
    sway,
    traversal::{self, Motions, SecondaryPose},
};
use std::{
    collections::BTreeMap,
    sync::{Arc, OnceLock},
};
use strum::VariantArray;
use three_d_core::{Color3, Mesh, Vec3, VertexAttributes};

mod residents;
pub(super) use residents::{resident_look, residents_drawable};

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

/// The sRGB colour (0..=1) of `image`'s nearest texel at `uv`, wrapping like a repeat sampler.
fn sample(image: &gltf::image::Data, [u, v]: [f32; 2]) -> [f32; 3] {
    use gltf::image::Format as F;
    let (width, height) = (image.width as usize, image.height as usize);
    if width == 0 || height == 0 {
        return [1.; 3];
    }
    let x = ((u.rem_euclid(1.) * width as f32) as usize).min(width - 1);
    let y = ((v.rem_euclid(1.) * height as f32) as usize).min(height - 1);
    let stride = match image.format {
        F::R8 => 1,
        F::R8G8 => 2,
        F::R8G8B8 => 3,
        F::R8G8B8A8 => 4,
        _ => return [1.; 3],
    };
    let at = (y * width + x) * stride;
    let channel = |i: usize| f32::from(image.pixels[at + i.min(stride - 1)]) / 255.;
    if stride < 3 {
        [channel(0); 3]
    } else {
        [channel(0), channel(1), channel(2)]
    }
}

/// glTF base colours are linear; the native mesh path expects sRGB vertex colours.
fn srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1. / 2.4) - 0.055
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
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

#[derive(Clone)]
struct Channel {
    node: usize,
    path: Path,
    step: bool,
    times: Vec<f32>,
    values: Vec<[f32; 4]>,
}

#[derive(Clone)]
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

#[derive(Clone)]
struct Primitive {
    /// The glTF node drawing it, so a look can hide named parts (the Ranger's hood).
    node: Option<String>,
    /// Its material's name, so a hair tint can recolour it.
    material: Option<String>,
    /// Its material's authored linear base colour factor, which a recolour replaces.
    factor: [f32; 3],
    double_sided: bool,
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<Color3>,
    indices: Vec<u32>,
    /// Up to four (slot, weight) influences per vertex, slots into [`Model::slots`].
    influences: Vec<[(u16, f32); 4]>,
    /// Base-colour texture coordinates, so a head skin can be baked in place of the authored one.
    uvs: Arc<[[f32; 2]]>,
    /// Named morph targets (the face, age and build keys), shared between clones.
    targets: Arc<[Target]>,
}

/// One named morph target of a primitive: per-vertex position and normal offsets.
struct Target {
    name: String,
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
}

/// One imported model: rest hierarchy, skinned primitives and named clips.
#[derive(Clone)]
pub(super) struct Model {
    rest: Vec<Rest>,
    parents: Vec<Option<usize>>,
    /// Nodes ordered parents first.
    order: Vec<usize>,
    primitives: Vec<Primitive>,
    /// The joint palette's slots, shared by every primitive: driving node and its inverse
    /// bind matrix. An unskinned primitive's node is a slot with the identity.
    slots: Vec<(usize, M4)>,
    /// Each primitive's bind-pose upload, built on first draw (#595).
    binds: OnceLock<Arc<[Option<Bind>]>>,
    /// Each primitive baked in the rest pose, for static draws, built on first draw.
    rests: OnceLock<Arc<[Option<Baked>]>>,
    clips: Arc<BTreeMap<String, Clip>>,
    /// Node names, for attaching items to joints.
    names: Vec<Option<String>>,
    /// Where each node sits for masked layers: in the upper body ([`UPPER_BODY_ROOT`]'s
    /// subtree) or not, and on which hand's fingers.
    joints: Vec<Joint>,
    /// Rest-clip bounds: lowest point and height along +Y, horizontal centre.
    pub floor: f32,
    pub height: f32,
    pub centre: [f32; 2],
    /// The `Head` joint, and the rest head centre's offset from it with the head's radius.
    head: Option<(usize, [f32; 3], f32)>,
}

/// A primitive's bind pose as the renderer keeps it: its resource key, the mesh and its
/// per-vertex palette influences. Every figure of the model shares it (#595).
#[derive(Clone)]
struct Bind {
    key: String,
    mesh: Arc<Mesh>,
    influences: Arc<Influences>,
}

/// A primitive baked in a fixed pose: its resource key and mesh.
type Baked = (String, Arc<Mesh>);

/// A resource key unique to `mesh`'s allocation, which the renderer's `Weak` keeps from reuse.
fn mesh_key(kind: &str, mesh: &Arc<Mesh>) -> String {
    format!("{kind}:{:p}", Arc::as_ptr(mesh))
}

impl Model {
    pub fn parse(bytes: &[u8], rest_clip: &'static str) -> Result<Self, gltf::Error> {
        Self::parse_with_clips(bytes, rest_clip, None)
    }

    fn parse_with_clips(
        bytes: &[u8],
        rest_clip: &'static str,
        library: Option<&[u8]>,
    ) -> Result<Self, gltf::Error> {
        let (document, buffers, images) = gltf::import_slice(bytes)?;
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
        let mut model_slots: Vec<(usize, M4)> = Vec::new();
        for node in document.nodes() {
            let Some(mesh) = node.mesh() else { continue };
            let node_slots: Vec<(usize, M4)> = match node.skin() {
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
            // The node's slots in the model's one palette.
            let remap: Vec<u16> = node_slots
                .iter()
                .map(|slot| {
                    let index = model_slots
                        .iter()
                        .position(|s| s.0 == slot.0 && s.1 == slot.1)
                        .unwrap_or_else(|| {
                            model_slots.push(*slot);
                            model_slots.len() - 1
                        });
                    u16::try_from(index).expect("a palette fits u16 joint indices")
                })
                .collect();
            let skinned = node.skin().is_some();
            // Blender's exporter names the targets in the mesh's `extras.targetNames`.
            let target_names: Vec<String> = mesh
                .extras()
                .as_ref()
                .and_then(|extras| serde_json::from_str::<serde_json::Value>(extras.get()).ok())
                .map(|extras| strings(&extras["targetNames"]))
                .unwrap_or_default();
            for primitive in mesh.primitives() {
                let reader = primitive.reader(|b| Some(&buffers[b.index()]));
                let Some(positions) = reader.read_positions() else {
                    continue;
                };
                let positions: Vec<[f32; 3]> = positions.collect();
                let normals: Vec<[f32; 3]> = reader
                    .read_normals()
                    .map_or_else(|| vec![[0., 1., 0.]; positions.len()], Iterator::collect);
                let pbr = primitive.material().pbr_metallic_roughness();
                let base = pbr.base_color_factor();
                let factor = [srgb(base[0]), srgb(base[1]), srgb(base[2])];
                // A textured material bakes its base-colour texel at each vertex's UV.
                let texture = pbr
                    .base_color_texture()
                    .map(|info| &images[info.texture().source().index()]);
                let uvs: Arc<[[f32; 2]]> = reader
                    .read_tex_coords(0)
                    .map_or_else(|| Arc::from([]), |uvs| uvs.into_f32().collect());
                let colors = match texture {
                    Some(image) if uvs.len() == positions.len() => uvs
                        .iter()
                        .map(|&uv| {
                            let texel = sample(image, uv);
                            Color3::new(
                                texel[0] * factor[0],
                                texel[1] * factor[1],
                                texel[2] * factor[2],
                            )
                        })
                        .collect(),
                    _ => vec![Color3::new(factor[0], factor[1], factor[2]); positions.len()],
                };
                let targets: Arc<[Target]> = reader
                    .read_morph_targets()
                    .zip(&target_names)
                    .map(|((positions, normals, _), name)| Target {
                        name: name.clone(),
                        positions: positions.map_or_else(Vec::new, Iterator::collect),
                        normals: normals.map_or_else(Vec::new, Iterator::collect),
                    })
                    .collect();
                let indices: Vec<u32> = reader.read_indices().map_or_else(
                    || (0..u32::try_from(positions.len()).unwrap_or(0)).collect(),
                    |i| i.into_u32().collect(),
                );
                let slot = |j: u16| remap[usize::from(j).min(remap.len() - 1)];
                let influences = match (skinned, reader.read_joints(0), reader.read_weights(0)) {
                    (true, Some(joints), Some(weights)) => joints
                        .into_u16()
                        .zip(weights.into_f32())
                        .map(|(j, w)| std::array::from_fn(|i| (slot(j[i]), w[i])))
                        .collect(),
                    _ => vec![
                        [(slot(0), 1.), (slot(0), 0.), (slot(0), 0.), (slot(0), 0.)];
                        positions.len()
                    ],
                };
                primitives.push(Primitive {
                    node: node.name().map(str::to_owned),
                    material: primitive.material().name().map(str::to_owned),
                    factor: [base[0], base[1], base[2]],
                    double_sided: primitive.material().double_sided(),
                    positions,
                    normals,
                    colors,
                    indices,
                    influences,
                    uvs,
                    targets,
                });
            }
        }
        let mut clips = BTreeMap::new();
        let library = library.map(gltf::import_slice).transpose()?;
        let (animation_document, animation_buffers) = library
            .as_ref()
            .map(|(document, buffers, _)| (document, buffers))
            .unwrap_or((&document, &buffers));
        for animation in animation_document.animations() {
            let mut channels = Vec::new();
            let mut duration = 0_f32;
            for channel in animation.channels() {
                let node = if library.is_some() {
                    let name = channel
                        .target()
                        .node()
                        .name()
                        .expect("shared animation targets a named joint");
                    document
                        .nodes()
                        .find(|node| node.name() == Some(name))
                        .expect("shared animation joint exists in the character")
                        .index()
                } else {
                    channel.target().node().index()
                };
                let reader = channel.reader(|b| Some(&animation_buffers[b.index()]));
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
                    values = values.as_chunks::<3>().0.iter().map(|k| k[1]).collect();
                }
                if times.is_empty() || values.len() < times.len() {
                    continue;
                }
                duration = duration.max(*times.last().unwrap_or(&0.));
                channels.push(Channel {
                    node,
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
        let names: Vec<_> = document
            .nodes()
            .map(|node| node.name().map(str::to_owned))
            .collect();
        // Parents come first in `order`, so a child inherits its parent's region.
        let mut joints = vec![Joint::default(); count];
        for &node in &order {
            let name = names[node].as_deref().unwrap_or_default();
            let upper =
                name == UPPER_BODY_ROOT || parents[node].is_some_and(|parent| joints[parent].upper);
            joints[node] = Joint::new(name, upper);
        }
        let mut model = Self {
            rest,
            parents,
            order,
            primitives,
            slots: model_slots,
            binds: OnceLock::new(),
            rests: OnceLock::new(),
            clips: Arc::new(clips),
            names,
            joints,
            floor: 0.,
            height: 1.,
            centre: [0.; 2],
            head: None,
        };
        let (mut min, mut max) = ([f32::MAX; 3], [f32::MIN; 3]);
        let rest_pose = model.globals(&[Layer::single(rest_clip, 0.)]);
        for p in model
            .posed(&rest_pose)
            .iter()
            .flatten()
            .flat_map(Mesh::vertices)
        {
            for (i, v) in [p.x, p.y, p.z].into_iter().enumerate() {
                min[i] = min[i].min(v);
                max[i] = max[i].max(v);
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

    /// The model with each clip's cloth chain pose (`secondaryPose`, #500) played as rotation
    /// channels of the chain joints it has, like the browser's mixer tracks. No cloth spring
    /// exists yet (#498 sways only hanging gear), so the authored pose plays as it is.
    fn with_secondary(mut self, poses: &BTreeMap<String, SecondaryPose>) -> Self {
        let added: Vec<_> = poses
            .iter()
            .filter(|(clip, pose)| self.clips.contains_key(*clip) && pose.sample_rate > 0.)
            .flat_map(|(clip, pose)| {
                pose.joints.iter().filter_map(|(joint, rotations)| {
                    let node = self.node(joint)?;
                    #[allow(clippy::cast_precision_loss)]
                    let times = (0..rotations.len())
                        .map(|frame| frame as f32 / pose.sample_rate)
                        .collect();
                    Some((
                        clip.clone(),
                        Channel {
                            node,
                            path: Path::Rotation,
                            step: false,
                            times,
                            values: rotations.clone(),
                        },
                    ))
                })
            })
            .collect();
        if !added.is_empty() {
            let clips = Arc::make_mut(&mut self.clips);
            for (clip, channel) in added {
                if let Some(clip) = clips.get_mut(&clip) {
                    clip.channels.push(channel);
                }
            }
        }
        self
    }

    /// Vertices mostly bound to the `Head` joint, measured in the rest pose.
    fn rest_head(
        &self,
        document: &gltf::Document,
        clip: &'static str,
    ) -> Option<(usize, [f32; 3], f32)> {
        let joint = document.nodes().find(|n| n.name() == Some("Head"))?.index();
        let (mut min, mut max) = ([f32::MAX; 3], [f32::MIN; 3]);
        let globals = self.globals(&[Layer::single(clip, 0.)]);
        for (posed, p) in self.posed(&globals).iter().zip(&self.primitives) {
            let Some(posed) = posed else { continue };
            for (q, weights) in posed.vertices().iter().zip(&p.influences) {
                let main = weights.iter().max_by(|a, b| a.1.total_cmp(&b.1))?;
                if self.slots.get(usize::from(main.0)).map(|s| s.0) != Some(joint) {
                    continue;
                }
                for (i, v) in [q.x, q.y, q.z].into_iter().enumerate() {
                    min[i] = min[i].min(v);
                    max[i] = max[i].max(v);
                }
            }
        }
        if min[0] > max[0] {
            return None;
        }
        let at = globals[joint];
        let centre: [f32; 3] = std::array::from_fn(|i| (min[i] + max[i]) * 0.5);
        let radius = (0..3).map(|i| (max[i] - min[i]) * 0.5).fold(0., f32::max);
        Some((
            joint,
            std::array::from_fn(|i| centre[i] - at[12 + i]),
            radius,
        ))
    }

    /// The posed head centre and radius in model space, following the head joint's position.
    pub fn head(&self, pose: &[Layer]) -> Option<([f32; 3], f32)> {
        let (joint, offset, radius) = self.head?;
        let at = self.globals(pose)[joint];
        Some((std::array::from_fn(|i| at[12 + i] + offset[i]), radius))
    }

    /// Model-to-world placement: scaled to `height` metres, lowest rest point on `root`.
    pub fn placement(&self, root: Frame, height: f64) -> three_d_animation::Mat4 {
        let scale = height / f64::from(self.height);
        let mut placement = three_d_animation::Mat4 {
            elements: model(root, [scale; 3]),
        };
        let offset = [-self.centre[0], -self.floor, -self.centre[1]].map(f64::from);
        let m = &mut placement.elements;
        for r in 0..3 {
            let shift: f64 = (0..3).map(|k| f64::from(m[k * 4 + r]) * offset[k]).sum();
            m[12 + r] += shift as f32;
        }
        placement
    }

    /// Every clip with its length in seconds.
    pub fn clip_durations(&self) -> impl Iterator<Item = (String, f32)> + '_ {
        self.clips
            .iter()
            .map(|(name, clip)| (name.clone(), clip.duration))
    }

    /// Joint transforms for a layered pose (Atlas 6.6.45), the same weighting as three.js's
    /// mixer: each joint property takes the weighted average of the layers that drive and
    /// animate it, and a total weight below 1 leaves the remainder at rest. An empty pose rests.
    fn globals(&self, pose: &[Layer]) -> Vec<M4> {
        self.globals_of(&self.locals(pose))
    }

    /// Joint transforms for each node's local transform.
    fn globals_of(&self, local: &[Rest]) -> Vec<M4> {
        let mut globals = vec![IDENTITY; self.rest.len()];
        for &n in &self.order {
            let l = trs(local[n].t, local[n].r, local[n].s);
            globals[n] = self.parents[n].map_or(l, |p| mul(&globals[p], &l));
        }
        globals
    }

    /// Each node's local transform in a layered pose.
    fn locals(&self, pose: &[Layer]) -> Vec<Rest> {
        let mut local = self.rest.clone();
        if let [layer] = pose
            && layer.weight >= 1.
            && layer.mask == Mask::All
        {
            if let Some(clip) = self.clips.get(layer.clip) {
                for channel in &clip.channels {
                    let v = channel.sample(layer.time);
                    let r = &mut local[channel.node];
                    match channel.path {
                        Path::Translation => r.t = [v[0], v[1], v[2]],
                        Path::Rotation => r.r = v,
                        Path::Scale => r.s = [v[0], v[1], v[2]],
                    }
                }
            }
        } else if !pose.is_empty() {
            // Per node and path: the weighted sum and the total weight.
            let mut sums = vec![[([0f32; 4], 0f32); 3]; self.rest.len()];
            let gripped = gripped(pose);
            for layer in pose {
                let Some(clip) = self.clips.get(layer.clip) else {
                    continue;
                };
                for channel in &clip.channels {
                    if !layer.mask.drives(self.joints[channel.node], gripped) {
                        continue;
                    }
                    let mut v = channel.sample(layer.time);
                    let index = match channel.path {
                        Path::Translation => 0,
                        Path::Rotation => {
                            // Quaternions average on the rest rotation's hemisphere.
                            let rest = self.rest[channel.node].r;
                            if v.iter().zip(&rest).map(|(a, b)| a * b).sum::<f32>() < 0. {
                                v = v.map(|x| -x);
                            }
                            1
                        }
                        Path::Scale => 2,
                    };
                    let (sum, weight) = &mut sums[channel.node][index];
                    for (s, x) in sum.iter_mut().zip(v) {
                        *s += x * layer.weight;
                    }
                    *weight += layer.weight;
                }
            }
            for (r, sums) in local.iter_mut().zip(&sums) {
                let mix = |(sum, weight): ([f32; 4], f32), rest: [f32; 4]| -> [f32; 4] {
                    let fill = (1. - weight).max(0.);
                    let total = weight.max(1.);
                    std::array::from_fn(|i| (sum[i] + rest[i] * fill) / total)
                };
                let extend = |v: [f32; 3]| [v[0], v[1], v[2], 0.];
                if sums[0].1 > 0. {
                    let t = mix(sums[0], extend(r.t));
                    r.t = [t[0], t[1], t[2]];
                }
                if sums[1].1 > 0. {
                    let q = mix(sums[1], r.r);
                    let len = q.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-6);
                    r.r = q.map(|v| v / len);
                }
                if sums[2].1 > 0. {
                    let s = mix(sums[2], extend(r.s));
                    r.s = [s[0], s[1], s[2]];
                }
            }
        }
        local
    }

    /// The joint palette for joint transforms `globals`: each slot's node transform times its
    /// inverse bind matrix, which the renderer blends per vertex.
    fn palette(&self, globals: &[M4]) -> Arc<[M4]> {
        self.slots
            .iter()
            .map(|(node, inverse)| mul(&globals[*node], inverse))
            .collect()
    }

    /// Each primitive's bind-pose upload (None when its geometry is invalid), built once.
    fn binds(&self) -> &[Option<Bind>] {
        self.binds.get_or_init(|| {
            self.primitives
                .iter()
                .map(|p| {
                    let to_vec3 = |v: &[f32; 3]| Vec3::new(v[0], v[1], v[2]);
                    let mesh = Mesh::with_attributes(
                        p.positions.iter().map(to_vec3).collect(),
                        p.indices.clone(),
                        VertexAttributes {
                            normals: Some(p.normals.iter().map(to_vec3).collect()),
                            colors: Some(p.colors.clone()),
                            ..Default::default()
                        },
                    )
                    .ok()?;
                    let mesh = Arc::new(mesh);
                    Some(Bind {
                        key: mesh_key("imported", &mesh),
                        mesh,
                        influences: Arc::new(Influences {
                            joints: p.influences.iter().map(|i| i.map(|(j, _)| j)).collect(),
                            weights: p.influences.iter().map(|i| i.map(|(_, w)| w)).collect(),
                        }),
                    })
                })
                .collect()
        })
    }

    /// Each primitive posed on the CPU for joint transforms `globals`: load-time measurements
    /// and static rest geometry only; drawn figures pose on the GPU.
    fn posed(&self, globals: &[M4]) -> Vec<Option<Mesh>> {
        let palette = self.palette(globals);
        self.binds()
            .iter()
            .map(|bind| {
                let bind = bind.as_ref()?;
                Some(
                    DrawSkin {
                        figure: String::new(),
                        influences: Arc::clone(&bind.influences),
                        palette: Arc::clone(&palette),
                    }
                    .pose(&bind.mesh),
                )
            })
            .collect()
    }

    /// Each primitive baked once in the rest pose, for draws that never animate.
    fn rests(&self) -> &[Option<Baked>] {
        self.rests.get_or_init(|| {
            self.posed(&self.globals_of(&self.rest))
                .into_iter()
                .map(|mesh| {
                    let mesh = Arc::new(mesh?);
                    Some((mesh_key("imported-rest", &mesh), mesh))
                })
                .collect()
        })
    }

    /// Drops the uploads built from the old geometry, colours or rest pose.
    fn changed(&mut self) {
        self.binds = OnceLock::new();
        self.rests = OnceLock::new();
    }

    /// The model-space transform of the named joint in the posed clip.
    pub fn joint(&self, name: &str, pose: &[Layer]) -> Option<three_d_animation::Mat4> {
        let index = self.names.iter().position(|n| n.as_deref() == Some(name))?;
        let mut joint = three_d_animation::Mat4::IDENTITY;
        joint.elements = self.globals(pose)[index];
        Some(joint)
    }

    /// Gives the `names` materials the linear base colour `to` in place of their authored `from`,
    /// like the browser's tint of a material's colour.
    fn retint(&mut self, names: &[String], from: [f32; 3], to: [f32; 3]) {
        self.changed();
        let factor: [f32; 3] = std::array::from_fn(|i| srgb(to[i]) / srgb(from[i]).max(1e-4));
        for p in &mut self.primitives {
            if p.material.as_ref().is_some_and(|name| names.contains(name)) {
                for c in &mut p.colors {
                    *c = Color3::new(c.r * factor[0], c.g * factor[1], c.b * factor[2]);
                }
            }
        }
    }

    /// Gives the `materials` the linear base colour `to` in place of their own factor, on the
    /// primitives of the `nodes` glTF nodes (all with None), like the browser's
    /// `tintMaterials`.
    fn recolor(&mut self, nodes: Option<&[String]>, materials: &[String], to: [f32; 3]) {
        self.changed();
        for p in &mut self.primitives {
            let node = nodes.is_none_or(|nodes| p.node.as_ref().is_some_and(|n| nodes.contains(n)));
            if !node
                || !p
                    .material
                    .as_ref()
                    .is_some_and(|name| materials.contains(name))
            {
                continue;
            }
            let factor: [f32; 3] =
                std::array::from_fn(|i| srgb(to[i]) / srgb(p.factor[i]).max(1e-4));
            for c in &mut p.colors {
                *c = Color3::new(c.r * factor[0], c.g * factor[1], c.b * factor[2]);
            }
            p.factor = to;
        }
    }

    /// Bakes morph target `weights` into the rest shape of the primitives of the `nodes` glTF
    /// nodes (all with None), like the browser's `morphTargetInfluences`; each primitive takes the
    /// targets it has. Looks are built once, so the weights never change afterwards.
    fn morph(&mut self, nodes: Option<&[String]>, weights: &BTreeMap<String, f32>) {
        self.changed();
        for p in &mut self.primitives {
            if !nodes.is_none_or(|nodes| p.node.as_ref().is_some_and(|n| nodes.contains(n))) {
                continue;
            }
            let mut moved = false;
            for target in p.targets.iter() {
                let Some(&weight) = weights.get(&target.name).filter(|w| **w != 0.) else {
                    continue;
                };
                moved = true;
                for (position, delta) in p.positions.iter_mut().zip(&target.positions) {
                    for i in 0..3 {
                        position[i] += delta[i] * weight;
                    }
                }
                for (normal, delta) in p.normals.iter_mut().zip(&target.normals) {
                    for i in 0..3 {
                        normal[i] += delta[i] * weight;
                    }
                }
            }
            if moved {
                for normal in &mut p.normals {
                    let len = normal.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-6);
                    *normal = normal.map(|v| v / len);
                }
            }
        }
    }

    /// Bakes `image` as the base-colour texture of the `material` primitives in place of the
    /// authored one, under their own base colour factor: a painted head skin (asset-tooling #380).
    fn reskin(&mut self, material: &str, image: &gltf::image::Data) {
        self.changed();
        for p in &mut self.primitives {
            if p.material.as_deref() != Some(material) || p.uvs.len() != p.positions.len() {
                continue;
            }
            let factor = p.factor.map(srgb);
            p.colors = p
                .uvs
                .iter()
                .map(|&uv| {
                    let texel = sample(image, uv);
                    Color3::new(
                        texel[0] * factor[0],
                        texel[1] * factor[1],
                        texel[2] * factor[2],
                    )
                })
                .collect();
        }
    }

    /// Raises glTF node `node` by `dy` metres in its parent's frame.
    fn lift(&mut self, node: &str, dy: f32) {
        self.changed();
        if let Some(index) = self.names.iter().position(|n| n.as_deref() == Some(node)) {
            self.rest[index].t[1] += dy;
        }
    }

    /// The model of glTF node `node` alone: one armour plate of a multi-plate entry.
    fn piece(&self, node: &str) -> Self {
        Self {
            rest: self.rest.clone(),
            parents: self.parents.clone(),
            order: self.order.clone(),
            primitives: self
                .primitives
                .iter()
                .filter(|p| p.node.as_deref() == Some(node))
                .cloned()
                .collect(),
            slots: self.slots.clone(),
            binds: OnceLock::new(),
            rests: OnceLock::new(),
            clips: Arc::default(),
            names: self.names.clone(),
            joints: self.joints.clone(),
            floor: self.floor,
            height: self.height,
            centre: self.centre,
            head: None,
        }
    }

    /// Recolours the `names` materials by a linear base colour factor, like the browser's tint.
    fn tint(&mut self, names: &[String], factor: [f32; 3]) {
        self.changed();
        let factor = factor.map(srgb);
        for p in &mut self.primitives {
            if p.material.as_ref().is_some_and(|name| names.contains(name)) {
                for c in &mut p.colors {
                    *c = Color3::new(c.r * factor[0], c.g * factor[1], c.b * factor[2]);
                }
            }
        }
    }

    /// Draws for one posed instance standing at `root`, scaled to `height` metres, without
    /// the primitives of the `hidden` glTF nodes.
    pub fn draws(
        &self,
        id: &str,
        root: Frame,
        height: f64,
        pose: &[Layer],
        tint: Option<[f32; 3]>,
        hidden: &[String],
    ) -> Vec<SceneDraw> {
        let palette = self.palette(&self.globals(pose));
        self.skinned_draws(id, self.placement(root, height), palette, tint, hidden)
    }

    /// Draws for one instance under an explicit model-to-world `placement`: posed on the GPU,
    /// or with an empty pose the rest geometry, baked once.
    pub fn draws_placed(
        &self,
        id: &str,
        placement: three_d_animation::Mat4,
        pose: &[Layer],
        tint: Option<[f32; 3]>,
    ) -> Vec<SceneDraw> {
        if pose.is_empty() {
            return self.rest_draws(id, placement, tint);
        }
        let palette = self.palette(&self.globals(pose));
        self.skinned_draws(id, placement, palette, tint, &[])
    }

    /// The node index of glTF node `name`.
    fn node(&self, name: &str) -> Option<usize> {
        self.names.iter().position(|n| n.as_deref() == Some(name))
    }

    /// Draws for one instance at rest under `placement`, with node `moved.0` translated by
    /// `moved.1` from its rest in its parent's frame: a bow's drawn nock (#556).
    fn draws_moved(
        &self,
        id: &str,
        placement: three_d_animation::Mat4,
        moved: Option<(usize, [f32; 3])>,
        tint: Option<[f32; 3]>,
    ) -> Vec<SceneDraw> {
        let mut local = self.rest.clone();
        if let Some((node, by)) = moved {
            for (t, by) in local[node].t.iter_mut().zip(by) {
                *t += by;
            }
        }
        let palette = self.palette(&self.globals_of(&local));
        self.skinned_draws(id, placement, palette, tint, &[])
    }

    /// A primitive's material: its colours are in its vertices, a tint multiplies them.
    fn material(p: &Primitive, tint: Option<[f32; 3]>) -> DrawMaterial {
        DrawMaterial {
            color: tint.unwrap_or([1.; 3]),
            emissive: [0.; 3],
            opacity: 1.,
            unlit: false,
            double_sided: p.double_sided,
        }
    }

    /// Draws of the figure `id` posed by `palette` on the GPU (#595): every primitive's shared
    /// bind pose, without the primitives of the `hidden` glTF nodes.
    fn skinned_draws(
        &self,
        id: &str,
        placement: three_d_animation::Mat4,
        palette: Arc<[M4]>,
        tint: Option<[f32; 3]>,
        hidden: &[String],
    ) -> Vec<SceneDraw> {
        self.binds()
            .iter()
            .zip(&self.primitives)
            .enumerate()
            .filter(|(_, (_, p))| p.node.as_ref().is_none_or(|node| !hidden.contains(node)))
            .filter_map(|(index, (bind, p))| {
                let bind = bind.as_ref()?;
                Some(SceneDraw {
                    id: format!("{id}-mesh-{index}"),
                    resource_key: bind.key.clone(),
                    mesh: Arc::clone(&bind.mesh),
                    model: placement,
                    material: Self::material(p, tint),
                    skin: Some(DrawSkin {
                        figure: id.to_owned(),
                        influences: Arc::clone(&bind.influences),
                        palette: Arc::clone(&palette),
                    }),
                })
            })
            .collect()
    }

    /// Draws of the rest geometry under `placement`: static meshes, baked once.
    fn rest_draws(
        &self,
        id: &str,
        placement: three_d_animation::Mat4,
        tint: Option<[f32; 3]>,
    ) -> Vec<SceneDraw> {
        self.rests()
            .iter()
            .zip(&self.primitives)
            .enumerate()
            .filter_map(|(index, (rest, p))| {
                let (key, mesh) = rest.as_ref()?;
                Some(SceneDraw {
                    id: format!("{id}-mesh-{index}"),
                    resource_key: key.clone(),
                    mesh: Arc::clone(mesh),
                    model: placement,
                    material: Self::material(p, tint),
                    skin: None,
                })
            })
            .collect()
    }
}

pub(super) struct Asset {
    pub model: Model,
}

macro_rules! bundled {
    ($file:literal) => {
        include_bytes!(concat!("../../../../assets/quaternius/", $file)).as_slice()
    };
}

/// The Ranger bundle's glTF and declared resources, embedded by `build.rs`.
mod ranger_bundle {
    include!(concat!(env!("OUT_DIR"), "/ranger_bundle.rs"));
}

/// The female figure's bundle (#493): her sockets, clips, appearance and prepared entries.
#[allow(
    dead_code,
    reason = "only the male figure carries the bow whose draw MOTIONS holds"
)]
mod ranger_female_bundle {
    include!(concat!(env!("OUT_DIR"), "/ranger_female_bundle.rs"));
}

/// The Greyhaven inn bundle's drawn entries and their resources, embedded by `build.rs` (#158).
mod inn_bundle {
    include!(concat!(env!("OUT_DIR"), "/inn_bundle.rs"));
}

/// Marshal Elden Greywatch's house bundle's drawn entries and resources (#158).
mod marshal_house_bundle {
    include!(concat!(env!("OUT_DIR"), "/marshal_house_bundle.rs"));
}

type BuildingDraws = Vec<(&'static str, Vec<SceneDraw>)>;

fn building_draws(
    entries: &'static [(&'static str, &'static str)],
    files: &[(&str, &[u8])],
) -> BuildingDraws {
    entries
        .iter()
        .map(|&(key, path)| {
            let model = Model::parse(&bundle_glb(path, files), "")
                .expect("a walk-in building entry is valid glTF");
            let draws = model.draws_placed(key, three_d_animation::Mat4::IDENTITY, &[], None);
            (key, draws)
        })
        .collect()
}

/// A walk-in building's entries (by its bundle, `mmorpg_presentation::buildings::Spec::bundle`)
/// as `(key, draws in the entry frame)`, imported once. Static meshes never re-skin: callers
/// only replace each draw's model matrix, so the GPU upload happens once.
pub(super) fn building_draws_of(bundle: &str) -> &'static [(&'static str, Vec<SceneDraw>)] {
    static INN: OnceLock<BuildingDraws> = OnceLock::new();
    static MARSHAL_HOUSE: OnceLock<BuildingDraws> = OnceLock::new();
    match bundle {
        "greyhaven.inn" => {
            INN.get_or_init(|| building_draws(inn_bundle::ENTRIES, inn_bundle::FILES))
        }
        "greyhaven.marshal-house" => MARSHAL_HOUSE.get_or_init(|| {
            building_draws(marshal_house_bundle::ENTRIES, marshal_house_bundle::FILES)
        }),
        _ => &[],
    }
}

/// The Sunken Shrine kit's modules and their resources, embedded by `build.rs` (#431).
mod shrine_bundle {
    include!(concat!(env!("OUT_DIR"), "/shrine_bundle.rs"));
}

/// The shrine kit's draws (in the module frame) of entry `key`, every module imported once.
pub(super) fn shrine_draws(key: &str) -> &'static [SceneDraw] {
    static DRAWS: OnceLock<Vec<(&'static str, Vec<SceneDraw>)>> = OnceLock::new();
    DRAWS
        .get_or_init(|| {
            shrine_bundle::ENTRIES
                .iter()
                .map(|&(key, path)| {
                    let model = Model::parse(&bundle_glb(path, shrine_bundle::FILES), "")
                        .expect("a shrine kit entry is valid glTF");
                    let draws =
                        model.draws_placed(key, three_d_animation::Mat4::IDENTITY, &[], None);
                    (key, draws)
                })
                .collect()
        })
        .iter()
        .find(|(entry, _)| *entry == key)
        .map_or(&[], |(_, draws)| draws.as_slice())
}

/// The Outpost building kit's modules, roof variants and resources, embedded by `build.rs`
/// (#420).
mod outpost_bundle {
    include!(concat!(env!("OUT_DIR"), "/outpost_bundle.rs"));
}

/// The Outpost kit's draws (in the module frame) of `entry` (`<key>` or `<key>/<variant>`),
/// every entry imported once.
pub(super) fn outpost_draws(entry: &str) -> &'static [SceneDraw] {
    static DRAWS: OnceLock<Vec<(&'static str, Vec<SceneDraw>)>> = OnceLock::new();
    DRAWS
        .get_or_init(|| {
            outpost_bundle::ENTRIES
                .iter()
                .map(|&(entry, path)| {
                    let model = Model::parse(&bundle_glb(path, outpost_bundle::FILES), "")
                        .expect("an Outpost kit entry is valid glTF");
                    let draws =
                        model.draws_placed(entry, three_d_animation::Mat4::IDENTITY, &[], None);
                    (entry, draws)
                })
                .collect()
        })
        .iter()
        .find(|(key, _)| *key == entry)
        .map_or(&[], |(_, draws)| draws.as_slice())
}

/// The market square's settlement props and their resources, embedded by `build.rs` (#421).
mod settlement_bundle {
    include!(concat!(env!("OUT_DIR"), "/settlement_bundle.rs"));
}

/// A settlement prop's draws (in the prop frame) of `entry`, every entry imported once and
/// shared by each placement.
pub(super) fn settlement_draws(entry: &str) -> &'static [SceneDraw] {
    static DRAWS: OnceLock<Vec<(&'static str, Vec<SceneDraw>)>> = OnceLock::new();
    DRAWS
        .get_or_init(|| {
            settlement_bundle::ENTRIES
                .iter()
                .map(|&(entry, path)| {
                    let model = Model::parse(&bundle_glb(path, settlement_bundle::FILES), "")
                        .expect("a settlement prop entry is valid glTF");
                    let draws =
                        model.draws_placed(entry, three_d_animation::Mat4::IDENTITY, &[], None);
                    (entry, draws)
                })
                .collect()
        })
        .iter()
        .find(|(key, _)| *key == entry)
        .map_or(&[], |(_, draws)| draws.as_slice())
}

/// Greyhaven's ambient animals and their resources, embedded by `build.rs` (#419).
mod ambient_bundle {
    include!(concat!(env!("OUT_DIR"), "/ambient_bundle.rs"));
}

/// A skinned ambient model and its clip lengths by clip name.
type AmbientModel = (Model, BTreeMap<String, f32>);

/// The skinned ambient animal of entry `key` with its clip lengths, each imported once.
pub(super) fn ambient_model(key: &str) -> Option<&'static AmbientModel> {
    static MODELS: OnceLock<Vec<(&'static str, AmbientModel)>> = OnceLock::new();
    MODELS
        .get_or_init(|| {
            ambient_bundle::ENTRIES
                .iter()
                .map(|&(key, path)| {
                    let model = Model::parse(&bundle_glb(path, ambient_bundle::FILES), "Idle")
                        .expect("an ambient animal is valid glTF");
                    let clips = model.clip_durations().collect();
                    (key, (model, clips))
                })
                .collect()
        })
        .iter()
        .find(|(entry, _)| *entry == key)
        .map(|(_, model)| model)
}

/// Normalizes `base`'s directory joined with a relative `uri` ("../../resources/x.bin").
fn resolve(base: &str, uri: &str) -> String {
    let mut parts: Vec<&str> = base.split('/').collect();
    parts.pop();
    for part in uri.split('/') {
        match part {
            ".." => {
                parts.pop();
            }
            "." | "" => {}
            part => parts.push(part),
        }
    }
    parts.join("/")
}

/// One GLB of an asset-tooling `gltf-resources-v1` entry: every external buffer and image
/// becomes a view of a single binary chunk, so the ordinary GLB import reads it.
fn bundle_glb(entry: &str, files: &[(&str, &[u8])]) -> Vec<u8> {
    let file = |path: &str| {
        files
            .iter()
            .find(|(name, _)| *name == path)
            .map(|(_, bytes)| *bytes)
            .unwrap_or_else(|| panic!("the bundle embeds {path}"))
    };
    let mut json: serde_json::Value =
        serde_json::from_slice(file(entry)).expect("the bundle glTF is JSON");
    let mut bin = Vec::new();
    let mut append = |bytes: &[u8]| {
        while !bin.len().is_multiple_of(4) {
            bin.push(0);
        }
        let offset = bin.len();
        bin.extend_from_slice(bytes);
        offset
    };
    let mut offsets = Vec::new();
    for buffer in json["buffers"].as_array().cloned().unwrap_or_default() {
        let uri = buffer["uri"].as_str().expect("bundle buffers are external");
        offsets.push(append(file(&resolve(entry, uri))));
    }
    for view in json
        .get_mut("bufferViews")
        .and_then(serde_json::Value::as_array_mut)
        .into_iter()
        .flatten()
    {
        let buffer = view["buffer"].as_u64().expect("a view names its buffer") as usize;
        let start = view["byteOffset"].as_u64().unwrap_or(0) as usize + offsets[buffer];
        view["buffer"] = 0.into();
        view["byteOffset"] = start.into();
    }
    let mut views = Vec::new();
    for image in json
        .get_mut("images")
        .and_then(serde_json::Value::as_array_mut)
        .into_iter()
        .flatten()
    {
        let uri = image["uri"]
            .as_str()
            .expect("bundle images are external")
            .to_owned();
        let bytes = file(&resolve(entry, &uri));
        let offset = append(bytes);
        views.push(
            serde_json::json!({"buffer": 0, "byteOffset": offset, "byteLength": bytes.len()}),
        );
        if let Some(image) = image.as_object_mut() {
            image.remove("uri");
        }
    }
    let first = json["bufferViews"].as_array().map_or(0, Vec::len);
    for (index, image) in json
        .get_mut("images")
        .and_then(serde_json::Value::as_array_mut)
        .into_iter()
        .flatten()
        .enumerate()
    {
        image["bufferView"] = (first + index).into();
    }
    if let Some(list) = json
        .get_mut("bufferViews")
        .and_then(serde_json::Value::as_array_mut)
    {
        list.extend(views);
    }
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    json["buffers"] = serde_json::json!([{"byteLength": bin.len()}]);
    let mut text = serde_json::to_vec(&json).expect("JSON serializes");
    while !text.len().is_multiple_of(4) {
        text.push(b' ');
    }
    let length = 12 + 8 + text.len() + 8 + bin.len();
    let mut glb = Vec::with_capacity(length);
    let word = |value: usize| {
        u32::try_from(value)
            .expect("bundle fits a GLB")
            .to_le_bytes()
    };
    glb.extend_from_slice(b"glTF");
    glb.extend_from_slice(&2_u32.to_le_bytes());
    glb.extend_from_slice(&word(length));
    glb.extend_from_slice(&word(text.len()));
    glb.extend_from_slice(b"JSON");
    glb.extend_from_slice(&text);
    glb.extend_from_slice(&word(bin.len()));
    glb.extend_from_slice(b"BIN\0");
    glb.extend_from_slice(&bin);
    glb
}

/// The Warden's carried items: a model each and the joint and socket it is held at.
pub(super) struct Equipment {
    pub items: Vec<(Model, &'static str, three_d_animation::Mat4)>,
    /// Per item, whether it hangs loose on its socket and swings (Atlas 6.6.39, `sway::hangs`).
    pub hanging: Vec<bool>,
    /// The item stowed on [`BOW_STOWED_SOCKET`] that a bow shot takes in hand (#556).
    pub bow: Option<Bow>,
}

/// A carried bow (#556): its item, the [`BOW_HAND_SOCKET`] it is held at through the bow cycle,
/// and its [`BOW_NOCK_JOINT`] node.
pub(super) struct Bow {
    index: usize,
    hand: (&'static str, three_d_animation::Mat4),
    nock: Option<usize>,
}

/// The male Ranger's `humanoid-clips` traversal data for the shared presenter (#500).
pub(super) fn ranger_motions() -> Motions {
    let descriptor = std::str::from_utf8(ranger_file(ranger_bundle::MOTIONS))
        .expect("the humanoid-clips descriptor is UTF-8");
    Motions::parse(descriptor).expect("the humanoid-clips descriptor is humanoid-clips-v1")
}

/// Each clip's cloth chain pose (#500) in a figure's `humanoid-clips` descriptor `bytes`.
fn secondary_of(bytes: &[u8]) -> BTreeMap<String, SecondaryPose> {
    let descriptor: serde_json::Value =
        serde_json::from_slice(bytes).expect("the humanoid-clips descriptor is JSON");
    traversal::secondary_poses(&descriptor)
}

/// The male Ranger's cloth chain poses (#500).
fn ranger_secondary() -> &'static BTreeMap<String, SecondaryPose> {
    static POSES: OnceLock<BTreeMap<String, SecondaryPose>> = OnceLock::new();
    POSES.get_or_init(|| secondary_of(ranger_file(ranger_bundle::MOTIONS)))
}

/// Each clip's `humanoid-clips` `bow` draw (#556): its sample rate and `drawMeters`.
fn bow_draws() -> &'static BTreeMap<String, (f32, Vec<f32>)> {
    static DRAWS: OnceLock<BTreeMap<String, (f32, Vec<f32>)>> = OnceLock::new();
    DRAWS.get_or_init(|| {
        let motions: serde_json::Value =
            serde_json::from_slice(ranger_file(ranger_bundle::MOTIONS))
                .expect("the humanoid-clips descriptor is JSON");
        motions["clips"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|clip| {
                let bow = &clip["bow"];
                let rate = bow["sampleRate"].as_f64()? as f32;
                let meters = bow["drawMeters"]
                    .as_array()?
                    .iter()
                    .map(|v| v.as_f64().map(|v| v as f32))
                    .collect::<Option<Vec<_>>>()?;
                Some((clip["name"].as_str()?.to_owned(), (rate, meters)))
            })
            .collect()
    })
}

/// The `humanoid-sockets` socket `id` as its joint and column-major transform.
fn socket(descriptor: &serde_json::Value, id: &str) -> (&'static str, three_d_animation::Mat4) {
    let entry = descriptor["sockets"]
        .as_array()
        .and_then(|sockets| sockets.iter().find(|socket| socket["id"] == id))
        .unwrap_or_else(|| panic!("the bundle declares socket {id}"));
    let numbers = |key: &str| -> Vec<f32> {
        entry[key]
            .as_array()
            .map(|values| {
                values
                    .iter()
                    .filter_map(|v| v.as_f64())
                    .map(|v| v as f32)
                    .collect()
            })
            .unwrap_or_default()
    };
    let (t, q) = (numbers("translation"), numbers("rotation"));
    let mut matrix = three_d_animation::Mat4::IDENTITY;
    matrix.elements = trs([t[0], t[1], t[2]], [q[0], q[1], q[2], q[3]], [1.; 3]);
    let joint: &'static str = Box::leak(
        entry["joint"]
            .as_str()
            .expect("a socket names its joint")
            .to_owned()
            .into_boxed_str(),
    );
    (joint, matrix)
}

impl Equipment {
    /// Bundle items imported once, each held at its `humanoid-sockets` socket.
    fn of(items: &[(&str, &str)]) -> Self {
        let descriptor: serde_json::Value =
            serde_json::from_slice(ranger_file(ranger_bundle::SOCKETS))
                .expect("the sockets descriptor is JSON");
        let items_of: Vec<_> = items
            .iter()
            .map(|&(entry, socket_id)| {
                let model =
                    Model::parse(ranger_file(entry), "").expect("a bundle item is valid glTF");
                let (joint, matrix) = socket(&descriptor, socket_id);
                (model, joint, matrix)
            })
            .collect();
        let bow = items
            .iter()
            .position(|&(entry, socket_id)| {
                entry == ranger_bundle::BOW && socket_id == BOW_STOWED_SOCKET
            })
            .map(|index| Bow {
                index,
                hand: socket(&descriptor, BOW_HAND_SOCKET),
                nock: items_of[index].0.node(BOW_NOCK_JOINT),
            });
        Self {
            items: items_of,
            hanging: items
                .iter()
                .map(|&(_, socket_id)| sway::hangs(socket_id))
                .collect(),
            bow,
        }
    }

    /// The socket items of a loadout (asset-tooling #375, #379): the sword and shield, the
    /// stowed bow and quiver, the sword, the dagger, a class staff, the crystal staff and tome,
    /// or the mace. The bow rests in its bind pose on the back.
    pub fn loadout(loadout: Loadout) -> &'static Self {
        static SWORD_AND_SHIELD: OnceLock<Equipment> = OnceLock::new();
        static BOW_AND_QUIVER: OnceLock<Equipment> = OnceLock::new();
        static SWORD: OnceLock<Equipment> = OnceLock::new();
        static DAGGER: OnceLock<Equipment> = OnceLock::new();
        static BONE: OnceLock<Equipment> = OnceLock::new();
        static CRYSTAL_AND_TOME: OnceLock<Equipment> = OnceLock::new();
        static MACE: OnceLock<Equipment> = OnceLock::new();
        static GNARLED: OnceLock<Equipment> = OnceLock::new();
        let (cell, items): (_, &[(&str, &str)]) = match loadout {
            Loadout::SwordAndShield => (
                &SWORD_AND_SHIELD,
                &[
                    (ranger_bundle::SWORD, "hand_r.weapon"),
                    (ranger_bundle::SHIELD, "hand_l.shield"),
                ],
            ),
            Loadout::BowAndQuiver => (
                &BOW_AND_QUIVER,
                &[
                    (ranger_bundle::BOW, "back.bow.stowed"),
                    (ranger_bundle::QUIVER, "back.quiver"),
                ],
            ),
            Loadout::Sword => (&SWORD, &[(ranger_bundle::SWORD, "hand_r.weapon")]),
            Loadout::Dagger => (&DAGGER, &[(ranger_bundle::DAGGER, "hand_r.weapon")]),
            Loadout::BoneStaff => (&BONE, &[(ranger_bundle::BONE_STAFF, "hand_r.staff")]),
            // The tome is held like a shield: spine along the forearm.
            Loadout::CrystalStaffAndTome => (
                &CRYSTAL_AND_TOME,
                &[
                    (ranger_bundle::CRYSTAL_STAFF, "hand_r.staff"),
                    (ranger_bundle::TOME, "hand_l.shield"),
                ],
            ),
            Loadout::FlangedMace => (&MACE, &[(ranger_bundle::MACE, "hand_r.weapon")]),
            Loadout::GnarledStaff => (&GNARLED, &[(ranger_bundle::GNARLED_STAFF, "hand_r.staff")]),
        };
        cell.get_or_init(|| Self::of(items))
    }

    /// The authored headwear item worn on the head (#374); the hood is the character's own part
    /// and the class headwear the outfit's (#379).
    pub fn headwear(hat: Hat) -> Option<&'static Self> {
        static CAP: OnceLock<Equipment> = OnceLock::new();
        static HELM: OnceLock<Equipment> = OnceLock::new();
        match hat {
            Hat::Hood | Hat::Class => None,
            Hat::Cap => Some(CAP.get_or_init(|| Self::of(&[(ranger_bundle::CAP, "head.armor")]))),
            Hat::Helm => {
                Some(HELM.get_or_init(|| Self::of(&[(ranger_bundle::HELM, "head.armor")])))
            }
        }
    }
}

/// The Ranger's `humanoid-appearance` descriptor.
fn appearance() -> &'static serde_json::Value {
    static APPEARANCE: OnceLock<serde_json::Value> = OnceLock::new();
    APPEARANCE.get_or_init(|| {
        serde_json::from_slice(ranger_file(ranger_bundle::APPEARANCE))
            .expect("the appearance descriptor is JSON")
    })
}

fn strings(value: &serde_json::Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item.as_str().map(str::to_owned))
        .collect()
}

/// Whether the viewer's headwear hides the hair (#376): the hood, or an item whose
/// `humanoid-appearance` headwear entry lists `hair`. The class headwear hides it only under
/// the outfit's own hood ([`ClassOutfit::hood`]).
pub fn hides_hair(hat: Option<Hat>) -> bool {
    let entry = match hat {
        None | Some(Hat::Class) => return false,
        Some(Hat::Hood) => return true,
        Some(Hat::Cap) => ranger_bundle::CAP_KEY,
        Some(Hat::Helm) => ranger_bundle::HELM_KEY,
    };
    appearance()["headwear"]
        .as_array()
        .and_then(|items| items.iter().find(|item| item["entry"] == entry))
        .is_some_and(|item| {
            strings(&item["hides"])
                .iter()
                .any(|hidden| hidden == "hair")
        })
}

impl Equipment {
    /// The starter hair (#376: Parted, Brown) on `head.hair`; native has no hair picker.
    pub fn hair() -> &'static Self {
        static HAIR: OnceLock<Equipment> = OnceLock::new();
        HAIR.get_or_init(|| {
            let mut hair = Self::of(&[(ranger_bundle::HAIR, "head.hair")]);
            let look = appearance();
            let materials = look["hair"]
                .as_array()
                .and_then(|styles| {
                    styles
                        .iter()
                        .find(|style| style["entry"] == ranger_bundle::HAIR_KEY)
                })
                .map(|style| strings(&style["tintMaterials"]))
                .unwrap_or_default();
            let tint = look["tints"]
                .as_array()
                .and_then(|tints| tints.iter().find(|tint| tint["id"] == "tint.brown"))
                .and_then(|tint| tint["baseColorFactor"].as_array())
                .map(|c| std::array::from_fn(|i| c[i].as_f64().unwrap_or(1.) as f32))
                .expect("the appearance declares tint.brown");
            for (model, _, _) in &mut hair.items {
                model.tint(&materials, tint);
            }
            hair
        })
    }
}

/// The glTF nodes of the Ranger's toggleable hood (`humanoid-appearance` part `outfit.hood`).
pub fn hood_nodes() -> &'static [String] {
    static NODES: OnceLock<Vec<String>> = OnceLock::new();
    NODES.get_or_init(|| {
        appearance()["parts"]
            .as_array()
            .and_then(|parts| parts.iter().find(|part| part["id"] == "outfit.hood"))
            .and_then(|hood| hood["nodes"].as_array())
            .expect("the appearance declares the outfit.hood part")
            .iter()
            .map(|node| node.as_str().expect("a hood node name").to_owned())
            .collect()
    })
}

/// Armour plates on their `humanoid-sockets` sockets, authored in rest model space with the
/// socket as origin: the socket id, the plate and its joint and socket transform.
pub(super) struct Plates {
    items: Vec<(String, Model, &'static str, three_d_animation::Mat4)>,
}

impl Plates {
    /// The `pieces` (`{node, socket}`) of the armour entry embedded at `path`.
    fn of(path: &str, pieces: &serde_json::Value) -> Self {
        let descriptor: serde_json::Value =
            serde_json::from_slice(ranger_file(ranger_bundle::SOCKETS))
                .expect("the sockets descriptor is JSON");
        let armor = Model::parse(ranger_file(path), "").expect("a bundle armour is valid glTF");
        Self {
            items: pieces
                .as_array()
                .into_iter()
                .flatten()
                .map(|piece| {
                    let node = piece["node"].as_str().expect("an armour piece node");
                    let id = piece["socket"].as_str().expect("an armour piece socket");
                    let (joint, matrix) = socket(&descriptor, id);
                    (id.to_owned(), armor.piece(node), joint, matrix)
                })
                .collect(),
        }
    }

    /// Draws of the plates whose socket `keep` accepts, on a character posed at `placement`.
    pub fn draws(
        &self,
        id: &str,
        character: &Model,
        placement: three_d_animation::Mat4,
        pose: &[Layer],
        tint: Option<[f32; 3]>,
        keep: impl Fn(&str) -> bool,
    ) -> Vec<SceneDraw> {
        let mut draws = Vec::new();
        for (index, (socket, model, joint, matrix)) in self.items.iter().enumerate() {
            if !keep(socket) {
                continue;
            }
            let Some(at) = character.joint(joint, pose) else {
                continue;
            };
            let mut held = three_d_animation::Mat4::IDENTITY;
            held.elements = mul(&mul(&placement.elements, &at.elements), &matrix.elements);
            draws.extend(model.draws_placed(&format!("{id}-plate-{index}"), held, &[], tint));
        }
        draws
    }
}

/// The `humanoid-appearance` outfit `id`.
fn outfit_entry(id: &str) -> &'static serde_json::Value {
    appearance()["outfits"]
        .as_array()
        .and_then(|outfits| outfits.iter().find(|outfit| outfit["id"] == id))
        .unwrap_or_else(|| panic!("the appearance declares {id}"))
}

/// A class starter outfit (asset-tooling #379): the character in its outfit variant with the
/// `classTrim` in the class colour, its armour plates and its own hood garment.
pub(super) struct ClassOutfit {
    pub model: Model,
    pub plates: Plates,
    /// The glTF nodes of the outfit's hood garment; empty when it has none.
    pub hood: Vec<String>,
    /// Whether the preset wears hair (the native client draws the starter hair).
    pub hair: bool,
    /// Whether the outfit variant keeps the bundle's own hood for `wayfarer-hood`.
    pub base_hood: bool,
}

/// The starter outfit of `outfit`'s class, imported once per process on first use.
pub(super) fn class_outfit(outfit: Outfit) -> &'static ClassOutfit {
    static OUTFITS: OnceLock<Vec<OnceLock<ClassOutfit>>> = OnceLock::new();
    let slots = OUTFITS.get_or_init(|| {
        ranger_bundle::OUTFITS
            .iter()
            .map(|_| OnceLock::new())
            .collect()
    });
    let index = ranger_bundle::OUTFITS
        .iter()
        .position(|(preset, _, _)| *preset == outfit.preset())
        .expect("every class preset is embedded");
    slots[index].get_or_init(|| {
        let (preset_id, path, armor) = ranger_bundle::OUTFITS[index];
        let preset = appearance()["presets"]
            .as_array()
            .and_then(|presets| presets.iter().find(|preset| preset["id"] == preset_id))
            .expect("the appearance declares the class preset");
        let entry = outfit_entry(preset["outfit"].as_str().expect("a preset outfit"));
        let mut model = Model::parse_with_clips(
            ranger_file(path),
            Kind::Ranger.clips().idle,
            Some(ranger_file(ranger_bundle::CLIPS)),
        )
        .expect("a class outfit is valid glTF")
        .with_secondary(ranger_secondary());
        let trim = &entry["classTrim"];
        let from = trim["defaultBaseColorFactor"]
            .as_array()
            .map(|c| std::array::from_fn(|i| c[i].as_f64().unwrap_or(1.) as f32))
            .expect("the outfit declares its classTrim colour");
        model.retint(&strings(&trim["materials"]), from, outfit.trim());
        // The preset's face, as the browser draws every class outfit but the viewer's own.
        let variant = entry["skins"]
            .as_array()
            .and_then(|skins| skins.iter().find(|skin| skin["id"] == preset["skin"]))
            .and_then(|skin| skin["variant"].as_str())
            .unwrap_or_default();
        model.morph(
            Some(&strings(&appearance()["face"]["nodes"])),
            &residents::face_morphs(Figure::Male, preset, variant),
        );
        let hood = entry["garments"]
            .as_array()
            .and_then(|garments| {
                garments
                    .iter()
                    .find(|garment| garment["id"] == entry["hood"])
            })
            .map(|garment| strings(&garment["nodes"]))
            .unwrap_or_default();
        let base_hood = model.primitives.iter().any(|p| {
            p.node
                .as_ref()
                .is_some_and(|node| hood_nodes().contains(node) && !hood.contains(node))
        });
        ClassOutfit {
            base_hood,
            model,
            plates: Plates::of(armor, &entry["armor"]["pieces"]),
            hood,
            hair: preset["hair"].is_string(),
        }
    })
}

/// The plates equipped drops wear (`imported::DROP_ARMOR_OUTFIT`), imported once.
pub(super) fn drop_plates() -> &'static Plates {
    static PLATES: OnceLock<Plates> = OnceLock::new();
    PLATES.get_or_init(|| {
        Plates::of(
            ranger_bundle::DROP_ARMOR,
            &outfit_entry(DROP_ARMOR_OUTFIT)["armor"]["pieces"],
        )
    })
}

impl Equipment {
    /// Draws of the items held by a character posed at `placement` in `pose`.
    pub fn draws(
        &self,
        id: &str,
        character: &Model,
        placement: three_d_animation::Mat4,
        pose: &[Layer],
        tint: Option<[f32; 3]>,
    ) -> Vec<SceneDraw> {
        self.draws_swung(
            id,
            character,
            placement,
            pose,
            tint,
            sway::Swing::default(),
            None,
        )
    }

    /// [`Self::draws`] with the hanging items turned by `swing` about their grip, in the
    /// model's frame (Atlas 6.6.39), and the bow in hand with its nock drawn through a bow
    /// `shot` (#556).
    #[allow(clippy::too_many_arguments)]
    pub fn draws_swung(
        &self,
        id: &str,
        character: &Model,
        placement: three_d_animation::Mat4,
        pose: &[Layer],
        tint: Option<[f32; 3]>,
        swing: sway::Swing,
        shot: Option<BowShot>,
    ) -> Vec<SceneDraw> {
        let turn = (!swing.is_rest()).then(|| trs([0.; 3], swing.quaternion(), [1.; 3]));
        let mut draws = Vec::new();
        for (index, (model, joint, matrix)) in self.items.iter().enumerate() {
            let bow = self.bow.as_ref().filter(|bow| bow.index == index);
            let in_hand = bow.zip(shot);
            let (joint, matrix) =
                in_hand.map_or((*joint, matrix), |(bow, _)| (bow.hand.0, &bow.hand.1));
            let Some(at) = character.joint(joint, pose) else {
                continue;
            };
            // The item's grip frame is its origin: holder = placement × joint × socket.
            let mut socket = mul(&at.elements, &matrix.elements);
            if let Some((bow, shot)) = in_hand {
                // The nock follows the clip's draw along the bow's draw axis.
                let draw = bow_draws()
                    .get(shot.clip)
                    .map_or(0., |(rate, table)| draw_meters(table, *rate, shot.time));
                let mut held = three_d_animation::Mat4::IDENTITY;
                held.elements = mul(&placement.elements, &socket);
                let moved = bow.nock.map(|nock| (nock, BOW_DRAW_AXIS.map(|a| a * draw)));
                draws.extend(model.draws_moved(&format!("{id}-item-{index}"), held, moved, tint));
                continue;
            }
            if let Some(turn) = turn.filter(|_| self.hanging.get(index) == Some(&true)) {
                // Turn the item about its grip in the model's axes: the grip stays put.
                let grip = [socket[12], socket[13], socket[14]];
                socket = mul(&turn, &socket);
                socket[12..15].copy_from_slice(&grip);
            }
            let mut held = three_d_animation::Mat4::IDENTITY;
            held.elements = mul(&placement.elements, &socket);
            draws.extend(model.draws_placed(&format!("{id}-item-{index}"), held, &[], tint));
        }
        draws
    }
}

fn ranger_file(path: &str) -> &'static [u8] {
    ranger_bundle::FILES
        .iter()
        .find(|(name, _)| *name == path)
        .map(|(_, bytes)| *bytes)
        .expect("prepared Ranger file is embedded")
}

/// The model file of an imported unit kind: asset-tooling's Ranger humanoid for players
/// (Atlas 6.3.7) and the bundled Quaternius CC0 animals (Atlas 6.3.6; see
/// `assets/quaternius/PROVENANCE.md`).
fn source(kind: Kind) -> std::borrow::Cow<'static, [u8]> {
    match kind {
        Kind::Ranger => ranger_file(ranger_bundle::CHARACTER).into(),
        Kind::Wolf => bundled!("Wolf.glb").into(),
        Kind::Rat => bundled!("Rat.glb").into(),
        Kind::Pig => bundled!("Pig.glb").into(),
    }
}

/// The parsed model of `kind`, imported once per process on first use.
pub(super) fn asset(kind: Kind) -> &'static Asset {
    static ASSETS: OnceLock<Vec<OnceLock<Asset>>> = OnceLock::new();
    let slots = ASSETS.get_or_init(|| Kind::VARIANTS.iter().map(|_| OnceLock::new()).collect());
    let index = Kind::VARIANTS
        .iter()
        .position(|candidate| *candidate == kind)
        .expect("every kind is listed");
    slots[index].get_or_init(|| Asset {
        model: Model::parse_with_clips(
            &source(kind),
            kind.clips().idle,
            matches!(kind, Kind::Ranger).then(|| ranger_file(ranger_bundle::CLIPS)),
        )
        .expect("an imported unit model is valid glTF")
        .with_secondary(if matches!(kind, Kind::Ranger) {
            ranger_secondary()
        } else {
            const { &BTreeMap::new() }
        }),
    })
}

#[cfg(test)]
mod rolling_ranger_tests {
    use super::*;

    #[test]
    fn class_outfits_wear_their_plates_trim_and_drops_cover_every_armour_socket() {
        use mmorpg_presentation::units::imported::ARMOR_SOCKETS;
        for &class in mmorpg_core::PlayerClass::VARIANTS {
            let outfit = Outfit::new(class, None);
            let worn = class_outfit(outfit);
            assert!(!worn.plates.items.is_empty(), "{class:?} wears its armour");
            assert!(worn.model.clips.contains_key(Kind::Ranger.clips().idle));
            assert!(
                worn.model
                    .primitives
                    .iter()
                    .any(|p| p.material.as_deref() == Some("class")),
                "{class:?} has classTrim garments"
            );
        }
        // The Ranger, Rogue and Necromancer outfits carry their own hood.
        let hooded: Vec<_> = mmorpg_core::PlayerClass::VARIANTS
            .iter()
            .copied()
            .filter(|&class| !class_outfit(Outfit::new(class, None)).hood.is_empty())
            .collect();
        assert_eq!(
            hooded,
            [
                mmorpg_core::PlayerClass::Ranger,
                mmorpg_core::PlayerClass::Necromancer,
                mmorpg_core::PlayerClass::Rogue,
            ]
        );
        let drops = drop_plates();
        for (_, sockets) in ARMOR_SOCKETS {
            for socket in sockets {
                assert!(
                    drops.items.iter().any(|(id, ..)| id == socket),
                    "a drop covers {socket}"
                );
            }
        }
    }

    #[test]
    fn layered_poses_drive_each_region_from_its_own_clip() {
        let model = &asset(Kind::Ranger).model;
        let node = |name: &str| {
            model
                .names
                .iter()
                .position(|n| n.as_deref() == Some(name))
                .expect(name)
        };
        let (hand, calf, pelvis) = (node("hand_r"), node("calf_l"), node("pelvis"));
        let upper = |node: usize| model.joints[node].upper;
        assert!(upper(hand) && !upper(calf) && !upper(pelvis));
        let swing = Layer::single("Sword_Attack", 0.4);
        let land = Layer::single("Jump_Land", 0.3);
        let layered = model.locals(&[
            Layer {
                mask: Mask::Upper,
                ..swing
            },
            Layer {
                mask: Mask::Lower,
                ..land
            },
        ]);
        let (swing, land) = (model.locals(&[swing]), model.locals(&[land]));
        let close = |a: Rest, b: Rest| {
            a.r.iter().zip(&b.r).all(|(x, y)| (x - y).abs() < 1e-5)
                && a.t.iter().zip(&b.t).all(|(x, y)| (x - y).abs() < 1e-5)
        };
        assert!(close(layered[hand], swing[hand]));
        assert!(close(layered[calf], land[calf]));
        assert!(close(layered[pelvis], land[pelvis]));
        assert_ne!(swing[calf], land[calf]);
        // A grip (asset-tooling #285) owns its hand's fingers over a clip that keys them; the
        // other hand keeps the clip's.
        let (index_r, index_l) = (node("index_02_r"), node("index_02_l"));
        let strafe = Layer::single("Walk_Strafe_Left_Loop", 0.3);
        let grip = Layer {
            mask: Mask::Fingers(mmorpg_presentation::units::imported::Hand::Right),
            ..Layer::single("Grip_Weapon_R", 0.)
        };
        let gripping = model.locals(&[strafe, grip]);
        let (strafe, grip) = (model.locals(&[strafe]), model.locals(&[grip]));
        assert!(close(gripping[index_r], grip[index_r]));
        assert!(!close(gripping[index_r], strafe[index_r]));
        assert!(close(gripping[index_l], strafe[index_l]));
    }

    #[test]
    fn traversal_clips_turn_the_cloth_chains_by_their_secondary_pose() {
        // The Cleric's robe has the skirt and cape chains (asset-tooling #391).
        let outfit = class_outfit(Outfit::new(mmorpg_core::PlayerClass::Cleric, None));
        let model = &outfit.model;
        let Some(skirt) = model.node("skirt_f_01") else {
            panic!("the class outfit has no skirt chain");
        };
        let pose = &ranger_secondary()["Mantle_Mid"];
        let (_, rotations) = pose
            .joints
            .iter()
            .find(|(joint, _)| joint == "skirt_f_01")
            .expect("the mantle turns the front skirt panel");
        let locals = model.locals(&[Layer::single("Mantle_Mid", 0.)]);
        let turned = locals[skirt].r;
        assert!(
            turned
                .iter()
                .zip(&rotations[0])
                .all(|(a, b)| (a - b).abs() < 1e-5),
            "{turned:?}"
        );
        // Other clips leave the chain as authored.
        let idle = model.locals(&[Layer::single("Idle_Loop", 0.)]);
        assert_eq!(idle[skirt], model.rest[skirt]);
    }

    #[test]
    fn a_bow_shot_holds_the_bow_in_hand_and_draws_its_nock() {
        let character = &asset(Kind::Ranger).model;
        let equipment = Equipment::loadout(Loadout::BowAndQuiver);
        let bow = equipment
            .bow
            .as_ref()
            .expect("the bow-and-quiver loadout carries a bow");
        let pose = [Layer::single("Bow_Hold_Loop", 0.5)];
        let shot = BowShot {
            clip: "Bow_Hold_Loop",
            time: 0.5,
        };
        let origin = |shot: Option<BowShot>| {
            let draws = equipment.draws_swung(
                "archer",
                character,
                three_d_animation::Mat4::IDENTITY,
                &pose,
                None,
                sway::Swing::default(),
                shot,
            );
            let draw = draws
                .iter()
                .find(|draw| draw.id.starts_with(&format!("archer-item-{}-", bow.index)))
                .expect("the bow draws");
            [12, 13, 14].map(|i| draw.model.elements[i])
        };
        let joint = |name: &str| {
            let at = character.joint(name, &pose).expect(name);
            [12, 13, 14].map(|i| at.elements[i])
        };
        let distance = |a: [f32; 3], b: [f32; 3]| {
            a.iter()
                .zip(b)
                .map(|(a, b)| (a - b).powi(2))
                .sum::<f32>()
                .sqrt()
        };
        let (hand, back) = (joint(bow.hand.0), joint("spine_03"));
        let (held, stowed) = (origin(Some(shot)), origin(None));
        assert!(
            distance(held, hand) < distance(held, back),
            "held at the hand"
        );
        assert!(
            distance(stowed, back) < distance(stowed, hand),
            "stowed on the back"
        );
        // The nock follows the hold's draw (about 0.48 m) along the bow's -Z.
        let (rate, table) = &bow_draws()["Bow_Hold_Loop"];
        let draw = draw_meters(table, *rate, 0.5);
        assert!((0.4..0.55).contains(&draw), "{draw}");
        let model = &equipment.items[bow.index].0;
        let nock = bow.nock.expect("the bow has a nock joint");
        let mut local = model.rest.clone();
        let braced = model.posed(&model.globals_of(&local));
        local[nock].t[2] -= draw;
        let drawn = model.posed(&model.globals_of(&local));
        let moved = braced
            .iter()
            .zip(&drawn)
            .filter_map(|(a, b)| Some((a.as_ref()?, b.as_ref()?)))
            .flat_map(|(a, b)| a.vertices().iter().zip(b.vertices()))
            .map(|(a, b)| distance([a.x, a.y, a.z], [b.x, b.y, b.z]))
            .fold(0., f32::max);
        assert!((moved - draw).abs() < 0.01, "the string moves {moved} m");
    }

    #[test]
    fn shared_ranger_clips_bind_to_character_joints() {
        let model = &asset(Kind::Ranger).model;
        let clip = &model.clips["Walk_Loop"];
        assert!(clip.duration > 0.);
        assert!(!clip.channels.is_empty());
        assert!(
            clip.channels
                .iter()
                .all(|channel| channel.node < model.rest.len())
        );
        assert!(model.height > 0.);
        let palette = |time| model.palette(&model.globals(&[Layer::single("Walk_Loop", time)]));
        assert_ne!(palette(0.), palette(clip.duration * 0.25));
    }

    #[test]
    fn ambient_animals_skin_their_idle_and_walk_at_the_contract_size() {
        // Heights from asset-tooling docs/greyhaven-ambient-animals.md (#419).
        for (key, height) in [
            ("greyhaven.ambient.cat", 0.45),
            ("greyhaven.ambient.dog", 0.65),
            ("greyhaven.ambient.rodent", 0.17),
        ] {
            let (model, clips) = ambient_model(key).expect(key);
            assert!(
                (model.height - height).abs() < 0.03,
                "{key} {}",
                model.height
            );
            assert!(model.floor.abs() < 0.03, "{key} {}", model.floor);
            let walk = clips["Walk"];
            assert!(clips["Idle"] > 0. && walk > 0.);
            let palette = |time| model.palette(&model.globals(&[Layer::single("Walk", time)]));
            assert_ne!(palette(0.), palette(walk * 0.5));
        }
    }

    /// The rolling kits are read live (#507): every module, roof variant and prop the world
    /// places must still exist in the fetched release and load as drawable glTF.
    #[test]
    fn every_placed_kit_entry_loads_from_the_live_bundles() {
        let scenery = mmorpg_scenery::greyhaven_vale_scenery();
        for part in mmorpg_presentation::shrine::parts(&scenery) {
            assert!(!shrine_draws(part.key).is_empty(), "{}", part.key);
        }
        for part in mmorpg_presentation::outpost_kit::parts(&scenery) {
            assert!(!outpost_draws(part.entry).is_empty(), "{}", part.entry);
        }
        for part in mmorpg_presentation::settlement_props::parts(&scenery) {
            assert!(!settlement_draws(part.entry).is_empty(), "{}", part.entry);
        }
    }

    /// GPU skinning (#595): every figure of a model draws the same bind-pose meshes, frame
    /// after frame; only its palette is its own and follows its pose.
    #[test]
    fn figures_share_bind_meshes_and_pose_by_palette() {
        let model = &asset(Kind::Ranger).model;
        let idle = Kind::Ranger.clips().idle;
        let draws = |unit: usize, time: f32| {
            model.draws(
                &format!("observed-{unit}"),
                Frame {
                    p: [0.0; 3],
                    q: [0.0, 0.0, 0.0, 1.0],
                },
                1.8,
                &[Layer::single(idle, time)],
                None,
                &[],
            )
        };
        let (first, other, later) = (draws(0, 0.25), draws(1, 0.25), draws(0, 0.75));
        assert!(!first.is_empty());
        for ((a, b), c) in first.iter().zip(&other).zip(&later) {
            assert_eq!(a.resource_key, b.resource_key);
            assert!(Arc::ptr_eq(&a.mesh, &b.mesh) && Arc::ptr_eq(&a.mesh, &c.mesh));
            let skin = |draw: &SceneDraw| draw.skin.clone().expect("a posed figure is skinned");
            let (a, b, c) = (skin(a), skin(b), skin(c));
            assert_ne!(a.figure, b.figure);
            assert_eq!(a.palette, b.palette, "the same pose");
            assert_ne!(a.palette, c.palette, "a later pose");
            assert_eq!(a.palette.len(), model.slots.len());
        }
        // Items at rest are static meshes, baked once.
        let rest = || model.draws_placed("rest", three_d_animation::Mat4::IDENTITY, &[], None);
        let (once, again) = (rest(), rest());
        assert!(once.iter().all(|draw| draw.skin.is_none()));
        assert!(
            once.iter()
                .zip(&again)
                .all(|(a, b)| Arc::ptr_eq(&a.mesh, &b.mesh))
        );
    }
}
