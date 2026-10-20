//! Native composition of the shared consumer catalog and settings-core.
//! Values are client preferences only; bindings retain input-bindings ownership.
use serde::Deserialize;
use settings_core::{
    PreservedEntries, SettingDefinition, SettingId, SettingScope, SettingValue, SettingsRegistry,
    SettingsState, export_scope_json, import_scope_json,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    path::Path,
};

pub const FIELD_OF_VIEW: &str = "camera.field_of_view";
pub const LOOK_SENSITIVITY: &str = "controls.look_sensitivity";
pub const INVERT_VERTICAL_LOOK: &str = "controls.invert_vertical_look";
pub const REDUCE_MOTION: &str = "accessibility.reduce_motion";
const CATALOG: &str = include_str!("../../../content/settings/catalog.json");

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Target {
    Shared,
    Browser,
    Native,
}
#[derive(Deserialize)]
struct Entry {
    target: Target,
    definition: SettingDefinition,
}
#[derive(Deserialize)]
struct Catalog {
    schema_version: u32,
    entries: Vec<Entry>,
}
struct Scope {
    state: SettingsState,
    preserved: PreservedEntries,
}

/// One native preference session. The shared foundation validates definitions,
/// typed edits and imported snapshots, and preserves inactive portable entries.
pub struct Preferences {
    registry: SettingsRegistry,
    scopes: BTreeMap<SettingScope, Scope>,
}
impl Preferences {
    pub fn new() -> Result<Self, String> {
        let catalog: Catalog = serde_json::from_str(CATALOG).map_err(message)?;
        if catalog.schema_version != 1 {
            return Err("Unsupported MMORPG settings catalog".into());
        }
        let mut registry = SettingsRegistry::new();
        let mut ids = BTreeSet::new();
        for entry in catalog.entries {
            if !ids.insert(entry.definition.id.clone()) {
                return Err(format!("Duplicate setting {}", entry.definition.id));
            }
            if matches!(entry.target, Target::Shared | Target::Native) {
                registry.register(entry.definition).map_err(message)?;
            }
        }
        let scopes = [SettingScope::User, SettingScope::Device]
            .into_iter()
            .map(|scope| {
                Ok((
                    scope,
                    Scope {
                        state: SettingsState::new(),
                        preserved: PreservedEntries::new(scope).map_err(message)?,
                    },
                ))
            })
            .collect::<Result<_, String>>()?;
        Ok(Self { registry, scopes })
    }

    /// Load each durable scope independently. Unreadable data keeps safe defaults
    /// and returns a visible diagnostic; loading never rewrites the source file.
    pub fn load(directory: &Path) -> Result<(Self, Vec<String>), String> {
        let mut preferences = Self::new()?;
        let mut problems = Vec::new();
        for (scope, name) in durable_scopes() {
            let path = directory.join(name);
            match std::fs::read_to_string(&path) {
                Ok(json) => match preferences.import(scope, &json) {
                    Ok(diagnostics) => problems.extend(diagnostics),
                    Err(error) => problems.push(format!("{}: {error}", path.display())),
                },
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => problems.push(format!("{}: {error}", path.display())),
            }
        }
        Ok((preferences, problems))
    }

    /// Persist delta-only User and Device snapshots to separate atomic targets.
    /// A failed replacement leaves the previous complete snapshot recoverable.
    pub fn save(&self, directory: &Path) -> Result<(), String> {
        std::fs::create_dir_all(directory).map_err(message)?;
        for (scope, name) in durable_scopes() {
            let json = self.export(scope)?;
            let mut temporary = tempfile::NamedTempFile::new_in(directory).map_err(message)?;
            temporary.write_all(json.as_bytes()).map_err(message)?;
            temporary.as_file().sync_all().map_err(message)?;
            temporary.persist(directory.join(name)).map_err(message)?;
        }
        #[cfg(unix)]
        std::fs::File::open(directory)
            .and_then(|dir| dir.sync_all())
            .map_err(message)?;
        Ok(())
    }

    pub fn definition(&self, raw_id: &str) -> Result<&SettingDefinition, String> {
        let (id, _) = self.known(raw_id)?;
        self.registry
            .get(&id)
            .ok_or_else(|| format!("Unknown preference {raw_id}"))
    }

    pub fn value(&self, raw_id: &str) -> Result<&SettingValue, String> {
        let (id, scope) = self.known(raw_id)?;
        self.scopes[&scope]
            .state
            .effective_value(&self.registry, &id)
            .ok_or_else(|| format!("Missing preference {raw_id}"))
    }

