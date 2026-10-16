//! Vendors through the public zone API: atomic purchases and sales, every
//! refusal leaving bag, copper and revision unchanged, content identity and
//! canonical continuation of pending trades.
mod support;

use std::sync::Arc;

use mmorpg_core::{
    EntityRef, ErrorCode, INVENTORY_SLOTS, Inventory, ItemId, ItemStack, MAX_VENDOR_OFFERS, Npc,
    NpcId, NpcRole, VENDOR_REACH_UNITS, VendorOffer, VendorStock, VendorStockError, ZoneCommand,
    ZoneContent, ZoneEvent, ZoneId, ZoneSimulation, greyhaven_vale, sell_price,
    starter_vendor_stock,
};
use support::arena;

const FUR: ItemId = ItemId::new(1);
const DAGGER: ItemId = ItemId::new(2);
const SHORTSWORD: ItemId = ItemId::new(3);
const TROUSERS: ItemId = ItemId::new(8);
const BOOTS: ItemId = ItemId::new(9);
const VENDOR: NpcId = NpcId::new(3);
const GUARD: NpcId = NpcId::new(6);
/// The starter stock lists the Worn Dagger first (5 copper), Worn Boots
/// second (12 copper) and the Militia Shortsword seventh (25 copper).
const DAGGER_OFFER: u8 = 0;
const BOOTS_OFFER: u8 = 1;
const SHORTSWORD_OFFER: u8 = 6;

type Bag = [Option<ItemStack>; INVENTORY_SLOTS];
/// Vendor position, bag, copper, health, command, refusal and its target.
type RefusalCase = (
    [i32; 2],
    Bag,
    u32,
    Option<u32>,
    ZoneCommand,
    ErrorCode,
    Option<EntityRef>,
);

fn stack(item: ItemId, quantity: u16) -> Option<ItemStack> {
    Some(ItemStack::new(item, quantity).unwrap())
}

fn starter_bag() -> Bag {
    let mut bag = [None; INVENTORY_SLOTS];
    bag[0] = stack(FUR, 3);
    bag[1] = stack(DAGGER, 1);
    bag
}

fn npc(id: NpcId, role: NpcRole, position: [i32; 2]) -> Npc {
    Npc {
        role,
        ..arena::npc(id.get(), position)
    }
}

/// An arena with the starter vendor `vendor_at` and a guard, both bound to
/// vendor catalog 1, and one player at the origin restored with `bag`,
/// `copper` and `health` (full when `None`).
fn market(vendor_at: [i32; 2], bag: Bag, copper: u32, health: Option<u32>) -> ZoneSimulation {
    let content = ZoneContent::clone(&arena::arena(
        vec![],
        vec![],
        vec![
            npc(VENDOR, NpcRole::Vendor, vendor_at),
            npc(GUARD, NpcRole::Guard, [0, -600]),
        ],
    ))
    .with_vendors(1, vec![(VENDOR, starter_vendor_stock())])
    .unwrap();
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), Arc::new(content)).unwrap();
    zone.add_player(1).unwrap();
    let mut state = zone.snapshot().unwrap();
    let player = &mut state.players[0];
    player.inventory = Inventory::from_slots(bag);
    player.copper = copper;
    if let Some(health) = health {
        player.combat.health = health;
    }
    ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap()
}

/// Beside the vendor: 3 m west of the player.
fn near(bag: Bag, copper: u32) -> ZoneSimulation {
    market([-300, 0], bag, copper, None)
}

fn step(zone: &mut ZoneSimulation, sequence: u32, command: ZoneCommand) -> Vec<ZoneEvent> {
    zone.apply_command(1, sequence, command).unwrap();
    zone.advance_tick().unwrap();
    zone.snapshot_for_player(1).unwrap().events
}

fn buy(offer: u8, quantity: u16) -> ZoneCommand {
    ZoneCommand::BuyItem {
        npc: VENDOR,
        offer,
        quantity,
    }
}

fn sell(bag_slot: u8, quantity: u16) -> ZoneCommand {
    ZoneCommand::SellItem {
        npc: VENDOR,
        bag_slot,
        quantity,
    }
}

