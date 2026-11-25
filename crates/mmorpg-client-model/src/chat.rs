//! Chat input parsing and the received chat and combat logs. The zone decides
//! who hears a line; the logs only read what it delivered.
use mmorpg_core::{
    ChatChannel, ChatMessage, ChatText, Emote, EntityKind, EntityRef, ErrorCode, PostureView,
    QuestId, RaidNotice, ZoneCommand, ZoneEvent, ZoneId, ZoneSnapshot, ability::ids, ability_by_id,
};
use std::collections::{BTreeMap, VecDeque};
use strum::VariantArray;

/// Each log keeps the newest lines only.
pub const MAX_LOG_LINES: usize = 50;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LineKind {
    Say,
    Yell,
    Emote,
    System,
    Combat,
    Quest,
    Refusal,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogLine {
    pub kind: LineKind,
    pub text: String,
}

/// The slash command and the verb of an emote: `wave`, `bow`, …
#[must_use]
pub const fn emote_name(emote: Emote) -> &'static str {
    match emote {
        Emote::Wave => "wave",
        Emote::Bow => "bow",
        Emote::Cheer => "cheer",
        Emote::Laugh => "laugh",
        Emote::Point => "point",
    }
}

/// Reads a typed line: `/y` or `/yell` yells, `/s`, `/say` or no prefix says,
/// and `/wave`, `/bow`, `/cheer`, `/laugh` or `/point` emotes. Returns the
/// command or why the line cannot be sent.
pub fn parse_chat_input(raw: &str) -> Result<ZoneCommand, String> {
    let trimmed = raw.trim();
    if let Some(word) = trimmed.strip_prefix('/') {
        let (verb, rest) = word
            .split_once(char::is_whitespace)
            .map_or((word, ""), |(verb, rest)| (verb, rest.trim_start()));
        let verb = verb.to_ascii_lowercase();
        if rest.is_empty()
            && let Some(&emote) = Emote::VARIANTS.iter().find(|e| emote_name(**e) == verb)
        {
            return Ok(ZoneCommand::Emote(emote));
        }
        // Sits on the ground where the player stands (#619).
        if rest.is_empty() && verb == "sit" {
            return Ok(ZoneCommand::Sit { seat: None });
        }
        let channel = match verb.as_str() {
            "s" | "say" => ChatChannel::Say,
            "y" | "yell" => ChatChannel::Yell,
            _ => {
                let emotes: Vec<_> = Emote::VARIANTS.iter().map(|e| emote_name(*e)).collect();
                return Err(format!(
                    "Use /s to say, /y to yell, /sit to sit, or /{}.",
                    emotes.join(", /")
                ));
            }
        };
        return text(rest).map(|text| ZoneCommand::Chat { channel, text });
    }
    text(raw).map(|text| ZoneCommand::Chat {
        channel: ChatChannel::Say,
        text,
    })
}

/// The core chat text rule as a sentence: "Chat text must not be blank."
fn text(raw: &str) -> Result<ChatText, String> {
    ChatText::new(raw).map_err(|error| {
        let message = error.message();
        let mut chars = message.chars();
        chars.next().map_or_else(String::new, |first| {
            format!("{}{}.", first.to_uppercase(), chars.as_str())
        })
    })
}

/// `NotEnoughMoney` reads "Not enough money".
#[must_use]
pub fn humanize(debug: &str) -> String {
    let mut out = String::new();
    for (index, character) in debug.chars().enumerate() {
        if character.is_uppercase() && index > 0 {
            out.push(' ');
            out.extend(character.to_lowercase());
        } else {
            out.push(character);
        }
    }
    out
}

