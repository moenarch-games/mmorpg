//! The browser's thin wrappers over `mmorpg-client-model` (#543): the action
//! bar and cast bar, chat parsing and the chat log, the movement outbox and
//! quest views, so both clients run the same client rules. Inputs arrive in
//! the browser's wire shape ([`crate::wire`]); results cross as plain objects
//! whose TypeScript types are generated here. Quest views read the hosted
//! content, the content every browser world presents quests from.
use mmorpg_client_model::{
    chat::{self, LineKind, Logs},
    hud::{self, CooldownKind},
    outbox::{self, MovementCommands},
    quest,
};
use mmorpg_core as core;
use serde::{Deserialize, Serialize};
use tsify::{Ts, Tsify};
use wasm_bindgen::prelude::{JsError, JsValue, wasm_bindgen};

use crate::host::hosted_content;
use crate::wire::{
    CastState, ChatChannel, ClassId, EmoteName, QuestSheet, WorldCommand, ZoneSnapshot,
};

fn core_snapshot(snapshot: &ZoneSnapshot) -> Result<core::ZoneSnapshot, JsError> {
    core::ZoneSnapshot::try_from(snapshot).map_err(|error| JsError::new(&error))
}

/// A list or optional result as a plain value, absent options as `null` like
/// every generated type here.
fn to_js<T: Serialize>(value: &T) -> Result<JsValue, JsError> {
    Ok(value.serialize(&serde_wasm_bindgen::Serializer::new().serialize_missing_as_null(true))?)
}

/// The class abilities on action-bar slots 1–8, in catalog order.
#[wasm_bindgen(js_name = classKit)]
pub fn class_kit(class: Ts<ClassId>) -> Result<Vec<u8>, JsError> {
    Ok(hud::class_kit(class.to_rust()?.into()))
}

/// Which wait a slot shows: its own cooldown or the global one.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "lowercase")]
pub enum SlotCooldownKind {
    Cooldown,
    Gcd,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
pub struct SlotCooldown {
    pub kind: SlotCooldownKind,
    pub remaining: u16,
    pub total: u16,
}

/// What one action-bar slot shows; the zone answers every use itself.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "camelCase")]
#[tsify(missing_as_null)]
pub struct SlotState {
    pub ability: u8,
    pub learned: bool,
    /// Level at which the class learns the ability.
    pub unlock_level: u8,
    /// The class resource is below the cost.
    pub resource_short: bool,
    /// The target is known and farther than the ability reaches.
    pub out_of_range: bool,
    /// The viewer is casting or channelling this ability.
    pub casting: bool,
    pub cooldown: Option<SlotCooldown>,
}

impl From<hud::SlotState> for SlotState {
    fn from(slot: hud::SlotState) -> Self {
        Self {
            ability: slot.ability,
            learned: slot.learned,
            unlock_level: slot.unlock_level,
            resource_short: slot.resource_short,
            out_of_range: slot.out_of_range,
            casting: slot.casting,
            cooldown: slot.cooldown.map(|cooldown| SlotCooldown {
                kind: match cooldown.kind {
                    CooldownKind::Ability => SlotCooldownKind::Cooldown,
                    CooldownKind::Global => SlotCooldownKind::Gcd,
                },
                remaining: cooldown.remaining,
                total: cooldown.total,
            }),
        }
    }
}

/// The action bar for a projection: the viewer's class kit, slot by slot.
#[wasm_bindgen(js_name = actionBar, unchecked_return_type = "SlotState[]")]
pub fn action_bar(snapshot: Ts<ZoneSnapshot>) -> Result<JsValue, JsError> {
    let snapshot = core_snapshot(&snapshot.to_rust()?)?;
    let slots: Vec<SlotState> = hud::CombatHud::from_projection(&snapshot)
        .slots
        .into_iter()
        .map(Into::into)
        .collect();
    to_js(&slots)
}

/// What a cast bar shows: a cast fills, a channel drains.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Tsify)]
pub struct CastBar {
    /// Filled share, 0–1.
    pub fill: f32,
    /// Ticks left.
    pub remaining: u16,
    pub channel: bool,
}

#[wasm_bindgen(js_name = castBar)]
pub fn cast_bar(cast: Ts<CastState>) -> Result<Ts<CastBar>, JsError> {
    let bar = hud::cast_bar(cast.to_rust()?.into());
    Ok(CastBar {
        fill: bar.fill,
        remaining: bar.remaining,
        channel: bar.channel,
    }
    .into_ts()?)
}

/// A typed chat line: the command to send, or why it cannot be sent.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(untagged)]
pub enum ChatInput {
    Command { command: WorldCommand },
    Error { error: String },
}

/// Reads a typed line: `/y` or `/yell` yells, `/s`, `/say` or no prefix says,
/// and `/wave`, `/bow`, `/cheer`, `/laugh` or `/point` emotes.
#[wasm_bindgen(js_name = parseChatInput)]
pub fn parse_chat_input(raw: &str) -> Result<Ts<ChatInput>, JsError> {
    let parsed = match chat::parse_chat_input(raw) {
        Ok(core::ZoneCommand::Chat { channel, text }) => ChatInput::Command {
            command: WorldCommand::Chat {
                channel: ChatChannel::from(channel),
                text: text.as_str().to_owned(),
            },
        },
        Ok(core::ZoneCommand::Emote(emote)) => ChatInput::Command {
            command: WorldCommand::Emote {
                emote: EmoteName::from(emote),
            },
        },
        Ok(other) => unreachable!("chat input parses to chat or an emote, not {other:?}"),
        Err(error) => ChatInput::Error { error },
    };
    Ok(parsed.into_ts()?)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "lowercase")]
