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
    presentation: settings_presentation::PresentationEntry,
}
#[derive(Deserialize)]
struct Category {
    key: String,
    order: i32,
}
#[derive(Deserialize)]
struct Catalog {
    schema_version: u32,
    entries: Vec<Entry>,
    categories: Vec<Category>,
    text: BTreeMap<String, String>,
}
#[derive(Clone)]
struct Scope {
    state: SettingsState,
    preserved: PreservedEntries,
}

/// A display-only change computed from foundation-owned values and overrides.
#[derive(Clone, Debug)]
pub struct UserPreferenceChange {
    pub id: String,
    pub before: SettingValue,
    pub after: SettingValue,
    pub operation: &'static str,
    pub apply_mode: settings_core::ApplyMode,
}

/// An isolated, validated replacement. Confirmation rechecks its baseline and
/// persists only User before installing it; a failed save leaves it retryable.
pub struct UserImport {
    pub changes: Vec<UserPreferenceChange>,
    pub diagnostics: Vec<String>,
    baseline: String,
    candidate: Scope,
}

/// One native preference session. The shared foundation validates definitions,
/// typed edits and imported snapshots, and preserves inactive portable entries.
pub struct Preferences {
    registry: SettingsRegistry,
    presentation: settings_presentation::PresentationRegistry,
    categories: Vec<String>,
    text: BTreeMap<String, String>,
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
        let mut presentation_entries = Vec::new();
        for entry in catalog.entries {
            if !ids.insert(entry.definition.id.clone()) {
                return Err(format!("Duplicate setting {}", entry.definition.id));
            }
            if matches!(entry.target, Target::Shared | Target::Native) {
                registry.register(entry.definition).map_err(message)?;
                presentation_entries.push(entry.presentation);
            }
        }
        let presentation = settings_presentation::PresentationRegistry::from_entries(
            &registry,
            presentation_entries,
        )
        .map_err(message)?;
        let mut categories = catalog.categories;
        let keys = categories
            .iter()
            .map(|category| category.key.as_str())
            .collect::<BTreeSet<_>>();
        if keys.len() != categories.len()
            || presentation
                .ordered_entries()
                .iter()
                .any(|entry| !keys.contains(entry.metadata.category_key.as_str()))
        {
            return Err("Invalid Settings category manifest".into());
        }
        categories.sort_by(|left, right| {
            left.order
                .cmp(&right.order)
                .then_with(|| left.key.cmp(&right.key))
        });
        let categories = categories
            .into_iter()
            .filter(|category| {
                presentation
                    .ordered_entries()
                    .iter()
                    .any(|entry| entry.metadata.category_key.as_str() == category.key)
            })
            .map(|category| category.key)
            .collect();
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
        Ok(Self {
            registry,
            scopes,
            presentation,
            categories,
            text: catalog.text,
        })
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
        for (scope, _) in durable_scopes() {
            self.save_scope(directory, scope)?;
        }
        Ok(())
    }

    pub fn save_scope(&self, directory: &Path, scope: SettingScope) -> Result<(), String> {
        let name = durable_scopes()
            .into_iter()
            .find(|(known, _)| *known == scope)
            .map(|(_, name)| name)
            .ok_or("Unsupported durable scope")?;
        atomic_write(&directory.join(name), self.export(scope)?.as_bytes())
    }

    pub fn export_user_file(&self, path: &Path) -> Result<(), String> {
        atomic_write(path, self.export(SettingScope::User)?.as_bytes())
    }

    pub fn preview_user_import(&self, json: &str) -> Result<UserImport, String> {
        let report =
            import_scope_json(&self.registry, SettingScope::User, json).map_err(message)?;
        let baseline = self.export(SettingScope::User)?;
        let before = &self.scopes[&SettingScope::User].state;
        let after = &report.state;
        let changes = self
            .registry
            .iter()
            .filter(|(_, definition)| definition.scope == SettingScope::User)
            .filter_map(|(id, definition)| {
                let before_override = before.override_value(id);
                let after_override = after.override_value(id);
                if before_override == after_override {
                    return None;
                }
                Some(UserPreferenceChange {
                    id: id.to_string(),
                    before: before.effective_value(&self.registry, id)?.clone(),
                    after: after.effective_value(&self.registry, id)?.clone(),
                    operation: if after_override.is_none() {
                        "Reset to default"
                    } else if before_override.is_none() {
                        "Add override"
                    } else {
                        "Change override"
                    },
                    apply_mode: definition.apply_mode,
                })
            })
            .collect();
        Ok(UserImport {
            changes,
            diagnostics: report
                .diagnostics
                .iter()
                .map(|item| format!("{item:?}"))
                .collect(),
            baseline,
            candidate: Scope {
                state: report.state,
                preserved: report.preserved_entries,
            },
        })
    }

    pub fn confirm_user_import(
        &mut self,
        preview: &UserImport,
        directory: &Path,
    ) -> Result<(), String> {
        if self.export(SettingScope::User)? != preview.baseline {
            return Err("User preferences changed; preview the import again.".into());
        }
        let json = export_scope_json(
            &self.registry,
            &preview.candidate.state,
            SettingScope::User,
            &preview.candidate.preserved,
        )
        .map_err(message)?;
        atomic_write(&directory.join("settings.user.json"), json.as_bytes())?;
        self.scopes
            .insert(SettingScope::User, preview.candidate.clone());
        Ok(())
    }

    pub fn presentation(&self) -> Vec<settings_presentation::PresentationEntry> {
        self.presentation.ordered_entries()
    }
    pub fn categories(&self) -> &[String] {
        &self.categories
    }
    pub fn text<'a>(&'a self, key: &'a str) -> &'a str {
        self.text.get(key).map(String::as_str).unwrap_or(key)
    }

    pub fn overridden(&self, id: &str) -> Result<bool, String> {
        let (id, scope) = self.known(id)?;
        Ok(self.scopes[&scope].state.override_value(&id).is_some())
    }
    pub fn reset_many(&mut self, ids: &[String]) -> Result<(), String> {
        let targets = ids
            .iter()
            .map(|id| self.known(id))
            .collect::<Result<Vec<_>, _>>()?;
        for (id, scope) in targets {
            self.scopes
                .get_mut(&scope)
                .expect("known scope")
                .state
                .reset(&self.registry, &id)
                .map_err(message)?;
        }
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
fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent).map_err(message)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(message)?;
    temporary.write_all(contents).map_err(message)?;
    temporary.as_file().sync_all().map_err(message)?;
    temporary.persist(path).map_err(message)?;
    #[cfg(unix)]
    std::fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(message)?;
    Ok(())
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
    fn category_reset_uses_shared_metadata_and_preserves_other_categories_and_dormant_entries() {
        let mut preferences = Preferences::new().unwrap();
        preferences
            .import(
                SettingScope::User,
                include_str!("../../../fixtures/settings/user-profile-v2.json"),
            )
            .unwrap();
        assert_eq!(
            preferences.categories(),
            &[
                "settings.category.camera",
                "settings.category.controls",
                "settings.category.accessibility",
                "settings.category.audio"
            ]
        );
        let controls = preferences
            .presentation()
            .into_iter()
            .filter(|entry| entry.metadata.category_key.as_str() == "settings.category.controls")
            .map(|entry| entry.id.to_string())
            .collect::<Vec<_>>();
        preferences.reset_many(&controls).unwrap();
        assert_eq!(
            preferences.value(LOOK_SENSITIVITY).unwrap(),
            &SettingValue::Number(1.0)
        );
        assert_eq!(
            preferences.value(INVERT_VERTICAL_LOOK).unwrap(),
            &SettingValue::Bool(false)
        );
        assert_eq!(
            preferences.value(FIELD_OF_VIEW).unwrap(),
            &SettingValue::Integer(70)
        );
        assert_eq!(
            preferences.value(REDUCE_MOTION).unwrap(),
            &SettingValue::Choice("reduce".into())
        );
        assert!(
            preferences
                .export(SettingScope::User)
                .unwrap()
                .contains("other_platform.preference")
        );
    }

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

