//! Selected-person context projected from immutable content and received self sheets.
//! This is navigation and intent composition; the zone validates every request.
use mmorpg_core::{
    Inventory, NpcId, NpcRole, QuestSheet, ZoneCommand, ZoneContent, ZoneSnapshot, item_template,
    sell_price,
};

#[derive(Default)]
pub struct NpcContext {
    pub npc: Option<NpcId>,
    pub row: usize,
    pub choice: usize,
    pub inventory: Option<Inventory>,
    pub quests: Option<QuestSheet>,
    pub feedback: String,
    revision: Option<u64>,
    detail_page: usize,
    copper: u32,
}
impl NpcContext {
    pub fn observe(&mut self, snapshot: &ZoneSnapshot) {
        self.copper = snapshot.viewer.copper;
        if self.revision != Some(snapshot.inventory_revision) {
            self.inventory = None;
            self.quests = None;
            self.revision = Some(snapshot.inventory_revision);
        }
        if let Some(inventory) = &snapshot.inventory {
            self.inventory = Some(inventory.clone());
        }
        if let Some(quests) = &snapshot.quests {
            self.quests = Some(quests.clone());
        }
        for event in &snapshot.events {
            if let mmorpg_core::ZoneEvent::Error { code, .. } = event {
                self.feedback = format!("refused: {code:?}");
            }
        }
    }
    pub fn open(&mut self, npc: NpcId) {
        self.npc = Some(npc);
        self.row = 0;
        self.choice = 0;
        self.feedback.clear();
        self.detail_page = 0;
    }
    pub fn close(&mut self) {
        self.npc = None;
        self.feedback.clear();
    }
    /// The NPC's quests in display order: turn-ins, then offers, then the rest.
    #[must_use]
    pub fn quest_rows<'a>(
        &self,
        content: &'a ZoneContent,
        npc: NpcId,
    ) -> Vec<&'a mmorpg_core::Quest> {
        let sheet = self.quests.as_ref();
        let mut rows: Vec<_> = content
            .quests()
            .iter()
            .filter(|q| q.giver == npc || q.ender == npc)
            .collect();
        rows.sort_by_key(|q| {
            let active = sheet.is_some_and(|s| s.entries.iter().any(|e| e.quest == q.id));
            let done = sheet.is_some_and(|s| s.completed & (1u32 << (q.id.get() - 1)) != 0);
            let unlocked = q
                .prerequisite
                .is_none_or(|p| sheet.is_some_and(|s| s.completed & (1u32 << (p.get() - 1)) != 0));
            (
                if active && q.ender == npc {
                    0
                } else if !done && unlocked && q.giver == npc {
                    1
                } else {
                    2
                },
                q.id,
            )
        });
        rows
    }
    pub fn navigate(&mut self, content: &ZoneContent, delta: isize, reward: bool) {
        let Some(npc) = self.npc else { return };
        self.detail_page = 0;
        if reward {
            let count = self
                .quest_rows(content, npc)
                .get(self.row)
                .map_or(0, |q| q.rewards.choices.len());
            self.choice = if count == 0 {
                0
            } else {
                (self.choice as isize + delta).rem_euclid(count as isize) as usize
            };
        } else {
            let count = if content.vendor_stock(npc).is_some() {
                mmorpg_core::INVENTORY_SLOTS
                    .max(content.vendor_stock(npc).map_or(0, |s| s.offers().len()))
            } else {
                self.quest_rows(content, npc).len()
            };
            self.row = if count == 0 {
                0
            } else {
                (self.row as isize + delta).rem_euclid(count as isize) as usize
            };
            self.choice = 0;
        }
    }
    pub fn action(&mut self, content: &ZoneContent, sell: bool) -> Option<ZoneCommand> {
        let npc = self.npc?;
        match content.npc(npc)?.role {
            NpcRole::QuestGiver if !sell => {
                let q = *self.quest_rows(content, npc).get(self.row)?;
                let active = self
                    .quests
                    .as_ref()
                    .is_some_and(|s| s.entries.iter().any(|e| e.quest == q.id));
                let sheet = self.quests.as_ref()?;
                let done = sheet.completed & (1u32 << (q.id.get() - 1)) != 0;
                let unlocked = q
                    .prerequisite
                    .is_none_or(|p| sheet.completed & (1u32 << (p.get() - 1)) != 0);
                if active && q.ender == npc {
                    Some(ZoneCommand::CompleteQuest {
                        npc,
                        quest: q.id.get(),
                        choice: (self.choice % q.rewards.choices.len().max(1)) as u8,
                    })
                } else if !active && !done && unlocked && q.giver == npc {
                    Some(ZoneCommand::AcceptQuest {
                        npc,
                        quest: q.id.get(),
                    })
                } else {
                    None
                }
            }
            NpcRole::Vendor if sell => {
                // An omitted newer sheet is loading, not an empty slot. A fresh
                // deliberate request still names this bag slot; only core settles it.
                if self.row >= mmorpg_core::INVENTORY_SLOTS {
                    return None;
                }
                if let Some(bag) = &self.inventory {
                    bag.slots().get(self.row)?.as_ref()?;
                }
                Some(ZoneCommand::SellItem {
                    npc,
                    bag_slot: self.row as u8,
                    quantity: 1,
                })
            }
            NpcRole::Vendor => {
                content.vendor_stock(npc)?.offers().get(self.row)?;
                Some(ZoneCommand::BuyItem {
                    npc,
                    offer: self.row as u8,
                    quantity: 1,
                })
            }
            NpcRole::Guard | NpcRole::SpiritHealer | NpcRole::QuestGiver => None,
        }
    }
    /// Page authored dialogue and objective descriptions without hiding the world.
    pub fn next_detail(&mut self, content: &ZoneContent) {
        let Some(npc) = self.npc else { return };
        if let Some(q) = self.quest_rows(content, npc).get(self.row) {
            self.detail_page = (self.detail_page + 1) % (dialogue_pages(q, content).len() + 1);
        }
    }
    pub fn text(&self, content: &ZoneContent) -> String {
        let Some(npc) = self.npc.and_then(|id| content.npc(id)) else {
            return String::new();
        };
        let detail = match npc.role {
            NpcRole::Guard | NpcRole::SpiritHealer => "no interaction supported".into(),
            NpcRole::Vendor => {
                let stock = content.vendor_stock(npc.id);
                let offer = stock.and_then(|s| s.offers().get(self.row));
                let buy = offer.map_or_else(
                    || "no offer in this row".into(),
                    |o| format!("{}: {} copper", item_name(o.item), o.price),
                );
                let sale = self
                    .inventory
                    .as_ref()
                    .and_then(|bag| bag.slots().get(self.row))
                    .copied()
                    .flatten()
                    .map_or_else(
                        || {
                            if self.inventory.is_none() {
                                "current sheet pending (zone validates slot)".into()
                            } else {
                                "empty".into()
                            }
                        },
                        |s| {
                            format!(
                                "{} x{}: {} copper each",
                                item_name(s.item()),
                                s.quantity(),
                                sell_price(s.item())
                            )
                        },
                    );
                format!(
                    "{}c · {}: {} · {}: {} · Up/Down row {}",
                    self.copper,
                    if offer.is_some() {
                        "1 Buy"
                    } else {
                        "Buy unavailable"
                    },
                    buy,
                    if self.inventory.is_none()
                        || self
                            .inventory
                            .as_ref()
                            .and_then(|bag| bag.slots().get(self.row))
                            .is_some_and(Option::is_some)
                    {
                        "2 Sell"
                    } else {
                        "Sell unavailable"
                    },
                    sale,
                    self.row + 1
                )
            }
            NpcRole::QuestGiver => {
                if self.quests.is_none() {
                    return format!("{} — waiting for current quest sheet", npc.name);
                }
                let rows = self.quest_rows(content, npc.id);
                if self.detail_page > 0
                    && let Some(q) = rows.get(self.row)
                {
                    let pages = dialogue_pages(q, content);
                    let page = (self.detail_page - 1) % pages.len();
                    return format!(
                        "{} — {} · H next page {}/{} · Esc close",
                        npc.name,
                        pages[page],
                        page + 1,
                        pages.len()
                    );
                }
                rows.get(self.row).map_or_else(||"no quests available".into(),|q| {
                    let sheet=self.quests.as_ref();
                    let entry=sheet.and_then(|s|s.entries.iter().find(|e|e.quest==q.id));
                    let completed=sheet.is_some_and(|s|s.completed & (1u32 << (q.id.get()-1))!=0);
                    let unlocked=q.prerequisite.is_none_or(|p|sheet.is_some_and(|s|s.completed & (1u32 << (p.get()-1))!=0));
                    let state=if completed {"completed"} else if entry.is_some() && q.ender==npc.id {"1 Turn in (zone checks objectives)"} else if entry.is_some() {"in progress; return to its named ender"} else if unlocked && q.giver==npc.id {"available · 1 Accept"} else if unlocked {"available from its giver (H)"} else {"locked by prerequisite"};
                    let choices=q.rewards.choices.iter().enumerate().map(|(i,item)|format!("{}{}",if i==self.choice % q.rewards.choices.len().max(1) {">"} else {""},item_name(*item))).collect::<Vec<_>>().join(" / ");
                    format!("{} — {state} · rewards {} copper {} · Up/Down quest {}/{} · Left/Right reward · H dialogue/objectives",q.name,q.rewards.copper,choices,self.row+1,rows.len())
                })
            }
        };
        format!("{} — {detail} · {} · Esc close", npc.name, self.feedback)
    }
}
fn item_name(id: mmorpg_core::ItemId) -> &'static str {
    item_template(id).map_or("unknown item", |item| item.name)
}

