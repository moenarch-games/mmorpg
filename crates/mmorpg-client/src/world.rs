//! Native lowering of the shared static world (`mmorpg-presentation`, #184):
//! both clients draw the same procedural terrain, roads, water and props; this
//! module lowers those draws into immutable meshes once (render-engine uploads each
//! resource once) and only selects visible groups and animated transforms per frame.
//! Legacy blockout fields remain available for picking and tests.
pub(crate) mod geometry;
pub use crate::environment::Lighting;
use std::{collections::BTreeMap, sync::Arc};
use three_d_animation::Mat4;

#[derive(Clone, Debug, PartialEq)]
pub struct DrawMaterial {
    pub color: [f32; 3],
    pub emissive: [f32; 3],
    pub opacity: f32,
    pub unlit: bool,
    pub double_sided: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SceneDraw {
    pub id: String,
    pub resource_key: String,
    pub mesh: Arc<three_d_core::Mesh>,
    pub model: Mat4,
    pub material: DrawMaterial,
    /// A GPU-skinned draw's pose (#595): `mesh` stays in its bind pose.
    pub skin: Option<DrawSkin>,
}

/// The pose of a GPU-skinned draw: its bind-pose mesh's per-vertex influences and the figure's
/// joint palette, which the renderer blends per vertex (render-engine's skinned meshes).
#[derive(Clone, Debug, PartialEq)]
pub struct DrawSkin {
    /// The posed figure; its draws share one palette, uploaded once per frame.
    pub figure: String,
    /// Per bind-pose vertex, four palette indices and their weights.
    pub influences: Arc<Influences>,
    /// Per joint, the column-major mesh-local matrix that moves a bind-pose vertex with it:
    /// the joint's model-space transform times its inverse bind matrix.
    pub palette: Arc<[[f32; 16]]>,
}

#[derive(Debug, PartialEq)]
pub struct Influences {
    pub joints: Vec<[u16; 4]>,
    pub weights: Vec<[f32; 4]>,
}

impl DrawSkin {
    /// `mesh` posed on the CPU: for picking a clicked body and for baking static geometry
    /// once, never per rendered frame.
    #[must_use]
    pub fn pose(&self, mesh: &three_d_core::Mesh) -> three_d_core::Mesh {
        use three_d_core::Vec3;
        let blend = |v: Vec3, joints: &[u16; 4], weights: &[f32; 4], w: f32| {
            let mut out = [0.; 3];
            for (&joint, &weight) in joints.iter().zip(weights) {
                if weight <= 0. {
                    continue;
                }
                let m = &self.palette[usize::from(joint).min(self.palette.len() - 1)];
                for (r, out) in out.iter_mut().enumerate() {
                    *out += (m[r] * v.x + m[4 + r] * v.y + m[8 + r] * v.z + m[12 + r] * w) * weight;
                }
            }
            Vec3::new(out[0], out[1], out[2])
        };
        let influences = self.influences.joints.iter().zip(&self.influences.weights);
        let positions = mesh
            .vertices()
            .iter()
            .zip(influences.clone())
            .map(|(p, (j, w))| blend(*p, j, w, 1.))
            .collect();
        let mut attributes = mesh.attributes().clone();
        if let Some(normals) = &mut attributes.normals {
            for (n, (j, w)) in normals.iter_mut().zip(influences) {
                let b = blend(*n, j, w, 0.);
                let len = (b.x * b.x + b.y * b.y + b.z * b.z).sqrt().max(1e-6);
                *n = Vec3::new(b.x / len, b.y / len, b.z / len);
            }
        }
        three_d_core::Mesh::with_attributes(positions, mesh.indices().to_vec(), attributes)
            .expect("posing keeps the mesh's topology")
    }
}

/// Draws that never change while they live: the static scenery's visibility groups and water,
/// placed kit modules. A frame shares them by reference; the renderer resolves each batch into
/// GPU instances once and reuses them until the batch is dropped (#588).
pub type DrawBatch = Arc<[SceneDraw]>;

#[derive(Clone, Debug)]
pub struct WorldFrame {
    /// Unchanging draws shown this frame.
    pub batches: Vec<DrawBatch>,
    /// Draws sampled this frame: units, effects, animated props and door swings.
    pub draws: Vec<SceneDraw>,
    pub lighting: Lighting,
    /// The `greyhaven-nature` trees and rocks drawn this frame (#49).
    pub nature: Vec<NatureDraw>,
}

/// One drawn level of a placed tree or rock (#49): `crate::nature::library` entry `entry`
/// (an index into `mmorpg_presentation::nature::ENTRIES`) at level `level`, placed by
/// `model`; `fade` as `mmorpg_presentation::nature::Shown::fade`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NatureDraw {
    pub entry: usize,
    pub level: usize,
    pub fade: f32,
    pub model: [f32; 16],
}

