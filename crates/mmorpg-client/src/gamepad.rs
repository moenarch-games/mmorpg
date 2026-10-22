//! Native gamepad controls over the shared `input-bindings` runtime (#183).
//!
//! The pad catalog mirrors the browser's standard-mapping gamepad bindings in
//! `web/src/input/game-controls.ts` (same action ids, button/axis indices,
//! thresholds and contexts): the left stick runs and strafes, A jumps, B
//! closes a panel or cancels a cast, X toggles auto-attack, Y releases the
//! spirit, the right bumper cycles targets, the d-pad uses slots 1–4 and
//! Select toggles bags. Native additions use otherwise unbound buttons: the
//! left bumper cycles NPCs, Start talks to the selected NPC, the left stick
//! click toggles the character pane and the right trigger the quest log.
//! Holding the left trigger raises the Warden's shield guard (#156, owner
//! answer #156.Q1 A): the right stick's vertical deflection picks the lane
//! (up high, centred middle, down low) and the camera stops tilting meanwhile. While an interface window is
//! open, the non-blocking `panels` layer turns the d-pad and A into focus
//! navigation and activation of the egui controls; movement still falls
//! through to gameplay. B never leaves the world natively: the window has no
//! character screen to return to.
//!
//! Device input comes from `gilrs` (W3C standard indices via [`read_pad`]).
//! Resolution, contexts and held-state release stay with `InputRuntime`;
//! this module only samples sticks and buttons into the runtime's strokes,
//! with the same threshold/deadzone hysteresis as `input-bindings-web`'s
//! `gamepadStrokeActive`. The right stick orbits the presentation camera with
//! an analog rate. Nothing here knows gameplay rules: actions become the same
//! intents as their keys.
use crate::controls::Panel;
use input_bindings_core::{
    ActionDefinition, ActionRegistry, AxisDirection, Binding, ContextLayer, DeviceClass,
    DeviceStroke, InputRuntime, InputRuntimeOptions, InputStroke, Provenance, RepeatPolicy,
    RuntimeDecision, SemanticControlState, WhenExpr,
};

/// A semantic pad action that dispatches once per press.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PadAction {
    Jump,
    TargetNext,
    ToggleAutoAttack,
    /// Action-bar slot 1–4.
    AbilitySlot(u8),
    CancelCast,
    ReleaseSpirit,
    TogglePanel(Panel),
    /// Cycle the selected NPC (native `world.nextNpc`).
    NextNpc,
    /// Talk to the selected NPC (native `world.interact`).
    Talk,
    /// Close the topmost interface window.
    ClosePanel,
    /// Move interface focus to the previous or next control.
    FocusPrevious,
    FocusNext,
    /// Press the focused interface control.
    Activate,
}

/// Standard-mapping indices (W3C Gamepad API), as in the browser catalog.
pub mod standard {
    pub const A: u16 = 0;
    pub const B: u16 = 1;
    pub const X: u16 = 2;
    pub const Y: u16 = 3;
    pub const LB: u16 = 4;
    pub const RB: u16 = 5;
    pub const LT: u16 = 6;
    pub const RT: u16 = 7;
    pub const SELECT: u16 = 8;
    pub const START: u16 = 9;
    pub const L3: u16 = 10;
    pub const R3: u16 = 11;
    pub const DPAD_UP: u16 = 12;
    pub const DPAD_DOWN: u16 = 13;
    pub const DPAD_LEFT: u16 = 14;
    pub const DPAD_RIGHT: u16 = 15;
    pub const HOME: u16 = 16;
    pub const BUTTONS: usize = 17;
    pub const LEFT_X: usize = 0;
    pub const LEFT_Y: usize = 1;
    pub const RIGHT_X: usize = 2;
    pub const RIGHT_Y: usize = 3;
    pub const AXES: usize = 4;
}

/// One pad's sampled state in standard mapping: buttons 0–1 pressed amount,
/// axes −1–1 with +Y pointing down (W3C convention).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PadState {
    pub buttons: [f32; standard::BUTTONS],
    pub axes: [f32; standard::AXES],
}