#[cfg(test)]
mod import_tests {
    use super::*;
    #[test]
    fn the_shared_portable_profile_preserves_dormant_entries_through_resets() {
        let directory = tempfile::tempdir().unwrap();
        let mut preferences = Preferences::new().unwrap();
        preferences.import(SettingScope::User, r#"{"schema_version":2,"scope":"user","overrides":{"old.platform.preference":{"type":"text","value":"drop"}}}"#).unwrap();
        let json = include_str!("../../../fixtures/settings/user-profile-v2.json");
        let preview = preferences.preview_user_import(json).unwrap();
        assert!(
            !preview.diagnostics.is_empty(),
            "unknown platform entry remains visible in preview diagnostics"
        );
        preferences
            .confirm_user_import(&preview, directory.path())
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(
                &preferences.export(SettingScope::User).unwrap()
            )
            .unwrap(),
            serde_json::from_str::<serde_json::Value>(json).unwrap()
        );
        for id in [
            FIELD_OF_VIEW,
            LOOK_SENSITIVITY,
            INVERT_VERTICAL_LOOK,
            REDUCE_MOTION,
        ] {
            preferences.reset(id).unwrap();
        }
        let exported = preferences.export(SettingScope::User).unwrap();
        assert!(exported.contains("other_platform.preference"));
        assert!(!exported.contains("old.platform.preference"));
        assert!(!exported.contains(FIELD_OF_VIEW));
    }

