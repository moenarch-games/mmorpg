//! Versioned content catalog for clients: what projections refer to by ID.
//!
//! Player projections name creatures by template ID and NPCs by NPC ID, and
//! clients read area names from core content. The catalog (format
//! [`CATALOG_FORMAT`], version [`CATALOG_FORMAT_VERSION`]) exports those
//! tables of the hosted [`ZoneContent`] as compact JSON: creature templates
//! (name, family, behaviour, levels, elite, body size), NPCs (name, role,
//! level), areas (name), items (name, stack limit, equipment slot, stats,
//! sale value), vendors (NPC and priced offers), classes (name, resource),
//! abilities (name, user, unlock level, cost, cast time, cooldown, aura) and
//! quests (name, text, giver, ender, prerequisite, objectives, rewards). It carries the content
//! revision and fingerprint,
//! so a client can refuse a catalog of other content. Combat numbers,
//! spawn points and AI stay on the server side.

use std::sync::OnceLock;

use mmorpg_core::ZoneContent;
use serde::Serialize;

use crate::host::hosted_content;

pub const CATALOG_FORMAT: &str = "mmorpg.catalog";
pub const CATALOG_FORMAT_VERSION: u32 = 6;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogExport {
    pub format: &'static str,
    pub version: u32,
    /// Decimal `u64`, so JavaScript never rounds it.
    pub content_revision: String,
    /// The content fingerprint as 16 lower-case hex digits.
    pub content_fingerprint: String,
    pub creature_templates: Vec<CreatureTemplateExport>,
    pub npcs: Vec<NpcExport>,
    pub areas: Vec<AreaNameExport>,
    pub item_catalog_revision: String,
    pub items: Vec<ItemExport>,
    /// Class choices by wire value (0 Warden, 1 Ranger, 2 Arcanist).
    pub classes: Vec<ClassExport>,
    pub ability_catalog_revision: String,
    pub abilities: Vec<AbilityExport>,
    /// `"0"` for content without vendors.
    pub vendor_catalog_revision: String,
    /// Ordered by NPC ID; offer order is the wire offer index.
    pub vendors: Vec<VendorExport>,
    /// `"0"` for content without quests.
    pub quest_catalog_revision: String,
    /// Ordered by quest ID; objective order is the wire progress index.
    pub quests: Vec<QuestExport>,
}

/// A quest as clients present it: dialog text, who gives and ends it,
/// what it asks for and what it grants.
#[derive(Clone, Debug, Serialize)]
pub struct QuestExport {
    pub id: u8,
    pub name: String,
    pub text: String,
    pub giver: u32,
    pub ender: u32,
    pub prerequisite: Option<u8>,
    pub objectives: Vec<ObjectiveExport>,
    pub experience: u32,
    pub copper: u32,
    /// Item IDs; the reward choice is the index into this list.
    pub choices: Vec<u16>,
}

/// `kind` is `kill` (`target` is a creature template), `collect` (`target`
/// is an item that creatures of template `source` drop), `talk` (`target`
/// is an NPC) or `explore` (`target` is an area).
#[derive(Clone, Copy, Debug, Serialize)]
pub struct ObjectiveExport {
    pub kind: &'static str,
    pub target: u32,
    pub source: Option<u16>,
    pub count: u8,
}

