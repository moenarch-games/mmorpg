//! The native game interface, composed with external egui (Atlas 6.1.4, #183).
//!
//! Everything shown is read from the received player-scoped projection, the
//! retained self sheets and immutable content; every control produces an
//! existing intent ([`UiIntent`]) that the window turns into the same shared
//! commands as the keyboard. The zone decides what happens; this module owns
//! no gameplay legality, quest progress or inventory transactions.
pub mod chat;
pub mod minimap;
pub mod overlay;
mod panels;
pub mod world_labels;

use crate::{
    hud::{CombatHud, CooldownKind},
    interaction::NpcContext,
    roster::{Headwear, MAX_CHARACTERS, Roster},
};
use chat::{LineKind, Logs};
use egui::{Align2, Color32, RichText, Vec2};
use minimap::MinimapLayers;
use mmorpg_core::{
    EntityKind, EntityRef, Equipment, Inventory, LootClaim, PlayerClass, QuestSheet, ResourceKind,
    Sex, ZoneCommand, ZoneContent, ZoneSnapshot, ability_by_id,
};
use mmorpg_scenery::Scenery;
use std::time::{Duration, Instant};

/// What an interface control asks the window to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiIntent {
    /// One deliberate shared command (quest, vendor, bag, equipment, loot, chat).
    Command(ZoneCommand),
    /// Use action-bar slot 1–4.
    AbilitySlot(u8),
    /// Start auto-attacking the target, or stop.
    ToggleAttack,
    ReleaseSpirit,
    /// Request a selection; `None` clears it.
    Select(Option<EntityRef>),
    /// Open the selected NPC's context.
    Talk,
    /// Close the NPC context.
    CloseContext,
    /// The chosen class, sex and local headwear to enter the world with.
    ChooseCharacter(PlayerClass, Sex, Headwear),
}

/// The latest self sheets: an omitted newer sheet is pending, never empty.
#[derive(Default)]
pub struct Sheets {
    revision: Option<u64>,
    pub inventory: Option<Inventory>,
    pub equipment: Option<Equipment>,
    pub quests: Option<QuestSheet>,
}

impl Sheets {
    fn observe(&mut self, snapshot: &ZoneSnapshot) {
        if self.revision != Some(snapshot.inventory_revision) {
            *self = Self {
                revision: Some(snapshot.inventory_revision),
                ..Self::default()
            };
        }
        if let Some(inventory) = &snapshot.inventory {
            self.inventory = Some(inventory.clone());
        }
        if let Some(equipment) = snapshot.equipment {
            self.equipment = Some(equipment);
        }
        if let Some(quests) = &snapshot.quests {
            self.quests = Some(quests.clone());
        }
    }
}

/// Read-only inputs of one interface frame.
pub struct UiFrame<'a> {
    pub snapshot: Option<&'a ZoneSnapshot>,
    pub content: &'a ZoneContent,
    pub hud: Option<&'a CombatHud>,
    /// Display name of a unit.
    pub names: &'a dyn Fn(EntityRef) -> String,
    /// The camera's horizontal heading in world XZ.
    pub forward: [f32; 2],
    /// Connection problems shown instead of the world state.
    pub connection: Option<&'a str>,
    /// The character still needs a class and sex.
    pub choosing: bool,
    /// Nameplate anchors of the presented units.
    pub labels: &'a [world_labels::UnitLabel],
    /// World point to screen point, as the last presented frame.
    pub project: world_labels::Project<'a>,
    /// The camera eye in world metres.
    pub eye: [f32; 3],
}

/// The character being previewed on the turntable while choosing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CharacterPreview {
    pub choice: mmorpg_core::ClassChoice,
    pub headwear: Headwear,
    /// Turntable rotation away from facing the camera.
    pub degrees: f32,
}

/// The browser's turntable step (`TURN_STEP_DEGREES`).
const TURN_STEP_DEGREES: f32 = 15.0;

