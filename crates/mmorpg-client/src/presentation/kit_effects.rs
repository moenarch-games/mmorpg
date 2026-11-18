//! The `strike-effects` and `spell-effects` bundles (#894), embedded and drawn where the shared
//! presenter places them ([`mmorpg_presentation::units::kit_effects`]).
//!
//! Every entry is a small rigid hierarchy: its clip moves, turns and scales nodes and drives one
//! morph target (an arc's sweep, a bolt's tail, the beam's crackle). Nodes are posed on the CPU
//! each frame and each morphed primitive is baked once at [`MORPH_STEPS`] weights, so a frame only
//! picks meshes and matrices. Colours come from `COLOR_0` times the material, which a tint
//! replaces.
use super::skinned::bundle_glb;
use crate::world::{DrawMaterial, SceneDraw};
use mmorpg_presentation::units::kit_effects::Instance;
use std::{
    collections::BTreeMap,
    sync::{Arc, OnceLock},
};
use three_d_animation::Mat4;
use three_d_core::{Color3, Mesh, Vec3, VertexAttributes};

/// Embedded strike effects (#894, asset-tooling#725).
mod strike_bundle {
    include!(concat!(env!("OUT_DIR"), "/strike_effects_bundle.rs"));
}

/// Embedded spell effects (#894, asset-tooling#728).
mod spell_bundle {
    include!(concat!(env!("OUT_DIR"), "/spell_effects_bundle.rs"));
}

/// Morph weights baked per morphed primitive, evenly from 0 to 1.
const MORPH_STEPS: usize = 17;

type M4 = [f32; 16];

#[derive(Clone, Copy)]
enum Path {
    Translation,
    Rotation,
    Scale,
    Weight,
}

struct Track {
    node: usize,
    path: Path,
    step: bool,
    times: Vec<f32>,
    values: Vec<[f32; 4]>,
}

impl Track {
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

struct Clip {
    duration: f32,
    tracks: Vec<Track>,
}

/// One primitive: its node, its meshes (one, or one per baked morph weight) and material.
struct Part {
    node: usize,
    meshes: Vec<(String, Arc<Mesh>)>,
    /// The node's rest morph weight.
    weight: f32,
    color: [f32; 3],
    emissive: [f32; 3],
    opacity: f32,
    double_sided: bool,
}

struct Effect {
    /// Rest translation, rotation and scale per node.
    rest: Vec<([f32; 3], [f32; 4], [f32; 3])>,
    parents: Vec<Option<usize>>,
    parts: Vec<Part>,
    clips: BTreeMap<String, Clip>,
}

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

/// glTF colours are linear; the native mesh path expects sRGB.
fn srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1. / 2.4) - 0.055
    }
}

