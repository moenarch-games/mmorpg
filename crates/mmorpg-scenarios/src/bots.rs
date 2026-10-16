//! Scripted bots against the real zone host composition, stepped in-process.
//!
//! The runner builds the same `MatchHost` as `mmorpg-zone-host`, admits bots
//! through the `game-server` session path, submits `mmorpg-protocol` command
//! bytes, advances ticks explicitly and decodes each bot's player-scoped
//! snapshot bytes. It owns no gameplay, session or visibility rules; it only
//! records what those authorities decide and compares it with expectations.

mod units;

use std::collections::BTreeMap;

use game_server::{
    CommandOutcome, MatchHost, MatchId, MatchRuntime, RECONNECT_TOKEN_BYTES, ReconnectToken,
    RuntimeError, SessionLease,
};
use mmorpg_core::{
    Area, EntityKind, EntityRef, EntitySnapshot, MAX_PLAYERS_PER_ZONE, PlayerId, ZoneCommand,
    ZoneId, ZoneSnapshot, greyhaven_vale,
};
use mmorpg_game_server::{ZoneGameServerAdapter, build_zone_host, zone_match_id};
use mmorpg_protocol::{decode_snapshot, encode_command};
use serde::Deserialize;
use serde_json::json;

use crate::report::{Report, snake_kind, validate_name};
use units::{EventSpec, UnitSpec, UnitState, event_token};

pub const SCHEMA: &str = "mmorpg.bot-scenario/v1";
const MAX_TICKS: u64 = 100_000;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BotScenario {
    pub name: String,
    pub zone: u32,
    /// Number of simulation ticks to advance. Observations exist for ticks 1..=ticks.
    pub ticks: u64,
    #[serde(default = "default_grace")]
    pub reconnect_grace_ticks: u64,
    /// Print a digest every N ticks; ticks with steps, expectations or
    /// visibility changes are always printed.
    #[serde(default = "default_interval")]
    pub digest_interval: u64,
    pub bots: Vec<BotSpec>,
    #[serde(default)]
    pub steps: Vec<Step>,
    #[serde(default)]
    pub expect: Vec<Expectation>,
}

fn default_grace() -> u64 {
    120
}

fn default_interval() -> u64 {
    1
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BotSpec {
    pub name: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Join,
    Move,
    Jump,
    SelectTarget,
    StartAttack,
    StopAttack,
    ReleaseSpirit,
    MoveItem,
    EquipItem,
    UnequipItem,
    BuyItem,
    SellItem,
    Chat,
    Emote,
    Loot,
    ChooseClass,
    UseAbility,
    CancelCast,
    AcceptQuest,
    CompleteQuest,
    AbandonQuest,
    Disconnect,
    Reconnect,
}

impl Action {
    fn name(self) -> &'static str {
        match self {
            Self::Join => "join",
            Self::Move => "move",
            Self::Jump => "jump",
            Self::SelectTarget => "select_target",
            Self::StartAttack => "start_attack",
            Self::StopAttack => "stop_attack",
            Self::ReleaseSpirit => "release_spirit",
            Self::MoveItem => "move_item",
            Self::EquipItem => "equip_item",
            Self::UnequipItem => "unequip_item",
            Self::BuyItem => "buy_item",
            Self::SellItem => "sell_item",
            Self::Chat => "chat",
            Self::Emote => "emote",
            Self::Loot => "loot",
            Self::ChooseClass => "choose_class",
            Self::UseAbility => "use_ability",
            Self::CancelCast => "cancel_cast",
            Self::AcceptQuest => "accept_quest",
            Self::CompleteQuest => "complete_quest",
            Self::AbandonQuest => "abandon_quest",
            Self::Disconnect => "disconnect",
            Self::Reconnect => "reconnect",
        }
    }

    fn success(self) -> &'static str {
        match self {
            Self::Join => "joined",
            Self::Move
            | Self::Jump
            | Self::SelectTarget
            | Self::StartAttack
            | Self::StopAttack
            | Self::ReleaseSpirit
            | Self::MoveItem
            | Self::EquipItem
            | Self::UnequipItem
            | Self::BuyItem
            | Self::SellItem
            | Self::Chat
            | Self::Emote
            | Self::Loot
            | Self::ChooseClass
            | Self::UseAbility
            | Self::CancelCast
            | Self::AcceptQuest
            | Self::CompleteQuest
            | Self::AbandonQuest => "applied",
            Self::Disconnect => "disconnected",
            Self::Reconnect => "resumed",
        }
    }

    /// Whether the action submits a zone command through the session.
    const fn is_command(self) -> bool {
        match self {
            Self::Move
            | Self::Jump
            | Self::SelectTarget
            | Self::StartAttack
            | Self::StopAttack
            | Self::ReleaseSpirit
            | Self::MoveItem
            | Self::EquipItem
            | Self::UnequipItem
            | Self::BuyItem
            | Self::SellItem
            | Self::Chat
            | Self::Emote
            | Self::Loot
            | Self::ChooseClass
            | Self::UseAbility
            | Self::CancelCast
            | Self::AcceptQuest
            | Self::CompleteQuest
            | Self::AbandonQuest => true,
            Self::Join | Self::Disconnect | Self::Reconnect => false,
        }
    }
}

/// One bot action, applied before the zone advances from `tick` to `tick + 1`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub tick: u64,
    pub bot: String,
    pub action: Action,
    /// Move intent, passed through unchanged; the zone accepts `-1..=1`.
    pub forward: Option<i8>,
    pub strafe: Option<i8>,
    /// Move heading: 65 536 steps per turn, 0 faces +Z, 16 384 faces +X.
    pub facing: Option<u16>,
    /// The unit a `select_target` step selects (`none` clears), or the
    /// optional target of a `use_ability` step (omitted or `none`: the
    /// current selection).
    pub entity: Option<UnitSpec>,
    /// `choose_class`: `warden`, `ranger` or `arcanist`, passed through as
    /// its wire value; any other name is sent as an invalid value.
    pub class: Option<String>,
    /// `choose_class`: `female` or `male`.
    pub sex: Option<String>,
    /// `use_ability`: the global ability ID, passed through unchanged.
    pub ability: Option<u8>,
    pub creature: Option<u32>,
    pub died_at: Option<u64>,
    pub source_slot: Option<u8>,
    pub destination_slot: Option<u8>,
    pub quantity: Option<u16>,
    /// `equip_item` and `sell_item`: the bag slot, passed through unchanged.
    pub bag_slot: Option<u8>,
    /// `buy_item` and `sell_item`: the vendor's NPC ID; `accept_quest` and
    /// `complete_quest`: the quest's giver or ender. Passed through unchanged.
    pub npc: Option<u32>,
    /// `accept_quest`, `complete_quest` and `abandon_quest`: the quest ID,
    /// passed through unchanged.
    pub quest: Option<u8>,
    /// `complete_quest`: the reward choice index, passed through unchanged.
    pub choice: Option<u8>,
    /// `buy_item`: the offer index in the vendor's stock, passed through unchanged.
    pub offer: Option<u8>,
    /// `chat`: `say` or `yell`.
    pub channel: Option<String>,
    /// `chat`: the line; the runner refuses text core would refuse.
    pub text: Option<String>,
    /// `emote`: `wave`, `bow`, `cheer`, `laugh` or `point`.
    pub emote: Option<String>,
    /// `unequip_item`: the equipment slot (0 main hand … 5 feet), passed
    /// through unchanged.
    pub equipment_slot: Option<u8>,
    /// Command sequence override (commands only); defaults to the bot's next sequence.
    pub seq: Option<u32>,
    /// Connection epoch override; defaults to the bot's current epoch.
    pub connection_epoch: Option<u32>,
    /// Expected outcome tag; defaults to the action's success tag.
    pub expect: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ExpectKind {
    Sees,
    NotSees,
    Position,
    Acknowledged,
    Identity,
    VisibleCount,
    Area,
    Health,
    Progression,
    Inventory,
    Equipment,
    Copper,
    Loot,
    Target,
    Event,
    Unit,
    Resource,
    Chat,
    Quest,
    Marker,
}

