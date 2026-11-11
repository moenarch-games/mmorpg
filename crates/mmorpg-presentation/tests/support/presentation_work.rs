//! Deterministic work counts of Greyhaven's client presentation (#645), shared by the
//! `presentation_work` regression test and example. The counts are taken at the boundaries both
//! clients drive: the static scenery's residency streams (`tiles::Streamer` over the terrain
//! tiles and the static groups, as `mmorpg-wasm`'s `TerrainStream`/`GroupStream` run them for
//! the browser and the native client runs them for its terrain) and the shared
//! `units::UnitPresenter`. Nothing is timed, no GPU is involved and nothing is hidden: every
//! scenario draws the complete scene.
#![allow(dead_code)] // The test and the example each use part of it.

use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::TAU;
use std::sync::Arc;

use mmorpg_core::{
    CreatureFamily, EntityFlags, EntityKind, EntitySnapshot, ZoneId, ZoneSimulation, ZoneSnapshot,
    greyhaven_vale,
};
use mmorpg_presentation::tiles::{BUILDS_PER_FRAME, Level, Streamer};
use mmorpg_presentation::units::{FrameInput, SampledUnit, UnitPresenter, Viewer};
use mmorpg_presentation::{Draw, GROUP_BUILDS_PER_FRAME, Kits, StaticScene};

/// The village hub, where a fresh character's camera starts (metres).
pub const HUB: [f64; 3] = [0., 10., 0.];
/// Frames in one camera orbit lap.
pub const LAP_FRAMES: usize = 64;
/// The most near tiles and static groups one streaming frame may build.
pub const FRAME_BUILD_BUDGET: usize = BUILDS_PER_FRAME + GROUP_BUILDS_PER_FRAME;
/// Bytes one uploaded vertex takes in the browser's packed ground layout (`mmorpg-wasm`
/// `static_scene::pack`): position, normal, colour, UV, tangent and alpha floats.
const VERTEX_BYTES: usize = (3 + 3 + 3 + 2 + 4 + 1) * 4;
const INDEX_BYTES: usize = 4;
/// Walking pace of the region walk: 6 m/s at 60 frames a second.
const WALK_METRES_PER_FRAME: f64 = 0.1;

/// What static residency did over one or more frames.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StaticWork {
    pub frames: usize,
    /// Static groups whose draws were built (lowered and uploaded).
    pub group_builds: usize,
    /// Terrain tiles whose near level was built.
    pub tile_builds: usize,
    pub evictions: usize,
    pub uploaded_vertices: usize,
    pub uploaded_bytes: usize,
    /// The most groups and tiles one frame built.
    pub largest_frame_builds: usize,
    /// The most static draws one frame drew (shown groups, near tiles and far cover).
    pub largest_frame_draws: usize,
    /// Near tiles resident after the last frame (visible/loaded tiles V).
    pub resident_tiles: usize,
}

impl StaticWork {
    #[must_use]
    pub fn builds(&self) -> usize {
        self.group_builds + self.tile_builds
    }

    fn upload(&mut self, draws: &[Draw]) {
        for draw in draws {
            let g = &draw.geometry;
            self.uploaded_vertices += g.p.len();
            self.uploaded_bytes += g.p.len() * VERTEX_BYTES + g.indices.len() * INDEX_BYTES;
        }
    }

    fn add(&mut self, frame: Self) {
        self.frames += frame.frames;
        self.group_builds += frame.group_builds;
        self.tile_builds += frame.tile_builds;
        self.evictions += frame.evictions;
        self.uploaded_vertices += frame.uploaded_vertices;
        self.uploaded_bytes += frame.uploaded_bytes;
        self.largest_frame_builds = self.largest_frame_builds.max(frame.builds());
        self.largest_frame_draws = self.largest_frame_draws.max(frame.largest_frame_draws);
        self.resident_tiles = frame.resident_tiles;
    }
}

/// The static scene's residency for one client, every kit drawn.
pub struct Statics<'a> {
    scene: &'a StaticScene,
    /// Per resident near tile, its draw count.
    tiles: Streamer<usize>,
    groups: Streamer<()>,
}