impl Effect {
    fn parse(key: &str, bytes: &[u8]) -> Result<Self, gltf::Error> {
        let (document, buffers, _) = gltf::import_slice(bytes)?;
        let mut parents = vec![None; document.nodes().len()];
        let mut rest = Vec::new();
        for node in document.nodes() {
            rest.push(node.transform().decomposed());
            for child in node.children() {
                parents[child.index()] = Some(node.index());
            }
        }
        let mut parts = Vec::new();
        for node in document.nodes() {
            let Some(mesh) = node.mesh() else { continue };
            let weight = mesh.weights().and_then(|w| w.first().copied()).unwrap_or(0.);
            for primitive in mesh.primitives() {
                let reader = primitive.reader(|b| Some(&buffers[b.index()]));
                let Some(positions) = reader.read_positions() else {
                    continue;
                };
                let positions: Vec<[f32; 3]> = positions.collect();
                let normals: Vec<[f32; 3]> = reader
                    .read_normals()
                    .map_or_else(|| vec![[0., 1., 0.]; positions.len()], Iterator::collect);
                let rgba: Vec<[f32; 4]> = reader
                    .read_colors(0)
                    .map_or_else(|| vec![[1.; 4]; positions.len()], |c| c.into_rgba_f32().collect());
                let indices: Vec<u32> = reader.read_indices().map_or_else(
                    || (0..u32::try_from(positions.len()).unwrap_or(0)).collect(),
                    |i| i.into_u32().collect(),
                );
                let target: Option<(Vec<[f32; 3]>, Vec<[f32; 3]>)> =
                    reader.read_morph_targets().next().map(|(p, n, _)| {
                        (
                            p.map_or_else(Vec::new, Iterator::collect),
                            n.map_or_else(Vec::new, Iterator::collect),
                        )
                    });
                let colors: Vec<Color3> = rgba
                    .iter()
                    .map(|c| Color3::new(srgb(c[0]), srgb(c[1]), srgb(c[2])))
                    .collect();
                let index = parts.len();
                let bake = |w: f32, step: usize| {
                    let moved = |base: &[[f32; 3]], delta: Option<&Vec<[f32; 3]>>| -> Vec<Vec3> {
                        base.iter()
                            .enumerate()
                            .map(|(i, v)| {
                                let d = delta.and_then(|d| d.get(i)).copied().unwrap_or([0.; 3]);
                                Vec3::new(v[0] + d[0] * w, v[1] + d[1] * w, v[2] + d[2] * w)
                            })
                            .collect()
                    };
                    let mesh = Mesh::with_attributes(
                        moved(&positions, target.as_ref().map(|t| &t.0)),
                        indices.clone(),
                        VertexAttributes {
                            normals: Some(
                                moved(&normals, target.as_ref().map(|t| &t.1))
                                    .into_iter()
                                    .map(|n| {
                                        let len = (n.x * n.x + n.y * n.y + n.z * n.z)
                                            .sqrt()
                                            .max(1e-6);
                                        Vec3::new(n.x / len, n.y / len, n.z / len)
                                    })
                                    .collect(),
                            ),
                            colors: Some(colors.clone()),
                            ..Default::default()
                        },
                    )
                    .ok()?;
                    Some((format!("kit-effect:{key}:{index}:{step}"), Arc::new(mesh)))
                };
                #[allow(clippy::cast_precision_loss)]
                let meshes: Vec<_> = if target.is_some() {
                    (0..MORPH_STEPS)
                        .filter_map(|step| bake(step as f32 / (MORPH_STEPS - 1) as f32, step))
                        .collect()
                } else {
                    bake(0., 0).into_iter().collect()
                };
                if meshes.is_empty() {
                    continue;
                }
                let material = primitive.material();
                let base = material.pbr_metallic_roughness().base_color_factor();
                #[allow(clippy::cast_precision_loss)]
                let alpha = rgba.iter().map(|c| c[3]).sum::<f32>() / rgba.len().max(1) as f32;
                parts.push(Part {
                    node: node.index(),
                    meshes,
                    weight,
                    color: [srgb(base[0]), srgb(base[1]), srgb(base[2])],
                    emissive: material.emissive_factor().map(srgb),
                    opacity: match material.alpha_mode() {
                        gltf::material::AlphaMode::Blend => (base[3] * alpha).clamp(0.05, 1.),
                        _ => 1.,
                    },
                    double_sided: material.double_sided(),
                });
            }
        }
        let mut clips = BTreeMap::new();
        for animation in document.animations() {
            let mut tracks = Vec::new();
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
                    // One weight per key: every effect mesh has a single target.
                    O::MorphTargetWeights(v) => {
                        let flat: Vec<f32> = v.into_f32().collect();
                        let per = (flat.len() / times.len().max(1)).max(1);
                        (
                            Path::Weight,
                            flat.chunks(per).map(|w| [w[0], 0., 0., 0.]).collect(),
                        )
                    }
                };
                let interpolation = channel.sampler().interpolation();
                if interpolation == gltf::animation::Interpolation::CubicSpline {
                    values = values.as_chunks::<3>().0.iter().map(|k| k[1]).collect();
                }
                if times.is_empty() || values.len() < times.len() {
                    continue;
                }
                duration = duration.max(*times.last().unwrap_or(&0.));
                tracks.push(Track {
                    node: channel.target().node().index(),
                    path,
                    step: interpolation == gltf::animation::Interpolation::Step,
                    times,
                    values,
                });
            }
            clips.insert(
                animation.name().unwrap_or_default().to_owned(),
                Clip { duration, tracks },
            );
        }
        Ok(Self {
            rest,
            parents,
            parts,
            clips,
        })
    }

    /// Each node's model-space matrix and morph weight with `clip` at `time`.
    fn pose(&self, clip: Option<&str>, time: f32) -> (Vec<M4>, Vec<Option<f32>>) {
        let mut local = self.rest.clone();
        let mut weights = vec![None; self.rest.len()];
        if let Some(clip) = clip.and_then(|clip| self.clips.get(clip)) {
            let t = if clip.duration > 0. {
                time.rem_euclid(clip.duration)
            } else {
                0.
            };
            for track in &clip.tracks {
                let v = track.sample(t);
                let node = &mut local[track.node];
                match track.path {
                    Path::Translation => node.0 = [v[0], v[1], v[2]],
                    Path::Rotation => node.1 = v,
                    Path::Scale => node.2 = [v[0], v[1], v[2]],
                    Path::Weight => weights[track.node] = Some(v[0]),
                }
            }
        }
        let mut globals: Vec<Option<M4>> = vec![None; local.len()];
        fn global(
            node: usize,
            local: &[([f32; 3], [f32; 4], [f32; 3])],
            parents: &[Option<usize>],
            globals: &mut [Option<M4>],
        ) -> M4 {
            if let Some(m) = globals[node] {
                return m;
            }
            let (t, r, s) = local[node];
            let own = trs(t, r, s);
            let m = match parents[node] {
                Some(parent) => mul(&global(parent, local, parents, globals), &own),
                None => own,
            };
            globals[node] = Some(m);
            m
        }
        let matrices = (0..local.len())
            .map(|node| global(node, &local, &self.parents, &mut globals))
            .collect();
        (matrices, weights)
    }

    fn draws(&self, instance: &Instance, out: &mut Vec<SceneDraw>) {
        let (globals, weights) = self.pose(instance.clip, instance.time);
        for (index, part) in self.parts.iter().enumerate() {
            let weight = weights[part.node].unwrap_or(part.weight).clamp(0., 1.);
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::cast_precision_loss
            )]
            let step = ((weight * (part.meshes.len() - 1) as f32).round() as usize)
                .min(part.meshes.len() - 1);
            let (key, mesh) = &part.meshes[step];
            out.push(SceneDraw {
                id: format!("kit-effect-{}-{index}", instance.id),
                resource_key: key.clone(),
                mesh: Arc::clone(mesh),
                model: Mat4 {
                    elements: mul(&instance.model, &globals[part.node]),
                },
                material: DrawMaterial {
                    color: instance.tint.unwrap_or(part.color),
                    emissive: instance.tint.unwrap_or(part.emissive),
                    opacity: part.opacity,
                    unlit: false,
                    double_sided: part.double_sided,
                    ground: None,
                },
                skin: None,
            });
        }
    }
}