#[test]
fn buying_and_selling_move_items_and_copper_together_under_the_bag_revision() {
    let mut zone = near(starter_bag(), 30);
    let before = zone.snapshot().unwrap().players.remove(0);
    assert!(step(&mut zone, 1, buy(BOOTS_OFFER, 1)).is_empty());
    let state = zone.snapshot().unwrap();
    let player = &state.players[0];
    assert_eq!(player.copper, 18);
    assert_eq!(player.inventory.slots()[2], stack(BOOTS, 1));
    assert_eq!(player.inventory_revision, before.inventory_revision + 1);
    assert_eq!(player.inventory_changed_at, state.tick);
    let view = zone.snapshot_for_player(1).unwrap();
    assert_eq!(view.viewer.copper, 18);
    assert_eq!(view.inventory.as_ref(), Some(&player.inventory));

    // Selling part of a stack pays its sale value per unit.
    assert!(step(&mut zone, 2, sell(0, 2)).is_empty());
    let player = zone.snapshot().unwrap().players.remove(0);
    assert_eq!(player.copper, 18 + 2 * sell_price(FUR));
    assert_eq!(player.inventory.slots()[0], stack(FUR, 1));
    // Selling the rest empties the slot.
    assert!(step(&mut zone, 3, sell(1, 1)).is_empty());
    let player = zone.snapshot().unwrap().players.remove(0);
    assert_eq!(player.copper, 20 + sell_price(DAGGER));
    assert_eq!(player.inventory.slots()[1], None);
    assert_eq!(player.inventory_revision, before.inventory_revision + 3);

    // A multi-unit purchase of a stack-one item fills one slot per unit.
    let mut zone = near(starter_bag(), 24);
    assert!(step(&mut zone, 1, buy(BOOTS_OFFER, 2)).is_empty());
    let player = zone.snapshot().unwrap().players.remove(0);
    assert_eq!(player.copper, 0);
    assert_eq!(player.inventory.slots()[2], stack(BOOTS, 1));
    assert_eq!(player.inventory.slots()[3], stack(BOOTS, 1));
}

