//! MMORPG world-action catalog and thin native strokes over input-bindings.
use input_bindings_core::{
    ActionDefinition, ActionRegistry, Binding, ContextLayer, DeviceClass, DeviceStroke,
    InputRuntime, InputRuntimeOptions, InputStroke, KeyMatch, KeyStroke, Modifiers, RepeatPolicy,
    SemanticControlState, WhenExpr,
};
use strum::VariantArray;
use winit::keyboard::KeyCode;

#[derive(Clone, Copy, Debug, PartialEq, Eq, strum::VariantArray)]
pub enum WorldAction {
    /// A right click that never became a camera drag selects (#517: the left button swings).
    Select,
    /// The left mouse button swings the weapon on press (#517).
    Swing,
    Interact,
    NextNpc,
    Cancel,
    Primary,
    Sell,
    Previous,
    Next,
    RewardPrevious,
    RewardNext,
    Details,
}
impl WorldAction {
    fn definition(self) -> (&'static str, &'static str, InputStroke) {
        let (id, key, context) = match self {
            Self::Select => return ("world.select", "world", mouse(2)),
            Self::Swing => return ("world.swing", "world", mouse(0)),
            Self::Details => ("interaction.details", "KeyH", "interaction"),
            // E interacts (#515), as the browser's `ui.talk`; Enter only chats. Q stays unbound.
            Self::Interact => ("world.interact", "KeyE", "world"),
            Self::NextNpc => ("world.nextNpc", "KeyN", "world"),
            Self::Cancel => ("world.cancel", "Escape", "world"),
            Self::Primary => ("interaction.primary", "Digit1", "interaction"),
            Self::Sell => ("interaction.sell", "Digit2", "interaction"),
            Self::Previous => ("interaction.previous", "ArrowUp", "interaction"),
            Self::Next => ("interaction.next", "ArrowDown", "interaction"),
            Self::RewardPrevious => ("interaction.rewardPrevious", "ArrowLeft", "interaction"),
            Self::RewardNext => ("interaction.rewardNext", "ArrowRight", "interaction"),
        };
        (
            id,
            context,
            InputStroke::Keyboard(KeyStroke {
                key: KeyMatch::Physical { value: key.into() },
                modifiers: Modifiers::default(),
            }),
        )
    }
}
fn mouse(button: u16) -> InputStroke {
    InputStroke::Device(DeviceStroke::MouseButton {
        button,
        modifiers: Modifiers::default(),
    })
}
pub fn world_action_registry() -> ActionRegistry {
    ActionRegistry {
        actions: WorldAction::VARIANTS
            .iter()
            .copied()
            .map(|action| {
                let (id, context, stroke) = action.definition();
                ActionDefinition {
                    id: id.into(),
                    title: id.into(),
                    description: None,
                    category_path: Vec::new(),
                    repeat_policy: RepeatPolicy::Never,
                    allowed_devices: vec![if matches!(
                        action,
                        WorldAction::Select | WorldAction::Swing
                    ) {
                        DeviceClass::Mouse
                    } else {
                        DeviceClass::Keyboard
                    }],
                    defaults: vec![Binding {
                        id: format!("{id}.0"),
                        action: id.into(),
                        sequence: vec![stroke],
                        when: WhenExpr::Context { id: context.into() },
                        priority: 0,
                    }],
                    provenance: None,
                }
            })
            .collect(),
    }
}

/// What one mouse button edge did in the world.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PointerEdge {
    /// The release ended a selection click.
    pub selected: bool,
    /// The press swung the weapon (#517).
    pub swung: bool,
}

#[derive(Clone, Debug)]
pub struct WorldControls {
    runtime: InputRuntime,
    state: SemanticControlState,
    interaction: bool,
    sources: crate::controls::KeyboardSources,
    pointer_sources: std::collections::BTreeMap<u16, InputStroke>,
}
impl Default for WorldControls {
    fn default() -> Self {
        Self::new()
    }
}
impl WorldControls {
    #[must_use]
    pub fn new() -> Self {
        let registry = crate::bindings::action_registry();
        let mut runtime = InputRuntime::new(registry, None, InputRuntimeOptions::default());
        assert!(
            runtime.validation_report().valid,
            "{:?}",
            runtime.validation_report().diagnostics
        );
        runtime.set_context_stack(Some(vec![ContextLayer {
            id: "world".into(),
            blocks_lower: false,
        }]));
        Self {
            runtime,
            state: SemanticControlState::new(),
            interaction: false,
            sources: crate::controls::KeyboardSources::default(),
            pointer_sources: std::collections::BTreeMap::new(),
        }
    }
    pub fn set_profile(&mut self, profile: input_bindings_core::Profile) -> Result<(), String> {
        crate::bindings::validate_profile(&profile)?;
        let decision = self.runtime.update_profile(Some(profile));
        self.state.apply_decisions(&[decision]);
        self.state.drain_presses();
        Ok(())
    }

