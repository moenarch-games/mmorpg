//! Gather to smith (#917): on the real Greyhaven content a Warden skins two boars, gathers two
//! ridge veins and has the Outpost smith make the Hollowpine boar-spear, which takes exactly its
//! materials and equips. Gathers, skins and the craft never repeat, through replay and canonical
//! recovery, and spent veins regrow on their own deterministic schedule.
use std::sync::Arc;

use mmorpg_core::greyhaven_vale::ore::ORE_PER_VEIN;
use mmorpg_core::{
    BOAR_HIDE, BOAR_TUSK, ClassChoice, CreatureId, CreatureLife, EquipmentSlot,
    HOLLOWPINE_BOAR_SPEAR, IRON_ORE, ItemId, ItemQuality, ItemTrait, LootClaim, NpcId,
    PLAYER_HALF_EXTENTS_UNITS, PlayerClass, Sex, StrikeVariant, WeaponFamily, ZoneCommand,
    ZoneEvent, ZoneId, ZoneSimulation, greyhaven_vale, item_find, shaped_strike, smith_stock,
};

const WARDEN: ClassChoice = ClassChoice {
    class: PlayerClass::Warden,
    sex: Sex::Male,
};
const SMITH: NpcId = NpcId::new(43);
/// Two of the Wolfrun Woods' young boars.
const BOARS: [CreatureId; 2] = [CreatureId::new(120), CreatureId::new(121)];
/// Where the player stands at each of the first two veins: 2.5 m from its centre.
const AT_VEINS: [[i32; 2]; 2] = [[-3_150, -4_050], [-4_700, -3_500]];
/// In the smithy's back yard, 2.5 m in front of the smith.
const AT_SMITH: [i32; 2] = [2_300, 0];

fn restore(zone: &ZoneSimulation) -> ZoneSimulation {
    ZoneSimulation::from_snapshot(zone.snapshot().unwrap(), Arc::clone(zone.content())).unwrap()
}

/// `zone` restored with player 1 standing on the ground at `at`.
fn moved(zone: &ZoneSimulation, at: [i32; 2]) -> ZoneSimulation {
    let mut state = zone.snapshot().unwrap();
    let y = zone.definition().ground_at(at[0], at[1]) + PLAYER_HALF_EXTENTS_UNITS[1];
    state.players[0].position = mmorpg_core::physics::from_units([at[0], y, at[1]]);
    state.players[0].velocity = [0.0; 3];
    ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap()
}

/// Sends `command` and runs its tick; the errors player 1 received.
fn act(zone: &mut ZoneSimulation, command: ZoneCommand) -> Vec<mmorpg_core::ErrorCode> {
    let sequence = zone.snapshot().unwrap().players[0].last_sequence + 1;
    zone.apply_command(1, sequence, command).unwrap();
    zone.advance_tick().unwrap();
    zone.snapshot_for_player(1)
        .unwrap()
        .events
        .into_iter()
        .filter_map(|event| match event {
            ZoneEvent::Error { code, .. } => Some(code),
            _ => None,
        })
        .collect()
}

fn count(zone: &ZoneSimulation, item: ItemId) -> u32 {
    zone.snapshot().unwrap().players[0].inventory.count(item)
}

/// A Warden with copper for the smith's fee, and both boars lying dead at its feet, tapped by it.
fn hunted() -> ZoneSimulation {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), greyhaven_vale::content()).unwrap();
    zone.create_character(1, WARDEN).unwrap();
    zone.advance_tick().unwrap();
    let zone = moved(&zone, [-9_800, 700]);
    let mut state = zone.snapshot().unwrap();
    state.players[0].copper = 100;
    let feet = state.players[0].position.map(|axis| axis.round());
    for boar in BOARS {
        let creature = state
            .creatures
            .iter_mut()
            .find(|creature| creature.creature_id == boar)
            .unwrap();
        creature.life = CreatureLife::Corpse {
            died_at: state.tick,
        };
        creature.health = 0;
        creature.position = [feet[0] + 100.0, feet[1] - 50.0, feet[2]];
        creature.velocity = [0.0; 3];
        creature.ai = mmorpg_core::CreatureAi::Idle {
            timer: 0,
            destination: None,
        };
        creature.threat.clear();
        creature.tapped_by = Some(1);
    }
    ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap()
}