impl ExpectKind {
    fn name(self) -> &'static str {
        match self {
            Self::Sees => "sees",
            Self::NotSees => "not_sees",
            Self::Position => "position",
            Self::Acknowledged => "acknowledged",
            Self::Identity => "identity",
            Self::VisibleCount => "visible_count",
            Self::Area => "area",
            Self::Health => "health",
            Self::Progression => "progression",
            Self::Inventory => "inventory",
            Self::Equipment => "equipment",
            Self::Copper => "copper",
            Self::Loot => "loot",
            Self::Target => "target",
            Self::Event => "event",
            Self::Unit => "unit",
            Self::Resource => "resource",
            Self::Chat => "chat",
            Self::Quest => "quest",
            Self::Marker => "marker",
        }
    }

    /// Kinds that may wait for a condition within a window (`by_tick`).
    const fn allows_window(self) -> bool {
        match self {
            Self::Sees | Self::Event | Self::Unit | Self::Resource | Self::Chat => true,
            Self::NotSees
            | Self::Position
            | Self::Acknowledged
            | Self::Identity
            | Self::VisibleCount
            | Self::Area
            | Self::Health
            | Self::Progression
            | Self::Inventory
            | Self::Equipment
            | Self::Copper
            | Self::Loot
            | Self::Target
            | Self::Quest
            | Self::Marker => false,
        }
    }
}

/// A check against the decoded snapshot a bot received at an observed tick.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expectation {
    pub kind: ExpectKind,
    pub bot: String,
    pub target: Option<String>,
    pub tick: Option<u64>,
    pub by_tick: Option<u64>,
    /// With `by_tick`: the window starts at this tick (default 1).
    pub from_tick: Option<u64>,
    pub position: Option<[i32; 3]>,
    pub sequence: Option<u32>,
    pub count: Option<usize>,
    /// Name of the core area the target stands in (`area` expectations).
    pub area: Option<String>,
    /// The bot's own exact health (`health` expectations).
    pub health: Option<u32>,
    pub level: Option<u8>,
    pub experience: Option<u32>,
    pub copper: Option<u32>,
    pub creature: Option<u32>,
    pub died_at: Option<u64>,
    pub inventory_revision: Option<u64>,
    pub sheet: Option<bool>,
    pub slot: Option<u8>,
    pub item: Option<u16>,
    pub quantity: Option<u16>,
    /// The equipment slot an `equipment` expectation checks in the sheet.
    pub equipment_slot: Option<u8>,
    /// The unit a `target`, `event` or `unit` expectation is about.
    pub entity: Option<UnitSpec>,
    /// The feedback event kind of an `event` expectation.
    pub event: Option<EventSpec>,
    /// The visible state of a `unit` expectation.
    pub state: Option<UnitState>,
    /// The bot's exact class resource (`resource` expectations).
    pub resource: Option<u16>,
    /// `chat`: the line heard from bot `target`, or `count` lines in total.
    pub text: Option<String>,
    /// `chat`: the emote heard from bot `target`.
    pub emote: Option<String>,
    /// `quest`: the quest ID checked in the bot's latest quest sheet.
    pub quest: Option<u8>,
    /// `quest`: `active`, `completed` or `absent`.
    pub quest_state: Option<String>,
    /// `quest`: the progress of every objective slot of an active quest.
    pub progress: Option<[u8; mmorpg_core::MAX_QUEST_OBJECTIVES]>,
    /// `marker`: the NPC whose marker the bot's latest quest sheet shows.
    pub npc: Option<u32>,
    /// `marker`: `available`, `in_progress`, `complete` or `none`.
    pub marker: Option<String>,
}

/// Parses and validates a scenario at the file trust boundary.
pub fn load(text: &str) -> Result<BotScenario, String> {
    let scenario: BotScenario = toml::from_str(text).map_err(|error| error.to_string())?;
    validate(&scenario)?;
    Ok(scenario)
}