/// Every embedded effect by key, imported once.
fn effects() -> &'static BTreeMap<&'static str, Effect> {
    static EFFECTS: OnceLock<BTreeMap<&'static str, Effect>> = OnceLock::new();
    EFFECTS.get_or_init(|| {
        let strikes = strike_bundle::ENTRIES
            .iter()
            .map(|entry| (entry, strike_bundle::FILES));
        let spells = spell_bundle::ENTRIES
            .iter()
            .map(|entry| (entry, spell_bundle::FILES));
        strikes
            .chain(spells)
            .map(|(&(key, path), files)| {
                let effect = Effect::parse(key, &bundle_glb(path, files))
                    .expect("an effect entry is valid glTF");
                (key, effect)
            })
            .collect()
    })
}

/// The draws of the shared presenter's effects this frame.
pub(super) fn draws(instances: &[Instance]) -> Vec<SceneDraw> {
    let mut out = Vec::new();
    for instance in instances {
        if let Some(effect) = effects().get(instance.key) {
            effect.draws(instance, &mut out);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmorpg_presentation::units::kit_effects::ENTRIES;

    #[test]
    fn every_entry_the_presenter_places_is_embedded_with_its_clip() {
        for (key, clip, duration) in ENTRIES {
            let effect = effects().get(key).unwrap_or_else(|| panic!("{key} embedded"));
            assert!(!effect.parts.is_empty(), "{key} draws");
            if let Some(clip) = clip {
                let authored = effect.clips.get(clip).expect(key).duration;
                #[allow(clippy::cast_possible_truncation)]
                let declared = duration as f32;
                assert!((authored - declared).abs() < 0.02, "{key}: {authored}");
            }
        }
    }
}
