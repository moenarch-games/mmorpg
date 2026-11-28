//! The browser's thin wrappers over `mmorpg-client-model` (#543): the action
//! bar and cast bar, chat parsing and the chat log, floating combat text, the movement outbox, the
//! hold-to-draw rule and quest views, so both clients run the same client
//! rules. Inputs arrive in
//! the browser's wire shape ([`crate::wire`]); results cross as plain objects
//! whose TypeScript types are generated here. Quest views read the hosted
//! content, the content every browser world presents quests from.
use mmorpg_client_model::{
    chat::{self, LineKind, Logs},
    combat_text, exploration,
    hud::{self, CooldownKind},
    interaction, manual_aim, mouse_owner,
    outbox::{self, MovementCommands},
    pie_menu,
    power_draw::{self, DrawRelease},
    quest,
};
use mmorpg_core as core;
use serde::{Deserialize, Serialize};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
};
use tsify::{Ts, Tsify};
use wasm_bindgen::prelude::{JsError, JsValue, wasm_bindgen};

use crate::host::hosted_content;
use crate::wire::{
    CastState, ChatChannel, ClassId, EmoteName, QuestSheet, StrikeAim, StrikeHeight, WorldCommand,
    ZoneSnapshot,
};

/// How many recent projection views [`remember_view`] keeps: a few sessions' latest.
const REMEMBERED_VIEWS: usize = 4;

thread_local! {
    /// The views [`crate::ProjectionStore`] handed to the page last, with their decoded
    /// projections (#1257).
    static VIEWS: RefCell<VecDeque<(JsValue, Rc<core::ZoneSnapshot>)>> =
        const { RefCell::new(VecDeque::new()) };
    /// Projections the rules deserialised from a page object (`projection-rules`, #1257).
    static DESERIALISED: Cell<u32> = const { Cell::new(0) };
}

/// How many projections the client rules deserialised from a page object since the page
/// loaded: none for the views a projection store handed out (`projection-rules` in
/// `.performance/contract.json`).
#[wasm_bindgen(js_name = projectionViewDeserialisations)]
#[must_use]
pub fn projection_view_deserialisations() -> u32 {
    DESERIALISED.with(Cell::get)
}

/// Remembers the page's `view` of the ingested projection `decoded`: the rules below read
/// `decoded` when the page hands them that view. The page completes a view only with the bag and
/// quest sheets the store already holds, so `decoded` is what the view reads as.
pub(crate) fn remember_view(view: JsValue, decoded: Rc<core::ZoneSnapshot>) {
    VIEWS.with(|views| {
        let mut views = views.borrow_mut();
        if views.len() == REMEMBERED_VIEWS {
            views.pop_front();
        }
        views.push_back((view, decoded));
    });
}

/// The projection a rule reads: an ingested view's decoded projection, otherwise (a projection
/// no store ingested, such as a test's) the view deserialised. A tick's projection is handed to
/// a dozen rules, and deserialising it from the page object cost thousands of calls into the
/// page each time (#1257).
pub(crate) fn core_snapshot(
    snapshot: &Ts<ZoneSnapshot>,
) -> Result<Rc<core::ZoneSnapshot>, JsError> {
    let view = snapshot.js_value();
    let remembered = VIEWS.with(|views| {
        views
            .borrow()
            .iter()
            .rev()
            .find(|(kept, _)| *kept == view)
            .map(|(_, decoded)| Rc::clone(decoded))
    });
    if let Some(decoded) = remembered {
        return Ok(decoded);
    }
    DESERIALISED.with(|count| count.set(count.get().wrapping_add(1)));
    core::ZoneSnapshot::try_from(&snapshot.to_rust()?)
        .map(Rc::new)
        .map_err(|error| JsError::new(&error))
}

/// A list or optional result as a plain value, absent options as `null` like
/// every generated type here.
pub(crate) fn to_js<T: Serialize>(value: &T) -> Result<JsValue, JsError> {
    Ok(value.serialize(&serde_wasm_bindgen::Serializer::new().serialize_missing_as_null(true))?)
}

/// The class abilities on action-bar slots 1–8, in catalog order.
#[wasm_bindgen(js_name = classKit)]
pub fn class_kit(class: Ts<ClassId>) -> Result<Vec<u32>, JsError> {
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
#[tsify(missing_as_null)]
pub struct SlotCooldown {
    pub kind: SlotCooldownKind,
    pub remaining: u16,
    pub total: u16,
    /// The whole seconds the slot shows, `null` below 2 s and for the global cooldown (#1280).
    pub seconds: Option<u16>,
}

/// What one action-bar slot shows; the zone answers every use itself.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "camelCase")]
#[tsify(missing_as_null)]
pub struct SlotState {
    pub ability: u32,
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
                seconds: cooldown.shown_seconds(),
            }),
        }
    }
}

