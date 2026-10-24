//! The shared static world (`mmorpg-presentation`, #184) for the browser renderer.
//!
//! The browser draws exactly the draws the native client lowers: the same
//! terrain, roads, water and prop geometry, IDs, resource keys, transforms and
//! materials. The layout is JSON (format [`STATIC_SCENE_FORMAT`]); bulk numbers travel as typed arrays:
//!
//! - **geometries**: one entry per distinct resource, with offsets into
//!   [`vertices`] (positions, then normals, then linear vertex colours when
//!   `colored`, three floats per vertex each) and [`indices`];
//! - **groups**: static colour batches (`instanced: false`, one draw) and
//!   instance groups (one draw per placement), with their cull class and XZ
//!   bounds; each draw names its geometry, a model matrix index into
//!   [`models`] (16 column-major floats) and its material;
//! - **water** draws and **animated** parts, whose matrices [`motion`]
//!   computes per frame.
//!
//! Geometry is built once per page; nothing here is gameplay, and the local
//! zone host never reads it.

use std::sync::OnceLock;

use mmorpg_presentation::{Draw, StaticScene};
use serde::Serialize;

use crate::scenery::hosted_scenery;

pub const STATIC_SCENE_FORMAT: &str = "mmorpg.static-scene";

#[must_use]
pub fn hosted_static_scene() -> &'static StaticScene {
    static SCENE: OnceLock<StaticScene> = OnceLock::new();
    SCENE.get_or_init(|| StaticScene::new(hosted_scenery()))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Layout {
    format: &'static str,
    prefix: String,
    geometries: Vec<GeometryEntry>,
    groups: Vec<GroupEntry>,
    water: Vec<DrawEntry>,
    animated: Vec<Vec<DrawEntry>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GeometryEntry {
    resource_key: String,
    vertex_offset: usize,
    vertex_count: usize,
    colored: bool,
    index_offset: usize,
    index_count: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GroupEntry {
    cull: &'static str,
    instanced: bool,
    min: [f64; 2],
    max: [f64; 2],
    draws: Vec<DrawEntry>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DrawEntry {
    id: String,
    geometry: usize,
    model: usize,
    color: String,
    /// `None` for the default lit material.
    emissive: Option<String>,
    opacity: f32,
}

struct Buffers {
    keys: Vec<String>,
    geometries: Vec<GeometryEntry>,
    vertices: Vec<f32>,
    indices: Vec<u32>,
    models: Vec<f32>,
}

fn hex(color: [f32; 3]) -> String {
    let byte = |channel: f32| (channel.clamp(0., 1.) * 255.).round() as u8;
    format!(
        "#{:02x}{:02x}{:02x}",
        byte(color[0]),
        byte(color[1]),
        byte(color[2])
    )
}

impl Buffers {
    fn draw(&mut self, draw: &Draw) -> DrawEntry {
        let geometry =
            if let Some(index) = self.keys.iter().position(|key| *key == draw.resource_key) {
                index
            } else {
                let g = &draw.geometry;
                let vertex_offset = self.vertices.len();
                for values in [&g.p, &g.n] {
                    self.vertices
                        .extend(values.iter().flatten().map(|value| *value as f32));
                }
                let colored = !g.colors.is_empty();
                self.vertices
                    .extend(g.colors.iter().flatten().map(|value| *value as f32));
                let index_offset = self.indices.len();
                self.indices.extend(&g.indices);
                self.keys.push(draw.resource_key.clone());
                self.geometries.push(GeometryEntry {
                    resource_key: draw.resource_key.clone(),
                    vertex_offset,
                    vertex_count: g.p.len(),
                    colored,
                    index_offset,
                    index_count: g.indices.len(),
                });
                self.keys.len() - 1
            };
        let model = self.models.len() / 16;
        self.models.extend(draw.model);
        let material = &draw.material;
        DrawEntry {
            id: draw.id.clone(),
            geometry,
            model,
            color: hex(material.color),
            emissive: (material.emissive != [0.; 3]).then(|| hex(material.emissive)),
            opacity: material.opacity,
        }
    }
}

fn buffers() -> &'static (String, Vec<f32>, Vec<u32>, Vec<f32>) {
    static BUFFERS: OnceLock<(String, Vec<f32>, Vec<u32>, Vec<f32>)> = OnceLock::new();
    BUFFERS.get_or_init(|| encode(hosted_static_scene()))
}

pub(crate) fn encode(scene: &StaticScene) -> (String, Vec<f32>, Vec<u32>, Vec<f32>) {
    let mut buffers = Buffers {
        keys: Vec::new(),
        geometries: Vec::new(),
        vertices: Vec::new(),
        indices: Vec::new(),
        models: Vec::new(),
    };
    let groups = scene
        .groups
        .iter()
        .map(|group| GroupEntry {
            cull: group.cull.name(),
            instanced: group.instanced,
            min: group.min,
            max: group.max,
            draws: group.draws.iter().map(|draw| buffers.draw(draw)).collect(),
        })
        .collect();
    let water = scene.water.iter().map(|draw| buffers.draw(draw)).collect();
    let animated = scene
        .animated
        .iter()
        .map(|part| part.draws.iter().map(|draw| buffers.draw(draw)).collect())
        .collect();
    let layout = Layout {
        format: STATIC_SCENE_FORMAT,
        prefix: scene.prefix.clone(),
        geometries: std::mem::take(&mut buffers.geometries),
        groups,
        water,
        animated,
    };
    (
        serde_json::to_string(&layout).expect("the static scene layout is plain data"),
        buffers.vertices,
        buffers.indices,
        buffers.models,
    )
}

/// The JSON layout of the hosted static scene.
#[must_use]
pub fn layout() -> &'static str {
    &buffers().0
}

/// Every geometry's positions, normals and optional colours, in layout order.
#[must_use]
pub fn vertices() -> &'static [f32] {
    &buffers().1
}