fn validate(scenario: &BotScenario) -> Result<(), String> {
    validate_name("scenario name", &scenario.name)?;
    if scenario.ticks == 0 || scenario.ticks > MAX_TICKS {
        return Err(format!("ticks must be in 1..={MAX_TICKS}"));
    }
    if scenario.digest_interval == 0 {
        return Err("digest_interval must be positive".into());
    }
    if scenario.bots.is_empty() || scenario.bots.len() > MAX_PLAYERS_PER_ZONE {
        return Err(format!(
            "bots must contain 1..={MAX_PLAYERS_PER_ZONE} entries"
        ));
    }
    let mut names = BTreeMap::new();
    for bot in &scenario.bots {
        validate_name("bot name", &bot.name)?;
        if names.insert(bot.name.as_str(), ()).is_some() {
            return Err(format!("bot {} is declared twice", bot.name));
        }
    }
    let known = |name: &str| {
        names
            .contains_key(name)
            .then_some(())
            .ok_or_else(|| format!("unknown bot {name}"))
    };
    let known_unit = |unit: &UnitSpec| match unit {
        UnitSpec::Bot(name) => known(name),
        UnitSpec::None | UnitSpec::Creature(_) | UnitSpec::Npc(_) => Ok(()),
    };
    let mut previous_tick = 0;
    for (index, step) in scenario.steps.iter().enumerate() {
        let at = format!("steps[{index}]");
        known(&step.bot).map_err(|error| format!("{at}: {error}"))?;
        if step.tick >= scenario.ticks {
            return Err(format!("{at}: tick must be below ticks"));
        }
        if step.tick < previous_tick {
            return Err(format!("{at}: steps must be ordered by tick"));
        }
        previous_tick = step.tick;
        let is_move = step.action == Action::Move;
        let intent = [
            step.forward.is_some(),
            step.strafe.is_some(),
            step.facing.is_some(),
        ];
        if intent.iter().any(|&present| present != is_move) {
            return Err(format!(
                "{at}: forward, strafe and facing are required for move and only for move"
            ));
        }
        let is_inventory_move = step.action == Action::MoveItem;
        if [step.source_slot.is_some(), step.destination_slot.is_some()]
            .iter()
            .any(|&present| present != is_inventory_move)
        {
            return Err(format!(
                "{at}: source_slot and destination_slot are required only for move_item"
            ));
        }
        let trades = matches!(step.action, Action::BuyItem | Action::SellItem);
        if step.quantity.is_some() != (is_inventory_move || trades) {
            return Err(format!(
                "{at}: quantity is required only for move_item, buy_item and sell_item"
            ));
        }
        let at_npc = matches!(step.action, Action::AcceptQuest | Action::CompleteQuest);
        if step.npc.is_some() != (trades || at_npc) {
            return Err(format!(
                "{at}: npc is required only for buy_item, sell_item, accept_quest and complete_quest"
            ));
        }
        if step.quest.is_some() != (at_npc || step.action == Action::AbandonQuest) {
            return Err(format!(
                "{at}: quest is required only for accept_quest, complete_quest and abandon_quest"
            ));
        }
        if step.choice.is_some() != (step.action == Action::CompleteQuest) {
            return Err(format!("{at}: choice is required only for complete_quest"));
        }
        if step.offer.is_some() != (step.action == Action::BuyItem) {
            return Err(format!("{at}: offer is required only for buy_item"));
        }
        let is_chat = step.action == Action::Chat;
        if step.channel.is_some() != is_chat || step.text.is_some() != is_chat {
            return Err(format!("{at}: channel and text are required only for chat"));
        }
        if is_chat {
            chat_channel(step.channel.as_deref().unwrap_or_default())
                .ok_or_else(|| format!("{at}: channel must be say or yell"))?;
            mmorpg_core::ChatText::new(step.text.as_deref().unwrap_or_default())
                .map_err(|error| format!("{at}: {error}"))?;
        }
        if step.emote.is_some() != (step.action == Action::Emote) {
            return Err(format!("{at}: emote is required only for emote"));
        }
        if let Some(name) = &step.emote {
            emote_named(name).ok_or_else(|| format!("{at}: unknown emote {name:?}"))?;
        }
        if [step.creature.is_some(), step.died_at.is_some()]
            .iter()
            .any(|&present| present != (step.action == Action::Loot))
        {
            return Err(format!(
                "{at}: creature and died_at are required only for loot"
            ));
        }
        let targets = matches!(step.action, Action::SelectTarget | Action::UseAbility);
        if (step.entity.is_some() && !targets)
            || (step.entity.is_none() && step.action == Action::SelectTarget)
        {
            return Err(format!(
                "{at}: entity is required for select_target and allowed only for it and use_ability"
            ));
        }
        if [step.class.is_some(), step.sex.is_some()]
            .iter()
            .any(|&present| present != (step.action == Action::ChooseClass))
        {
            return Err(format!(
                "{at}: class and sex are required only for choose_class"
            ));
        }
        if step.bag_slot.is_some() != matches!(step.action, Action::EquipItem | Action::SellItem) {
            return Err(format!(
                "{at}: bag_slot is required only for equip_item and sell_item"
            ));
        }
        if step.equipment_slot.is_some() != (step.action == Action::UnequipItem) {
            return Err(format!(
                "{at}: equipment_slot is required only for unequip_item"
            ));
        }
        if step.ability.is_some() != (step.action == Action::UseAbility) {
            return Err(format!("{at}: ability is required only for use_ability"));
        }
        if let Some(entity) = &step.entity {
            known_unit(entity).map_err(|error| format!("{at}: {error}"))?;
        }
        if !step.action.is_command() && (step.seq.is_some() || step.connection_epoch.is_some()) {
            return Err(format!("{at}: seq/connection_epoch apply only to commands"));
        }
    }
    for (index, expectation) in scenario.expect.iter().enumerate() {
        let at = format!("expect[{index}]");
        known(&expectation.bot).map_err(|error| format!("{at}: {error}"))?;
        if let Some(target) = &expectation.target {
            known(target).map_err(|error| format!("{at}: {error}"))?;
        }
        if let Some(entity) = &expectation.entity {
            known_unit(entity).map_err(|error| format!("{at}: {error}"))?;
        }
        let tick = match (expectation.tick, expectation.by_tick) {
            (Some(tick), None) => tick,
            (None, Some(tick)) if expectation.kind.allows_window() => tick,
            _ => {
                return Err(format!(
                    "{at}: exactly one of tick/by_tick is required (by_tick only for sees, event, unit and resource)"
                ));
            }
        };
        if tick == 0 || tick > scenario.ticks {
            return Err(format!("{at}: tick must be in 1..=ticks"));
        }
        if let Some(from) = expectation.from_tick
            && (expectation.by_tick.is_none() || from == 0 || from > tick)
        {
            return Err(format!(
                "{at}: from_tick needs by_tick and must be in 1..=by_tick"
            ));
        }
        let required = match expectation.kind {
            ExpectKind::Sees | ExpectKind::NotSees => expectation.target.is_some(),
            ExpectKind::Position => expectation.position.is_some(),
            ExpectKind::Acknowledged => expectation.sequence.is_some(),
            ExpectKind::Identity => true,
            ExpectKind::VisibleCount => expectation.count.is_some(),
            ExpectKind::Area => expectation.area.is_some(),
            ExpectKind::Health => expectation.health.is_some(),
            ExpectKind::Progression => {
                expectation.level.is_some() && expectation.experience.is_some()
            }
            ExpectKind::Inventory => {
                let slot_fields = [
                    expectation.slot.is_some(),
                    expectation.item.is_some(),
                    expectation.quantity.is_some(),
                ];
                if slot_fields
                    .iter()
                    .any(|&present| present != expectation.slot.is_some())
                    || expectation.slot.is_some_and(|slot| slot >= 16)
                    || (expectation.sheet != Some(true) && expectation.slot.is_some())
                {
                    return Err(format!(
                        "{at}: inventory slot/item/quantity must be supplied together for a present sheet, slot below 16"
                    ));
                }
                expectation.inventory_revision.is_some() && expectation.sheet.is_some()
            }
            ExpectKind::Equipment => {
                expectation.equipment_slot.is_some_and(|slot| slot < 6)
                    && expectation.item.is_some()
            }
            ExpectKind::Copper => expectation.copper.is_some(),
            ExpectKind::Chat => {
                let known_emote = expectation
                    .emote
                    .as_deref()
                    .is_none_or(|name| emote_named(name).is_some());
                let fields = [
                    expectation.text.is_some(),
                    expectation.emote.is_some(),
                    expectation.count.is_some(),
                ];
                // Exactly one of text, emote or count; a heard line also names its speaker.
                known_emote
                    && fields.iter().filter(|&&set| set).count() == 1
                    && (expectation.count.is_some() || expectation.target.is_some())
            }
            ExpectKind::Loot => {
                expectation.sheet.is_some()
                    && (expectation.sheet == Some(false)
                        || expectation.creature.is_some() && expectation.died_at.is_some())
            }
            ExpectKind::Target => expectation.entity.is_some(),
            ExpectKind::Event => expectation.event.is_some(),
            ExpectKind::Unit => {
                expectation.state.is_some()
                    && expectation
                        .entity
                        .as_ref()
                        .is_some_and(|entity| *entity != UnitSpec::None)
            }
            ExpectKind::Resource => expectation.resource.is_some(),
            ExpectKind::Quest => {
                let state = expectation.quest_state.as_deref();
                expectation.quest.is_some()
                    && matches!(state, Some("active" | "completed" | "absent"))
                    && (expectation.progress.is_none() || state == Some("active"))
            }
            ExpectKind::Marker => {
                expectation.npc.is_some()
                    && matches!(
                        expectation.marker.as_deref(),
                        Some("available" | "in_progress" | "complete" | "none")
                    )
            }
        };
        if !required {
            return Err(format!(
                "{at}: {} is missing its required field",
                expectation.kind.name()
            ));
        }
    }
    Ok(())
}

struct Bot {
    name: String,
    index: u32,
    tokens_minted: u32,
    lease: Option<SessionLease>,
    first_player: Option<PlayerId>,
    connected: bool,
    next_sequence: u32,
    view: Option<ZoneSnapshot>,
    previous_sees: Vec<String>,
    /// Digest of the bot's own combat state, empty while unhurt and idle.
    previous_status: String,
    /// The latest quest sheet the bot received; sheets are periodic.
    quests: Option<mmorpg_core::QuestSheet>,
}

impl Bot {
    /// Deterministic, per-bot unique tokens. They are test credentials only.
    fn mint_token(&mut self) -> ReconnectToken {
        self.tokens_minted += 1;
        let mut bytes = [0; RECONNECT_TOKEN_BYTES];
        bytes[..4].copy_from_slice(&(self.index + 1).to_be_bytes());
        bytes[4..8].copy_from_slice(&self.tokens_minted.to_be_bytes());
        ReconnectToken(bytes)
    }
}

struct Runner<'a> {
    scenario: &'a BotScenario,
    host: MatchHost<ZoneGameServerAdapter>,
    match_id: MatchId,
    bots: Vec<Bot>,
    report: Report,
    satisfied: Vec<bool>,
}

