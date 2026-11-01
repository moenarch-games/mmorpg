//! Greyhaven's ground layer materials (#525) from the embedded `greyhaven-terrain` bundle:
//! per [`Layer`], the base colour (alpha = height), normal and occlusion-roughness-metal
//! textures of its glTF material. They ship as KTX2 (Basis Universal, asset-tooling#475) with
//! their mip chains, transcoded here to RGBA8 by the pure-Rust `basisu` transcoder. A layer the
//! bundle lacks is absent, and its draws stay flat in their vertex colours
//! (`mmorpg_presentation::ground`'s fallback). Presentation only.
use std::{collections::BTreeMap, sync::OnceLock};

use mmorpg_presentation::ground::Layer;
use render_engine::{
    glam::Vec3,
    material::{Sampler, TextureData},
};

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

const KTX2_IDENTIFIER: [u8; 12] = *b"\xABKTX 20\xBB\r\n\x1A\n";

/// A KTX2 (Basis Universal) texture transcoded to RGBA8 with the file's own mip chain, or a
/// PNG or JPEG mipmapped here.
fn decode(bytes: &[u8], srgb: bool) -> Option<TextureData> {
    if !bytes.starts_with(&KTX2_IDENTIFIER) {
        return Some(texture(bytes, srgb));
    }
    let transcoder = basisu::Transcoder::new(bytes).ok()?;
    let (width, height) = transcoder.base_dimensions();
    let levels = (0..transcoder.level_count())
        .map(|level| {
            transcoder
                .transcode(
                    level,
                    basisu::TargetFormat::Rgba32,
                    basisu::DecodeFlags::NONE,
                )
                .ok()
        })
        .collect::<Option<Vec<_>>>()?;
    Some(TextureData {
        width,
        height,
        format: if srgb {
            render_engine::wgpu::TextureFormat::Rgba8UnormSrgb
        } else {
            render_engine::wgpu::TextureFormat::Rgba8Unorm
        },
        levels,
        sampler: Sampler::default(),
    })
}

/// The layer's material from its bundle entry, or `None` when the bundle lacks it. The glTF
/// is read as JSON: its textures are `KHR_texture_basisu` (KTX2) sources, which the `gltf`
/// crate does not resolve.
fn import(layer: Layer) -> Option<LayerTextures> {
    let key = layer.key();
    let path = bundle::ENTRIES
        .iter()
        .find(|(name, _)| *name == key)
        .map(|(_, path)| *path)?;
    let gltf: serde_json::Value = serde_json::from_slice(file(path)?).ok()?;
    let material = &gltf["materials"][0];
    let image = |info: &serde_json::Value, srgb: bool| {
        let texture = &gltf["textures"][usize::try_from(info["index"].as_u64()?).ok()?];
        let source = texture["extensions"]["KHR_texture_basisu"]["source"]
            .as_u64()
            .or_else(|| texture["source"].as_u64())?;
        let uri = gltf["images"][usize::try_from(source).ok()?]["uri"].as_str()?;
        decode(file(&resolve(path, uri))?, srgb)
    };
    let pbr = &material["pbrMetallicRoughness"];
    let base_color = image(&pbr["baseColorTexture"], true)?;
    let texels = &base_color.levels[0];
    let mut sum = Vec3::ZERO;
    for texel in texels.as_chunks::<4>().0 {
        sum += Vec3::new(linear(texel[0]), linear(texel[1]), linear(texel[2]));
    }
    let mean = (sum / (texels.len() / 4).max(1) as f32).max(Vec3::splat(1e-3));
    Some(LayerTextures {
        normal: image(&material["normalTexture"], false),
        occlusion_roughness_metal: image(&pbr["metallicRoughnessTexture"], false),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_layer_imports_its_three_textures_with_full_mip_chains() {
        let library = library();
        assert_eq!(library.len(), Layer::ALL.len());
        for textures in library.values() {
            let maps = [
                Some(&textures.base_color),
                textures.normal.as_ref(),
                textures.occlusion_roughness_metal.as_ref(),
            ];
            for map in maps {
                let map = map.expect("every layer has its normal and AO/roughness/metal maps");
                assert_eq!(
                    map.levels.len() as u32,
                    map.width.max(map.height).ilog2() + 1
                );
                assert_eq!(map.levels[0].len() as u32, map.width * map.height * 4);
            }
        }
    }
}