/// The action bar for a projection: the viewer's class kit, slot by slot.
#[wasm_bindgen(js_name = actionBar, unchecked_return_type = "SlotState[]")]
pub fn action_bar(snapshot: Ts<ZoneSnapshot>) -> Result<JsValue, JsError> {
    let snapshot = core_snapshot(&snapshot)?;
    let slots: Vec<SlotState> = hud::CombatHud::from_projection(&snapshot)
        .slots
        .into_iter()
        .map(Into::into)
        .collect();
    to_js(&slots)
}

/// What a floating combat text says (#1234), which picks its look.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "kebab-case")]
pub enum CombatTextKind {
    Damage,
    Critical,
    Periodic,
    Taken,
    Heal,
    Absorb,
    Interrupt,
    Miss,
    Block,
    GuardBreak,
    Cleave,
    Flank,
    Mastery,
}

impl From<combat_text::CombatTextKind> for CombatTextKind {
    fn from(kind: combat_text::CombatTextKind) -> Self {
        use combat_text::CombatTextKind as Kind;
        match kind {
            Kind::Damage => Self::Damage,
            Kind::Critical => Self::Critical,
            Kind::Periodic => Self::Periodic,
            Kind::Taken => Self::Taken,
            Kind::Heal => Self::Heal,
            Kind::Absorb => Self::Absorb,
            Kind::Interrupt => Self::Interrupt,
            Kind::Miss => Self::Miss,
            Kind::Block => Self::Block,
            Kind::GuardBreak => Self::GuardBreak,
            Kind::Cleave => Self::Cleave,
            Kind::Flank => Self::Flank,
            Kind::Mastery => Self::Mastery,
        }
    }
}

/// One floating combat text; the browser lists it in its HUD rather than over a unit.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
pub struct CombatText {
    pub kind: CombatTextKind,
    pub text: String,
}

fn combat_texts_js(texts: Vec<combat_text::CombatText>) -> Result<JsValue, JsError> {
    let texts: Vec<CombatText> = texts
        .into_iter()
        .map(|text| CombatText {
            kind: text.kind.into(),
            text: text.text,
        })
        .collect();
    to_js(&texts)
}

/// The floating texts one projection's events produce, in event order.
#[wasm_bindgen(js_name = combatTexts, unchecked_return_type = "CombatText[]")]
pub fn combat_texts(snapshot: Ts<ZoneSnapshot>) -> Result<JsValue, JsError> {
    let snapshot = core_snapshot(&snapshot)?;
    combat_texts_js(combat_text::combat_texts(&snapshot))
}

/// Whether this projection interrupted the viewer's own cast.
#[wasm_bindgen(js_name = viewerInterrupted)]
pub fn viewer_interrupted(snapshot: Ts<ZoneSnapshot>) -> Result<bool, JsError> {
    let snapshot = core_snapshot(&snapshot)?;
    Ok(combat_text::viewer_interrupted(&snapshot))
}

/// How long a floating combat text lives, in milliseconds.
#[wasm_bindgen(js_name = combatTextMs)]
#[must_use]
pub fn combat_text_ms() -> f64 {
    combat_text::COMBAT_TEXT_MS as f64
}

/// The floating texts a stream of projections raises (#1234), each projection taken once:
/// its events' texts, then a cleave or pierce through several units and a mastery level gained.
#[wasm_bindgen]
#[derive(Default)]
pub struct CombatTextFeed {
    feed: combat_text::CombatTextFeed,
}

#[wasm_bindgen]
impl CombatTextFeed {
    #[wasm_bindgen(constructor)]
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        self.feed.reset();
    }

    /// The texts a newer projection adds; none for one already taken.
    #[wasm_bindgen(unchecked_return_type = "CombatText[]")]
    pub fn observe(&mut self, snapshot: Ts<ZoneSnapshot>) -> Result<JsValue, JsError> {
        let snapshot = core_snapshot(&snapshot)?;
        combat_texts_js(self.feed.observe(&snapshot))
    }
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
        Ok(core::ZoneCommand::Sit { seat }) => ChatInput::Command {
            command: WorldCommand::Sit {
                seat: seat.map(core::SeatId::get),
            },
        },
        Ok(other) => unreachable!("chat input parses to chat, an emote or a sit, not {other:?}"),
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
        let snapshot = core_snapshot(&snapshot)?;
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

/// The shared hold-to-draw rule (#607): pressing and releasing a held slot, and the shot it
/// fires. Nothing here decides an outcome; the zone answers the shot.
#[wasm_bindgen]
#[derive(Default)]
pub struct PowerDraw {
    draw: power_draw::PowerDraw,
}

#[wasm_bindgen]
impl PowerDraw {
    #[wasm_bindgen(constructor)]
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A held input pressed `slot`: `true` starts a draw and sends nothing.
    pub fn press(
        &mut self,
        slot: u8,
        ability: u32,
        snapshot: Ts<ZoneSnapshot>,
        interface_open: bool,
    ) -> Result<bool, JsError> {
        let snapshot = core_snapshot(&snapshot)?;
        Ok(self
            .draw
            .press(slot, ability, Some(&snapshot), interface_open))
    }

