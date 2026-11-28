//! The words both clients show for the zone's answers (#1278): what a refusal says, generally and
//! in the vendor, loot, bag, equipment and quest panels; one line per event from the viewer's
//! point of view; unit names; and the HUD's status, stance, combo, mastery and breath lines.
//! Everything reads a received projection and immutable content; nothing decides gameplay.
use mmorpg_core::{
    AbilityId, ClashKind, ComboPhase, CreatureTemplateId, EntityRef, EntitySnapshot, ErrorCode,
    Mastery, MeleeStance, PlayerId, RaidNotice, ResourceKind, StrikeHeight, StrikeReward,
    StrikeSide, ZoneContent, ZoneEvent, ZoneSnapshot, ability::ids, ability_by_id,
};

use crate::chat::LineKind;

/// The panel a refusal answers, which words its consequence (#1278).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RefusalContext {
    /// A vendor trade (buy or sell).
    Vendor,
    /// A corpse claim.
    Loot,
    /// A bag move.
    Bag,
    /// Equipping an item.
    Equip,
    /// Unequipping an item.
    Unequip,
    /// A quest accept or turn-in.
    Quest,
}

impl RefusalContext {
    /// What the player asked for, as "That … was refused." names it.
    const fn request(self) -> &'static str {
        match self {
            Self::Vendor => "trade",
            Self::Loot => "claim",
            Self::Bag => "bag move",
            Self::Equip | Self::Unequip => "equipment change",
            Self::Quest => "reward",
        }
    }

    /// What stays as it was.
    const fn unchanged(self) -> &'static str {
        match self {
            Self::Loot => "The corpse keeps its rewards.",
            Self::Unequip => "The item stays equipped.",
            Self::Vendor | Self::Bag | Self::Equip | Self::Quest => "Your items are unchanged.",
        }
    }
}