pub enum ChatLineKind {
    Say,
    Yell,
    Emote,
    System,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
pub struct ChatEntry {
    pub kind: ChatLineKind,
    pub text: String,
}

/// Received chat lines with readable speaker names, each projection taken once.
#[wasm_bindgen]
#[derive(Default)]
pub struct ChatLog {
    logs: Logs,
}

#[wasm_bindgen]
impl ChatLog {
    #[wasm_bindgen(constructor)]
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        self.logs.reset();
    }

    /// Appends a local note, such as why a typed line was not sent.
    pub fn note(&mut self, text: &str) {
        self.logs.note(LineKind::System, text);
    }

    /// Takes a newer projection's chat; returns whether lines were added.
    pub fn receive(&mut self, snapshot: Ts<ZoneSnapshot>) -> Result<bool, JsError> {
        let snapshot = core_snapshot(&snapshot.to_rust()?)?;
        Ok(self.logs.receive_chat(&snapshot))
    }

    /// Lines added since the log began, for skipping unchanged renders.
    #[must_use]
    pub fn added(&self) -> f64 {
        self.logs.chat.added() as f64
    }

    /// The newest lines, oldest first.
    #[wasm_bindgen(unchecked_return_type = "ChatEntry[]")]
    pub fn entries(&self) -> Result<JsValue, JsError> {
        let entries: Vec<ChatEntry> = self
            .logs
            .chat
            .lines()
            .map(|line| ChatEntry {
                kind: match line.kind {
                    LineKind::Say => ChatLineKind::Say,
                    LineKind::Yell => ChatLineKind::Yell,
                    LineKind::Emote => ChatLineKind::Emote,
                    LineKind::System | LineKind::Combat | LineKind::Quest | LineKind::Refusal => {
                        ChatLineKind::System
                    }
                },
                text: line.text.clone(),
            })
            .collect();
        to_js(&entries)
    }
}

/// The shared movement outbox: numbers in, the movement commands to send out.
#[wasm_bindgen]
pub struct MovementOutbox {
    outbox: outbox::MovementOutbox,
}

const fn movement_input(
    forward: i8,
    strafe: i8,
    facing: u16,
    jumps: u32,
    rolls: u32,
    rise: i8,
) -> outbox::MovementInput {
    outbox::MovementInput {
        forward,
        strafe,
        facing,
        jumps,
        rolls,
        rise,
    }
}

fn movement_commands(commands: MovementCommands) -> Vec<WorldCommand> {
    let mut out = Vec::new();
    if commands.jump {
        out.push(WorldCommand::Jump);
    }
    if let Some(rise) = commands.rise {
        out.push(WorldCommand::Rise { rise });
    }
    if let Some((forward, strafe, facing)) = commands.movement {
        out.push(WorldCommand::Move {
            forward,
            strafe,
            facing,
        });
    }
    if commands.roll {
        out.push(WorldCommand::Roll);
    }
    out
}

#[wasm_bindgen]
impl MovementOutbox {
    #[wasm_bindgen(constructor)]
    #[must_use]
    pub fn new(jumps: u32, rolls: u32) -> Self {
        Self {
            outbox: outbox::MovementOutbox::new(movement_input(0, 0, 0, jumps, rolls, 0)),
        }
    }

    /// Starts a new session: only current presses count, earlier ones are dropped.
    pub fn reset(&mut self, jumps: u32, rolls: u32) {
        self.outbox.resume(movement_input(0, 0, 0, jumps, rolls, 0));
    }

    /// The commands the current input needs; `dead` comes from the latest projection.
    #[allow(clippy::too_many_arguments)]
    #[wasm_bindgen(unchecked_return_type = "WorldCommand[]")]
    pub fn update(
        &mut self,
        forward: i8,
        strafe: i8,
        facing: u16,
        jumps: u32,
        rolls: u32,
        rise: i8,
        now_ms: f64,
        dead: bool,
    ) -> Result<JsValue, JsError> {
        let input = movement_input(forward, strafe, facing, jumps, rolls, rise);
        let commands = self.outbox.update(input, now_ms, dead, false);
        to_js(&movement_commands(commands))
    }
}

/// One objective with its received progress.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
pub struct ObjectiveView {
    pub label: String,
    pub text: String,
    pub current: u8,
    pub required: u8,
    pub done: bool,
}

