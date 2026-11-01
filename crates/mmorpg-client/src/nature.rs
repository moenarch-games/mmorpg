//! Greyhaven's trees, rocks and bushes from the embedded `greyhaven-nature` bundle (#49) and its
//! ladders, ropes and ivy from the embedded `climbing-kit` (#621), imported once into
//! render-engine's mesh, texture and material data. Both bundles' entries share one library and
//! one level selection ([`entries`], [`lods`]).
//!
//! Every level of every entry keeps its textures (mipmapped here, since the engine never
//! generates mipmaps), normal maps with MikkTSpace tangents, vertex colours and alpha masks.
//! Each primitive also gets two dissolve copies for the cross-fade between levels
//! ([`mmorpg_presentation::nature::Shown`]): the same mesh with a per-vertex alpha noise, and
//! with the complementary noise. A fading level draws its copy through a masked material
//! whose cut-off rises with the fade ([`fade_material`]), so the outgoing level dissolves
//! away where the incoming one dissolves in. Presentation only.
use std::{collections::BTreeMap, sync::OnceLock};

use mmorpg_presentation::lod::EntryLods;
use render_engine::{
    glam::{Mat4, Vec2, Vec3, Vec4},
    material::{AlphaMode, Sampler, StandardMaterial, TextureData},
    mesh::MeshData,
};

/// The bundles' entries (`<key>` and `<key>/<variant>`) and resources, embedded by `build.rs`.
mod bundle {
    include!(concat!(env!("OUT_DIR"), "/nature_bundle.rs"));
}

mod descriptor {
    include!(concat!(env!("OUT_DIR"), "/nature_descriptor.rs"));
}

mod climbing_bundle {
    include!(concat!(env!("OUT_DIR"), "/climbing_bundle.rs"));
}

mod climbing_descriptor {
    include!(concat!(env!("OUT_DIR"), "/climbing_descriptor.rs"));
}

type Files = &'static [(&'static str, &'static [u8])];

/// One embedded bundle: the entries the presentation places, its glTF entries and files.
struct Source {
    keys: &'static [&'static str],
    entries: &'static [(&'static str, &'static str)],
    files: Files,
}

const SOURCES: [Source; 2] = [
    Source {
        keys: &mmorpg_presentation::nature::ENTRIES,
        entries: bundle::ENTRIES,
        files: bundle::FILES,
    },
    Source {
        keys: &mmorpg_presentation::climbing::ENTRIES,
        entries: climbing_bundle::ENTRIES,
        files: climbing_bundle::FILES,
    },
];

/// Fade materials per primitive: a fade quantizes to one of these dissolve thresholds.
pub const FADE_STEPS: usize = 8;

/// The library's entries: the nature bundle's (`mmorpg_presentation::nature::ENTRIES`), then
/// the climbing kit's (`mmorpg_presentation::climbing::ENTRIES`).
#[must_use]
pub fn entries() -> Vec<&'static str> {
    SOURCES
        .iter()
        .flat_map(|source| source.keys.iter().copied())
        .collect()
}

/// Every entry's levels of detail from the embedded descriptors.
#[must_use]
pub fn lods() -> Vec<EntryLods> {
    let mut lods = mmorpg_presentation::nature::descriptor_lods(descriptor::DESCRIPTOR)
        .expect("the embedded nature descriptor is valid");
    lods.extend(
        mmorpg_presentation::climbing::descriptor_lods(climbing_descriptor::DESCRIPTOR)
            .expect("the embedded climbing descriptor is valid"),
    );
    lods
}

/// Which textures a primitive's material samples, by index into [`Library::textures`].
#[derive(Clone, Copy, Debug, Default)]
pub struct Textures {
    pub base_color: Option<usize>,
    pub normal: Option<usize>,
    pub metallic_roughness: Option<usize>,
}

/// One primitive of one level, in the entry frame.
pub struct Part {
    pub mesh: MeshData,
    /// The mesh with alpha noise `n` per vertex, and with `1 - n`.
    pub fade_in: MeshData,
    pub fade_out: MeshData,
    /// The material without its textures, which [`Part::textures`] names.
    pub material: StandardMaterial,
    pub textures: Textures,
}

