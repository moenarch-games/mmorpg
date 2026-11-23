//! Weapon-family movesets (#914): swapping a Warden's sword for the Militia Spear changes what
//! its basic strike reaches, how wide it sweeps and when it lands, deterministically; and a
//! class may only equip the families it wields.
mod support;

use std::sync::Arc;

use mmorpg_core::{
    CreatureBehaviour, CreatureId, CreatureTemplate, EntityRef, ErrorCode, FamilyMelee,
    GreatWeapon, INVENTORY_SLOTS, Inventory, ItemId, ItemStack, WeaponFamily, ZoneAreas,
    ZoneCommand, ZoneContent, ZoneEvent, ZoneId, ZoneSimulation,
};
use support::arena::{self, GRAVEYARD, WOLF};

const SOUTH: u16 = 32_768;
const MILITIA_SPEAR: ItemId = ItemId::new(23);
/// The empty bag slot the spear is put in.
const SPEAR_SLOT: u8 = 2;
const CREATURE: EntityRef = EntityRef::Creature(CreatureId::new(1));

/// A sturdy neutral creature at `position` (the player spawns at the origin).
fn content(position: [i32; 2]) -> Arc<ZoneContent> {
    let template = CreatureTemplate {
        behaviour: CreatureBehaviour::Neutral,
        ..arena::wolf(50_000, [1, 1])
    };
    Arc::new(
        ZoneContent::new(
            arena::ground(),
            ZoneAreas::default(),
            vec![template],
            vec![arena::spawn(1, WOLF, position)],
            Vec::new(),
            GRAVEYARD,
        )
        .unwrap()
        .with_creature_abilities(Default::default())
        .unwrap(),
    )
}

/// What one standing strike did: its family, its wind-up, the recovery it started and
/// whether its contact covered the creature.
#[derive(Debug, Eq, PartialEq)]
struct Outcome {
    family: WeaponFamily,
    contact: u16,
    recovery: u16,
    reached: bool,
}

/// A player of `class` facing south with the Militia Spear in its bag, equipping it first when
/// `spear`; the refusal the equip met, if any, and one standing strike's outcome.
fn strike(class: u8, creature: [i32; 2], spear: bool) -> (Option<ErrorCode>, Outcome) {
    strike_with(class, creature, spear.then_some(MILITIA_SPEAR))
}

/// [`strike`] with `weapon` put in the bag and equipped first, if any.
fn strike_with(
    class: u8,
    creature: [i32; 2],
    weapon: Option<ItemId>,
) -> (Option<ErrorCode>, Outcome) {
    let content = content(creature);
    let mut zone = ZoneSimulation::with_content(ZoneId::new(9), Arc::clone(&content)).unwrap();
    zone.add_player(1).unwrap();
    let mut sequence = 0;
    let mut send = |zone: &mut ZoneSimulation, command| {
        sequence += 1;
        zone.apply_command(1, sequence, command).unwrap();
    };
    send(&mut zone, ZoneCommand::ChooseClass { class, sex: 0 });
    send(&mut zone, arena::stand(SOUTH));
    zone.advance_tick().unwrap();
    let mut state = zone.snapshot().unwrap();
    let mut slots = *state.players[0].inventory.slots();
    assert_eq!(slots[usize::from(SPEAR_SLOT)], None);
    slots[usize::from(SPEAR_SLOT)] =
        Some(ItemStack::new(weapon.unwrap_or(MILITIA_SPEAR), 1).unwrap());
    assert_eq!(slots.len(), INVENTORY_SLOTS);
    state.players[0].inventory = Inventory::from_slots(slots);
    let mut zone = ZoneSimulation::from_snapshot(state, content).unwrap();
    let mut refusal = None;
    if weapon.is_some() {
        send(
            &mut zone,
            ZoneCommand::EquipItem {
                bag_slot: SPEAR_SLOT,
            },
        );
        zone.advance_tick().unwrap();
        refusal =
            zone.snapshot_for_player(1)
                .unwrap()
                .events
                .iter()
                .find_map(|event| match *event {
                    ZoneEvent::Error { code, .. } => Some(code),
                    _ => None,
                });
    }
    send(&mut zone, ZoneCommand::Swing { facing: SOUTH });
    let mut began = None;
    for _ in 0..120 {
        zone.advance_tick().unwrap();
        for event in zone.snapshot_for_player(1).unwrap().events {
            match event {
                ZoneEvent::StrikeBegan {
                    family, contact, ..
                } => began = Some((family, contact)),
                ZoneEvent::AttackPerformed {
                    source: EntityRef::Player(1),
                    target,
                } => {
                    let (family, contact) = began.expect("the strike began before its contact");
                    let recovery = FamilyMelee::of(family).chain[0].interval_ticks;
                    return (
                        refusal,
                        Outcome {
                            family,
                            contact,
                            recovery,
                            reached: target == Some(CREATURE),
                        },
                    );
                }
                _ => {}
            }
        }
    }
    panic!("the strike never reached its contact");
}