    /// `slot` went up, or its hold was retired; the ability to send, if it fires.
    pub fn release(&mut self, slot: u8, retired: bool) -> Option<u32> {
        match self.draw.release(slot, retired) {
            Some(DrawRelease::Fire(ability)) => Some(ability),
            Some(DrawRelease::Drop) | None => None,
        }
    }

    /// Drops the draw without firing; `true` if there was one.
    pub fn cancel(&mut self) -> bool {
        self.draw.cancel()
    }

    /// Follows a projection; `true` if the draw just became impossible and was dropped.
    pub fn observe(
        &mut self,
        snapshot: Ts<ZoneSnapshot>,
        interface_open: bool,
    ) -> Result<bool, JsError> {
        let snapshot = core_snapshot(&snapshot)?;
        Ok(self.draw.observe(&snapshot, interface_open))
    }

    /// The ability being drawn.
    #[must_use]
    pub fn drawing(&self) -> Option<u32> {
        self.draw.drawing()
    }
}

/// Who owns the mouse (#636): free look, focus, or the gesture holding it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "kebab-case")]
pub enum MouseOwnerKind {
    FreeLook,
    Focus,
    Strike,
    Guard,
    Aim,
    Wheel,
}

/// A gesture that ended, or `none`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "kebab-case")]
pub enum EndedGesture {
    None,
    Strike,
    Guard,
    Aim,
    Wheel,
}

impl From<Option<mouse_owner::Gesture>> for EndedGesture {
    fn from(gesture: Option<mouse_owner::Gesture>) -> Self {
        use mouse_owner::{Gesture, Hold};
        match gesture {
            None => Self::None,
            Some(Gesture::Hold(Hold::Strike)) => Self::Strike,
            Some(Gesture::Hold(Hold::Guard)) => Self::Guard,
            Some(Gesture::Aim) => Self::Aim,
            Some(Gesture::Wheel) => Self::Wheel,
        }
    }
}

/// A strike or guard hold.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "kebab-case")]
pub enum HoldKind {
    Strike,
    Guard,
}

impl From<HoldKind> for mouse_owner::Hold {
    fn from(hold: HoldKind) -> Self {
        match hold {
            HoldKind::Strike => Self::Strike,
            HoldKind::Guard => Self::Guard,
        }
    }
}

/// What a click means: a left click selects, confirms or does nothing; a right click cancels the
/// cast or the gesture (`ended`).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "kebab-case")]
pub enum ClickOutcome {
    Select,
    Confirm,
    /// An armed manual aim (#649) throws `ability` along the aim.
    Throw,
    Nothing,
    CancelCast,
    Cancel,
}

/// What a held slot press does under the manual-aim rule (#649).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "kebab-case")]
pub enum AimPress {
    /// Not a manual aim: use the slot as usual.
    Use,
    /// The aim owns the mouse now; nothing is sent.
    Aim,
    /// Another gesture owns the mouse; nothing happens.
    Ignore,
}

/// A click's outcome and the gesture it ended, if any.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[tsify(missing_as_null)]
pub struct Click {
    pub outcome: ClickOutcome,
    pub ended: EndedGesture,
    /// The ability a `throw` uses.
    pub ability: Option<u32>,
    /// The wheel action (#653) a `confirm` runs, by its semantic id; `null` for the centre.
    pub action: Option<String>,
}

/// Which radial wheel a key opens (#653).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "kebab-case")]
pub enum WheelKindName {
    Fixed,
    Context,
}

/// One wheel sector: the semantic action id it runs and its label.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
pub struct WheelSector {
    pub id: String,
    pub label: String,
}

/// The open wheel: its sectors north first and clockwise (`null` gaps), and the pointed-at one.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[tsify(missing_as_null)]
pub struct WheelView {
    pub kind: WheelKindName,
    pub sectors: Vec<Option<WheelSector>>,
    pub highlight: Option<u8>,
}

/// What a wheel key's release did.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "kebab-case")]
pub enum WheelOutcome {
    /// No wheel was open.
    Ignored,
    /// The wheel ended and runs `action`.
    Pick,
    /// A quick tap: the wheel stays open for a left click.
    Open,
    /// The wheel ended without an action.
    Cancel,
}

/// A wheel key release: its outcome and the picked action's id.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[tsify(missing_as_null)]
pub struct WheelReleaseView {
    pub outcome: WheelOutcome,
    pub action: Option<String>,
}

/// A hold's press: whether it owns the mouse, and the other hold it ended without firing.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
pub struct BeginHold {
    pub started: bool,
    pub preempted: EndedGesture,
}

/// The held gesture's current aim: a strike's side and height or a guard's lane.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[tsify(missing_as_null)]
pub struct GestureAim {
    pub strike: Option<StrikeAim>,
    pub guard: Option<StrikeHeight>,
}

