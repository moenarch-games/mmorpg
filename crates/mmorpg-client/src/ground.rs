//! Greyhaven's ground layer materials (#525) from the embedded `greyhaven-terrain` bundle:
//! per [`Layer`], the base colour (alpha = height), normal and occlusion-roughness-metal
//! textures of its glTF material, mipmapped here since the engine never generates mipmaps.
//! A layer the bundle lacks is absent, and its draws stay flat in their vertex colours
//! (`mmorpg_presentation::ground`'s fallback). Presentation only.
use std::{collections::BTreeMap, sync::OnceLock};

use mmorpg_presentation::ground::Layer;
use render_engine::{glam::Vec3, material::TextureData};

use crate::nature::{resolve, texture};

mod bundle {
    include!(concat!(env!("OUT_DIR"), "/terrain_bundle.rs"));
}

/// One layer's textures.
pub struct LayerTextures {
    pub base_color: TextureData,
    pub normal: Option<TextureData>,
    /// Occlusion in red, roughness in green, metal in blue (glTF).
    pub occlusion_roughness_metal: Option<TextureData>,
    /// The base colour texture's mean in linear light: the material divides by it, so the
    /// texture adds detail around the vertex colours' palette.
    pub mean: Vec3,
}

fn file(path: &str) -> Option<&'static [u8]> {
    bundle::FILES
        .iter()
        .find(|(name, _)| *name == path)
        .map(|(_, bytes)| *bytes)
}

fn linear(channel: u8) -> f32 {
    let c = f32::from(channel) / 255.;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// The layer's material from its bundle entry, or `None` when the bundle lacks it.
fn import(layer: Layer) -> Option<LayerTextures> {
    let key = layer.key();
    let path = bundle::ENTRIES
        .iter()
        .find(|(name, _)| *name == key)
        .map(|(_, path)| *path)?;
    let gltf = gltf::Gltf::from_slice(file(path)?).ok()?;
    let material = gltf.materials().next()?;
    let image = |info: Option<gltf::texture::Texture<'_>>, srgb: bool| {
        let gltf::image::Source::Uri { uri, .. } = info?.source().source() else {
            return None;
        };
        Some(texture(file(&resolve(path, uri))?, srgb))
    };
    let pbr = material.pbr_metallic_roughness();
    let base_color = image(pbr.base_color_texture().map(|t| t.texture()), true)?;
    let texels = &base_color.levels[0];
    let mut sum = Vec3::ZERO;
    for texel in texels.as_chunks::<4>().0 {
        sum += Vec3::new(linear(texel[0]), linear(texel[1]), linear(texel[2]));
    }
    let mean = (sum / (texels.len() / 4).max(1) as f32).max(Vec3::splat(1e-3));
    Some(LayerTextures {
        normal: image(material.normal_texture().map(|t| t.texture()), false),
        occlusion_roughness_metal: image(
            pbr.metallic_roughness_texture().map(|t| t.texture()),
            false,
        ),
        base_color,
        mean,
    })
}

/// Every layer the bundle has, imported on first use.
pub fn library() -> &'static BTreeMap<Layer, LayerTextures> {
    static LIBRARY: OnceLock<BTreeMap<Layer, LayerTextures>> = OnceLock::new();
    LIBRARY.get_or_init(|| {
        Layer::ALL
            .into_iter()
            .filter_map(|layer| Some((layer, import(layer)?)))
            .collect()
    })
}
