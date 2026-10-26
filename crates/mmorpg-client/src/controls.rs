//! Native semantic controls over the shared `input-bindings` runtime.
//!
//! The client owns only the MMORPG catalog (the same action ids, contexts and
//! default physical keys as the browser's `web/src/input/game-controls.ts`)
//! and the translation from winit keys to physical strokes. Binding
//! resolution, repeat suppression and held-state release belong to
//! `input_bindings_core::InputRuntime` and `SemanticControlState`. A control
//! action names an action-bar slot, never an ability: what a slot use does
//! stays with the session and the zone.
use input_bindings_core::{
    ActionDefinition, ActionRegistry, Binding, ContextLayer, DeviceClass, InputRuntime,
    InputRuntimeOptions, InputStroke, KeyMatch, KeyStroke, Modifiers, Provenance, RepeatPolicy,
    RuntimeActionPhase, RuntimeDecision, RuntimeDispatchReason, SemanticControlState, WhenExpr,
};
use std::collections::BTreeMap;
use winit::keyboard::KeyCode;

/// A semantic action the native client resolves from input.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ControlAction {
    MoveForward,
    MoveBackward,
    MoveLeft,
    MoveRight,
    Jump,
    /// Tap to dodge-roll (#516).
    Roll,
    /// Held while swimming: descend (#274).
    Descend,
    TargetNext,
    /// Swing the weapon where the character faces (#517); the left mouse button and gamepad X
    /// swing by default, the keyboard has no default.
    Swing,
    ReleaseSpirit,
    /// Action-bar slot 1–8.
    AbilitySlot(u8),
    /// Cancel the viewer's cast or channel.
    CancelCast,
    /// Open or close an interface window.
    TogglePanel(Panel),
    /// Move keyboard focus to the chat field.
    OpenChat,
    /// Hold the Warden's shield guard; pointer motion picks the lane (#156).
    Guard,
    /// A Necromancer's group order for all its minions (#295, Atlas 5.2.84).
    Minions(MinionCommand),
}

/// The four minion orders, as in the browser's `minions.*` actions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MinionCommand {
    /// Attack the selected creature.
    Attack,
    /// Defend the spot the summoner stands on.
    Defend,
    Follow,
    Retreat,
}

/// A toggled interface window, as in the browser's `ui.toggle*` actions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Panel {
    Bags,
    Character,
    QuestLog,
}

impl ControlAction {
    /// Every control action in catalog order.
    pub const ALL: [Self; 28] = [
        Self::MoveForward,
        Self::MoveBackward,
        Self::MoveLeft,
        Self::MoveRight,
        Self::Jump,
        Self::Roll,
        Self::Descend,
        Self::TargetNext,
        Self::Swing,
        Self::ReleaseSpirit,
        Self::AbilitySlot(1),
        Self::AbilitySlot(2),
        Self::AbilitySlot(3),
        Self::AbilitySlot(4),
        Self::AbilitySlot(5),
        Self::AbilitySlot(6),
        Self::AbilitySlot(7),
        Self::AbilitySlot(8),
        Self::CancelCast,
        Self::TogglePanel(Panel::Bags),
        Self::TogglePanel(Panel::Character),
        Self::TogglePanel(Panel::QuestLog),
        Self::OpenChat,
        Self::Guard,
        Self::Minions(MinionCommand::Attack),
        Self::Minions(MinionCommand::Defend),
        Self::Minions(MinionCommand::Follow),
        Self::Minions(MinionCommand::Retreat),
    ];

