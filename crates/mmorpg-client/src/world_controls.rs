//! MMORPG world-action catalog and thin native strokes over input-bindings.
use input_bindings_core::{
    ActionDefinition, ActionRegistry, Binding, ContextLayer, DeviceClass, DeviceStroke,
    InputRuntime, InputRuntimeOptions, InputStroke, KeyMatch, KeyStroke, Modifiers, RepeatPolicy,
    SemanticControlState, WhenExpr,
};
use winit::keyboard::KeyCode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorldAction {
    Select,
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
            Self::Select => return ("world.select", "world", mouse()),
            Self::Details => ("interaction.details", "KeyH", "interaction"),
            Self::Interact => ("world.interact", "Enter", "world"),
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
    const ALL: [Self; 11] = [
        Self::Select,
        Self::Interact,
        Self::NextNpc,
        Self::Cancel,
        Self::Primary,
        Self::Sell,
        Self::Previous,
        Self::Next,
        Self::RewardPrevious,
        Self::RewardNext,
        Self::Details,
    ];
}
fn mouse() -> InputStroke {
    InputStroke::Device(DeviceStroke::MouseButton {
        button: 0,
        modifiers: Modifiers::default(),
    })
}
pub fn world_action_registry() -> ActionRegistry {
    ActionRegistry {
        actions: WorldAction::ALL
            .into_iter()
            .map(|action| {
                let (id, context, stroke) = action.definition();
                ActionDefinition {
                    id: id.into(),
                    title: id.into(),
                    description: None,
                    category_path: Vec::new(),
                    repeat_policy: RepeatPolicy::Never,
                    allowed_devices: vec![if action == WorldAction::Select {
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

#[derive(Clone, Debug)]
pub struct WorldControls {
    runtime: InputRuntime,
    state: SemanticControlState,
    interaction: bool,
    sources: crate::controls::KeyboardSources,
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
                WorldAction::ALL
                    .into_iter()
                    .find(|action| action.definition().0 == id)
            })
            .collect()
    }
    pub fn pointer(&mut self, down: bool) -> bool {
        let held = self.state.is_held("world.select");
        let decisions = if down {
            self.runtime.input_down(mouse(), false, 0)
        } else {
            self.runtime.input_up(&mouse(), 0)
        };
        self.state.apply_decisions(&decisions);
        self.state.drain_presses();
        !down && held
    }
    pub fn reset(&mut self) {
        self.sources.clear();
        let decision = self.runtime.reset("focusLost");
        self.state.apply_decisions(&[decision]);
        self.state.drain_presses();
    }
}
