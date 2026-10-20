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
            if self.user_preferences_file(ui) {
                if ui.button("Close").clicked() {
                    self.open = false;
                }
                return;
            }
            ui.label("Changes apply immediately.");
            let mut edits = Vec::new();
            for (id, label) in [
                (FIELD_OF_VIEW, "Field of view (°)"),
                (LOOK_SENSITIVITY, "Look sensitivity"),
            ] {
                let value = self
                    .preferences
                    .value(id)
                    .expect("known preference")
                    .clone();
                let kind = self
                    .preferences
                    .definition(id)
                    .expect("known preference")
                    .kind
                    .clone();
                ui.horizontal(|ui| {
                    ui.label(label);
                    let changed = match (value, kind) {
                        (SettingValue::Integer(mut value), SettingKind::Integer { min, max }) => ui
                            .add(egui::DragValue::new(&mut value).range(min..=max))
                            .changed()
                            .then_some(SettingValue::Integer(value)),
                        (SettingValue::Number(mut value), SettingKind::Number { min, max }) => ui
                            .add(
                                egui::DragValue::new(&mut value)
                                    .range(min..=max)
                                    .speed(0.05),
                            )
                            .changed()
                            .then_some(SettingValue::Number(value)),
                        _ => None,
                    };
                    if let Some(value) = changed {
                        edits.push((id, Some(value)));
                    }
                    if ui.small_button("Reset").clicked() {
                        edits.push((id, None));
                    }
                });
            }
            let mut inverted = matches!(
                self.preferences.value(INVERT_VERTICAL_LOOK),
                Ok(SettingValue::Bool(true))
            );
            if ui.checkbox(&mut inverted, "Invert vertical look").changed() {
                edits.push((INVERT_VERTICAL_LOOK, Some(SettingValue::Bool(inverted))));
            }
            if ui.small_button("Reset look inversion").clicked() {
                edits.push((INVERT_VERTICAL_LOOK, None));
            }
            let SettingValue::Choice(mut motion) = self
                .preferences
                .value(REDUCE_MOTION)
                .expect("known preference")
                .clone()
            else {
                unreachable!("catalog motion choice")
            };
            let before = motion.clone();
            egui::ComboBox::from_label("Motion")
                .selected_text(match motion.as_str() {
                    "reduce" => "Reduce",
                    "full" => "Full motion",
                    _ => "Follow system",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut motion, "system".into(), "Follow system");
                    ui.selectable_value(&mut motion, "reduce".into(), "Reduce");
                    ui.selectable_value(&mut motion, "full".into(), "Full motion");
                });
            if motion != before {
                edits.push((REDUCE_MOTION, Some(SettingValue::Choice(motion))));
            }
            if ui.button("Reset motion").clicked() {
                edits.push((REDUCE_MOTION, None));
            }
            for (id, value) in edits {
                let result = match value {
                    Some(value) => self.preferences.set(id, value),
                    None => self.preferences.reset(id),
                };
                match result.and_then(|()| {
                    self.directory
                        .as_deref()
                        .ok_or_else(|| "No configuration directory".into())
                        .and_then(|dir| self.preferences.save(dir))
                }) {
                    Ok(()) => self.problem = None,
                    Err(error) => self.problem = Some(error),
                }
            }
            ui.separator();
            self.bindings.show(ui);
            if let Some(problem) = &self.problem {
                ui.colored_label(egui::Color32::LIGHT_RED, problem);
            }
            if ui.button("Close").clicked() {
                self.open = false;
            }
        });
        if response.should_close() {
            self.open = false;
        }
        if !self.open {
            self.user_import = None;
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
