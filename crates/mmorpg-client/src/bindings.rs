//! Native composition of one action catalog and character-scoped portable profiles.
//! input-bindings owns profile application, validation, conflicts and serialization.
use input_bindings_core::{
    ActionDefinition, ActionRegistry, ConfigurationDiagnosticSeverity, PortableConfigurationV1,
    Profile, portable_configuration_from_profile, profile_from_portable_configuration,
    resolve_portable_configuration, validate_registry,
};
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
};

pub const REGISTRY_VERSION: u32 = 1;

pub fn action_registry() -> ActionRegistry {
    let mut actions: BTreeMap<String, ActionDefinition> = BTreeMap::new();
    for registry in [
        crate::controls::native_action_registry(),
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
    for action in actions.values_mut() {
        action.category_path = vec![
            match action.id.split('.').next().unwrap_or("") {
                "move" => "Movement",
                "combat" | "ability" | "target" | "player" => "Combat",
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

pub fn load(directory: &Path, character: &str) -> Result<Profile, String> {
    let path = profile_path(directory, character)?;
    let json = match std::fs::read_to_string(path) {
        Ok(json) => json,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(default_profile(character));
        }
        Err(error) => return Err(error.to_string()),
    };
    let configuration: PortableConfigurationV1 =
        serde_json::from_str(&json).map_err(|error| error.to_string())?;
    let report = resolve_portable_configuration(
        &configuration,
        &action_registry(),
        REGISTRY_VERSION,
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
        portable_configuration_from_profile(profile, &defaults, REGISTRY_VERSION, None);
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
        profile.patches.push(BindingPatch::Remove {
            binding_id: "move.jump.pad0".into(),
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
}