/// The local character roster window (#183): pick, create, re-hat or delete
/// a locally kept character, previewed on a turntable.
struct RosterWindow {
    roster: Roster,
    /// Why the roster could not be loaded or saved; loading failures keep the
    /// file untouched and the roster in memory.
    problem: Option<String>,
    /// `None` shows the creation form.
    selected: Option<usize>,
    name: String,
    class: PlayerClass,
    sex: Sex,
    headwear: Headwear,
    degrees: f32,
    confirm_delete: bool,
    name_focused: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum LogTab {
    #[default]
    Chat,
    Combat,
}

/// Interface state that is purely local presentation.
pub struct GameUi {
    pub sheets: Sheets,
    pub logs: Logs,
    minimap: MinimapLayers,
    pub bags_open: bool,
    pub character_open: bool,
    pub quest_log_open: bool,
    chat_text: String,
    chat_focus_requested: bool,
    chat_focused: bool,
    log_tab: LogTab,
    selected_bag: Option<u8>,
    move_quantity: u16,
    selected_quest: Option<mmorpg_core::QuestId>,
    zoom: usize,
    refusal: Option<(String, Instant)>,
    roster: RosterWindow,
    /// The entered local character's name, shown in the player frame.
    pub character_name: Option<String>,
    floating: world_labels::FloatingTexts,
    dismissed_loot: Option<LootClaim>,
}

const REFUSAL_SECONDS: Duration = Duration::from_secs(3);

impl GameUi {
    #[must_use]
    pub fn new(scenery: &Scenery) -> Self {
        Self {
            sheets: Sheets::default(),
            logs: Logs::default(),
            minimap: MinimapLayers::new(scenery),
            bags_open: false,
            character_open: false,
            quest_log_open: false,
            chat_text: String::new(),
            chat_focus_requested: false,
            chat_focused: false,
            log_tab: LogTab::default(),
            selected_bag: None,
            move_quantity: 1,
            selected_quest: None,
            zoom: 1,
            refusal: None,
            roster: RosterWindow {
                roster: Roster::default(),
                problem: None,
                selected: None,
                name: String::new(),
                class: PlayerClass::Warden,
                sex: Sex::Male,
                headwear: Headwear::for_class(PlayerClass::Warden),
                degrees: 0.0,
                confirm_delete: false,
                name_focused: false,
            },
            character_name: None,
            floating: world_labels::FloatingTexts::default(),
            dismissed_loot: None,
        }
    }

    /// Uses the loaded local roster, or shows why it could not be loaded and
    /// keeps new characters in memory only.
    pub fn set_roster(&mut self, roster: Result<Roster, String>) {
        let window = &mut self.roster;
        match roster {
            Ok(roster) => {
                window.selected = (!roster.characters.is_empty()).then_some(0);
                window.roster = roster;
                window.problem = None;
            }
            Err(problem) => {
                window.roster = Roster::default();
                window.selected = None;
                window.problem = Some(format!("{problem} New characters are not saved."));
            }
        }
    }

    /// The character previewed on the turntable while choosing.
    #[must_use]
    pub fn preview(&self) -> CharacterPreview {
        let window = &self.roster;
        let (class, sex, headwear) = match window
            .selected
            .and_then(|index| window.roster.characters.get(index))
        {
            Some(character) => (character.class, character.sex, character.headwear),
            None => (window.class, window.sex, window.headwear),
        };
        CharacterPreview {
            choice: mmorpg_core::ClassChoice { class, sex },
            headwear,
            degrees: window.degrees,
        }
    }

    /// A new connection epoch: sheets and pending interface choices restart.
    pub fn reset(&mut self) {
        self.floating.clear();
        self.sheets = Sheets::default();
        self.selected_bag = None;
        self.refusal = None;
        self.dismissed_loot = None;
    }

    /// Takes one received projection's sheets, chat and feedback.
    pub fn observe(
        &mut self,
        snapshot: &ZoneSnapshot,
        content: &ZoneContent,
        names: &dyn Fn(EntityRef) -> String,
        now: Instant,
    ) {
        self.sheets.observe(snapshot);
        let refusals = self.logs.receive(snapshot, names, |quest| {
            content
                .quest(quest)
                .map_or_else(|| "A quest".into(), |q| q.name.clone())
        });
        if let Some(text) = refusals.last() {
            self.refusal = Some((text.clone(), now));
        }
        if snapshot.loot.is_none() {
            self.dismissed_loot = None;
        }
        self.floating.observe(snapshot, now);
    }

    /// Whether a text field (chat or a new character's name) owns the keyboard.
    #[must_use]
    pub fn chat_focused(&self) -> bool {
        self.chat_focused || self.chat_focus_requested || self.roster.name_focused
    }

    pub fn open_chat(&mut self) {
        self.chat_focus_requested = true;
    }

