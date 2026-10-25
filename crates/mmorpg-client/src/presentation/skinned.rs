//! Imported Quaternius glTF units: material colours baked into vertex colours, clips sampled and
//! skinned on the CPU, then submitted as ordinary scene draws so rendering and picking agree.
//! Presentation only; the authoritative body box still comes from content.
use crate::world::{DrawMaterial, SceneDraw};
use mmorpg_presentation::units::{
    imported::{DROP_ARMOR_OUTFIT, Kind, Layer, Loadout, Mask, Outfit, UPPER_BODY_ROOT},
    kit::{Frame, model},
    player::Hat,
    sway,
};
use std::{
    collections::BTreeMap,
    sync::{Arc, OnceLock},
};
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

fn point(m: &M4, p: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|r| m[r] * p[0] + m[4 + r] * p[1] + m[8 + r] * p[2] + m[12 + r])
}

fn vector(m: &M4, v: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|r| m[r] * v[0] + m[4 + r] * v[1] + m[8 + r] * v[2])
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
    /// Up to four (slot, weight) influences per vertex.
    influences: Vec<[(u16, f32); 4]>,
    /// Influence slots: driving node and its inverse bind matrix.
    slots: Vec<(usize, M4)>,
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
    clips: Arc<BTreeMap<String, Clip>>,
    /// Node names, for attaching items to joints.
    names: Vec<Option<String>>,
    /// Whether each node is in the upper body ([`UPPER_BODY_ROOT`]'s subtree), for masked layers.
    upper: Vec<bool>,
    /// Rest-clip bounds: lowest point and height along +Y, horizontal centre.
    pub floor: f32,
    pub height: f32,
    pub centre: [f32; 2],
    /// The `Head` joint, and the rest head centre's offset from it with the head's radius.
    head: Option<(usize, [f32; 3], f32)>,
}