/// Runs a scenario. `Err` means the harness itself could not be built or
/// driven; scenario failures are recorded in the returned report instead.
pub fn run(scenario: &BotScenario) -> Result<Report, String> {
    let zone_id = ZoneId::new(scenario.zone);
    let host = build_zone_host([zone_id], scenario.reconnect_grace_ticks)
        .map_err(|error| error.to_string())?;
    let match_id = zone_match_id(zone_id).map_err(|error| error.to_string())?;
    let bots = (0_u32..)
        .zip(&scenario.bots)
        .map(|(index, spec)| Bot {
            name: spec.name.clone(),
            index,
            tokens_minted: 0,
            lease: None,
            first_player: None,
            connected: false,
            next_sequence: 1,
            view: None,
            previous_sees: Vec::new(),
            previous_status: String::new(),
            quests: None,
        })
        .collect();
    let mut runner = Runner {
        scenario,
        host,
        match_id,
        bots,
        report: Report::new(SCHEMA),
        satisfied: vec![false; scenario.expect.len()],
    };
    runner.run()?;
    Ok(runner.report)
}

impl Runner<'_> {
    fn runtime<R>(
        &mut self,
        operation: impl FnOnce(&mut MatchRuntime<ZoneGameServerAdapter>) -> R,
    ) -> Result<R, String> {
        self.host
            .with_runtime_mut(&self.match_id, operation)
            .ok_or_else(|| format!("match {} is not hosted", self.match_id.as_str()))
    }

    fn bot_index(&self, name: &str) -> Result<usize, String> {
        self.bots
            .iter()
            .position(|bot| bot.name == name)
            .ok_or_else(|| format!("unknown bot {name}"))
    }

    fn run(&mut self) -> Result<(), String> {
        let scenario = self.scenario;
        self.report.line(
            "scenario",
            format!(
                "scenario {} zone={} bots={} ticks={} grace={}",
                scenario.name,
                scenario.zone,
                scenario.bots.len(),
                scenario.ticks,
                scenario.reconnect_grace_ticks
            ),
            json!({
                "scenario": scenario.name,
                "zone": scenario.zone,
                "bots": scenario.bots.iter().map(|bot| bot.name.as_str()).collect::<Vec<_>>(),
                "ticks": scenario.ticks,
                "reconnect_grace_ticks": scenario.reconnect_grace_ticks,
            }),
        );
        let mut next_step = 0;
        for tick in 0..scenario.ticks {
            let mut stepped = false;
            while let Some(step) = scenario.steps.get(next_step) {
                if step.tick != tick {
                    break;
                }
                self.apply(step)?;
                next_step += 1;
                stepped = true;
            }
            let observed = tick + 1;
            if let Err(error) = self.runtime(MatchRuntime::advance_tick)? {
                self.report.fail();
                self.report.line(
                    "error",
                    format!("t={observed} advance_tick FAIL {error}"),
                    json!({ "tick": observed, "error": error.to_string() }),
                );
                break;
            }
            let changed = self.observe(observed)?;
            let checked = self.evaluate(observed);
            if stepped
                || changed
                || checked
                || observed % scenario.digest_interval == 0
                || observed == scenario.ticks
            {
                self.digest(observed);
            }
        }
        let failures = self.report.failures();
        let verdict = if failures == 0 { "PASS" } else { "FAIL" };
        self.report.line(
            "result",
            format!(
                "{verdict} {} ticks={} steps={} expectations={} failures={failures}",
                scenario.name,
                scenario.ticks,
                scenario.steps.len(),
                scenario.expect.len()
            ),
            json!({
                "scenario": scenario.name,
                "passed": failures == 0,
                "ticks": scenario.ticks,
                "steps": scenario.steps.len(),
                "expectations": scenario.expect.len(),
                "failures": failures,
            }),
        );
        Ok(())
    }

    fn apply(&mut self, step: &Step) -> Result<(), String> {
        let index = self.bot_index(&step.bot)?;
        let (tag, detail) = match step.action {
            Action::Join => self.join(index)?,
            Action::Move
            | Action::Jump
            | Action::SelectTarget
            | Action::StartAttack
            | Action::StopAttack
            | Action::ReleaseSpirit
            | Action::MoveItem
            | Action::EquipItem
            | Action::UnequipItem
            | Action::BuyItem
            | Action::SellItem
            | Action::Chat
            | Action::Emote
            | Action::Loot
            | Action::ChooseClass
            | Action::UseAbility
            | Action::CancelCast
            | Action::AcceptQuest
            | Action::CompleteQuest
            | Action::AbandonQuest => self.submit(index, step)?,
            Action::Disconnect => self.disconnect(index)?,
            Action::Reconnect => self.reconnect(index)?,
        };
        let expected = step
            .expect
            .as_deref()
            .unwrap_or_else(|| step.action.success());
        let ok = tag == expected;
        let mut text = format!(
            "t={} {} {} -> {tag}",
            step.tick,
            step.bot,
            step.action.name()
        );
        if !detail.is_empty() {
            text.push(' ');
            text.push_str(&detail);
        }
        if !ok {
            self.report.fail();
            text.push_str(&format!(" FAIL expected {expected}"));
        }
        self.report.line(
            "step",
            text,
            json!({
                "tick": step.tick,
                "bot": step.bot,
                "action": step.action.name(),
                "outcome": tag,
                "detail": detail,
                "expected": expected,
                "ok": ok,
            }),
        );
        Ok(())
    }

    fn join(&mut self, index: usize) -> Result<(String, String), String> {
        let token = self.bots[index].mint_token();
        Ok(match self.runtime(|runtime| runtime.admit(token))? {
            Ok(lease) => {
                let bot = &mut self.bots[index];
                bot.first_player.get_or_insert(lease.player_id);
                bot.lease = Some(lease);
                bot.connected = true;
                (
                    "joined".into(),
                    format!(
                        "player={} epoch={}",
                        lease.player_id, lease.connection_epoch
                    ),
                )
            }
            Err(error) => (rejected(&error), String::new()),
        })
    }

    fn submit(&mut self, index: usize, step: &Step) -> Result<(String, String), String> {
        let Some(lease) = self.bots[index].lease else {
            return Ok(("rejected:no_session".into(), String::new()));
        };
        let (command, intent) = match step.action {
            Action::Move => {
                let (forward, strafe, facing) = (
                    step.forward.unwrap_or_default(),
                    step.strafe.unwrap_or_default(),
                    step.facing.unwrap_or_default(),
                );
                (
                    ZoneCommand::Move {
                        forward,
                        strafe,
                        facing,
                    },
                    format!(" forward={forward} strafe={strafe} facing={facing}"),
                )
            }
            Action::Jump => (ZoneCommand::Jump, String::new()),
            Action::SelectTarget => {
                let entity = step.entity.as_ref().unwrap_or(&UnitSpec::None);
                match entity.resolve(|name| self.player_of(name)) {
                    Ok(target) => (
                        ZoneCommand::SelectTarget(target),
                        format!(" entity={entity}"),
                    ),
                    // The named bot has no player to select yet.
                    Err(_) => return Ok(("rejected:unknown_entity".into(), String::new())),
                }
            }
            Action::StartAttack => (ZoneCommand::StartAttack, String::new()),
            Action::StopAttack => (ZoneCommand::StopAttack, String::new()),
            Action::ReleaseSpirit => (ZoneCommand::ReleaseSpirit, String::new()),
            Action::Loot => {
                let creature = step.creature.ok_or("loot creature is required")?;
                let died_at = step.died_at.ok_or("loot died_at is required")?;
                (
                    ZoneCommand::Loot(mmorpg_core::LootClaim {
                        creature: mmorpg_core::CreatureId::new(creature),
                        died_at,
                    }),
                    format!(" creature={creature} died_at={died_at}"),
                )
            }
            Action::MoveItem => {
                let source = step
                    .source_slot
                    .ok_or("move_item source_slot is required")?;
                let destination = step
                    .destination_slot
                    .ok_or("move_item destination_slot is required")?;
                let quantity = step.quantity.ok_or("move_item quantity is required")?;
                (
                    ZoneCommand::MoveItem {
                        source,
                        destination,
                        quantity,
                    },
                    format!(
                        " source_slot={source} destination_slot={destination} quantity={quantity}"
                    ),
                )
            }
            Action::EquipItem => {
                let bag_slot = step.bag_slot.ok_or("equip_item bag_slot is required")?;
                (
                    ZoneCommand::EquipItem { bag_slot },
                    format!(" bag_slot={bag_slot}"),
                )
            }
            Action::Chat => {
                let channel = step.channel.as_deref().unwrap_or_default();
                let text = step.text.as_deref().unwrap_or_default();
                (
                    ZoneCommand::Chat {
                        channel: chat_channel(channel).ok_or("chat channel is invalid")?,
                        text: mmorpg_core::ChatText::new(text)
                            .map_err(|error| error.to_string())?,
                    },
                    format!(" channel={channel} text={text:?}"),
                )
            }
            Action::Emote => {
                let name = step.emote.as_deref().unwrap_or_default();
                (
                    ZoneCommand::Emote(emote_named(name).ok_or("emote is invalid")?),
                    format!(" emote={name}"),
                )
            }
            Action::AcceptQuest => {
                let npc = step.npc.ok_or("accept_quest npc is required")?;
                let quest = step.quest.ok_or("accept_quest quest is required")?;
                (
                    ZoneCommand::AcceptQuest {
                        npc: mmorpg_core::NpcId::new(npc),
                        quest,
                    },
                    format!(" npc={npc} quest={quest}"),
                )
            }
            Action::CompleteQuest => {
                let npc = step.npc.ok_or("complete_quest npc is required")?;
                let quest = step.quest.ok_or("complete_quest quest is required")?;
                let choice = step.choice.ok_or("complete_quest choice is required")?;
                (
                    ZoneCommand::CompleteQuest {
                        npc: mmorpg_core::NpcId::new(npc),
                        quest,
                        choice,
                    },
                    format!(" npc={npc} quest={quest} choice={choice}"),
                )
            }
            Action::AbandonQuest => {
                let quest = step.quest.ok_or("abandon_quest quest is required")?;
                (
                    ZoneCommand::AbandonQuest { quest },
                    format!(" quest={quest}"),
                )
            }
            Action::BuyItem => {
                let npc = step.npc.ok_or("buy_item npc is required")?;
                let offer = step.offer.ok_or("buy_item offer is required")?;
                let quantity = step.quantity.ok_or("buy_item quantity is required")?;
                (
                    ZoneCommand::BuyItem {
                        npc: mmorpg_core::NpcId::new(npc),
                        offer,
                        quantity,
                    },
                    format!(" npc={npc} offer={offer} quantity={quantity}"),
                )
            }
            Action::SellItem => {
                let npc = step.npc.ok_or("sell_item npc is required")?;
                let bag_slot = step.bag_slot.ok_or("sell_item bag_slot is required")?;
                let quantity = step.quantity.ok_or("sell_item quantity is required")?;
                (
                    ZoneCommand::SellItem {
                        npc: mmorpg_core::NpcId::new(npc),
                        bag_slot,
                        quantity,
                    },
                    format!(" npc={npc} bag_slot={bag_slot} quantity={quantity}"),
                )
            }
            Action::UnequipItem => {
                let equipment_slot = step
                    .equipment_slot
                    .ok_or("unequip_item equipment_slot is required")?;
                (
                    ZoneCommand::UnequipItem { equipment_slot },
                    format!(" equipment_slot={equipment_slot}"),
                )
            }
            Action::ChooseClass => {
                let class = step.class.as_deref().unwrap_or_default();
                let sex = step.sex.as_deref().unwrap_or_default();
                (
                    ZoneCommand::ChooseClass {
                        class: class_code(class),
                        sex: sex_code(sex),
                    },
                    format!(" class={class} sex={sex}"),
                )
            }
            Action::UseAbility => {
                let ability = step.ability.ok_or("use_ability ability is required")?;
                let entity = step.entity.as_ref().unwrap_or(&UnitSpec::None);
                match entity.resolve(|name| self.player_of(name)) {
                    Ok(target) => (
                        ZoneCommand::UseAbility { ability, target },
                        format!(" ability={ability} entity={entity}"),
                    ),
                    Err(_) => return Ok(("rejected:unknown_entity".into(), String::new())),
                }
            }
            Action::CancelCast => (ZoneCommand::CancelCast, String::new()),
            Action::Join | Action::Disconnect | Action::Reconnect => {
                return Err(format!("{} is not a command", step.action.name()));
            }
        };
        let bot = &mut self.bots[index];
        let sequence = step.seq.unwrap_or(bot.next_sequence);
        let epoch = step.connection_epoch.unwrap_or(lease.connection_epoch);
        bot.next_sequence = bot.next_sequence.max(sequence.saturating_add(1));
        let payload = encode_command(command);
        let detail = format!("seq={sequence} epoch={epoch}{intent}");
        let outcome = self.runtime(|runtime| {
            runtime.submit_command(lease.player_id, epoch, sequence, &payload)
        })?;
        let tag = match outcome {
            Ok(CommandOutcome::Applied) => "applied".into(),
            Ok(CommandOutcome::IgnoredStale) => "ignored_stale".into(),
            Err(error) => rejected(&error),
        };
        Ok((tag, detail))
    }

    fn disconnect(&mut self, index: usize) -> Result<(String, String), String> {
        let Some(lease) = self.bots[index].lease else {
            return Ok(("rejected:no_session".into(), String::new()));
        };
        let disconnected =
            self.runtime(|runtime| runtime.disconnect(lease.player_id, lease.connection_epoch))?;
        if !disconnected {
            return Ok(("rejected:not_connected".into(), String::new()));
        }
        let bot = &mut self.bots[index];
        bot.connected = false;
        bot.view = None;
        Ok(("disconnected".into(), format!("player={}", lease.player_id)))
    }

    fn reconnect(&mut self, index: usize) -> Result<(String, String), String> {
        let Some(previous) = self.bots[index].lease else {
            return Ok(("rejected:no_session".into(), String::new()));
        };
        let replacement = self.bots[index].mint_token();
        Ok(
            match self
                .runtime(|runtime| runtime.reconnect(previous.reconnect_token, replacement))?
            {
                Ok(lease) => {
                    let bot = &mut self.bots[index];
                    bot.lease = Some(lease);
                    bot.connected = true;
                    let identity = if bot.first_player == Some(lease.player_id) {
                        "kept"
                    } else {
                        "changed"
                    };
                    (
                        "resumed".into(),
                        format!(
                            "player={} epoch={} identity={identity}",
                            lease.player_id, lease.connection_epoch
                        ),
                    )
                }
                Err(error) => (rejected(&error), String::new()),
            },
        )
    }

    /// Decodes every connected bot's snapshot bytes. Returns whether any
    /// bot's visible set or own combat state changed or it received events.
    fn observe(&mut self, tick: u64) -> Result<bool, String> {
        let zone = ZoneId::new(self.scenario.zone);
        let mut changed = false;
        for index in 0..self.bots.len() {
            let bot = &self.bots[index];
            let Some(lease) = bot.lease.filter(|_| bot.connected) else {
                continue;
            };
            let bytes = self.runtime(|runtime| runtime.snapshot_for(lease.player_id))?;
            let decoded = bytes
                .map_err(|error| error.to_string())
                .and_then(|snapshot| {
                    decode_snapshot(&snapshot.payload).map_err(|error| error.to_string())
                })
                .and_then(|snapshot| {
                    if snapshot.zone_id != zone
                        || snapshot.tick != tick
                        || snapshot.viewer_id != lease.player_id
                    {
                        Err(format!(
                            "snapshot for zone {} tick {} viewer {} does not match zone {} tick {tick} viewer {}",
                            snapshot.zone_id.get(),
                            snapshot.tick,
                            snapshot.viewer_id,
                            zone.get(),
                            lease.player_id
                        ))
                    } else {
                        Ok(snapshot)
                    }
                });
            match decoded {
                Ok(snapshot) => {
                    let sees = self.sees(&snapshot);
                    let status = self.status(&snapshot);
                    let bot = &mut self.bots[index];
                    changed |= sees != bot.previous_sees
                        || status != bot.previous_status
                        || !snapshot.events.is_empty();
                    bot.previous_sees = sees;
                    bot.previous_status = status;
                    if let Some(quests) = &snapshot.quests {
                        bot.quests = Some(quests.clone());
                    }
                    bot.view = Some(snapshot);
                }
                Err(error) => {
                    self.report.fail();
                    let name = self.bots[index].name.clone();
                    self.report.line(
                        "error",
                        format!("t={tick} {name} snapshot FAIL {error}"),
                        json!({ "tick": tick, "bot": name, "error": error }),
                    );
                    self.bots[index].view = None;
                }
            }
        }
        Ok(changed)
    }

    fn player_name(&self, player_id: PlayerId) -> String {
        self.bots
            .iter()
            .find(|bot| bot.lease.is_some_and(|lease| lease.player_id == player_id))
            .map_or_else(|| format!("p{player_id}"), |bot| bot.name.clone())
    }

    fn sees(&self, snapshot: &ZoneSnapshot) -> Vec<String> {
        snapshot
            .entities
            .iter()
            .filter_map(player_of_entity)
            .filter(|&player_id| player_id != snapshot.viewer_id)
            .map(|player_id| self.player_name(player_id))
            .collect()
    }

    fn player_of(&self, name: &str) -> Option<PlayerId> {
        self.bots
            .iter()
            .find(|bot| bot.name == name)
            .and_then(|bot| bot.lease)
            .map(|lease| lease.player_id)
    }

    /// Players by bot name, other units as `creature:<id>` or `npc:<id>`.
    fn unit_name(&self, entity: EntityRef) -> String {
        match entity {
            EntityRef::Player(player_id) => self.player_name(player_id),
            EntityRef::Creature(id) => format!("creature:{}", id.get()),
            EntityRef::Npc(id) => format!("npc:{}", id.get()),
        }
    }

    /// The bot's own combat state for digests: health while hurt, death,
    /// target, auto-attack and combat. Empty while unhurt and idle.
    fn status(&self, view: &ZoneSnapshot) -> String {
        let me = &view.viewer;
        let mut parts = Vec::new();
        if me.health != me.max_health {
            parts.push(format!("hp{}/{}", me.health, me.max_health));
        }
        if me.dead {
            parts.push("dead".into());
        }
        if let Some(target) = me.target {
            parts.push(format!("target={}", self.unit_name(target)));
        }
        if me.auto_attacking {
            parts.push("attacking".into());
        }
        if me.in_combat {
            parts.push("in_combat".into());
        }
        if let Some(resource) = me.resource {
            parts.push(format!(
                "{}{}/{}",
                resource.kind.name(),
                resource.value,
                resource.max
            ));
        }
        // Cast progress changes every tick; the digest names the ability.
        if let Some(cast) = me.cast {
            parts.push(format!("casting{}", cast.ability.get()));
        }
        parts.join(" ")
    }

    fn event_tokens(&self, view: &ZoneSnapshot) -> Vec<String> {
        view.events
            .iter()
            .map(|event| event_token(event, |unit| self.unit_name(unit)))
            .collect()
    }

    fn digest(&mut self, tick: u64) {
        let mut parts = Vec::new();
        let mut objects = Vec::new();
        for bot in &self.bots {
            let Some(lease) = bot.lease else { continue };
            let view = bot.view.as_ref().filter(|_| bot.connected);
            let Some(view) = view else {
                parts.push(format!("{} p{} offline", bot.name, lease.player_id));
                objects.push(json!({
                    "bot": bot.name, "player": lease.player_id, "connected": false,
                }));
                continue;
            };
            let position = visible_player(view, lease.player_id).map(|entity| entity.position);
            let position_text =
                position.map_or_else(|| "(absent)".into(), |[x, y, z]| format!("({x},{y},{z})"));
            let events = self.event_tokens(view);
            let mut text = format!(
                "{} p{} e{} ack{} {position_text} sees[{}]",
                bot.name,
                lease.player_id,
                lease.connection_epoch,
                view.acknowledged_sequence,
                bot.previous_sees.join(",")
            );
            if !bot.previous_status.is_empty() {
                text.push(' ');
                text.push_str(&bot.previous_status);
            }
            if !events.is_empty() {
                text.push_str(&format!(" events[{}]", events.join(",")));
            }
            parts.push(text);
            let me = &view.viewer;
            objects.push(json!({
                "bot": bot.name,
                "player": lease.player_id,
                "connected": true,
                "epoch": lease.connection_epoch,
                "ack": view.acknowledged_sequence,
                "position": position,
                "sees": bot.previous_sees,
                "health": me.health,
                "max_health": me.max_health,
                "dead": me.dead,
                "in_combat": me.in_combat,
                "auto_attacking": me.auto_attacking,
                "target": me.target.map(|target| self.unit_name(target)),
                "events": events,
            }));
        }
        self.report.line(
            "tick",
            format!("t={tick} {}", parts.join(" | ")),
            json!({ "tick": tick, "bots": objects }),
        );
    }

    /// Evaluates expectations that are due at `tick`. Returns whether any ran.
    fn evaluate(&mut self, tick: u64) -> bool {
        let scenario = self.scenario;
        let mut any = false;
        for (index, expectation) in scenario.expect.iter().enumerate() {
            let result = if expectation.tick == Some(tick) {
                self.check(expectation)
            } else if let Some(by_tick) = expectation.by_tick {
                let from = expectation.from_tick.unwrap_or(1);
                if self.satisfied[index] || tick < from || tick > by_tick {
                    continue;
                }
                match self.check(expectation) {
                    Ok(detail) => {
                        self.satisfied[index] = true;
                        Ok(detail)
                    }
                    Err(_) if tick < by_tick => continue,
                    Err(reason) => Err(reason),
                }
            } else {
                continue;
            };
            any = true;
            let label = describe(expectation);
            let (ok, detail) = match result {
                Ok(detail) => (true, detail),
                Err(reason) => (false, reason),
            };
            if !ok {
                self.report.fail();
            }
            let verdict = if ok { "ok" } else { "FAIL" };
            self.report.line(
                "expect",
                format!("t={tick} expect {label} {verdict} {detail}"),
                json!({
                    "tick": tick,
                    "index": index,
                    "kind": expectation.kind.name(),
                    "expectation": label,
                    "ok": ok,
                    "detail": detail,
                }),
            );
        }
        any
    }

    fn check(&self, expectation: &Expectation) -> Result<String, String> {
        let bot = self
            .bots
            .iter()
            .find(|bot| bot.name == expectation.bot)
            .ok_or("unknown bot")?;
        let view = bot
            .view
            .as_ref()
            .filter(|_| bot.connected)
            .ok_or_else(|| format!("{} received no snapshot", bot.name))?;
        let target_name = expectation.target.as_deref().unwrap_or(&bot.name);
        let target = self.player_of(target_name);
        let visible = |player: Option<PlayerId>| player.and_then(|id| visible_player(view, id));
        match expectation.kind {
            ExpectKind::Sees => visible(target)
                .map(|record| format!("{target_name} at {:?}", record.position))
                .ok_or_else(|| format!("{target_name} not visible")),
            ExpectKind::NotSees => match visible(target) {
                None => Ok(format!("{target_name} not visible")),
                Some(record) => Err(format!("{target_name} visible at {:?}", record.position)),
            },
            ExpectKind::Position => {
                let expected = expectation.position.unwrap_or_default();
                let record = visible(target).ok_or_else(|| format!("{target_name} not visible"))?;
                if record.position == expected {
                    Ok(format!("{:?}", record.position))
                } else {
                    Err(format!("got {:?}", record.position))
                }
            }
            ExpectKind::Acknowledged => {
                let expected = expectation.sequence.unwrap_or_default();
                if view.acknowledged_sequence == expected {
                    Ok(format!("ack={expected}"))
                } else {
                    Err(format!("got ack={}", view.acknowledged_sequence))
                }
            }
            ExpectKind::Identity => {
                let lease = bot.lease.ok_or("no session")?;
                if bot.first_player != Some(lease.player_id) {
                    return Err(format!(
                        "player changed from {:?} to {}",
                        bot.first_player, lease.player_id
                    ));
                }
                visible(Some(lease.player_id))
                    .map(|_| {
                        format!(
                            "player={} epoch={}",
                            lease.player_id, lease.connection_epoch
                        )
                    })
                    .ok_or_else(|| "own player missing from snapshot".into())
            }
            ExpectKind::VisibleCount => {
                let expected = expectation.count.unwrap_or_default();
                let count = view.entities.iter().filter_map(player_of_entity).count();
                if count == expected {
                    Ok(format!("count={expected}"))
                } else {
                    Err(format!("got count={count}"))
                }
            }
            ExpectKind::Area => {
                let expected = expectation.area.as_deref().unwrap_or_default();
                let [x, _, z] = visible(target)
                    .ok_or_else(|| format!("{target_name} not visible"))?
                    .position;
                // The areas of the vale content that `build_zone_host` installs.
                match greyhaven_vale::area_at(x, z).map(Area::name) {
                    Some(name) if name == expected => Ok(format!("{target_name} in {name}")),
                    Some(name) => Err(format!("{target_name} in {name}")),
                    None => Err(format!("{target_name} in no named area")),
                }
            }
            ExpectKind::Resource => {
                let shown = view.viewer.resource.map_or_else(
                    || "resource=none".to_owned(),
                    |resource| {
                        format!(
                            "{}={}/{}",
                            resource.kind.name(),
                            resource.value,
                            resource.max
                        )
                    },
                );
                if view.viewer.resource.map(|resource| resource.value) == expectation.resource {
                    Ok(shown)
                } else {
                    Err(format!("got {shown}"))
                }
            }
            ExpectKind::Chat => {
                let heard: Vec<String> = view
                    .chat
                    .iter()
                    .map(|line| {
                        let speaker = self.player_name(line.speaker);
                        match line.message {
                            mmorpg_core::ChatMessage::Say(text) => {
                                format!("{speaker} says {:?}", text.as_str())
                            }
                            mmorpg_core::ChatMessage::Yell(text) => {
                                format!("{speaker} yells {:?}", text.as_str())
                            }
                            mmorpg_core::ChatMessage::Emote(emote) => {
                                format!("{speaker} emotes {}", emote_name(emote))
                            }
                        }
                    })
                    .collect();
                let shown = format!("chat[{}]", heard.join(", "));
                let from = |speaker: &String, line: &mmorpg_core::ChatLine| {
                    Some(line.speaker) == self.player_of(speaker)
                };
                let matched = match (
                    &expectation.text,
                    &expectation.emote,
                    &expectation.target,
                    expectation.count,
                ) {
                    (Some(text), None, Some(speaker), _) => view.chat.iter().any(|line| {
                        from(speaker, line)
                            && matches!(
                                line.message,
                                mmorpg_core::ChatMessage::Say(heard)
                                    | mmorpg_core::ChatMessage::Yell(heard)
                                    if heard.as_str() == text
                            )
                    }),
                    (None, Some(name), Some(speaker), _) => view.chat.iter().any(|line| {
                        from(speaker, line)
                            && emote_named(name).is_some_and(|emote| {
                                line.message == mmorpg_core::ChatMessage::Emote(emote)
                            })
                    }),
                    (None, None, _, Some(count)) => view.chat.len() == count,
                    _ => false,
                };
                if matched {
                    Ok(shown)
                } else {
                    Err(format!("got {shown}"))
                }
            }
            ExpectKind::Health => {
                let me = &view.viewer;
                let shown = format!("health={}/{}", me.health, me.max_health);
                if Some(me.health) == expectation.health {
                    Ok(shown)
                } else {
                    Err(format!("got {shown}"))
                }
            }
            ExpectKind::Progression => {
                let shown = format!(
                    "level={} xp={}/{}",
                    view.viewer.level, view.viewer.experience, view.viewer.experience_to_next_level
                );
                if Some(view.viewer.level) == expectation.level
                    && Some(view.viewer.experience) == expectation.experience
                {
                    Ok(shown)
                } else {
                    Err(format!("got {shown}"))
                }
            }
            ExpectKind::Quest => {
                let sheet = bot.quests.as_ref().ok_or("no quest sheet received")?;
                let quest = mmorpg_core::QuestId::new(expectation.quest.unwrap_or_default());
                let entry = sheet.entries.iter().find(|entry| entry.quest == quest);
                let completed = quest.bit().is_some_and(|bit| sheet.completed & bit != 0);
                let (state, progress) = match (entry, completed) {
                    (Some(entry), _) => ("active", Some(entry.progress)),
                    (None, true) => ("completed", None),
                    (None, false) => ("absent", None),
                };
                let shown = progress.map_or_else(
                    || format!("quest={} {state}", quest.get()),
                    |progress| format!("quest={} {state} progress={progress:?}", quest.get()),
                );
                if Some(state) == expectation.quest_state.as_deref()
                    && expectation
                        .progress
                        .is_none_or(|expected| Some(expected) == progress)
                {
                    Ok(shown)
                } else {
                    Err(format!("got {shown}"))
                }
            }
            ExpectKind::Marker => {
                let sheet = bot.quests.as_ref().ok_or("no quest sheet received")?;
                let npc = expectation.npc.unwrap_or_default();
                let marker = sheet
                    .markers
                    .iter()
                    .find(|marker| marker.npc.get() == npc)
                    .map_or("none", |marker| match marker.marker {
                        mmorpg_core::QuestMarker::Available => "available",
                        mmorpg_core::QuestMarker::InProgress => "in_progress",
                        mmorpg_core::QuestMarker::Complete => "complete",
                    });
                let shown = format!("npc={npc} marker={marker}");
                if Some(marker) == expectation.marker.as_deref() {
                    Ok(shown)
                } else {
                    Err(format!("got {shown}"))
                }
            }
            ExpectKind::Copper => {
                let shown = format!("copper={}", view.viewer.copper);
                if Some(view.viewer.copper) == expectation.copper {
                    Ok(shown)
                } else {
                    Err(format!("got {shown}"))
                }
            }
            ExpectKind::Loot => {
                let shown = view.loot.map_or_else(
                    || "sheet=false".to_owned(),
                    |loot| {
                        format!(
                            "sheet=true creature={} died_at={} copper={}",
                            loot.claim.creature.get(),
                            loot.claim.died_at,
                            loot.rewards.money
                        )
                    },
                );
                let matches = Some(view.loot.is_some()) == expectation.sheet
                    && view.loot.is_none_or(|loot| {
                        Some(loot.claim.creature.get()) == expectation.creature
                            && Some(loot.claim.died_at) == expectation.died_at
                    });
                if matches {
                    Ok(shown)
                } else {
                    Err(format!("got {shown}"))
                }
            }
            ExpectKind::Inventory => {
                let shown = format!(
                    "revision={} sheet={}",
                    view.inventory_revision,
                    view.inventory.is_some()
                );
                if Some(view.inventory_revision) != expectation.inventory_revision
                    || Some(view.inventory.is_some()) != expectation.sheet
                {
                    return Err(format!("got {shown}"));
                }
                if let Some(slot) = expectation.slot {
                    let bag = view.inventory.as_ref().ok_or("inventory sheet is absent")?;
                    let stack = bag.slots()[usize::from(slot)];
                    let (item, quantity) =
                        stack.map_or((0, 0), |stack| (stack.item().get(), stack.quantity()));
                    let shown = format!("{shown} slot={slot} item={item} quantity={quantity}");
                    if Some(item) != expectation.item || Some(quantity) != expectation.quantity {
                        Err(format!("got {shown}"))
                    } else {
                        Ok(shown)
                    }
                } else {
                    Ok(shown)
                }
            }
            ExpectKind::Equipment => {
                let slot = expectation
                    .equipment_slot
                    .ok_or("equipment expectation needs a slot")?;
                let equipment = view.equipment.ok_or("equipment sheet is absent")?;
                let item = equipment.slots()[usize::from(slot)].map_or(0, mmorpg_core::ItemId::get);
                let totals = equipment.totals();
                let shown = format!(
                    "revision={} slot={slot} item={item} stats={}/{}/{}/{}",
                    view.inventory_revision,
                    totals.stamina,
                    totals.strength,
                    totals.agility,
                    totals.intellect
                );
                if Some(item) == expectation.item {
                    Ok(shown)
                } else {
                    Err(format!("got {shown}"))
                }
            }
            ExpectKind::Target => {
                let expected = self.resolve_unit(expectation.entity.as_ref())?;
                let actual = view.viewer.target;
                let shown = actual.map_or_else(|| "none".into(), |unit| self.unit_name(unit));
                if actual == expected {
                    Ok(format!("target={shown}"))
                } else {
                    Err(format!("got target={shown}"))
                }
            }
            ExpectKind::Event => {
                let spec = expectation
                    .event
                    .ok_or("event expectation needs an event")?;
                let unit = self.resolve_unit(expectation.entity.as_ref())?;
                let viewer = EntityRef::Player(view.viewer_id);
                let shown = format!("events[{}]", self.event_tokens(view).join(","));
                if view
                    .events
                    .iter()
                    .any(|event| spec.matches(event, viewer, unit))
                {
                    Ok(shown)
                } else {
                    Err(format!("got {shown}"))
                }
            }
            ExpectKind::Unit => {
                let state = expectation.state.ok_or("unit expectation needs a state")?;
                let unit = self
                    .resolve_unit(expectation.entity.as_ref())?
                    .ok_or("unit expectation needs a unit")?;
                let record = view.entities.iter().find(|entity| entity.entity() == unit);
                let shown = record.map_or_else(|| "absent".into(), record_summary);
                if state.holds(record) {
                    Ok(shown)
                } else {
                    Err(format!("got {shown}"))
                }
            }
        }
    }

    /// The unit a scenario names; a bot must have joined.
    fn resolve_unit(&self, unit: Option<&UnitSpec>) -> Result<Option<EntityRef>, String> {
        unit.unwrap_or(&UnitSpec::None)
            .resolve(|name| self.player_of(name))
            .map_err(|name| format!("{name} has not joined"))
    }
}

