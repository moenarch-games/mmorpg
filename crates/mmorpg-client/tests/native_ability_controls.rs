//! Acceptance for #72: native ability input through the shared
//! `input-bindings` runtime.
//!
//! The native client resolves keys to the same MMORPG-owned semantic actions
//! as the browser (`web/src/input/game-controls.ts`): `ability.slot1`–`4` on
//! Digit1–Digit4 in the `gameplay` context and `combat.cancelCast` on Escape in
//! the `casting` context, which is pushed only while the received projection
//! shows the viewer casting (`viewer.cast.is_some()`) and does not block the
//! layers below it. Binding resolution, repeat suppression and held-state
//! release are `input-bindings-core`'s `InputRuntime` and
//! `SemanticControlState`; the client only translates winit keys to physical
//! strokes and maps dispatched presses to ability slots. What a slot use does
//! (ability id, target, validity) stays with the session and the zone.
//!
//! Required seam, module `mmorpg_client::controls` (built on
//! `input_bindings_core`, a `[dependencies]` git dependency pinned by `rev`):
//!
//! ```text
//! #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
//! pub enum ControlAction { AbilitySlot(u8) /* 1..=4 */, CancelCast }
//! impl ControlAction { pub fn id(self) -> &'static str }
//!     // "ability.slot1".."ability.slot4", "combat.cancelCast"
//!
//! pub fn native_action_registry() -> input_bindings_core::ActionRegistry;
//!     // the MMORPG-owned native catalog: every ControlAction, repeat policy
//!     // Never, keyboard defaults (physical key codes) with the browser's
//!     // contexts; it validates without diagnostics
//!
//! pub struct NativeControls { /* InputRuntime + SemanticControlState */ }
//! impl NativeControls {
//!     pub fn new() -> Self;                 // panics on an invalid catalog
//!     pub fn set_casting(&mut self, casting: bool);
//!         // pushes/pops the `casting` context; idempotent for an unchanged value
//!     pub fn key_down(&mut self, key: winit::keyboard::KeyCode, repeat: bool)
//!         -> Vec<ControlAction>;            // presses dispatched by this event, in order
//!     pub fn key_up(&mut self, key: winit::keyboard::KeyCode);
//!     pub fn focus_lost(&mut self);         // InputRuntime::reset: releases everything
//!     pub fn is_held(&self, action: ControlAction) -> bool;
//! }
//! ```
//!
//! A key down/up pair is one physical press; `repeat` is winit's
//! `KeyEvent::repeat` (OS auto-repeat). Releases and resets never use an
//! ability, so `key_up` and `focus_lost` emit nothing by construction.
//! Escape outside a cast resolves to nothing here, so the desktop shell keeps
//! closing the window on it.

use input_bindings_core::{
    ActionRegistry, InputStroke, KeyMatch, Modifiers, RepeatPolicy, WhenExpr, validate_registry,
};
use mmorpg_client::controls::{ControlAction, NativeControls, native_action_registry};
use winit::keyboard::KeyCode;

const BROWSER_CONTROLS: &str = include_str!("../../../web/src/input/game-controls.ts");

/// No control action.
const NONE: [ControlAction; 0] = [];

const SLOT_KEYS: [(KeyCode, u8); 4] = [
    (KeyCode::Digit1, 1),
    (KeyCode::Digit2, 2),
    (KeyCode::Digit3, 3),
    (KeyCode::Digit4, 4),
];

fn press(controls: &mut NativeControls, key: KeyCode) -> Vec<ControlAction> {
    let actions = controls.key_down(key, false);
    controls.key_up(key);
    actions
}

/// The browser's default for `id`: its context and physical key codes, read
/// from the `action("<id>", "<title>", "<context>", [key("…"), …])` line.
fn browser_default(id: &str) -> (String, Vec<String>) {
    let needle = format!("action(\"{id}\", ");
    let line = BROWSER_CONTROLS
        .lines()
        .find(|line| line.contains(&needle))
        .unwrap_or_else(|| panic!("the browser catalog defines {id}"));
    let quoted: Vec<&str> = line.split('"').skip(1).step_by(2).collect();
    // id, title, context, then the key codes.
    assert_eq!(quoted[0], id);
    let context = quoted[2].to_owned();
    let mut keys: Vec<String> = line
        .split("key(\"")
        .skip(1)
        .map(|rest| rest.split('"').next().unwrap().to_owned())
        .collect();
    keys.sort();
    (context, keys)
}