impl<'a> Statics<'a> {
    #[must_use]
    pub fn new(scene: &'a StaticScene) -> Self {
        Self {
            scene,
            tiles: Streamer::default(),
            groups: Streamer::default(),
        }
    }

    /// One frame for the camera `eye` (metres): stream, build what became resident and count
    /// what the frame draws.
    pub fn frame(&mut self, eye: [f64; 3]) -> StaticWork {
        let scene = self.scene;
        let terrain = &scene.terrain;
        let kits = Kits::ALL;
        let eye32 = eye.map(|v| v as f32);
        let mut work = StaticWork {
            frames: 1,
            ..StaticWork::default()
        };
        let mut built_tiles = Vec::new();
        work.evictions += self
            .tiles
            .update(terrain, eye, BUILDS_PER_FRAME, |index| {
                let near = terrain.near(index);
                let count = near.len();
                built_tiles.push(near);
                count
            })
            .len();
        for near in &built_tiles {
            work.tile_builds += 1;
            work.upload(near);
        }
        let mut built_groups = Vec::new();
        work.evictions += self
            .groups
            .stream(
                scene.groups.iter().map(|group| group.site(eye32, kits)),
                GROUP_BUILDS_PER_FRAME,
                |index| built_groups.push(index),
            )
            .len();
        for &index in &built_groups {
            work.group_builds += 1;
            work.upload(&scene.groups[index].draws);
        }
        let shows_near = |index: usize| self.tiles.level(index) == Level::Near;
        let mut draws: usize = (0..terrain.tiles.len())
            .filter(|&index| shows_near(index))
            .filter_map(|index| self.tiles.resident(index))
            .sum();
        draws += terrain
            .far_cover(&shows_near)
            .into_iter()
            .map(|(level, index)| match terrain.chunks.get(level) {
                Some(chunks) => chunks[index].far.len(),
                None => terrain.tiles[index].far.len(),
            })
            .sum::<usize>();
        draws += scene
            .groups
            .iter()
            .enumerate()
            .filter(|&(index, group)| {
                self.groups.resident(index).is_some() && group.shown(eye32, kits)
            })
            .map(|(_, group)| group.draws.len())
            .sum::<usize>();
        work.largest_frame_builds = work.builds();
        work.largest_frame_draws = draws;
        work.resident_tiles = self.tiles.resident_count();
        work
    }

    /// Frames at `eye` until one builds and evicts nothing; panics past `limit` frames.
    pub fn settle(&mut self, eye: [f64; 3], limit: usize) -> StaticWork {
        let mut total = StaticWork::default();
        for _ in 0..limit {
            let frame = self.frame(eye);
            total.add(frame);
            if frame.builds() == 0 && frame.evictions == 0 {
                return total;
            }
        }
        panic!("static residency never settles at {eye:?}");
    }
}

/// The camera eye `frame` frames into an orbit of `radius` metres around the hub.
fn orbit_eye(radius: f64, frame: usize) -> [f64; 3] {
    let angle = TAU * frame as f64 / LAP_FRAMES as f64;
    [
        HUB[0] + radius * angle.cos(),
        HUB[1],
        HUB[2] + radius * angle.sin(),
    ]
}

/// Camera-only frames: the viewer stands at the hub while the camera orbits it at `radius`
/// metres (0: a stationary camera). Residency settles and one warm-up lap runs first; the
/// returned work counts the `frames` frames after it.
#[must_use]
pub fn camera_only(scene: &StaticScene, radius: f64, frames: usize) -> StaticWork {
    let mut statics = Statics::new(scene);
    statics.settle(orbit_eye(radius, 0), 1_000);
    for frame in 0..LAP_FRAMES {
        statics.frame(orbit_eye(radius, frame));
    }
    let mut work = StaticWork::default();
    for frame in 0..frames {
        work.add(statics.frame(orbit_eye(radius, LAP_FRAMES + frame)));
    }
    work
}