/// What a night raid acknowledgment says (#532, Atlas 5.1.60): the lookout guard's own short line
/// for a report and for a prepared raid's end, which never claims more than the outcome.
#[must_use]
pub const fn raid_notice_text(notice: RaidNotice) -> &'static str {
    match notice {
        RaidNotice::Tracks => {
            "Fresh boot prints and a dropped Redbrand rag: someone has been watching Millbrook Farm. The east gate guard should hear of this."
        }
        RaidNotice::Warned => {
            "Greyhaven Guard: \"Redbrand scouts on the Rim Trail? I'll post the watch. Brace the barn's walls if you can; it may come to nothing.\""
        }
        RaidNotice::Braced => "You wedge a timber against the barn wall.",
        RaidNotice::Held => {
            "Greyhaven Guard: \"The barn stands. Being ready for them bought us the time.\""
        }
        RaidNotice::Fell => {
            "Greyhaven Guard: \"We were ready and it slowed them, but they broke into the barn all the same.\""
        }
        // A defence's end (#1019): what Osric says of his people.
        RaidNotice::Stood => {
            "Farmer Osric Mill: \"They're falling back, and my people are standing. We held!\""
        }
        RaidNotice::Overrun => {
            "Farmer Osric Mill: \"Too many of my people are down. Once they're tended and the barn is whole, we try again.\""
        }
    }
}

/// What a refusal tells the player.
#[must_use]
pub fn refusal_text(code: ErrorCode) -> String {
    match code {
        ErrorCode::DoorBlocked => "Something is in the way of the door.".into(),
        ErrorCode::DoorBusy => "The door is still swinging.".into(),
        ErrorCode::NoMinions => "You have no minions or companion to command.".into(),
        ErrorCode::HandoffCorridor => "Not here: you are crossing into another region.".into(),
        ErrorCode::InCombat => "You can't travel while in combat.".into(),
        ErrorCode::WeaponDrawn => "Sheathe your weapon to interact.".into(),
        ErrorCode::MissingMaterials => "You lack the materials for that.".into(),
        ErrorCode::InvalidWaystone => "The waystone cannot take you there.".into(),
        ErrorCode::NotWhileSwimming => "You can't do that while swimming.".into(),
        ErrorCode::VendorClosed => "The shop is closed for the night.".into(),
        ErrorCode::PrivateHome => "This is a private home.".into(),
        ErrorCode::NoAnswer => "You knock, but no one answers at this hour.".into(),
        ErrorCode::SeatTaken => "Someone is already sitting there.".into(),
        ErrorCode::CannotSitHere => "You can't sit down here.".into(),
        ErrorCode::NotAtNight => "No new quests at this hour. Come back in the morning.".into(),
        ErrorCode::DepthsLocked => {
            "The way down is barred. Someone in your party must first have ended Garrick Redbrand.".into()
        }
        ErrorCode::DepthsClosed => "No way down for you here: your party's run has begun without you, or the depths are full.".into(),
        ErrorCode::DepthsWipe => "Your party has fallen. You rise again at the checkpoint.".into(),
        ErrorCode::DepthsBossOpen => "The way to Captain Ulric Thorn lies open.".into(),
        ErrorCode::NoRoom => "There is no room for it to rise there.".into(),
        ErrorCode::NoLanding => "There is no clear place to land there.".into(),
        ErrorCode::MovementImmune => "It cannot be moved right now.".into(),
        ErrorCode::WrongForm => "You cannot do that in this form.".into(),
        ErrorCode::RewardsBusy => "The realm cannot record that reward right now. Try again in a moment.".into(),
        // NPC quest actions (#1019).
        ErrorCode::Distrusted => "They will not hear you. Bring a party member they trust to stand beside you, or earn a recommendation first.".into(),
        ErrorCode::NpcBusy => "Someone else is already escorting them.".into(),
        ErrorCode::EscortLost => "The escort is lost: you went too far ahead or fell. Find them at their post and try again.".into(),
        ErrorCode::NotSellable => "Vendors will not buy quest supplies or lent arms. Finish or abandon the quest to hand them back.".into(),
        // Carried loads (#1077).
        ErrorCode::HandsFull => "Your arms are full. Set the load down first (Interact).".into(),
        ErrorCode::NotCarrying => "You are not carrying the load they need. Fetch it and bring it in your arms.".into(),
        ErrorCode::NoFooting => "Stand on firm ground, out of the water, to lift or set down a load.".into(),
        ErrorCode::LoadLost => "The water took your load. Fetch another where you found it.".into(),
        // Millbrook's defence (#1019).
        ErrorCode::SiteNotReady => "Not yet: a fight is still on at the farm, or its barn and people are still being seen to.".into(),
        // The Sunken Shrine (#1019).
        ErrorCode::SealOpen => "The seal already stands open; its glyphs rest until it closes again.".into(),
        ErrorCode::Warded => "The ward turns your blow aside. Work the vault glyph to bare its weak point.".into(),
        // Lise's flock (#1019).
        ErrorCode::FlockUnsettled => "Not yet: a watch is still on at the fold, or Lise's ewes are still getting back on their feet.".into(),
        ErrorCode::FlockFell => "Shepherd Lise: \"The wolves got to too many of my ewes. Once they are up again, we watch again.\"".into(),
        ErrorCode::StrayBolted => "Left alone too long, the ewe bolted back to where you found her.".into(),
        ErrorCode::OutOfOrder => "The stones fall silent: that was not the inscription's order. Begin again from its first stone.".into(),
        ErrorCode::StaffShown => "They will not speak with a staff held in plain sight. Put it away, or bring a party member they trust or a good word they accept.".into(),
        ErrorCode::PatrolResting => "\"The last patrol has only just passed. Give them time to form up again.\"".into(),
        ErrorCode::PatrolArrived => "\"They made the camp in the hollow. I will watch for the next patrol; ask me when you are ready.\"".into(),
        _ => humanize(&format!("{code:?}")),
    }
}