/// Every entry level's primitives and their shared textures.
pub struct Library {
    pub textures: Vec<TextureData>,
    /// Per [`entries`] index, per level (as [`lods`] orders them), the primitives.
    pub levels: Vec<Vec<Vec<Part>>>,
}

/// The library, imported on first use.
pub fn library() -> &'static Library {
    static LIBRARY: OnceLock<Library> = OnceLock::new();
    LIBRARY.get_or_init(|| {
        let lods = lods();
        let mut textures = TextureSet::default();
        let levels = SOURCES
            .iter()
            .flat_map(|source| source.keys.iter().map(move |entry| (source, entry)))
            .map(|(source, entry)| {
                let Some(entry_lods) = lods.iter().find(|lods| lods.entry == *entry) else {
                    return Vec::new();
                };
                entry_lods
                    .levels
                    .iter()
                    .map(|(variant, _)| {
                        let key = if variant == "default" {
                            (*entry).to_owned()
                        } else {
                            format!("{entry}/{variant}")
                        };
                        let path = source
                            .entries
                            .iter()
                            .find(|(name, _)| *name == key)
                            .map(|(_, path)| *path)
                            .unwrap_or_else(|| panic!("a bundle embeds {key}"));
                        import(path, source.files, &mut textures)
                    })
                    .collect()
            })
            .collect();
        Library {
            textures: textures.data,
            levels,
        }
    })
}

/// The material a fading copy draws with at dissolve `threshold` in `(0, 1]`: a fragment of
/// vertex alpha `a` is kept while `a >= threshold` (with the texture's own mask on top).
#[must_use]
pub fn fade_material(material: &StandardMaterial, threshold: f32) -> StandardMaterial {
    let cutoff = match material.alpha_mode {
        AlphaMode::Mask(cutoff) => cutoff,
        AlphaMode::Opaque | AlphaMode::Blend => 1.0,
    };
    let mut faded = *material;
    faded.base_color.w = material.base_color.w / threshold.max(1e-3);
    faded.alpha_mode = AlphaMode::Mask(cutoff);
    faded
}

/// The dissolve threshold of fade step `step` (0..[`FADE_STEPS`]).
#[must_use]
pub fn fade_threshold(step: usize) -> f32 {
    (step as f32 + 0.5) / FADE_STEPS as f32
}

#[derive(Default)]
struct TextureSet {
    data: Vec<TextureData>,
    by_path: BTreeMap<(String, bool), usize>,
}

fn file(files: Files, path: &str) -> &'static [u8] {
    files
        .iter()
        .find(|(name, _)| *name == path)
        .map(|(_, bytes)| *bytes)
        .unwrap_or_else(|| panic!("a bundle embeds {path}"))
}

/// Normalizes `base`'s directory joined with a relative `uri`.
pub(crate) fn resolve(base: &str, uri: &str) -> String {
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

/// A decoded PNG or JPEG with a box-filtered mip chain; colour textures are sRGB.
pub(crate) fn texture(bytes: &[u8], srgb: bool) -> TextureData {
    let image = image::load_from_memory(bytes)
        .expect("bundle textures are PNG or JPEG")
        .to_rgba8();
    let (width, height) = image.dimensions();
    let mut levels = vec![image.into_raw()];
    let (mut w, mut h) = (width as usize, height as usize);
    while w > 1 || h > 1 {
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let previous = levels.last().expect("level 0 exists");
        let mut next = vec![0u8; nw * nh * 4];
        for y in 0..nh {
            for x in 0..nw {
                for channel in 0..4 {
                    let mut sum = 0u32;
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let sx = (x * 2 + dx).min(w - 1);
                        let sy = (y * 2 + dy).min(h - 1);
                        sum += u32::from(previous[(sy * w + sx) * 4 + channel]);
                    }
                    next[(y * nw + x) * 4 + channel] = ((sum + 2) / 4) as u8;
                }
            }
        }
        levels.push(next);
        (w, h) = (nw, nh);
    }
    TextureData {
        width,
        height,
        format: if srgb {
            render_engine::wgpu::TextureFormat::Rgba8UnormSrgb
        } else {
            render_engine::wgpu::TextureFormat::Rgba8Unorm
        },
        levels,
        sampler: Sampler::default(),
    }
}