    pub fn toggle(&mut self, panel: crate::controls::Panel) {
        use crate::controls::Panel;
        let open = match panel {
            Panel::Bags => &mut self.bags_open,
            Panel::Character => &mut self.character_open,
            Panel::QuestLog => &mut self.quest_log_open,
        };
        *open = !*open;
    }

    /// Escape closes open windows first, as the browser's `ui.closePanel`;
    /// returns whether any was open.
    pub fn close_panels(&mut self) -> bool {
        let open = self.bags_open || self.character_open || self.quest_log_open;
        self.bags_open = false;
        self.character_open = false;
        self.quest_log_open = false;
        self.selected_bag = None;
        open
    }

    /// Whether a closable window (bags, character, quest log or an
    /// undismissed loot window) is open, so a gamepad navigates the interface.
    #[must_use]
    pub fn windows_open(&self, snapshot: Option<&ZoneSnapshot>) -> bool {
        self.bags_open
            || self.character_open
            || self.quest_log_open
            || snapshot
                .and_then(|snapshot| snapshot.loot)
                .is_some_and(|loot| self.dismissed_loot != Some(loot.claim))
    }

    /// The gamepad's close: open windows first, then an undismissed loot
    /// window; returns whether anything closed.
    pub fn close_window(&mut self, snapshot: Option<&ZoneSnapshot>) -> bool {
        if self.close_panels() {
            return true;
        }
        match snapshot.and_then(|snapshot| snapshot.loot) {
            Some(loot) if self.dismissed_loot != Some(loot.claim) => {
                self.dismissed_loot = Some(loot.claim);
                true
            }
            _ => false,
        }
    }

