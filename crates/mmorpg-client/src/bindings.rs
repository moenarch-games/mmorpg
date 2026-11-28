//! Native composition of one action catalog and character-scoped portable profiles.
//! input-bindings owns profile application, validation, conflicts and serialization.
//!
//! The actions both clients bind are `content/settings/input-actions.json` (#1284), in
//! `input-bindings`' own registry format, which the browser's `web/src/input/game-controls.ts`
//! imports too: ids, titles, contexts, default keys, pad buttons, stick thresholds and the pad
//! guard rule. The native client adds only its world, interaction and pad-navigation actions.
use input_bindings_core::{
    ActionDefinition, ActionRegistry, ConfigurationDiagnosticSeverity, DeviceStroke, InputStroke,
    PortableBindingPatch, PortableConfigurationV1, Profile, portable_configuration_from_profile,
    profile_from_portable_configuration, resolve_portable_configuration, validate_registry,
};
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
    sync::OnceLock,
};

const SHARED_CATALOG: &str = include_str!("../../../content/settings/input-actions.json");

/// The pad guard rule both clients read (#156, owner answer #156.Q1 A): a pad holding
/// `trigger_button` past `trigger_threshold` % aims the guard with `stick_axis`, high or low past
/// `deflection` %.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GuardLaneRule {
    pub trigger_button: u16,
    pub trigger_threshold: u8,
    pub stick_axis: u8,
    pub deflection: u8,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SharedCatalog {
    registry_version: u32,
    guard_lane: GuardLaneRule,
    actions: Vec<ActionDefinition>,
}

fn shared_catalog() -> &'static SharedCatalog {
    static CATALOG: OnceLock<SharedCatalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(SHARED_CATALOG).expect("content/settings/input-actions.json parses")
    })
}

/// The catalog version stored with each portable binding profile, shared with the browser.
pub fn registry_version() -> u32 {
    shared_catalog().registry_version
}

/// The actions both clients bind, as the browser's `SHARED_ACTIONS`.
#[must_use]
pub fn shared_action_registry() -> ActionRegistry {
    ActionRegistry {
        actions: shared_catalog().actions.clone(),
    }
}

#[must_use]
pub fn guard_lane_rule() -> GuardLaneRule {
    shared_catalog().guard_lane
}

pub fn action_registry() -> ActionRegistry {
    let mut actions: BTreeMap<String, ActionDefinition> = BTreeMap::new();
    for registry in [
        shared_action_registry(),
        crate::world_controls::world_action_registry(),
        crate::gamepad::pad_action_registry(),
    ] {
        for action in registry.actions {
            if let Some(existing) = actions.get_mut(&action.id) {
                existing.defaults.extend(action.defaults);
                for device in action.allowed_devices {
                    if !existing.allowed_devices.contains(&device) {
                        existing.allowed_devices.push(device);
                    }
                }
            } else {
                actions.insert(action.id.clone(), action);
            }
        }
    }
    // The shared catalog names its categories; the native-only actions take theirs by id prefix.
    for action in actions
        .values_mut()
        .filter(|action| action.category_path.is_empty())
    {
        action.category_path = vec![
            match action.id.split('.').next().unwrap_or("") {
                "move" => "Movement",
                "combat" | "ability" | "target" | "player" | "minions" => "Combat",
                "world" | "interaction" => "World",
                _ => "Interface",
            }
            .into(),
        ];
    }
    ActionRegistry {
        actions: actions.into_values().collect(),
    }
}

pub fn validate_profile(profile: &Profile) -> Result<(), String> {
    let report = validate_registry(&action_registry(), Some(profile));
    if !report.valid || !report.diagnostics.is_empty() {
        return Err(format!("Invalid binding profile: {:?}", report.diagnostics));
    }
    Ok(())
}

pub fn default_profile(character: &str) -> Profile {
    Profile {
        id: format!("character:{character}"),
        patches: Vec::new(),
    }
}

fn profile_path(directory: &Path, character: &str) -> Result<PathBuf, String> {
    if character.is_empty() || character.len() > 128 {
        return Err("Invalid character identity for bindings".into());
    }
    // Encode the identity rather than interpreting a character ID as a filesystem path.
    let encoded = character
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(directory.join("bindings").join(format!("{encoded}.json")))
}