/// Level, health percent, position and set flags of a visible unit.
fn record_summary(record: &EntitySnapshot) -> String {
    let flags = record.flags;
    let names = [
        (flags.dead, "dead"),
        (flags.in_combat, "in_combat"),
        (flags.hostile, "hostile"),
        (flags.attackable, "attackable"),
        (flags.tapped_by_other, "tapped_by_other"),
        (flags.evading, "evading"),
        (flags.targets_viewer, "targets_viewer"),
    ]
    .into_iter()
    .filter_map(|(set, name)| set.then_some(name))
    .collect::<Vec<_>>();
    let [x, y, z] = record.position;
    format!(
        "L{} hp{}% ({x},{y},{z}) [{}]",
        record.level,
        record.health_percent,
        names.join(",")
    )
}

/// The player behind a visible entity. Bots name, count and position players
/// only; creatures and NPCs have their own expectations.
fn player_of_entity(entity: &EntitySnapshot) -> Option<PlayerId> {
    match entity.kind {
        EntityKind::Player => Some(entity.id),
        EntityKind::Creature | EntityKind::Npc => None,
    }
}

/// The wire value of a class name; unknown names map to an invalid value
/// so the zone, not the runner, refuses them.
fn chat_channel(name: &str) -> Option<mmorpg_core::ChatChannel> {
    match name {
        "say" => Some(mmorpg_core::ChatChannel::Say),
        "yell" => Some(mmorpg_core::ChatChannel::Yell),
        _ => None,
    }
}