#[test]
fn gathered_materials_make_one_boar_spear_that_equips_and_never_duplicates() {
    // The recipe: two veins' ore and two boars' hides and tusks, for a 20 copper fee.
    let recipe = smith_stock().offer(0).unwrap();
    assert_eq!(recipe.item, HOLLOWPINE_BOAR_SPEAR);
    assert_eq!(
        recipe.materials,
        [(IRON_ORE, 2 * ORE_PER_VEIN), (BOAR_HIDE, 2), (BOAR_TUSK, 2)]
    );

    // Skinning: each boar once, by its tapper; a second skin of the same corpse is empty.
    let mut zone = hunted();
    let tick = zone.snapshot().unwrap().tick;
    let claims = BOARS.map(|creature| LootClaim {
        creature,
        died_at: tick,
    });
    let start = restore(&zone);
    for claim in claims {
        assert_eq!(act(&mut zone, ZoneCommand::Skin(claim)), []);
    }
    assert_eq!(
        act(&mut zone, ZoneCommand::Skin(claims[0])),
        [mmorpg_core::ErrorCode::EmptyLoot]
    );
    // Recovered, the skinned corpses stay skinned.
    let mut recovered = restore(&zone);
    assert_eq!(
        act(&mut recovered, ZoneCommand::Skin(claims[1])),
        [mmorpg_core::ErrorCode::EmptyLoot]
    );
    assert_eq!((count(&zone, BOAR_HIDE), count(&zone, BOAR_TUSK)), (2, 2));

    // Gathering: a full vein yields its ore and is spent for everyone; another vein is full.
    let mut zone = moved(&zone, AT_VEINS[0]);
    assert_eq!(act(&mut zone, ZoneCommand::Gather { node: 0 }), []);
    assert_eq!(
        act(&mut zone, ZoneCommand::Gather { node: 0 }),
        [mmorpg_core::ErrorCode::EmptyLoot]
    );
    assert_eq!(zone.depleted_veins(), [0]);
    let spent_at = zone.snapshot().unwrap().tick;
    assert_eq!(
        act(&mut zone, ZoneCommand::Gather { node: 1 }),
        [mmorpg_core::ErrorCode::OutOfRange]
    );
    let mut zone = moved(&zone, AT_VEINS[1]);
    assert_eq!(act(&mut zone, ZoneCommand::Gather { node: 2 }), []);
    assert_eq!(zone.depleted_veins(), [0, 2]);
    assert_eq!(count(&zone, IRON_ORE), u32::from(2 * ORE_PER_VEIN));

    // The smith makes one spear from exactly the recipe; a second order lacks the materials.
    let mut zone = moved(&zone, AT_SMITH);
    let copper = zone.snapshot().unwrap().players[0].copper;
    let buy = ZoneCommand::BuyItem {
        npc: SMITH,
        offer: 0,
        quantity: 1,
    };
    let before_craft = restore(&zone);
    assert_eq!(act(&mut zone, buy), []);
    let crafted = zone.snapshot().unwrap();
    assert_eq!(count(&zone, HOLLOWPINE_BOAR_SPEAR), 1);
    for material in [IRON_ORE, BOAR_HIDE, BOAR_TUSK] {
        assert_eq!(count(&zone, material), 0);
    }
    assert_eq!(crafted.players[0].copper, copper - recipe.price);
    assert_eq!(
        act(&mut zone, buy),
        [mmorpg_core::ErrorCode::MissingMaterials]
    );
    assert_eq!(count(&zone, HOLLOWPINE_BOAR_SPEAR), 1);

    // Replayed from before the order, the craft settles to the same bytes.
    let mut replay = before_craft;
    assert_eq!(act(&mut replay, buy), []);
    assert_eq!(replay.snapshot().unwrap(), crafted);

    // It equips into the main hand and thrusts back quicker than the militia spear.
    let slot = crafted.players[0]
        .inventory
        .slots()
        .iter()
        .position(|stack| stack.is_some_and(|stack| stack.item() == HOLLOWPINE_BOAR_SPEAR))
        .unwrap();
    assert_eq!(
        act(
            &mut zone,
            ZoneCommand::EquipItem {
                bag_slot: u8::try_from(slot).unwrap()
            }
        ),
        []
    );
    let equipped = zone.snapshot().unwrap().players[0].equipment;
    assert_eq!(
        equipped.item(EquipmentSlot::MainHand),
        Some(HOLLOWPINE_BOAR_SPEAR)
    );
    let find = item_find(HOLLOWPINE_BOAR_SPEAR).unwrap();
    assert_eq!(find.quality, ItemQuality::Crafted);
    assert_eq!(find.trait_, Some(ItemTrait::Recovery { percent: 15 }));
    let [base, crafted_strike] = [None, Some(HOLLOWPINE_BOAR_SPEAR)]
        .map(|item| shaped_strike(WeaponFamily::Spear, StrikeVariant::First, item));
    assert!(crafted_strike.interval_ticks < base.interval_ticks);
    // The dropped route stays: the long-hafted find rolls from the militia spear.
    assert_eq!(
        item_find(ItemId::new(26)).unwrap().base,
        Some(ItemId::new(23))
    );

    // Canonical recovery keeps the spear, the spent veins and their regrowth; both regrow on
    // the same tick in the running zone and in its recovery.
    let mut recovered = restore(&zone);
    assert_eq!(recovered.snapshot().unwrap(), zone.snapshot().unwrap());
    // Both veins were spent within a few ticks of `spent_at`.
    let regrow = spent_at + 120 * u64::from(mmorpg_core::TICK_HZ) + 10;
    while zone.snapshot().unwrap().tick < regrow {
        zone.advance_tick().unwrap();
        recovered.advance_tick().unwrap();
    }
    assert_eq!(recovered.snapshot().unwrap(), zone.snapshot().unwrap());
    assert!(zone.depleted_veins().is_empty());
    assert_eq!(
        count(&zone, HOLLOWPINE_BOAR_SPEAR),
        0,
        "it is worn, not in the bag"
    );

    // The whole loop replays from the hunted start to the same skins.
    let mut replay = start;
    for claim in claims {
        assert_eq!(act(&mut replay, ZoneCommand::Skin(claim)), []);
    }
    assert_eq!(
        (count(&replay, BOAR_HIDE), count(&replay, BOAR_TUSK)),
        (2, 2)
    );
}