const WORLD: &str = "world";
const GAMEPLAY: &str = "gameplay";
const CASTING: &str = "casting";
const PANELS: &str = "panels";
const NO_CLOCK_MS: u64 = 0;

const MOVE_FORWARD: &str = "move.forward";
const MOVE_BACK: &str = "move.back";
const MOVE_LEFT: &str = "move.strafeLeft";
const MOVE_RIGHT: &str = "move.strafeRight";
const GUARD: &str = "combat.guard";
const JUMP: &str = "move.jump";
const DESCEND: &str = "move.descend";

fn button(index: u16) -> InputStroke {
    InputStroke::Device(DeviceStroke::GamepadButton {
        button: index,
        threshold: 50,
        gamepad: None,
    })
}

fn stick(axis: u8, direction: AxisDirection) -> InputStroke {
    InputStroke::Device(DeviceStroke::GamepadAxis {
        axis,
        direction,
        threshold: 50,
        deadzone: 20,
        gamepad: None,
    })
}

/// `(id, context, strokes, one-shot action)`; movement is held, not dispatched.
type Entry = (
    &'static str,
    &'static str,
    Vec<InputStroke>,
    Option<PadAction>,
);

fn catalog() -> Vec<Entry> {
    use AxisDirection::{Negative, Positive};
    use standard::*;
    vec![
        (MOVE_FORWARD, GAMEPLAY, vec![stick(1, Negative)], None),
        (MOVE_BACK, GAMEPLAY, vec![stick(1, Positive)], None),
        (MOVE_LEFT, GAMEPLAY, vec![stick(0, Negative)], None),
        (MOVE_RIGHT, GAMEPLAY, vec![stick(0, Positive)], None),
        (JUMP, GAMEPLAY, vec![button(A)], Some(PadAction::Jump)),
        // Held while swimming (#274): A ascends, R3 descends.
        (DESCEND, GAMEPLAY, vec![button(R3)], None),
        (
            "target.next",
            GAMEPLAY,
            vec![button(RB)],
            Some(PadAction::TargetNext),
        ),
        (
            "combat.toggleAutoAttack",
            GAMEPLAY,
            vec![button(X)],
            Some(PadAction::ToggleAutoAttack),
        ),
        (
            "ability.slot1",
            GAMEPLAY,
            vec![button(DPAD_UP)],
            Some(PadAction::AbilitySlot(1)),
        ),
        (
            "ability.slot2",
            GAMEPLAY,
            vec![button(DPAD_RIGHT)],
            Some(PadAction::AbilitySlot(2)),
        ),
        (
            "ability.slot3",
            GAMEPLAY,
            vec![button(DPAD_DOWN)],
            Some(PadAction::AbilitySlot(3)),
        ),
        (
            "ability.slot4",
            GAMEPLAY,
            vec![button(DPAD_LEFT)],
            Some(PadAction::AbilitySlot(4)),
        ),
        (
            "combat.cancelCast",
            CASTING,
            vec![button(B)],
            Some(PadAction::CancelCast),
        ),
        (
            "player.releaseSpirit",
            GAMEPLAY,
            vec![button(Y)],
            Some(PadAction::ReleaseSpirit),
        ),
        (
            "ui.toggleBags",
            GAMEPLAY,
            vec![button(SELECT)],
            Some(PadAction::TogglePanel(Panel::Bags)),
        ),
        (
            "ui.toggleCharacter",
            GAMEPLAY,
            vec![button(L3)],
            Some(PadAction::TogglePanel(Panel::Character)),
        ),
        (
            "ui.toggleQuestLog",
            GAMEPLAY,
            vec![button(RT)],
            Some(PadAction::TogglePanel(Panel::QuestLog)),
        ),
        (GUARD, GAMEPLAY, vec![button(LT)], None),
        (
            "world.nextNpc",
            GAMEPLAY,
            vec![button(LB)],
            Some(PadAction::NextNpc),
        ),
        (
            "ui.talk",
            GAMEPLAY,
            vec![button(START)],
            Some(PadAction::Talk),
        ),
        (
            "ui.closePanel",
            PANELS,
            vec![button(B)],
            Some(PadAction::ClosePanel),
        ),
        (
            "ui.focusPrevious",
            PANELS,
            vec![button(DPAD_UP), button(DPAD_LEFT)],
            Some(PadAction::FocusPrevious),
        ),
        (
            "ui.focusNext",
            PANELS,
            vec![button(DPAD_DOWN), button(DPAD_RIGHT)],
            Some(PadAction::FocusNext),
        ),
        (
            "ui.activate",
            PANELS,
            vec![button(A)],
            Some(PadAction::Activate),
        ),
    ]
}

