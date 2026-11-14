//! Acceptance for #72: native action-bar slot 1 uses the slot's projected
//! ability, and the shared HUD model (`mmorpg-client-model`, #543) shows the
//! spent rage and the global cooldown from the next received projection.
use mmorpg_client::controls::{ControlAction, NativeControls};
use mmorpg_client_model::hud::{CombatHud, CooldownKind};
use mmorpg_core::{EntityRef, ZoneCommand, ZoneId, ZoneSimulation, ZoneSnapshot, greyhaven_vale};
use std::sync::Arc;
use winit::keyboard::KeyCode;

/// What a client receives for `snapshot`: one trip through the wire codec.
fn received(snapshot: &ZoneSnapshot) -> ZoneSnapshot {
    let payload = mmorpg_protocol::encode_snapshot(snapshot).expect("fixture is encodable");
    mmorpg_protocol::decode_snapshot(&payload).expect("fixture decodes")
}

/// The second row (#294): Shift+Digit binds slots 11–20, and Digit alone stays on the first row.
#[test]
fn shift_number_keys_use_the_second_action_bar_row() {
    let shift = input_bindings_core::Modifiers {
        shift: true,
        ..Default::default()
    };
    let mut controls = NativeControls::new();
    controls.set_weapon_drawn(true);
    assert_eq!(
        controls.key_down_with_modifiers(KeyCode::Digit1, false, shift.clone()),
        [ControlAction::AbilitySlot(11)]
    );
    controls.key_up(KeyCode::Digit1);
    assert_eq!(
        controls.key_down_with_modifiers(KeyCode::Digit0, false, shift),
        [ControlAction::AbilitySlot(20)]
    );
    controls.key_up(KeyCode::Digit0);
    assert_eq!(
        controls.key_down(KeyCode::Digit1, false),
        [ControlAction::AbilitySlot(1)],
        "without Shift the digit stays on the first row"
    );
    controls.key_up(KeyCode::Digit1);
    assert_eq!(
        controls.key_down(KeyCode::ShiftLeft, false),
        [],
        "Shift itself no longer rolls"
    );
    assert_eq!(
        controls.key_down(KeyCode::KeyF, false),
        [ControlAction::Roll]
    );
}

#[test]
fn native_warden_slot_one_spends_rage_and_projects_global_cooldown() {
    let content = greyhaven_vale::content();
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), Arc::clone(&content)).unwrap();
    zone.add_player(1).unwrap();
    zone.apply_command(1, 1, ZoneCommand::ChooseClass { class: 0, sex: 0 })
        .unwrap();
    zone.advance_tick().unwrap();
    // A nearby enemy and earned rage are the initial authoritative fixture.
    let mut state = zone.snapshot().unwrap();
    state.players[0].position = mmorpg_core::physics::from_units([0, 90, -2_000]);
    state.players[0].combat.abilities.resource = 40;
    state.creatures[0].position = mmorpg_core::physics::from_units([100, 45, -2_000]);
    // Facing the creature to the east: Heroic Strike lands where the Warden faces (#517).
    state.players[0].facing = 16_384;
    let target = EntityRef::Creature(state.creatures[0].creature_id);
    let mut zone = ZoneSimulation::from_snapshot(state, content).unwrap();
    zone.apply_command(1, 2, ZoneCommand::SelectTarget(Some(target)))
        .unwrap();
    zone.advance_tick().unwrap();
    let before = received(&zone.snapshot_for_player(1).unwrap());
    let mut controls = NativeControls::new();
    // Hands-free exploration keeps the action bar quiet until the weapon is drawn (#802).
    assert_eq!(controls.key_down(KeyCode::Digit1, false), []);
    controls.key_up(KeyCode::Digit1);
    controls.set_weapon_drawn(true);
    let actions = controls.key_down(KeyCode::Digit1, false);
    assert_eq!(actions, [ControlAction::AbilitySlot(1)]);
    let ControlAction::AbilitySlot(slot) = actions[0] else {
        unreachable!()
    };
    let ability = CombatHud::from_projection(&before).slots[usize::from(slot - 1)].ability;
    assert_eq!(ability, 1);
    zone.apply_command(
        1,
        3,
        ZoneCommand::UseAbility {
            ability,
            target: None,
        },
    )
    .unwrap();
    zone.advance_tick().unwrap();
    let after = received(&zone.snapshot_for_player(1).unwrap());
    assert!(after.viewer.resource.unwrap().value < before.viewer.resource.unwrap().value);
    let hud = CombatHud::from_projection(&after);
    assert_eq!(
        hud.slots
            .iter()
            .map(|slot| slot.ability)
            .collect::<Vec<_>>(),
        [1, 2, 3, 4, 17, 51]
    );
    assert!(
        hud.slots[0]
            .cooldown
            .is_some_and(|cooldown| cooldown.kind == CooldownKind::Global)
    );
}
