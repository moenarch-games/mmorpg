//! Native Settings controls over the shared validated preference session.
use crate::{
    roster::config_dir,
    settings::{FIELD_OF_VIEW, INVERT_VERTICAL_LOOK, LOOK_SENSITIVITY, Preferences, REDUCE_MOTION},
};
use settings_core::{SettingKind, SettingValue};
use std::path::PathBuf;

pub struct SettingsWindow {
    pub open: bool,
    pub preferences: Preferences,
    pub bindings: super::bindings::BindingPanel,
    directory: Option<PathBuf>,
    problem: Option<String>,
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
        Self {
            open: false,
            preferences,
            bindings: super::bindings::BindingPanel::default(),
            directory,
            problem,
        }
    }
}
impl SettingsWindow {
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
            self.bindings.cancel_capture();
        }
        self.open
    }
}
