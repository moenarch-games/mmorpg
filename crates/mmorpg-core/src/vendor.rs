//! Vendor rules (#67): an immutable stock of copper-priced offers, the
//! copper a vendor pays for each catalog item, and the atomic purchase and
//! sale settlement. Content binds which NPCs sell which stock; the zone owns
//! eligibility, revisions and the commit point.

use std::{error::Error, fmt};

use crate::{
    EntityRef, ErrorCode, Inventory, InventoryError, ItemId, NpcId, NpcRole, PlayerId, ZoneError,
    ZoneSimulation, item_template,
};

/// Inclusive horizontal (XZ) distance from a vendor's feet to the player (5 m).
pub const VENDOR_REACH_UNITS: i32 = 500;

/// One item a vendor sells, at `price` copper per unit. A smith's recipe (#917) also takes
/// `materials` from the bag for each unit it makes; a plain offer takes none.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VendorOffer {
    pub item: ItemId,
    pub price: u32,
    /// `(item, quantity)` consumed per unit, in authored order; empty for a plain offer.
    pub materials: &'static [(ItemId, u16)],
}

/// One or more offers of distinct catalog items with
/// nonzero prices, in authored order; the offer index is the wire identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VendorStock {
    offers: Vec<VendorOffer>,
}

impl VendorStock {
    pub fn new(offers: &[VendorOffer]) -> Result<Self, VendorStockError> {
        if offers.is_empty() || u32::try_from(offers.len()).is_err() {
            return Err(VendorStockError::OfferCount);
        }
        for (index, offer) in offers.iter().enumerate() {
            if item_template(offer.item).is_none() {
                return Err(VendorStockError::UnknownItem);
            }
            if offer.price == 0 {
                return Err(VendorStockError::FreeOffer);
            }
            if offers[..index].iter().any(|other| other.item == offer.item) {
                return Err(VendorStockError::DuplicateItem);
            }
            if offer
                .materials
                .iter()
                .enumerate()
                .any(|(at, &(item, quantity))| {
                    quantity == 0
                        || item == offer.item
                        || item_template(item).is_none_or(|template| template.max_stack < quantity)
                        || offer.materials[..at].iter().any(|other| other.0 == item)
                })
            {
                return Err(VendorStockError::InvalidMaterials);
            }
        }
        Ok(Self {
            offers: offers.to_vec(),
        })
    }

    #[must_use]
    pub fn offers(&self) -> &[VendorOffer] {
        &self.offers
    }

    #[must_use]
    pub fn offer(&self, index: u32) -> Option<VendorOffer> {
        self.offers.get(usize::try_from(index).ok()?).copied()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VendorStockError {
    OfferCount,
    UnknownItem,
    FreeOffer,
    DuplicateItem,
    InvalidMaterials,
}

impl fmt::Display for VendorStockError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::OfferCount => "a vendor offers one to eight items",
            Self::UnknownItem => "a vendor offer names an unknown item",
            Self::FreeOffer => "a vendor offer has no price",
            Self::DuplicateItem => "a vendor offers an item twice",
            Self::InvalidMaterials => "a recipe's materials are invalid",
        })
    }
}

impl Error for VendorStockError {}

/// Copper any vendor pays for one unit of a catalog item; zero for unknown items. Every catalog
/// item is sellable except the quest supplies, lent trial arms, the rubbing, the locket, the bait,
/// the snare, the barrow's torc, the signal tinder and the order pouch (#1019, items 36 to 48): a
/// vendor refuses those, so accepting, selling and abandoning a quest never mints copper.
#[must_use]
pub const fn sell_price(item: ItemId) -> u32 {
    match item.get() {
        1 => 1,
        2 => 2,
        3 | 4 | 23 => 6,
        24..=26 => 8,
        27 => 14,
        5 => 5,
        6 | 7 => 4,
        8 | 9 => 3,
        10 | 11 => 1,
        12..=14 => 9,
        15..=18 => 18,
        19..=22 | 31 => 1,
        // Gathered materials and the smith's boar-spear (#917).
        28 | 29 => 2,
        30 => 10,
        // The smith's great weapons (#1073).
        32..=35 => 15,
        _ => 0,
    }
}

/// Bram Tolliver's stock: the starter armour and weapons that humanoids
/// also drop, at about four times their sale value, and a cheap Worn
/// Dagger that a new character's sold starter bag can just afford.
#[must_use]
pub fn starter_vendor_stock() -> VendorStock {
    let offer = |item, price| VendorOffer {
        item: ItemId::new(item),
        price,
        materials: &[],
    };
    VendorStock::new(&[
        offer(2, 5),
        offer(9, 12),
        offer(8, 12),
        offer(7, 15),
        offer(6, 15),
        offer(5, 20),
        offer(3, 25),
        offer(4, 25),
        offer(23, 25),
    ])
    .expect("the starter vendor stock is valid")
}

