//! Native Settings controls over the shared validated preference session.
use crate::{
    roster::config_dir,
    settings::{
        FIELD_OF_VIEW, INVERT_VERTICAL_LOOK, LOOK_SENSITIVITY, Preferences, REDUCE_MOTION,
        UserImport,
    },
};
use settings_core::{SettingKind, SettingValue};
use std::path::PathBuf;

pub struct SettingsWindow {
    pub open: bool,
    pub preferences: Preferences,
    pub bindings: super::bindings::BindingPanel,
    directory: Option<PathBuf>,
    problem: Option<String>,
    profile_path: String,
    user_import: Option<UserImport>,
    transfer_problem: Option<String>,
    category: Option<String>,
    query: String,
    advanced: std::collections::BTreeSet<String>,
    search_target: Option<String>,
    focus_target: Option<String>,
    reset_request: Option<(String, Vec<String>)>,
}
impl Default for SettingsWindow {
    fn default() -> Self {
        let directory = config_dir();
        let loaded = directory.as_deref().map(Preferences::load).transpose();
        let (preferences, problem) = match loaded {
            Ok(Some((preferences, problems))) => (
                preferences,
                (!problems.is_empty()).then(|| problems.join("\n")),
            ),
            Ok(None) => (
                Preferences::new().expect("valid bundled catalog"),
                Some("No configuration directory; preferences cannot be saved.".into()),
            ),
            Err(error) => (
                Preferences::new().expect("valid bundled catalog"),
                Some(error),
            ),
        };
        let profile_path = directory
            .as_ref()
            .map(|directory| directory.join("mmorpg-user-preferences.json"))
            .unwrap_or_else(|| "mmorpg-user-preferences.json".into())
            .display()
            .to_string();
        Self {
            open: false,
            preferences,
            bindings: super::bindings::BindingPanel::default(),
            directory,
            problem,
            profile_path,
            user_import: None,
            transfer_problem: None,
            category: None,
            query: String::new(),
            advanced: std::collections::BTreeSet::new(),
            search_target: None,
            focus_target: None,
            reset_request: None,
        }
    }
}
impl SettingsWindow {
    fn user_preferences_file(&mut self, ui: &mut egui::Ui) -> bool {
        if let Some(preview) = &self.user_import {
            ui.heading("Preview User preferences replacement");
            ui.label("Replace all User overrides, including entries retained for other platforms. Missing entries reset to defaults. Device settings and character bindings stay unchanged.");
            egui::ScrollArea::vertical()
                .max_height(260.0)
                .show(ui, |ui| {
                    for change in &preview.changes {
                        ui.label(format!(
                            "{}: {} — {} → {}",
                            change.operation,
                            preference_label(&change.id),
                            value_label(&change.before),
                            value_label(&change.after)
                        ));
                    }
                    for diagnostic in &preview.diagnostics {
                        ui.label(diagnostic);
                    }
                });
            if let Some(problem) = &self.transfer_problem {
                ui.colored_label(egui::Color32::LIGHT_RED, problem);
            }
            if ui.button("Replace User preferences").clicked() {
                let result = self
                    .directory
                    .as_deref()
                    .ok_or_else(|| "No configuration directory".to_owned())
                    .and_then(|directory| self.preferences.confirm_user_import(preview, directory));
                match result {
                    Ok(()) => {
                        self.user_import = None;
                        self.transfer_problem = None;
                    }
                    Err(error) => self.transfer_problem = Some(error),
                }
            }
            if ui.button("Cancel import").clicked() {
                self.user_import = None;
                self.transfer_problem = None;
            }
            return true;
        }
        egui::CollapsingHeader::new("User preferences file").show(ui, |ui| {
            ui.label("Export a portable User profile, or preview a file before replacing User preferences.");
            ui.text_edit_singleline(&mut self.profile_path);
            ui.horizontal(|ui| {
                if ui.button("Export User preferences").clicked() {
                    self.transfer_problem = self.preferences.export_user_file(std::path::Path::new(&self.profile_path)).err();
                }
                if ui.button("Preview import").clicked() {
                    let result = std::fs::read_to_string(&self.profile_path).map_err(|error| error.to_string())
                        .and_then(|json| self.preferences.preview_user_import(&json));
                    match result {
                        Ok(preview) => { self.bindings.cancel_capture(); self.user_import = Some(preview); self.transfer_problem = None; },
                        Err(error) => self.transfer_problem = Some(error),
                    }
                }
            });
            if let Some(problem) = &self.transfer_problem { ui.colored_label(egui::Color32::LIGHT_RED, problem); }
        });
        false
    }