/// The trees' and rocks' placements and their level-of-detail state (#49).
#[derive(Clone, Debug)]
struct Nature {
    lods: mmorpg_presentation::nature::Lods,
    entries: Vec<usize>,
    models: Vec<[f32; 16]>,
    /// The presentation clock of the last frame.
    seconds: Option<f64>,
}

impl WorldFrame {
    /// Every draw of the frame, batched ones first.
    pub fn all_draws(&self) -> impl Iterator<Item = &SceneDraw> {
        self.batches
            .iter()
            .flat_map(|batch| batch.iter())
            .chain(&self.draws)
    }
}
use crate::presentation::SceneBox;
use mmorpg_core::UNITS_PER_METRE;
use mmorpg_presentation::StaticScene;
use mmorpg_scenery::{Prop, PropKind, RockSize, Scenery, TreeVariant};

/// Terrain vertices are two metres apart over the scenery's ±200 m grid.
pub const TERRAIN_STEP_UNITS: i32 = 200;
const WATER_RIM_VERTICES: u32 = 64;

/// One lit, vertex-coloured mesh vertex in metres; colours are linear RGB.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub color: [f32; 3],
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mesh {
    pub vertices: Vec<MeshVertex>,
    /// Counter-clockwise triangles seen from above.
    pub indices: Vec<u32>,
}

/// Everything static the renderer uploads once per zone.
#[derive(Clone, Debug)]
pub struct WorldScene {
    pub terrain: Mesh,
    pub water: Mesh,
    pub props: Vec<SceneBox>,
    /// Additional caller-authored presentation geometry, rendered and picked with the world.
    pub additional_draws: Vec<SceneDraw>,
    scene: StaticScene,
    groups: Vec<DrawBatch>,
    water_draws: DrawBatch,
    animated: Vec<Vec<SceneDraw>>,
    /// The scenery's campfires and lantern posts (#529).
    point_lights: Vec<mmorpg_presentation::lights::PointLight>,
    nature: NatureState,
}

/// [`Nature`] behind a lock, so frames advance it through `&WorldScene`.
#[derive(Debug)]
struct NatureState(std::sync::Mutex<Nature>);

impl NatureState {
    fn lock(&self) -> std::sync::MutexGuard<'_, Nature> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Clone for NatureState {
    fn clone(&self) -> Self {
        Self(std::sync::Mutex::new(self.lock().clone()))
    }
}

/// Lowers shared draws, uploading each distinct resource's mesh once.
#[derive(Default)]
pub(crate) struct Lowering(BTreeMap<String, Arc<three_d_core::Mesh>>);

impl Lowering {
    pub(crate) fn draw(&mut self, draw: &mmorpg_presentation::Draw) -> SceneDraw {
        use geometry::ToMesh;
        let mesh = self
            .0
            .entry(draw.resource_key.clone())
            .or_insert_with(|| Arc::new(draw.geometry.mesh()));
        SceneDraw {
            id: draw.id.clone(),
            resource_key: draw.resource_key.clone(),
            mesh: Arc::clone(mesh),
            model: Mat4 {
                elements: draw.model,
            },
            material: material(&draw.material),
            skin: None,
        }
    }