    /// Lays out one interface frame and returns the intents its controls produced.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        frame: &UiFrame<'_>,
        npc: &mut NpcContext,
        now: Instant,
    ) -> Vec<UiIntent> {
        let mut intents = Vec::new();
        if let Some(text) = frame.connection {
            banner(
                ctx,
                "connection",
                text,
                Color32::from_rgb(255, 200, 80),
                40.0,
            );
        }
        if frame.choosing {
            self.character_selection(ctx, &mut intents);
        } else {
            self.roster.name_focused = false;
        }
        let Some(snapshot) = frame.snapshot else {
            return intents;
        };
        world_labels::paint(
            ctx,
            snapshot,
            frame.labels,
            frame.project,
            frame.eye,
            frame.names,
            self.sheets.quests.as_ref(),
            &mut self.floating,
            now,
        );
        self.unit_frames(ctx, frame, snapshot, &mut intents);
        self.action_bar(ctx, frame, snapshot, &mut intents);
        self.minimap_and_tracker(ctx, frame, snapshot);
        self.chat_frame(ctx, &mut intents);
        if let Some((text, at)) = &self.refusal
            && now.saturating_duration_since(*at) < REFUSAL_SECONDS
        {
            banner(ctx, "refusal", text, Color32::from_rgb(255, 80, 70), 80.0);
        }
        if snapshot.viewer.dead {
            egui::Window::new("You died")
                .anchor(Align2::CENTER_TOP, Vec2::new(0.0, 120.0))
                .resizable(false)
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.label("Your spirit waits in the world.");
                    if ui.button("Release spirit (R)").clicked() {
                        intents.push(UiIntent::ReleaseSpirit);
                    }
                });
        }
        self.bags(ctx, frame, snapshot, &mut intents);
        self.character(ctx, frame, snapshot, &mut intents);
        self.quest_log(ctx, frame, &mut intents);
        self.npc_dialog(ctx, frame, snapshot, npc, &mut intents);
        self.loot(ctx, snapshot, &mut intents);
        intents
    }

    fn character_selection(&mut self, ctx: &egui::Context, intents: &mut Vec<UiIntent>) {
        let window = &mut self.roster;
        let mut entered = None;
        egui::Window::new("Characters")
            .anchor(Align2::LEFT_CENTER, Vec2::new(24.0, 0.0))
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.set_width(300.0);
                if let Some(problem) = &window.problem {
                    ui.colored_label(Color32::from_rgb(255, 150, 90), problem);
                }
                for (index, character) in window.roster.characters.iter().enumerate() {
                    let text = format!(
                        "{} · level 1 {}",
                        character.name,
                        class_title(character.class)
                    );
                    if ui
                        .selectable_label(window.selected == Some(index), text)
                        .clicked()
                    {
                        window.selected = Some(index);
                        window.confirm_delete = false;
                    }
                }
                let full = window.roster.characters.len() >= MAX_CHARACTERS;
                if ui
                    .add_enabled(
                        !full,
                        egui::Button::selectable(
                            window.selected.is_none(),
                            "+ Create a new character",
                        ),
                    )
                    .clicked()
                {
                    window.selected = None;
                }
                ui.small(format!(
                    "{} / {MAX_CHARACTERS} local characters",
                    window.roster.characters.len()
                ));
                ui.separator();
                window.name_focused = false;
                match window.selected {
                    Some(index) if index < window.roster.characters.len() => {
                        let character = window.roster.characters[index].clone();
                        ui.label(RichText::new(&character.name).strong().size(18.0));
                        ui.label(format!(
                            "Human {} {} · Greyhaven Outpost",
                            sex_title(character.sex),
                            class_title(character.class)
                        ));
                        ui.label("Headwear");
                        ui.horizontal(|ui| {
                            for hat in Headwear::ALL {
                                if ui
                                    .selectable_label(character.headwear == hat, hat.title())
                                    .clicked()
                                    && hat != character.headwear
                                    && let Err(problem) = window.roster.set_headwear(index, hat)
                                {
                                    window.problem = Some(problem);
                                }
                            }
                        });
                        turntable(ui, &mut window.degrees);
                        ui.separator();
                        ui.horizontal(|ui| {
                            if ui
                                .add(egui::Button::new(RichText::new("Enter world").strong()))
                                .clicked()
                            {
                                entered = Some(character.clone());
                            }
                            let delete = if window.confirm_delete {
                                "Confirm delete"
                            } else {
                                "Delete"
                            };
                            if ui.button(delete).clicked() {
                                if window.confirm_delete {
                                    match window.roster.delete(index) {
                                        Ok(()) => {
                                            window.selected =
                                                (!window.roster.characters.is_empty()).then_some(0);
                                        }
                                        Err(problem) => window.problem = Some(problem),
                                    }
                                    window.confirm_delete = false;
                                } else {
                                    window.confirm_delete = true;
                                }
                            }
                        });
                    }
                    _ => {
                        window.selected = None;
                        ui.label("Name");
                        let edit = ui.add(
                            egui::TextEdit::singleline(&mut window.name)
                                .hint_text("2–24 letters")
                                .char_limit(48)
                                .desired_width(f32::INFINITY),
                        );
                        window.name_focused = edit.has_focus();
                        ui.label("Class");
                        ui.horizontal(|ui| {
                            for class in PlayerClass::ALL {
                                if ui
                                    .selectable_value(&mut window.class, class, class_title(class))
                                    .clicked()
                                {
                                    window.headwear = Headwear::for_class(class);
                                }
                            }
                        });
                        ui.small(match window.class {
                            PlayerClass::Warden => "A shielded melee fighter who builds rage.",
                            PlayerClass::Ranger => "A ranged archer who spends focus.",
                            PlayerClass::Arcanist => "A spellcaster who spends mana.",
                        });
                        ui.label("Body");
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut window.sex, Sex::Female, "Female");
                            ui.selectable_value(&mut window.sex, Sex::Male, "Male");
                        });
                        ui.label("Headwear");
                        ui.horizontal(|ui| {
                            for hat in Headwear::ALL {
                                ui.selectable_value(&mut window.headwear, hat, hat.title());
                            }
                        });
                        turntable(ui, &mut window.degrees);
                        ui.separator();
                        if ui
                            .add_enabled(
                                !full,
                                egui::Button::new(RichText::new("Create character").strong()),
                            )
                            .clicked()
                        {
                            match window.roster.create(
                                &window.name,
                                window.class,
                                window.sex,
                                window.headwear,
                            ) {
                                Ok(index) => {
                                    window.selected = Some(index);
                                    window.name.clear();
                                    window.problem = None;
                                }
                                Err(problem) => window.problem = Some(problem),
                            }
                        }
                    }
                }
            });
        if let Some(character) = entered {
            self.roster.name_focused = false;
            self.character_name = Some(character.name.clone());
            intents.push(UiIntent::ChooseCharacter(
                character.class,
                character.sex,
                character.headwear,
            ));
        }
    }

    fn unit_frames(
        &mut self,
        ctx: &egui::Context,
        frame: &UiFrame<'_>,
        snapshot: &ZoneSnapshot,
        intents: &mut Vec<UiIntent>,
    ) {
        let viewer = &snapshot.viewer;
        egui::Area::new(egui::Id::unique("player-frame"))
            .anchor(Align2::LEFT_TOP, Vec2::new(12.0, 12.0))
            .show(ctx, |ui| {
                panel_frame().show(ui, |ui| {
                    ui.set_width(230.0);
                    let class = viewer.class.map_or("Adventurer", |c| class_title(c.class));
                    let name = self.character_name.as_deref().unwrap_or("You");
                    ui.label(
                        RichText::new(format!("{name} · level {} {class}", viewer.level)).strong(),
                    );
                    bar(
                        ui,
                        fraction(viewer.health, viewer.max_health),
                        format!("{} / {}", viewer.health, viewer.max_health),
                        Color32::from_rgb(40, 170, 60),
                    );
                    if let Some(resource) = viewer.resource {
                        bar(
                            ui,
                            fraction(u32::from(resource.value), u32::from(resource.max)),
                            format!(
                                "{} {} / {}",
                                resource.kind.name(),
                                resource.value,
                                resource.max
                            ),
                            resource_color(resource.kind),
                        );
                    }
                    if viewer.experience_to_next_level > 0 {
                        bar(
                            ui,
                            fraction(viewer.experience, viewer.experience_to_next_level),
                            format!(
                                "XP {} / {}",
                                viewer.experience, viewer.experience_to_next_level
                            ),
                            Color32::from_rgb(140, 80, 200),
                        );
                    }
                    ui.horizontal(|ui| {
                        ui.label(format!("{} copper", viewer.copper));
                        if viewer.in_combat {
                            ui.colored_label(Color32::from_rgb(255, 120, 90), "in combat");
                        }
                    });
                    if let Some(stance) = viewer.stance {
                        ui.colored_label(Color32::from_rgb(230, 200, 120), stance_text(stance));
                    }
                    auras(ui, &snapshot.auras);
                });
            });
        let Some(target) = viewer.target else { return };
        let unit = snapshot.entities.iter().find(|e| e.entity() == target);
        egui::Area::new(egui::Id::unique("target-frame"))
            .anchor(Align2::LEFT_TOP, Vec2::new(270.0, 12.0))
            .show(ctx, |ui| {
                panel_frame().show(ui, |ui| {
                    ui.set_width(230.0);
                    let name = (frame.names)(target);
                    match unit {
                        Some(unit) => {
                            let mut title = format!("{name} · level {}", unit.level);
                            if unit.flags.tapped_by_other {
                                title.push_str(" · tapped");
                            }
                            let color = if unit.flags.hostile {
                                Color32::from_rgb(255, 110, 100)
                            } else if unit.kind == EntityKind::Npc {
                                Color32::from_rgb(120, 220, 120)
                            } else {
                                Color32::from_rgb(255, 230, 120)
                            };
                            ui.label(RichText::new(title).strong().color(color));
                            if unit.flags.dead {
                                ui.label("Dead");
                            } else {
                                bar(
                                    ui,
                                    f32::from(unit.health_percent) / 100.0,
                                    format!("{}%", unit.health_percent),
                                    Color32::from_rgb(40, 170, 60),
                                );
                            }
                        }
                        None => {
                            ui.label(RichText::new(format!("{name} · out of sight")).strong());
                        }
                    }
                    if let Some(cast) = snapshot.target_detail.cast {
                        cast_bar(ui, cast);
                    }
                    auras(ui, &snapshot.target_detail.auras);
                    if let Some(tot) = snapshot.target_of_target {
                        ui.small(format!("Target: {}", (frame.names)(tot)));
                    }
                    ui.horizontal(|ui| {
                        if unit.is_some_and(|u| u.flags.attackable) {
                            let label = if viewer.auto_attacking {
                                "Stop attack (F)"
                            } else {
                                "Attack (F)"
                            };
                            if ui.button(label).clicked() {
                                intents.push(UiIntent::ToggleAttack);
                            }
                        }
                        if matches!(target, EntityRef::Npc(_))
                            && ui.button("Talk (Enter)").clicked()
                        {
                            intents.push(UiIntent::Talk);
                        }
                        if unit.is_some_and(|u| u.flags.lootable)
                            && snapshot.loot.is_some()
                            && ui.button("Loot").clicked()
                        {
                            self.dismissed_loot = None;
                        }
                        if ui.button("Clear").clicked() {
                            intents.push(UiIntent::Select(None));
                        }
                    });
                });
            });
    }

    fn action_bar(
        &mut self,
        ctx: &egui::Context,
        frame: &UiFrame<'_>,
        snapshot: &ZoneSnapshot,
        intents: &mut Vec<UiIntent>,
    ) {
        egui::Area::new(egui::Id::unique("action-bar"))
            .anchor(Align2::CENTER_BOTTOM, Vec2::new(0.0, -12.0))
            .show(ctx, |ui| {
                panel_frame().show(ui, |ui| {
                    if let Some(cast) = snapshot.viewer.cast {
                        cast_bar(ui, cast);
                    }
                    ui.horizontal(|ui| {
                        let slots = frame.hud.map(|hud| hud.slots.as_slice()).unwrap_or(&[]);
                        for (index, slot) in slots.iter().enumerate() {
                            let name = ability_by_id(mmorpg_core::AbilityId::new(slot.ability))
                                .map_or("?", |a| a.name);
                            let button = egui::Button::new(
                                RichText::new(format!("{}\n{name}", index + 1)).size(12.0),
                            )
                            .min_size(Vec2::new(86.0, 46.0))
                            .selected(slot.casting);
                            let response = ui.add_enabled(!snapshot.viewer.dead, button);
                            if let Some(cooldown) = slot.cooldown {
                                let share = if cooldown.total == 0 {
                                    1.0
                                } else {
                                    (f32::from(cooldown.remaining) / f32::from(cooldown.total))
                                        .clamp(0.0, 1.0)
                                };
                                let rect = response.rect;
                                let covered = egui::Rect::from_min_max(
                                    egui::pos2(rect.min.x, rect.max.y - rect.height() * share),
                                    rect.max,
                                );
                                ui.painter().rect_filled(
                                    covered,
                                    2.0,
                                    Color32::from_black_alpha(match cooldown.kind {
                                        CooldownKind::Ability => 170,
                                        CooldownKind::Global => 110,
                                    }),
                                );
                                if cooldown.kind == CooldownKind::Ability {
                                    ui.painter().text(
                                        rect.center(),
                                        Align2::CENTER_CENTER,
                                        format!(
                                            "{:.0}",
                                            (f32::from(cooldown.remaining)
                                                / f32::from(mmorpg_core::TICK_HZ))
                                            .ceil()
                                        ),
                                        egui::FontId::proportional(18.0),
                                        Color32::WHITE,
                                    );
                                }
                            }
                            if response.clicked() {
                                intents.push(UiIntent::AbilitySlot(index as u8 + 1));
                            }
                        }
                        ui.separator();
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                if ui.button("Bags (B)").clicked() {
                                    self.bags_open = !self.bags_open;
                                }
                                if ui.button("Character (C)").clicked() {
                                    self.character_open = !self.character_open;
                                }
                            });
                            ui.horizontal(|ui| {
                                if ui.button("Quests (L)").clicked() {
                                    self.quest_log_open = !self.quest_log_open;
                                }
                                if ui.button("Chat (Enter)").clicked() {
                                    self.chat_focus_requested = true;
                                }
                            });
                        });
                    });
                });
            });
    }

    fn minimap_and_tracker(
        &mut self,
        ctx: &egui::Context,
        frame: &UiFrame<'_>,
        snapshot: &ZoneSnapshot,
    ) {
        egui::Area::new(egui::Id::unique("minimap"))
            .anchor(Align2::RIGHT_TOP, Vec2::new(-12.0, 12.0))
            .show(ctx, |ui| {
                panel_frame().show(ui, |ui| {
                    ui.set_width(200.0);
                    self.minimap.paint(
                        ui,
                        200.0,
                        snapshot,
                        self.sheets.quests.as_ref(),
                        frame.content,
                        frame.forward,
                        self.zoom,
                    );
                    ui.horizontal(|ui| {
                        if ui.small_button("−").on_hover_text("Zoom out").clicked() {
                            self.zoom = (self.zoom + 1).min(minimap::RADII_METRES.len() - 1);
                        }
                        if ui.small_button("+").on_hover_text("Zoom in").clicked() {
                            self.zoom = self.zoom.saturating_sub(1);
                        }
                        ui.small(format!("{} m", minimap::RADII_METRES[self.zoom]));
                    });
                    self.quest_tracker(ui, frame);
                });
            });
    }
}