/// One log: the newest [`MAX_LOG_LINES`] lines and how many were ever added,
/// so a renderer can tell whether anything changed.
#[derive(Clone, Debug, Default)]
pub struct Log {
    lines: VecDeque<LogLine>,
    added: u64,
}

impl Log {
    pub fn lines(&self) -> impl Iterator<Item = &LogLine> {
        self.lines.iter()
    }

    /// Lines added since the log began, including those already dropped.
    #[must_use]
    pub const fn added(&self) -> u64 {
        self.added
    }

    fn push(&mut self, kind: LineKind, text: String) {
        self.lines.push_back(LogLine { kind, text });
        self.added += 1;
        while self.lines.len() > MAX_LOG_LINES {
            self.lines.pop_front();
        }
    }
}

/// Received chat and the viewer's combat and quest feedback, each projection
/// taken once. A projection from another stream (zone, content revision or
/// viewer) starts its tick count afresh.
#[derive(Clone, Debug, Default)]
pub struct Logs {
    pub chat: Log,
    pub combat: Log,
    stream: Option<(ZoneId, u64, u32)>,
    tick: Option<u64>,
    /// The merchants on a cart journey (#992) in the last projection, by NPC ID.
    merchants: BTreeMap<u32, MerchantState>,
}

impl Logs {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// A local chat note, such as why a typed line was not sent.
    pub fn note(&mut self, kind: LineKind, text: impl Into<String>) {
        self.chat.push(kind, text.into());
    }

    /// Whether `snapshot` is newer than every projection taken so far.
    fn take(&mut self, snapshot: &ZoneSnapshot) -> bool {
        let stream = (
            snapshot.zone_id,
            snapshot.content_revision,
            snapshot.viewer_id,
        );
        if self.stream != Some(stream) {
            self.stream = Some(stream);
            self.tick = None;
        }
        if self.tick.is_some_and(|tick| snapshot.tick <= tick) {
            return false;
        }
        self.tick = Some(snapshot.tick);
        true
    }

    /// Reads a new projection's chat lines into the chat log only; returns
    /// whether lines were added.
    pub fn receive_chat(&mut self, snapshot: &ZoneSnapshot) -> bool {
        let before = self.chat.added;
        if self.take(snapshot) {
            self.chat_lines(snapshot);
        }
        self.chat.added != before
    }