    /// Drops meshes whose resources no longer appear in `draws`; per-frame
    /// unit and effect geometry would otherwise accumulate.
    pub(crate) fn retain_used(&mut self, draws: &[SceneDraw]) {
        let used: std::collections::BTreeSet<_> = draws
            .iter()
            .map(|draw| draw.resource_key.as_str())
            .collect();
        self.0.retain(|key, _| used.contains(key.as_str()));
    }
}

fn material(material: &mmorpg_presentation::Material) -> DrawMaterial {
    DrawMaterial {
        color: material.color,
        emissive: material.emissive,
        opacity: material.opacity,
        unlit: material.unlit,
        double_sided: material.double_sided,
    }
}

impl WorldScene {
    #[must_use]
    pub fn new(scenery: &Scenery) -> Self {
        let mut props = Vec::new();
        for prop in &scenery.props {
            // Collider-backed props are their exact physics body; decorations stand on the
            // physical ground (their y) raised by the presentation relief.
            let relief = if prop.collider.is_some() {
                0
            } else {
                scenery.height_at(prop.position[0], prop.position[2])
            };
            let base = metres(prop.position[1] + relief);
            blockout(prop, base, &mut props);
        }
        let scene = StaticScene::new(scenery);
        let mut lowering = Lowering::default();
        let groups = scene
            .groups
            .iter()
            .map(|group| group.draws.iter().map(|draw| lowering.draw(draw)).collect())
            .collect();
        let water_draws = scene.water.iter().map(|draw| lowering.draw(draw)).collect();
        let animated = scene
            .animated
            .iter()
            .map(|part| part.draws.iter().map(|draw| lowering.draw(draw)).collect())
            .collect();
        let placements = mmorpg_presentation::nature::placements(scenery);
        let nature = Nature {
            lods: mmorpg_presentation::nature::Lods::new(crate::nature::lods(), &placements),
            entries: placements
                .iter()
                .map(|placement| {
                    mmorpg_presentation::nature::ENTRIES
                        .iter()
                        .position(|entry| *entry == placement.entry)
                        .expect("placements name bundle entries")
                })
                .collect(),
            models: placements.iter().map(|placement| placement.model).collect(),
            seconds: None,
        };
        Self {
            nature: NatureState(std::sync::Mutex::new(nature)),
            additional_draws: Vec::new(),
            point_lights: mmorpg_presentation::lights::point_lights(&scene, scenery),
            groups,
            water_draws,
            animated,
            terrain: terrain_mesh(scenery),
            water: water_mesh(scenery),
            props,
            scene,
        }
    }
    /// Height of the rendered four-metre terrain triangles, in metres.
    #[must_use]
    pub fn surface_y(&self, x: f32, z: f32) -> f32 {
        self.scene.surface_y(f64::from(x), f64::from(z)) as f32
    }

    /// Presented world at a cosmetic clock and camera position, cave roofs cut away over the
    /// `viewer`'s feet (metres), for the default 50° field of view. Geometry is never copied.
    #[must_use]
    pub fn frame(
        &self,
        seconds: f64,
        animate: bool,
        eye: [f32; 3],
        viewer: Option<[f64; 3]>,
    ) -> WorldFrame {
        self.frame_viewed(seconds, animate, eye, 50.0, viewer)
    }