/// The shared mouse-owner state machine (#636, Atlas 5.2.i636-2); nothing here decides an outcome.
#[wasm_bindgen]
#[derive(Default)]
pub struct MouseOwners {
    owners: mouse_owner::MouseOwners,
    /// The opt-in local wheel usage counters (#653), while counting.
    usage: Option<pie_menu::WheelUsage>,
}

#[wasm_bindgen]
impl MouseOwners {
    #[wasm_bindgen(constructor)]
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn owner(&self) -> Result<Ts<MouseOwnerKind>, JsError> {
        use mouse_owner::{Gesture, Hold, MouseOwner};
        Ok(match self.owners.owner() {
            MouseOwner::FreeLook => MouseOwnerKind::FreeLook,
            MouseOwner::Focus => MouseOwnerKind::Focus,
            MouseOwner::Gesture(Gesture::Hold(Hold::Strike)) => MouseOwnerKind::Strike,
            MouseOwner::Gesture(Gesture::Hold(Hold::Guard)) => MouseOwnerKind::Guard,
            MouseOwner::Gesture(Gesture::Aim) => MouseOwnerKind::Aim,
            MouseOwner::Gesture(Gesture::Wheel) => MouseOwnerKind::Wheel,
        }
        .into_ts()?)
    }

    #[wasm_bindgen(js_name = setFocus)]
    pub fn set_focus(&mut self, focus: bool) {
        self.owners.set_focus(focus);
    }

    /// The `controls.hold_camera` setting: `"free"`, otherwise locked.
    #[wasm_bindgen(js_name = setHoldCamera)]
    pub fn set_hold_camera(&mut self, value: &str) {
        self.owners
            .set_hold_camera(mouse_owner::HoldCamera::from_setting(value));
    }

    /// The `controls.aim_style` setting (#649): `"armed"`, otherwise hold.
    #[wasm_bindgen(js_name = setAimStyle)]
    pub fn set_aim_style(&mut self, value: &str) {
        self.owners
            .set_aim_style(mouse_owner::AimStyle::from_setting(value));
    }

    /// A held input pressed `slot`, holding `ability` (#649): `use` it as usual, it now `aim`s,
    /// or another gesture owns the mouse and the press does nothing (`ignore`).
    #[wasm_bindgen(js_name = pressAim)]
    pub fn press_aim(
        &mut self,
        slot: u8,
        ability: u32,
        snapshot: Ts<ZoneSnapshot>,
        interface_open: bool,
    ) -> Result<Ts<AimPress>, JsError> {
        let snapshot = core_snapshot(&snapshot)?;
        let press = if !manual_aim::can_aim(&snapshot, slot, ability, interface_open) {
            AimPress::Use
        } else if self.owners.begin_aim(slot, ability) {
            AimPress::Aim
        } else {
            AimPress::Ignore
        };
        Ok(press.into_ts()?)
    }

    /// `slot` went up, or its hold was retired: the ability to throw along the aim, if any.
    #[wasm_bindgen(js_name = releaseAim)]
    pub fn release_aim(&mut self, slot: u8, retired: bool) -> Option<u32> {
        self.owners.release_aim(slot, retired)
    }

    /// Where a throw along `facing` is expected to land, in core units; an aid only.
    #[wasm_bindgen(js_name = aimLanding)]
    pub fn aim_landing(
        &self,
        snapshot: Ts<ZoneSnapshot>,
        facing: u16,
    ) -> Result<Option<Vec<i32>>, JsError> {
        let snapshot = core_snapshot(&snapshot)?;
        Ok(manual_aim::landing(&snapshot, facing).map(Vec::from))
    }

    /// A hold's key went down: `true` if it owns the mouse; `preempted` ends the other hold.
    #[wasm_bindgen(js_name = beginHold)]
    pub fn begin_hold(&mut self, hold: Ts<HoldKind>) -> Result<Ts<BeginHold>, JsError> {
        let (started, preempted) = match self.owners.begin_hold(hold.to_rust()?.into()) {
            mouse_owner::Begin::Refused => (false, None),
            mouse_owner::Begin::Started { preempted } => {
                (true, preempted.map(mouse_owner::Gesture::Hold))
            }
        };
        Ok(BeginHold {
            started,
            preempted: preempted.into(),
        }
        .into_ts()?)
    }

    /// A hold's key went up deliberately: `true` when it owned the mouse and now acts (the strike
    /// fires with the aim read just before, the guard lowers).
    #[wasm_bindgen(js_name = releaseHold)]
    pub fn release_hold(&mut self, hold: Ts<HoldKind>) -> Result<bool, JsError> {
        Ok(self.owners.release_hold(hold.to_rust()?.into()).is_some())
    }