/// A stable per-vertex noise in `(0, 1]`.
fn noise(index: usize) -> f32 {
    let mut x = (index as u32).wrapping_mul(0x9e37_79b9) ^ 0x85eb_ca6b;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2c1b_3c6d);
    x ^= x >> 12;
    ((x >> 8) as f32 + 1.) / (1 << 24) as f32
}

fn with_alpha(mesh: &MeshData, alpha: impl Fn(usize) -> f32) -> MeshData {
    let mut faded = mesh.clone();
    if faded.colors.len() != faded.positions.len() {
        faded.colors = vec![Vec4::ONE; faded.positions.len()];
    }
    for (index, color) in faded.colors.iter_mut().enumerate() {
        color.w = alpha(index);
    }
    faded
}

/// Every primitive of the bundle glTF at `path`, in its scene frame.
fn import(path: &str, files: Files, textures: &mut TextureSet) -> Vec<Part> {
    let gltf = gltf::Gltf::from_slice(file(files, path)).expect("a bundle entry is valid glTF");
    let buffers: Vec<&[u8]> = gltf
        .buffers()
        .map(|buffer| match buffer.source() {
            gltf::buffer::Source::Uri(uri) => file(files, &resolve(path, uri)),
            gltf::buffer::Source::Bin => panic!("nature entries use external buffers"),
        })
        .collect();
    let mut slot = |info: Option<gltf::texture::Texture<'_>>, srgb: bool| {
        let info = info?;
        let gltf::image::Source::Uri { uri, .. } = info.source().source() else {
            return None;
        };
        let resolved = resolve(path, uri);
        Some(
            *textures
                .by_path
                .entry((resolved.clone(), srgb))
                .or_insert_with(|| {
                    textures.data.push(texture(file(files, &resolved), srgb));
                    textures.data.len() - 1
                }),
        )
    };
    let scene = gltf
        .default_scene()
        .or_else(|| gltf.scenes().next())
        .expect("a nature entry has a scene");
    let mut parts = Vec::new();
    let mut stack: Vec<(gltf::Node<'_>, Mat4)> =
        scene.nodes().map(|node| (node, Mat4::IDENTITY)).collect();
    while let Some((node, parent)) = stack.pop() {
        let world = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
        stack.extend(node.children().map(|child| (child, world)));
        let Some(mesh) = node.mesh() else {
            continue;
        };
        let normal_matrix = world.inverse().transpose();
        for primitive in mesh.primitives() {
            let reader = primitive.reader(|buffer| buffers.get(buffer.index()).copied());
            let Some(positions) = reader.read_positions() else {
                continue;
            };
            let mut data = MeshData {
                positions: positions
                    .map(|p| world.transform_point3(Vec3::from(p)))
                    .collect(),
                ..MeshData::default()
            };
            data.normals = reader.read_normals().map_or_else(
                || vec![Vec3::Y; data.positions.len()],
                |normals| {
                    normals
                        .map(|n| {
                            normal_matrix
                                .transform_vector3(Vec3::from(n))
                                .normalize_or(Vec3::Y)
                        })
                        .collect()
                },
            );
            if let Some(uvs) = reader.read_tex_coords(0) {
                data.uvs = uvs.into_f32().map(Vec2::from).collect();
            }
            if let Some(colors) = reader.read_colors(0) {
                data.colors = colors.into_rgba_f32().map(Vec4::from).collect();
            }
            data.indices = reader.read_indices().map_or_else(
                || (0..data.positions.len() as u32).collect(),
                |indices| indices.into_u32().collect(),
            );
            let source = primitive.material();
            let pbr = source.pbr_metallic_roughness();
            let textures = Textures {
                base_color: slot(pbr.base_color_texture().map(|t| t.texture()), true),
                normal: slot(source.normal_texture().map(|t| t.texture()), false),
                metallic_roughness: slot(
                    pbr.metallic_roughness_texture().map(|t| t.texture()),
                    false,
                ),
            };
            if textures.normal.is_some() && data.generate_tangents().is_err() {
                data.tangents.clear();
            }
            let material = StandardMaterial {
                base_color: Vec4::from(pbr.base_color_factor()),
                metallic: pbr.metallic_factor(),
                perceptual_roughness: pbr.roughness_factor(),
                alpha_mode: match source.alpha_mode() {
                    gltf::material::AlphaMode::Mask => {
                        AlphaMode::Mask(source.alpha_cutoff().unwrap_or(0.5))
                    }
                    gltf::material::AlphaMode::Blend => AlphaMode::Blend,
                    gltf::material::AlphaMode::Opaque => AlphaMode::Opaque,
                },
                double_sided: source.double_sided(),
                ..StandardMaterial::default()
            };
            let fade_in = with_alpha(&data, noise);
            let fade_out = with_alpha(&data, |index| 1. - noise(index) + 1. / FADE_STEPS as f32);
            parts.push(Part {
                mesh: data,
                fade_in,
                fade_out,
                material,
                textures,
            });
        }
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_entry_level_imports_with_its_textures() {
        let library = library();
        let lods = lods();
        for (entry, levels) in entries().iter().zip(&library.levels) {
            let expected = lods.iter().find(|lods| lods.entry == *entry).unwrap();
            assert_eq!(levels.len(), expected.levels.len(), "{entry}");
            for parts in levels {
                assert!(!parts.is_empty(), "{entry}");
                assert!(
                    parts.iter().all(|part| part.textures.base_color.is_none()
                        || part.mesh.uvs.len() == part.mesh.positions.len()),
                    "{entry}"
                );
            }
        }
        assert!(library.textures.iter().all(|t| t.levels.len() > 1));
    }

    /// The core rock proxies (`mmorpg_core::ROCK_PROXIES`) transcribe the bundle's
    /// `collisionProxy` hulls and bounds to the centimetre (vertices within rounding).
    #[test]
    fn core_rock_proxies_match_the_bundle() {
        let value: serde_json::Value = serde_json::from_str(descriptor::DESCRIPTOR).unwrap();
        let rocks: Vec<_> = value["assets"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|asset| asset["family"] == "rock")
            .collect();
        assert_eq!(rocks.len(), mmorpg_core::ROCK_PROXIES.len());
        let cm = |v: &serde_json::Value| (v.as_f64().unwrap() * 100.).round() as i32;
        for (rock, proxy) in rocks.iter().zip(&mmorpg_core::ROCK_PROXIES) {
            let vertices: Vec<[i32; 3]> = rock["collisionProxy"]["vertices"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| std::array::from_fn(|axis| cm(&v[axis])))
                .collect();
            assert_eq!(vertices.len(), proxy.vertices.len(), "{}", rock["key"]);
            for (bundle, core) in vertices.iter().zip(proxy.vertices) {
                assert!(
                    (0..3).all(|axis| (bundle[axis] - core[axis]).abs() <= 1),
                    "{}: {bundle:?} vs {core:?}",
                    rock["key"]
                );
            }
            let bound = |side: &str| -> [i32; 3] {
                std::array::from_fn(|axis| cm(&rock["bounds"][side][axis]))
            };
            assert_eq!((bound("min"), bound("max")), (proxy.min, proxy.max));
        }
    }

    /// The bush footprints the presentation fits bushes by are the bundle's (#596).
    #[test]
    fn bush_sizes_match_the_bundle() {
        use mmorpg_presentation::nature::{BUSH_SIZES, BUSHES};
        let value: serde_json::Value = serde_json::from_str(descriptor::DESCRIPTOR).unwrap();
        for (key, size) in BUSHES.iter().zip(BUSH_SIZES) {
            let asset = value["assets"]
                .as_array()
                .unwrap()
                .iter()
                .find(|asset| asset["key"] == *key)
                .unwrap_or_else(|| panic!("the bundle declares {key}"));
            let footprint = &asset["footprint"];
            let bundle = ["width", "height", "depth"].map(|axis| footprint[axis].as_f64().unwrap());
            assert!(
                (0..3).all(|axis| (bundle[axis] - size[axis]).abs() < 0.01),
                "{key}: {bundle:?} vs {size:?}"
            );
        }
    }
}