    fn navigation(&mut self, ui: &mut egui::Ui) {
        if let Some((label, ids)) = self.reset_request.clone() {
            ui.heading(label);
            ui.label("This removes the listed overrides. Character bindings and dormant entries from other platforms are preserved.");
            for id in &ids {
                if let Some(entry) = self
                    .preferences
                    .presentation()
                    .iter()
                    .find(|entry| entry.id.as_str() == id)
                {
                    ui.label(self.preferences.text(entry.metadata.label_key.as_str()));
                }
            }
            if ui.button("Confirm reset").clicked() {
                let result = self
                    .preferences
                    .reset_many(&ids)
                    .and_then(|()| self.save_targets(&ids));
                self.problem = result.err();
                self.reset_request = None;
            }
            if ui.button("Cancel reset").clicked() {
                self.reset_request = None;
            }
            return;
        }
        let categories = self.preferences.categories().to_vec();
        if self.category.is_none() {
            self.category = categories.first().cloned();
        }
        ui.horizontal(|ui| {
            ui.label("Search settings");
            ui.text_edit_singleline(&mut self.query);
        });
        let entries = self.preferences.presentation();
        if !self.query.trim().is_empty() {
            let query = self.query.trim().to_lowercase();
            let mut matches = 0;
            for category in &categories {
                let found = entries
                    .iter()
                    .filter(|entry| {
                        if entry.metadata.category_key.as_str() != category {
                            return false;
                        }
                        let metadata = &entry.metadata;
                        let mut text = format!(
                            "{} {}",
                            entry.id,
                            self.preferences.text(metadata.label_key.as_str())
                        );
                        if let Some(description) = &metadata.description_key {
                            text.push_str(self.preferences.text(description.as_str()));
                        }
                        for key in &metadata.search_keys {
                            text.push_str(self.preferences.text(key.as_str()));
                        }
                        text.to_lowercase().contains(&query)
                    })
                    .collect::<Vec<_>>();
                if found.is_empty() {
                    continue;
                }
                ui.strong(self.preferences.text(category));
                for entry in found {
                    matches += 1;
                    if ui
                        .button(self.preferences.text(entry.metadata.label_key.as_str()))
                        .clicked()
                    {
                        self.category = Some(category.clone());
                        self.search_target = Some(entry.id.to_string());
                        self.focus_target = Some(entry.id.to_string());
                        self.query.clear();
                        if entry.metadata.discoverability
                            == settings_presentation::Discoverability::Advanced
                        {
                            self.advanced.insert(category.clone());
                        }
                    }
                }
            }
            if matches == 0 {
                ui.label("No matching settings.");
            }
            return;
        }
        egui::ComboBox::from_label("Category")
            .selected_text(
                self.preferences
                    .text(self.category.as_deref().unwrap_or("")),
            )
            .show_ui(ui, |ui| {
                for category in &categories {
                    if ui
                        .selectable_label(
                            self.category.as_ref() == Some(category),
                            self.preferences.text(category),
                        )
                        .clicked()
                    {
                        self.category = Some(category.clone());
                        self.search_target = None;
                    }
                }
            });
        ui.label("Changes apply immediately.");
        let selected = self.category.clone().unwrap_or_default();
        let category_entries = entries
            .iter()
            .filter(|entry| entry.metadata.category_key.as_str() == selected)
            .collect::<Vec<_>>();
        let mut edits = Vec::new();
        for entry in &category_entries {
            if entry.metadata.discoverability == settings_presentation::Discoverability::Primary
                || self.search_target.as_deref() == Some(entry.id.as_str())
                    && entry.metadata.discoverability
                        == settings_presentation::Discoverability::SearchOnly
            {
                self.preference_row(ui, entry, &mut edits);
            }
        }
        let advanced = category_entries
            .iter()
            .filter(|entry| {
                entry.metadata.discoverability == settings_presentation::Discoverability::Advanced
            })
            .collect::<Vec<_>>();
        if !advanced.is_empty() {
            let mut open = self.advanced.contains(&selected);
            let response = egui::CollapsingHeader::new("Advanced")
                .id_salt(("settings-advanced", selected.clone()))
                .open(
                    self.focus_target
                        .as_ref()
                        .filter(|target| advanced.iter().any(|entry| entry.id.as_str() == *target))
                        .map(|_| true),
                )
                .default_open(open)
                .show(ui, |ui| {
                    for entry in advanced {
                        self.preference_row(ui, entry, &mut edits);
                    }
                });
            open = response.fully_open();
            if open {
                self.advanced.insert(selected.clone());
            } else {
                self.advanced.remove(&selected);
            }
        }
        for (id, value) in edits {
            let result = match value {
                Some(value) => self.preferences.set(&id, value),
                None => self.preferences.reset(&id),
            };
            self.problem = result.and_then(|()| self.save_targets(&[id])).err();
        }
        let overridden = |ids: Vec<String>| {
            ids.into_iter()
                .filter(|id| self.preferences.overridden(id).unwrap_or(false))
                .collect::<Vec<_>>()
        };
        let category_ids = overridden(
            category_entries
                .iter()
                .map(|entry| entry.id.to_string())
                .collect(),
        );
        let all_ids = overridden(entries.iter().map(|entry| entry.id.to_string()).collect());
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !category_ids.is_empty(),
                    egui::Button::new("Reset category"),
                )
                .clicked()
            {
                self.reset_request = Some((
                    format!("Reset {} preferences?", self.preferences.text(&selected)),
                    category_ids,
                ));
            }
            if ui
                .add_enabled(!all_ids.is_empty(), egui::Button::new("Reset all settings"))
                .clicked()
            {
                self.reset_request = Some(("Reset all applicable preferences?".into(), all_ids));
            }
        });
        if selected == "settings.category.controls" {
            ui.separator();
            self.bindings.show(ui);
        }
    }

    fn save_targets(&self, ids: &[String]) -> Result<(), String> {
        let directory = self
            .directory
            .as_deref()
            .ok_or("No configuration directory")?;
        let scopes = ids
            .iter()
            .map(|id| {
                self.preferences
                    .definition(id)
                    .map(|definition| definition.scope)
            })
            .collect::<Result<std::collections::BTreeSet<_>, _>>()?;
        for scope in scopes {
            self.preferences.save_scope(directory, scope)?;
        }
        Ok(())
    }

    fn preference_row(
        &mut self,
        ui: &mut egui::Ui,
        entry: &settings_presentation::PresentationEntry,
        edits: &mut Vec<(String, Option<SettingValue>)>,
    ) {
        let id = entry.id.as_str();
        let value = self
            .preferences
            .value(id)
            .expect("declared setting")
            .clone();
        let kind = self
            .preferences
            .definition(id)
            .expect("declared setting")
            .kind
            .clone();
        let label = self
            .preferences
            .text(entry.metadata.label_key.as_str())
            .to_owned();
        ui.push_id(id, |ui| {
            ui.horizontal(|ui| {
                ui.label(label);
                let mut edit = None;
                let response = match (value, kind) {
                    (SettingValue::Integer(mut value), SettingKind::Integer { min, max }) => {
                        let response = ui.add(egui::DragValue::new(&mut value).range(min..=max));
                        if response.changed() {
                            edit = Some(SettingValue::Integer(value));
                        }
                        response
                    }
                    (SettingValue::Number(mut value), SettingKind::Number { min, max }) => {
                        let response = ui.add(
                            egui::DragValue::new(&mut value)
                                .range(min..=max)
                                .speed(0.05),
                        );
                        if response.changed() {
                            edit = Some(SettingValue::Number(value));
                        }
                        response
                    }
                    (SettingValue::Bool(mut value), SettingKind::Bool) => {
                        let response = ui.checkbox(&mut value, "");
                        if response.changed() {
                            edit = Some(SettingValue::Bool(value));
                        }
                        response
                    }
                    (SettingValue::Choice(mut value), SettingKind::Choice { options }) => {
                        let before = value.clone();
                        let response = egui::ComboBox::from_id_salt("value")
                            .selected_text(
                                self.preferences.text(&format!("settings.choice.{value}")),
                            )
                            .show_ui(ui, |ui| {
                                for option in options {
                                    ui.selectable_value(
                                        &mut value,
                                        option.clone(),
                                        self.preferences.text(&format!("settings.choice.{option}")),
                                    );
                                }
                            })
                            .response;
                        if value != before {
                            edit = Some(SettingValue::Choice(value));
                        }
                        response
                    }
                    _ => ui.label("This preference has no native control."),
                };
                if self.focus_target.as_deref() == Some(id) {
                    response.request_focus();
                    self.focus_target = None;
                }
                if let Some(value) = edit {
                    edits.push((id.into(), Some(value)));
                }
                if self.preferences.overridden(id).unwrap_or(false)
                    && ui.small_button("Reset").clicked()
                {
                    edits.push((id.into(), None));
                }
            });
            if let Some(description) = &entry.metadata.description_key {
                ui.small(self.preferences.text(description.as_str()));
            }
        });
    }

    /// Drawn on selection and in the world. Returns whether Settings owns input.
    pub fn show(&mut self, ctx: &egui::Context) -> bool {
        egui::Area::new(egui::Id::unique("settings-entry"))
            .anchor(egui::Align2::RIGHT_BOTTOM, [-12.0, -12.0])
            .show(ctx, |ui| {
                if ui.button("Settings").clicked() {
                    self.open = true;
                }
            });
        if !self.open {
            return false;
        }
        let response = egui::Modal::new(egui::Id::unique("settings-modal")).show(ctx, |ui| {
            ui.set_min_width(340.0);
            ui.heading("Settings");
            if self.reset_request.is_none() && self.user_preferences_file(ui) {
                if ui.button("Close").clicked() {
                    self.open = false;
                }
                return;
            }
            self.navigation(ui);
            if let Some(problem) = &self.problem {
                ui.colored_label(egui::Color32::LIGHT_RED, problem);
                if ui.button("Retry saving preferences").clicked() {
                    self.problem = self
                        .directory
                        .as_deref()
                        .ok_or_else(|| "No configuration directory".to_owned())
                        .and_then(|directory| self.preferences.save(directory))
                        .err();
                }
            }
            if ui.button("Close").clicked() {
                self.open = false;
            }
        });
        if response.should_close() {
            if self.reset_request.is_some() {
                self.reset_request = None;
            } else if self.user_import.is_some() {
                self.user_import = None;
            } else {
                self.open = false;
            }
        }
        if !self.open {
            self.user_import = None;
            self.reset_request = None;
            self.bindings.cancel_capture();
        }
        self.open
    }
}

fn preference_label(id: &str) -> &str {
    match id {
        FIELD_OF_VIEW => "Field of view",
        LOOK_SENSITIVITY => "Look sensitivity",
        INVERT_VERTICAL_LOOK => "Invert vertical look",
        REDUCE_MOTION => "Motion",
        "audio.master_volume" => "Master volume",
        "audio.effects_volume" => "Effects volume",
        "audio.ui_volume" => "Interface volume",
        "audio.muted" => "Mute",
        _ => id,
    }
}
fn value_label(value: &SettingValue) -> String {
    match value {
        SettingValue::Bool(value) => if *value { "On" } else { "Off" }.into(),
        SettingValue::Integer(value) => value.to_string(),
        SettingValue::Number(value) => value.to_string(),
        SettingValue::Text(value) | SettingValue::Choice(value) => value.clone(),
    }
}