#[test]
fn every_refusal_leaves_bag_copper_and_revision_unchanged() {
    let mut full = [stack(FUR, 20); INVENTORY_SLOTS];
    full[0] = stack(DAGGER, 1);
    let vendor = Some(EntityRef::Npc(VENDOR));
    let cases: Vec<RefusalCase> = vec![
        (
            [-300, 0],
            starter_bag(),
            11,
            None,
            buy(BOOTS_OFFER, 1),
            ErrorCode::NotEnoughMoney,
            vendor,
        ),
        (
            [-300, 0],
            starter_bag(),
            30,
            None,
            buy(BOOTS_OFFER, 3),
            ErrorCode::NotEnoughMoney,
            vendor,
        ),
        // 12 × 65 535 = 786 420 copper is computed without overflow.
        (
            [-300, 0],
            starter_bag(),
            786_419,
            None,
            buy(BOOTS_OFFER, u16::MAX),
            ErrorCode::NotEnoughMoney,
            vendor,
        ),
        (
            [-300, 0],
            starter_bag(),
            u32::MAX,
            None,
            buy(BOOTS_OFFER, u16::MAX),
            ErrorCode::InventoryFull,
            vendor,
        ),
        (
            [-300, 0],
            full,
            99,
            None,
            buy(SHORTSWORD_OFFER, 1),
            ErrorCode::InventoryFull,
            vendor,
        ),
        (
            [-300, 0],
            starter_bag(),
            99,
            None,
            buy(BOOTS_OFFER, 0),
            ErrorCode::InvalidInventoryMove,
            vendor,
        ),
        (
            [-300, 0],
            starter_bag(),
            99,
            None,
            buy(8, 1),
            ErrorCode::InvalidVendor,
            vendor,
        ),
        (
            [-300, 0],
            starter_bag(),
            99,
            None,
            buy(u8::MAX, 1),
            ErrorCode::InvalidVendor,
            vendor,
        ),
        (
            [-300, 0],
            starter_bag(),
            99,
            None,
            ZoneCommand::BuyItem {
                npc: GUARD,
                offer: 0,
                quantity: 1,
            },
            ErrorCode::InvalidVendor,
            Some(EntityRef::Npc(GUARD)),
        ),
        (
            [-300, 0],
            starter_bag(),
            99,
            None,
            ZoneCommand::SellItem {
                npc: NpcId::new(99),
                bag_slot: 0,
                quantity: 1,
            },
            ErrorCode::InvalidVendor,
            Some(EntityRef::Npc(NpcId::new(99))),
        ),
        // Reach is inclusive: one unit beyond 5 m is out of range.
        (
            [-VENDOR_REACH_UNITS - 1, 0],
            starter_bag(),
            99,
            None,
            buy(BOOTS_OFFER, 1),
            ErrorCode::OutOfRange,
            vendor,
        ),
        (
            [-VENDOR_REACH_UNITS - 1, 0],
            starter_bag(),
            0,
            None,
            sell(0, 1),
            ErrorCode::OutOfRange,
            vendor,
        ),
        (
            [-300, 0],
            starter_bag(),
            0,
            None,
            sell(2, 1),
            ErrorCode::InvalidInventoryMove,
            vendor,
        ),
        (
            [-300, 0],
            starter_bag(),
            0,
            None,
            sell(16, 1),
            ErrorCode::InvalidInventoryMove,
            vendor,
        ),
        (
            [-300, 0],
            starter_bag(),
            0,
            None,
            sell(0, 0),
            ErrorCode::InvalidInventoryMove,
            vendor,
        ),
        (
            [-300, 0],
            starter_bag(),
            0,
            None,
            sell(0, 4),
            ErrorCode::InvalidInventoryMove,
            vendor,
        ),
        (
            [-300, 0],
            starter_bag(),
            u32::MAX,
            None,
            sell(0, 1),
            ErrorCode::MoneyOverflow,
            vendor,
        ),
        (
            [-300, 0],
            starter_bag(),
            99,
            Some(0),
            buy(BOOTS_OFFER, 1),
            ErrorCode::YouAreDead,
            None,
        ),
        (
            [-300, 0],
            starter_bag(),
            99,
            Some(0),
            sell(0, 1),
            ErrorCode::YouAreDead,
            None,
        ),
    ];
    for (vendor_at, bag, copper, health, command, code, target) in cases {
        let mut zone = market(vendor_at, bag, copper, health);
        let before = zone.snapshot().unwrap().players.remove(0);
        let events = step(&mut zone, 1, command);
        assert_eq!(events, [ZoneEvent::Error { code, target }], "{command:?}");
        let after = zone.snapshot().unwrap().players.remove(0);
        assert_eq!(after.inventory, before.inventory, "{command:?}");
        assert_eq!(after.copper, before.copper, "{command:?}");
        assert_eq!(
            after.inventory_revision, before.inventory_revision,
            "{command:?}"
        );
        assert_eq!(after.inventory_changed_at, before.inventory_changed_at);
    }
    // An exhausted bag revision refuses a trade that would otherwise succeed.
    let zone = near(starter_bag(), 30);
    let mut state = zone.snapshot().unwrap();
    state.players[0].inventory_revision = u64::MAX;
    let mut zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
    assert_eq!(
        step(&mut zone, 1, buy(BOOTS_OFFER, 1)),
        [ZoneEvent::Error {
            code: ErrorCode::InvalidInventoryMove,
            target: Some(EntityRef::Npc(VENDOR))
        }]
    );
    assert_eq!(zone.snapshot().unwrap().players[0].copper, 30);
}

#[test]
fn content_without_vendors_refuses_every_trade_and_binding_is_validated() {
    let plain = arena::arena(
        vec![],
        vec![],
        vec![npc(VENDOR, NpcRole::Vendor, [-300, 0])],
    );
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), Arc::clone(&plain)).unwrap();
    zone.add_player(1).unwrap();
    assert_eq!(
        step(&mut zone, 1, sell(0, 1)),
        [ZoneEvent::Error {
            code: ErrorCode::InvalidVendor,
            target: Some(EntityRef::Npc(VENDOR))
        }]
    );
    let base = ZoneContent::clone(&plain);
    assert!(base.clone().with_vendors(0, vec![]).is_err());
    assert!(
        base.clone()
            .with_vendors(1, vec![(GUARD, starter_vendor_stock())])
            .is_err()
    );
    assert!(
        base.clone()
            .with_vendors(
                1,
                vec![
                    (VENDOR, starter_vendor_stock()),
                    (VENDOR, starter_vendor_stock())
                ]
            )
            .is_err()
    );
    let bound = base
        .clone()
        .with_vendors(1, vec![(VENDOR, starter_vendor_stock())])
        .unwrap();
    assert_ne!(bound.fingerprint(), base.fingerprint());
    assert_eq!(bound.rng_seed(), base.rng_seed());
    let cheaper = VendorStock::new(&[VendorOffer {
        item: BOOTS,
        price: 11,
    }])
    .unwrap();
    let other = base.with_vendors(1, vec![(VENDOR, cheaper)]).unwrap();
    assert_ne!(other.fingerprint(), bound.fingerprint());
}