impl From<quest::ObjectiveView> for ObjectiveView {
    fn from(view: quest::ObjectiveView) -> Self {
        Self {
            label: view.label,
            text: view.text,
            current: view.current,
            required: view.required,
            done: view.done,
        }
    }
}

/// An active quest with each objective's received progress.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
pub struct QuestView {
    pub quest: u8,
    pub objectives: Vec<ObjectiveView>,
    pub complete: bool,
}

impl From<quest::QuestView> for QuestView {
    fn from(view: quest::QuestView) -> Self {
        Self {
            quest: view.quest.get(),
            objectives: view.objectives.into_iter().map(Into::into).collect(),
            complete: view.complete,
        }
    }
}

fn quest_views_of(views: Vec<quest::QuestView>) -> Vec<QuestView> {
    views.into_iter().map(Into::into).collect()
}

/// The sheet's active quests in quest-ID order.
#[wasm_bindgen(js_name = questViews, unchecked_return_type = "QuestView[]")]
pub fn quest_views(sheet: Ts<QuestSheet>) -> Result<JsValue, JsError> {
    let sheet = core::QuestSheet::from(&sheet.to_rust()?);
    to_js(&quest_views_of(quest::quest_views(
        &sheet,
        &hosted_content(),
    )))
}

/// `quest` at `progress`, one count per objective; `null` for an unknown quest.
#[wasm_bindgen(js_name = questView, unchecked_return_type = "QuestView | null")]
pub fn quest_view(quest: u8, progress: &[u8]) -> Result<JsValue, JsError> {
    let content = hosted_content();
    let view: Option<QuestView> = content
        .quest(core::QuestId::new(quest))
        .map(|record| quest::quest_view(record, progress, &content).into());
    to_js(&view)
}

/// One quest-tracker row: a quest heading (`objective` null) or one of its objectives.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[tsify(missing_as_null)]
pub struct TrackerRow {
    pub text: String,
    pub quest: u8,
    pub objective: Option<u8>,
    pub done: bool,
}

#[wasm_bindgen(js_name = trackerRows, unchecked_return_type = "TrackerRow[]")]
pub fn tracker_rows(sheet: Ts<QuestSheet>) -> Result<JsValue, JsError> {
    let sheet = core::QuestSheet::from(&sheet.to_rust()?);
    let rows: Vec<TrackerRow> = quest::tracker_rows(&sheet, &hosted_content())
        .into_iter()
        .map(|row| TrackerRow {
            text: row.text,
            quest: row.quest.get(),
            objective: row.objective,
            done: row.done,
        })
        .collect();
    to_js(&rows)
}

/// A projected quest NPC and whether it looks within reach.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "camelCase")]
pub struct QuestNpc {
    pub npc: u32,
    pub in_reach: bool,
}

/// The quest NPC Talk addresses: the selected NPC only, else the nearest quest NPC.
#[wasm_bindgen(js_name = dialogNpc, unchecked_return_type = "QuestNpc | null")]
pub fn dialog_npc(snapshot: Ts<ZoneSnapshot>) -> Result<JsValue, JsError> {
    let snapshot = core_snapshot(&snapshot.to_rust()?)?;
    let npc = quest::dialog_npc(&snapshot, &hosted_content()).map(|npc| QuestNpc {
        npc: npc.npc.get(),
        in_reach: npc.in_reach,
    });
    to_js(&npc)
}

/// What one NPC offers: quests to accept, and active quests ending with it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "camelCase")]
#[tsify(missing_as_null)]
pub struct DialogView {
    /// A woken private-home resident's line before its turn-ins (#158).
    pub greeting: Option<String>,
    pub offered: Vec<u8>,
    pub ready: Vec<QuestView>,
    pub in_progress: Vec<QuestView>,
}

/// At `night` a private-home resident greets with its woken line and offers only turn-ins.
#[wasm_bindgen(js_name = dialogView)]
pub fn dialog_view(
    sheet: Ts<QuestSheet>,
    npc: u32,
    night: bool,
) -> Result<Ts<DialogView>, JsError> {
    let sheet = core::QuestSheet::from(&sheet.to_rust()?);
    let view = quest::dialog_view(&sheet, &hosted_content(), core::NpcId::new(npc), night);
    Ok(DialogView {
        greeting: view.greeting,
        offered: view.offered.iter().map(|quest| quest.get()).collect(),
        ready: quest_views_of(view.ready),
        in_progress: quest_views_of(view.in_progress),
    }
    .into_ts()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn movement_commands_keep_the_send_order() {
        let commands = MovementCommands {
            jump: true,
            rise: Some(1),
            movement: Some((1, 0, 7)),
            roll: true,
        };
        assert_eq!(
            movement_commands(commands),
            [
                WorldCommand::Jump,
                WorldCommand::Rise { rise: 1 },
                WorldCommand::Move {
                    forward: 1,
                    strafe: 0,
                    facing: 7
                },
                WorldCommand::Roll,
            ]
        );
        assert!(movement_commands(MovementCommands::default()).is_empty());
    }
}
