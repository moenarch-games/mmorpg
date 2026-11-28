//! Native semantic controls over the shared `input-bindings` runtime.
//!
//! A [`ControlAction`] is one action of the catalog both clients load,
//! `content/settings/input-actions.json` (#1284: its titles, contexts and
//! default keys live there, not here). The client owns the mapping from those
//! ids to its enum and the translation from winit keys to physical strokes.
//! Binding resolution, repeat suppression and held-state release belong to
//! `input_bindings_core::InputRuntime` and `SemanticControlState`. A control
//! action names an action-bar slot, never an ability: what a slot use does
//! stays with the session and the zone.
use input_bindings_core::{
    ActionRegistry, ContextLayer, InputRuntime, InputRuntimeOptions, InputStroke, KeyMatch,
    KeyStroke, Modifiers, RuntimeActionPhase, RuntimeDecision, RuntimeDispatchReason,
    SemanticControlState,
};
use std::collections::BTreeMap;
use strum::{IntoEnumIterator, VariantArray};
use winit::keyboard::KeyCode;

/// A semantic action the native client resolves from input.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, strum::EnumIter)]
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
    /// Explicitly draw or sheathe the current weapon (#802).
    ToggleWeapon,
    /// Swing the weapon where the character faces (#517): Q and gamepad X; the left mouse
    /// button selects since #636.
    Swing,
    ReleaseSpirit,
    /// Action-bar slot 1–20 (11–20 are the Shift row, #294).
    AbilitySlot(u8),
    /// Cancel the viewer's cast or channel.
    CancelCast,
    /// Open or close an interface window.
    TogglePanel(Panel),
    /// Move keyboard focus to the chat field.
    OpenChat,
    /// Hold the Warden's directed Heroic Strike; mouse motion aims it, the release strikes (#154).
    DirectedStrike,
    /// Hold the Warden's shield guard; mouse motion picks the lane (#156).
    Guard,
    /// A Necromancer's group order for all its minions (#295, Atlas 5.2.84).
    Minions(MinionCommand),
    /// Hold for the player's fixed radial wheel; flick and release, or tap and click (#653).
    FixedWheel,
    /// Hold for the context radial wheel: the target's and situation's actions (#653).
    ContextWheel,
}

/// The four minion orders, as in the browser's `minions.*` actions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, strum::VariantArray)]
pub enum MinionCommand {
    /// Attack the selected creature.
    #[default]
    Attack,
    /// Defend the spot the summoner stands on.
    Defend,
    Follow,
    Retreat,
}

/// A toggled interface window, as in the browser's `ui.toggle*` actions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, strum::VariantArray)]
pub enum Panel {
    #[default]
    Bags,
    Character,
    QuestLog,
    WorldMap,
}

/// The action-bar slots with a key, numbered from 1; 11–20 are Shift+1 to Shift+0 (#294).
pub const ABILITY_SLOTS: u8 = 20;

impl ControlAction {
    /// Every control action in catalog order: declaration order, with its slots, panels and
    /// minion commands flattened.
    pub fn all() -> impl Iterator<Item = Self> {
        Self::iter().flat_map(|action| -> Vec<Self> {
            match action {
                Self::AbilitySlot(_) => (1..=ABILITY_SLOTS).map(Self::AbilitySlot).collect(),
                Self::TogglePanel(_) => Panel::VARIANTS
                    .iter()
                    .copied()
                    .map(Self::TogglePanel)
                    .collect(),
                Self::Minions(_) => MinionCommand::VARIANTS
                    .iter()
                    .copied()
                    .map(Self::Minions)
                    .collect(),
                action => vec![action],
            }
        })
    }

    /// The shared semantic id. A slot outside 1–20 is no control action and has
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
            Self::ToggleWeapon => "combat.toggleWeapon",
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
            Self::AbilitySlot(9) => "ability.slot9",
            Self::AbilitySlot(10) => "ability.slot10",
            Self::AbilitySlot(11) => "ability.slot11",
            Self::AbilitySlot(12) => "ability.slot12",
            Self::AbilitySlot(13) => "ability.slot13",
            Self::AbilitySlot(14) => "ability.slot14",
            Self::AbilitySlot(15) => "ability.slot15",
            Self::AbilitySlot(16) => "ability.slot16",
            Self::AbilitySlot(17) => "ability.slot17",
            Self::AbilitySlot(18) => "ability.slot18",
            Self::AbilitySlot(19) => "ability.slot19",
            Self::AbilitySlot(20) => "ability.slot20",
            Self::AbilitySlot(_) => "",
            Self::CancelCast => "combat.cancelCast",
            Self::TogglePanel(Panel::Bags) => "ui.toggleBags",
            Self::TogglePanel(Panel::Character) => "ui.toggleCharacter",
            Self::TogglePanel(Panel::QuestLog) => "ui.toggleQuestLog",
            Self::TogglePanel(Panel::WorldMap) => "ui.toggleWorldMap",
            Self::OpenChat => "ui.openChat",
            Self::DirectedStrike => "combat.directedStrike",
            Self::Guard => "combat.guard",
            Self::Minions(MinionCommand::Attack) => "minions.attack",
            Self::Minions(MinionCommand::Defend) => "minions.defend",
            Self::Minions(MinionCommand::Follow) => "minions.follow",
            Self::Minions(MinionCommand::Retreat) => "minions.retreat",
            Self::FixedWheel => "wheel.fixed",
            Self::ContextWheel => "wheel.context",
        }
    }

    fn from_id(id: &str) -> Option<Self> {
        Self::all().find(|action| action.id() == id)
    }
}