/// Trader Maren's stock at Fenford (#826): the padded armour, the buckler and the shortsword at
/// Bram's prices, and the level 6 mace, jerkin and cowl at five times their sale value, for
/// players working the region's eastern chains.
#[must_use]
pub fn fenford_trader_stock() -> VendorStock {
    let offer = |item, price| VendorOffer {
        item: ItemId::new(item),
        price,
        materials: &[],
    };
    VendorStock::new(&[
        offer(9, 12),
        offer(8, 12),
        offer(7, 15),
        offer(5, 20),
        offer(3, 25),
        offer(23, 25),
        offer(12, 45),
        offer(13, 45),
        offer(14, 45),
    ])
    .expect("the Fenford trader's stock is valid")
}

/// What the smith's boar-spear takes per spear (#917): two veins' ore and two boars' hides and
/// tusks.
pub const BOAR_SPEAR_MATERIALS: [(ItemId, u16); 3] = [
    (crate::IRON_ORE, 4),
    (crate::BOAR_HIDE, 2),
    (crate::BOAR_TUSK, 2),
];

/// The Outpost smith's stock (#917): the Hollowpine boar-spear, made from gathered materials
/// for a small fee, and the four great weapons (#1073) for copper, at four times their sale
/// value.
#[must_use]
pub fn smith_stock() -> VendorStock {
    let great = |look: crate::GreatWeapon| VendorOffer {
        item: look.item(),
        price: 60,
        materials: &[],
    };
    VendorStock::new(&[
        VendorOffer {
            item: crate::HOLLOWPINE_BOAR_SPEAR,
            price: 20,
            materials: &BOAR_SPEAR_MATERIALS,
        },
        great(crate::GreatWeapon::Maul),
        great(crate::GreatWeapon::GreatHammer),
        great(crate::GreatWeapon::GreatAxe),
        great(crate::GreatWeapon::Greatsword),
    ])
    .expect("the smith's stock is valid")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VendorTradeError {
    /// The copper balance does not cover the price.
    NotEnoughMoney,
    /// Crediting the sale would overflow the copper balance.
    MoneyOverflow,
    /// The item has no sale value: a quest's supply or lent arms (#1019).
    NotSellable,
    Inventory(InventoryError),
}

impl fmt::Display for VendorTradeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotEnoughMoney => formatter.write_str("not enough copper"),
            Self::MoneyOverflow => formatter.write_str("the sale would overflow copper"),
            Self::NotSellable => formatter.write_str("vendors do not buy that item"),
            Self::Inventory(error) => error.fmt(formatter),
        }
    }
}

impl Error for VendorTradeError {}

/// Buys `quantity` units of one offer: the full price and every material must be covered and
/// the full quantity must fit once the materials are taken, or bag and copper stay unchanged.
pub fn settle_purchase(
    inventory: &mut Inventory,
    copper: &mut u32,
    offer: VendorOffer,
    quantity: u16,
) -> Result<(), VendorTradeError> {
    if quantity == 0 {
        return Err(VendorTradeError::Inventory(InventoryError::InvalidQuantity));
    }
    let cost = u64::from(offer.price) * u64::from(quantity);
    let balance = u64::from(*copper)
        .checked_sub(cost)
        .ok_or(VendorTradeError::NotEnoughMoney)?;
    let mut bag = inventory.clone();
    for &(item, each) in offer.materials {
        let needed = each
            .checked_mul(quantity)
            .ok_or(VendorTradeError::Inventory(InventoryError::InvalidQuantity))?;
        bag.remove_item(item, needed)
            .map_err(VendorTradeError::Inventory)?;
    }
    bag.insert(offer.item, quantity)
        .map_err(VendorTradeError::Inventory)?;
    *inventory = bag;
    *copper = u32::try_from(balance).map_err(|_| VendorTradeError::MoneyOverflow)?;
    Ok(())
}

/// Sells `quantity` units from one bag slot at their sale value; an item without one
/// ([`sell_price`] zero) is refused, and overflow refuses rather than saturating, leaving bag
/// and copper unchanged.
pub fn settle_sale(
    inventory: &mut Inventory,
    copper: &mut u32,
    slot: usize,
    quantity: u16,
) -> Result<(), VendorTradeError> {
    let mut bag = inventory.clone();
    let sold = bag
        .remove(slot, quantity)
        .map_err(VendorTradeError::Inventory)?;
    if sell_price(sold.item()) == 0 {
        return Err(VendorTradeError::NotSellable);
    }
    let value = u64::from(sell_price(sold.item())) * u64::from(sold.quantity());
    let balance =
        u32::try_from(u64::from(*copper) + value).map_err(|_| VendorTradeError::MoneyOverflow)?;
    *inventory = bag;
    *copper = balance;
    Ok(())
}

