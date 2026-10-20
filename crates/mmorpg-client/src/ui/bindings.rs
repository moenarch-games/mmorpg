//! Native controls view. The foundation derives deltas, validates edits and plans repairs.
use crate::{bindings, roster::config_dir};
use input_bindings_core::{
    Binding, Conflict, ConflictRepair, ConflictRepairPlan, DeviceClass, DeviceStroke, InputStroke,
    KeyMatch, KeyStroke, Modifiers, Profile, analyze_conflicts, apply_conflict_repair,
    plan_conflict_repairs, profile_from_bindings, validate_registry,
};
use winit::keyboard::KeyCode;

struct Pending {
    bindings: Vec<Binding>,
    accepted: Vec<Conflict>,
}
pub struct BindingPanel {
    directory: Option<std::path::PathBuf>,
    character: Option<String>,
    profile: Option<Profile>,
    bindings: Vec<Binding>,
    pending: Option<Pending>,
    capture: Option<Binding>,
    problem: Option<String>,
    revision: u64,
    search: String,
}
impl Default for BindingPanel {
    fn default() -> Self {
        Self::new(config_dir())
    }
}
impl BindingPanel {
    fn new(directory: Option<std::path::PathBuf>) -> Self {
        Self {
            directory,
            character: None,
            profile: None,
            bindings: Vec::new(),
            pending: None,
            capture: None,
            problem: None,
            revision: 0,
            search: String::new(),
        }
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn profile(&self) -> Option<&Profile> {
        self.profile.as_ref()
    }
    pub fn problem(&mut self, problem: String) {
        self.problem = Some(problem);
    }
    pub fn cancel_capture(&mut self) {
        self.capture = None;
        self.pending = None;
    }

    pub fn select_character(&mut self, character: Option<&str>) {
        if self.character.as_deref() == character {
            return;
        }
        self.cancel_capture();
        self.character = character.map(str::to_owned);
        self.profile = None;
        self.bindings.clear();
        self.problem = None;
        if let Some(character) = character {
            let loaded = self
                .directory
                .as_deref()
                .ok_or("No configuration directory".to_owned())
                .and_then(|directory| bindings::load(&directory, character));
            let profile = match loaded {
                Ok(profile) => profile,
                Err(error) => {
                    self.problem = Some(format!("{error}; using defaults."));
                    bindings::default_profile(character)
                }
            };
            self.bindings =
                validate_registry(&bindings::action_registry(), Some(&profile)).effective_bindings;
            self.profile = Some(profile);
        }
        self.revision = self.revision.wrapping_add(1);
    }

    /// Physical recording is platform input adaptation; editing semantics stay in core.
    pub fn capture_key(&mut self, key: KeyCode, repeat: bool) -> bool {
        if self.capture.is_none() {
            return false;
        }
        if repeat {
            return true;
        }
        if key == KeyCode::Escape {
            self.capture = None;
            return true;
        }
        let mut binding = self.capture.take().expect("capture exists");
        binding.sequence = vec![InputStroke::Keyboard(KeyStroke {
            key: KeyMatch::Physical {
                value: format!("{key:?}"),
            },
            modifiers: Modifiers::default(),
        })];
        self.replace(binding);
        true
    }

    fn replace(&mut self, binding: Binding) {
        let mut next = self.bindings.clone();
        if let Some(existing) = next.iter_mut().find(|existing| existing.id == binding.id) {
            *existing = binding;
        } else {
            next.push(binding);
        }
        self.propose(next);
    }

    fn plans(&self) -> Vec<ConflictRepairPlan> {
        let Some(pending) = &self.pending else {
            return Vec::new();
        };
        let baseline = analyze_conflicts(&self.bindings);
        analyze_conflicts(&pending.bindings)
            .iter()
            .filter(|conflict| !baseline.contains(conflict) && !pending.accepted.contains(conflict))
            .map(|conflict| plan_conflict_repairs(&pending.bindings, conflict))
            .collect()
    }

    fn propose(&mut self, next: Vec<Binding>) {
        let Some(profile) = &self.profile else {
            return;
        };
        let delta = profile_from_bindings(&bindings::action_registry(), &next, &profile.id);
        if let Err(error) = bindings::validate_profile(&delta) {
            self.problem = Some(error);
            return;
        }
        self.pending = Some(Pending {
            bindings: next,
            accepted: Vec::new(),
        });
        self.finish_if_reviewed();
    }

    fn finish_if_reviewed(&mut self) {
        if !self.plans().is_empty() {
            return;
        }
        let (Some(pending), Some(profile), Some(character)) =
            (&self.pending, &self.profile, &self.character)
        else {
            return;
        };
        let delta =
            profile_from_bindings(&bindings::action_registry(), &pending.bindings, &profile.id);
        let result = self
            .directory
            .as_deref()
            .ok_or("No configuration directory".to_owned())
            .and_then(|directory| bindings::save(&directory, character, &delta));
        match result {
            Ok(()) => {
                self.bindings = self.pending.take().expect("pending edit").bindings;
                self.profile = Some(delta);
                self.problem = None;
                self.revision = self.revision.wrapping_add(1);
            }
            Err(error) => self.problem = Some(error),
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui) {
        ui.heading("Controls");
        let Some(_) = &self.character else {
            ui.label("Select a roster character to edit its controls.");
            return;
        };
        ui.label("Confirmed changes apply and save for this character only.");
        if let Some(problem) = &self.problem {
            ui.colored_label(egui::Color32::LIGHT_RED, problem);
        }
        if let Some(binding) = &self.capture {
            ui.label(format!(
                "Press a key for {}. Escape cancels.",
                binding.action
            ));
            return;
        }
        if self.pending.is_some() {
            self.review(ui);
            return;
        }
        ui.horizontal(|ui| {
            ui.label("Find action");
            ui.text_edit_singleline(&mut self.search);
        });
        let registry = bindings::action_registry();
        let mut capture = None;
        let mut replacement = None;
        let mut unbind = None;
        let mut reset = None;
        egui::ScrollArea::vertical()
            .max_height(320.0)
            .show(ui, |ui| {
                for action in &registry.actions {
                    if !format!("{} {}", action.id, action.title)
                        .to_lowercase()
                        .contains(&self.search.to_lowercase())
                    {
                        continue;
                    }
                    ui.push_id(&action.id, |ui| {
                        ui.strong(&action.title);
                        for binding in self
                            .bindings
                            .iter()
                            .filter(|binding| binding.action == action.id)
                        {
                            ui.push_id(&binding.id, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(sequence_label(&binding.sequence));
                                    ui.menu_button("Change", |ui| {
                                        assignment_menu(
                                            ui,
                                            binding,
                                            &action.allowed_devices,
                                            &mut capture,
                                            &mut replacement,
                                        );
                                    });
                                    if ui.small_button("Unbind").clicked() {
                                        unbind = Some(binding.id.clone());
                                    }
                                });
                            });
                        }
                        ui.horizontal(|ui| {
                            if let Some(default) = action.defaults.first() {
                                let mut added = default.clone();
                                for index in 0.. {
                                    added.id = format!("{}.custom{index}", action.id);
                                    if !self.bindings.iter().any(|binding| binding.id == added.id) {
                                        break;
                                    }
                                }
                                ui.menu_button("Add binding", |ui| {
                                    assignment_menu(
                                        ui,
                                        &added,
                                        &action.allowed_devices,
                                        &mut capture,
                                        &mut replacement,
                                    );
                                });
                            }
                            if ui.small_button("Reset action").clicked() {
                                reset = Some(action.id.clone());
                            }
                        });
                        ui.separator();
                    });
                }
            });
        if let Some(binding) = capture {
            self.capture = Some(binding);
        }
        if let Some(binding) = replacement {
            self.replace(binding);
        }
        if let Some(id) = unbind {
            self.propose(
                self.bindings
                    .iter()
                    .filter(|binding| binding.id != id)
                    .cloned()
                    .collect(),
            );
        }
        if let Some(id) = reset {
            let mut next = self
                .bindings
                .iter()
                .filter(|binding| binding.action != id)
                .cloned()
                .collect::<Vec<_>>();
            if let Some(action) = registry.actions.iter().find(|action| action.id == id) {
                next.extend(action.defaults.clone());
            }
            self.propose(next);
        }
    }

    fn choose_repair(&mut self, conflict: &Conflict, repair: &ConflictRepair) {
        if let Some(pending) = &mut self.pending {
            if matches!(repair, ConflictRepair::Keep { .. }) {
                pending.accepted.push(conflict.clone());
            }
            pending.bindings = apply_conflict_repair(&pending.bindings, repair);
        }
        self.finish_if_reviewed();
    }

    fn review(&mut self, ui: &mut egui::Ui) {
        let plans = self.plans();
        if let Some(plan) = plans.first() {
            ui.label("Review this conflict before changing saved controls:");
            ui.label(format!(
                "{} ↔ {}",
                plan.conflict.left_binding_id, plan.conflict.right_binding_id
            ));
            let mut chosen = None;
            for repair in &plan.repairs {
                if ui.button(repair_label(repair)).clicked() {
                    chosen = Some(repair.clone());
                }
            }
            if let Some(repair) = chosen {
                self.choose_repair(&plan.conflict, &repair);
            }
        } else if ui.button("Retry saving confirmed edit").clicked() {
            self.finish_if_reviewed();
        }
        if ui.button("Discard binding edit").clicked() {
            self.pending = None;
        }
    }
}