    /// [`Self::frame`] for a camera with a vertical field of view of `fov_y_degrees`, which
    /// picks the trees' and rocks' levels of detail (#49).
    #[must_use]
    pub fn frame_viewed(
        &self,
        seconds: f64,
        animate: bool,
        eye: [f32; 3],
        fov_y_degrees: f32,
        viewer: Option<[f64; 3]>,
    ) -> WorldFrame {
        // The native build embeds every kit (#420, #421, #49): no fallback is drawn.
        let kits = mmorpg_presentation::Kits::ALL;
        let mut batches: Vec<_> = self
            .scene
            .groups
            .iter()
            .zip(&self.groups)
            .filter(|(group, _)| group.shown(eye, kits))
            .map(|(_, draws)| Arc::clone(draws))
            .collect();
        batches.push(Arc::clone(&self.water_draws));
        let mut draws = Vec::new();
        for (part, model) in self
            .animated
            .iter()
            .zip(self.scene.motion(seconds, animate))
            // The native build embeds the Outpost kit (#420) and the settlement props (#421),
            // which the presentation draws, so the static scene never shows their fallbacks.
            .zip(self.scene.shown(viewer, kits))
            .filter_map(|(placed, shown)| shown.then_some(placed))
        {
            for draw in part {
                let mut draw = draw.clone();
                draw.model = Mat4 { elements: model };
                draws.push(draw);
            }
        }
        draws.extend(self.additional_draws.iter().cloned());
        let mut lighting = Lighting::at(crate::environment::pages_hour(seconds, animate));
        lighting.point_lights = self
            .point_lights
            .iter()
            .map(|light| crate::environment::PointLight {
                position: light.position.map(|v| v as f32),
                color: crate::environment::rgb(light.color),
                lumens: light.lumens_at(seconds, animate) as f32,
                range: light.range as f32,
            })
            .collect();
        WorldFrame {
            batches,
            draws,
            lighting,
            nature: self.nature_draws(seconds, animate, eye, fov_y_degrees),
        }
    }

    /// The trees and rocks at each one's level of detail for this camera (#49).
    fn nature_draws(
        &self,
        seconds: f64,
        animate: bool,
        eye: [f32; 3],
        fov_y_degrees: f32,
    ) -> Vec<NatureDraw> {
        let mut nature = self.nature.lock();
        let elapsed = nature
            .seconds
            .map_or(0., |last| (seconds - last).clamp(0., 0.25));
        nature.seconds = Some(seconds);
        let Nature {
            lods,
            entries,
            models,
            ..
        } = &mut *nature;
        lods.update(
            eye.map(f64::from),
            f64::from(fov_y_degrees).to_radians(),
            elapsed,
            1.,
            animate,
            None,
        )
        .iter()
        .map(|shown| NatureDraw {
            entry: entries[shown.placement],
            level: shown.level,
            fade: shown.fade,
            model: models[shown.placement],
        })
        .collect()
    }
}

fn metres(units: i32) -> f32 {
    units as f32 / UNITS_PER_METRE as f32
}

/// sRGB channel to linear light.
fn linear(color: [u8; 3]) -> [f32; 3] {
    color.map(|channel| {
        let value = f32::from(channel) / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    })
}

fn terrain_mesh(scenery: &Scenery) -> Mesh {
    let grid = scenery.terrain_grid(TERRAIN_STEP_UNITS);
    let step = metres(grid.step);
    let height = |column: usize, row: usize| {
        let column = column.min(grid.columns - 1);
        let row = row.min(grid.rows - 1);
        metres(grid.heights[row * grid.columns + column])
    };
    let mut vertices = Vec::with_capacity(grid.columns * grid.rows);
    for row in 0..grid.rows {
        for column in 0..grid.columns {
            let index = row * grid.columns + column;
            // Central differences give a smooth, deterministic normal.
            let dx = height(column + 1, row) - height(column.saturating_sub(1), row);
            let dz = height(column, row + 1) - height(column, row.saturating_sub(1));
            let normal = normalize([-dx, 2.0 * step, -dz]);
            vertices.push(MeshVertex {
                position: [
                    metres(grid.origin[0]) + column as f32 * step,
                    metres(grid.heights[index]),
                    metres(grid.origin[1]) + row as f32 * step,
                ],
                normal,
                color: linear(grid.colors[index]),
            });
        }
    }
    let columns = u32::try_from(grid.columns).unwrap_or(0);
    let rows = u32::try_from(grid.rows).unwrap_or(0);
    let mut indices = Vec::new();
    for row in 0..rows.saturating_sub(1) {
        for column in 0..columns.saturating_sub(1) {
            let near = row * columns + column;
            let far = near + columns;
            indices.extend_from_slice(&[near, far, near + 1, near + 1, far, far + 1]);
        }
    }
    Mesh { vertices, indices }
}