/// The MMORPG-owned native gamepad catalog.
#[must_use]
pub fn pad_action_registry() -> ActionRegistry {
    ActionRegistry {
        actions: catalog()
            .into_iter()
            .map(|(id, context, strokes, _)| ActionDefinition {
                id: id.to_owned(),
                title: id.to_owned(),
                description: None,
                category_path: Vec::new(),
                repeat_policy: RepeatPolicy::Never,
                allowed_devices: vec![DeviceClass::Gamepad],
                defaults: strokes
                    .into_iter()
                    .enumerate()
                    .map(|(index, stroke)| Binding {
                        id: format!("{id}.pad{index}"),
                        action: id.to_owned(),
                        sequence: vec![stroke],
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
            })
            .collect(),
    }
}

/// Whether `stroke` is active on any pad; an active axis stays active until it
/// falls to its deadzone (as `input-bindings-web`'s `gamepadStrokeActive`).
fn stroke_active(stroke: &InputStroke, pads: &[PadState], was_active: bool) -> bool {
    match stroke {
        InputStroke::Device(DeviceStroke::GamepadButton {
            button, threshold, ..
        }) => pads.iter().any(|pad| {
            pad.buttons
                .get(usize::from(*button))
                .is_some_and(|value| value * 100.0 >= f32::from(*threshold))
        }),
        InputStroke::Device(DeviceStroke::GamepadAxis {
            axis,
            direction,
            threshold,
            deadzone,
            ..
        }) => pads.iter().any(|pad| {
            let Some(&value) = pad.axes.get(usize::from(*axis)) else {
                return false;
            };
            let signed = match direction {
                AxisDirection::Positive => value,
                AxisDirection::Negative => -value,
            };
            if !signed.is_finite() || signed <= 0.0 {
                return false;
            }
            let magnitude = signed * 100.0;
            if was_active {
                magnitude > f32::from(*deadzone)
            } else {
                magnitude >= f32::from(*threshold)
            }
        }),
        InputStroke::Keyboard(_) | InputStroke::Device(_) => false,
    }
}

fn layer(id: &str) -> ContextLayer {
    ContextLayer {
        id: id.to_owned(),
        blocks_lower: false,
    }
}

/// Held pad movement: run/backpedal and strafe in −1–1, and whether any
/// movement is held (so the character faces the camera).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PadMovement {
    pub forward: i8,
    pub strafe: i8,
    pub steering: bool,
}

/// Gamepad controls of the native client.
#[derive(Clone, Debug)]
pub struct PadControls {
    runtime: InputRuntime,
    state: SemanticControlState,
    /// Every distinct pad stroke the catalog binds, with whether it is down.
    triggers: Vec<(InputStroke, bool)>,
    casting: bool,
    panels: bool,
    settings: bool,
    inactive: bool,
}

impl Default for PadControls {
    fn default() -> Self {
        Self::new()
    }
}

impl PadControls {
    /// # Panics
    ///
    /// If the native pad catalog does not validate.
    #[must_use]
    pub fn new() -> Self {
        let registry = crate::bindings::action_registry();
        let mut triggers: Vec<(InputStroke, bool)> = Vec::new();
        for action in &registry.actions {
            for binding in &action.defaults {
                for stroke in &binding.sequence {
                    if !triggers.iter().any(|(known, _)| known == stroke) {
                        triggers.push((stroke.clone(), false));
                    }
                }
            }
        }
        let mut runtime = InputRuntime::new(registry, None, InputRuntimeOptions::default());
        let report = runtime.validation_report();
        assert!(
            report.valid,
            "the native pad catalog is invalid: {:?}",
            report.diagnostics
        );
        runtime.set_context_stack(Some(vec![layer(WORLD), layer(GAMEPLAY)]));
        Self {
            runtime,
            state: SemanticControlState::new(),
            triggers,
            casting: false,
            panels: false,
            settings: false,
            inactive: false,
        }
    }

    /// The `casting` layer follows the latest projection's cast.
    pub fn set_casting(&mut self, casting: bool) {
        if casting != self.casting {
            self.casting = casting;
            self.restack();
        }
    }

    pub fn set_profile(&mut self, profile: input_bindings_core::Profile) -> Result<(), String> {
        crate::bindings::validate_profile(&profile)?;
        let decision = self.runtime.update_profile(Some(profile));
        self.state.apply_decisions(&[decision]);
        self.state.drain_presses();
        // Await physical release before admitting a held control under its new mapping.
        self.triggers.clear();
        for binding in self.runtime.effective_bindings() {
            for stroke in &binding.sequence {
                if !self.triggers.iter().any(|(known, _)| known == stroke) {
                    self.triggers.push((stroke.clone(), true));
                }
            }
        }
        Ok(())
    }

    /// Settings is a blocking gameplay context with interface navigation above it.
    /// Inactive windows/chat block every pad action. Continue sampling physical
    /// states so a control must actually release before it can activate again.
    pub fn set_input_context(&mut self, panels: bool, settings: bool, inactive: bool) {
        if (self.panels, self.settings, self.inactive) == (panels, settings, inactive) {
            return;
        }
        let held = self
            .triggers
            .iter()
            .map(|(_, active)| *active)
            .collect::<Vec<_>>();
        self.panels = panels;
        self.settings = settings;
        self.inactive = inactive;
        self.restack();
        for ((_, active), previous) in self.triggers.iter_mut().zip(held) {
            *active = previous;
        }
    }

    /// The `panels` layer is on top while an interface window is open.
    pub fn set_panels(&mut self, open: bool) {
        if open != self.panels {
            self.panels = open;
            self.restack();
        }
    }

    fn restack(&mut self) {
        let mut stack = vec![layer(WORLD), layer(GAMEPLAY)];
        if self.casting {
            stack.push(layer(CASTING));
        }
        if self.settings {
            stack.push(ContextLayer {
                id: "settings".into(),
                blocks_lower: true,
            });
        }
        if self.panels {
            stack.push(layer(PANELS));
        }
        if self.inactive {
            stack.push(ContextLayer {
                id: "inactive".into(),
                blocks_lower: true,
            });
        }
        let decision = self.runtime.set_context_stack(Some(stack));
        self.settle(&[decision]);
    }

    /// A context change, disconnect or focus loss retires every held action.
    /// A still-deflected stick re-presses on the next sample, so movement
    /// resumes under the new contexts; a still-held button stays latched
    /// until released, so it never fires twice.
    fn settle(&mut self, decisions: &[RuntimeDecision]) {
        self.state.apply_decisions(decisions);
        self.state.drain_presses();
        for (stroke, active) in &mut self.triggers {
            if !self.settings
                && !self.inactive
                && matches!(
                    stroke,
                    InputStroke::Device(DeviceStroke::GamepadAxis { .. })
                )
            {
                *active = false;
            }
        }
    }

    /// The window lost focus or a pad disconnected: held input lets go.
    pub fn retire(&mut self, reason: &str) {
        let decision = self.runtime.reset(reason);
        self.settle(&[decision]);
    }

    /// Samples every connected pad once and returns the presses dispatched,
    /// in order. Releases never act.
    pub fn sample(&mut self, pads: &[PadState]) -> Vec<PadAction> {
        let mut decisions = Vec::new();
        for (stroke, active) in &mut self.triggers {
            let next = stroke_active(stroke, pads, *active);
            if next && !*active {
                decisions.extend(self.runtime.input_down(stroke.clone(), false, NO_CLOCK_MS));
            } else if !next && *active {
                decisions.extend(self.runtime.input_up(stroke, NO_CLOCK_MS));
            }
            *active = next;
        }
        self.state.apply_decisions(&decisions);
        let catalog = catalog();
        self.state
            .drain_presses()
            .iter()
            .filter_map(|id| {
                catalog
                    .iter()
                    .find(|(known, ..)| known == id)
                    .and_then(|(.., action)| *action)
            })
            .collect()
    }

    /// Whether the pad holds the shield guard (left trigger).
    #[must_use]
    pub fn guarding(&self) -> bool {
        self.state.is_held(GUARD)
    }

    /// Held swim rise (#274): A ascends, R3 descends.
    #[must_use]
    pub fn rise(&self) -> i8 {
        self.state.axis(DESCEND, JUMP)
    }

    /// Movement held on the pad, as the keyboard's held movement.
    #[must_use]
    pub fn movement(&self) -> PadMovement {
        let held = |id| self.state.is_held(id);
        PadMovement {
            forward: self.state.axis(MOVE_BACK, MOVE_FORWARD),
            strafe: self.state.axis(MOVE_LEFT, MOVE_RIGHT),
            steering: [MOVE_FORWARD, MOVE_BACK, MOVE_LEFT, MOVE_RIGHT]
                .into_iter()
                .any(held),
        }
    }
}

/// The egui key a pad action presses in the interface: Tab/Shift+Tab move
/// the focus, Enter presses the focused control and Escape drops the focus.
#[must_use]
pub fn interface_key(action: PadAction) -> Option<egui::Event> {
    let (key, modifiers) = match action {
        PadAction::FocusNext => (egui::Key::Tab, egui::Modifiers::NONE),
        PadAction::FocusPrevious => (egui::Key::Tab, egui::Modifiers::SHIFT),
        PadAction::Activate => (egui::Key::Enter, egui::Modifiers::NONE),
        PadAction::ClosePanel => (egui::Key::Escape, egui::Modifiers::NONE),
        _ => return None,
    };
    Some(egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    })
}