    /// Reads a new projection's chat lines and events; `name` names units and
    /// `quest_name` quests. Returns the refusal messages it found.
    pub fn receive(
        &mut self,
        snapshot: &ZoneSnapshot,
        name: impl Fn(EntityRef) -> String,
        quest_name: impl Fn(QuestId) -> String,
    ) -> Vec<String> {
        if !self.take(snapshot) {
            return Vec::new();
        }
        self.chat_lines(snapshot);
        let me = EntityRef::Player(snapshot.viewer_id);
        let who = |entity: EntityRef| {
            if entity == me {
                "You".to_owned()
            } else {
                name(entity)
            }
        };
        let ability = |id| ability_by_id(id).map_or("an ability", |a| a.name);
        let mut refusals = Vec::new();
        for event in &snapshot.events {
            let (kind, text) = match *event {
                // A companion's or minion's blows reach its summoner too (#934).
                ZoneEvent::DamageDealt {
                    source,
                    target,
                    amount,
                    critical,
                } => (
                    LineKind::Combat,
                    if source == me {
                        format!(
                            "You {} {} for {amount}.",
                            if critical { "critically hit" } else { "hit" },
                            who(target)
                        )
                    } else {
                        format!(
                            "{} {} {} for {amount}.",
                            who(source),
                            if critical { "critically hits" } else { "hits" },
                            who(target)
                        )
                    },
                ),
                ZoneEvent::DamageTaken {
                    source,
                    target,
                    amount,
                    critical,
                } => (
                    LineKind::Combat,
                    format!(
                        "{} {} {} for {amount}.",
                        who(source),
                        if critical { "critically hits" } else { "hits" },
                        if target == me {
                            "you".into()
                        } else {
                            who(target)
                        }
                    ),
                ),
                ZoneEvent::Miss { source, target } => (
                    LineKind::Combat,
                    format!("{} missed {}.", who(source), who(target)),
                ),
                ZoneEvent::Died { entity, .. } => (
                    LineKind::Combat,
                    if entity == me {
                        "You died.".into()
                    } else {
                        format!("{} dies.", who(entity))
                    },
                ),
                ZoneEvent::TrapHit { target, amount, .. } => (
                    LineKind::Combat,
                    format!("A trap hits {} for {amount}.", who(target)),
                ),
                ZoneEvent::Evade { target, .. } => {
                    (LineKind::Combat, format!("{} evades.", who(target)))
                }
                // Two attacks met (#679).
                ZoneEvent::Clashed {
                    source,
                    target,
                    kind,
                } => (
                    LineKind::Combat,
                    match kind {
                        mmorpg_core::ClashKind::Swings => {
                            format!("{} and {} clash blades.", who(source), who(target))
                        }
                        mmorpg_core::ClashKind::Projectiles => {
                            format!("{}'s and {}'s shots collide.", who(source), who(target))
                        }
                        mmorpg_core::ClashKind::Parry => {
                            format!("{} cuts down {}'s shot.", who(source), who(target))
                        }
                    },
                ),
                // A flanking Sinister Strike (#949).
                ZoneEvent::Flanked { target, .. } => (
                    LineKind::Combat,
                    format!("Flank! You strike {} off guard.", who(target)),
                ),
                // A strike's timing reward (#928).
                ZoneEvent::StrikeRewarded { reward, .. } => (
                    LineKind::Combat,
                    match reward {
                        mmorpg_core::StrikeReward::LandingStrike => {
                            "Landing strike! You recover quickly.".into()
                        }
                        mmorpg_core::StrikeReward::FollowUp => "Follow up!".into(),
                    },
                ),
                // A trailing lurker gave up on the diver (#274).
                ZoneEvent::LostInterest { .. } => {
                    (LineKind::Combat, "The lurker loses interest.".into())
                }
                ZoneEvent::Healed {
                    target,
                    ability: id,
                    amount,
                    ..
                } => (
                    LineKind::Combat,
                    format!("{} heals {} for {amount}.", ability(id), who(target)),
                ),
                ZoneEvent::AuraApplied {
                    target,
                    ability: id,
                    ..
                } if target == me => (LineKind::Combat, format!("You gain {}.", ability(id))),
                // The caster's root takes hold, and breaks or runs out (#934).
                ZoneEvent::AuraApplied {
                    target,
                    ability: id,
                    ..
                } if id == ids::ENTANGLING_ROOTS => (
                    LineKind::Combat,
                    format!("{} is rooted by {}.", who(target), ability(id)),
                ),
                ZoneEvent::AuraRemoved {
                    target,
                    ability: id,
                    ..
                } if id == ids::ENTANGLING_ROOTS && target != me => (
                    LineKind::Combat,
                    format!("{} breaks free of {}.", who(target), ability(id)),
                ),
                // A signaler's horn call (#952): the warning, and whether help came.
                ZoneEvent::CastStarted {
                    source,
                    ability: id,
                    ..
                } if id == ids::SIGNAL_HORN => (
                    LineKind::Combat,
                    format!("{} raises a signal horn to call for help!", who(source)),
                ),
                ZoneEvent::AbilityUsed {
                    source,
                    ability: id,
                    target,
                } if id == ids::SIGNAL_HORN => (
                    LineKind::Combat,
                    match target {
                        Some(helper) => format!(
                            "{} sounds the horn: {} answers the call!",
                            who(source),
                            who(helper)
                        ),
                        None => format!("{} sounds the horn, but no one answers.", who(source)),
                    },
                ),
                ZoneEvent::Interrupted {
                    target,
                    ability: id,
                    ..
                } => (
                    LineKind::Combat,
                    format!("{}'s {} is interrupted.", who(target), ability(id)),
                ),
                ZoneEvent::Absorbed { target, amount, .. } => (
                    LineKind::Combat,
                    format!("{amount} absorbed on {}.", who(target)),
                ),
                ZoneEvent::QuestProgress {
                    quest,
                    objective,
                    count,
                } => (
                    LineKind::Quest,
                    format!(
                        "{}: objective {} now {count}.",
                        quest_name(quest),
                        objective + 1
                    ),
                ),
                ZoneEvent::QuestCompleted { quest } => {
                    (LineKind::Quest, format!("{} completed.", quest_name(quest)))
                }
                ZoneEvent::Raid { notice } => (LineKind::System, raid_notice_text(notice).into()),
                ZoneEvent::Error { code, target } => {
                    // An NPC quest refusal (#1019) names the NPC.
                    let text = match (code, target) {
                        (
                            ErrorCode::Distrusted
                            | ErrorCode::StaffShown
                            | ErrorCode::NpcBusy
                            | ErrorCode::EscortLost
                            | ErrorCode::NotCarrying
                            | ErrorCode::PatrolResting
                            | ErrorCode::PatrolArrived,
                            Some(npc @ EntityRef::Npc(_)),
                        ) => format!("{}: {}", name(npc), refusal_text(code)),
                        _ => refusal_text(code),
                    };
                    refusals.push(text.clone());
                    (LineKind::Refusal, text)
                }
                _ => continue,
            };
            self.combat.push(kind, text);
        }
        refusals
    }