/// A buy or sell request at one vendor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Trade {
    Buy { offer: u32, quantity: u16 },
    Sell { bag_slot: u8, quantity: u16 },
}

impl ZoneSimulation {
    /// Tick step 1 of `BuyItem` and `SellItem` for a living player: checks
    /// the vendor and reach, stages bag and copper, preflights the bag
    /// revision, then commits both. Refusals change nothing.
    pub(crate) fn trade(
        &mut self,
        player_id: PlayerId,
        npc: NpcId,
        trade: Trade,
    ) -> Result<Option<(ErrorCode, Option<EntityRef>)>, ZoneError> {
        let target = Some(EntityRef::Npc(npc));
        let refuse = |code| Ok(Some((code, target)));
        let Some(vendor) = self
            .content
            .npc(npc)
            .filter(|npc| npc.role == NpcRole::Vendor)
        else {
            return refuse(ErrorCode::InvalidVendor);
        };
        let Some(feet) = self.npc_feet(vendor.id) else {
            return refuse(ErrorCode::InvalidVendor);
        };
        let Some(stock) = self.content.vendor_stock(npc) else {
            return refuse(ErrorCode::InvalidVendor);
        };
        let offer = match trade {
            Trade::Buy { offer, .. } => match stock.offer(offer) {
                Some(offer) => Some(offer),
                None => return refuse(ErrorCode::InvalidVendor),
            },
            Trade::Sell { .. } => None,
        };
        let from = self.player_position(player_id)?;
        // i128 keeps the squared distance exact for any recovered i32 position.
        let [dx, dz] = [
            i128::from(from.x) - i128::from(feet[0]),
            i128::from(from.z) - i128::from(feet[1]),
        ];
        if dx * dx + dz * dz > i128::from(VENDOR_REACH_UNITS).pow(2)
            || !self.npc_in_sight(player_id, npc)?
        {
            return refuse(ErrorCode::OutOfRange);
        }
        if !self.vendor_open(npc) {
            return refuse(ErrorCode::VendorClosed);
        }
        let player = self
            .players
            .get_mut(&player_id)
            .ok_or_else(|| ZoneError::new("trading player is absent"))?;
        let (mut bag, mut copper) = (player.inventory.clone(), player.copper);
        let settled = match (trade, offer) {
            (Trade::Buy { quantity, .. }, Some(offer)) => {
                settle_purchase(&mut bag, &mut copper, offer, quantity)
            }
            (Trade::Sell { bag_slot, quantity }, _) => {
                settle_sale(&mut bag, &mut copper, usize::from(bag_slot), quantity)
            }
            (Trade::Buy { .. }, None) => return refuse(ErrorCode::InvalidVendor),
        };
        let code = match settled {
            Err(VendorTradeError::NotEnoughMoney) => Some(ErrorCode::NotEnoughMoney),
            Err(VendorTradeError::Inventory(InventoryError::InsufficientItems))
                if matches!(trade, Trade::Buy { .. }) =>
            {
                Some(ErrorCode::MissingMaterials)
            }
            Err(VendorTradeError::MoneyOverflow) => Some(ErrorCode::MoneyOverflow),
            Err(VendorTradeError::NotSellable) => Some(ErrorCode::NotSellable),
            Err(VendorTradeError::Inventory(InventoryError::NoCapacity)) => {
                Some(ErrorCode::InventoryFull)
            }
            Err(VendorTradeError::Inventory(_)) => Some(ErrorCode::InvalidInventoryMove),
            Ok(()) => match player.inventory_revision.checked_add(1) {
                None => Some(ErrorCode::InvalidInventoryMove),
                Some(revision) => {
                    player.inventory = bag;
                    player.copper = copper;
                    player.inventory_revision = revision;
                    None
                }
            },
        };
        Ok(code.map(|code| (code, target)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ITEM_CATALOG;

    #[test]
    fn every_catalog_item_but_the_quest_supplies_is_sellable_and_every_offer_costs_more() {
        for item in ITEM_CATALOG {
            // The quest supplies, lent trial arms, the rubbing, the locket, the bait, the snare,
            // the barrow's torc, the signal tinder and the order pouch (#1019) are worth nothing to
            // a vendor.
            let supply = (36..=48).contains(&item.id.get());
            assert_eq!(sell_price(item.id) == 0, supply, "{}", item.name);
        }
        assert_eq!(sell_price(ItemId::new(0)), 0);
        assert_eq!(sell_price(ItemId::new(99)), 0);
        for offer in starter_vendor_stock()
            .offers()
            .iter()
            .chain(fenford_trader_stock().offers())
        {
            assert!(offer.price > sell_price(offer.item));
        }
    }
}