fn water_mesh(scenery: &Scenery) -> Mesh {
    let mut mesh = Mesh::default();
    let color = [0.05, 0.16, 0.26];
    for water in &scenery.water {
        let centre = [
            metres(water.centre[0]),
            metres(water.surface),
            metres(water.centre[1]),
        ];
        let first = u32::try_from(mesh.vertices.len()).unwrap_or(0);
        let up = [0.0, 1.0, 0.0];
        mesh.vertices.push(MeshVertex {
            position: centre,
            normal: up,
            color,
        });
        for step in 0..WATER_RIM_VERTICES {
            let angle = step as f32 / WATER_RIM_VERTICES as f32 * std::f32::consts::TAU;
            mesh.vertices.push(MeshVertex {
                position: [
                    centre[0] + metres(water.radii[0]) * angle.sin(),
                    centre[1],
                    centre[2] + metres(water.radii[1]) * angle.cos(),
                ],
                normal: up,
                color,
            });
        }
        for step in 0..WATER_RIM_VERTICES {
            let next = (step + 1) % WATER_RIM_VERTICES;
            mesh.indices
                .extend_from_slice(&[first, first + 1 + next, first + 1 + step]);
        }
    }
    mesh
}

fn normalize(value: [f32; 3]) -> [f32; 3] {
    let length = value.iter().map(|axis| axis * axis).sum::<f32>().sqrt();
    if length > 0.0 {
        value.map(|axis| axis / length)
    } else {
        [0.0, 1.0, 0.0]
    }
}

const STONE: [f32; 3] = [0.42, 0.41, 0.38];
const DARK_STONE: [f32; 3] = [0.27, 0.26, 0.25];
const WOOD: [f32; 3] = [0.36, 0.23, 0.12];
const DARK_WOOD: [f32; 3] = [0.22, 0.14, 0.08];
const PLASTER: [f32; 3] = [0.72, 0.66, 0.52];
const ROOF: [f32; 3] = [0.42, 0.14, 0.09];
const THATCH: [f32; 3] = [0.55, 0.45, 0.22];
const LEAVES: [f32; 3] = [0.11, 0.3, 0.08];
const NEEDLES: [f32; 3] = [0.05, 0.19, 0.08];
const BIRCH_LEAVES: [f32; 3] = [0.3, 0.46, 0.12];