fn emote_named(name: &str) -> Option<mmorpg_core::Emote> {
    mmorpg_core::Emote::ALL
        .into_iter()
        .find(|&emote| emote_name(emote) == name)
}

fn emote_name(emote: mmorpg_core::Emote) -> &'static str {
    match emote {
        mmorpg_core::Emote::Wave => "wave",
        mmorpg_core::Emote::Bow => "bow",
        mmorpg_core::Emote::Cheer => "cheer",
        mmorpg_core::Emote::Laugh => "laugh",
        mmorpg_core::Emote::Point => "point",
    }
}

fn class_code(name: &str) -> u8 {
    match name {
        "warden" => 0,
        "ranger" => 1,
        "arcanist" => 2,
        _ => u8::MAX,
    }
}

fn sex_code(name: &str) -> u8 {
    match name {
        "female" => 0,
        "male" => 1,
        _ => u8::MAX,
    }
}

fn visible_player(view: &ZoneSnapshot, player_id: PlayerId) -> Option<&EntitySnapshot> {
    view.entities
        .iter()
        .find(|entity| player_of_entity(entity) == Some(player_id))
}

fn describe(expectation: &Expectation) -> String {
    let bot = &expectation.bot;
    let target = expectation.target.as_deref().unwrap_or(bot);
    let by = match (expectation.from_tick, expectation.by_tick) {
        (Some(from), Some(by)) => format!(" within t={from}..={by}"),
        (None, Some(by)) => format!(" by t={by}"),
        _ => String::new(),
    };
    match expectation.kind {
        ExpectKind::Sees => format!("{bot} sees {target}{by}"),
        ExpectKind::NotSees => format!("{bot} not_sees {target}"),
        ExpectKind::Position => format!("{bot} position {target}"),
        ExpectKind::Acknowledged => format!("{bot} acknowledged"),
        ExpectKind::Identity => format!("{bot} identity"),
        ExpectKind::VisibleCount => format!("{bot} visible_count"),
        ExpectKind::Area => format!("{bot} area {target}"),
        ExpectKind::Health => format!("{bot} health"),
        ExpectKind::Progression => format!("{bot} progression"),
        ExpectKind::Inventory => format!("{bot} inventory"),
        ExpectKind::Equipment => format!("{bot} equipment"),
        ExpectKind::Copper => format!("{bot} copper"),
        ExpectKind::Chat => format!("{bot} chat"),
        ExpectKind::Loot => format!("{bot} loot"),
        ExpectKind::Target => format!("{bot} target {}", unit_text(expectation)),
        ExpectKind::Event => {
            let event = expectation
                .event
                .map_or_else(String::new, |event| event.to_string());
            let about = expectation
                .entity
                .as_ref()
                .map_or_else(String::new, |entity| format!(" {entity}"));
            format!("{bot} event {event}{about}{by}")
        }
        ExpectKind::Unit => {
            let state = expectation.state.map_or("", UnitState::name);
            format!("{bot} unit {} {state}{by}", unit_text(expectation))
        }
        ExpectKind::Resource => format!("{bot} resource{by}"),
        ExpectKind::Quest => format!("{bot} quest {}", expectation.quest.unwrap_or_default()),
        ExpectKind::Marker => format!("{bot} marker npc:{}", expectation.npc.unwrap_or_default()),
    }
}

fn unit_text(expectation: &Expectation) -> String {
    expectation
        .entity
        .as_ref()
        .unwrap_or(&UnitSpec::None)
        .to_string()
}

fn rejected(error: &RuntimeError) -> String {
    let kind = match error {
        RuntimeError::Session(session) => snake_kind(&format!("{session:?}")),
        RuntimeError::Simulation(_) => "simulation".into(),
        other => snake_kind(&format!("{other:?}")),
    };
    format!("rejected:{kind}")
}
