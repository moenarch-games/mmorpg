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
    audio::{AudioCues, AudioOp},
    units::{
        FrameInput, SampledUnit, UnitPresenter, Viewer,
        imported::{Kind, Loadout, Resident, Throwable},
        player::Hat,
    },
};
use serde::Serialize;

use crate::scenery::{hosted_scenery, presented_content};

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
    /// The drawn body's heading: the unit's yaw turned by its swim attitude (Atlas 6.6.41).
    yaw: f64,
    /// The swim attitude's forward lean in radians about the turned model's X axis
    /// the yaw (Euler order `YXZ`); `feet` already holds the chest pivot.
    pitch: f64,
    height: f64,
    /// The upper body's target clip and time: what the unit is seen doing.
    clip: &'static str,
    time: f32,
    /// The legs' target clip and time.
    lower: (&'static str, f32),
    /// The layered clips to pose (Atlas 6.6.45).
    layers: Vec<LayerEntry>,
    tint: Option<[f32; 3]>,
    /// The carried weapons by `Loadout::name`, or null.
    loadout: Option<&'static str>,
    /// A resident's or humanoid enemy's `humanoid-appearance` preset id (#418, #39), or null.
    resident: Option<&'static str>,
    /// The Ranger figure a resident is drawn on, by its character key (`imported::Figure`, #493),
    /// or null.
    figure: Option<&'static str>,
    /// A varied resident's look seed, its NPC or creature id (asset-tooling #380), or null.
    variety: Option<u32>,
    /// A player's class starter outfit (asset-tooling #379), or null.
    outfit: Option<OutfitEntry>,
    headwear: bool,
    /// The loose gear's swing (Atlas 6.6.39) as a model-frame quaternion `[x, y, z, w]`, or
    /// null at rest.
    swing: Option<[f32; 4]>,
    /// A bow carrier's shot (#556): the bow cycle clip and time whose draw the nock follows,
    /// with the bow in the left hand; null keeps the bow stowed.
    bow: Option<BowEntry>,
    /// A dagger holder's sheathed daggers, left then right (`hip_l.stowed`, `hip_r.stowed`),
    /// and the throwable in `hand_r.weapon`, `knife`, `flask`, `bomb` or null (#603).
    sheathed: [bool; 2],
    throwable: Option<&'static str>,
}

#[derive(Serialize)]
struct BowEntry {
    clip: &'static str,
    time: f32,
}

/// A class starter outfit: its `humanoid-appearance` preset id and the armour sockets whose
/// outfit pieces equipped drops replace.
#[derive(Serialize)]
struct OutfitEntry {
    preset: &'static str,
    replaced: Vec<&'static str>,
}

#[derive(Serialize)]
struct LayerEntry {
    clip: &'static str,
    time: f32,
    weight: f32,
    /// `all`, `upper`, `lower`, `fingers_l` or `fingers_r`.
    mask: &'static str,
}

#[derive(Serialize)]
struct FrameLayout {
    draws: Vec<DrawEntry>,
    geometries: Vec<GeometryEntry>,
    units: Vec<UnitEntry>,
    imported: Vec<ImportedEntry>,
    platforms: Vec<PlatformEntry>,
}

/// A platform the page's kit draws (#591): where its box's bottom centre stands, metres.
#[derive(Serialize)]
struct PlatformEntry {
    platform: u8,
    base: [f32; 3],
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
    /// Sound cues of the projections pushed since the page last took them (#447).
    audio: AudioCues,
    audio_ops: Vec<AudioOp>,
}

/// One sound operation for the page's `game-audio` handle.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
enum AudioEntry<'a> {
    Play {
        key: &'a str,
        kind: &'a str,
        entity: u64,
        position: [f32; 3],
    },
    Stop {
        entity: u64,
    },
}