/// Region enter/leave: from settled residency at the hub, the camera walks `metres` east at
/// a running pace, entering and leaving tiles and group reaches; counts the walk.
#[must_use]
pub fn region_walk(scene: &StaticScene, metres: usize) -> StaticWork {
    let mut statics = Statics::new(scene);
    statics.settle(HUB, 1_000);
    let frames = (metres as f64 / WALK_METRES_PER_FRAME).round() as usize;
    let mut work = StaticWork::default();
    for frame in 1..=frames {
        let x = HUB[0] + frame as f64 * WALK_METRES_PER_FRAME;
        work.add(statics.frame([x, HUB[1], HUB[2]]));
    }
    work
}

/// What unit presentation did over the measured frames.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UnitWork {
    pub frames: usize,
    /// Presented units in the last frame (animated characters A, the viewer included).
    pub units: usize,
    /// Procedural draws in the last frame.
    pub unit_draws: usize,
    /// Distinct geometries the measured frames drew.
    pub geometries: usize,
    /// Geometries first drawn during the measured frames (a client uploads each once).
    pub geometry_uploads: usize,
    pub uploaded_vertices: usize,
}

/// `characters` wolves around the viewer at the hub, `moving` of them walking east (changed
/// presentation entities Δ); one warm-up frame, then `frames` measured frames.
#[must_use]
pub fn crowd(characters: usize, moving: usize, frames: usize) -> UnitWork {
    let content = greyhaven_vale::content();
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), Arc::clone(&content))
        .expect("the vale admits a zone");
    zone.add_player(1).expect("the viewer joins");
    let mut snapshot: ZoneSnapshot = zone.snapshot_for_player(1).expect("the viewer projects");
    let spawn = content
        .creature_spawns()
        .iter()
        .find(|spawn| {
            content
                .creature_template(spawn.template)
                .is_some_and(|template| template.family == CreatureFamily::Wolf)
        })
        .expect("the vale has wolves");
    let viewer = snapshot.entities[0].clone();
    let columns = 8;
    for index in 0..characters {
        let (column, row) = ((index % columns) as i32, (index / columns) as i32);
        snapshot.entities.push(EntitySnapshot {
            kind: EntityKind::Creature,
            id: spawn.id.get() + u32::try_from(index).expect("a small crowd") * 10_000,
            appearance: spawn.template.get(),
            position: [
                viewer.position[0] + 300 + column * 250,
                viewer.position[1],
                viewer.position[2] + (row - 4) * 250,
            ],
            velocity: [0; 3],
            facing: 0,
            level: 2,
            health_percent: 100,
            flags: EntityFlags::default(),
        });
    }
    let scenery = Arc::new(mmorpg_scenery::greyhaven_vale_scenery());
    let mut presenter = UnitPresenter::new(1, content, scenery);
    let mut uploaded: BTreeMap<String, usize> = BTreeMap::new();
    let mut work = UnitWork::default();
    let mut drawn = BTreeSet::new();
    for frame in 0..=frames {
        let seconds = frame as f64 / 60.;
        for record in snapshot.entities.iter_mut().skip(1).take(moving) {
            record.position[0] += 10;
            record.velocity = [10, 0, 0];
        }
        snapshot.tick += 1;
        let units: Vec<_> = snapshot
            .entities
            .iter()
            .map(|record| SampledUnit {
                record: record.clone(),
                centre: presenter.rendered(record.position),
                yaw: 0.,
            })
            .collect();
        let out = presenter.frame(&FrameInput {
            snapshot: &snapshot,
            units: &units,
            viewer: Viewer::default(),
            seconds,
            dt: 1. / 60.,
            reduced_motion: false,
            bar_yaw: 0.,
        });
        for draw in &out.draws {
            let fresh = !uploaded.contains_key(&draw.resource_key);
            if fresh {
                uploaded.insert(draw.resource_key.clone(), draw.geometry.p.len());
            }
            if frame > 0 {
                drawn.insert(draw.resource_key.clone());
                if fresh {
                    work.geometry_uploads += 1;
                    work.uploaded_vertices += draw.geometry.p.len();
                }
            }
        }
        if frame > 0 {
            work.frames += 1;
            work.units = out.units.len();
            work.unit_draws = out.draws.len();
        }
    }
    work.geometries = drawn.len();
    work
}