/// Native profiles saved before the shared catalog (#1284) name a shared action's pad defaults
/// `{action}.pad{n}`; the catalog numbers them after the action's keys, as the browser always did.
/// The native-only pad actions keep their `.pad{n}` ids, so only a shared action's are renamed.
fn rename_legacy_pad_bindings(configuration: &mut PortableConfigurationV1) {
    let renamed = |id: &mut String| {
        let Some((action, index)) = id.rsplit_once(".pad") else {
            return;
        };
        let Ok(index) = index.parse::<usize>() else {
            return;
        };
        let current = shared_catalog()
            .actions
            .iter()
            .find(|shared| shared.id == action)
            .and_then(|shared| {
                shared
                    .defaults
                    .iter()
                    .filter(|binding| {
                        binding.sequence.iter().any(|stroke| {
                            matches!(
                                stroke,
                                InputStroke::Device(
                                    DeviceStroke::GamepadButton { .. }
                                        | DeviceStroke::GamepadAxis { .. }
                                )
                            )
                        })
                    })
                    .nth(index)
            });
        if let Some(binding) = current {
            id.clone_from(&binding.id);
        }
    };
    // An added binding never had a default's id; renaming one could only collide.
    for patch in &mut configuration.patches {
        match patch {
            PortableBindingPatch::Add { .. } => {}
            PortableBindingPatch::Remove { binding_id, .. } => renamed(binding_id),
            PortableBindingPatch::Replace {
                binding_id,
                binding,
                ..
            } => {
                renamed(binding_id);
                renamed(&mut binding.id);
            }
        }
    }
}

pub fn load(directory: &Path, character: &str) -> Result<Profile, String> {
    let path = profile_path(directory, character)?;
    let json = match std::fs::read_to_string(path) {
        Ok(json) => json,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(default_profile(character));
        }
        Err(error) => return Err(error.to_string()),
    };
    let mut configuration: PortableConfigurationV1 =
        serde_json::from_str(&json).map_err(|error| error.to_string())?;
    rename_legacy_pad_bindings(&mut configuration);
    let report = resolve_portable_configuration(
        &configuration,
        &action_registry(),
        registry_version(),
        &[],
        &[],
    );
    if !report.valid {
        return Err(format!(
            "Invalid saved binding profile: {:?}",
            report.diagnostics
        ));
    }
    let configuration = report
        .configuration
        .ok_or("Missing binding configuration")?;
    let profile = profile_from_portable_configuration(&configuration);
    if profile.id != default_profile(character).id {
        return Err("Saved binding profile belongs to a different character".into());
    }
    validate_profile(&profile)?;
    Ok(profile)
}

