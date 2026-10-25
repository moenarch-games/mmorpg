//! Native sound (#447): the shared `game-audio` director on its kira backend.
//!
//! [`mmorpg_presentation::audio`] turns each received projection into cues and fades; this
//! module plays them from the embedded CC0 `combat-sounds` bundle, places the listener at the
//! camera, suspends while the window is unfocused or minimised and applies the volume and mute
//! preferences through the `settings` audio adapter. Without an output device every call is
//! silent; the visual telegraph never depends on sound.
use std::{convert::Infallible, time::Instant};

use game_audio::{
    AudioSettingsCommand, CueEvent, Director, DirectorConfig, EntityId, KiraBackend, SoundManifest,
    settings::{SettingValue as AudioValue, keys},
};
use mmorpg_presentation::audio::AudioOp;
use settings_adapters::{AdapterDescriptor, AudioSettingsAdapter, DomainSettingsAdapter};
use settings_core::{SettingChange, SettingValue};

use crate::{camera::CameraView, settings::Preferences};

mod bundle {
    include!(concat!(env!("OUT_DIR"), "/sound_bundle.rs"));
}

/// The audio preference ids, in the shared catalog (`content/settings/catalog.json`).
pub const SETTINGS: [&str; 4] = [
    keys::MASTER_VOLUME,
    keys::EFFECTS_VOLUME,
    keys::UI_VOLUME,
    keys::MUTED,
];

/// The `settings` audio adapter: maps the catalog's audio preferences to mixer commands. The
/// definitions themselves live in the shared catalog, so its descriptor adds none.
#[derive(Clone, Copy, Debug, Default)]
pub struct AudioPreferences;

impl AudioPreferences {
    #[must_use]
    pub fn command(id: &str, value: &SettingValue) -> Option<AudioSettingsCommand> {
        let value = match *value {
            SettingValue::Bool(value) => AudioValue::Bool(value),
            SettingValue::Integer(value) => AudioValue::Integer(value),
            SettingValue::Number(value) => AudioValue::Number(value),
            SettingValue::Text(_) | SettingValue::Choice(_) => return None,
        };
        AudioSettingsCommand::from_setting(id, value)
    }
}

impl DomainSettingsAdapter for AudioPreferences {
    type Command = AudioSettingsCommand;
    type Error = Infallible;

    fn descriptor(&self) -> AdapterDescriptor {
        AdapterDescriptor::new([])
    }

    fn command_for_change(
        &self,
        change: &SettingChange,
    ) -> Result<Option<Self::Command>, Infallible> {
        Ok(Self::command(change.id.as_str(), &change.after))
    }
}

impl AudioSettingsAdapter for AudioPreferences {}

/// The combat-sounds manifest with mmorpg's cues, as both clients play it.
///
/// # Errors
///
/// The embedded bundle's manifest is malformed.
pub fn manifest() -> Result<SoundManifest, String> {
    let bundle = bundle::FILES
        .iter()
        .find(|(path, _)| *path == bundle::MANIFEST)
        .map(|(_, bytes)| String::from_utf8_lossy(bytes))
        .ok_or("the sound bundle has no manifest")?;
    let json = mmorpg_presentation::audio::sound_manifest(&bundle)?;
    SoundManifest::from_json(&json).map_err(|error| error.to_string())
}

pub struct GameAudio {
    director: Director,
    backend: Option<KiraBackend>,
    origin: Instant,
    focused: bool,
    minimised: bool,
}

impl GameAudio {
    /// Opens the default output device; without one (or a valid manifest) audio stays silent.
    #[must_use]
    pub fn new() -> Option<Self> {
        let manifest = manifest()
            .inspect_err(|error| eprintln!("audio disabled: {error}"))
            .ok()?;
        let backend = KiraBackend::new()
            .inspect_err(|error| eprintln!("audio disabled: {error}"))
            .ok()
            .map(|backend| backend.with_embedded_clips(bundle::FILES.iter().copied()));
        Some(Self {
            director: Director::new(manifest, DirectorConfig::default()),
            backend,
            origin: Instant::now(),
            focused: true,
            minimised: false,
        })
    }