    #[wasm_bindgen(js_name = leftClick)]
    pub fn left_click(&mut self) -> Result<Ts<Click>, JsError> {
        use mouse_owner::{Gesture, LeftClick};
        let mut action = None;
        let (outcome, ended, ability) = match self.owners.left_click() {
            LeftClick::Select => (ClickOutcome::Select, None, None),
            LeftClick::Nothing => (ClickOutcome::Nothing, None, None),
            LeftClick::Pick(picked) => {
                action = picked.map(pie_menu::WheelAction::id);
                (ClickOutcome::Confirm, Some(Gesture::Wheel), None)
            }
            LeftClick::Throw(ability) => (ClickOutcome::Throw, Some(Gesture::Aim), Some(ability)),
        };
        Ok(Click {
            outcome,
            ended: ended.into(),
            ability,
            action,
        }
        .into_ts()?)
    }

    #[wasm_bindgen(js_name = rightClick)]
    pub fn right_click(&mut self) -> Result<Ts<Click>, JsError> {
        let (outcome, ended) = match self.owners.right_click() {
            mouse_owner::RightClick::CancelCast => (ClickOutcome::CancelCast, None),
            mouse_owner::RightClick::Cancel(gesture) => (ClickOutcome::Cancel, Some(gesture)),
        };
        Ok(Click {
            outcome,
            ended: ended.into(),
            ability: None,
            action: None,
        }
        .into_ts()?)
    }

    /// A wheel key went down (#653): opens the fixed wheel from its eight sector `choices`, or
    /// the context wheel from the projection and the hosted content. `true` if it is open now;
    /// a hold, an aim, an empty wheel or a second press of a tapped-open wheel refuse.
    #[wasm_bindgen(js_name = openWheel)]
    pub fn open_wheel(
        &mut self,
        kind: Ts<WheelKindName>,
        choices: Vec<String>,
        snapshot: Ts<ZoneSnapshot>,
    ) -> Result<bool, JsError> {
        let wheel = match kind.to_rust()? {
            WheelKindName::Fixed => {
                let choices: Vec<&str> = choices.iter().map(String::as_str).collect();
                pie_menu::Wheel::fixed(&choices)
            }
            WheelKindName::Context => {
                let snapshot = core_snapshot(&snapshot)?;
                pie_menu::Wheel::context(&snapshot, &hosted_content())
            }
        };
        Ok(self.owners.open_wheel(wheel))
    }

    /// The open wheel, labelled for `snapshot`'s class, or `null`.
    pub fn wheel(&self, snapshot: Ts<ZoneSnapshot>) -> Result<JsValue, JsError> {
        let Some(wheel) = self.owners.wheel() else {
            return Ok(JsValue::NULL);
        };
        let class = core_snapshot(&snapshot)?
            .viewer
            .class
            .map(|choice| choice.class);
        let view = WheelView {
            kind: match wheel.kind {
                pie_menu::WheelKind::Fixed => WheelKindName::Fixed,
                pie_menu::WheelKind::Context => WheelKindName::Context,
            },
            sectors: wheel
                .sectors()
                .iter()
                .map(|sector| {
                    sector.map(|action| WheelSector {
                        id: action.id(),
                        label: action.label(class),
                    })
                })
                .collect(),
            highlight: self
                .owners
                .wheel_sector()
                .and_then(|sector| u8::try_from(sector).ok()),
        };
        to_js(&view)
    }

    /// The wheel key went up after `held_ms`, or was `retired`: pick, stay open or cancel.
    #[wasm_bindgen(js_name = releaseWheel)]
    pub fn release_wheel(
        &mut self,
        held_ms: f64,
        retired: bool,
    ) -> Result<Ts<WheelReleaseView>, JsError> {
        use mouse_owner::WheelRelease;
        let (outcome, action) = match self.owners.release_wheel(held_ms, retired) {
            WheelRelease::Ignored => (WheelOutcome::Ignored, None),
            WheelRelease::Pick(action) => (WheelOutcome::Pick, Some(action.id())),
            WheelRelease::Open => (WheelOutcome::Open, None),
            WheelRelease::Cancel => (WheelOutcome::Cancel, None),
        };
        Ok(WheelReleaseView { outcome, action }.into_ts()?)
    }

    /// Turns the opt-in local wheel usage counters (#653) on from their stored JSON (`""` starts
    /// at zero), or off with `null`.
    #[wasm_bindgen(js_name = setUsageCounting)]
    pub fn set_usage_counting(&mut self, stored: Option<String>) {
        self.usage = stored.map(|text| pie_menu::WheelUsage::from_json(&text));
        self.owners.take_tally();
    }

    /// `ms` milliseconds passed under the current owner; takes the wheel events too.
    #[wasm_bindgen(js_name = countUsage)]
    pub fn count_usage(&mut self, ms: f64) {
        let tally = self.owners.take_tally();
        if let Some(usage) = self.usage.as_mut() {
            usage.add(tally);
            usage.spend(self.owners.owner(), ms);
        }
    }