/// Below this deflection the camera stick is at rest.
const CAMERA_DEADZONE: f32 = 0.2;
/// Full deflection orbits this many mouse-drag pixels per second.
const CAMERA_PIXELS_PER_SECOND: f32 = 420.0;

/// Right-stick deflection past which a held guard aims high or low.
const GUARD_LANE_DEFLECTION: f32 = 0.5;

/// The lane a held pad guard aims: the right stick's most deflected vertical
/// reading, up high, down low, otherwise middle.
#[must_use]
pub fn guard_lane(pads: &[PadState]) -> mmorpg_core::StrikeHeight {
    let y = pads
        .iter()
        .map(|pad| pad.axes[standard::RIGHT_Y])
        .filter(|value| value.is_finite())
        .fold(0.0_f32, |most, value| {
            if value.abs() > most.abs() {
                value
            } else {
                most
            }
        });
    if y <= -GUARD_LANE_DEFLECTION {
        mmorpg_core::StrikeHeight::High
    } else if y >= GUARD_LANE_DEFLECTION {
        mmorpg_core::StrikeHeight::Low
    } else {
        mmorpg_core::StrikeHeight::Middle
    }
}

/// The right stick's analog camera orbit for `seconds`, in the mouse drag's
/// pixel units: right turns the view right, down raises the eye.
#[must_use]
pub fn camera_orbit(pads: &[PadState], seconds: f32) -> [f64; 2] {
    let mut total = [0.0_f32; 2];
    for pad in pads {
        let x = pad.axes[standard::RIGHT_X];
        let y = pad.axes[standard::RIGHT_Y];
        let length = x.hypot(y);
        if !length.is_finite() || length <= CAMERA_DEADZONE {
            continue;
        }
        // Rescale past the deadzone so the rate starts at zero.
        let scale = ((length - CAMERA_DEADZONE) / (1.0 - CAMERA_DEADZONE)).min(1.0) / length;
        total[0] += x * scale;
        total[1] += y * scale;
    }
    let step = CAMERA_PIXELS_PER_SECOND * seconds.clamp(0.0, 0.1);
    total.map(|value| f64::from(value.clamp(-1.0, 1.0) * step))
}

