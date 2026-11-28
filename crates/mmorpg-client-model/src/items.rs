//! How both clients word items (#1280): an item's name and attributes, an equipment slot's
//! title, a vendor offer and what selling a bag stack does. The zone settles every trade and
//! equipment change itself.
use mmorpg_core::{EquipmentSlot, ItemId, ItemStats, VendorOffer, find_text, item_template, sell_price};

/// The attribute names in the order items and stat totals list them.
pub const ATTRIBUTE_NAMES: [&str; 4] = ["Stamina", "Strength", "Agility", "Intellect"];

/// The catalog name of `item`, or "Unknown item" for an ID this build does not know.
#[must_use]
pub fn item_name(item: ItemId) -> &'static str {
    item_template(item).map_or("Unknown item", |template| template.name)
}

/// `stats` in [`ATTRIBUTE_NAMES`] order, for example `+2 Strength, +2 Agility`; empty when the
/// item adds none.
#[must_use]
pub fn stats_text(stats: ItemStats) -> String {
    [stats.stamina, stats.strength, stats.agility, stats.intellect]
        .into_iter()
        .zip(ATTRIBUTE_NAMES)
        .filter(|(value, _)| *value > 0)
        .map(|(value, name)| format!("+{value} {name}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// An item's attributes and, for a find (#918), its quality, tactical trait and source, joined
/// by ` · `; empty for a plain bag item.
#[must_use]
pub fn item_detail(item: ItemId) -> String {
    let attributes = item_template(item).map_or_else(String::new, |template| stats_text(template.stats));
    match find_text(item) {
        Some(find) if attributes.is_empty() => find,
        Some(find) => format!("{attributes} · {find}"),
        None => attributes,
    }
}

/// An item's name with its detail in brackets when it has one: `Hunting Dagger (+2 Strength)`.
#[must_use]
pub fn item_line(item: ItemId) -> String {
    let detail = item_detail(item);
    if detail.is_empty() {
        item_name(item).to_owned()
    } else {
        format!("{} ({detail})", item_name(item))
    }
}

/// The title of an equipment slot.
#[must_use]
pub const fn slot_title(slot: EquipmentSlot) -> &'static str {
    match slot {
        EquipmentSlot::MainHand => "Main hand",
        EquipmentSlot::OffHand => "Off hand",
        EquipmentSlot::Head => "Head",
        EquipmentSlot::Chest => "Chest",
        EquipmentSlot::Legs => "Legs",
        EquipmentSlot::Feet => "Feet",
    }
}

/// A vendor offer's row: `Iron Sword · 40 copper + 2 Iron Ore (+3 Strength)`. A smith's recipe
/// (#917) names what it takes from the bag.
#[must_use]
pub fn offer_text(offer: &VendorOffer) -> String {
    let materials = offer
        .materials
        .iter()
        .map(|&(item, quantity)| format!("{quantity} {}", item_name(item)))
        .collect::<Vec<_>>()
        .join(", ");
    let detail = item_detail(offer.item);
    let mut text = format!("{} · {} copper", item_name(offer.item), offer.price);
    if !materials.is_empty() {
        text.push_str(&format!(" + {materials}"));
    }
    if !detail.is_empty() {
        text.push_str(&format!(" ({detail})"));
    }
    text
}

/// What selling a bag stack does: a sale is always the whole stack.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Sale {
    /// The quantity the sell command names.
    pub quantity: u16,
    /// Copper the whole stack fetches.
    pub value: u32,
    /// The stack's row: `Wolf Pelt × 3 · 6 copper`.
    pub text: String,
    /// The sell button's accessible label: `Sell Wolf Pelt × 3 for 6 copper`.
    pub label: String,
}

/// Selling `quantity` of `item` from one bag slot.
#[must_use]
pub fn sale(item: ItemId, quantity: u16) -> Sale {
    let value = sell_price(item).saturating_mul(u32::from(quantity));
    let name = item_name(item);
    Sale {
        quantity,
        value,
        text: format!("{name} × {quantity} · {value} copper"),
        label: format!("Sell {name} × {quantity} for {value} copper"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attributes_list_in_catalog_order_and_skip_zeroes() {
        let stats = ItemStats {
            stamina: 1,
            strength: 0,
            agility: 2,
            intellect: 3,
        };
        assert_eq!(stats_text(stats), "+1 Stamina, +2 Agility, +3 Intellect");
        assert_eq!(stats_text(ItemStats::NONE), "");
        assert_eq!(slot_title(EquipmentSlot::OffHand), "Off hand");
    }

    #[test]
    fn a_sale_is_the_whole_stack() {
        let item = mmorpg_core::ITEM_CATALOG[0].id;
        let sold = sale(item, 3);
        assert_eq!(sold.quantity, 3);
        assert_eq!(sold.value, sell_price(item) * 3);
        assert_eq!(
            sold.text,
            format!("{} × 3 · {} copper", item_name(item), sold.value)
        );
    }

    #[test]
    fn an_offer_names_price_materials_and_detail() {
        let item = mmorpg_core::ITEM_CATALOG
            .iter()
            .find(|item| item.stats != ItemStats::NONE)
            .unwrap();
        let offer = VendorOffer {
            item: item.id,
            price: 12,
            materials: &[],
        };
        assert_eq!(
            offer_text(&offer),
            format!("{} · 12 copper ({})", item.name, item_detail(item.id))
        );
    }
}