fn objective(objective: mmorpg_core::QuestObjective) -> ObjectiveExport {
    use mmorpg_core::QuestObjective;
    let (kind, target, source) = match objective {
        QuestObjective::Kill { template, .. } => ("kill", u32::from(template.get()), None),
        QuestObjective::Collect { item, source, .. } => {
            ("collect", u32::from(item.get()), Some(source.get()))
        }
        QuestObjective::Talk { npc } => ("talk", npc.get(), None),
        QuestObjective::Explore { area } => ("explore", u32::from(area.get()), None),
    };
    ObjectiveExport {
        kind,
        target,
        source,
        count: objective.required(),
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct VendorExport {
    pub npc: u32,
    pub offers: Vec<VendorOfferExport>,
}

/// One unit of `item` costs `price` copper.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct VendorOfferExport {
    pub item: u16,
    pub price: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct ClassExport {
    pub id: u8,
    pub name: &'static str,
    pub resource: &'static str,
}

/// What clients show and validate about an ability; its effect numbers
/// stay on the server.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AbilityExport {
    pub id: u8,
    pub name: &'static str,
    /// A class name, or `creature`.
    pub user: &'static str,
    pub level: u8,
    pub cost: u16,
    /// 0 for instants.
    pub cast_ticks: u16,
    pub channel: bool,
    pub cooldown: u16,
    /// The wire code of the aura kind it applies (1 damage over time …
    /// 7 haste, as in snapshot aura records), if any.
    pub aura: Option<u8>,
}

/// `[id, name, family, behaviour, minLevel, maxLevel, elite, halfExtents]`
/// in object form.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatureTemplateExport {
    pub id: u16,
    pub name: String,
    pub family: &'static str,
    pub behaviour: &'static str,
    pub min_level: u8,
    pub max_level: u8,
    pub elite: bool,
    /// Collision box in units; presentation sizes placeholder bodies by it.
    pub half_extents: [i32; 3],
}

#[derive(Clone, Debug, Serialize)]
pub struct NpcExport {
    pub id: u32,
    pub name: String,
    pub role: &'static str,
    pub level: u8,
}

#[derive(Clone, Debug, Serialize)]
pub struct AreaNameExport {
    pub id: u16,
    pub name: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemExport {
    pub id: u16,
    pub name: &'static str,
    pub max_stack: u16,
    /// The camel-case equipment slot name, or `null` for plain bag items.
    pub slot: Option<&'static str>,
    pub stats: StatsExport,
    /// Copper a vendor pays for one unit.
    pub sell_price: u32,
}

/// The attributes an item adds while equipped.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct StatsExport {
    pub stamina: u8,
    pub strength: u8,
    pub agility: u8,
    pub intellect: u8,
}

/// The catalog of `content`, every table ordered by ID.
#[must_use]
pub fn catalog(content: &ZoneContent) -> CatalogExport {
    CatalogExport {
        format: CATALOG_FORMAT,
        version: CATALOG_FORMAT_VERSION,
        content_revision: content.revision().to_string(),
        item_catalog_revision: mmorpg_core::ITEM_CATALOG_REVISION.to_string(),
        items: mmorpg_core::ITEM_CATALOG
            .iter()
            .map(|item| ItemExport {
                id: item.id.get(),
                name: item.name,
                max_stack: item.max_stack,
                slot: item.slot.map(mmorpg_core::EquipmentSlot::name),
                stats: StatsExport {
                    stamina: item.stats.stamina,
                    strength: item.stats.strength,
                    agility: item.stats.agility,
                    intellect: item.stats.intellect,
                },
                sell_price: mmorpg_core::sell_price(item.id),
            })
            .collect(),
        vendor_catalog_revision: content.vendor_revision().to_string(),
        vendors: content
            .vendors()
            .iter()
            .map(|(npc, stock)| VendorExport {
                npc: npc.get(),
                offers: stock
                    .offers()
                    .iter()
                    .map(|offer| VendorOfferExport {
                        item: offer.item.get(),
                        price: offer.price,
                    })
                    .collect(),
            })
            .collect(),
        quest_catalog_revision: content.quest_revision().to_string(),
        quests: content
            .quests()
            .iter()
            .map(|quest| QuestExport {
                id: quest.id.get(),
                name: quest.name.clone(),
                text: quest.text.clone(),
                giver: quest.giver.get(),
                ender: quest.ender.get(),
                prerequisite: quest.prerequisite.map(mmorpg_core::QuestId::get),
                objectives: quest.objectives.iter().copied().map(objective).collect(),
                experience: quest.rewards.experience,
                copper: quest.rewards.copper,
                choices: quest
                    .rewards
                    .choices
                    .iter()
                    .map(|item| item.get())
                    .collect(),
            })
            .collect(),
        content_fingerprint: format!("{:016x}", content.fingerprint()),
        classes: mmorpg_core::PlayerClass::ALL
            .iter()
            .map(|class| ClassExport {
                id: class.code(),
                name: class.name(),
                resource: class.resource().name(),
            })
            .collect(),
        ability_catalog_revision: mmorpg_core::ABILITY_CATALOG_REVISION.to_string(),
        abilities: mmorpg_core::ABILITY_CATALOG
            .iter()
            .map(|ability| AbilityExport {
                id: ability.id.get(),
                name: ability.name,
                user: match ability.user {
                    mmorpg_core::AbilityUser::Class(class) => class.name(),
                    mmorpg_core::AbilityUser::Creature => "creature",
                },
                level: ability.level,
                cost: ability.cost,
                cast_ticks: ability.cast.ticks(),
                channel: ability.cast.is_channel(),
                cooldown: ability.cooldown,
                aura: ability.aura().map(|aura| aura.kind.code()),
            })
            .collect(),
        creature_templates: content
            .creature_templates()
            .iter()
            .map(|template| CreatureTemplateExport {
                id: template.id.get(),
                name: template.name.clone(),
                family: template.family.name(),
                behaviour: template.behaviour.name(),
                min_level: template.min_level,
                max_level: template.max_level,
                elite: template.elite,
                half_extents: template.half_extents,
            })
            .collect(),
        npcs: content
            .npcs()
            .iter()
            .map(|npc| NpcExport {
                id: npc.id.get(),
                name: npc.name.clone(),
                role: npc.role.name(),
                level: npc.level,
            })
            .collect(),
        areas: content
            .areas()
            .areas()
            .iter()
            .map(|area| AreaNameExport {
                id: area.id().get(),
                name: area.name().to_owned(),
            })
            .collect(),
    }
}