    /// The shared semantic id. A slot outside 1–8 is no control action and has
    /// the empty id, which no binding uses.
    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Self::MoveForward => "move.forward",
            Self::MoveBackward => "move.back",
            Self::MoveLeft => "move.strafeLeft",
            Self::MoveRight => "move.strafeRight",
            Self::Jump => "move.jump",
            Self::Roll => "move.roll",
            Self::Descend => "move.descend",
            Self::TargetNext => "target.next",
            Self::Swing => "combat.swing",
            Self::ReleaseSpirit => "player.releaseSpirit",
            Self::AbilitySlot(1) => "ability.slot1",
            Self::AbilitySlot(2) => "ability.slot2",
            Self::AbilitySlot(3) => "ability.slot3",
            Self::AbilitySlot(4) => "ability.slot4",
            Self::AbilitySlot(5) => "ability.slot5",
            Self::AbilitySlot(6) => "ability.slot6",
            Self::AbilitySlot(7) => "ability.slot7",
            Self::AbilitySlot(8) => "ability.slot8",
            Self::AbilitySlot(_) => "",
            Self::CancelCast => "combat.cancelCast",
            Self::TogglePanel(Panel::Bags) => "ui.toggleBags",
            Self::TogglePanel(Panel::Character) => "ui.toggleCharacter",
            Self::TogglePanel(Panel::QuestLog) => "ui.toggleQuestLog",
            Self::OpenChat => "ui.openChat",
            Self::Guard => "combat.guard",
            Self::Minions(MinionCommand::Attack) => "minions.attack",
            Self::Minions(MinionCommand::Defend) => "minions.defend",
            Self::Minions(MinionCommand::Follow) => "minions.follow",
            Self::Minions(MinionCommand::Retreat) => "minions.retreat",
        }
    }

    fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|action| action.id() == id)
    }

    /// Title, context and default physical keys, as in the browser catalog.
    fn default_binding(self) -> (&'static str, &'static str, &'static [&'static str]) {
        match self {
            Self::MoveForward => ("Move forward", GAMEPLAY, &["KeyW", "ArrowUp"]),
            Self::MoveBackward => ("Move backward", GAMEPLAY, &["KeyS", "ArrowDown"]),
            Self::MoveLeft => ("Strafe left", GAMEPLAY, &["KeyA", "ArrowLeft"]),
            Self::MoveRight => ("Strafe right", GAMEPLAY, &["KeyD", "ArrowRight"]),
            Self::Jump => ("Jump (hold to swim up)", GAMEPLAY, &["Space"]),
            Self::Roll => ("Dodge roll", GAMEPLAY, &["ShiftLeft", "ShiftRight"]),
            Self::Descend => ("Swim down (hold)", GAMEPLAY, &["KeyX"]),
            Self::TargetNext => ("Next target", GAMEPLAY, &["Tab"]),
            Self::Swing => ("Swing weapon", GAMEPLAY, &[]),
            Self::ReleaseSpirit => ("Release spirit", GAMEPLAY, &["KeyR"]),
            Self::AbilitySlot(1) => ("Ability 1", GAMEPLAY, &["Digit1"]),
            Self::AbilitySlot(2) => ("Ability 2", GAMEPLAY, &["Digit2"]),
            Self::AbilitySlot(3) => ("Ability 3", GAMEPLAY, &["Digit3"]),
            Self::AbilitySlot(4) => ("Ability 4", GAMEPLAY, &["Digit4"]),
            Self::AbilitySlot(5) => ("Ability 5", GAMEPLAY, &["Digit5"]),
            Self::AbilitySlot(6) => ("Ability 6", GAMEPLAY, &["Digit6"]),
            Self::AbilitySlot(7) => ("Ability 7", GAMEPLAY, &["Digit7"]),
            Self::AbilitySlot(_) => ("Ability 8", GAMEPLAY, &["Digit8"]),
            Self::CancelCast => ("Cancel cast", CASTING, &["Escape"]),
            Self::TogglePanel(Panel::Bags) => ("Toggle bags", GAMEPLAY, &["KeyB"]),
            Self::TogglePanel(Panel::Character) => ("Toggle character pane", GAMEPLAY, &["KeyC"]),
            Self::TogglePanel(Panel::QuestLog) => ("Toggle quest log", GAMEPLAY, &["KeyL"]),
            Self::OpenChat => ("Chat", GAMEPLAY, &["Enter", "NumpadEnter"]),
            Self::Guard => ("Shield guard (hold, aim up/down)", GAMEPLAY, &["KeyZ"]),
            Self::Minions(MinionCommand::Attack) => ("Minions: attack target", GAMEPLAY, &["KeyY"]),
            Self::Minions(MinionCommand::Defend) => ("Minions: defend here", GAMEPLAY, &["KeyU"]),
            Self::Minions(MinionCommand::Follow) => ("Minions: follow", GAMEPLAY, &["KeyI"]),
            Self::Minions(MinionCommand::Retreat) => ("Minions: retreat", GAMEPLAY, &["KeyO"]),
        }
    }
}