pub fn save(directory: &Path, character: &str, profile: &Profile) -> Result<(), String> {
    if profile.id != default_profile(character).id {
        return Err("Binding profile belongs to a different character".into());
    }
    validate_profile(profile)?;
    let registry = action_registry();
    let defaults = registry
        .actions
        .into_iter()
        .flat_map(|action| action.defaults)
        .collect::<Vec<_>>();
    let (configuration, diagnostics) =
        portable_configuration_from_profile(profile, &defaults, registry_version(), None);
    if diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == ConfigurationDiagnosticSeverity::Error)
    {
        return Err(format!("Cannot save binding profile: {diagnostics:?}"));
    }
    let json = serde_json::to_vec(&configuration).map_err(|error| error.to_string())?;
    let path = profile_path(directory, character)?;
    let parent = path.parent().ok_or("Missing binding directory")?;
    let write = || -> Result<(), Box<dyn std::error::Error>> {
        std::fs::create_dir_all(parent)?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        temporary.write_all(&json)?;
        temporary.as_file().sync_all()?;
        temporary.persist(&path)?;
        #[cfg(unix)]
        std::fs::File::open(parent)?.sync_all()?;
        Ok(())
    };
    write().map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use input_bindings_core::{BindingPatch, InputStroke, KeyMatch};
    #[test]
    fn weapon_mode_retains_h_and_contextual_combat_keys() {
        use crate::controls::{ControlAction, NativeControls};
        use winit::keyboard::KeyCode;
        let mut keyboard = NativeControls::new();
        assert!(keyboard.key_down(KeyCode::KeyQ, false).is_empty());
        keyboard.key_up(KeyCode::KeyQ);
        assert_eq!(
            keyboard.key_down(KeyCode::KeyH, false),
            vec![ControlAction::ToggleWeapon]
        );
        keyboard.key_up(KeyCode::KeyH);
        keyboard.set_weapon_drawn(true);
        assert_eq!(
            keyboard.key_down(KeyCode::KeyQ, false),
            vec![ControlAction::Swing]
        );
        keyboard.key_up(KeyCode::KeyQ);
        keyboard.set_weapon_drawn(false);
        assert!(keyboard.key_down(KeyCode::KeyQ, false).is_empty());
    }
    #[test]
    fn one_character_profile_contains_all_devices_and_reload_does_not_change_another() {
        let registry = action_registry();
        let report = validate_registry(&registry, None);
        assert!(report.valid && report.diagnostics.is_empty(), "{report:?}");
        let jump = registry
            .actions
            .iter()
            .find(|action| action.id == "move.jump")
            .unwrap();
        assert!(jump.allowed_devices.len() > 1);
        let mut replacement = jump
            .defaults
            .iter()
            .find(|binding| binding.id == "move.jump.0")
            .unwrap()
            .clone();
        let InputStroke::Keyboard(stroke) = &mut replacement.sequence[0] else {
            panic!("keyboard binding")
        };
        stroke.key = KeyMatch::Physical {
            value: "KeyJ".into(),
        };
        let mut profile = default_profile("a");
        profile.patches.push(BindingPatch::Replace {
            binding_id: replacement.id.clone(),
            binding: replacement,
        });
        // The pad A default, as the browser numbers it (#1284).
        profile.patches.push(BindingPatch::Remove {
            binding_id: "move.jump.1".into(),
        });
        let mut keyboard = crate::controls::NativeControls::new();
        keyboard.set_profile(Some(profile.clone())).unwrap();
        assert_eq!(
            keyboard.key_down(winit::keyboard::KeyCode::KeyJ, false),
            vec![crate::controls::ControlAction::Jump]
        );
        let dir = tempfile::tempdir().unwrap();
        save(dir.path(), "a", &profile).unwrap();
        assert_eq!(load(dir.path(), "a").unwrap(), profile);
        assert_eq!(load(dir.path(), "b").unwrap(), default_profile("b"));
        assert!(save(dir.path(), "b", &profile).is_err());
    }

    /// Both clients see one catalog (#1284): the native registry is the shared file's actions,
    /// unchanged, beside native-only actions that reuse none of their ids.
    #[test]
    fn the_native_catalog_is_the_shared_catalog_plus_native_only_actions() {
        use crate::controls::ControlAction;
        let shared = shared_action_registry();
        let native = action_registry();
        for action in &shared.actions {
            assert_eq!(
                native.actions.iter().find(|known| known.id == action.id),
                Some(action),
                "{} is the shared definition",
                action.id
            );
        }
        let native_only = [
            crate::world_controls::world_action_registry(),
            crate::gamepad::pad_action_registry(),
        ]
        .into_iter()
        .flat_map(|registry| registry.actions);
        for action in native_only {
            assert!(
                !shared.actions.iter().any(|known| known.id == action.id),
                "{} is native-only",
                action.id
            );
        }
        let mut controls = ControlAction::all()
            .map(ControlAction::id)
            .collect::<Vec<_>>();
        let mut ids = shared
            .actions
            .iter()
            .map(|action| action.id.as_str())
            .collect::<Vec<_>>();
        controls.sort_unstable();
        ids.sort_unstable();
        assert_eq!(controls, ids, "every shared action is a ControlAction");
    }

    #[test]
    fn a_profile_saved_before_the_shared_catalog_keeps_its_pad_bindings() {
        use input_bindings_core::{
            Binding, PORTABLE_CONFIGURATION_SCHEMA_VERSION, PortableBindingPatch,
        };
        let registry = action_registry();
        let default = |id: &str| -> Binding {
            registry
                .actions
                .iter()
                .flat_map(|action| &action.defaults)
                .find(|binding| binding.id == id)
                .unwrap()
                .clone()
        };
        let mut select = default("ui.toggleCharacter.1");
        select.id = "ui.toggleCharacter.pad0".into();
        let legacy = PortableConfigurationV1 {
            schema_version: PORTABLE_CONFIGURATION_SCHEMA_VERSION,
            registry_version: 1,
            profile_id: "character:a".into(),
            preset_id: None,
            patches: vec![
                PortableBindingPatch::Remove {
                    action_id: "move.jump".into(),
                    binding_id: "move.jump.pad0".into(),
                },
                PortableBindingPatch::Replace {
                    action_id: "ui.toggleCharacter".into(),
                    binding_id: "ui.toggleCharacter.pad0".into(),
                    binding: select,
                },
                // A native-only pad action keeps its id.
                PortableBindingPatch::Remove {
                    action_id: "ui.closePanel".into(),
                    binding_id: "ui.closePanel.pad0".into(),
                },
            ],
        };
        let dir = tempfile::tempdir().unwrap();
        let path = profile_path(dir.path(), "a").unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
        let profile = load(dir.path(), "a").unwrap();
        // In the foundation's canonical patch order.
        assert_eq!(
            profile.patches,
            [
                BindingPatch::Remove {
                    binding_id: "move.jump.1".into()
                },
                BindingPatch::Remove {
                    binding_id: "ui.closePanel.pad0".into()
                },
                BindingPatch::Replace {
                    binding_id: "ui.toggleCharacter.1".into(),
                    binding: default("ui.toggleCharacter.1"),
                },
            ]
        );
    }
}