/// The native catalog's default for `id`: its contexts and physical key codes.
fn native_default(registry: &ActionRegistry, id: &str) -> (Vec<String>, Vec<String>) {
    let action = registry
        .actions
        .iter()
        .find(|action| action.id == id)
        .unwrap_or_else(|| panic!("the native catalog defines {id}"));
    assert_eq!(
        action.repeat_policy,
        RepeatPolicy::Never,
        "{id} never repeats"
    );
    let mut contexts = Vec::new();
    let mut keys = Vec::new();
    for binding in &action.defaults {
        assert_eq!(binding.action, id);
        let [InputStroke::Keyboard(stroke)] = binding.sequence.as_slice() else {
            continue;
        };
        let KeyMatch::Physical { value } = &stroke.key else {
            panic!("{id}: native keys bind physical codes, got {stroke:?}");
        };
        assert_eq!(stroke.modifiers, Modifiers::default(), "{id}: no modifiers");
        let WhenExpr::Context { id: context } = &binding.when else {
            panic!("{id}: bound in one context, got {:?}", binding.when);
        };
        contexts.push(context.clone());
        keys.push(value.clone());
    }
    contexts.sort();
    contexts.dedup();
    keys.sort();
    (contexts, keys)
}

#[test]
fn action_ids_are_the_shared_semantic_ids() {
    for (_, slot) in SLOT_KEYS {
        assert_eq!(
            ControlAction::AbilitySlot(slot).id(),
            format!("ability.slot{slot}")
        );
    }
    assert_eq!(ControlAction::CancelCast.id(), "combat.cancelCast");
}

#[test]
fn native_defaults_match_the_browser_catalog() {
    let registry = native_action_registry();
    let report = validate_registry(&registry, None);
    assert!(
        report.valid && report.diagnostics.is_empty(),
        "the native catalog validates: {:?}",
        report.diagnostics
    );
    let ids: Vec<&str> = SLOT_KEYS
        .iter()
        .map(|&(_, slot)| ControlAction::AbilitySlot(slot).id())
        .chain([ControlAction::CancelCast.id()])
        .collect();
    for id in ids {
        let (browser_context, browser_keys) = browser_default(id);
        let (native_contexts, native_keys) = native_default(&registry, id);
        assert_eq!(native_contexts, [browser_context], "{id}: same context");
        assert_eq!(native_keys, browser_keys, "{id}: same default keys");
    }
    // Pin the expectation itself, so a browser edit cannot silently move both.
    assert_eq!(
        browser_default("ability.slot1"),
        ("gameplay".to_owned(), vec!["Digit1".to_owned()])
    );
    assert_eq!(
        browser_default("combat.cancelCast"),
        ("casting".to_owned(), vec!["Escape".to_owned()])
    );
}

#[test]
fn digits_one_to_four_use_slots_one_to_four() {
    let mut controls = NativeControls::new();
    for (key, slot) in SLOT_KEYS {
        assert_eq!(
            press(&mut controls, key),
            [ControlAction::AbilitySlot(slot)],
            "{key:?}"
        );
    }
    // Two keys held together each use their own slot once.
    assert_eq!(
        controls.key_down(KeyCode::Digit2, false),
        [ControlAction::AbilitySlot(2)]
    );
    assert_eq!(
        controls.key_down(KeyCode::Digit3, false),
        [ControlAction::AbilitySlot(3)]
    );
    controls.key_up(KeyCode::Digit2);
    controls.key_up(KeyCode::Digit3);
}

#[test]
fn a_held_slot_key_uses_its_ability_once_per_press() {
    let mut controls = NativeControls::new();
    assert_eq!(
        controls.key_down(KeyCode::Digit1, false),
        [ControlAction::AbilitySlot(1)]
    );
    assert!(controls.is_held(ControlAction::AbilitySlot(1)));
    for _ in 0..5 {
        assert_eq!(
            controls.key_down(KeyCode::Digit1, true),
            NONE,
            "OS key repeat never uses the ability again"
        );
    }
    assert!(
        controls.is_held(ControlAction::AbilitySlot(1)),
        "repeat keeps it held"
    );
    controls.key_up(KeyCode::Digit1);
    assert!(!controls.is_held(ControlAction::AbilitySlot(1)));
    assert_eq!(
        controls.key_down(KeyCode::Digit1, false),
        [ControlAction::AbilitySlot(1)],
        "a fresh press uses it again"
    );
    controls.key_up(KeyCode::Digit1);
}