/// A held action-bar slot let go (#414, #485): the zone reads it only for an experimental
/// graph's hold-to-charge and chord triggers. A key or button going up is a deliberate release;
/// every other retirement (focus loss, a context or profile change) is `retired` and never acts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotRelease {
    /// Action-bar slot 1–8.
    pub slot: u8,
    pub retired: bool,
}

/// The ability-slot releases among `decisions`, in order.
pub(crate) fn slot_releases(decisions: &[RuntimeDecision]) -> impl Iterator<Item = SlotRelease> {
    decisions
        .iter()
        .flat_map(|decision| &decision.dispatches)
        .filter(|dispatch| dispatch.phase == RuntimeActionPhase::Release)
        .filter_map(|dispatch| {
            let slot = dispatch
                .action
                .strip_prefix("ability.slot")?
                .parse::<u8>()
                .ok()
                .filter(|slot| (1..=8).contains(slot))?;
            Some(SlotRelease {
                slot,
                retired: dispatch.reason != RuntimeDispatchReason::KeyUp,
            })
        })
}

const WORLD: &str = "world";
const GAMEPLAY: &str = "gameplay";
const CASTING: &str = "casting";

/// The catalog has no chords, so no timeout is ever pending and the runtime
/// needs no clock: every event carries the same timestamp.
const NO_CLOCK_MS: u64 = 0;

/// The MMORPG-owned native catalog: every [`ControlAction`] with repeat
/// policy `Never` and its browser default key in the browser's context.
#[must_use]
pub fn native_action_registry() -> ActionRegistry {
    ActionRegistry {
        actions: ControlAction::ALL
            .into_iter()
            .map(|action| {
                let id = action.id();
                let (title, context, keys) = action.default_binding();
                ActionDefinition {
                    id: id.to_owned(),
                    title: title.to_owned(),
                    description: None,
                    category_path: Vec::new(),
                    repeat_policy: RepeatPolicy::Never,
                    allowed_devices: vec![DeviceClass::Keyboard],
                    defaults: keys
                        .iter()
                        .enumerate()
                        .map(|(index, key)| Binding {
                            id: format!("{id}.{index}"),
                            action: id.to_owned(),
                            sequence: vec![physical(key)],
                            when: WhenExpr::Context {
                                id: context.to_owned(),
                            },
                            priority: 0,
                        })
                        .collect(),
                    provenance: Some(Provenance {
                        source: "mmorpg".to_owned(),
                        version: Some("1".to_owned()),
                    }),
                }
            })
            .collect(),
    }
}

fn physical(code: &str) -> InputStroke {
    InputStroke::Keyboard(KeyStroke {
        key: KeyMatch::Physical {
            value: code.to_owned(),
        },
        modifiers: Modifiers::default(),
    })
}

/// winit's `KeyCode` names are the W3C physical key codes.
fn stroke(key: KeyCode) -> InputStroke {
    physical(&format!("{key:?}"))
}

/// Preserve the activation stroke until its physical key releases, even when
/// modifiers or contexts change before key-up. No binding resolution lives here.
#[derive(Clone, Debug, Default)]
pub(crate) struct KeyboardSources(BTreeMap<KeyCode, InputStroke>);
impl KeyboardSources {
    pub(crate) fn down(&mut self, key: KeyCode, modifiers: Modifiers) -> InputStroke {
        self.0
            .entry(key)
            .or_insert_with(|| {
                InputStroke::Keyboard(KeyStroke {
                    key: KeyMatch::Physical {
                        value: format!("{key:?}"),
                    },
                    modifiers,
                })
            })
            .clone()
    }
    pub(crate) fn up(&mut self, key: KeyCode) -> InputStroke {
        self.0.remove(&key).unwrap_or_else(|| stroke(key))
    }
    pub(crate) fn clear(&mut self) {
        self.0.clear();
    }
}

fn layer(id: &str) -> ContextLayer {
    ContextLayer {
        id: id.to_owned(),
        blocks_lower: false,
    }
}

/// Keyboard controls of the in-world native client.
#[derive(Clone, Debug)]
pub struct NativeControls {
    runtime: InputRuntime,
    state: SemanticControlState,
    casting: bool,
    sources: KeyboardSources,
    /// Slot releases not yet drained.
    releases: Vec<SlotRelease>,
}

impl Default for NativeControls {
    fn default() -> Self {
        Self::new()
    }
}

