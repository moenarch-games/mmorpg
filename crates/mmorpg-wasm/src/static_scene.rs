//! The shared static world (`mmorpg-presentation`, #184) for the browser renderer.
//!
//! The browser draws exactly the draws the native client lowers: the same
//! terrain, roads, water and prop geometry, IDs, resource keys, transforms and
//! materials. The layout is JSON (format [`STATIC_SCENE_FORMAT`]); bulk numbers travel as typed arrays:
//!
//! - **geometries**: one entry per distinct resource, with offsets into
//!   [`vertices`] (positions, then normals, then linear vertex colours when
//!   `colored`, three floats per vertex each; then, for a textured ground layer (#525),
//!   texture coordinates and tangents when `textured`, two and four floats per vertex, and
//!   weights when `weighted`, one per vertex) and [`indices`];
//! - **groups**: static colour batches (`instanced: false`, one draw) and
//!   instance groups (one draw per placement), with their cull class and XZ
//!   bounds; each draw names its geometry, a model matrix index into
//!   [`models`] (16 column-major floats) and its material;
//! - **water** draws and **animated** parts, whose matrices [`motion`]
//!   computes per frame.
//!
//! Geometry is built once per page; nothing here is gameplay, and the local
//! zone host never reads it.

use mmorpg_presentation::tiles::{BUILDS_PER_FRAME, Level, Streamer};
use mmorpg_presentation::{Draw, StaticScene};
use serde::Serialize;
use wasm_bindgen::prelude::wasm_bindgen;

use crate::scenery::{PerZone, hosted_scenery};

pub const STATIC_SCENE_FORMAT: &str = "mmorpg.static-scene";

#[must_use]
pub fn hosted_static_scene() -> &'static StaticScene {
    static SCENE: PerZone<StaticScene> = PerZone::new();
    SCENE.get(|_| StaticScene::new(hosted_scenery()))
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
    /// Texture coordinates (two floats per vertex) then tangents (four) follow.
    textured: bool,
    /// One weight per vertex (the vertex colour alpha) follows.
    weighted: bool,
    index_offset: usize,
    index_count: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GroupEntry {
    cull: &'static str,
    instanced: bool,
    /// `"nature"` for the procedural trees, rocks and bushes the nature bundle replaces (#49).
    fallback: Option<&'static str>,
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
    /// The textured ground layer a terrain draw shows (#525).
    #[serde(skip_serializing_if = "Option::is_none")]
    ground: Option<GroundEntry>,
}

#[derive(Serialize)]
struct GroundEntry {
    /// `mmorpg_presentation::ground::Layer::name`; its material is bundle entry
    /// `greyhaven.terrain.<layer>`.
    layer: &'static str,
    /// A masked overlay (weights in the vertex alpha), else the opaque base.
    overlay: bool,
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
                // Ground layers (#525): texture coordinates, tangents and per-vertex weights.
                let textured = !g.uv.is_empty();
                if textured {
                    self.vertices
                        .extend(g.uv.iter().flatten().map(|value| *value as f32));
                    self.vertices
                        .extend(g.tangents.iter().flatten().map(|value| *value as f32));
                }
                let weighted = !g.alpha.is_empty();
                self.vertices
                    .extend(g.alpha.iter().map(|value| *value as f32));
                let index_offset = self.indices.len();
                self.indices.extend(&g.indices);
                self.keys.push(draw.resource_key.clone());
                self.geometries.push(GeometryEntry {
                    resource_key: draw.resource_key.clone(),
                    vertex_offset,
                    vertex_count: g.p.len(),
                    colored,
                    textured,
                    weighted,
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
            ground: material.ground.map(|ground| GroundEntry {
                layer: ground.layer.name(),
                overlay: ground.overlay,
            }),
        }
    }
}

pub(crate) type SceneBuffers = (String, Vec<f32>, Vec<u32>, Vec<f32>);

fn buffers() -> &'static SceneBuffers {
    static BUFFERS: PerZone<SceneBuffers> = PerZone::new();
    BUFFERS.get(|_| encode(hosted_static_scene()))
}