/// Kind-specific blockout: the first box of every collider-backed prop is
/// its exact collider body, so structures render where physics has them.
fn blockout(prop: &Prop, base: f32, boxes: &mut Vec<SceneBox>) {
    let half = prop.half_extents.map(metres);
    let size = half.map(|value| value * 2.0);
    let [x, _, z] = prop.position.map(metres);
    let yaw = f32::from(prop.yaw) / 65_536.0 * std::f32::consts::TAU;
    let scale = f32::from(prop.scale_permille) / 1_000.0;
    let top = base + size[1];
    let mut part = |offset: [f32; 3], size: [f32; 3], color: [f32; 3]| {
        // Offsets are local to the prop and turn with its yaw.
        let (sin, cos) = (yaw.sin(), yaw.cos());
        boxes.push(SceneBox {
            position: [
                x + offset[0] * cos + offset[2] * sin,
                offset[1],
                z + offset[2] * cos - offset[0] * sin,
            ],
            size,
            color,
            yaw,
        });
    };
    let body = |part: &mut dyn FnMut([f32; 3], [f32; 3], [f32; 3]), color| {
        part([0.0, base + half[1], 0.0], size, color);
    };
    let roof = |part: &mut dyn FnMut([f32; 3], [f32; 3], [f32; 3]), color| {
        part(
            [0.0, top + 0.25, 0.0],
            [size[0] + 0.6, 0.5, size[2] + 0.6],
            color,
        );
        part(
            [0.0, top + 0.9, 0.0],
            [size[0] + 0.2, 0.8, size[2] * 0.45],
            color,
        );
    };
    match prop.kind {
        PropKind::Keep => {
            body(&mut part, STONE);
            part(
                [0.0, top + 0.3, 0.0],
                [size[0] + 0.4, 0.6, size[2] + 0.4],
                DARK_STONE,
            );
            part(
                [half[0] - 1.5, top + 2.5, half[2] - 1.5],
                [3.0, 5.0, 3.0],
                STONE,
            );
        }
        PropKind::Inn | PropKind::House | PropKind::Farmhouse => {
            body(&mut part, PLASTER);
            roof(
                &mut part,
                if prop.kind == PropKind::Inn {
                    ROOF
                } else {
                    THATCH
                },
            );
        }
        PropKind::Smithy => {
            body(&mut part, DARK_STONE);
            roof(&mut part, DARK_WOOD);
            part([half[0] - 0.6, top + 1.5, 0.0], [0.8, 3.0, 0.8], STONE);
        }
        PropKind::Barn => {
            body(&mut part, [0.45, 0.1, 0.06]);
            roof(&mut part, DARK_WOOD);
        }
        PropKind::Windmill => {
            body(&mut part, STONE);
            part(
                [0.0, top + 1.0, 0.0],
                [size[0] * 0.8, 2.0, size[2] * 0.8],
                DARK_WOOD,
            );
            let hub = [0.0, top + 1.0, -half[2] - 0.4];
            part(hub, [0.5, 11.0, 0.2], PLASTER);
            part(hub, [11.0, 0.5, 0.2], PLASTER);
        }
        PropKind::Well => {
            body(&mut part, STONE);
            part([half[0] - 0.1, base + 1.2, 0.0], [0.15, 2.4, 0.15], WOOD);
            part([-half[0] + 0.1, base + 1.2, 0.0], [0.15, 2.4, 0.15], WOOD);
            part(
                [0.0, base + 2.5, 0.0],
                [size[0] + 0.4, 0.2, size[2] + 0.4],
                ROOF,
            );
        }
        PropKind::PalisadeSegment | PropKind::Fence => body(&mut part, WOOD),
        PropKind::GatePost => {
            body(&mut part, DARK_WOOD);
            part(
                [0.0, top + 0.15, 0.0],
                [size[0] + 0.2, 0.3, size[2] + 0.2],
                STONE,
            );
        }
        PropKind::Waystone => {
            body(&mut part, [0.3, 0.34, 0.4]);
            part(
                [0.0, top + 0.2, 0.0],
                [size[0] * 0.8, 0.4, size[2] * 0.8],
                [0.2, 0.9, 0.95],
            );
        }
        PropKind::Gravestone | PropKind::Cliff | PropKind::CaveRoof => body(&mut part, DARK_STONE),
        PropKind::Rock(size_class) => {
            body(
                &mut part,
                match size_class {
                    RockSize::Large => DARK_STONE,
                    RockSize::Medium | RockSize::Small => STONE,
                },
            );
        }
        PropKind::Tree(variant) => {
            // The trunk is the collider (or a scaled trunk); canopies sit above heads.
            let trunk_color = if variant == TreeVariant::Birch {
                [0.75, 0.74, 0.68]
            } else {
                WOOD
            };
            body(&mut part, trunk_color);
            let grow = if prop.collider.is_some() { 1.0 } else { scale };
            match variant {
                TreeVariant::Oak => {
                    part(
                        [0.0, base + 4.2 * grow, 0.0],
                        [4.6 * grow, 3.6 * grow, 4.6 * grow],
                        LEAVES,
                    );
                }
                TreeVariant::Pine => {
                    part(
                        [0.0, base + 3.6 * grow, 0.0],
                        [3.4 * grow, 2.8 * grow, 3.4 * grow],
                        NEEDLES,
                    );
                    part(
                        [0.0, base + 6.0 * grow, 0.0],
                        [2.0 * grow, 2.4 * grow, 2.0 * grow],
                        NEEDLES,
                    );
                }
                TreeVariant::Birch => {
                    part(
                        [0.0, base + 4.4 * grow, 0.0],
                        [3.0 * grow, 3.8 * grow, 3.0 * grow],
                        BIRCH_LEAVES,
                    );
                }
            }
        }
        PropKind::Bush => body(&mut part, [0.1, 0.26, 0.07]),
        PropKind::GrassTuft => body(&mut part, [0.16, 0.34, 0.07]),
        PropKind::Flowers => {
            let palette = [
                [0.9, 0.75, 0.1],
                [0.55, 0.25, 0.75],
                [0.92, 0.92, 0.88],
                [0.8, 0.15, 0.12],
            ];
            let pick = usize::from(prop.yaw % 4);
            body(&mut part, palette[pick]);
        }
        PropKind::Reeds => body(&mut part, [0.36, 0.42, 0.16]),
        PropKind::Tent => {
            body(&mut part, [0.62, 0.55, 0.42]);
            part(
                [0.0, top + 0.2, 0.0],
                [size[0] * 0.3, 0.4, size[2] + 0.2],
                DARK_WOOD,
            );
        }
        PropKind::Campfire => {
            body(&mut part, DARK_STONE);
            part(
                [0.0, top + 0.25, 0.0],
                [size[0] * 0.5, 0.5, size[2] * 0.5],
                [1.0, 0.45, 0.05],
            );
        }
        PropKind::Crate => body(&mut part, [0.5, 0.36, 0.18]),
        PropKind::Barrel => body(&mut part, DARK_WOOD),
        PropKind::Cart => {
            part(
                [0.0, base + 0.6, 0.0],
                [size[0], size[1] * 0.6, size[2]],
                WOOD,
            );
            part(
                [half[0] * 0.5, base + 0.35, half[2]],
                [0.7, 0.7, 0.1],
                DARK_WOOD,
            );
            part(
                [half[0] * 0.5, base + 0.35, -half[2]],
                [0.7, 0.7, 0.1],
                DARK_WOOD,
            );
        }
        PropKind::Signpost => {
            body(&mut part, WOOD);
            part([0.3, top - 0.25, 0.0], [0.8, 0.3, 0.06], PLASTER);
        }
        PropKind::Lamp => {
            body(&mut part, DARK_WOOD);
            part([0.0, top + 0.15, 0.0], [0.3, 0.3, 0.3], [1.0, 0.85, 0.4]);
        }
        PropKind::MineEntrance => {
            // A timber frame with a dark opening on its +Z face, into the hollow.
            body(&mut part, WOOD);
            part(
                [0.0, base + 1.6, half[2] + 0.02],
                [size[0] * 0.6, 3.2, 0.05],
                [0.02, 0.02, 0.02],
            );
        }
        PropKind::CropRow => body(&mut part, [0.62, 0.52, 0.16]),
        PropKind::Dock => part([0.0, base + 0.2, 0.0], size, WOOD),
        PropKind::LoadingDock => body(&mut part, STONE),
        PropKind::Ladder => body(&mut part, WOOD),
        // A ramp's wedge (#59) is no box; the shared static scene draws it. The shrine kit
        // (#431) is drawn from its bundle. Graybox geometry (#552) is the shared static scene's.
        PropKind::Ramp
        | PropKind::ShrineKit
        | PropKind::GrayboxWall
        | PropKind::GrayboxTower
        | PropKind::GrayboxWalkway
        | PropKind::GrayboxRamp => {}
    }
}