    /// What the zone delivered as readable lines, and the throttle notice.
    fn chat_lines(&mut self, snapshot: &ZoneSnapshot) {
        let viewer = snapshot.viewer_id;
        for line in &snapshot.chat {
            let you = line.speaker == viewer;
            let speaker = if you {
                "You".to_owned()
            } else {
                format!("Player {}", line.speaker)
            };
            let verb = |own: &str, other: &str| if you { own } else { other }.to_owned();
            let (kind, text) = match line.message {
                ChatMessage::Emote(emote) => (
                    LineKind::Emote,
                    format!("{speaker} {}{}.", emote_name(emote), verb("", "s")),
                ),
                ChatMessage::Say(text) => (
                    LineKind::Say,
                    format!("{speaker} {}: {}", verb("say", "says"), text.as_str()),
                ),
                ChatMessage::Yell(text) => (
                    LineKind::Yell,
                    format!("{speaker} {}: {}", verb("yell", "yells"), text.as_str()),
                ),
            };
            self.chat.push(kind, text);
        }
        for event in &snapshot.events {
            if let ZoneEvent::Error {
                code: ErrorCode::ChatThrottled,
                ..
            } = event
            {
                self.chat.push(
                    LineKind::System,
                    "You can speak or emote once per second.".into(),
                );
            }
        }
        self.merchant_cries(snapshot);
    }