#[test]
fn a_warden_swapping_sword_for_spear_changes_reach_arc_and_timing() {
    // 3 m straight ahead: beyond the sword's 2.55 m, inside the spear's 3.3 m thrust.
    let ahead = [0, -300];
    // 2 m away, 40° off the facing: inside the sword's 50° arc, outside the spear's 30°.
    let aside = [129, -153];
    let (refusal, sword_far) = strike(0, ahead, false);
    assert_eq!(refusal, None);
    let (refusal, spear_far) = strike(0, ahead, true);
    assert_eq!(refusal, None, "the Warden wields spears");
    assert_eq!(sword_far.family, WeaponFamily::Sword);
    assert_eq!(spear_far.family, WeaponFamily::Spear);
    assert!(!sword_far.reached && spear_far.reached, "reach");

    let (_, sword_aside) = strike(0, aside, false);
    let (_, spear_aside) = strike(0, aside, true);
    assert!(sword_aside.reached && !spear_aside.reached, "arc");

    // The spear's thrust winds up longer and commits to a longer recovery.
    assert!(
        spear_far.contact > sword_far.contact,
        "{spear_far:?} {sword_far:?}"
    );
    assert!(spear_far.recovery > sword_far.recovery);

    // Replaying the same inputs gives the same outcomes.
    assert_eq!(strike(0, ahead, true), (None, spear_far));
    assert_eq!(strike(0, aside, false), (None, sword_aside));
}

#[test]
fn a_class_may_only_equip_the_weapon_families_it_wields() {
    // The Arcanist (2) wields no spear: the equip is refused and it keeps its staff strikes.
    let (refusal, outcome) = strike(2, [0, -300], true);
    assert_eq!(refusal, Some(ErrorCode::NotEquippable));
    assert_eq!(outcome.family, WeaponFamily::Staff);
    // The Ranger (1) does.
    let (refusal, outcome) = strike(1, [0, -300], true);
    assert_eq!(refusal, None);
    assert_eq!(outcome.family, WeaponFamily::Spear);
}

#[test]
fn a_warden_with_a_great_weapon_strikes_later_and_further_than_with_a_mace() {
    // 2.7 m straight ahead: beyond the mace's 2.45 m, inside the great weapon's 2.75 m.
    let ahead = [0, -270];
    let brute_breaker = Some(ItemId::new(12));
    for great in <GreatWeapon as strum::VariantArray>::VARIANTS {
        let (refusal, heavy) = strike_with(0, ahead, Some(great.item()));
        assert_eq!(refusal, None, "the Warden wields {great:?}");
        assert_eq!(heavy.family, WeaponFamily::TwoHanded);
        let (_, mace) = strike_with(0, ahead, brute_breaker);
        assert_eq!(mace.family, WeaponFamily::Mace);
        assert!(heavy.reached && !mace.reached, "reach");
        assert!(heavy.contact > mace.contact && heavy.recovery > mace.recovery);
    }
    // No other class wields one: the Cleric (5) keeps its mace strikes.
    let (refusal, outcome) = strike_with(5, ahead, Some(GreatWeapon::Maul.item()));
    assert_eq!(refusal, Some(ErrorCode::NotEquippable));
    assert_eq!(outcome.family, WeaponFamily::Mace);
}
