//! Windows of the game interface: chat, quest tracker/log, NPC dialogue,
//! vendor, bags, character and loot. Each control only names a shared command.
use super::{GameUi, LogTab, UiFrame, UiIntent, chat, line_color, panel_frame};
use crate::interaction::{NpcContext, quest_brief};
use egui::{Align2, Color32, RichText, Vec2};
use mmorpg_core::{
    EquipmentSlot, ItemId, NpcRole, Quest, QuestObjective, ZoneCommand, ZoneContent, ZoneSnapshot,
    item_template, sell_price,
};

fn item_name(id: ItemId) -> &'static str {
    item_template(id).map_or("unknown item", |item| item.name)
}

fn item_stats(id: ItemId) -> String {
    let Some(stats) = item_template(id).map(|item| item.stats) else {
        return String::new();
    };
    [
        (stats.stamina, "Stamina"),
        (stats.strength, "Strength"),
        (stats.agility, "Agility"),
        (stats.intellect, "Intellect"),
    ]
    .into_iter()
    .filter(|(value, _)| *value > 0)
    .map(|(value, name)| format!("+{value} {name}"))
    .collect::<Vec<_>>()
    .join(", ")
}

fn slot_title(slot: EquipmentSlot) -> &'static str {
    match slot {
        EquipmentSlot::MainHand => "Main hand",
        EquipmentSlot::OffHand => "Off hand",
        EquipmentSlot::Head => "Head",
        EquipmentSlot::Chest => "Chest",
        EquipmentSlot::Legs => "Legs",
        EquipmentSlot::Feet => "Feet",
    }
}

/// One objective as the tracker names it.
fn objective_text(objective: QuestObjective, content: &ZoneContent) -> String {
    match objective {
        QuestObjective::Kill { template, .. } => format!(
            "{} slain",
            content
                .creature_template(template)
                .map_or("Creatures", |t| t.name.as_str())
        ),
        QuestObjective::Collect { item, .. } | QuestObjective::Retrieve { item, .. } => {
            item_name(item).to_owned()
        }
        QuestObjective::Talk { npc } => format!(
            "Speak to {}",
            content
                .npc(npc)
                .map_or("the named NPC", |n| n.name.as_str())
        ),
        QuestObjective::Explore { area } => format!(
            "Explore {}",
            content
                .areas()
                .areas()
                .iter()
                .find(|a| a.id() == area)
                .map_or("the area", |a| a.name())
        ),
    }
}

fn quest_progress(ui: &mut egui::Ui, quest: &Quest, progress: &[u8], content: &ZoneContent) {
    for (index, objective) in quest.objectives.iter().enumerate() {
        let current = progress.get(index).copied().unwrap_or(0);
        let required = objective.required();
        let done = current >= required;
        ui.small(
            RichText::new(format!(
                "  {} {current}/{required}",
                objective_text(*objective, content)
            ))
            .color(if done {
                Color32::from_rgb(140, 220, 120)
            } else {
                Color32::from_gray(220)
            }),
        );
    }
}

fn completed(sheet: &mmorpg_core::QuestSheet, quest: mmorpg_core::QuestId) -> bool {
    quest.bit().is_some_and(|bit| sheet.completed & bit != 0)
}