    /// The counters as JSON, while counting.
    #[wasm_bindgen(js_name = usageJson)]
    pub fn usage_json(&self) -> Option<String> {
        self.usage.as_ref().map(pie_menu::WheelUsage::to_json)
    }

    /// Focus loss, a menu or panel, a lost pointer lock or a retired key: no firing.
    pub fn interrupt(&mut self) -> Result<Ts<EndedGesture>, JsError> {
        Ok(EndedGesture::from(self.owners.interrupt()).into_ts()?)
    }

    /// Death or a stun in the projection ends the gesture without firing.
    pub fn observe(&mut self, snapshot: Ts<ZoneSnapshot>) -> Result<Ts<EndedGesture>, JsError> {
        let snapshot = core_snapshot(&snapshot)?;
        Ok(EndedGesture::from(self.owners.observe(&snapshot)).into_ts()?)
    }

    /// Mouse motion in CSS pixels; `true` when the camera turns with it.
    pub fn motion(&mut self, dx: f64, dy: f64) -> bool {
        self.owners.motion(dx, dy)
    }

    pub fn aim(&self) -> Result<Ts<GestureAim>, JsError> {
        Ok(GestureAim {
            strike: self.owners.strike_aim().map(Into::into),
            guard: self.owners.guard_lane().map(Into::into),
        }
        .into_ts()?)
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
    pub quest: u32,
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
pub fn quest_view(quest: u32, progress: &[u8]) -> Result<JsValue, JsError> {
    let content = hosted_content();
    let view: Option<QuestView> = content
        .quest(core::QuestId::new(quest))
        .map(|record| quest::quest_view(record, progress, &content).into());
    to_js(&view)
}

/// The quest evidence (#1019) a player centred at `[x, y, z]` units would inspect now, or 0: the
/// nearest spot within its reach that an active quest of `sheet` still needs (`mmorpg-client-model`).
#[wasm_bindgen(js_name = evidenceInReach)]
pub fn evidence_in_reach(sheet: Ts<QuestSheet>, x: i32, y: i32, z: i32) -> Result<u32, JsError> {
    let sheet = core::QuestSheet::from(&sheet.to_rust()?);
    Ok(
        quest::evidence_in_reach(&hosted_content(), &sheet.entries, [x, y, z])
            .map_or(0, core::EvidenceId::get),
    )
}

/// The load (#1077) Interact lifts for a player centred at `[x, y, z]` units with the active quests
/// of `sheet`, seeing the set-down `loads`: 0 for its own set-down load in reach, the nearest
/// load source in reach an open delivery names, or -1 for none or while it `carrying` one
/// (`mmorpg-client-model`'s rule; the zone checks everything again).
#[wasm_bindgen(js_name = liftInReach)]
pub fn lift_in_reach(
    sheet: Ts<QuestSheet>,
    x: i32,
    y: i32,
    z: i32,
    carrying: bool,
    #[wasm_bindgen(unchecked_param_type = "SetDownLoad[]")] loads: JsValue,
) -> Result<f64, JsError> {
    let sheet = core::QuestSheet::from(&sheet.to_rust()?);
    let loads: Vec<crate::wire::SetDownLoad> = serde_wasm_bindgen::from_value(loads)?;
    let loads: Vec<_> = loads
        .into_iter()
        .map(|load| core::LoadView {
            kind: load.kind.into(),
            own: load.own,
            at: [load.x, load.y, load.z],
            facing: load.facing,
        })
        .collect();
    Ok(
        match quest::lift_in_reach(
            &hosted_content(),
            &sheet.entries,
            [x, y, z],
            carrying,
            &loads,
        ) {
            Some(core::ZoneCommand::Lift { source }) => {
                f64::from(source.map_or(0, core::LoadSpotId::get))
            }
            _ => -1.,
        },
    )
}

/// What Interact does at NPC `npc` for the active quests of `sheet` (#1019): the label of the
/// quest action it performs there ("Give Clean Bandage to Guardsman Tam Hollis"), or null when
/// Interact talks (`mmorpg-client-model`'s rule; the zone checks everything again).
#[wasm_bindgen(js_name = questActionAt)]
pub fn quest_action_at(sheet: Ts<QuestSheet>, npc: u32) -> Result<Option<String>, JsError> {
    let sheet = core::QuestSheet::from(&sheet.to_rust()?);
    let content = hosted_content();
    Ok(
        quest::quest_action_at(&content, &sheet.entries, core::NpcId::new(npc))
            .map(|objective| quest::objective_label(objective, &content)),
    )
}

/// One quest-tracker row: a quest heading (`objective` null) or one of its objectives.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[tsify(missing_as_null)]
pub struct TrackerRow {
    pub text: String,
    pub quest: u32,
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

/// The HUD line of a running defence (#1019) at assault `phase` with `standing` civilians on
/// their feet, as the raid signal carries them, at the site whose signal reaches the viewer's
/// feet `(x, z)` units when given (#1019); the shared client model's text.
#[wasm_bindgen(js_name = defenceStatus)]
#[must_use]
pub fn defence_status(phase: u8, standing: u8, x: Option<i32>, z: Option<i32>) -> Option<String> {
    quest::defence_status(
        phase,
        standing,
        &hosted_content(),
        x.zip(z).map(|(x, z)| [x, z]),
    )
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
    let snapshot = core_snapshot(&snapshot)?;
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
    pub offered: Vec<u32>,
    pub ready: Vec<QuestView>,
    pub in_progress: Vec<QuestView>,
    /// The decision (#1019) this NPC awaits: its quest and options; send `Decide` with option
    /// `index + 1`.
    pub decision: Option<DecisionView>,
}

/// A quest decision at an NPC (#1019).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
pub struct DecisionView {
    pub quest: u32,
    pub options: Vec<String>,
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
        decision: view.decision.map(|decision| DecisionView {
            quest: decision.quest.get(),
            options: decision.options,
        }),
    }
    .into_ts()?)
}

/// A mechanism (#1019) Interact works: its ID and name.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
pub struct MechanismInReach {
    pub id: u32,
    pub name: String,
}

/// The glyph (#1019) a player centred at `[x, y, z]` units would work now, or null: the nearest
/// mechanism within its reach (`mmorpg-client-model`; the zone checks reach again).
#[wasm_bindgen(js_name = mechanismInReach, unchecked_return_type = "MechanismInReach | null")]
pub fn mechanism_in_reach(x: i32, y: i32, z: i32) -> Result<JsValue, JsError> {
    let content = hosted_content();
    let mechanism =
        quest::mechanism_in_reach(&content, [x, y, z]).map(|mechanism| MechanismInReach {
            id: mechanism.id.get(),
            name: mechanism.name.clone(),
        });
    to_js(&mechanism)
}

/// The HUD lines of the shrine's seals and wards (#1019) in `snapshot`, at its tick: the shared
/// client model's text.
#[wasm_bindgen(js_name = shrineStatus)]
pub fn shrine_status(snapshot: Ts<ZoneSnapshot>) -> Result<Vec<String>, JsError> {
    let snapshot = core_snapshot(&snapshot)?;
    Ok(quest::shrine_status(
        &snapshot.seals,
        &snapshot.wards,
        snapshot.tick,
        &hosted_content(),
    ))
}

/// What a press of Interact does (#1252), from `mmorpg-client-model`'s ladder, in the browser's
/// shape.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum InteractPress {
    /// Send `command`; `label` names it on a button ("Open", "Mine", "Skin").
    Command {
        command: WorldCommand,
        label: String,
    },
    /// Talk to NPC `npc`: open its dialog or trade, selecting it first unless `selected`.
    Talk {
        npc: u32,
        selected: bool,
    },
    /// The drawn weapon refuses it: show `text`.
    SheatheFirst {
        text: String,
    },
    Nothing,
}