/// A held action-bar slot let go (#414, #485): the zone reads it only for an experimental
/// graph's hold-to-charge and chord triggers. A key or button going up is a deliberate release;
/// every other retirement (focus loss, a context or profile change) is `retired` and never acts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotRelease {
    /// Action-bar slot 1–20.
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
                .filter(|slot| (1..=ABILITY_SLOTS).contains(slot))?;
            Some(SlotRelease {
                slot,
                retired: dispatch.reason != RuntimeDispatchReason::KeyUp,
            })
        })
}

const WORLD: &str = "world";
const GAMEPLAY: &str = "gameplay";
const COMBAT: &str = "combat";
const EXPLORATION: &str = "exploration";
const CASTING: &str = "casting";

/// The catalog has no chords, so no timeout is ever pending and the runtime
/// needs no clock: every event carries the same timestamp.
const NO_CLOCK_MS: u64 = 0;

/// The catalog every [`ControlAction`] comes from: the actions both clients
/// bind, with their keyboard and pad defaults, as the browser loads them.
#[must_use]
pub fn native_action_registry() -> ActionRegistry {
    crate::bindings::shared_action_registry()
}

/// winit's `KeyCode` names are the W3C physical key codes.
fn stroke(key: KeyCode) -> InputStroke {
    InputStroke::Keyboard(KeyStroke {
        key: KeyMatch::Physical {
            value: format!("{key:?}"),
        },
        modifiers: Modifiers::default(),
    })
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
    weapon_drawn: bool,
    sources: KeyboardSources,
    /// Slot releases not yet drained.
    releases: Vec<SlotRelease>,
    /// Held strike and guard releases not yet drained: the hold and whether a retirement let it go.
    hold_releases: Vec<(mmorpg_client_model::mouse_owner::Hold, bool)>,
    /// Wheel key releases not yet drained: whether a retirement let it go (#653).
    wheel_releases: Vec<bool>,
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
        runtime.set_context_stack(Some(vec![
            layer(WORLD),
            layer(GAMEPLAY),
            layer(EXPLORATION),
        ]));
        Self {
            runtime,
            state: SemanticControlState::new(),
            casting: false,
            weapon_drawn: false,
            sources: KeyboardSources::default(),
            releases: Vec::new(),
            hold_releases: Vec::new(),
            wheel_releases: Vec::new(),
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
        self.restack();
    }

    /// A projected mode change retires held strikes, guards and slot input.
    pub fn set_weapon_drawn(&mut self, drawn: bool) {
        if drawn != self.weapon_drawn {
            self.weapon_drawn = drawn;
            self.restack();
        }
    }

    pub fn weapon_drawn(&self) -> bool {
        self.weapon_drawn
    }

    fn restack(&mut self) {
        let mut stack = vec![
            layer(WORLD),
            layer(GAMEPLAY),
            layer(if self.weapon_drawn {
                COMBAT
            } else {
                EXPLORATION
            }),
        ];
        if self.casting {
            stack.push(layer(CASTING));
        }
        let decision = self.runtime.set_context_stack(Some(stack));
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

    /// The strike and guard key releases since the last drain, in order: the hold and whether a
    /// retirement (focus loss, a context or profile change) rather than a key-up let it go.
    pub fn drain_hold_releases(&mut self) -> Vec<(mmorpg_client_model::mouse_owner::Hold, bool)> {
        std::mem::take(&mut self.hold_releases)
    }

    /// The wheel key releases since the last drain, in order: whether a retirement (focus loss,
    /// a context or profile change) rather than a key-up let it go (#653).
    pub fn drain_wheel_releases(&mut self) -> Vec<bool> {
        std::mem::take(&mut self.wheel_releases)
    }

    fn apply(&mut self, decisions: &[RuntimeDecision]) {
        self.releases.extend(slot_releases(decisions));
        self.wheel_releases.extend(
            decisions
                .iter()
                .flat_map(|decision| &decision.dispatches)
                .filter(|dispatch| {
                    dispatch.phase == RuntimeActionPhase::Release
                        && matches!(dispatch.action.as_str(), "wheel.fixed" | "wheel.context")
                })
                .map(|dispatch| dispatch.reason != RuntimeDispatchReason::KeyUp),
        );
        use mmorpg_client_model::mouse_owner::Hold;
        self.hold_releases.extend(
            decisions
                .iter()
                .flat_map(|decision| &decision.dispatches)
                .filter(|dispatch| dispatch.phase == RuntimeActionPhase::Release)
                .filter_map(|dispatch| {
                    let hold = match dispatch.action.as_str() {
                        "combat.directedStrike" => Hold::Strike,
                        "combat.guard" => Hold::Guard,
                        _ => return None,
                    };
                    Some((hold, dispatch.reason != RuntimeDispatchReason::KeyUp))
                }),
        );
        self.state.apply_decisions(decisions);
    }
}