fn class_title(class: PlayerClass) -> &'static str {
    match class {
        PlayerClass::Warden => "Warden",
        PlayerClass::Ranger => "Ranger",
        PlayerClass::Arcanist => "Arcanist",
    }
}

const fn sex_title(sex: Sex) -> &'static str {
    match sex {
        Sex::Female => "female",
        Sex::Male => "male",
    }
}

/// Turntable controls, as the browser's left/right/front buttons.
fn turntable(ui: &mut egui::Ui, degrees: &mut f32) {
    ui.horizontal(|ui| {
        ui.label("Turn");
        if ui.button("<").on_hover_text("Turn left").clicked() {
            *degrees = (*degrees - TURN_STEP_DEGREES).rem_euclid(360.0);
        }
        ui.add(
            egui::Slider::new(degrees, 0.0..=359.0)
                .suffix("°")
                .show_value(false),
        );
        if ui.button(">").on_hover_text("Turn right").clicked() {
            *degrees = (*degrees + TURN_STEP_DEGREES).rem_euclid(360.0);
        }
        if ui.button("Front").clicked() {
            *degrees = 0.0;
        }
    });
}

fn fraction(value: u32, max: u32) -> f32 {
    if max == 0 {
        0.0
    } else {
        (value as f32 / max as f32).clamp(0.0, 1.0)
    }
}