    /// Plays and fades the cues of the projections just presented.
    pub fn apply(&mut self, ops: impl IntoIterator<Item = AudioOp>, now: Instant) {
        let now_ms = now.saturating_duration_since(self.origin).as_secs_f64() * 1000.;
        for op in ops {
            match op {
                AudioOp::Play(cue) => {
                    let event = CueEvent::new(cue.key, cue.kind)
                        .on(EntityId(cue.entity))
                        .at(cue.position);
                    self.director.trigger(&event, now_ms);
                }
                AudioOp::Stop(entity) => self.director.stop_entity(EntityId(entity)),
            }
        }
    }

    /// A new connection: every voice fades; already heard cue keys stay silent.
    pub fn reset(&mut self) {
        self.director.reset();
    }

    /// The listener stands at the camera eye, its right ear along the view's right.
    pub fn listen(&mut self, view: CameraView) {
        let forward = [view.target[0] - view.eye[0], view.target[2] - view.eye[2]];
        let length = forward[0].hypot(forward[1]);
        if length > f32::EPSILON {
            // forward × up on the ground plane.
            let right = [-forward[1] / length, 0., forward[0] / length];
            self.director.set_listener(view.eye, right);
        }
    }

    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
        self.director.set_suspended(!self.focused || self.minimised);
    }

    pub fn set_minimised(&mut self, minimised: bool) {
        self.minimised = minimised;
        self.director.set_suspended(!self.focused || self.minimised);
    }

    /// Applies the current volume and mute preferences (only changed buses reach the mixer).
    pub fn follow(&mut self, preferences: &Preferences) {
        for id in SETTINGS {
            if let Ok(value) = preferences.value(id)
                && let Some(command) = AudioPreferences::command(id, value)
            {
                self.director.apply_settings_command(command);
            }
        }
    }

    /// Once per frame: frees finished voices and sends the queued commands.
    pub fn pump(&mut self) {
        match &mut self.backend {
            Some(backend) => {
                self.director.pump(backend);
                for error in backend.take_errors() {
                    eprintln!("audio: {error}");
                }
            }
            None => self.director.drain().for_each(drop),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use game_audio::{RecordingBackend, TriggerOutcome};

    #[test]
    fn every_mapped_cue_kind_is_in_the_embedded_manifest() {
        let manifest = manifest().unwrap();
        for kind in [
            "combat.hit",
            "combat.hit.defeated",
            "combat.hurt",
            "combat.blocked",
            "combat.guardBroken",
            "combat.whiff",
            "world.refused",
            "combat.defeat",
            mmorpg_presentation::audio::WINDUP,
        ] {
            assert!(
                manifest.cues.iter().any(|cue| cue.kind == kind),
                "{kind} is not in the manifest"
            );
        }
        // Every clip resolves to an embedded file.
        let mut director = Director::new(manifest, DirectorConfig::default());
        let started = director.trigger(
            &CueEvent::new("1/1/0", mmorpg_presentation::audio::WINDUP).at([0., 0., 1.]),
            0.,
        );
        assert!(matches!(started, TriggerOutcome::Started { .. }));
        let mut backend = RecordingBackend::new();
        director.pump(&mut backend);
        for command in backend.take() {
            if let game_audio::Command::Load {
                source: game_audio::ClipSource::Url(url),
                ..
            } = command
            {
                assert!(bundle::FILES.iter().any(|(path, _)| *path == url), "{url}");
            }
        }
    }

    #[test]
    fn audio_preferences_map_to_mixer_commands() {
        assert_eq!(
            AudioPreferences::command(keys::MASTER_VOLUME, &SettingValue::Integer(80)),
            Some(AudioSettingsCommand::SetVolume {
                bus: game_audio::Bus::Master,
                volume: 0.8
            })
        );
        assert_eq!(
            AudioPreferences::command(keys::MUTED, &SettingValue::Bool(true)),
            Some(AudioSettingsCommand::SetMuted(true))
        );
        let preferences = Preferences::new().unwrap();
        for id in SETTINGS {
            assert!(
                AudioPreferences::command(id, preferences.value(id).unwrap()).is_some(),
                "{id} is a catalog audio preference"
            );
        }
    }
}
