//! The shared unit presentation (`mmorpg-presentation::units`, #184) for the browser.
//!
//! The page pushes every projection it receives (by its id in the session's projection store,
//! decoded once, #1009) and, per frame, the interpolated units it samples. This adapter runs the presenter the native client runs
//! and lays the frame out as typed-array records the page reads in place (#1057,
//! [`crate::unit_layout`]): draws with their model matrices, the geometry of every resource the
//! page did not receive in the previous frame, where each unit stands, and the imported models
//! (with chosen clip and time) the page skins itself.
//! Presentation only: the local zone host never reads it.

use std::{collections::HashMap, rc::Rc, sync::Arc};

use mmorpg_core::{ClassChoice, CreatureId, EntityRef, NpcId, ZoneSnapshot};
use mmorpg_presentation::{
    audio::{AudioCues, AudioOp},
    units::{
        FrameInput, SampledUnit, UnitFrame, UnitPresenter, Viewer, imported::Kind, player::Hat,
    },
};
use serde::Serialize;

use crate::{
    scenery::{hosted_scenery, presented_content},
    unit_layout::{LayoutWork, UnitLayout},
};

/// Where the page finds the last layout's buffers in WASM memory: `[pointer, capacity, length]`
/// of `words`, `numbers`, `models`, `vertices` and `indices`, in that order (#1057).
pub type Directory = [usize; 15];

pub struct Units {
    presenter: UnitPresenter,
    latest: Option<Rc<ZoneSnapshot>>,
    /// The latest projection's records by entity, indexed once per push (#1057).
    records: HashMap<EntityRef, usize>,
    /// This frame's sampled units, reused across frames.
    sampled: Vec<SampledUnit>,
    layout: UnitLayout,
    directory: Box<Directory>,
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

/// The unit a page names by its kind code (0 player, 1 creature, 2 npc, 3 minion) and id.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn entity_of(kind: f64, id: f64) -> Result<EntityRef, String> {
    let id = id as u32;
    Ok(match kind as u8 {
        0 => EntityRef::Player(id),
        1 => EntityRef::Creature(CreatureId::new(id)),
        2 => EntityRef::Npc(NpcId::new(id)),
        3 => EntityRef::Minion(mmorpg_core::MinionId::new(id)),
        _ => return Err("unknown unit kind".into()),
    })
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
            records: HashMap::new(),
            sampled: Vec::new(),
            layout: UnitLayout::default(),
            directory: Box::new([0; 15]),
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

    /// What the page's windows are open toward this frame (#619): the loot window, and the NPC
    /// a dialogue, quest or vendor window faces.
    pub fn set_interaction(&mut self, loot_window: bool, npc: Option<u32>) {
        self.presenter
            .set_interaction(mmorpg_presentation::units::Interaction {
                loot_window,
                npc: npc.map(mmorpg_core::NpcId::new),
            });
    }

    /// The crag kit's descriptor once the page draws it, `""` while it does not (#668).
    pub fn set_crag_descriptor(&mut self, json: &str) -> Result<(), String> {
        self.presenter
            .set_crag_descriptor((!json.is_empty()).then_some(json))
    }

    /// Whether the page draws the strike and spell effect bundles (#894).
    pub fn set_effect_kits(&mut self, drawn: bool) {
        self.presenter.set_effect_kits(drawn);
    }

    /// The platforms the page's kit draws instead of their procedural pillar (#591).
    pub fn set_kit_platforms(&mut self, platforms: &[u32]) {
        self.presenter.set_kit_platforms(
            platforms
                .iter()
                .map(|&id| mmorpg_core::PlatformId::new(id))
                .collect(),
        );
    }

    /// Where each caster's spell starts in the world (#612): `[kind, id, x, y, z]` per unit,
    /// metres, its imported `channel` point posed this frame.
    ///
    /// # Errors
    ///
    /// For a malformed list.
    pub fn set_channel_origins(&mut self, origins: &[f64]) -> Result<(), String> {
        if !origins.len().is_multiple_of(5) {
            return Err("channel origins must be five numbers each".into());
        }
        let origins = origins
            .as_chunks::<5>()
            .0
            .iter()
            .map(|o| Ok((entity_of(o[0], o[1])?, [o[2], o[3], o[4]])))
            .collect::<Result<Vec<_>, String>>()?;
        self.presenter.set_channel_origins(origins);
        Ok(())
    }

    /// Observes one received projection, decoded once by the session's projection store
    /// (#1009) and shared with it.
    pub fn push(&mut self, snapshot: Rc<ZoneSnapshot>, seconds: f64) {
        self.presenter.push(&snapshot, seconds);
        let presenter = &self.presenter;
        self.audio_ops.extend(
            self.audio
                .push(&snapshot, |unit| presenter.rendered(unit.position)),
        );
        self.records.clear();
        self.records.extend(
            snapshot
                .entities
                .iter()
                .enumerate()
                .map(|(position, record)| (record.entity(), position)),
        );
        self.latest = Some(snapshot);
    }