fn dialogue_pages(quest: &mmorpg_core::Quest, content: &ZoneContent) -> Vec<String> {
    quest_brief(quest, content)
        .chars()
        .collect::<Vec<_>>()
        .chunks(100)
        .map(|chunk| chunk.iter().collect())
        .collect()
}

/// A quest's authored text, where it starts and ends, and its named objectives.
#[must_use]
pub fn quest_brief(quest: &mmorpg_core::Quest, content: &ZoneContent) -> String {
    use mmorpg_core::QuestObjective;
    let ender = content
        .npc(quest.ender)
        .map_or("quest ender", |npc| npc.name.as_str());
    let giver = content
        .npc(quest.giver)
        .map_or("quest giver", |npc| npc.name.as_str());
    let mut text = format!("{} Accept from {giver}; return to {ender}. ", quest.text);
    for objective in &quest.objectives {
        text.push_str(&match *objective {
            QuestObjective::Kill { template, count } => format!(
                "Defeat {count} {}. ",
                content
                    .creature_template(template)
                    .map_or("creatures", |t| t.name.as_str())
            ),
            QuestObjective::Collect { item, count, .. } => {
                format!("Collect {count} {}. ", item_name(item))
            }
            QuestObjective::Talk { npc } => format!(
                "Speak to {}. ",
                content
                    .npc(npc)
                    .map_or("the named NPC", |n| n.name.as_str())
            ),
            QuestObjective::Explore { area } => format!(
                "Explore {}. ",
                mmorpg_core::greyhaven_vale::areas()
                    .areas()
                    .iter()
                    .find(|a| a.id() == area)
                    .map_or("the objective area", |a| a.name())
            ),
        });
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmorpg_core::{ZoneId, ZoneSimulation, greyhaven_vale};

    #[test]
    fn every_catalog_offer_and_bag_slot_remains_reachable() {
        let content = greyhaven_vale::content();
        let mut zone = ZoneSimulation::with_content(ZoneId::new(1), content.clone()).unwrap();
        zone.add_player(1).unwrap();
        let snapshot = zone.snapshot_for_player(1).unwrap();
        let mut context = NpcContext::default();
        context.observe(&snapshot);
        let npc = NpcId::new(3);
        context.open(npc);
        for row in 0..mmorpg_core::INVENTORY_SLOTS {
            assert_eq!(context.row, row);
            let action = context.action(&content, false);
            if row < content.vendor_stock(npc).unwrap().offers().len() {
                assert_eq!(
                    action,
                    Some(ZoneCommand::BuyItem {
                        npc,
                        offer: row as u8,
                        quantity: 1
                    })
                );
            } else {
                assert_eq!(action, None);
            }
            let sale = context.action(&content, true);
            assert_eq!(
                sale.is_some(),
                snapshot.inventory.as_ref().unwrap().slots()[row].is_some()
            );
            context.navigate(&content, 1, false);
        }
        assert_eq!(context.row, 0);
    }

    #[test]
    fn new_bag_revision_cannot_reuse_an_omitted_older_sheet() {
        let content = greyhaven_vale::content();
        let mut zone = ZoneSimulation::with_content(ZoneId::new(1), content.clone()).unwrap();
        zone.add_player(1).unwrap();
        let mut snapshot = zone.snapshot_for_player(1).unwrap();
        let mut context = NpcContext::default();
        context.observe(&snapshot);
        context.open(NpcId::new(3));
        assert!(context.action(&content, true).is_some());
        snapshot.inventory_revision += 1;
        snapshot.inventory = None;
        snapshot.quests = None;
        context.observe(&snapshot);
        assert!(context.inventory.is_none());
        assert!(context.text(&content).contains("sheet pending"));
        assert_eq!(
            context.action(&content, true),
            Some(ZoneCommand::SellItem {
                npc: NpcId::new(3),
                bag_slot: 0,
                quantity: 1
            })
        );
        assert!(context.quests.is_none());
    }
}