/// One `gilrs` pad in standard mapping (gilrs' bumpers are `LeftTrigger`/
/// `RightTrigger`, its triggers `…Trigger2`; its stick Y points up).
#[must_use]
pub fn read_pad(pad: &gilrs::Gamepad<'_>) -> PadState {
    use gilrs::{Axis, Button};
    use standard::*;
    const BUTTONS: [(Button, u16); 17] = [
        (Button::South, A),
        (Button::East, B),
        (Button::West, X),
        (Button::North, Y),
        (Button::LeftTrigger, LB),
        (Button::RightTrigger, RB),
        (Button::LeftTrigger2, LT),
        (Button::RightTrigger2, RT),
        (Button::Select, SELECT),
        (Button::Start, START),
        (Button::LeftThumb, L3),
        (Button::RightThumb, R3),
        (Button::DPadUp, DPAD_UP),
        (Button::DPadDown, DPAD_DOWN),
        (Button::DPadLeft, DPAD_LEFT),
        (Button::DPadRight, DPAD_RIGHT),
        (Button::Mode, HOME),
    ];
    let mut state = PadState::default();
    for (button, index) in BUTTONS {
        state.buttons[usize::from(index)] = pad.button_data(button).map_or(
            if pad.is_pressed(button) { 1.0 } else { 0.0 },
            gilrs::ev::state::ButtonData::value,
        );
    }
    // Pads that report the d-pad as a hat axis.
    let hat = [pad.value(Axis::DPadX), -pad.value(Axis::DPadY)];
    for (value, negative, positive) in [
        (hat[0], DPAD_LEFT, DPAD_RIGHT),
        (hat[1], DPAD_UP, DPAD_DOWN),
    ] {
        let (index, amount) = if value < 0.0 {
            (negative, -value)
        } else {
            (positive, value)
        };
        let slot = &mut state.buttons[usize::from(index)];
        *slot = slot.max(amount);
    }
    state.axes = [
        pad.value(Axis::LeftStickX),
        -pad.value(Axis::LeftStickY),
        pad.value(Axis::RightStickX),
        -pad.value(Axis::RightStickY),
    ];
    state
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad() -> PadState {
        PadState::default()
    }

    fn pressed(index: u16) -> PadState {
        let mut state = pad();
        state.buttons[usize::from(index)] = 1.0;
        state
    }

    fn left_stick(x: f32, y: f32) -> PadState {
        let mut state = pad();
        state.axes[standard::LEFT_X] = x;
        state.axes[standard::LEFT_Y] = y;
        state
    }

    #[test]
    fn the_left_stick_feeds_held_movement_with_hysteresis() {
        let mut controls = PadControls::new();
        assert!(controls.sample(&[left_stick(0.0, -0.6)]).is_empty());
        assert_eq!(
            controls.movement(),
            PadMovement {
                forward: 1,
                strafe: 0,
                steering: true
            }
        );
        // Below the threshold but above the deadzone keeps running.
        controls.sample(&[left_stick(0.0, -0.3)]);
        assert_eq!(controls.movement().forward, 1);
        controls.sample(&[left_stick(0.7, 0.0)]);
        assert_eq!(
            controls.movement(),
            PadMovement {
                forward: 0,
                strafe: 1,
                steering: true
            }
        );
        controls.sample(&[left_stick(0.1, 0.0)]);
        assert_eq!(controls.movement(), PadMovement::default());
        // A fresh press needs the threshold, not just the deadzone.
        controls.sample(&[left_stick(-0.3, 0.6)]);
        assert_eq!(
            controls.movement(),
            PadMovement {
                forward: -1,
                strafe: 0,
                steering: true
            }
        );
    }

    #[test]
    fn buttons_dispatch_the_browser_actions_once_per_press() {
        let mut controls = PadControls::new();
        assert_eq!(controls.sample(&[pressed(standard::A)]), [PadAction::Jump]);
        // Held: no repeat.
        assert!(controls.sample(&[pressed(standard::A)]).is_empty());
        assert!(controls.sample(&[pad()]).is_empty());
        for (button, action) in [
            (standard::RB, PadAction::TargetNext),
            (standard::X, PadAction::ToggleAutoAttack),
            (standard::Y, PadAction::ReleaseSpirit),
            (standard::DPAD_UP, PadAction::AbilitySlot(1)),
            (standard::DPAD_RIGHT, PadAction::AbilitySlot(2)),
            (standard::DPAD_DOWN, PadAction::AbilitySlot(3)),
            (standard::DPAD_LEFT, PadAction::AbilitySlot(4)),
            (standard::SELECT, PadAction::TogglePanel(Panel::Bags)),
            (standard::LB, PadAction::NextNpc),
            (standard::START, PadAction::Talk),
            (standard::L3, PadAction::TogglePanel(Panel::Character)),
            (standard::RT, PadAction::TogglePanel(Panel::QuestLog)),
        ] {
            assert_eq!(controls.sample(&[pressed(button)]), [action], "{button}");
            controls.sample(&[pad()]);
        }
        // B does nothing in plain gameplay (no leaving the native world).
        assert!(controls.sample(&[pressed(standard::B)]).is_empty());
    }

    #[test]
    fn b_cancels_a_cast_and_panels_claim_b_the_dpad_and_a() {
        let mut controls = PadControls::new();
        controls.set_casting(true);
        assert_eq!(
            controls.sample(&[pressed(standard::B)]),
            [PadAction::CancelCast]
        );
        controls.sample(&[pad()]);
        controls.set_panels(true);
        for (button, action) in [
            (standard::B, PadAction::ClosePanel),
            (standard::A, PadAction::Activate),
            (standard::DPAD_UP, PadAction::FocusPrevious),
            (standard::DPAD_LEFT, PadAction::FocusPrevious),
            (standard::DPAD_DOWN, PadAction::FocusNext),
            (standard::DPAD_RIGHT, PadAction::FocusNext),
        ] {
            assert_eq!(controls.sample(&[pressed(button)]), [action], "{button}");
            controls.sample(&[pad()]);
        }
        // Movement and gameplay buttons still fall through the panels layer.
        controls.sample(&[left_stick(0.0, -1.0)]);
        assert_eq!(controls.movement().forward, 1);
        assert_eq!(
            controls.sample(&[left_stick(0.0, -1.0), pressed(standard::RB)]),
            [PadAction::TargetNext]
        );
    }

    #[test]
    fn settings_blocks_gameplay_and_held_inputs_wait_for_release_after_close() {
        let mut controls = PadControls::new();
        let mut held = left_stick(0.0, -1.0);
        held.buttons[usize::from(standard::RB)] = 1.0;
        assert_eq!(controls.sample(&[held]), [PadAction::TargetNext]);
        controls.set_input_context(true, true, false);
        assert!(controls.sample(&[held]).is_empty());
        assert_eq!(controls.movement().forward, 0);
        controls.set_input_context(false, false, false);
        assert!(controls.sample(&[held]).is_empty());
        assert_eq!(controls.movement().forward, 0);
        controls.sample(&[pad()]);
        assert_eq!(controls.sample(&[held]), [PadAction::TargetNext]);
        assert_eq!(controls.movement().forward, 1);
    }

    #[test]
    fn retiring_releases_held_movement_and_latches_held_buttons() {
        let mut controls = PadControls::new();
        let mut held = left_stick(0.0, -1.0);
        held.buttons[usize::from(standard::SELECT)] = 1.0;
        assert_eq!(
            controls.sample(&[held]),
            [PadAction::TogglePanel(Panel::Bags)]
        );
        // Opening the bags changes the context: movement resumes from the
        // still-deflected stick, the held Select does not toggle again.
        controls.set_panels(true);
        assert_eq!(controls.movement(), PadMovement::default());
        assert!(controls.sample(&[held]).is_empty());
        assert_eq!(controls.movement().forward, 1);
        // A disconnect lets everything go; nothing is sampled afterwards.
        controls.retire("gamepadDisconnected");
        assert_eq!(controls.movement(), PadMovement::default());
        assert!(controls.sample(&[]).is_empty());
        assert_eq!(controls.movement(), PadMovement::default());
    }

    #[test]
    fn the_left_trigger_holds_the_guard_and_the_right_stick_aims_it() {
        let mut controls = PadControls::new();
        let mut held = pressed(standard::LT);
        assert!(controls.sample(&[held]).is_empty());
        assert!(controls.guarding());
        assert_eq!(guard_lane(&[held]), mmorpg_core::StrikeHeight::Middle);
        held.axes[standard::RIGHT_Y] = -0.8;
        assert_eq!(guard_lane(&[held]), mmorpg_core::StrikeHeight::High);
        held.axes[standard::RIGHT_Y] = 0.6;
        assert_eq!(guard_lane(&[held]), mmorpg_core::StrikeHeight::Low);
        controls.sample(&[pad()]);
        assert!(!controls.guarding());
        // Focus loss lowers a still-held guard.
        controls.sample(&[pressed(standard::LT)]);
        controls.retire("focusLost");
        assert!(!controls.guarding());
    }

    #[test]
    fn the_right_stick_orbits_the_camera_with_an_analog_rate() {
        let mut state = pad();
        assert_eq!(camera_orbit(&[state], 0.016), [0.0, 0.0]);
        state.axes[standard::RIGHT_X] = 0.15;
        assert_eq!(camera_orbit(&[state], 0.016), [0.0, 0.0]);
        state.axes[standard::RIGHT_X] = 1.0;
        let full = camera_orbit(&[state], 0.1);
        assert!((full[0] - 42.0).abs() < 1e-3 && full[1] == 0.0, "{full:?}");
        state.axes[standard::RIGHT_X] = 0.6;
        let half = camera_orbit(&[state], 0.1);
        assert!((half[0] - 21.0).abs() < 1e-3, "{half:?}");
        state.axes = [0.0, 0.0, 0.0, -1.0];
        assert!(camera_orbit(&[state], 0.1)[1] < 0.0);
    }

    #[test]
    fn the_pad_focuses_and_presses_interface_buttons() {
        let ctx = egui::Context::default();
        let mut clicks = [0_u32; 2];
        let pass = |clicks: &mut [u32; 2], actions: &[PadAction]| {
            let input = egui::RawInput {
                events: actions.iter().filter_map(|a| interface_key(*a)).collect(),
                ..Default::default()
            };
            let mut output = ctx.run_pass(input, |ctx| {
                egui::Window::new("panel").title_bar(false).show(ctx, |ui| {
                    for (index, count) in clicks.iter_mut().enumerate() {
                        if ui.button(format!("button {index}")).clicked() {
                            *count += 1;
                        }
                    }
                });
            });
            output.textures_delta.clear();
        };
        pass(&mut clicks, &[]);
        // Nothing focused: A presses nothing.
        pass(&mut clicks, &[PadAction::Activate]);
        assert_eq!(clicks, [0, 0]);
        pass(&mut clicks, &[PadAction::FocusNext]);
        pass(&mut clicks, &[PadAction::FocusNext]);
        pass(&mut clicks, &[PadAction::Activate]);
        assert_eq!(clicks, [0, 1]);
        pass(&mut clicks, &[PadAction::FocusPrevious]);
        pass(&mut clicks, &[]);
        pass(&mut clicks, &[PadAction::Activate]);
        assert_eq!(clicks, [1, 1]);
        // B drops the focus, so A no longer presses.
        pass(&mut clicks, &[PadAction::ClosePanel]);
        pass(&mut clicks, &[PadAction::Activate]);
        assert_eq!(clicks, [1, 1]);
        assert_eq!(interface_key(PadAction::Jump), None);
    }

    #[test]
    fn the_pad_catalog_validates() {
        let runtime =
            InputRuntime::new(pad_action_registry(), None, InputRuntimeOptions::default());
        assert!(
            runtime.validation_report().diagnostics.is_empty(),
            "{:?}",
            runtime.validation_report().diagnostics
        );
    }
}