/// The browser's wire form of a command the Interact ladder sends.
fn interact_command(command: core::ZoneCommand) -> WorldCommand {
    use core::ZoneCommand as C;
    match command {
        C::Operate { mechanism } => WorldCommand::Operate { mechanism },
        C::RaidPrepare { prep } => WorldCommand::RaidPrepare {
            step: match prep {
                core::RaidPrep::Investigate => 0,
                core::RaidPrep::Report => 1,
                core::RaidPrep::Brace(brace) => brace.saturating_add(2),
            },
        },
        C::Inspect { spot } => WorldCommand::Inspect { spot },
        C::Lift { source } => WorldCommand::Lift {
            source: source.map_or(0, core::LoadSpotId::get),
        },
        C::QuestAction { npc } => WorldCommand::QuestAction { npc_id: npc.get() },
        C::Skin { creature } => WorldCommand::Skin {
            creature_id: creature.get(),
        },
        C::SetDown => WorldCommand::SetDown,
        C::UseDoor { door, open } => WorldCommand::UseDoor {
            door: door.get(),
            open,
        },
        C::Sit { seat } => WorldCommand::Sit {
            seat: seat.map(core::SeatId::get),
        },
        C::Repair { piece } => WorldCommand::Repair {
            piece: piece.map(core::PieceId::get),
        },
        C::RaidAid { task } => WorldCommand::RaidAid { task },
        C::Gather { node } => WorldCommand::Gather { node },
        other => unreachable!("the Interact ladder never sends {other:?}"),
    }
}

impl From<interaction::Interaction> for InteractPress {
    fn from(next: interaction::Interaction) -> Self {
        let label = next.label().to_owned();
        match next {
            interaction::Interaction::Command(command) => Self::Command {
                command: interact_command(command),
                label,
            },
            interaction::Interaction::Talk { npc, selected } => Self::Talk {
                npc: npc.get(),
                selected,
            },
        }
    }
}