    pub fn set_interaction(&mut self, active: bool) {
        if self.interaction == active {
            return;
        }
        self.interaction = active;
        let decision = if active {
            self.runtime.push_context(ContextLayer {
                id: "interaction".into(),
                blocks_lower: false,
            })
        } else {
            self.runtime.pop_context().1
        };
        self.state.apply_decisions(&[decision]);
    }
    pub fn key(&mut self, key: KeyCode, down: bool, repeat: bool) -> Vec<WorldAction> {
        self.key_with_modifiers(key, down, repeat, Modifiers::default())
    }
    pub fn key_with_modifiers(
        &mut self,
        key: KeyCode,
        down: bool,
        repeat: bool,
        modifiers: Modifiers,
    ) -> Vec<WorldAction> {
        let decisions = if down {
            self.runtime
                .input_down(self.sources.down(key, modifiers), repeat, 0)
        } else {
            self.runtime.input_up(&self.sources.up(key), 0)
        };
        self.state.apply_decisions(&decisions);
        self.state
            .drain_presses()
            .iter()
            .filter_map(|id| {
                WorldAction::VARIANTS
                    .iter()
                    .copied()
                    .find(|action| action.definition().0 == id)
            })
            .collect()
    }
    pub fn selecting(&self) -> bool {
        self.state.is_held("world.select")
    }
    /// One mouse button edge: whether it ended a selection click, and whether it swung the
    /// weapon (#517), which happens on the press.
    pub fn pointer_button(&mut self, button: u16, down: bool, modifiers: Modifiers) -> PointerEdge {
        let held = self.selecting();
        let stroke = if down {
            self.pointer_sources
                .entry(button)
                .or_insert_with(|| {
                    InputStroke::Device(DeviceStroke::MouseButton { button, modifiers })
                })
                .clone()
        } else {
            self.pointer_sources
                .remove(&button)
                .unwrap_or(InputStroke::Device(DeviceStroke::MouseButton {
                    button,
                    modifiers,
                }))
        };
        let decisions = if down {
            self.runtime.input_down(stroke, false, 0)
        } else {
            self.runtime.input_up(&stroke, 0)
        };
        self.state.apply_decisions(&decisions);
        let swung = self
            .state
            .drain_presses()
            .iter()
            .any(|id| id == "world.swing");
        PointerEdge {
            selected: !down && held && !self.selecting(),
            swung,
        }
    }
    pub fn reset(&mut self) {
        self.sources.clear();
        self.pointer_sources.clear();
        let decision = self.runtime.reset("focusLost");
        self.state.apply_decisions(&[decision]);
        self.state.drain_presses();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_left_button_swings_on_press_and_a_right_click_selects() {
        let mut controls = WorldControls::new();
        let press = controls.pointer_button(0, true, Modifiers::default());
        assert!(
            press.swung && !press.selected,
            "the swing is immediate (#517)"
        );
        assert_eq!(
            controls.pointer_button(0, false, Modifiers::default()),
            PointerEdge::default()
        );
        assert!(!controls.pointer_button(2, true, Modifiers::default()).swung);
        assert!(controls.selecting());
        assert!(
            controls
                .pointer_button(2, false, Modifiers::default())
                .selected
        );
    }
    #[test]
    fn selection_uses_the_rebound_button_and_keeps_its_original_modifiers_until_release() {
        let registry = crate::bindings::action_registry();
        let mut binding = registry
            .actions
            .iter()
            .find(|action| action.id == "world.select")
            .unwrap()
            .defaults[0]
            .clone();
        binding.sequence = vec![InputStroke::Device(DeviceStroke::MouseButton {
            button: 2,
            modifiers: Modifiers {
                ctrl: true,
                ..Modifiers::default()
            },
        })];
        let mut controls = WorldControls::new();
        controls
            .set_profile(input_bindings_core::Profile {
                id: "character:a".into(),
                patches: vec![input_bindings_core::BindingPatch::Replace {
                    binding_id: binding.id.clone(),
                    binding,
                }],
            })
            .unwrap();
        controls.pointer_button(0, true, Modifiers::default());
        assert!(!controls.selecting());
        assert!(
            !controls
                .pointer_button(0, false, Modifiers::default())
                .selected
        );
        controls.pointer_button(
            2,
            true,
            Modifiers {
                ctrl: true,
                ..Modifiers::default()
            },
        );
        assert!(controls.selecting());
        assert!(
            !controls
                .pointer_button(0, false, Modifiers::default())
                .selected
        );
        assert!(
            controls
                .pointer_button(2, false, Modifiers::default())
                .selected
        );
        assert!(!controls.selecting());
        controls.pointer_button(
            2,
            true,
            Modifiers {
                ctrl: true,
                ..Modifiers::default()
            },
        );
        controls.reset();
        assert!(
            !controls
                .pointer_button(2, false, Modifiers::default())
                .selected
        );
    }
}