pub(crate) fn encode(scene: &StaticScene) -> SceneBuffers {
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
            fallback: group.fallback.map(|kit| match kit {
                mmorpg_presentation::Kit::Outpost => "outpost",
                mmorpg_presentation::Kit::SettlementProps => "settlementProps",
                mmorpg_presentation::Kit::Nature => "nature",
                // Graybox fallbacks are animated parts per body, never batched groups.
                mmorpg_presentation::Kit::Graybox(_) => "fortress",
            }),
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

/// The scenery's point lights (`mmorpg_presentation::lights`, #600) at a cosmetic clock, as the
/// native client lights them: per light its position (metres), sRGB colour (0–1), lumens at
/// this frame's flicker and range (metres), eight floats each.
#[must_use]
pub fn point_lights(seconds: f64, animate: bool) -> Vec<f32> {
    static LIGHTS: PerZone<Vec<mmorpg_presentation::lights::PointLight>> = PerZone::new();
    let lights = LIGHTS.get(|_| {
        mmorpg_presentation::lights::point_lights(hosted_static_scene(), hosted_scenery())
    });
    lights
        .iter()
        .flat_map(|light| {
            let [x, y, z] = light.position;
            let channel = |shift: u32| ((light.color >> shift) & 255) as f32 / 255.0;
            [
                x as f32,
                y as f32,
                z as f32,
                channel(16),
                channel(8),
                channel(0),
                light.lumens_at(seconds, animate) as f32,
                light.range as f32,
            ]
        })
        .collect()
}

/// The terrain's visual tiles (#526, `mmorpg_presentation::tiles`) for one page renderer: the
/// renderer keeps every tile's far level and the near levels this stream says are resident.
#[wasm_bindgen]
pub struct TerrainStream {
    arena: bool,
    streamer: Streamer<()>,
}

#[wasm_bindgen]
impl TerrainStream {
    /// A stream over the hosted zone's terrain, or the Battle Royale arena's (#552).
    #[must_use]
    pub(crate) fn new(arena: bool) -> Self {
        Self {
            arena,
            streamer: Streamer::default(),
        }
    }

    fn scene(&self) -> &'static StaticScene {
        if self.arena {
            crate::arena_presentation::scene()
        } else {
            hosted_static_scene()
        }
    }

    /// How many tiles the terrain has.
    #[must_use]
    pub fn count(&self) -> usize {
        self.scene().terrain.tiles.len()
    }

    /// Tile `index`'s far level, packed by [`pack`].
    #[must_use]
    pub fn far(&self, index: usize) -> Vec<f32> {
        pack(&self.scene().terrain.tiles[index].far)
    }

    /// Tile `index`'s near level, built now, packed by [`pack`].
    #[must_use]
    pub fn near(&self, index: usize) -> Vec<f32> {
        pack(&self.scene().terrain.near(index))
    }

    /// Advances residency for the camera eye (metres): the tiles whose near level the renderer
    /// builds now (it calls [`Self::near`] for each), those it drops, then one level per tile
    /// (1 near, 0 far): `[built count, built…, evicted count, evicted…, levels…]`.
    #[must_use]
    pub fn update(&mut self, x: f64, y: f64, z: f64) -> Vec<u32> {
        let tiles = &self.scene().terrain;
        let mut built = Vec::new();
        let evicted = self
            .streamer
            .update(tiles, [x, y, z], BUILDS_PER_FRAME, |index| {
                built.push(index)
            });
        let mut out = Vec::with_capacity(2 + built.len() + evicted.len() + tiles.tiles.len());
        out.push(built.len() as u32);
        out.extend(built.iter().map(|&index| index as u32));
        out.push(evicted.len() as u32);
        out.extend(evicted.iter().map(|&(index, ())| index as u32));
        out.extend(
            (0..tiles.tiles.len())
                .map(|index| u32::from(self.streamer.level(index) == Level::Near)),
        );
        out
    }
}

/// Ground draws as one float array: the draw count, then per draw its layer
/// (`mmorpg_presentation::ground::Layer` order), overlay and far flags (0 or 1), vertex and index
/// counts, then positions and normals (three floats per vertex), sRGB colours (three), texture
/// coordinates (two, zero when untextured), tangents (four, likewise), the vertex colour alpha
/// (one, 1 for the opaque base) and the indices.
fn pack(draws: &[Draw]) -> Vec<f32> {
    let mut out = vec![draws.len() as f32];
    for draw in draws {
        let ground = draw
            .material
            .ground
            .expect("terrain tile draws are ground layers");
        let g = &draw.geometry;
        let n = g.p.len();
        out.extend([
            f32::from(ground.layer as u8),
            f32::from(u8::from(ground.overlay)),
            f32::from(u8::from(ground.far)),
            n as f32,
            g.indices.len() as f32,
        ]);
        out.extend(g.p.iter().flatten().map(|&v| v as f32));
        out.extend(g.n.iter().flatten().map(|&v| v as f32));
        out.extend(g.colors.iter().flatten().map(|&v| v as f32));
        if g.uv.len() == n {
            out.extend(g.uv.iter().flatten().map(|&v| v as f32));
            out.extend(g.tangents.iter().flatten().map(|&v| v as f32));
        } else {
            out.extend(std::iter::repeat_n(0., n * 6));
        }
        if g.alpha.len() == n {
            out.extend(g.alpha.iter().map(|&v| v as f32));
        } else {
            out.extend(std::iter::repeat_n(1., n));
        }
        out.extend(g.indices.iter().map(|&i| i as f32));
    }
    out
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

    /// The node and vertex budgets the web client checks too (`local-zone-wasm.test.ts`),
    /// here so every Rust change meets them on PR CI (#520).
    #[test]
    fn the_static_scene_stays_within_its_node_and_vertex_budget() {
        let layout: serde_json::Value = serde_json::from_str(layout()).unwrap();
        let nodes = layout["groups"].as_array().unwrap().len();
        let vertices: u64 = layout["geometries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["vertexCount"].as_u64().unwrap())
            .sum();
        assert!(nodes <= 3_800, "{nodes} static nodes");
        // The terrain is drawn in visual tiles (#526), outside the static scene.
        assert!(vertices <= 400_000, "{vertices} static vertices");
    }
}