/// The active quest entries of a sheet the browser may not have received yet.
fn quest_entries(sheet: JsValue) -> Result<Vec<core::QuestEntry>, JsError> {
    if sheet.is_null() || sheet.is_undefined() {
        return Ok(Vec::new());
    }
    let sheet: QuestSheet = serde_wasm_bindgen::from_value(sheet)?;
    Ok(core::QuestSheet::from(&sheet).entries)
}

/// What a press of Interact does now for the viewer of `snapshot` with the active quests of
/// `sheet` (#1252): the hosted content's one ladder both clients and touch read; the zone checks
/// everything again.
#[wasm_bindgen(js_name = interactPress, unchecked_return_type = "InteractPress")]
pub fn interact_press(
    snapshot: Ts<ZoneSnapshot>,
    #[wasm_bindgen(unchecked_param_type = "QuestSheet | null")] sheet: JsValue,
) -> Result<JsValue, JsError> {
    let snapshot = core_snapshot(&snapshot)?;
    let entries = quest_entries(sheet)?;
    let press = match interaction::press(&snapshot, &hosted_content(), &entries) {
        interaction::Press::Interact(next) => next.into(),
        interaction::Press::SheatheFirst => InteractPress::SheatheFirst {
            text: interaction::SHEATHE_FIRST.to_owned(),
        },
        interaction::Press::Nothing => InteractPress::Nothing,
    };
    to_js(&press)
}

/// What Interact would do with free hands (#1252), for a button that names it; never
/// `sheathe-first`.
#[wasm_bindgen(js_name = nextInteraction, unchecked_return_type = "InteractPress")]
pub fn next_interaction(
    snapshot: Ts<ZoneSnapshot>,
    #[wasm_bindgen(unchecked_param_type = "QuestSheet | null")] sheet: JsValue,
) -> Result<JsValue, JsError> {
    let snapshot = core_snapshot(&snapshot)?;
    let entries = quest_entries(sheet)?;
    let next = interaction::next_interaction(&snapshot, &hosted_content(), &entries)
        .map_or(InteractPress::Nothing, Into::into);
    to_js(&next)
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

/// A named area on the exploration map (#983), in metres (x east, z south): its traced
/// outline (#1063), outer ring first, then any holes, each open.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Tsify)]
pub struct MapArea {
    pub id: u32,
    pub name: String,
    pub rings: Vec<Vec<[f32; 2]>>,
    /// Where the area's name goes.
    pub centre: [f32; 2],
    /// How far `centre` lies from the area's edge.
    pub radius: f32,
    /// Discovered by the viewer, or the starting area.
    pub discovered: bool,
}

/// An approximate area for one open quest objective (#983, Atlas 5.1.13), a circle in metres.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Tsify)]
pub struct QuestHint {
    pub quest: u32,
    pub label: String,
    pub area: u32,
    pub center: [f32; 2],
    pub radius: f32,
}

/// A point of interest in a discovered area (#1063), in metres. `kind` is `waystone`,
/// `dungeon`, `landmark`, `service` or `quest`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Tsify)]
pub struct MapPoint {
    pub kind: String,
    pub name: String,
    pub detail: String,
    pub at: [f32; 2],
}

/// The viewer's exploration map (#983): every named area, the discovered count of the
/// content's discoverable areas, the active quests' approximate areas and the points of
/// interest in the discovered areas (#1063).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Tsify)]
pub struct ExplorationMap {
    pub areas: Vec<MapArea>,
    pub discovered: u32,
    pub total: u32,
    pub hints: Vec<QuestHint>,
    pub points: Vec<MapPoint>,
}

/// The exploration map for the viewer's discovered area IDs and received quest sheet.
#[wasm_bindgen(js_name = explorationMap, unchecked_return_type = "ExplorationMap")]
pub fn exploration_map(discoveries: &[u32], sheet: Ts<QuestSheet>) -> Result<JsValue, JsError> {
    let sheet = core::QuestSheet::from(&sheet.to_rust()?);
    let discoveries = discoveries.iter().copied().map(core::AreaId::new).collect();
    let map = exploration::exploration_map(&hosted_content(), &discoveries, Some(&sheet));
    to_js(&ExplorationMap {
        areas: map
            .areas
            .into_iter()
            .map(|area| MapArea {
                id: area.id.get(),
                name: area.name,
                rings: area.rings,
                centre: area.centre,
                radius: area.radius,
                discovered: area.discovered,
            })
            .collect(),
        discovered: u32::try_from(map.discovered).unwrap_or(u32::MAX),
        total: u32::try_from(map.total).unwrap_or(u32::MAX),
        hints: map
            .hints
            .into_iter()
            .map(|hint| QuestHint {
                quest: hint.quest,
                label: hint.label,
                area: hint.area.get(),
                center: hint.center,
                radius: hint.radius,
            })
            .collect(),
        points: map
            .points
            .into_iter()
            .map(|point| MapPoint {
                kind: point.kind.name().to_owned(),
                name: point.name,
                detail: point.detail,
                at: point.at,
            })
            .collect(),
    })
}