#[test]
fn focus_loss_releases_held_slots_and_uses_nothing() {
    let mut controls = NativeControls::new();
    assert_eq!(
        controls.key_down(KeyCode::Digit1, false),
        [ControlAction::AbilitySlot(1)]
    );
    assert_eq!(
        controls.key_down(KeyCode::Digit4, false),
        [ControlAction::AbilitySlot(4)]
    );
    controls.focus_lost();
    for (_, slot) in SLOT_KEYS {
        assert!(!controls.is_held(ControlAction::AbilitySlot(slot)));
    }
    // The keys may still be physically down: their repeats and releases after
    // the focus loss belong to no press and use nothing.
    assert_eq!(controls.key_down(KeyCode::Digit1, true), NONE);
    controls.key_up(KeyCode::Digit1);
    controls.key_up(KeyCode::Digit4);
    assert!(!controls.is_held(ControlAction::AbilitySlot(1)));
    // Back in focus, a new press works.
    assert_eq!(
        press(&mut controls, KeyCode::Digit4),
        [ControlAction::AbilitySlot(4)]
    );
}

#[test]
fn unbound_keys_use_nothing() {
    let mut controls = NativeControls::new();
    for key in [
        KeyCode::Digit0,
        KeyCode::Digit5,
        KeyCode::Digit9,
        KeyCode::Numpad1,
        KeyCode::KeyZ,
        KeyCode::Backquote,
        KeyCode::F12,
    ] {
        assert_eq!(controls.key_down(key, false), NONE, "{key:?}");
        assert_eq!(controls.key_down(key, true), NONE, "{key:?} repeat");
        controls.key_up(key);
    }
    // Escape outside a cast is not a control action; the shell closes on it.
    assert_eq!(press(&mut controls, KeyCode::Escape), NONE);
    assert!(!controls.is_held(ControlAction::CancelCast));
}

#[test]
fn escape_cancels_a_cast_once_per_press() {
    let mut controls = NativeControls::new();
    controls.set_casting(true);
    assert_eq!(
        controls.key_down(KeyCode::Escape, false),
        [ControlAction::CancelCast]
    );
    assert_eq!(controls.key_down(KeyCode::Escape, true), NONE, "no repeat");
    controls.key_up(KeyCode::Escape);
    assert_eq!(
        press(&mut controls, KeyCode::Escape),
        [ControlAction::CancelCast]
    );
    // Setting the same state again changes nothing.
    controls.set_casting(true);
    assert_eq!(
        press(&mut controls, KeyCode::Escape),
        [ControlAction::CancelCast]
    );
    // The casting layer does not block gameplay: slots still work mid-cast.
    assert_eq!(
        press(&mut controls, KeyCode::Digit2),
        [ControlAction::AbilitySlot(2)]
    );
    // Once the projection shows no cast, Escape cancels nothing.
    controls.set_casting(false);
    assert_eq!(press(&mut controls, KeyCode::Escape), NONE);
}

#[test]
fn a_cast_starting_under_a_held_key_does_not_repeat_it() {
    let mut controls = NativeControls::new();
    assert_eq!(
        controls.key_down(KeyCode::Digit1, false),
        [ControlAction::AbilitySlot(1)]
    );
    // The cast that press started arrives in the next projection.
    controls.set_casting(true);
    assert_eq!(controls.key_down(KeyCode::Digit1, true), NONE);
    controls.key_up(KeyCode::Digit1);
    // A held Escape that cancelled the cast does not cancel again after it ends.
    assert_eq!(
        controls.key_down(KeyCode::Escape, false),
        [ControlAction::CancelCast]
    );
    controls.set_casting(false);
    assert_eq!(controls.key_down(KeyCode::Escape, true), NONE);
    controls.key_up(KeyCode::Escape);
    assert!(!controls.is_held(ControlAction::CancelCast));
}