/// What a refusal tells the player; a resource shortage names the viewer's `resource`.
#[must_use]
pub fn refusal_text(code: ErrorCode, resource: Option<ResourceKind>) -> String {
    let text = match code {
        ErrorCode::NoTarget => "You have no target.",
        ErrorCode::OutOfRange => "Out of range.",
        ErrorCode::TargetDead => "Your target is dead.",
        ErrorCode::NotAttackable => "You cannot attack that.",
        ErrorCode::YouAreDead => "You are dead.",
        ErrorCode::NotDead => "You are not dead.",
        ErrorCode::InvalidTarget => "That is not a valid target for that ability.",
        ErrorCode::TooManyIntents => "Too many actions at once.",
        ErrorCode::InvalidInventoryMove => "Bag move refused.",
        ErrorCode::InventoryFull => "Your bags are full.",
        ErrorCode::InvalidLoot => "That corpse is no longer available.",
        ErrorCode::NotLootOwner => "That corpse belongs to another player.",
        ErrorCode::EmptyLoot => "No rewards remain on that corpse.",
        ErrorCode::MoneyOverflow => "You cannot hold any more copper.",
        ErrorCode::NoClass => "You have no class yet.",
        ErrorCode::NotLearned => "You have not learned that ability yet.",
        ErrorCode::NotReady => "That ability is not ready yet.",
        ErrorCode::NotEnoughResource => {
            return resource.map_or_else(
                || "Not enough resource.".into(),
                |kind| format!("Not enough {}.", kind.name()),
            );
        }
        ErrorCode::Stunned => "You are stunned and cannot act.",
        ErrorCode::AlreadyCasting => "You are already casting. Press Esc to cancel.",
        ErrorCode::LineOfSight => "Target not in line of sight.",
        ErrorCode::InvalidClass => "That class choice is not available.",
        ErrorCode::NotEquippable => "That item cannot be equipped.",
        ErrorCode::InvalidVendor => "That vendor does not trade that.",
        ErrorCode::NotEnoughMoney => "You do not have enough copper.",
        ErrorCode::ChatThrottled => "You can speak or emote once per second.",
        ErrorCode::InvalidQuest => "That quest is not available here.",
        ErrorCode::QuestLogFull => "Your quest log is full. Abandon a quest first.",
        ErrorCode::QuestIncomplete => "That quest is not complete yet.",
        ErrorCode::DoorBlocked => "Something is in the way of the door.",
        ErrorCode::VendorClosed => "The shop is closed for the night.",
        ErrorCode::DoorBusy => "The door is still swinging.",
        ErrorCode::NoMinions => "You have no minions or companion to command.",
        ErrorCode::NotWhileSwimming => "You can't do that while swimming.",
        ErrorCode::HandoffCorridor => "Not here: you are crossing into another region.",
        ErrorCode::InCombat => "You can't travel while in combat.",
        ErrorCode::WeaponDrawn => "Sheathe your weapon to interact (H).",
        ErrorCode::InvalidWaystone => "The waystone cannot take you there.",
        ErrorCode::PrivateHome => "This is a private home.",
        ErrorCode::NoAnswer => "You knock, but no one answers at this hour.",
        ErrorCode::SeatTaken => "Someone is already sitting there.",
        ErrorCode::CannotSitHere => "You can't sit down here.",
        ErrorCode::NotAtNight => "No new quests at this hour. Come back in the morning.",
        ErrorCode::DepthsLocked => {
            "The way down is barred. Someone in your party must first have ended Garrick Redbrand."
        }
        ErrorCode::DepthsClosed => {
            "No way down for you here: your party's run has begun without you, or the depths are full."
        }
        ErrorCode::DepthsWipe => "Your party has fallen. You rise again at the checkpoint.",
        ErrorCode::DepthsBossOpen => "The way to Captain Ulric Thorn lies open.",
        ErrorCode::NoRoom => "There is no room for it to rise there.",
        ErrorCode::NoLanding => "There is no clear place to land there.",
        ErrorCode::MovementImmune => "It cannot be moved right now.",
        ErrorCode::MissingMaterials => "You lack the materials for that.",
        ErrorCode::WrongForm => "You cannot do that in this form.",
        ErrorCode::RewardsBusy => {
            "The realm cannot record that reward right now. Try again in a moment."
        }
        // NPC quest actions (#1019); [`event_line`] names the NPC.
        ErrorCode::Distrusted => {
            "They will not hear you. Bring a party member they trust to stand beside you, or earn a recommendation first."
        }
        ErrorCode::NpcBusy => "Someone else is already escorting them.",
        ErrorCode::EscortLost => {
            "The escort is lost: you went too far ahead or fell. Find them at their post and try again."
        }
        ErrorCode::NotSellable => {
            "Vendors will not buy quest supplies or lent arms. Finish or abandon the quest to hand them back."
        }
        // Carried loads (#1077).
        ErrorCode::HandsFull => "Your arms are full. Set the load down first (Interact).",
        ErrorCode::NotCarrying => {
            "You are not carrying the load they need. Fetch it and bring it in your arms."
        }
        ErrorCode::NoFooting => {
            "Stand on firm ground, out of the water, to lift or set down a load."
        }
        ErrorCode::LoadLost => "The water took your load. Fetch another where you found it.",
        // Millbrook's defence (#1019).
        ErrorCode::SiteNotReady => {
            "Not yet: a fight is still on at the farm, or its barn and people are still being seen to."
        }
        // The Sunken Shrine (#1019).
        ErrorCode::SealOpen => {
            "The seal already stands open; its glyphs rest until it closes again."
        }
        ErrorCode::Warded => {
            "The ward turns your blow aside. Work the vault glyph to bare its weak point."
        }
        // Lise's flock (#1019).
        ErrorCode::FlockUnsettled => {
            "Not yet: a watch is still on at the fold, or Lise's ewes are still getting back on their feet."
        }
        ErrorCode::FlockFell => {
            "Shepherd Lise: \"The wolves got to too many of my ewes. Once they are up again, we watch again.\""
        }
        ErrorCode::StrayBolted => {
            "Left alone too long, the ewe bolted back to where you found her."
        }
        // Barrowdown's ring and Old Maud (#1019).
        ErrorCode::OutOfOrder => {
            "The stones fall silent: that was not the inscription's order. Begin again from its first stone."
        }
        ErrorCode::StaffShown => {
            "They will not speak with a staff held in plain sight. Put it away, or bring a party member they trust or a good word they accept."
        }
        // The Downs road's supply patrol and Fenford's crossings (#1019): the speaking NPC's
        // name comes from the event's target.
        ErrorCode::PatrolResting => {
            "\"The last patrol has only just passed. Give them time to form up again.\""
        }
        ErrorCode::PatrolArrived => {
            "\"They made the camp in the hollow. I will watch for the next patrol; ask me when you are ready.\""
        }
        ErrorCode::CartResting => {
            "\"Not yet. Let me see to the cart before we try the ford again.\""
        }
        ErrorCode::CartLost => {
            "\"They broke the cart in the ford! Give me time to mend it, then we try again.\""
        }
        ErrorCode::TooDeep => {
            "\"Not through there, it's over my head! Take me by the shallows.\" They will not follow into deep water."
        }
    };
    text.into()
}