fn kind_name(kind: EntityKind) -> &'static str {
    match kind {
        EntityKind::Player => "player",
        EntityKind::Creature => "creature",
        EntityKind::Npc => "npc",
        EntityKind::Minion => "minion",
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
            presented_content(),
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
            audio: AudioCues::default(),
            audio_ops: Vec::new(),
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

    /// An imported model's `humanoid-clips` descriptor JSON (#500).
    ///
    /// # Errors
    ///
    /// For an unknown kind or a malformed descriptor.
    pub fn model_motions(&mut self, kind: &str, descriptor: &str) -> Result<(), String> {
        let kind = Kind::from_name(kind).ok_or_else(|| format!("unknown model {kind}"))?;
        let motions = mmorpg_presentation::units::traversal::Motions::parse(descriptor)?;
        self.presenter.model_motions(kind, motions);
        Ok(())
    }

    /// The shared presenter, for the local wind-up (Atlas 6.6.10).
    pub fn presenter_mut(&mut self) -> &mut UnitPresenter {
        &mut self.presenter
    }

    /// The residents' appearance presets loaded (#418).
    pub fn residents_ready(&mut self) {
        self.presenter.residents_ready();
    }

    /// Tunes the imported models' layered blend (#384).
    pub fn set_blend(&mut self, blend: mmorpg_presentation::units::imported::Blend) {
        self.presenter.set_blend(blend);
    }

    /// Shows or hides the development body overlay (#39).
    pub fn set_body_overlay(&mut self, shown: bool) {
        self.presenter.set_body_overlay(shown);
    }

    /// The platforms the page's kit draws instead of their procedural pillar (#591).
    pub fn set_kit_platforms(&mut self, platforms: &[u8]) {
        self.presenter.set_kit_platforms(
            platforms
                .iter()
                .map(|&id| mmorpg_core::PlatformId::new(id))
                .collect(),
        );
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
        let presenter = &self.presenter;
        self.audio_ops.extend(
            self.audio
                .push(&snapshot, |unit| presenter.rendered(unit.position)),
        );
        self.latest = Some(snapshot);
        Ok(())
    }

    pub fn reset(&mut self) {
        self.presenter.reset();
        self.audio_ops.extend(self.audio.reset());
        self.latest = None;
        self.sent.clear();
    }

    /// Takes the sound operations since the last call as a JSON array of
    /// `{"play": {key, kind, entity, position}}` and `{"stop": {entity}}`.
    pub fn take_audio(&mut self) -> String {
        let entries: Vec<AudioEntry<'_>> = self
            .audio_ops
            .iter()
            .map(|op| match op {
                AudioOp::Play(cue) => AudioEntry::Play {
                    key: &cue.key,
                    kind: cue.kind,
                    entity: cue.entity,
                    position: cue.position,
                },
                AudioOp::Stop(entity) => AudioEntry::Stop { entity: *entity },
            })
            .collect();
        let json = serde_json::to_string(&entries).unwrap_or_else(|_| "[]".to_owned());
        self.audio_ops.clear();
        json
    }

    /// One frame. `units` holds `[kind (0 player, 1 creature, 2 npc), id, x, y, z, facing]`
    /// per sampled unit in units and yaw steps; `choice` is the viewer's appearance code
    /// (0 for none), `headwear` a headwear style or empty, `preview_yaw` NaN without preview.
    ///
    /// # Errors
    ///
    /// Before the first projection, or for a malformed unit list or headwear style.
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
            "class" => Some(Hat::Class),
            other => return Err(format!("unknown headwear style {other}")),
        };
        let mut sampled = Vec::with_capacity(units.len() / 6);
        for unit in units.as_chunks::<6>().0 {
            let id = unit[1] as u32;
            let entity = match unit[0] as u8 {
                0 => EntityRef::Player(id),
                1 => EntityRef::Creature(CreatureId::new(id)),
                2 => EntityRef::Npc(NpcId::new(id)),
                3 => EntityRef::Minion(mmorpg_core::MinionId::new(id)),
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
                    yaw: yaw + unit.animation.attitude.heading,
                    pitch: unit.animation.attitude.pitch,
                    height: unit.height,
                    clip: unit.animation.upper.0,
                    time: unit.animation.upper.1,
                    lower: unit.animation.lower,
                    layers: unit
                        .animation
                        .layers
                        .iter()
                        .map(|layer| LayerEntry {
                            clip: layer.clip,
                            time: layer.time,
                            weight: layer.weight,
                            mask: layer.mask.name(),
                        })
                        .collect(),
                    tint: unit.tint,
                    loadout: unit.loadout.map(Loadout::name),
                    resident: unit.resident.map(Resident::preset),
                    figure: unit.resident.map(|resident| resident.figure().character()),
                    variety: unit.variety,
                    outfit: unit.outfit.map(|outfit| OutfitEntry {
                        preset: outfit.preset(),
                        replaced: outfit.replaced(),
                    }),
                    headwear: unit.headwear.is_some(),
                    swing: (!unit.swing.is_rest()).then(|| unit.swing.quaternion()),
                    bow: unit.bow.map(|shot| BowEntry {
                        clip: shot.clip,
                        time: shot.time,
                    }),
                    sheathed: unit.animation.carry.sheathed,
                    throwable: unit.animation.carry.throwable.map(Throwable::name),
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
            platforms: frame
                .platforms
                .iter()
                .map(|view| PlatformEntry {
                    platform: view.platform.get(),
                    base: view.base,
                })
                .collect(),
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
