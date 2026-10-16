//! Vendor rules (#67): an immutable stock of copper-priced offers, the
//! copper a vendor pays for each catalog item, and the atomic purchase and
//! sale settlement. Content binds which NPCs sell which stock; the zone owns
//! eligibility, revisions and the commit point.

use std::{error::Error, fmt};

use crate::{
    EntityRef, ErrorCode, Inventory, InventoryError, ItemId, NpcId, NpcRole, PlayerId, ZoneError,
    ZoneSimulation, item_template,
};

/// Revision of the vendor rules: the sale values below and the hosted stock.
/// Revision 2 prices the Wolf Pelt (item catalog 3).
pub const VENDOR_CATALOG_REVISION: u64 = 2;
/// A vendor offers at most this many items.
pub const MAX_VENDOR_OFFERS: usize = 8;
/// Inclusive horizontal (XZ) distance from a vendor's feet to the player (5 m).
pub const VENDOR_REACH_UNITS: i32 = 500;

/// One item a vendor sells, at `price` copper per unit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VendorOffer {
    pub item: ItemId,
    pub price: u32,
}

/// One to [`MAX_VENDOR_OFFERS`] offers of distinct catalog items with
/// nonzero prices, in authored order; the offer index is the wire identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VendorStock {
    offers: Vec<VendorOffer>,
}

impl VendorStock {
    pub fn new(offers: &[VendorOffer]) -> Result<Self, VendorStockError> {
        if offers.is_empty() || offers.len() > MAX_VENDOR_OFFERS {
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
    pub fn offer(&self, index: u8) -> Option<VendorOffer> {
        self.offers.get(usize::from(index)).copied()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VendorStockError {
    OfferCount,
    UnknownItem,
    FreeOffer,
    DuplicateItem,
}

impl fmt::Display for VendorStockError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::OfferCount => "a vendor offers one to eight items",
            Self::UnknownItem => "a vendor offer names an unknown item",
            Self::FreeOffer => "a vendor offer has no price",
            Self::DuplicateItem => "a vendor offers an item twice",
        })
    }
}

impl Error for VendorStockError {}

/// Copper any vendor pays for one unit of a catalog item; zero for unknown
/// items. Every catalog item is sellable.
#[must_use]
pub const fn sell_price(item: ItemId) -> u32 {
    match item.get() {
        1 => 1,
        2 => 2,
        3 | 4 => 6,
        5 => 5,
        6 | 7 => 4,
        8 | 9 => 3,
        10 => 1,
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
    ])
    .expect("the starter vendor stock is valid")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VendorTradeError {
    /// The copper balance does not cover the price.
    NotEnoughMoney,
    /// Crediting the sale would overflow the copper balance.
    MoneyOverflow,
    Inventory(InventoryError),
}

impl fmt::Display for VendorTradeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotEnoughMoney => formatter.write_str("not enough copper"),
            Self::MoneyOverflow => formatter.write_str("the sale would overflow copper"),
            Self::Inventory(error) => error.fmt(formatter),
        }
    }
}

impl Error for VendorTradeError {}

/// Buys `quantity` units of one offer: the full price must be covered and
/// the full quantity must fit, or bag and copper stay unchanged.
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
    bag.insert(offer.item, quantity)
        .map_err(VendorTradeError::Inventory)?;
    *inventory = bag;
    *copper = u32::try_from(balance).map_err(|_| VendorTradeError::MoneyOverflow)?;
    Ok(())
}

/// Sells `quantity` units from one bag slot at their sale value; overflow
/// refuses rather than saturating, leaving bag and copper unchanged.
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
    Buy { offer: u8, quantity: u16 },
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
        let feet = vendor.position;
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
        if dx * dx + dz * dz > i128::from(VENDOR_REACH_UNITS).pow(2) {
            return refuse(ErrorCode::OutOfRange);
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
            Err(VendorTradeError::MoneyOverflow) => Some(ErrorCode::MoneyOverflow),
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
                    player.inventory_changed_at = self.tick + 1;
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
    fn every_catalog_item_is_sellable_and_every_offer_costs_more() {
        for item in ITEM_CATALOG {
            assert!(sell_price(item.id) > 0, "{} has no sale value", item.name);
        }
        assert_eq!(sell_price(ItemId::new(0)), 0);
        assert_eq!(sell_price(ItemId::new(11)), 0);
        for offer in starter_vendor_stock().offers() {
            assert!(offer.price > sell_price(offer.item));
        }
    }
}