/// What a refusal says in a panel's feedback line, or `None` when that panel ignores `code`. A
/// vendor trade answers with any refusal; the other panels only with their own.
#[must_use]
pub fn panel_refusal(code: ErrorCode, context: RefusalContext) -> Option<String> {
    use RefusalContext::{Bag, Equip, Loot, Quest, Unequip, Vendor};
    let unchanged = context.unchanged();
    let text = match (code, context) {
        (ErrorCode::TooManyIntents, Quest) => "Too many actions. Try again.".into(),
        (ErrorCode::TooManyIntents, _) => {
            format!("Too many actions. Try the {} again.", context.request())
        }
        (ErrorCode::YouAreDead, Vendor) => "You cannot trade while dead.".into(),
        (ErrorCode::YouAreDead, Loot) => "You cannot loot while dead.".into(),
        (ErrorCode::YouAreDead, Bag) => "You cannot move items while dead.".into(),
        (ErrorCode::YouAreDead, Equip | Unequip) => {
            "You cannot change equipment while dead.".into()
        }
        (ErrorCode::YouAreDead, Quest) => "You cannot do that while dead.".into(),
        (ErrorCode::OutOfRange, Vendor) => "Move closer to the vendor.".into(),
        (ErrorCode::OutOfRange, Loot) => "Move closer to the corpse.".into(),
        (ErrorCode::OutOfRange, Quest) => "Move closer to talk.".into(),
        (ErrorCode::InvalidInventoryMove, _) => {
            format!("That {} was refused. {unchanged}", context.request())
        }
        (ErrorCode::InventoryFull, Bag | Equip) => format!("That stack is full. {unchanged}"),
        (ErrorCode::InventoryFull, Vendor | Loot | Quest | Unequip)
        | (ErrorCode::MoneyOverflow | ErrorCode::HandoffCorridor | ErrorCode::RewardsBusy, Loot)
        | (ErrorCode::MoneyOverflow, Vendor | Quest)
        | (ErrorCode::NotEquippable, Bag | Equip | Unequip) => {
            format!("{} {unchanged}", refusal_text(code, None))
        }
        (ErrorCode::InvalidLoot | ErrorCode::NotLootOwner | ErrorCode::EmptyLoot, Loot)
        | (ErrorCode::InvalidQuest | ErrorCode::QuestLogFull | ErrorCode::QuestIncomplete, Quest)
        | (_, Vendor) => refusal_text(code, None),
        _ => return None,
    };
    Some(text)
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

/// The display name of a minion whose projected appearance is `appearance`: a Necromancer's
/// skeleton (#295) or a Druid's companion (#296).
#[must_use]
pub fn minion_name(appearance: u32) -> &'static str {
    if mmorpg_core::is_companion(mmorpg_core::minion_ability(appearance)) {
        "Wolf Companion"
    } else {
        "Skeleton"
    }
}

/// A unit's display name: "you" for `viewer`, content names for NPCs and creatures (`unit` is its
/// projected record, when in sight), the minion's kind.
#[must_use]
pub fn unit_name(
    entity: EntityRef,
    viewer: PlayerId,
    unit: Option<&EntitySnapshot>,
    content: &ZoneContent,
) -> String {
    match entity {
        EntityRef::Player(id) if id == viewer => "you".into(),
        EntityRef::Player(id) => format!("Player {id}"),
        EntityRef::Minion(_) => unit
            .map_or("Skeleton", |unit| minion_name(unit.appearance))
            .into(),
        EntityRef::Npc(id) => content
            .npc(id)
            .map_or_else(|| format!("NPC {}", id.get()), |npc| npc.name.clone()),
        EntityRef::Creature(_) => unit
            .and_then(|unit| content.creature_template(CreatureTemplateId::new(unit.appearance)))
            .map_or_else(|| "a creature".into(), |template| template.name.clone()),
    }
}

/// `entity`'s name as `snapshot`'s viewer reads it.
#[must_use]
pub fn unit_name_in(entity: EntityRef, snapshot: &ZoneSnapshot, content: &ZoneContent) -> String {
    let unit = snapshot
        .entities
        .iter()
        .find(|unit| unit.entity() == entity);
    unit_name(entity, snapshot.viewer_id, unit, content)
}

fn ability_name(ability: AbilityId) -> &'static str {
    ability_by_id(ability).map_or("an ability", |ability| ability.name)
}

fn capitalised(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

const fn side_name(side: StrikeSide) -> &'static str {
    match side {
        StrikeSide::Left => "left",
        StrikeSide::Centre => "centre",
        StrikeSide::Right => "right",
    }
}

const fn height_name(height: StrikeHeight) -> &'static str {
    match height {
        StrikeHeight::Low => "low",
        StrikeHeight::Middle => "middle",
        StrikeHeight::High => "high",
    }
}

/// The units an event names, read from one projection.
struct Names<'a> {
    snapshot: &'a ZoneSnapshot,
    content: &'a ZoneContent,
}