fn assignment_menu(
    ui: &mut egui::Ui,
    template: &Binding,
    allowed: &[DeviceClass],
    capture: &mut Option<Binding>,
    replacement: &mut Option<Binding>,
) {
    if allowed.contains(&DeviceClass::Keyboard) && ui.button("Record keyboard key…").clicked() {
        *capture = Some(template.clone());
        ui.close();
    }
    if allowed.contains(&DeviceClass::Mouse) {
        ui.menu_button("Mouse button", |ui| {
            for (button, label) in [
                (0, "Left"),
                (1, "Middle"),
                (2, "Right"),
                (3, "Back"),
                (4, "Forward"),
            ] {
                if ui.button(label).clicked() {
                    let mut binding = template.clone();
                    binding.sequence = vec![InputStroke::Device(DeviceStroke::MouseButton {
                        button,
                        modifiers: Modifiers::default(),
                    })];
                    *replacement = Some(binding);
                    ui.close();
                }
            }
        });
    }
    if allowed.contains(&DeviceClass::Gamepad) {
        ui.menu_button("Controller button", |ui| {
            for button in 0..crate::gamepad::standard::BUTTONS {
                if ui.button(format!("Button {button}")).clicked() {
                    let mut binding = template.clone();
                    binding.sequence = vec![InputStroke::Device(DeviceStroke::GamepadButton {
                        button: button as u16,
                        threshold: 50,
                        gamepad: None,
                    })];
                    *replacement = Some(binding);
                    ui.close();
                }
            }
        });
    }
}
fn sequence_label(sequence: &[InputStroke]) -> String {
    sequence
        .iter()
        .map(|stroke| match stroke {
            InputStroke::Keyboard(key) => match &key.key {
                KeyMatch::Physical { value } | KeyMatch::Logical { value } => value.clone(),
            },
            InputStroke::Device(DeviceStroke::MouseButton { button, .. }) => {
                format!("Mouse {button}")
            }
            InputStroke::Device(DeviceStroke::GamepadButton { button, .. }) => {
                format!("Controller button {button}")
            }
            InputStroke::Device(DeviceStroke::GamepadAxis {
                axis, direction, ..
            }) => format!("Controller axis {axis} {direction:?}"),
            _ => "Other input".into(),
        })
        .collect::<Vec<_>>()
        .join(" then ")
}
fn repair_label(repair: &ConflictRepair) -> String {
    match repair {
        ConflictRepair::Keep { .. } => "Keep both mappings".into(),
        ConflictRepair::Unbind { binding_id } => format!("Unbind {binding_id}"),
        ConflictRepair::Prefer { binding_id, .. } => format!("Prefer {binding_id}"),
        ConflictRepair::NarrowContext { binding_id, .. } => {
            format!("Restrict {binding_id} to non-conflicting contexts")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recorded_rebinds_persist_per_character_and_conflicts_wait_for_explicit_repair() {
        let directory = tempfile::tempdir().unwrap();
        let mut panel = BindingPanel::new(Some(directory.path().to_path_buf()));
        panel.select_character(Some("a"));
        let jump = |panel: &BindingPanel| {
            panel
                .bindings
                .iter()
                .find(|binding| binding.id == "move.jump.0")
                .unwrap()
                .clone()
        };
        panel.capture = Some(jump(&panel));
        assert!(panel.capture_key(KeyCode::KeyJ, false));
        assert!(panel.pending.is_none());
        let saved = bindings::load(directory.path(), "a").unwrap();
        panel.select_character(Some("b"));
        assert_eq!(panel.profile, Some(bindings::default_profile("b")));
        panel.select_character(Some("a"));
        assert_eq!(panel.profile, Some(saved.clone()));
        panel.capture = Some(jump(&panel));
        let revision = panel.revision();
        panel.capture_key(KeyCode::KeyW, false);
        assert_eq!(panel.revision(), revision);
        assert_eq!(bindings::load(directory.path(), "a").unwrap(), saved);
        let plan = panel.plans().into_iter().find(|plan| plan.repairs.iter().any(|repair| matches!(repair, ConflictRepair::Unbind { binding_id } if binding_id == "move.forward.0"))).unwrap();
        let repair = plan.repairs.iter().find(|repair| matches!(repair, ConflictRepair::Unbind { binding_id } if binding_id == "move.forward.0")).unwrap();
        panel.choose_repair(&plan.conflict, repair);
        assert!(panel.pending.is_none());
        assert_ne!(panel.revision(), revision);
        assert_eq!(
            bindings::load(directory.path(), "a").unwrap(),
            panel.profile.clone().unwrap()
        );
        assert_eq!(
            bindings::load(directory.path(), "b").unwrap(),
            bindings::default_profile("b")
        );
    }
}