/// The Warden's melee stance and what it does (#155).
const fn stance_text(stance: mmorpg_core::MeleeStance) -> &'static str {
    match stance {
        mmorpg_core::MeleeStance::Planted => "Planted · braced, −10% damage taken",
        mmorpg_core::MeleeStance::Charging => "Charging · +20% melee damage",
        mmorpg_core::MeleeStance::Moving => "Moving · −15% melee damage",
        mmorpg_core::MeleeStance::Airborne => "Airborne · +35% melee damage, +25% taken",
        mmorpg_core::MeleeStance::Landing => "Landing · melee waits",
    }
}

const fn resource_color(kind: ResourceKind) -> Color32 {
    match kind {
        ResourceKind::Rage => Color32::from_rgb(200, 40, 30),
        ResourceKind::Focus => Color32::from_rgb(220, 180, 50),
        ResourceKind::Mana => Color32::from_rgb(50, 100, 230),
    }
}

fn panel_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(Color32::from_black_alpha(170))
        .corner_radius(4.0)
        .inner_margin(8.0)
}

fn bar(ui: &mut egui::Ui, fill: f32, text: String, color: Color32) {
    ui.add(
        egui::ProgressBar::new(fill)
            .fill(color)
            .text(RichText::new(text).color(Color32::WHITE))
            .desired_height(16.0),
    );
}