impl Names<'_> {
    fn viewer(&self, entity: EntityRef) -> bool {
        entity == EntityRef::Player(self.snapshot.viewer_id)
    }

    /// "you", "Timber Wolf", …
    fn name(&self, entity: EntityRef) -> String {
        unit_name_in(entity, self.snapshot, self.content)
    }

    /// The name starting a sentence: "You", "Timber Wolf", …
    fn subject(&self, entity: EntityRef) -> String {
        capitalised(&self.name(entity))
    }

    /// "your", "Timber Wolf's", …
    fn possessive(&self, entity: EntityRef) -> String {
        if self.viewer(entity) {
            "your".into()
        } else {
            format!("{}'s", self.name(entity))
        }
    }

    /// The subject and its verb: "You hit", "Timber Wolf hits".
    fn says(&self, entity: EntityRef, own: &str, other: &str) -> String {
        let verb = if self.viewer(entity) { own } else { other };
        format!("{} {verb}", self.subject(entity))
    }
}

/// One line for `event` from `snapshot`'s viewer's point of view, or `None` for an event shown on
/// the body rather than in words. A quest refusal from an NPC (#1019) names the NPC.
#[must_use]
pub fn event_line(
    event: &ZoneEvent,
    snapshot: &ZoneSnapshot,
    content: &ZoneContent,
) -> Option<(LineKind, String)> {
    let names = Names { snapshot, content };
    let hit = |critical: bool| {
        if critical {
            ("critically hit", "critically hits")
        } else {
            ("hit", "hits")
        }
    };
    let combat = |text: String| Some((LineKind::Combat, text));
    match *event {
        // A basic strike's or a beast's wind-up (#885, #889) shows on the body, not in words.
        ZoneEvent::AttackPerformed { .. }
        | ZoneEvent::StrikeBegan { .. }
        | ZoneEvent::CreatureAttackBegan { .. }
        | ZoneEvent::DamageObserved { .. } => None,
        // A companion's or minion's blows and wounds reach its summoner too (#934).
        ZoneEvent::DamageDealt {
            source,
            target,
            amount,
            critical,
        }
        | ZoneEvent::DamageTaken {
            source,
            target,
            amount,
            critical,
        } => {
            let (own, other) = hit(critical);
            combat(format!(
                "{} {} for {amount}.",
                names.says(source, own, other),
                names.name(target)
            ))
        }
        ZoneEvent::Miss { source, target } => combat(format!(
            "{} {}.",
            names.says(source, "miss", "misses"),
            names.name(target)
        )),
        ZoneEvent::TrapHit { target, amount, .. } => {
            combat(format!("A trap hits {} for {amount}.", names.name(target)))
        }
        ZoneEvent::Evade { target, .. } => {
            combat(format!("{}.", names.says(target, "evade", "evades")))
        }
        ZoneEvent::Died { entity, .. } => combat(if names.viewer(entity) {
            "You died.".into()
        } else {
            format!("{} dies.", names.subject(entity))
        }),
        ZoneEvent::Error { code, target } => {
            let text = refusal_text(code, snapshot.viewer.resource.map(|view| view.kind));
            let text = match (code, target) {
                (
                    ErrorCode::Distrusted
                    | ErrorCode::StaffShown
                    | ErrorCode::NpcBusy
                    | ErrorCode::EscortLost
                    | ErrorCode::NotCarrying
                    | ErrorCode::PatrolResting
                    | ErrorCode::PatrolArrived
                    | ErrorCode::CartResting
                    | ErrorCode::CartLost
                    | ErrorCode::TooDeep,
                    Some(npc @ EntityRef::Npc(_)),
                ) => format!("{}: {text}", names.name(npc)),
                _ => text,
            };
            Some((LineKind::Refusal, text))
        }
        // A signaler's horn call (#952): the warning, and whether help came.
        ZoneEvent::CastStarted {
            source, ability, ..
        } if ability == ids::SIGNAL_HORN => combat(format!(
            "{} raises a signal horn to call for help!",
            names.subject(source)
        )),
        ZoneEvent::CastStarted {
            source, ability, ..
        } => combat(format!(
            "{} {}.",
            names.says(source, "begin", "begins"),
            ability_name(ability)
        )),
        ZoneEvent::AbilityUsed {
            source,
            ability,
            target,
        } if ability == ids::SIGNAL_HORN => combat(match target {
            Some(helper) => format!(
                "{} sounds the horn: {} answers the call!",
                names.subject(source),
                names.name(helper)
            ),
            None => format!(
                "{} sounds the horn, but no one answers.",
                names.subject(source)
            ),
        }),
        ZoneEvent::AbilityUsed {
            source, ability, ..
        } => combat(format!(
            "{} {}.",
            names.says(source, "use", "uses"),
            ability_name(ability)
        )),
        ZoneEvent::StrikeSwept {
            source,
            target,
            ability,
            aim,
        } => {
            let ability = ability_name(ability);
            combat(if !names.viewer(source) {
                format!("{} sweeps {ability}.", names.subject(source))
            } else {
                let aim = format!("{}-{}", height_name(aim.height), side_name(aim.side));
                match target {
                    None => format!("Your {aim} {ability} cuts only air."),
                    Some(target) => {
                        format!("Your {aim} {ability} strikes {}.", names.name(target))
                    }
                }
            })
        }
        // A flanking Sinister Strike (#949), the viewer's own.
        ZoneEvent::Flanked { target, .. } => combat(format!(
            "Flank! You strike {} off guard.",
            names.name(target)
        )),
        // A strike's timing reward (#928), the viewer's own; its cue also shows at the feet.
        ZoneEvent::StrikeRewarded { reward, .. } => combat(
            match reward {
                StrikeReward::LandingStrike => "Landing strike! You recover quickly.",
                StrikeReward::FollowUp => "Follow up!",
            }
            .into(),
        ),
        // A timed swing's wind-up (#679): an opponent's shows, the viewer's own does not.
        ZoneEvent::SwingBegan { source, .. } if names.viewer(source) => None,
        ZoneEvent::SwingBegan {
            source, ability, ..
        } => combat(format!(
            "{} winds up {}!",
            names.subject(source),
            ability_name(ability)
        )),
        // Two attacks met (#679).
        ZoneEvent::Clashed {
            source,
            target,
            kind,
        } => combat(match kind {
            ClashKind::Swings => format!(
                "{} and {} clash blades.",
                names.subject(source),
                names.name(target)
            ),
            ClashKind::Projectiles => "Two shots collide in the air.".into(),
            ClashKind::Parry => format!(
                "{} down {} shot.",
                names.says(source, "cut", "cuts"),
                names.possessive(target)
            ),
        }),
        ZoneEvent::Blocked {
            source,
            target,
            lane,
            ..
        } => combat(format!(
            "{} {} {} blow.",
            names.says(target, "block", "blocks"),
            names.possessive(source),
            height_name(lane)
        )),
        ZoneEvent::GuardBroken {
            source,
            target,
            lane,
            ..
        } => combat(if names.viewer(target) {
            format!(
                "{} your {} guard!",
                names.says(source, "break", "breaks"),
                height_name(lane)
            )
        } else {
            format!(
                "{} {} guard.",
                names.says(source, "break", "breaks"),
                names.possessive(target)
            )
        }),
        ZoneEvent::Healed {
            target,
            ability,
            amount,
            ..
        } => combat(format!(
            "{} heals {} for {amount}.",
            ability_name(ability),
            names.name(target)
        )),
        // The caster's root takes hold, and breaks or runs out (#934).
        ZoneEvent::AuraApplied {
            target, ability, ..
        } if ability == ids::ENTANGLING_ROOTS => combat(format!(
            "{} rooted by {}.",
            names.says(target, "are", "is"),
            ability_name(ability)
        )),
        ZoneEvent::AuraApplied {
            target, ability, ..
        } => combat(format!(
            "{} {}.",
            names.says(target, "gain", "gains"),
            ability_name(ability)
        )),
        ZoneEvent::AuraRemoved {
            target, ability, ..
        } if ability == ids::ENTANGLING_ROOTS => combat(format!(
            "{} free of {}.",
            names.says(target, "break", "breaks"),
            ability_name(ability)
        )),
        ZoneEvent::AuraRemoved {
            target, ability, ..
        } => combat(format!(
            "{} fades from {}.",
            ability_name(ability),
            names.name(target)
        )),
        ZoneEvent::Interrupted {
            target, ability, ..
        } => combat(format!(
            "{} interrupted ({}).",
            names.says(target, "are", "is"),
            ability_name(ability)
        )),
        ZoneEvent::Absorbed { amount, .. } => combat(format!("A shield absorbs {amount} damage.")),
        ZoneEvent::QuestProgress {
            quest,
            objective,
            count,
        } => Some((
            LineKind::Quest,
            quest_progress(content, quest, objective, count),
        )),
        ZoneEvent::QuestCompleted { quest } => Some((
            LineKind::Quest,
            content.quest(quest).map_or_else(
                || "Quest completed.".into(),
                |quest| format!("{} completed.", quest.name),
            ),
        )),
        // A trailing lurker gave up on the diver (#274).
        ZoneEvent::LostInterest { .. } => combat("The lurker loses interest.".into()),
        ZoneEvent::Raid { notice } => Some((LineKind::System, raid_notice_text(notice).into())),
    }
}