impl NativeControls {
    /// # Panics
    ///
    /// If the native catalog does not validate.
    #[must_use]
    pub fn new() -> Self {
        let mut runtime = InputRuntime::new(
            crate::bindings::action_registry(),
            None,
            InputRuntimeOptions::default(),
        );
        let report = runtime.validation_report();
        assert!(
            report.valid && report.diagnostics.is_empty(),
            "the native control catalog is invalid: {:?}",
            report.diagnostics
        );
        runtime.set_context_stack(Some(vec![layer(WORLD), layer(GAMEPLAY)]));
        Self {
            runtime,
            state: SemanticControlState::new(),
            casting: false,
            sources: KeyboardSources::default(),
            releases: Vec::new(),
        }
    }

    /// Apply a foundation-validated character profile and retire held actions.
    pub fn set_profile(
        &mut self,
        profile: Option<input_bindings_core::Profile>,
    ) -> Result<(), String> {
        let report = input_bindings_core::validate_registry(
            &crate::bindings::action_registry(),
            profile.as_ref(),
        );
        if !report.valid || !report.diagnostics.is_empty() {
            return Err(format!("Invalid control profile: {:?}", report.diagnostics));
        }
        let decision = self.runtime.update_profile(profile);
        self.apply(&[decision]);
        Ok(())
    }

    /// Follows the latest projection's cast: the non-blocking `casting` layer
    /// is on top while the viewer casts. A context change retires held
    /// actions, so an unchanged value leaves the runtime alone.
    pub fn set_casting(&mut self, casting: bool) {
        if casting == self.casting {
            return;
        }
        self.casting = casting;
        let decision = if casting {
            self.runtime.push_context(layer(CASTING))
        } else {
            self.runtime.pop_context().1
        };
        self.apply(&[decision]);
    }

    /// A key went down; `repeat` is OS auto-repeat. Returns the presses this
    /// event dispatched, in order.
    pub fn key_down(&mut self, key: KeyCode, repeat: bool) -> Vec<ControlAction> {
        self.key_down_with_modifiers(key, repeat, Modifiers::default())
    }

    pub fn key_down_with_modifiers(
        &mut self,
        key: KeyCode,
        repeat: bool,
        modifiers: Modifiers,
    ) -> Vec<ControlAction> {
        let stroke = self.sources.down(key, modifiers);
        let decisions = self.runtime.input_down(stroke, repeat, NO_CLOCK_MS);
        self.apply(&decisions);
        self.state
            .drain_presses()
            .iter()
            .filter_map(|id| ControlAction::from_id(id))
            .collect()
    }

    /// A key went up. Releases never use an action; a held slot's release is
    /// queued for [`Self::drain_releases`].
    pub fn key_up(&mut self, key: KeyCode) {
        let stroke = self.sources.up(key);
        let decisions = self.runtime.input_up(&stroke, NO_CLOCK_MS);
        self.apply(&decisions);
    }

    /// The window lost focus: every held action is released.
    pub fn focus_lost(&mut self) {
        self.sources.clear();
        let decision = self.runtime.reset("focusLost");
        self.apply(&[decision]);
    }

    #[must_use]
    pub fn is_held(&self, action: ControlAction) -> bool {
        self.state.is_held(action.id())
    }

    /// Shared semantic held-state aggregation, independent of physical keys.
    pub fn movement(&self) -> crate::gamepad::PadMovement {
        crate::gamepad::PadMovement {
            forward: self.state.axis("move.back", "move.forward"),
            strafe: self.state.axis("move.strafeLeft", "move.strafeRight"),
            steering: [
                ControlAction::MoveForward,
                ControlAction::MoveBackward,
                ControlAction::MoveLeft,
                ControlAction::MoveRight,
            ]
            .into_iter()
            .any(|action| self.is_held(action)),
        }
    }

    /// Held swim rise (#274): Jump ascends, Descend descends.
    #[must_use]
    pub fn rise(&self) -> i8 {
        self.state.axis("move.descend", "move.jump")
    }

    /// The slot releases since the last drain, in order: key-ups and retirements.
    pub fn drain_releases(&mut self) -> Vec<SlotRelease> {
        std::mem::take(&mut self.releases)
    }

    fn apply(&mut self, decisions: &[RuntimeDecision]) {
        self.releases.extend(slot_releases(decisions));
        self.state.apply_decisions(decisions);
    }
}