/// The serialized catalog of the hosted content. Serializing plain data
/// cannot fail.
#[must_use]
pub fn hosted_catalog_json() -> &'static str {
    static JSON: OnceLock<String> = OnceLock::new();
    JSON.get_or_init(|| {
        serde_json::to_string(&catalog(&hosted_content()))
            .expect("catalog export is plain serializable data")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    #[test]
    fn the_catalog_is_versioned_json_of_the_hosted_content() {
        let json = hosted_catalog_json();
        let value: Value = serde_json::from_str(json).unwrap();
        let content = hosted_content();
        assert_eq!(value["format"], CATALOG_FORMAT);
        assert_eq!(value["version"], CATALOG_FORMAT_VERSION);
        assert_eq!(value["itemCatalogRevision"], "3");
        let stats = |stamina, strength, agility, intellect| json!({"stamina": stamina, "strength": strength, "agility": agility, "intellect": intellect});
        let item = |id, name, slot, stats| {
            let max_stack = match id {
                1 => 20,
                10 => 10,
                _ => 1,
            };
            let sell_price = mmorpg_core::sell_price(mmorpg_core::ItemId::new(id));
            json!({"id": id, "name": name, "maxStack": max_stack, "slot": slot, "stats": stats, "sellPrice": sell_price})
        };
        assert_eq!(
            value["items"],
            json!([
                item(1, "Torn Fur", Value::Null, stats(0, 0, 0, 0)),
                item(2, "Worn Dagger", json!("mainHand"), stats(0, 2, 2, 0)),
                item(
                    3,
                    "Militia Shortsword",
                    json!("mainHand"),
                    stats(1, 3, 0, 0)
                ),
                item(4, "Apprentice Wand", json!("mainHand"), stats(0, 0, 0, 4)),
                item(5, "Pine Buckler", json!("offHand"), stats(2, 0, 0, 0)),
                item(6, "Cloth Hood", json!("head"), stats(1, 0, 0, 2)),
                item(7, "Padded Tunic", json!("chest"), stats(2, 0, 0, 0)),
                item(8, "Padded Trousers", json!("legs"), stats(1, 0, 0, 0)),
                item(9, "Worn Boots", json!("feet"), stats(1, 0, 2, 0)),
                item(10, "Wolf Pelt", Value::Null, stats(0, 0, 0, 0)),
            ])
        );
        assert_eq!(value["vendorCatalogRevision"], "2");
        assert_eq!(value["questCatalogRevision"], "1");
        let quests = value["quests"].as_array().unwrap();
        assert_eq!(quests.len(), 9);
        assert_eq!(
            quests[1],
            json!({
                "id": 2, "name": "Pelts for the Tanner",
                "text": "The Marshal's wolves left good fur behind. Bring me five wolf pelts from the Wolfrun Woods.",
                "giver": 2, "ender": 2, "prerequisite": 1,
                "objectives": [{"kind": "collect", "target": 10, "source": 1, "count": 5}],
                "experience": 200, "copper": 15, "choices": [9, 6],
            })
        );
        assert_eq!(
            quests[6]["objectives"],
            json!([
                {"kind": "explore", "target": 5, "source": null, "count": 1},
                {"kind": "kill", "target": 6, "source": null, "count": 10},
            ])
        );
        assert_eq!(quests[8]["objectives"][0]["kind"], "talk");
        let offer = |item, price| json!({"item": item, "price": price});
        assert_eq!(
            value["vendors"],
            json!([{"npc": 3, "offers": [
                offer(2, 5), offer(9, 12), offer(8, 12), offer(7, 15),
                offer(6, 15), offer(5, 20), offer(3, 25), offer(4, 25),
            ]}])
        );
        assert_eq!(value["contentRevision"], content.revision().to_string());
        assert_eq!(
            value["contentFingerprint"],
            format!("{:016x}", content.fingerprint())
        );
        assert_eq!(
            value["creatureTemplates"][0],
            json!({
                "id": 1,
                "name": "Timber Wolf",
                "family": "wolf",
                "behaviour": "aggressive",
                "minLevel": 1,
                "maxLevel": 2,
                "elite": false,
                "halfExtents": [40, 45, 40],
            })
        );
        let garrick = &value["creatureTemplates"][6];
        assert_eq!(
            (&garrick["name"], &garrick["elite"]),
            (&json!("Garrick Redbrand"), &json!(true))
        );
        assert_eq!(
            value["npcs"][4],
            json!({ "id": 5, "name": "Brother Aldous", "role": "spirit_healer", "level": 10 })
        );
        assert_eq!(
            value["classes"],
            json!([
                {"id": 0, "name": "warden", "resource": "rage"},
                {"id": 1, "name": "ranger", "resource": "focus"},
                {"id": 2, "name": "arcanist", "resource": "mana"}
            ])
        );
        assert_eq!(value["abilityCatalogRevision"], "1");
        assert_eq!(
            value["abilities"][8],
            json!({
                "id": 9, "name": "Firebolt", "user": "arcanist", "level": 1, "cost": 25,
                "castTicks": 60, "channel": false, "cooldown": 0, "aura": null
            })
        );
        assert_eq!(value["abilities"][12]["user"], "creature");
        assert_eq!(value["abilities"][9]["aura"], 4, "Frost Nova roots");
        let names: Vec<_> = value["areas"]
            .as_array()
            .unwrap()
            .iter()
            .map(|area| area["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            [
                "Greyhaven Outpost",
                "Wolfrun Woods",
                "Millbrook Farm",
                "Stillwater Lake",
                "Redbrand Hollow"
            ]
        );
        assert_eq!(
            value["creatureTemplates"].as_array().unwrap().len(),
            content.creature_templates().len()
        );
        assert_eq!(
            value["npcs"].as_array().unwrap().len(),
            content.npcs().len()
        );
    }

    #[test]
    fn the_catalog_is_deterministic_and_carries_no_server_numbers() {
        assert_eq!(
            hosted_catalog_json(),
            serde_json::to_string(&catalog(&hosted_content())).unwrap()
        );
        let json = hosted_catalog_json();
        for secret in ["damage", "health", "swing", "respawn", "position", "wander"] {
            assert!(!json.contains(secret), "the catalog exposes {secret}");
        }
    }
}