impl GameUi {
    pub(super) fn chat_frame(&mut self, ctx: &egui::Context, intents: &mut Vec<UiIntent>) {
        egui::Area::new(egui::Id::unique("chat"))
            .anchor(Align2::LEFT_BOTTOM, Vec2::new(12.0, -12.0))
            .show(ctx, |ui| {
                panel_frame().show(ui, |ui| {
                    ui.set_width(300.0);
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.log_tab, LogTab::Chat, "Chat");
                        ui.selectable_value(&mut self.log_tab, LogTab::Combat, "Combat");
                    });
                    let lines = match self.log_tab {
                        LogTab::Chat => &self.logs.chat,
                        LogTab::Combat => &self.logs.combat,
                    };
                    egui::ScrollArea::vertical()
                        .max_height(130.0)
                        .stick_to_bottom(true)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            for line in lines {
                                ui.label(
                                    RichText::new(&line.text)
                                        .size(12.5)
                                        .color(line_color(line.kind)),
                                );
                            }
                        });
                    let edit = ui.add(
                        egui::TextEdit::singleline(&mut self.chat_text)
                            .hint_text("Enter to chat · /s /y /wave /bow /cheer /laugh /point")
                            .desired_width(f32::INFINITY),
                    );
                    if self.chat_focus_requested {
                        edit.request_focus();
                        self.chat_focus_requested = false;
                    }
                    if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        let raw = std::mem::take(&mut self.chat_text);
                        if !raw.trim().is_empty() {
                            match chat::parse_chat_input(&raw) {
                                Ok(command) => intents.push(UiIntent::Command(command)),
                                Err(problem) => {
                                    self.logs.note(chat::LineKind::System, problem);
                                }
                            }
                        }
                    }
                    self.chat_focused = edit.has_focus();
                });
            });
    }

    pub(super) fn quest_tracker(&self, ui: &mut egui::Ui, frame: &UiFrame<'_>) {
        let Some(sheet) = &self.sheets.quests else {
            return;
        };
        if sheet.entries.is_empty() {
            return;
        }
        ui.separator();
        ui.label(RichText::new("Quests").strong());
        for entry in &sheet.entries {
            let Some(quest) = frame.content.quest(entry.quest) else {
                continue;
            };
            let ready = quest
                .objectives
                .iter()
                .enumerate()
                .all(|(i, o)| entry.progress.get(i).copied().unwrap_or(0) >= o.required());
            ui.label(
                RichText::new(if ready {
                    format!("{} (ready to turn in)", quest.name)
                } else {
                    quest.name.clone()
                })
                .color(Color32::from_rgb(255, 210, 0)),
            );
            quest_progress(ui, quest, &entry.progress, frame.content);
        }
    }

    pub(super) fn quest_log(
        &mut self,
        ctx: &egui::Context,
        frame: &UiFrame<'_>,
        intents: &mut Vec<UiIntent>,
    ) {
        let mut open = self.quest_log_open;
        egui::Window::new("Quest log")
            .open(&mut open)
            .default_pos([300.0, 120.0])
            .default_width(380.0)
            .show(ctx, |ui| {
                let Some(sheet) = &self.sheets.quests else {
                    ui.label("Waiting for the current quest sheet…");
                    return;
                };
                if sheet.entries.is_empty() {
                    ui.label("Your quest log is empty. Talk to people marked ! to find work.");
                }
                for entry in &sheet.entries {
                    let Some(quest) = frame.content.quest(entry.quest) else {
                        continue;
                    };
                    let selected = self.selected_quest == Some(quest.id);
                    if ui.selectable_label(selected, &quest.name).clicked() {
                        self.selected_quest = Some(quest.id);
                    }
                    if selected {
                        ui.label(quest_brief(quest, frame.content));
                        quest_progress(ui, quest, &entry.progress, frame.content);
                        if ui.button("Abandon quest").clicked() {
                            intents.push(UiIntent::Command(ZoneCommand::AbandonQuest {
                                quest: quest.id.get(),
                            }));
                        }
                        ui.separator();
                    }
                }
                ui.small(format!(
                    "{} of {} quests in the log · {} completed",
                    sheet.entries.len(),
                    mmorpg_core::MAX_QUEST_LOG,
                    sheet.completed.count_ones()
                ));
            });
        self.quest_log_open = open;
    }

    pub(super) fn npc_dialog(
        &mut self,
        ctx: &egui::Context,
        frame: &UiFrame<'_>,
        snapshot: &ZoneSnapshot,
        context: &mut NpcContext,
        intents: &mut Vec<UiIntent>,
    ) {
        let Some(npc) = context.npc.and_then(|id| frame.content.npc(id)) else {
            return;
        };
        let content = frame.content;
        let mut open = true;
        egui::Window::new(npc.name.as_str())
            .id(egui::Id::unique("npc-dialog"))
            .open(&mut open)
            .default_pos([300.0, 140.0])
            .default_width(420.0)
            .show(ctx, |ui| {
                // A row is the same navigation the keyboard context uses, so
                // every click names the same command its keys would.
                let mut act = |context: &mut NpcContext, row: usize, sell: bool| {
                    context.row = row;
                    if let Some(command) = context.action(content, sell) {
                        context.feedback = "request sent".into();
                        intents.push(UiIntent::Command(command));
                    }
                };
                match npc.role {
                    NpcRole::QuestGiver => {
                        let Some(sheet) = context.quests.clone() else {
                            ui.label("Waiting for the current quest sheet…");
                            return;
                        };
                        let rows: Vec<&Quest> = context.quest_rows(content, npc.id);
                        if rows.is_empty() {
                            ui.label("No quests here.");
                        }
                        for (index, quest) in rows.iter().enumerate() {
                            let entry = sheet.entries.iter().find(|e| e.quest == quest.id);
                            let done = completed(&sheet, quest.id);
                            let unlocked = quest.prerequisite.is_none_or(|p| completed(&sheet, p));
                            let (state, action) = if done {
                                ("completed", None)
                            } else if entry.is_some() && quest.ender == npc.id {
                                ("ready to turn in?", Some("Complete quest"))
                            } else if entry.is_some() {
                                ("in progress", None)
                            } else if unlocked && quest.giver == npc.id {
                                ("available", Some("Accept"))
                            } else if unlocked {
                                ("offered by another", None)
                            } else {
                                ("locked", None)
                            };
                            let selected = context.row == index;
                            if ui
                                .selectable_label(selected, format!("{} — {state}", quest.name))
                                .clicked()
                            {
                                context.row = index;
                                context.choice = 0;
                            }
                            if !selected {
                                continue;
                            }
                            ui.label(quest_brief(quest, content));
                            if let Some(entry) = entry {
                                quest_progress(ui, quest, &entry.progress, content);
                            }
                            ui.label(format!(
                                "Rewards: {} XP, {} copper",
                                quest.rewards.experience, quest.rewards.copper
                            ));
                            if !quest.rewards.choices.is_empty() {
                                ui.horizontal_wrapped(|ui| {
                                    ui.label("Choose one:");
                                    for (choice, item) in quest.rewards.choices.iter().enumerate() {
                                        let label =
                                            format!("{} {}", item_name(*item), item_stats(*item));
                                        if ui
                                            .selectable_label(
                                                context.choice == choice,
                                                label.trim(),
                                            )
                                            .clicked()
                                        {
                                            context.choice = choice;
                                        }
                                    }
                                });
                            }
                            if let Some(label) = action
                                && ui.button(label).clicked()
                            {
                                act(context, index, false);
                            }
                            ui.separator();
                        }
                    }
                    NpcRole::Vendor => {
                        ui.label(format!("You have {} copper.", snapshot.viewer.copper));
                        ui.label(RichText::new("For sale").strong());
                        if let Some(stock) = content.vendor_stock(npc.id) {
                            for (index, offer) in stock.offers().iter().enumerate() {
                                ui.horizontal(|ui| {
                                    ui.label(format!(
                                        "{} {} — {} copper",
                                        item_name(offer.item),
                                        item_stats(offer.item),
                                        offer.price
                                    ));
                                    let affordable = snapshot.viewer.copper >= offer.price;
                                    if ui
                                        .add_enabled(affordable, egui::Button::new("Buy"))
                                        .clicked()
                                    {
                                        act(context, index, false);
                                    }
                                });
                            }
                        }
                        ui.separator();
                        ui.label(RichText::new("Sell from your bags").strong());
                        match context.inventory.clone() {
                            None => {
                                ui.label("Waiting for the current bag sheet…");
                            }
                            Some(bag) => {
                                for (index, stack) in bag.slots().iter().enumerate() {
                                    let Some(stack) = stack else { continue };
                                    ui.horizontal(|ui| {
                                        ui.label(format!(
                                            "{} ×{} — {} copper each",
                                            item_name(stack.item()),
                                            stack.quantity(),
                                            sell_price(stack.item())
                                        ));
                                        if ui.button("Sell one").clicked() {
                                            act(context, index, true);
                                        }
                                    });
                                }
                            }
                        }
                    }
                    NpcRole::Guard | NpcRole::SpiritHealer => {
                        ui.label(format!("{} has nothing to offer.", npc.name));
                    }
                }
                if !context.feedback.is_empty() {
                    ui.colored_label(Color32::from_rgb(255, 210, 120), &context.feedback);
                }
            });
        if !open {
            intents.push(UiIntent::CloseContext);
        }
    }

    pub(super) fn bags(
        &mut self,
        ctx: &egui::Context,
        _frame: &UiFrame<'_>,
        snapshot: &ZoneSnapshot,
        intents: &mut Vec<UiIntent>,
    ) {
        let mut open = self.bags_open;
        egui::Window::new("Bags")
            .open(&mut open)
            .default_pos([960.0, 330.0])
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(format!("{} copper", snapshot.viewer.copper));
                let Some(bag) = self.sheets.inventory.clone() else {
                    ui.label("Waiting for the current bag sheet…");
                    return;
                };
                egui::Grid::new("bag-grid")
                    .spacing([4.0, 4.0])
                    .show(ui, |ui| {
                        for (index, stack) in bag.slots().iter().enumerate() {
                            let slot = index as u8;
                            let text = stack.map_or_else(String::new, |s| {
                                let name: String = item_name(s.item()).chars().take(10).collect();
                                if s.quantity() > 1 {
                                    format!("{name}\n×{}", s.quantity())
                                } else {
                                    name
                                }
                            });
                            let button = egui::Button::new(RichText::new(text).size(11.0))
                                .min_size(Vec2::new(70.0, 44.0))
                                .selected(self.selected_bag == Some(slot));
                            let mut response = ui.add(button);
                            if let Some(stack) = stack {
                                response = response.on_hover_text(format!(
                                    "{} ×{} {}",
                                    item_name(stack.item()),
                                    stack.quantity(),
                                    item_stats(stack.item())
                                ));
                            }
                            if response.clicked() {
                                match self.selected_bag {
                                    Some(source) if source == slot => self.selected_bag = None,
                                    Some(source) => {
                                        // Move, merge or split onto the clicked slot; the zone settles it.
                                        let available = bag.slots()[usize::from(source)]
                                            .map_or(0, |s| s.quantity());
                                        intents.push(UiIntent::Command(ZoneCommand::MoveItem {
                                            source,
                                            destination: slot,
                                            quantity: self.move_quantity.clamp(1, available.max(1)),
                                        }));
                                        self.selected_bag = None;
                                    }
                                    None if stack.is_some() => {
                                        self.selected_bag = Some(slot);
                                        self.move_quantity = stack.map_or(1, |s| s.quantity());
                                    }
                                    None => {}
                                }
                            }
                            if index % 4 == 3 {
                                ui.end_row();
                            }
                        }
                    });
                if let Some(source) = self.selected_bag
                    && let Some(stack) = bag.slots()[usize::from(source)]
                {
                    ui.separator();
                    ui.label(format!(
                        "{} selected — click another slot to move, merge or split.",
                        item_name(stack.item())
                    ));
                    if stack.quantity() > 1 {
                        ui.add(
                            egui::Slider::new(&mut self.move_quantity, 1..=stack.quantity())
                                .text("quantity"),
                        );
                    }
                    if item_template(stack.item()).is_some_and(|t| t.slot.is_some())
                        && ui
                            .button(format!("Equip {}", item_name(stack.item())))
                            .clicked()
                    {
                        intents.push(UiIntent::Command(ZoneCommand::EquipItem {
                            bag_slot: source,
                        }));
                        self.selected_bag = None;
                    }
                }
            });
        self.bags_open = open;
    }

    pub(super) fn character(
        &mut self,
        ctx: &egui::Context,
        _frame: &UiFrame<'_>,
        snapshot: &ZoneSnapshot,
        intents: &mut Vec<UiIntent>,
    ) {
        let mut open = self.character_open;
        egui::Window::new("Character")
            .open(&mut open)
            .default_pos([760.0, 130.0])
            .resizable(false)
            .show(ctx, |ui| {
                let viewer = &snapshot.viewer;
                let Some(equipment) = self.sheets.equipment else {
                    ui.label("Waiting for the current equipment sheet…");
                    return;
                };
                egui::Grid::new("equipment").striped(true).show(ui, |ui| {
                    for slot in EquipmentSlot::ALL {
                        ui.label(slot_title(slot));
                        match equipment.item(slot) {
                            Some(item) => {
                                ui.label(format!("{} {}", item_name(item), item_stats(item)));
                                if ui.button("Unequip").clicked() {
                                    intents.push(UiIntent::Command(ZoneCommand::UnequipItem {
                                        equipment_slot: slot.index(),
                                    }));
                                }
                            }
                            None => {
                                ui.label(RichText::new("Empty").weak());
                                ui.label("");
                            }
                        }
                        ui.end_row();
                    }
                });
                ui.separator();
                let totals = equipment.totals();
                egui::Grid::new("stats").show(ui, |ui| {
                    for (name, value) in [
                        ("Stamina", totals.stamina),
                        ("Strength", totals.strength),
                        ("Agility", totals.agility),
                        ("Intellect", totals.intellect),
                    ] {
                        ui.label(name);
                        ui.label(value.to_string());
                        ui.end_row();
                    }
                    ui.label("Health");
                    ui.label(format!("{} / {}", viewer.health, viewer.max_health));
                    ui.end_row();
                    ui.label("Damage");
                    ui.label(format!("{}–{}", viewer.damage[0], viewer.damage[1]));
                    ui.end_row();
                    ui.label("Level");
                    ui.label(viewer.level.to_string());
                    ui.end_row();
                });
            });
        self.character_open = open;
    }

    pub(super) fn loot(
        &mut self,
        ctx: &egui::Context,
        snapshot: &ZoneSnapshot,
        intents: &mut Vec<UiIntent>,
    ) {
        let Some(view) = snapshot.loot else { return };
        if self.dismissed_loot == Some(view.claim) {
            return;
        }
        let mut open = true;
        egui::Window::new("Loot")
            .open(&mut open)
            .default_pos([520.0, 300.0])
            .resizable(false)
            .show(ctx, |ui| {
                let rewards = view.rewards;
                if rewards.money > 0 {
                    ui.label(format!("{} copper", rewards.money));
                }
                for stack in [rewards.item, rewards.quest_item].into_iter().flatten() {
                    ui.label(format!("{} ×{}", item_name(stack.item()), stack.quantity()));
                }
                ui.small("Stand within 3 m of the corpse to take it.");
                if ui.button("Take all").clicked() {
                    intents.push(UiIntent::Command(ZoneCommand::Loot(view.claim)));
                }
            });
        if !open {
            self.dismissed_loot = Some(view.claim);
        }
    }
}