    /// A merchant on its cart journey (#992) cries out as the viewer sees it held up, robbed or
    /// saved: each line follows the projected posture and health alone, once per change.
    fn merchant_cries(&mut self, snapshot: &ZoneSnapshot) {
        let mut seen = BTreeMap::new();
        for entity in &snapshot.entities {
            if entity.kind != EntityKind::Npc {
                continue;
            }
            let state = match entity.flags.posture {
                PostureView::Hauling if entity.health_percent < 100 => MerchantState::Held,
                PostureView::Hauling => MerchantState::Travelling,
                PostureView::Hurt => MerchantState::Hurt,
                _ => continue,
            };
            let before = self.merchants.get(&entity.id).copied();
            let line = match (before, state) {
                (Some(MerchantState::Held), MerchantState::Held) => None,
                (_, MerchantState::Held) => Some((
                    LineKind::Yell,
                    "A merchant yells: Redbrands! Help, someone, they're after my cart!",
                )),
                (Some(MerchantState::Held), MerchantState::Hurt) => Some((
                    LineKind::Yell,
                    "A merchant cries: They've taken the cart, everything!",
                )),
                (Some(MerchantState::Held), MerchantState::Travelling) => Some((
                    LineKind::Say,
                    "A merchant says: Bless you, friend! On to Millbrook, then.",
                )),
                _ => None,
            };
            if let Some((kind, text)) = line {
                self.chat.push(kind, text.to_owned());
            }
            seen.insert(entity.id, state);
        }
        self.merchants = seen;
    }
}

/// What a viewer last saw of a merchant on its cart journey (#992).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MerchantState {
    Travelling,
    Held,
    Hurt,
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmorpg_core::ChatLine;

    fn say(text: &str) -> ZoneCommand {
        ZoneCommand::Chat {
            channel: ChatChannel::Say,
            text: ChatText::new(text).unwrap(),
        }
    }

    #[test]
    fn typed_lines_become_the_shared_chat_commands() {
        assert_eq!(parse_chat_input("hello"), Ok(say("hello")));
        assert_eq!(parse_chat_input("/s hello"), Ok(say("hello")));
        assert_eq!(parse_chat_input("/say  hello"), Ok(say("hello")));
        assert_eq!(
            parse_chat_input("/YELL over here"),
            Ok(ZoneCommand::Chat {
                channel: ChatChannel::Yell,
                text: ChatText::new("over here").unwrap()
            })
        );
        assert_eq!(
            parse_chat_input("/wave"),
            Ok(ZoneCommand::Emote(Emote::Wave))
        );
        assert_eq!(
            parse_chat_input("/Bow "),
            Ok(ZoneCommand::Emote(Emote::Bow))
        );
        assert_eq!(
            parse_chat_input("/dance"),
            Err(
                "Use /s to say, /y to yell, /sit to sit, or /wave, /bow, /cheer, /laugh, /point."
                    .into()
            )
        );
        assert_eq!(
            parse_chat_input("/s"),
            Err("Chat text must be 1 to 80 bytes.".into())
        );
        assert_eq!(
            parse_chat_input(&"a".repeat(81)),
            Err("Chat text must be 1 to 80 bytes.".into())
        );
        assert_eq!(
            parse_chat_input("tab\there"),
            Err("Chat text must not contain control characters.".into())
        );
        // U+FEFF is not whitespace, so the line is valid.
        assert_eq!(parse_chat_input("\u{feff}"), Ok(say("\u{feff}")));
        assert_eq!(humanize("NotEnoughMoney"), "Not enough money");
    }

    #[test]
    fn received_lines_read_with_speaker_names_and_each_projection_counts_once() {
        let line = |speaker, message| ChatLine { speaker, message };
        let text = |text| ChatText::new(text).unwrap();
        let mut snapshot = crate::test_support::snapshot(7, 5);
        snapshot.chat = vec![
            line(7, ChatMessage::Say(text("hello"))),
            line(9, ChatMessage::Yell(text("over here"))),
            line(9, ChatMessage::Emote(Emote::Wave)),
            line(7, ChatMessage::Emote(Emote::Bow)),
        ];
        snapshot.events = vec![ZoneEvent::Error {
            code: ErrorCode::ChatThrottled,
            target: None,
        }];
        let mut logs = Logs::default();
        assert!(logs.receive_chat(&snapshot));
        let lines: Vec<_> = logs.chat.lines().map(|line| line.text.as_str()).collect();
        assert_eq!(
            lines,
            [
                "You say: hello",
                "Player 9 yells: over here",
                "Player 9 waves.",
                "You bow.",
                "You can speak or emote once per second.",
            ]
        );
        assert!(!logs.receive_chat(&snapshot), "a projection counts once");
        // Another zone counts its own ticks.
        snapshot.zone_id = ZoneId::new(2);
        snapshot.tick = 1;
        snapshot.events.clear();
        assert!(logs.receive_chat(&snapshot));
        assert_eq!(logs.chat.added(), 9);
    }

    #[test]
    fn a_summoner_reads_its_companions_fight_and_its_root() {
        let (me, wolf, foe) = (
            EntityRef::Player(7),
            EntityRef::Minion(mmorpg_core::MinionId::new(1)),
            EntityRef::Creature(mmorpg_core::CreatureId::new(220)),
        );
        let mut snapshot = crate::test_support::snapshot(7, 5);
        snapshot.events = vec![
            ZoneEvent::DamageDealt {
                source: wolf,
                target: foe,
                amount: 9,
                critical: false,
            },
            ZoneEvent::DamageTaken {
                source: foe,
                target: wolf,
                amount: 4,
                critical: true,
            },
            ZoneEvent::AuraApplied {
                source: me,
                target: foe,
                ability: ids::ENTANGLING_ROOTS,
                ticks: 360,
            },
            ZoneEvent::AuraRemoved {
                source: me,
                target: foe,
                ability: ids::ENTANGLING_ROOTS,
            },
        ];
        let mut logs = Logs::default();
        let name = |entity| match entity {
            EntityRef::Minion(_) => "Wolf Companion".to_owned(),
            _ => "Field Marauder".to_owned(),
        };
        logs.receive(&snapshot, name, |_| String::new());
        let lines: Vec<_> = logs.combat.lines().map(|line| line.text.as_str()).collect();
        assert_eq!(
            lines,
            [
                "Wolf Companion hits Field Marauder for 9.",
                "Field Marauder critically hits Wolf Companion for 4.",
                "Field Marauder is rooted by Entangling Roots.",
                "Field Marauder breaks free of Entangling Roots.",
            ]
        );
    }

    #[test]
    fn a_log_keeps_the_newest_lines() {
        let mut logs = Logs::default();
        for index in 0..MAX_LOG_LINES + 5 {
            logs.note(LineKind::System, format!("{index}"));
        }
        assert_eq!(logs.chat.lines().count(), MAX_LOG_LINES);
        assert_eq!(logs.chat.lines().next().unwrap().text, "5");
        assert_eq!(logs.chat.added(), (MAX_LOG_LINES + 5) as u64);
    }
}