/// "Wolves at the Fold: Timber Wolf slain 4/6." or "…: Speak with Osric done."
fn quest_progress(
    content: &ZoneContent,
    quest: mmorpg_core::QuestId,
    objective: u8,
    count: u8,
) -> String {
    let Some(record) = content.quest(quest) else {
        return "Quest progress.".into();
    };
    let index = usize::from(objective);
    let mut progress = [0; 3];
    if let Some(slot) = progress.get_mut(index) {
        *slot = count;
    }
    let view = crate::quest::quest_view(record, &progress, content);
    match (record.objectives.get(index), view.objectives.get(index)) {
        (Some(&kind), Some(view)) if crate::quest::counted(kind) => format!(
            "{}: {} {}/{}.",
            record.name,
            view.label,
            view.current.min(view.required),
            view.required
        ),
        (Some(_), Some(view)) => format!("{}: {} done.", record.name, view.label),
        _ => "Quest progress.".into(),
    }
}

/// The Warden's movement-dependent melee stance and what it does (#155, Atlas 5.2.69–5.2.70).
#[must_use]
pub const fn stance_text(stance: MeleeStance) -> &'static str {
    match stance {
        MeleeStance::Planted => "Planted: braced, −10% damage taken",
        MeleeStance::Charging => "Charging: +20% melee damage",
        MeleeStance::Moving => "Moving: −15% melee damage",
        MeleeStance::Airborne => "Airborne: +35% melee damage, +25% taken",
        MeleeStance::Landing => "Landing: melee waits",
    }
}