/// Skinned `(positions, normals)` of one primitive in model space.
type SkinnedPrimitive = (Vec<[f32; 3]>, Vec<[f32; 3]>);

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
                let influences = match (skinned, reader.read_joints(0), reader.read_weights(0)) {
                    (true, Some(joints), Some(weights)) => joints
                        .into_u16()
                        .zip(weights.into_f32())
                        .map(|(j, w)| std::array::from_fn(|i| (j[i], w[i])))
                        .collect(),
                    _ => vec![[(0, 1.), (0, 0.), (0, 0.), (0, 0.)]; positions.len()],
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
                    slots: slots.clone(),
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
        let mut upper = vec![false; count];
        for &node in &order {
            upper[node] = names[node].as_deref() == Some(UPPER_BODY_ROOT)
                || parents[node].is_some_and(|parent| upper[parent]);
        }
        let mut model = Self {
            rest,
            parents,
            order,
            primitives,
            clips: Arc::new(clips),
            names,
            upper,
            floor: 0.,
            height: 1.,
            centre: [0.; 2],
            head: None,
        };
        let (mut min, mut max) = ([f32::MAX; 3], [f32::MIN; 3]);
        for p in model
            .skin(&[Layer::single(rest_clip, 0.)])
            .iter()
            .flat_map(|(p, _)| p)
        {
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
    fn rest_head(
        &self,
        document: &gltf::Document,
        clip: &'static str,
    ) -> Option<(usize, [f32; 3], f32)> {
        let joint = document.nodes().find(|n| n.name() == Some("Head"))?.index();
        let (mut min, mut max) = ([f32::MAX; 3], [f32::MIN; 3]);
        for ((positions, _), p) in self
            .skin(&[Layer::single(clip, 0.)])
            .iter()
            .zip(&self.primitives)
        {
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
        let at = self.globals(&[Layer::single(clip, 0.)])[joint];
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
        let local = self.locals(pose);
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
            for layer in pose {
                let Some(clip) = self.clips.get(layer.clip) else {
                    continue;
                };
                for channel in &clip.channels {
                    if !layer.mask.drives(self.upper[channel.node]) {
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

    /// Skinned positions and normals per primitive in model space.
    fn skin(&self, pose: &[Layer]) -> Vec<SkinnedPrimitive> {
        let globals = self.globals(pose);
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
            clips: Arc::default(),
            names: self.names.clone(),
            upper: self.upper.clone(),
            floor: self.floor,
            height: self.height,
            centre: self.centre,
            head: None,
        }
    }

    /// Recolours the `names` materials by a linear base colour factor, like the browser's tint.
    fn tint(&mut self, names: &[String], factor: [f32; 3]) {
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
        self.draws_filtered(id, self.placement(root, height), pose, tint, hidden)
    }

    /// Draws for one posed instance under an explicit model-to-world `placement`.
    pub fn draws_placed(
        &self,
        id: &str,
        placement: three_d_animation::Mat4,
        pose: &[Layer],
        tint: Option<[f32; 3]>,
    ) -> Vec<SceneDraw> {
        self.draws_filtered(id, placement, pose, tint, &[])
    }

    fn draws_filtered(
        &self,
        id: &str,
        placement: three_d_animation::Mat4,
        pose: &[Layer],
        tint: Option<[f32; 3]>,
        hidden: &[String],
    ) -> Vec<SceneDraw> {
        self.skin(pose)
            .into_iter()
            .zip(&self.primitives)
            .enumerate()
            .filter(|(_, (_, p))| p.node.as_ref().is_none_or(|node| !hidden.contains(node)))
            .filter_map(|(index, ((positions, normals), p))| {
                let colors = match tint {
                    Some([r, g, b]) => p
                        .colors
                        .iter()
                        .map(|c| Color3::new(c.r * r, c.g * g, c.b * b))
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

/// The Greyhaven inn bundle's drawn entries and their resources, embedded by `build.rs` (#158).
mod inn_bundle {
    include!(concat!(env!("OUT_DIR"), "/inn_bundle.rs"));
}

/// The inn's entries as `(key, draws in the entry frame)`, imported once. Static meshes never
/// re-skin: callers only replace each draw's model matrix, so the GPU upload happens once.
pub(super) fn inn_draws() -> &'static [(&'static str, Vec<SceneDraw>)] {
    static DRAWS: OnceLock<Vec<(&'static str, Vec<SceneDraw>)>> = OnceLock::new();
    DRAWS.get_or_init(|| {
        inn_bundle::ENTRIES
            .iter()
            .map(|&(key, path)| {
                let model = Model::parse(&bundle_glb(path, inn_bundle::FILES), "")
                    .expect("an inn entry is valid glTF");
                let draws = model.draws_placed(key, three_d_animation::Mat4::IDENTITY, &[], None);
                (key, draws)
            })
            .collect()
    })
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
        Self {
            items: items
                .iter()
                .map(|&(entry, socket_id)| {
                    let model =
                        Model::parse(ranger_file(entry), "").expect("a bundle item is valid glTF");
                    let (joint, matrix) = socket(&descriptor, socket_id);
                    (model, joint, matrix)
                })
                .collect(),
            hanging: items
                .iter()
                .map(|&(_, socket_id)| sway::hangs(socket_id))
                .collect(),
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
        .expect("a class outfit is valid glTF");
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
            &residents::face_morphs(preset, variant),
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
        self.draws_swung(id, character, placement, pose, tint, sway::Swing::default())
    }

    /// [`Self::draws`] with the hanging items turned by `swing` about their grip, in the
    /// model's frame (Atlas 6.6.39).
    pub fn draws_swung(
        &self,
        id: &str,
        character: &Model,
        placement: three_d_animation::Mat4,
        pose: &[Layer],
        tint: Option<[f32; 3]>,
        swing: sway::Swing,
    ) -> Vec<SceneDraw> {
        let turn = (!swing.is_rest()).then(|| trs([0.; 3], swing.quaternion(), [1.; 3]));
        let mut draws = Vec::new();
        for (index, (model, joint, matrix)) in self.items.iter().enumerate() {
            let Some(at) = character.joint(joint, pose) else {
                continue;
            };
            // The item's grip frame is its origin: holder = placement × joint × socket.
            let mut socket = mul(&at.elements, &matrix.elements);
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
    let slots = ASSETS.get_or_init(|| Kind::ALL.iter().map(|_| OnceLock::new()).collect());
    let index = Kind::ALL
        .iter()
        .position(|candidate| *candidate == kind)
        .expect("every kind is listed");
    slots[index].get_or_init(|| Asset {
        model: Model::parse_with_clips(
            &source(kind),
            kind.clips().idle,
            matches!(kind, Kind::Ranger).then(|| ranger_file(ranger_bundle::CLIPS)),
        )
        .expect("an imported unit model is valid glTF"),
    })
}

#[cfg(test)]
mod rolling_ranger_tests {
    use super::*;

    #[test]
    fn class_outfits_wear_their_plates_trim_and_drops_cover_every_armour_socket() {
        use mmorpg_presentation::units::imported::ARMOR_SOCKETS;
        for class in mmorpg_core::PlayerClass::ALL {
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
        let hooded: Vec<_> = mmorpg_core::PlayerClass::ALL
            .into_iter()
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
        assert!(model.upper[hand] && !model.upper[calf] && !model.upper[pelvis]);
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
        assert_ne!(
            model.skin(&[Layer::single("Walk_Loop", 0.)]),
            model.skin(&[Layer::single("Walk_Loop", clip.duration * 0.25)])
        );
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
            assert_ne!(
                model.skin(&[Layer::single("Walk", 0.)]),
                model.skin(&[Layer::single("Walk", walk * 0.5)])
            );
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
}

#[cfg(test)]
mod allocation_observation {
    use super::*;

    #[test]
    #[ignore = "manual allocation observation for issue #338"]
    fn unchanged_imported_pose_rebuilds_mesh_identity() {
        let model = &asset(Kind::Ranger).model;
        for count in [1, 10, 50] {
            let frame = || {
                (0..count)
                    .flat_map(|unit| {
                        model.draws(
                            &format!("observed-{unit}"),
                            Frame {
                                p: [0.0; 3],
                                q: [0.0, 0.0, 0.0, 1.0],
                            },
                            1.8,
                            &[Layer::single(Kind::Ranger.clips().idle, 0.25)],
                            None,
                            &[],
                        )
                    })
                    .collect::<Vec<_>>()
            };
            let first = frame();
            let second = frame();
            assert_eq!(first.len(), second.len());
            let replaced = first
                .iter()
                .zip(&second)
                .filter(|(a, b)| a.resource_key == b.resource_key && !Arc::ptr_eq(&a.mesh, &b.mesh))
                .count();
            println!(
                "unchanged_pose units={count} mesh_creations={} replaced_identities={replaced}",
                second.len()
            );
            assert_eq!(replaced, second.len());
        }
    }
}