#[must_use]
pub fn indices() -> &'static [u32] {
    &buffers().2
}

/// Column-major model matrices, 16 floats per draw.
#[must_use]
pub fn models() -> &'static [f32] {
    &buffers().3
}

/// One model matrix (16 floats) per animated part at a cosmetic clock.
#[must_use]
pub fn motion(seconds: f64, animate: bool) -> Vec<f32> {
    hosted_static_scene()
        .motion(seconds, animate)
        .into_iter()
        .flatten()
        .collect()
}

/// Per animated part 1 (drawn) or 0 (cut away, a cave roof over the `viewer`'s feet in
/// metres; empty without a viewer), or a kit's fallback while `kits` draws that kit.
#[must_use]
pub fn shown(viewer: &[f64], kits: mmorpg_presentation::Kits) -> Vec<u8> {
    let viewer = (viewer.len() == 3).then(|| [viewer[0], viewer[1], viewer[2]]);
    hosted_static_scene()
        .shown(viewer, kits)
        .into_iter()
        .map(u8::from)
        .collect()
}

/// Rendered terrain height in metres at `(x, z)` metres.
#[must_use]
pub fn surface_y(x: f64, z: f64) -> f64 {
    hosted_static_scene().surface_y(x, z)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_layout_indexes_every_draw_into_the_shared_buffers() {
        let layout: serde_json::Value = serde_json::from_str(layout()).unwrap();
        let geometries = layout["geometries"].as_array().unwrap();
        let last = geometries.last().unwrap();
        let floats_per_vertex = |entry: &serde_json::Value| {
            if entry["colored"].as_bool().unwrap() {
                9
            } else {
                6
            }
        };
        assert_eq!(
            vertices().len(),
            last["vertexOffset"].as_u64().unwrap() as usize
                + last["vertexCount"].as_u64().unwrap() as usize * floats_per_vertex(last)
        );
        assert_eq!(
            indices().len(),
            last["indexOffset"].as_u64().unwrap() as usize
                + last["indexCount"].as_u64().unwrap() as usize
        );
        let scene = hosted_static_scene();
        let draws = scene
            .groups
            .iter()
            .map(|group| group.draws.len())
            .sum::<usize>()
            + scene.water.len()
            + scene
                .animated
                .iter()
                .map(|part| part.draws.len())
                .sum::<usize>();
        assert_eq!(models().len(), draws * 16);
        assert_eq!(motion(1.0, true).len(), scene.animated.len() * 16);
    }
}