/// The pending slash follow-up (#157), and the cleave's extra swing (#910).
#[must_use]
pub const fn combo_text(combo: ComboPhase) -> &'static str {
    match combo {
        ComboPhase::Impact => "Combo: slash landing, Shield Bash follows",
        ComboPhase::Open => "Combo: Shield Bash now",
        ComboPhase::CleaveImpact => "Combo: cleave landing, swing again or Shield Bash follows",
        ComboPhase::CleaveOpen => "Combo: cleave! Swing again or Shield Bash now",
    }
}

/// Sword mastery (#915) once practised: "Sword mastery 1 (12/80)" or "(mastered)".
#[must_use]
pub fn mastery_text(mastery: Mastery) -> Option<String> {
    if mastery == Mastery::default() {
        return None;
    }
    let progress = match mmorpg_core::practice_to_next_level(mastery.level) {
        None | Some(0) => "mastered".to_owned(),
        Some(next) => format!("{}/{next}", mastery.progress),
    };
    Some(format!("Sword mastery {} ({progress})", mastery.level))
}

/// Breath (#274) while it is spent: "Breath 15 s", or "Drowning!" at zero.
#[must_use]
pub fn breath_text(breath: u16) -> Option<String> {
    if breath >= mmorpg_core::water::BREATH_MAX_TICKS {
        None
    } else if breath == 0 {
        Some("Drowning!".into())
    } else {
        Some(format!(
            "Breath {} s",
            breath.div_ceil(mmorpg_core::TICK_HZ)
        ))
    }
}