fn cast_bar(ui: &mut egui::Ui, cast: mmorpg_core::CastView) {
    let progress = fraction(u32::from(cast.elapsed), u32::from(cast.total));
    let name = ability_by_id(cast.ability).map_or("Casting", |a| a.name);
    let (fill, color) = if cast.channel {
        (1.0 - progress, Color32::from_rgb(70, 180, 240))
    } else {
        (progress, Color32::from_rgb(240, 165, 40))
    };
    bar(ui, fill, name.to_owned(), color);
}

fn auras(ui: &mut egui::Ui, auras: &[mmorpg_core::AuraView]) {
    if auras.is_empty() {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        for aura in auras {
            let name = ability_by_id(aura.ability).map_or(aura.kind.name(), |a| a.name);
            let seconds = f32::from(aura.remaining) / f32::from(mmorpg_core::TICK_HZ);
            ui.small(format!("{name} {seconds:.0}s"))
                .on_hover_text(aura.kind.name());
        }
    });
}

fn banner(ctx: &egui::Context, id: &str, text: &str, color: Color32, top: f32) {
    egui::Area::new(egui::Id::unique(id))
        .anchor(Align2::CENTER_TOP, Vec2::new(0.0, top))
        .interactable(false)
        .show(ctx, |ui| {
            ui.label(RichText::new(text).size(18.0).strong().color(color));
        });
}

fn line_color(kind: LineKind) -> Color32 {
    match kind {
        LineKind::Say => Color32::from_gray(235),
        LineKind::Yell => Color32::from_rgb(255, 90, 80),
        LineKind::Emote => Color32::from_rgb(255, 160, 80),
        LineKind::System => Color32::from_rgb(255, 230, 120),
        LineKind::Combat => Color32::from_gray(210),
        LineKind::Quest => Color32::from_rgb(255, 210, 0),
        LineKind::Refusal => Color32::from_rgb(255, 110, 100),
    }
}