    pub fn reset(&mut self) {
        self.presenter.reset();
        self.audio_ops.extend(self.audio.reset());
        self.latest = None;
        self.records.clear();
        self.layout.reset();
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
    /// One frame, laid out for the page (#1057, [`crate::unit_layout`]). `units` holds
    /// `[kind (0 player, 1 creature, 2 npc, 3 minion), id, x, y, z, facing]` per sampled unit in
    /// units and yaw steps; `choice` is the viewer's appearance code (0 for none), `headwear` a
    /// headwear style or empty, `preview_yaw` NaN without preview.
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
        choice: u32,
        headwear: &str,
        preview_yaw: f32,
    ) -> Result<(), String> {
        let viewer = viewer(choice, headwear, preview_yaw)?;
        self.sample(units)?;
        let frame = self.present(viewer, seconds, dt, reduced_motion, bar_yaw)?;
        self.write(&frame);
        Ok(())
    }

    /// Samples the page's units against the latest projection's records, one indexed lookup
    /// each.
    ///
    /// # Errors
    ///
    /// Before the first projection, or for a malformed unit list.
    pub fn sample(&mut self, units: &[f64]) -> Result<(), String> {
        let snapshot = self.latest.as_ref().ok_or("no projection yet")?;
        if !units.len().is_multiple_of(6) {
            return Err("units must be six numbers each".into());
        }
        self.sampled.clear();
        self.layout.count_lookups(units.len() / 6);
        for unit in units.as_chunks::<6>().0 {
            let entity = entity_of(unit[0], unit[1])?;
            let Some(&position) = self.records.get(&entity) else {
                continue;
            };
            let position_units = [unit[2], unit[3], unit[4]];
            let relief = hosted_scenery().height_at(
                position_units[0].round() as i32,
                position_units[2].round() as i32,
            );
            let metres = |value: f64| (value / f64::from(mmorpg_core::UNITS_PER_METRE)) as f32;
            self.sampled.push(SampledUnit {
                record: snapshot.entities[position].clone(),
                centre: [
                    metres(position_units[0]),
                    metres(position_units[1] + f64::from(relief)),
                    metres(position_units[2]),
                ],
                yaw: ((unit[5] / 65_536.).rem_euclid(1.) * std::f64::consts::TAU) as f32,
            });
        }
        Ok(())
    }

    /// The shared presenter's frame of the last [`Self::sample`].
    ///
    /// # Errors
    ///
    /// Before the first projection.
    pub fn present(
        &mut self,
        viewer: Viewer,
        seconds: f64,
        dt: f64,
        reduced_motion: bool,
        bar_yaw: f32,
    ) -> Result<UnitFrame, String> {
        let snapshot = self.latest.as_ref().ok_or("no projection yet")?;
        Ok(self.presenter.frame(&FrameInput {
            snapshot,
            units: &self.sampled,
            viewer,
            seconds,
            dt,
            reduced_motion,
            bar_yaw,
        }))
    }

    /// Lays `frame` out for the page and points the directory at its buffers.
    pub fn write(&mut self, frame: &UnitFrame) {
        self.layout.write(frame);
        let layout = &self.layout;
        let parts = [
            (
                layout.words.as_ptr() as usize,
                layout.words.capacity(),
                layout.words.len(),
            ),
            (
                layout.numbers.as_ptr() as usize,
                layout.numbers.capacity(),
                layout.numbers.len(),
            ),
            (
                layout.models.as_ptr() as usize,
                layout.models.capacity(),
                layout.models.len(),
            ),
            (
                layout.vertices.as_ptr() as usize,
                layout.vertices.capacity(),
                layout.vertices.len(),
            ),
            (
                layout.indices.as_ptr() as usize,
                layout.indices.capacity(),
                layout.indices.len(),
            ),
        ];
        for (slot, (pointer, capacity, length)) in parts.into_iter().enumerate() {
            self.directory[slot * 3..slot * 3 + 3].copy_from_slice(&[pointer, capacity, length]);
        }
    }

    /// The last layout's buffers, for views over WASM memory.
    #[must_use]
    pub fn directory(&self) -> &Directory {
        &self.directory
    }

    /// The last layout, read in place by native tests.
    #[must_use]
    pub fn layout(&self) -> &UnitLayout {
        &self.layout
    }

    /// The layout's deterministic work so far.
    #[must_use]
    pub fn layout_work(&self) -> LayoutWork {
        self.layout.work()
    }
}

/// The viewer's look from the page's appearance code, headwear style and preview yaw.
///
/// # Errors
///
/// For an unknown headwear style.
pub fn viewer(choice: u32, headwear: &str, preview_yaw: f32) -> Result<Viewer, String> {
    let headwear = match headwear {
        "" => None,
        "ironcrest-helm" => Some(Hat::Helm),
        "ranger-cap" => Some(Hat::Cap),
        "wayfarer-hood" => Some(Hat::Hood),
        "class" => Some(Hat::Class),
        other => return Err(format!("unknown headwear style {other}")),
    };
    Ok(Viewer {
        choice: ClassChoice::from_appearance(choice),
        headwear,
        preview_yaw: preview_yaw.is_finite().then_some(preview_yaw),
    })
}