    #[test]
    fn user_import_is_isolated_replaces_user_deltas_and_leaves_device_bytes_alone() {
        let directory = tempfile::tempdir().unwrap();
        let mut destination = Preferences::new().unwrap();
        destination
            .set(FIELD_OF_VIEW, SettingValue::Integer(65))
            .unwrap();
        destination.save(directory.path()).unwrap();
        let device_before = std::fs::read(directory.path().join("settings.device.json")).unwrap();
        let mut source = Preferences::new().unwrap();
        source
            .set(LOOK_SENSITIVITY, SettingValue::Number(2.0))
            .unwrap();
        let preview = destination
            .preview_user_import(&source.export(SettingScope::User).unwrap())
            .unwrap();
        assert!(
            preview
                .changes
                .iter()
                .any(|change| change.id == FIELD_OF_VIEW && change.operation == "Reset to default")
        );
        assert_eq!(
            destination.value(FIELD_OF_VIEW).unwrap(),
            &SettingValue::Integer(65)
        );
        assert_eq!(
            Preferences::load(directory.path())
                .unwrap()
                .0
                .value(FIELD_OF_VIEW)
                .unwrap(),
            &SettingValue::Integer(65)
        );
        destination
            .confirm_user_import(&preview, directory.path())
            .unwrap();
        assert_eq!(
            destination.value(FIELD_OF_VIEW).unwrap(),
            &SettingValue::Integer(50)
        );
        assert_eq!(
            destination.value(LOOK_SENSITIVITY).unwrap(),
            &SettingValue::Number(2.0)
        );
        assert_eq!(
            std::fs::read(directory.path().join("settings.device.json")).unwrap(),
            device_before
        );
        assert_eq!(
            Preferences::load(directory.path())
                .unwrap()
                .0
                .value(LOOK_SENSITIVITY)
                .unwrap(),
            &SettingValue::Number(2.0)
        );
    }

    #[test]
    fn failed_save_and_stale_preview_leave_live_user_preferences_unchanged() {
        let mut destination = Preferences::new().unwrap();
        let mut source = Preferences::new().unwrap();
        source
            .set(FIELD_OF_VIEW, SettingValue::Integer(70))
            .unwrap();
        let preview = destination
            .preview_user_import(&source.export(SettingScope::User).unwrap())
            .unwrap();
        let directory = tempfile::tempdir().unwrap();
        let blocker = directory.path().join("not-a-directory");
        std::fs::write(&blocker, "block").unwrap();
        assert!(destination.confirm_user_import(&preview, &blocker).is_err());
        assert_eq!(
            destination.value(FIELD_OF_VIEW).unwrap(),
            &SettingValue::Integer(50)
        );
        destination
            .confirm_user_import(&preview, directory.path())
            .unwrap();
        assert_eq!(
            destination.value(FIELD_OF_VIEW).unwrap(),
            &SettingValue::Integer(70)
        );
        destination
            .set(FIELD_OF_VIEW, SettingValue::Integer(60))
            .unwrap();
        assert!(
            destination
                .confirm_user_import(&preview, directory.path())
                .is_err()
        );
        assert_eq!(
            destination.value(FIELD_OF_VIEW).unwrap(),
            &SettingValue::Integer(60)
        );
        assert!(destination.preview_user_import("broken").is_err());
        assert!(
            destination
                .preview_user_import(&source.export(SettingScope::Device).unwrap())
                .is_err()
        );
    }
}