    pub fn set(&mut self, raw_id: &str, value: SettingValue) -> Result<(), String> {
        let (id, scope) = self.known(raw_id)?;
        self.scopes
            .get_mut(&scope)
            .expect("registered scope")
            .state
            .set(&self.registry, &id, value)
            .map_err(message)
    }

    pub fn reset(&mut self, raw_id: &str) -> Result<(), String> {
        let (id, scope) = self.known(raw_id)?;
        self.scopes
            .get_mut(&scope)
            .expect("registered scope")
            .state
            .reset(&self.registry, &id)
            .map_err(message)
    }

    /// Import replaces only the named scope after successful foundation parsing.
    /// Diagnostics remain visible to the caller; invalid values never become active.
    pub fn import(&mut self, scope: SettingScope, json: &str) -> Result<Vec<String>, String> {
        if !self.scopes.contains_key(&scope) {
            return Err("Unsupported native preference scope".into());
        }
        let report = import_scope_json(&self.registry, scope, json).map_err(message)?;
        let diagnostics = report
            .diagnostics
            .iter()
            .map(|item| format!("{item:?}"))
            .collect();
        self.scopes.insert(
            scope,
            Scope {
                state: report.state,
                preserved: report.preserved_entries,
            },
        );
        Ok(diagnostics)
    }

    pub fn export(&self, scope: SettingScope) -> Result<String, String> {
        let data = self
            .scopes
            .get(&scope)
            .ok_or("Unsupported native preference scope")?;
        export_scope_json(&self.registry, &data.state, scope, &data.preserved).map_err(message)
    }

    fn known(&self, raw_id: &str) -> Result<(SettingId, SettingScope), String> {
        let id = SettingId::new(raw_id).map_err(message)?;
        let definition = self
            .registry
            .get(&id)
            .ok_or_else(|| format!("Unknown preference {raw_id}"))?;
        if !self.scopes.contains_key(&definition.scope) {
            return Err("Unsupported native preference scope".into());
        }
        Ok((id, definition.scope))
    }
}
fn durable_scopes() -> [(SettingScope, &'static str); 2] {
    [
        (SettingScope::User, "settings.user.json"),
        (SettingScope::Device, "settings.device.json"),
    ]
}
fn message(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_scope_files_reload_and_corrupt_user_data_keeps_defaults() {
        let directory = tempfile::tempdir().unwrap();
        let mut preferences = Preferences::new().unwrap();
        preferences
            .set(FIELD_OF_VIEW, SettingValue::Integer(70))
            .unwrap();
        preferences.save(directory.path()).unwrap();
        let (restored, problems) = Preferences::load(directory.path()).unwrap();
        assert!(problems.is_empty());
        assert_eq!(
            restored.value(FIELD_OF_VIEW).unwrap(),
            &SettingValue::Integer(70)
        );
        std::fs::write(directory.path().join("settings.user.json"), "broken").unwrap();
        let (defaults, problems) = Preferences::load(directory.path()).unwrap();
        assert_eq!(problems.len(), 1);
        assert_eq!(
            defaults.value(FIELD_OF_VIEW).unwrap(),
            &SettingValue::Integer(50)
        );
        assert_eq!(
            std::fs::read_to_string(directory.path().join("settings.user.json")).unwrap(),
            "broken"
        );
        assert!(defaults.export(SettingScope::Session).is_err());
    }

    #[test]
    fn shared_camera_preference_round_trips_as_a_delta_and_reset_removes_it() {
        let mut preferences = Preferences::new().unwrap();
        assert_eq!(
            preferences.value(FIELD_OF_VIEW).unwrap(),
            &SettingValue::Integer(50)
        );
        preferences
            .set(FIELD_OF_VIEW, SettingValue::Integer(65))
            .unwrap();
        let snapshot = preferences.export(SettingScope::User).unwrap();
        let mut restored = Preferences::new().unwrap();
        assert!(
            restored
                .import(SettingScope::User, &snapshot)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            restored.value(FIELD_OF_VIEW).unwrap(),
            &SettingValue::Integer(65)
        );
        restored.reset(FIELD_OF_VIEW).unwrap();
        assert_eq!(
            restored.value(FIELD_OF_VIEW).unwrap(),
            &SettingValue::Integer(50)
        );
        assert!(
            !restored
                .export(SettingScope::User)
                .unwrap()
                .contains(FIELD_OF_VIEW)
        );
        assert!(
            restored
                .set(FIELD_OF_VIEW, SettingValue::Integer(200))
                .is_err()
        );
    }
}
