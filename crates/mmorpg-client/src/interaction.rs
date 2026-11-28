//! Selected-person context projected from immutable content and received self sheets.
//! This is navigation and intent composition; the zone validates every request.
use mmorpg_client_model::items::{item_name, sale};
use mmorpg_client_model::quest::{QuestStanding, quest_brief, standing};
use mmorpg_core::{Inventory, NpcId, NpcRole, QuestSheet, ZoneCommand, ZoneContent, ZoneSnapshot};

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
        let empty = QuestSheet::default();
        let sheet = self.quests.as_ref().unwrap_or(&empty);
        let mut rows: Vec<_> = content
            .quests()
            .iter()
            .filter(|q| q.giver == npc || q.ender == npc)
            .collect();
        rows.sort_by_key(|q| {
            let rank = match standing(sheet, q, npc) {
                QuestStanding::TurnIn => 0,
                QuestStanding::Offered => 1,
                _ => 2,
            };
            (rank, q.id)
        });
        rows
    }

    /// The viewer's standing with `quest` at the open NPC, once a sheet arrived.
    #[must_use]
    pub fn standing(&self, quest: &mmorpg_core::Quest) -> Option<QuestStanding> {
        Some(standing(self.quests.as_ref()?, quest, self.npc?))
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
        // An open decision (#1019) takes the dialog's two keys: 1 for its first option, 2 for its
        // second.
        if let Some(decision) = self.decision(content) {
            let option = if sell { 2 } else { 1 };
            return (usize::from(option) <= decision.options.len())
                .then_some(ZoneCommand::Decide { npc, option });
        }
        match content.npc(npc)?.role {
            NpcRole::QuestGiver if !sell => {
                let q = *self.quest_rows(content, npc).get(self.row)?;
                match self.standing(q)? {
                    QuestStanding::TurnIn => Some(ZoneCommand::CompleteQuest {
                        npc,
                        quest: q.id.get(),
                        choice: (self.choice % q.rewards.choices.len().max(1)) as u8,
                    }),
                    QuestStanding::Offered => Some(ZoneCommand::AcceptQuest {
                        npc,
                        quest: q.id.get(),
                    }),
                    _ => None,
                }
            }
            NpcRole::Vendor if sell => {
                // A sale is the whole stack (`mmorpg_client_model::items::sale`), so it waits for
                // the current bag sheet: an omitted newer sheet is loading, not an empty slot.
                let stack = (*self.inventory.as_ref()?.slots().get(self.row)?)?;
                Some(ZoneCommand::SellItem {
                    npc,
                    bag_slot: u8::try_from(self.row).ok()?,
                    quantity: sale(stack.item(), stack.quantity()).quantity,
                })
            }
            NpcRole::Vendor => {
                content.vendor_stock(npc)?.offers().get(self.row)?;
                Some(ZoneCommand::BuyItem {
                    npc,
                    offer: u32::try_from(self.row).ok()?,
                    quantity: 1,
                })
            }
            NpcRole::Guard | NpcRole::SpiritHealer | NpcRole::QuestGiver | NpcRole::Resident => {
                None
            }
        }
    }
    /// The decision (#1019) the open NPC awaits from the viewer, if any.
    #[must_use]
    pub fn decision(
        &self,
        content: &ZoneContent,
    ) -> Option<mmorpg_client_model::quest::DecisionView> {
        mmorpg_client_model::quest::decision_at(content, &self.quests.as_ref()?.entries, self.npc?)
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
            NpcRole::Resident => "A resident of Greyhaven.".into(),
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
                        |s| sale(s.item(), s.quantity()).text,
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
                    if self
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
                // An open decision (#1019) comes first: its options on the dialog's two keys.
                if let Some(decision) = self.decision(content) {
                    let quest = content
                        .quest(decision.quest)
                        .map_or("a quest", |quest| quest.name.as_str());
                    let options = decision
                        .options
                        .iter()
                        .take(2)
                        .enumerate()
                        .map(|(index, option)| format!("{} {option}", index + 1))
                        .collect::<Vec<_>>()
                        .join(" · ");
                    return format!("{} — {quest}: decide · {options} · Esc close", npc.name);
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
                    let state = match self.standing(q) {
                        Some(QuestStanding::Completed) => "completed",
                        Some(QuestStanding::TurnIn) => "1 Turn in (zone checks objectives)",
                        Some(QuestStanding::InProgress) => "in progress; return to its named ender",
                        Some(QuestStanding::Offered) => "available · 1 Accept",
                        Some(QuestStanding::OfferedElsewhere) => "available from its giver (H)",
                        Some(QuestStanding::Locked) | None => "locked by prerequisite",
                    };
                    let choices=q.rewards.choices.iter().enumerate().map(|(i,item)|format!("{}{}",if i==self.choice % q.rewards.choices.len().max(1) {">"} else {""},item_name(*item))).collect::<Vec<_>>().join(" / ");
                    format!("{} — {state} · rewards {} copper {} · Up/Down quest {}/{} · Left/Right reward · H dialogue/objectives",q.name,q.rewards.copper,choices,self.row+1,rows.len())
                })
            }
        };
        format!("{} — {detail} · {} · Esc close", npc.name, self.feedback)
    }
}
fn dialogue_pages(quest: &mmorpg_core::Quest, content: &ZoneContent) -> Vec<String> {
    quest_brief(quest, content)
        .chars()
        .collect::<Vec<_>>()
        .chunks(100)
        .map(|chunk| chunk.iter().collect())
        .collect()
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
                        offer: u32::try_from(row).unwrap(),
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
        let bag = snapshot.inventory.take();
        let stack = bag.as_ref().unwrap().slots()[0].unwrap();
        snapshot.inventory_revision += 1;
        snapshot.quests = None;
        context.observe(&snapshot);
        assert!(context.inventory.is_none());
        assert!(context.text(&content).contains("sheet pending"));
        // A sale is the whole stack, so it waits for the current sheet.
        assert_eq!(context.action(&content, true), None);
        snapshot.inventory = bag;
        context.observe(&snapshot);
        assert_eq!(
            context.action(&content, true),
            Some(ZoneCommand::SellItem {
                npc: NpcId::new(3),
                bag_slot: 0,
                quantity: stack.quantity()
            })
        );
        assert!(context.quests.is_none());
    }
}