/// The viewer's status line: health, resource, level, breath, stance, combo, cast, mastery,
/// combat and target, or how to release a dead spirit.
#[must_use]
pub fn status_line(snapshot: &ZoneSnapshot, content: &ZoneContent) -> String {
    let viewer = &snapshot.viewer;
    if viewer.dead {
        return "You are dead. Press R to release your spirit to the graveyard.".into();
    }
    let mut parts = vec![format!("Health {}/{}", viewer.health, viewer.max_health)];
    if let Some(resource) = viewer.resource {
        parts.push(format!(
            "{} {}/{}",
            capitalised(resource.kind.name()),
            resource.value,
            resource.max
        ));
    }
    parts.push(format!("Level {}", viewer.level));
    parts.extend(breath_text(viewer.breath));
    parts.extend(viewer.stance.map(|stance| stance_text(stance).to_owned()));
    parts.extend(viewer.combo.map(|combo| combo_text(combo).to_owned()));
    if let Some(cast) = viewer.cast {
        let verb = if cast.channel {
            "Channelling"
        } else {
            "Casting"
        };
        parts.push(format!(
            "{verb} {} {}/{}",
            ability_name(cast.ability),
            cast.elapsed,
            cast.total
        ));
    }
    parts.extend(mastery_text(viewer.mastery));
    if viewer.in_combat {
        parts.push("In combat".into());
    }
    if let Some(target) = viewer.target {
        let unit = snapshot
            .entities
            .iter()
            .find(|unit| unit.entity() == target);
        let name = unit_name(target, snapshot.viewer_id, unit, content);
        let detail = unit.map_or_else(
            || "out of sight".into(),
            |unit| {
                if unit.flags.dead {
                    format!("level {}, dead", unit.level)
                } else {
                    format!("level {}, {}%", unit.level, unit.health_percent)
                }
            },
        );
        let casting = snapshot
            .target_detail
            .cast
            .map_or_else(String::new, |cast| {
                format!(
                    ", casting {} {}/{}",
                    ability_name(cast.ability),
                    cast.elapsed,
                    cast.total
                )
            });
        // Divine Reckoning (#936): the harm this enemy dealt, ready to return.
        let reckoning = if viewer.reckoning > 0 {
            format!(", reckoning {} stored", viewer.reckoning)
        } else {
            String::new()
        };
        parts.push(format!("Target: {name} ({detail}{casting}{reckoning})"));
    }
    parts.join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmorpg_core::{
        CastView, CreatureId, MinionId, NpcId, ResourceView, StrikeAim, greyhaven_vale,
    };

    #[test]
    fn every_refusal_reads_as_a_sentence() {
        let codes: Vec<_> = (0..=u16::from(u8::MAX))
            .filter_map(|code| mmorpg_protocol::error_code_from_wire(code).ok())
            .collect();
        assert!(codes.len() > 60, "every wire code");
        for code in codes {
            let text = refusal_text(code, None);
            assert!(
                text.starts_with(|c: char| c.is_uppercase() || c == '"')
                    && text.ends_with(['.', '"']),
                "{code:?}: {text}"
            );
        }
        assert_eq!(
            refusal_text(ErrorCode::NotEnoughResource, Some(ResourceKind::Rage)),
            "Not enough rage."
        );
        assert_eq!(
            refusal_text(ErrorCode::NotEnoughResource, None),
            "Not enough resource."
        );
    }

    #[test]
    fn panels_word_their_consequence_and_ignore_other_refusals() {
        use RefusalContext::*;
        let says = |code, context| panel_refusal(code, context);
        assert_eq!(
            says(ErrorCode::InventoryFull, Loot).as_deref(),
            Some("Your bags are full. The corpse keeps its rewards.")
        );
        assert_eq!(
            says(ErrorCode::InventoryFull, Unequip).as_deref(),
            Some("Your bags are full. The item stays equipped.")
        );
        assert_eq!(
            says(ErrorCode::InventoryFull, Bag).as_deref(),
            Some("That stack is full. Your items are unchanged.")
        );
        assert_eq!(
            says(ErrorCode::InvalidInventoryMove, Equip).as_deref(),
            Some("That equipment change was refused. Your items are unchanged.")
        );
        assert_eq!(
            says(ErrorCode::TooManyIntents, Vendor).as_deref(),
            Some("Too many actions. Try the trade again.")
        );
        // A vendor trade answers with any refusal it receives.
        assert_eq!(
            says(ErrorCode::VendorClosed, Vendor).as_deref(),
            Some("The shop is closed for the night.")
        );
        assert_eq!(says(ErrorCode::NotReady, Loot), None);
        assert_eq!(says(ErrorCode::NotReady, Bag), None);
        assert_eq!(says(ErrorCode::NotAtNight, Quest), None);
        assert_eq!(
            says(ErrorCode::QuestLogFull, Quest).as_deref(),
            Some("Your quest log is full. Abandon a quest first.")
        );
    }

    fn unit(kind: mmorpg_core::EntityKind, id: u32, appearance: u32) -> EntitySnapshot {
        EntitySnapshot {
            kind,
            id,
            appearance,
            position: [0; 3],
            velocity: [0; 3],
            facing: 0,
            level: 3,
            health_percent: 40,
            flags: mmorpg_core::EntityFlags::default(),
        }
    }

    fn lines(snapshot: &ZoneSnapshot, events: Vec<ZoneEvent>) -> Vec<String> {
        let content = greyhaven_vale::content();
        events
            .iter()
            .filter_map(|event| event_line(event, snapshot, &content))
            .map(|(_, text)| text)
            .collect()
    }

    #[test]
    fn events_read_from_the_viewers_point_of_view() {
        let content = greyhaven_vale::content();
        let template = content.creature_templates()[0].clone();
        let (me, foe) = (
            EntityRef::Player(7),
            EntityRef::Creature(CreatureId::new(220)),
        );
        let mut snapshot = crate::test_support::snapshot(7, 5);
        snapshot.entities = vec![unit(
            mmorpg_core::EntityKind::Creature,
            220,
            template.id.get(),
        )];
        let foe_name = template.name.clone();
        let aim = StrikeAim::new(StrikeSide::Right, StrikeHeight::High);
        assert_eq!(
            lines(
                &snapshot,
                vec![
                    ZoneEvent::DamageDealt {
                        source: me,
                        target: foe,
                        amount: 9,
                        critical: true
                    },
                    ZoneEvent::DamageTaken {
                        source: foe,
                        target: me,
                        amount: 4,
                        critical: false
                    },
                    ZoneEvent::Miss {
                        source: me,
                        target: foe
                    },
                    ZoneEvent::Miss {
                        source: foe,
                        target: me
                    },
                    ZoneEvent::Evade {
                        source: foe,
                        target: me
                    },
                    ZoneEvent::TrapHit {
                        trap: mmorpg_core::TrapId::new(1),
                        target: me,
                        amount: 3
                    },
                    ZoneEvent::Died {
                        entity: me,
                        killer: None
                    },
                    ZoneEvent::Died {
                        entity: foe,
                        killer: Some(me)
                    },
                    ZoneEvent::Clashed {
                        source: foe,
                        target: me,
                        kind: ClashKind::Parry
                    },
                    ZoneEvent::Interrupted {
                        source: None,
                        target: me,
                        ability: ids::SIGNAL_HORN
                    },
                    ZoneEvent::StrikeSwept {
                        source: me,
                        target: None,
                        ability: ids::SIGNAL_HORN,
                        aim
                    },
                    ZoneEvent::AttackPerformed {
                        source: me,
                        target: None
                    },
                ]
            ),
            [
                format!("You critically hit {foe_name} for 9."),
                format!("{foe_name} hits you for 4."),
                format!("You miss {foe_name}."),
                format!("{foe_name} misses you."),
                "You evade.".into(),
                "A trap hits you for 3.".into(),
                "You died.".into(),
                format!("{foe_name} dies."),
                format!("{foe_name} cuts down your shot."),
                "You are interrupted (Signal Horn).".into(),
                "Your high-right Signal Horn cuts only air.".to_owned(),
            ]
        );
    }

    #[test]
    fn a_summoner_reads_its_companions_fight_and_npc_refusals_name_the_npc() {
        let content = greyhaven_vale::content();
        let npc = content.npcs()[0].clone();
        let (me, wolf) = (EntityRef::Player(7), EntityRef::Minion(MinionId::new(1)));
        let mut snapshot = crate::test_support::snapshot(7, 5);
        let companion = (0..256)
            .find(|&appearance| minion_name(appearance) == "Wolf Companion")
            .unwrap();
        snapshot.entities = vec![unit(mmorpg_core::EntityKind::Minion, 1, companion)];
        assert_eq!(
            lines(
                &snapshot,
                vec![
                    ZoneEvent::AuraApplied {
                        source: me,
                        target: wolf,
                        ability: ids::ENTANGLING_ROOTS,
                        ticks: 5
                    },
                    ZoneEvent::Error {
                        code: ErrorCode::CartResting,
                        target: Some(EntityRef::Npc(npc.id))
                    },
                ]
            ),
            [
                "Wolf Companion is rooted by Entangling Roots.".to_owned(),
                format!(
                    "{}: \"Not yet. Let me see to the cart before we try the ford again.\"",
                    npc.name
                ),
            ]
        );
        assert_eq!(
            unit_name(EntityRef::Npc(NpcId::new(u32::MAX)), 7, None, &content),
            format!("NPC {}", u32::MAX)
        );
    }

    #[test]
    fn the_status_line_names_every_state_the_viewer_is_in() {
        let content = greyhaven_vale::content();
        let mut snapshot = crate::test_support::snapshot(7, 5);
        snapshot.viewer.health = 50;
        snapshot.viewer.max_health = 50;
        snapshot.viewer.level = 1;
        snapshot.viewer.breath = mmorpg_core::water::BREATH_MAX_TICKS;
        assert_eq!(status_line(&snapshot, &content), "Health 50/50 · Level 1");
        snapshot.viewer.resource = Some(ResourceView {
            kind: ResourceKind::Mana,
            value: 110,
            max: 110,
        });
        snapshot.viewer.breath = 15 * mmorpg_core::TICK_HZ;
        snapshot.viewer.stance = Some(MeleeStance::Charging);
        snapshot.viewer.combo = Some(ComboPhase::Open);
        snapshot.viewer.cast = Some(CastView {
            ability: ids::SIGNAL_HORN,
            elapsed: 3,
            total: 60,
            channel: false,
        });
        snapshot.viewer.in_combat = true;
        snapshot.viewer.target = Some(EntityRef::Creature(CreatureId::new(9)));
        assert_eq!(
            status_line(&snapshot, &content),
            "Health 50/50 · Mana 110/110 · Level 1 · Breath 15 s · Charging: +20% melee damage · \
             Combo: Shield Bash now · Casting Signal Horn 3/60 · In combat · \
             Target: a creature (out of sight)"
        );
        snapshot.viewer.breath = 0;
        assert!(status_line(&snapshot, &content).contains("Drowning!"));
        snapshot.viewer.dead = true;
        assert!(status_line(&snapshot, &content).contains("Press R to release"));
        assert_eq!(mastery_text(Mastery::default()), None);
    }
}