#[test]
fn vendor_stock_is_validated() {
    let offer = |item, price| VendorOffer {
        item: ItemId::new(item),
        price,
    };
    assert_eq!(VendorStock::new(&[]), Err(VendorStockError::OfferCount));
    assert_eq!(
        VendorStock::new(&[offer(9, 1); MAX_VENDOR_OFFERS + 1]),
        Err(VendorStockError::OfferCount)
    );
    assert_eq!(
        VendorStock::new(&[offer(11, 1)]),
        Err(VendorStockError::UnknownItem)
    );
    assert_eq!(
        VendorStock::new(&[offer(0, 1)]),
        Err(VendorStockError::UnknownItem)
    );
    assert_eq!(
        VendorStock::new(&[offer(9, 0)]),
        Err(VendorStockError::FreeOffer)
    );
    assert_eq!(
        VendorStock::new(&[offer(9, 1), offer(9, 2)]),
        Err(VendorStockError::DuplicateItem)
    );
    let stock = starter_vendor_stock();
    assert_eq!(stock.offers().len(), MAX_VENDOR_OFFERS);
    assert_eq!(stock.offer(DAGGER_OFFER), Some(offer(2, 5)));
    assert_eq!(stock.offer(BOOTS_OFFER), Some(offer(9, 12)));
    assert_eq!(stock.offer(2), Some(offer(8, 12)));
    assert_eq!(stock.offer(SHORTSWORD_OFFER), Some(offer(3, 25)));
    assert_eq!(stock.offer(8), None);
    assert_eq!(sell_price(TROUSERS), 3);
}

#[test]
fn greyhaven_binds_bram_tolliver_as_its_vendor() {
    let content = greyhaven_vale::content();
    assert_eq!(
        content.vendor_revision(),
        mmorpg_core::VENDOR_CATALOG_REVISION
    );
    let [(npc, stock)] = content.vendors() else {
        panic!("one vendor");
    };
    assert_eq!(*npc, VENDOR);
    assert_eq!(content.npc(VENDOR).unwrap().role, NpcRole::Vendor);
    assert_eq!(stock, &starter_vendor_stock());
}

#[test]
fn pending_trades_continue_exactly_after_canonical_recovery() {
    let mut original = near(starter_bag(), 40);
    original.apply_command(1, 1, buy(BOOTS_OFFER, 1)).unwrap();
    original.apply_command(1, 2, sell(0, 3)).unwrap();
    original
        .apply_command(1, 3, buy(SHORTSWORD_OFFER, 1))
        .unwrap();
    let state = original.snapshot().unwrap();
    assert_eq!(state.players[0].combat.intents.len(), 3);
    let mut recovered =
        ZoneSimulation::from_snapshot(state, Arc::clone(original.content())).unwrap();
    for _ in 0..3 {
        original.advance_tick().unwrap();
        recovered.advance_tick().unwrap();
        assert_eq!(recovered.snapshot().unwrap(), original.snapshot().unwrap());
    }
    let player = recovered.snapshot().unwrap().players.remove(0);
    // 40 − 12 + 3 = 31 covers the shortsword (25).
    assert_eq!(player.copper, 6);
    assert_eq!(player.inventory.slots()[0], stack(SHORTSWORD, 1));
    assert_eq!(player.inventory.slots()[2], stack(BOOTS, 1));
    // A stale or duplicate sequence cannot buy again.
    assert!(recovered.apply_command(1, 3, buy(BOOTS_OFFER, 1)).is_err());
}

#[test]
fn a_recovered_player_at_the_coordinate_limit_is_out_of_reach_without_overflow() {
    let zone = near(starter_bag(), 30);
    let mut state = zone.snapshot().unwrap();
    state.players[0].position = [i32::MIN + 100, 50, i32::MIN + 100];
    let mut zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
    zone.apply_command(1, 1, buy(DAGGER_OFFER, 1)).unwrap();
    // The trade resolves before physics moves the far-away player.
    zone.advance_tick().unwrap();
    assert_eq!(
        zone.snapshot_for_player(1).unwrap().events,
        [ZoneEvent::Error {
            code: ErrorCode::OutOfRange,
            target: Some(EntityRef::Npc(VENDOR))
        }]
    );
    assert_eq!(zone.snapshot().unwrap().players[0].copper, 30);
}