#[cfg(test)]
mod merchant_tests {
    use super::*;
    use mmorpg_core::{EntityFlags, EntitySnapshot};

    fn merchant(posture: PostureView, health_percent: u8) -> EntitySnapshot {
        EntitySnapshot {
            kind: EntityKind::Npc,
            id: 17,
            appearance: 17,
            position: [0; 3],
            velocity: [0; 3],
            facing: 0,
            level: 3,
            health_percent,
            flags: EntityFlags {
                posture,
                ..EntityFlags::default()
            },
        }
    }

    #[test]
    fn a_held_up_merchant_cries_once_and_then_by_its_outcome() {
        let mut logs = Logs::default();
        let mut tick = 0;
        let mut see = |logs: &mut Logs, entities: Vec<EntitySnapshot>| {
            tick += 1;
            let mut snapshot = crate::test_support::snapshot(1, tick);
            snapshot.entities = entities;
            logs.receive_chat(&snapshot);
            logs.chat.lines().map(|line| line.kind).collect::<Vec<_>>()
        };
        assert!(see(&mut logs, vec![merchant(PostureView::Hauling, 100)]).is_empty());
        let held = vec![merchant(PostureView::Hauling, 60)];
        assert_eq!(see(&mut logs, held.clone()), [LineKind::Yell]);
        assert_eq!(see(&mut logs, held.clone()), [LineKind::Yell]);
        let saved = see(&mut logs, vec![merchant(PostureView::Hauling, 100)]);
        assert_eq!(saved, [LineKind::Yell, LineKind::Say]);
        see(&mut logs, held);
        let robbed = see(&mut logs, vec![merchant(PostureView::Hurt, 35)]);
        assert_eq!(robbed.len(), 4);
        // A merchant already hurt when it comes into view says nothing.
        let mut fresh = Logs::default();
        assert!(see(&mut fresh, vec![merchant(PostureView::Hurt, 35)]).is_empty());
    }
}
