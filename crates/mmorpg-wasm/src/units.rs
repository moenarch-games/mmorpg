//! The shared unit presentation (`mmorpg-presentation::units`, #184) for the browser.
//!
//! The page pushes every projection it receives (the same bytes it decodes) and, per frame,
//! the interpolated units it samples. This adapter runs the presenter the native client runs
//! and returns the frame as JSON plus typed arrays: draws with their model matrices, the
//! geometry of every resource the page did not receive in the previous frame, where each unit
//! stands, and the imported models (with chosen clip and time) the page skins itself.
//! Presentation only: the local zone host never reads it.

use std::{collections::BTreeSet, sync::Arc};

use mmorpg_core::{ClassChoice, CreatureId, EntityKind, EntityRef, NpcId, ZoneSnapshot};
use mmorpg_presentation::{
    Draw,
    units::{FrameInput, SampledUnit, UnitPresenter, Viewer, imported::Kind, player::Hat},
};
use serde::Serialize;

use crate::{host::hosted_content, scenery::hosted_scenery};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DrawEntry {
    id: String,
    key: String,
    model: usize,
    color: String,
    emissive: Option<String>,
    opacity: f32,
    unlit: bool,
    double_sided: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GeometryEntry {
    key: String,
    vertex_offset: usize,
    vertex_count: usize,
    colored: bool,
    index_offset: usize,
    index_count: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UnitEntry {
    kind: &'static str,
    entity_id: u32,
    node: String,
    feet: [f64; 3],
    centre_y: f32,
    yaw: f64,
    label: [f32; 3],
    body: [usize; 2],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ImportedEntry {
    id: String,
    kind: &'static str,
    entity_kind: &'static str,
    entity_id: u32,
    feet: [f64; 3],
    yaw: f64,
    height: f64,
    clip: &'static str,
    time: f32,
    tint: Option<f32>,
    sword_and_shield: bool,
    bow_and_quiver: bool,
    blade: bool,
    headwear: bool,
}

#[derive(Serialize)]
struct FrameLayout {
    draws: Vec<DrawEntry>,
    geometries: Vec<GeometryEntry>,
    units: Vec<UnitEntry>,
    imported: Vec<ImportedEntry>,
}

/// Bulk numbers of the last call, read by the page right after it.
#[derive(Default)]
pub struct Buffers {
    pub models: Vec<f32>,
    pub vertices: Vec<f32>,
    pub indices: Vec<u32>,
}

pub struct Units {
    presenter: UnitPresenter,
    latest: Option<ZoneSnapshot>,
    /// Resources the page holds: those drawn in the previous frame.
    sent: BTreeSet<String>,
    pub buffers: Buffers,
}

fn kind_name(kind: EntityKind) -> &'static str {
    match kind {
        EntityKind::Player => "player",
        EntityKind::Creature => "creature",
        EntityKind::Npc => "npc",
    }
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

impl Units {
    #[must_use]
    pub fn new(player_id: u32) -> Self {
        Self::with_content(
            player_id,
            hosted_content(),
            Arc::new(hosted_scenery().clone()),
        )
    }

    pub(crate) fn with_content(
        player_id: u32,
        content: Arc<mmorpg_core::ZoneContent>,
        scenery: Arc<mmorpg_scenery::Scenery>,
    ) -> Self {
        Self {
            presenter: UnitPresenter::new(player_id, content, scenery),
            latest: None,
            sent: BTreeSet::new(),
            buffers: Buffers::default(),
        }
    }

    /// An imported model finished loading; `clips` is its `[[name, seconds], …]` JSON.
    ///
    /// # Errors
    ///
    /// For an unknown kind or malformed clip list.
    pub fn model_ready(&mut self, kind: &str, clips: &str) -> Result<(), String> {
        let kind = Kind::from_name(kind).ok_or_else(|| format!("unknown model {kind}"))?;
        let clips: Vec<(String, f32)> =
            serde_json::from_str(clips).map_err(|error| error.to_string())?;
        self.presenter.model_ready(kind, clips);
        Ok(())
    }

    /// Decodes and observes one received projection.
    ///
    /// # Errors
    ///
    /// When the bytes are not a projection.
    pub fn push(&mut self, bytes: &[u8], seconds: f64) -> Result<(), String> {
        let snapshot =
            mmorpg_protocol::decode_snapshot(bytes).map_err(|error| error.to_string())?;
        self.presenter.push(&snapshot, seconds);
        self.latest = Some(snapshot);
        Ok(())
    }

    pub fn reset(&mut self) {
        self.presenter.reset();
        self.latest = None;
        self.sent.clear();
    }

    /// One frame. `units` holds `[kind (0 player, 1 creature, 2 npc), id, x, y, z, facing]`
    /// per sampled unit in units and yaw steps; `choice` is the viewer's appearance code
    /// (0 for none), `headwear` a headwear style or empty, `preview_yaw` NaN without preview.
    ///
    /// # Errors
    ///
    /// Before the first projection, or for a malformed unit list or headwear style.
    #[allow(clippy::too_many_arguments)]
    pub fn frame(
        &mut self,
        units: &[f64],
        seconds: f64,
        dt: f64,
        reduced_motion: bool,
        bar_yaw: f32,
        choice: u16,
        headwear: &str,
        preview_yaw: f32,
    ) -> Result<String, String> {
        let snapshot = self.latest.as_ref().ok_or("no projection yet")?;
        if !units.len().is_multiple_of(6) {
            return Err("units must be six numbers each".into());
        }
        let headwear = match headwear {
            "" => None,
            "ironcrest-helm" => Some(Hat::Helm),
            "ranger-cap" => Some(Hat::Cap),
            "wayfarer-hood" => Some(Hat::Hood),
            other => return Err(format!("unknown headwear style {other}")),
        };
        let mut sampled = Vec::with_capacity(units.len() / 6);
        for unit in units.chunks_exact(6) {
            let id = unit[1] as u32;
            let entity = match unit[0] as u8 {
                0 => EntityRef::Player(id),
                1 => EntityRef::Creature(CreatureId::new(id)),
                2 => EntityRef::Npc(NpcId::new(id)),
                _ => return Err("unknown unit kind".into()),
            };
            let Some(record) = snapshot.entities.iter().find(|e| e.entity() == entity) else {
                continue;
            };
            let position = [unit[2], unit[3], unit[4]];
            let relief =
                hosted_scenery().height_at(position[0].round() as i32, position[2].round() as i32);
            let metres = |value: f64| (value / f64::from(mmorpg_core::UNITS_PER_METRE)) as f32;
            sampled.push(SampledUnit {
                record: record.clone(),
                centre: [
                    metres(position[0]),
                    metres(position[1] + f64::from(relief)),
                    metres(position[2]),
                ],
                yaw: ((unit[5] / 65_536.).rem_euclid(1.) * std::f64::consts::TAU) as f32,
            });
        }
        let frame = self.presenter.frame(&FrameInput {
            snapshot,
            units: &sampled,
            viewer: Viewer {
                choice: ClassChoice::from_appearance(choice),
                headwear,
                preview_yaw: preview_yaw.is_finite().then_some(preview_yaw),
            },
            seconds,
            dt,
            reduced_motion,
            bar_yaw,
        });
        self.buffers = Buffers::default();
        let previous = std::mem::take(&mut self.sent);
        let mut geometries = Vec::new();
        let draws = frame
            .draws
            .iter()
            .map(|draw| self.entry(draw, &previous, &mut geometries))
            .collect();
        let imported = frame
            .imported
            .iter()
            .map(|unit| {
                let yaw = frame
                    .units
                    .iter()
                    .find(|view| view.entity == unit.entity)
                    .map_or(0., |view| view.yaw);
                ImportedEntry {
                    id: unit.id.clone(),
                    kind: unit.kind.name(),
                    entity_kind: kind_name(unit.entity.kind()),
                    entity_id: unit.entity.id(),
                    feet: unit.root.p,
                    yaw,
                    height: unit.height,
                    clip: unit.clip,
                    time: unit.time,
                    tint: unit.tint,
                    sword_and_shield: unit.sword_and_shield,
                    bow_and_quiver: unit.bow_and_quiver,
                    blade: unit.blade,
                    headwear: unit.headwear.is_some(),
                }
            })
            .collect();
        let units = frame
            .units
            .iter()
            .map(|view| UnitEntry {
                kind: kind_name(view.entity.kind()),
                entity_id: view.entity.id(),
                node: view.id.clone(),
                feet: view.feet,
                centre_y: view.centre_y,
                yaw: view.yaw,
                label: view.label,
                body: [view.body.start, view.body.end],
            })
            .collect();
        serde_json::to_string(&FrameLayout {
            draws,
            geometries,
            units,
            imported,
        })
        .map_err(|error| error.to_string())
    }

    fn entry(
        &mut self,
        draw: &Draw,
        previous: &BTreeSet<String>,
        geometries: &mut Vec<GeometryEntry>,
    ) -> DrawEntry {
        if !previous.contains(&draw.resource_key) && !self.sent.contains(&draw.resource_key) {
            let g = &draw.geometry;
            let vertex_offset = self.buffers.vertices.len();
            for values in [&g.p, &g.n, &g.colors] {
                self.buffers
                    .vertices
                    .extend(values.iter().flatten().map(|value| *value as f32));
            }
            let index_offset = self.buffers.indices.len();
            self.buffers.indices.extend(&g.indices);
            geometries.push(GeometryEntry {
                key: draw.resource_key.clone(),
                vertex_offset,
                vertex_count: g.p.len(),
                colored: !g.colors.is_empty(),
                index_offset,
                index_count: g.indices.len(),
            });
        }
        self.sent.insert(draw.resource_key.clone());
        let model = self.buffers.models.len() / 16;
        self.buffers.models.extend(draw.model);
        let material = &draw.material;
        DrawEntry {
            id: draw.id.clone(),
            key: draw.resource_key.clone(),
            model,
            color: hex(material.color),
            emissive: (material.emissive != [0.; 3]).then(|| hex(material.emissive)),
            opacity: material.opacity,
            unlit: material.unlit,
            double_sided: material.double_sided,
        }
    }
}
