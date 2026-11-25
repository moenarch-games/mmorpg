//! Quest views: objective text and progress, the quest tracker, what an NPC
//! offers and takes back, which quest NPC Talk addresses and which quest evidence
//! Interact inspects (#1019).
//!
//! Everything is derived from the received quest sheet and immutable content;
//! nothing is predicted. The zone checks reach, availability and completion.
use mmorpg_core::{
    EntityKind, EvidenceId, NpcId, QUEST_REACH_UNITS, Quest, QuestEntry, QuestId, QuestMarker,
    QuestObjective, QuestSheet, ZoneContent, ZoneSnapshot, item_template,
};

/// Whether `quest` is turned in according to `sheet`.
#[must_use]
pub fn turned_in(sheet: &QuestSheet, quest: QuestId) -> bool {
    sheet.completed.contains(&quest)
}

fn active(sheet: &QuestSheet, quest: QuestId) -> bool {
    sheet.entries.iter().any(|entry| entry.quest == quest)
}

/// Whether the viewer may accept `quest`: not active, not turned in, and its
/// prerequisite and required quests (#1019) turned in.
#[must_use]
pub fn available(sheet: &QuestSheet, quest: &Quest) -> bool {
    !active(sheet, quest.id)
        && !turned_in(sheet, quest.id)
        && quest
            .prerequisite
            .iter()
            .chain(&quest.requires)
            .all(|before| turned_in(sheet, *before))
}

/// Where the viewer stands with `quest` at `npc`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum QuestStanding {
    /// Active and ending here: turn it in (the zone checks the objectives).
    TurnIn,
    /// Available, and `npc` gives it.
    Offered,
    /// Active, ending elsewhere.
    InProgress,
    /// Available from its giver elsewhere.
    OfferedElsewhere,
    /// Waiting for its prerequisite.
    Locked,
    Completed,
}

/// The viewer's standing with `quest` at `npc`; the variants are in display order.
#[must_use]
pub fn standing(sheet: &QuestSheet, quest: &Quest, npc: NpcId) -> QuestStanding {
    if turned_in(sheet, quest.id) {
        QuestStanding::Completed
    } else if active(sheet, quest.id) {
        if quest.ender == npc {
            QuestStanding::TurnIn
        } else {
            QuestStanding::InProgress
        }
    } else if !available(sheet, quest) {
        QuestStanding::Locked
    } else if quest.giver == npc {
        QuestStanding::Offered
    } else {
        QuestStanding::OfferedElsewhere
    }
}

/// "Timber Wolf slain", "Wolf Pelt", "Speak with Farmer Osric Mill", "Explore Stillwater Lake"
/// or "Inspect the cut sacks".
#[must_use]
pub fn objective_label(objective: QuestObjective, content: &ZoneContent) -> String {
    let item = |item| {
        item_template(item)
            .map_or("Item", |item| item.name)
            .to_owned()
    };
    match objective {
        QuestObjective::Kill { template, .. } => format!(
            "{} slain",
            content
                .creature_template(template)
                .map_or("Creature", |template| template.name.as_str())
        ),
        QuestObjective::Collect { item: id, .. } | QuestObjective::Retrieve { item: id, .. } => {
            item(id)
        }
        QuestObjective::Talk { npc } => format!(
            "Speak with {}",
            content.npc(npc).map_or("someone", |npc| npc.name.as_str())
        ),
        QuestObjective::Explore { area } => format!(
            "Explore {}",
            content
                .areas()
                .areas()
                .iter()
                .find(|candidate| candidate.id() == area)
                .map_or("the area", |area| area.name())
        ),
        QuestObjective::DefeatThorn => {
            format!("Defeat {}", mmorpg_core::redbrand_depths::THORN_NAME)
        }
        QuestObjective::Inspect { spot } => format!(
            "Inspect {}",
            content
                .evidence_spot(spot)
                .map_or("the evidence", |spot| spot.name.as_str())
        ),
        // The quest actions and credits of #1019.
        QuestObjective::UseItem { item: id, npc } => {
            format!("Give {} to {}", item(id), objective_npc(content, npc))
        }
        QuestObjective::Escort { npc, area } => format!(
            "Escort {} to {}",
            objective_npc(content, npc),
            area_name(content, area)
        ),
        QuestObjective::Interview { npc } => format!("Be heard by {}", objective_npc(content, npc)),
        QuestObjective::DefeatWithLoan { template, .. } => format!(
            "{} slain with the loaned weapon",
            content
                .creature_template(template)
                .map_or("Creature", |template| template.name.as_str())
        ),
        QuestObjective::Crush { template, .. } => format!(
            "{} crushed by a boulder",
            content
                .creature_template(template)
                .map_or("Creature", |template| template.name.as_str())
        ),
        QuestObjective::Demolish { piece } => format!(
            "Bring down {}",
            content
                .pieces()
                .iter()
                .find(|candidate| candidate.id == piece)
                .map(|piece| piece.name.as_str())
                .filter(|name| !name.is_empty())
                .unwrap_or("the structure")
        ),
        QuestObjective::DeliverLoad { source, npc } => format!(
            "Carry {} to {}",
            content
                .load_spot(source)
                .map_or("the load", |spot| spot.name.as_str()),
            objective_npc(content, npc)
        ),
        // Construction, defence and aid (#1019).
        QuestObjective::Build { piece } => format!(
            "Raise {}",
            content
                .pieces()
                .iter()
                .find(|candidate| candidate.id == piece)
                .map(|piece| piece.name.as_str())
                .filter(|name| !name.is_empty())
                .unwrap_or("the structure")
        ),
        QuestObjective::Defend { npc } => {
            // The site its NPC keeps (#1019).
            let site = content.raid_of_steward(npc).map(|(_, site)| site);
            format!(
                "Hold {} with {} of its people standing",
                site.map_or("the farm", |site| site.name.as_str()),
                site.map_or(1, mmorpg_core::RaidSite::hold)
            )
        }
        QuestObjective::Aid { chore, .. } => match chore {
            mmorpg_core::AidChore::Mend => "Breaches mended".to_owned(),
            mmorpg_core::AidChore::Tend => "Wounded tended".to_owned(),
        },
        // The shrine and the ledger (#1019); a hidden find reads its landmark directions, since the
        // map marks nothing.
        QuestObjective::Transcribe { spot, item: id } => {
            let spot = content.evidence_spot(spot);
            match spot.and_then(|spot| spot.directions.as_deref()) {
                Some(directions) => format!("Find {} {directions}", item(id)),
                None => format!(
                    "{} of {}",
                    item(id),
                    spot.map_or("the inscription", |spot| spot.name.as_str())
                ),
            }
        }
        QuestObjective::Unseal { piece } => format!(
            "Open {}",
            content
                .pieces()
                .iter()
                .find(|candidate| candidate.id == piece)
                .map(|piece| piece.name.as_str())
                .filter(|name| !name.is_empty())
                .unwrap_or("the seal")
        ),
        QuestObjective::Decide { npc } => {
            // A decision over an item the quest had the player bring first, such as Fenford's
            // smuggler's token (#1019), is about that item, told to the NPC.
            let subject = content.quests().iter().find_map(|quest| {
                let index = quest.objectives.iter().position(|o| *o == objective)?;
                quest.objectives[..index]
                    .iter()
                    .find_map(|earlier| earlier.carried_item())
            });
            match subject {
                Some(id) => format!(
                    "Tell {} what becomes of the {}",
                    objective_npc(content, npc),
                    item(id)
                ),
                None => format!("Decide what becomes of {}", objective_npc(content, npc)),
            }
        }
        // Lise's flock (#1019): "Drive Lise's Stray Ewe into Lise's fold", "Keep 2 of the 3 ewes
        // standing through the watch", "Lay Tainted Mutton at the north-east gap in the hedge".
        QuestObjective::Herd { npc } => {
            format!("Drive {} home into its pen", objective_npc(content, npc))
        }
        QuestObjective::Watch { .. } => content.watch().map_or_else(
            || "Keep the flock through the watch".to_owned(),
            |watch| {
                format!(
                    "Keep {} of the {} ewes standing through the watch",
                    watch.hold,
                    watch.flock.len()
                )
            },
        ),
        QuestObjective::Lay { spot, item: id } => format!(
            "Lay {} at {}",
            item(id),
            content
                .evidence_spot(spot)
                .map_or("its place", |spot| spot.name.as_str())
        ),
        // The missing family and the muster (#1019): "Lead Hal Tolly and his family home to
        // Goodwife Ena", "Win Ploughman Garr to the muster".
        QuestObjective::EscortTo { npc, to } => {
            let family = if content.escort_line(npc).is_empty() {
                ""
            } else {
                " and his family"
            };
            format!(
                "Lead {}{family} home to {}",
                objective_npc(content, npc),
                objective_npc(content, to)
            )
        }
        QuestObjective::Recruit { npc } => {
            format!("Win {} to the muster", objective_npc(content, npc))
        }
        // The ring's measure (#1019): "Wake the ring's stones in the inscription's order"; the
        // quest view names the order once the inscription is read ([`quest_view`]).
        QuestObjective::WorkInOrder { sequence, .. } => format!(
            "Wake {} in the inscription's order",
            content
                .sequence(sequence)
                .map_or("the stones", |sequence| sequence.name.as_str())
        ),
        // The Downs road (#1019): "Watch the supply patrol come up the road with Lookout Brenna",
        // "Stop the supply patrol before it reaches its camp"; Interact at the lookout calls it out.
        QuestObjective::Sight { npc } => format!(
            "Watch the supply patrol come up the road with {}",
            objective_npc(content, npc)
        ),
        QuestObjective::Ambush { .. } => {
            "Stop the supply patrol before it reaches its camp".to_owned()
        }
        // The ford (#1019): "See Carter Aldo's cart over the ford", "Bring down River Crayfish
        // from the side or back"; Interact at the carter starts the crossing.
        QuestObjective::Crossing { npc } => {
            format!("See {}'s cart over the ford", objective_npc(content, npc))
        }
        QuestObjective::Outflank { template, .. } => format!(
            "Bring down {} from the side or back",
            content
                .creature_template(template)
                .map_or("them", |template| template.name.as_str())
        ),
    }
}

/// The order of an in-order objective's mechanisms (#1019), as in "the dawn stone, the noon
/// stone, the dusk stone, then the night stone".
fn sequence_order(content: &ZoneContent, sequence: mmorpg_core::SequenceId) -> Option<String> {
    let names: Vec<&str> = content
        .sequence(sequence)?
        .mechanisms
        .iter()
        .map(|id| {
            content
                .mechanism(*id)
                .map_or("a stone", |m| m.name.as_str())
        })
        .collect();
    let (last, rest) = names.split_last()?;
    Some(format!("{}, then {last}", rest.join(", ")))
}

fn objective_npc(content: &ZoneContent, npc: NpcId) -> &str {
    content.npc(npc).map_or("someone", |npc| npc.name.as_str())
}

fn area_name(content: &ZoneContent, area: mmorpg_core::AreaId) -> &str {
    content
        .areas()
        .areas()
        .iter()
        .find(|candidate| candidate.id() == area)
        .map_or("the area", |area| area.name())
}

/// One objective with its received progress.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectiveView {
    pub label: String,
    /// "Timber Wolf slain: 4/6" for counted objectives, otherwise the label,
    /// with " (done)" once done.
    pub text: String,
    pub current: u8,
    pub required: u8,
    pub done: bool,
}

/// `objective` at `current` progress.
#[must_use]
pub fn objective_view(
    objective: QuestObjective,
    current: u8,
    content: &ZoneContent,
) -> ObjectiveView {
    let label = objective_label(objective, content);
    let required = objective.required();
    let done = current >= required;
    let text = match objective {
        QuestObjective::Kill { .. }
        | QuestObjective::Collect { .. }
        | QuestObjective::Retrieve { .. }
        | QuestObjective::DefeatWithLoan { .. }
        | QuestObjective::Crush { .. }
        | QuestObjective::Aid { .. }
        | QuestObjective::Transcribe { .. }
        | QuestObjective::WorkInOrder { .. }
        | QuestObjective::Outflank { .. } => {
            format!("{label}: {}/{required}", current.min(required))
        }
        QuestObjective::Talk { .. }
        | QuestObjective::Explore { .. }
        | QuestObjective::DefeatThorn
        | QuestObjective::Inspect { .. }
        | QuestObjective::UseItem { .. }
        | QuestObjective::Escort { .. }
        | QuestObjective::Interview { .. }
        | QuestObjective::Demolish { .. }
        | QuestObjective::DeliverLoad { .. }
        | QuestObjective::Build { .. }
        | QuestObjective::Defend { .. }
        | QuestObjective::Unseal { .. }
        | QuestObjective::Decide { .. }
        | QuestObjective::Herd { .. }
        | QuestObjective::Watch { .. }
        | QuestObjective::Lay { .. }
        | QuestObjective::EscortTo { .. }
        | QuestObjective::Recruit { .. }
        | QuestObjective::Sight { .. }
        | QuestObjective::Ambush { .. }
        | QuestObjective::Crossing { .. }
            if done =>
        {
            format!("{label} (done)")
        }
        QuestObjective::Talk { .. }
        | QuestObjective::Explore { .. }
        | QuestObjective::DefeatThorn
        | QuestObjective::Inspect { .. }
        | QuestObjective::UseItem { .. }
        | QuestObjective::Escort { .. }
        | QuestObjective::Interview { .. }
        | QuestObjective::Demolish { .. }
        | QuestObjective::DeliverLoad { .. }
        | QuestObjective::Build { .. }
        | QuestObjective::Defend { .. }
        | QuestObjective::Unseal { .. }
        | QuestObjective::Decide { .. }
        | QuestObjective::Herd { .. }
        | QuestObjective::Watch { .. }
        | QuestObjective::Lay { .. }
        | QuestObjective::EscortTo { .. }
        | QuestObjective::Recruit { .. }
        | QuestObjective::Sight { .. }
        | QuestObjective::Ambush { .. }
        | QuestObjective::Crossing { .. } => label.clone(),
    };
    ObjectiveView {
        label,
        text,
        current,
        required,
        done,
    }
}

/// An active quest with each objective's received progress.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuestView {
    pub quest: QuestId,
    pub objectives: Vec<ObjectiveView>,
    pub complete: bool,
}

/// `quest` at `progress`, one count per objective (missing counts are 0). An in-order objective
/// (#1019) names its order once every earlier objective is done, the inscription read: "Wake the
/// ring's stones in the inscription's order: the dawn stone, ..., then the night stone: 2/4".
#[must_use]
pub fn quest_view(quest: &Quest, progress: &[u8], content: &ZoneContent) -> QuestView {
    let count = |index: usize| progress.get(index).copied().unwrap_or(0);
    let objectives: Vec<_> = quest
        .objectives
        .iter()
        .enumerate()
        .map(|(index, objective)| {
            let mut view = objective_view(*objective, count(index), content);
            let read = quest.objectives[..index]
                .iter()
                .enumerate()
                .all(|(earlier, objective)| count(earlier) >= objective.required());
            if let QuestObjective::WorkInOrder { sequence, .. } = *objective
                && read
                && let Some(order) = sequence_order(content, sequence)
            {
                view.label = format!("{}: {order}", view.label);
                view.text = format!(
                    "{}: {}/{}",
                    view.label,
                    view.current.min(view.required),
                    view.required
                );
            }
            view
        })
        .collect();
    QuestView {
        quest: quest.id,
        complete: objectives.iter().all(|objective| objective.done),
        objectives,
    }
}

/// The sheet's active quests in quest-ID order; quests the content does not
/// know are left out.
#[must_use]
pub fn quest_views(sheet: &QuestSheet, content: &ZoneContent) -> Vec<QuestView> {
    sheet
        .entries
        .iter()
        .filter_map(|entry| {
            let quest = content.quest(entry.quest)?;
            Some(quest_view(quest, &entry.progress, content))
        })
        .collect()
}

/// One quest-tracker row: a quest heading (`objective` `None`) or one of its objectives.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrackerRow {
    pub text: String,
    pub quest: QuestId,
    pub objective: Option<u8>,
    /// The quest is complete, or the objective done.
    pub done: bool,
}

/// The HUD line of a running defence (#1019) at assault `phase` with `standing` of the raid
/// site's civilians on their feet, as the raid signal carries them: "Millbrook Farm under
/// attack: wave 2 of 3, 3 of 3 of its people standing (2 must stand)". The site is the one whose
/// signal reaches the viewer's feet `at` (#1019), otherwise the night raid target. `None` without
/// a raid site.
#[must_use]
pub fn defence_status(
    phase: u8,
    standing: u8,
    content: &ZoneContent,
    at: Option<[i32; 2]>,
) -> Option<String> {
    let site = at
        .and_then(|at| content.raid_near(at))
        .map(|(_, site)| site)
        .or_else(|| content.raid())?;
    Some(format!(
        "{} under attack: wave {phase} of {}, {standing} of {} of its people standing ({} must stand)",
        site.name,
        mmorpg_core::raid::RAID_PHASES,
        site.civilians.len(),
        site.hold()
    ))
}

/// The HUD tracker: each active quest followed by its objectives.
#[must_use]
pub fn tracker_rows(sheet: &QuestSheet, content: &ZoneContent) -> Vec<TrackerRow> {
    let mut rows = Vec::new();
    for view in quest_views(sheet, content) {
        let Some(quest) = content.quest(view.quest) else {
            continue;
        };
        rows.push(TrackerRow {
            text: if view.complete {
                format!(
                    "{} — return to {}",
                    quest.name,
                    npc_name(content, quest.ender)
                )
            } else {
                quest.name.clone()
            },
            quest: view.quest,
            objective: None,
            done: view.complete,
        });
        for (index, objective) in (0u8..).zip(view.objectives) {
            rows.push(TrackerRow {
                text: format!(
                    "  {} {}",
                    if objective.done { "✓" } else { "·" },
                    objective.text
                ),
                quest: view.quest,
                objective: Some(index),
                done: objective.done,
            });
        }
    }
    rows
}

fn npc_name(content: &ZoneContent, npc: NpcId) -> &str {
    content
        .npc(npc)
        .map_or("the quest giver", |npc| npc.name.as_str())
}

/// The viewer's marker over `npc`, if any.
#[must_use]
pub fn marker(sheet: &QuestSheet, npc: NpcId) -> Option<QuestMarker> {
    sheet
        .markers
        .iter()
        .find(|marker| marker.npc == npc)
        .map(|marker| marker.marker)
}

/// A projected quest giver or ender and whether it looks within reach.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QuestNpc {
    pub npc: NpcId,
    pub in_reach: bool,
}

fn quest_npc(content: &ZoneContent, npc: NpcId) -> bool {
    content.quests().iter().any(|quest| {
        quest.giver == npc
            || quest.ender == npc
            // An NPC who hears a decision (#1019).
            || quest
                .objectives
                .contains(&QuestObjective::Decide { npc })
    })
}

/// The quest NPC that Talk addresses. An explicitly selected NPC is the only
/// possible addressee, even without quests; without an NPC selection it is
/// the nearest projected quest NPC (lower ID on a tie). The zone checks reach.
#[must_use]
pub fn dialog_npc(snapshot: &ZoneSnapshot, content: &ZoneContent) -> Option<QuestNpc> {
    let viewer = snapshot.entities.first()?;
    let selected = snapshot
        .viewer
        .target
        .filter(|target| target.kind() == EntityKind::Npc);
    let distance = |position: [i32; 3]| {
        f64::from(position[0] - viewer.position[0])
            .hypot(f64::from(position[2] - viewer.position[2]))
    };
    let (npc, distance) = snapshot
        .entities
        .iter()
        .filter(|entity| entity.kind == EntityKind::Npc)
        .filter(|entity| selected.is_none_or(|selected| selected == entity.entity()))
        .filter(|entity| quest_npc(content, NpcId::new(entity.id)))
        .map(|entity| (entity.id, distance(entity.position)))
        .min_by(|left, right| left.1.total_cmp(&right.1).then(left.0.cmp(&right.0)))?;
    Some(QuestNpc {
        npc: NpcId::new(npc),
        in_reach: distance <= f64::from(QUEST_REACH_UNITS),
    })
}

/// What one NPC offers the viewer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DialogView {
    /// A woken private-home resident's line before its turn-ins (#158).
    pub greeting: Option<String>,
    /// Quests to accept, in quest-ID order.
    pub offered: Vec<QuestId>,
    /// Active quests ending here, complete.
    pub ready: Vec<QuestView>,
    /// Active quests ending here, still in progress.
    pub in_progress: Vec<QuestView>,
    /// The decision (#1019) this NPC awaits from the viewer, if one is open.
    pub decision: Option<DecisionView>,
}

/// A quest decision the viewer can make at an NPC (#1019): the quest and its options, offered
/// as [`mmorpg_core::ZoneCommand::Decide`] with option `index + 1`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecisionView {
    pub quest: QuestId,
    pub options: Vec<String>,
}

/// The decision (#1019) open at `npc` for a player with the active quests `entries`: the first
/// undecided decide objective at that NPC whose quest's earlier objectives are all done, as the
/// zone picks it. The zone checks reach and the option again.
#[must_use]
pub fn decision_at(
    content: &ZoneContent,
    entries: &[QuestEntry],
    npc: NpcId,
) -> Option<DecisionView> {
    entries.iter().find_map(|entry| {
        let quest = content.quest(entry.quest)?;
        let index = quest.objectives.iter().position(
            |objective| matches!(*objective, QuestObjective::Decide { npc: at } if at == npc),
        )?;
        let ready = entry.progress[index] == 0
            && quest.objectives[..index]
                .iter()
                .zip(entry.progress)
                .all(|(earlier, count)| count >= earlier.required());
        // A paid option (#1019) names its price, and one that pays at the turn-in its reward.
        ready.then(|| DecisionView {
            quest: quest.id,
            options: quest
                .decision
                .iter()
                .map(|option| match (option.copper, option.reward) {
                    (0, 0) => option.label.clone(),
                    (0, reward) => format!("{} (+{reward} copper)", option.label),
                    (copper, 0) => format!("{} ({copper} copper)", option.label),
                    (copper, reward) => {
                        format!("{} ({copper} copper, +{reward} copper)", option.label)
                    }
                })
                .collect(),
        })
    })
}

/// At `night` a private-home resident greets with its woken line and offers
/// no new quests, only turn-ins; the zone enforces both.
#[must_use]
pub fn dialog_view(
    sheet: &QuestSheet,
    content: &ZoneContent,
    npc: NpcId,
    night: bool,
) -> DialogView {
    let greeting = content
        .private_home(npc)
        .filter(|_| night)
        .map(|home| home.woken_line.clone());
    let offered = if greeting.is_some() {
        Vec::new()
    } else {
        content
            .quests()
            .iter()
            .filter(|quest| quest.giver == npc && available(sheet, quest))
            .map(|quest| quest.id)
            .collect()
    };
    let (ready, in_progress) = quest_views(sheet, content)
        .into_iter()
        .filter(|view| {
            content
                .quest(view.quest)
                .is_some_and(|quest| quest.ender == npc)
        })
        .partition(|view| view.complete);
    DialogView {
        greeting,
        offered,
        ready,
        in_progress,
        decision: decision_at(content, &sheet.entries, npc),
    }
}

/// The quest evidence (#1019) a player whose body centre is at `centre` units (as projections place
/// it) would inspect now: the nearest spot within its reach that an objective of an active quest in `entries` still needs, so a press of
/// Interact there reads it, or a lure's stake where an active quest's supplied bait is cast. Spots no
/// quest needs are none; the zone checks everything again.
#[must_use]
pub fn evidence_in_reach(
    content: &ZoneContent,
    entries: &[QuestEntry],
    centre: [i32; 3],
) -> Option<EvidenceId> {
    let feet = [
        centre[0],
        centre[1] - mmorpg_core::PLAYER_HALF_EXTENTS_UNITS[1],
        centre[2],
    ];
    let open = entries.iter().flat_map(|entry| {
        content
            .quest(entry.quest)
            .into_iter()
            .flat_map(|quest| quest.objectives.iter().zip(entry.progress))
            .filter_map(|(objective, progress)| match *objective {
                QuestObjective::Inspect { spot } if progress == 0 => Some(spot),
                // A rubbing (#1019) is taken by inspecting its spot while the bag holds none.
                QuestObjective::Transcribe { spot, .. } if progress == 0 => Some(spot),
                // Bait or a snare (#1019) is laid by inspecting its spot.
                QuestObjective::Lay { spot, .. } if progress == 0 => Some(spot),
                _ => None,
            })
    });
    // A lure's stake (#1019) casts the bait an active quest supplied, as often as wanted.
    let lures = content
        .lures()
        .iter()
        .filter(|lure| {
            entries.iter().any(|entry| {
                content.quest(entry.quest).is_some_and(|quest| {
                    quest.supplies.iter().any(|supply| supply.item == lure.item)
                })
            })
        })
        .map(|lure| lure.spot);
    open.chain(lures)
        .filter_map(|spot| content.evidence_spot(spot))
        .filter(|spot| spot.within(feet))
        .min_by_key(|spot| {
            let (dx, dz) = (
                i64::from(spot.at[0] - feet[0]),
                i64::from(spot.at[2] - feet[2]),
            );
            (dx * dx + dz * dz, spot.id)
        })
        .map(|spot| spot.id)
}

/// The load (#1077) Interact lifts for a player centred at `centre` units with the active quests
/// `entries`, seeing the set-down `loads`: its own set-down load in reach first, otherwise the
/// nearest load source in reach that an open delivery or build (#1019) of theirs names. `None` when there is
/// none, or while the player already carries one (`carrying`, Interact then sets it down unless
/// a selected NPC takes it). The zone checks everything again.
#[must_use]
pub fn lift_in_reach(
    content: &ZoneContent,
    entries: &[QuestEntry],
    centre: [i32; 3],
    carrying: bool,
    loads: &[mmorpg_core::LoadView],
) -> Option<mmorpg_core::ZoneCommand> {
    if carrying {
        return None;
    }
    let feet = [
        centre[0],
        centre[1] - mmorpg_core::PLAYER_HALF_EXTENTS_UNITS[1],
        centre[2],
    ];
    let distance = |at: [i32; 3]| {
        let (dx, dz) = (i64::from(at[0] - feet[0]), i64::from(at[2] - feet[2]));
        dx * dx + dz * dz
    };
    let own = loads.iter().any(|load| {
        load.own
            && distance(load.at) <= i64::from(mmorpg_core::LOAD_REACH_UNITS).pow(2)
            && (load.at[1] - feet[1]).abs() <= mmorpg_core::LOAD_HEIGHT_UNITS
    });
    if own {
        return Some(mmorpg_core::ZoneCommand::Lift { source: None });
    }
    entries
        .iter()
        .flat_map(|entry| {
            content
                .quest(entry.quest)
                .into_iter()
                .flat_map(|quest| quest.objectives.iter().zip(entry.progress))
                .filter_map(|(objective, progress)| match *objective {
                    QuestObjective::DeliverLoad { source, .. } if progress == 0 => Some(source),
                    // A build's material (#1019).
                    QuestObjective::Build { piece } if progress == 0 => {
                        content.construction(piece).map(|site| site.source)
                    }
                    _ => None,
                })
        })
        .filter_map(|source| content.load_spot(source))
        .filter(|spot| spot.within(feet))
        .min_by_key(|spot| (distance(spot.at), spot.id))
        .map(|spot| mmorpg_core::ZoneCommand::Lift {
            source: Some(spot.id),
        })
}

/// The quest action (#1019) Interact performs at `npc` for a player with the active quests
/// `entries`: the first open use-item, escort or interview objective at that NPC, in log order,
/// whose quest's earlier objectives are all done, as the zone picks it; otherwise a decision
/// decided there whose option still acts in the world ([`Quest::unresolved_decision`]: the fight
/// faced again, the item handed over anew). `None` leaves Interact to talk. The zone checks
/// reach, the bag, the NPC's trust and everything else again.
#[must_use]
pub fn quest_action_at(
    content: &ZoneContent,
    entries: &[QuestEntry],
    npc: NpcId,
) -> Option<QuestObjective> {
    open_action_at(content, entries, npc).or_else(|| {
        entries.iter().find_map(|entry| {
            let quest = content.quest(entry.quest)?;
            quest
                .unresolved_decision(&entry.progress, npc)
                .map(|(index, _)| quest.objectives[index])
        })
    })
}

fn open_action_at(
    content: &ZoneContent,
    entries: &[QuestEntry],
    npc: NpcId,
) -> Option<QuestObjective> {
    entries.iter().find_map(|entry| {
        let quest = content.quest(entry.quest)?;
        quest
            .objectives
            .iter()
            .enumerate()
            .find(|(index, objective)| {
                // A decision (#1019) is made in the dialog, never by Interact.
                !matches!(objective, QuestObjective::Decide { .. })
                    && objective.action_npc() == Some(npc)
                    && entry.progress[*index] < objective.required()
                    && quest.objectives[..*index].iter().zip(entry.progress).all(
                        |(earlier, count)| {
                            // A muster's recruits (#1019) come in any order, as the zone rules.
                            count >= earlier.required()
                                || (matches!(objective, QuestObjective::Recruit { .. })
                                    && matches!(earlier, QuestObjective::Recruit { .. }))
                        },
                    )
            })
            .map(|(_, objective)| *objective)
    })
}

/// The mechanism (#1019) Interact works for a player centred at `centre` units: the nearest glyph
/// within its reach, quest or not, since anyone may work one. The zone checks reach again.
#[must_use]
pub fn mechanism_in_reach(
    content: &ZoneContent,
    centre: [i32; 3],
) -> Option<&mmorpg_core::Mechanism> {
    let feet = [
        centre[0],
        centre[1] - mmorpg_core::PLAYER_HALF_EXTENTS_UNITS[1],
        centre[2],
    ];
    content
        .mechanisms()
        .iter()
        .filter(|mechanism| mechanism.within(feet))
        .min_by_key(|mechanism| {
            let (dx, dz) = (
                i64::from(mechanism.at[0] - feet[0]),
                i64::from(mechanism.at[2] - feet[2]),
            );
            (dx * dx + dz * dz, mechanism.id)
        })
}

/// The HUD lines of the shrine's seals and wards (#1019) at `tick`, as the world sheet carries
/// them: "The shrine's seal: 1 of 2 glyphs set", "The shrine's seal stands open" and "Shrine
/// Warden's weak point bared: 12 s". A closed seal with no glyph set and a warded guardian show
/// nothing.
#[must_use]
pub fn shrine_status(
    seals: &[mmorpg_core::SealView],
    wards: &[mmorpg_core::WardView],
    tick: u64,
    content: &ZoneContent,
) -> Vec<String> {
    let mut lines = Vec::new();
    for (view, seal) in seals.iter().zip(content.seals()) {
        let name = content
            .pieces()
            .iter()
            .find(|piece| piece.id == seal.piece)
            .map_or("the seal", |piece| piece.name.as_str());
        let mut name = name.to_owned();
        if let Some(first) = name.get_mut(..1) {
            first.make_ascii_uppercase();
        }
        if view.open {
            lines.push(format!("{name} stands open"));
        } else if view.set != 0 {
            lines.push(format!(
                "{name}: {} of {} glyphs set",
                view.set.count_ones(),
                seal.mechanisms.len()
            ));
        }
    }
    for (view, ward) in wards.iter().zip(content.wards()) {
        if view.exposed_at(tick) {
            let seconds = (view.exposed_until - tick).div_ceil(u64::from(mmorpg_core::TICK_HZ));
            lines.push(format!(
                "{}'s weak point bared: {seconds} s",
                content
                    .creature_template(ward.template)
                    .map_or("The guardian", |template| template.name.as_str())
            ));
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmorpg_core::{
        EntityFlags, EntityRef, EntitySnapshot, MAX_QUEST_OBJECTIVES, QuestEntry, greyhaven_vale,
    };
    use std::sync::Arc;

    fn content() -> Arc<ZoneContent> {
        greyhaven_vale::content()
    }

    fn entry(quest: QuestId, progress: &[u8]) -> QuestEntry {
        let mut counts = [0; MAX_QUEST_OBJECTIVES];
        counts[..progress.len()].copy_from_slice(progress);
        QuestEntry {
            quest,
            progress: counts,
        }
    }

    fn sheet(completed: &[QuestId], entries: Vec<QuestEntry>) -> QuestSheet {
        QuestSheet {
            completed: completed.iter().copied().collect(),
            entries,
            markers: Vec::new(),
        }
    }

    #[test]
    fn objectives_read_their_progress_and_count_only_counted_kinds() {
        let content = content();
        for quest in content.quests() {
            let none = quest_view(quest, &[], &content);
            assert!(!none.complete, "{}", quest.name);
            let full: Vec<_> = quest.objectives.iter().map(|o| o.required()).collect();
            let done = quest_view(quest, &full, &content);
            assert!(done.complete);
            for (objective, view) in quest.objectives.iter().zip(&done.objectives) {
                let required = objective.required();
                match objective {
                    QuestObjective::Kill { .. }
                    | QuestObjective::Collect { .. }
                    | QuestObjective::Retrieve { .. }
                    | QuestObjective::DefeatWithLoan { .. }
                    | QuestObjective::Crush { .. }
                    | QuestObjective::Aid { .. }
                    | QuestObjective::Transcribe { .. }
                    | QuestObjective::WorkInOrder { .. }
                    | QuestObjective::Outflank { .. } => {
                        assert_eq!(view.text, format!("{}: {required}/{required}", view.label));
                    }
                    QuestObjective::Talk { .. }
                    | QuestObjective::Explore { .. }
                    | QuestObjective::DefeatThorn
                    | QuestObjective::Inspect { .. }
                    | QuestObjective::UseItem { .. }
                    | QuestObjective::Escort { .. }
                    | QuestObjective::Interview { .. }
                    | QuestObjective::Demolish { .. }
                    | QuestObjective::DeliverLoad { .. }
                    | QuestObjective::Build { .. }
                    | QuestObjective::Defend { .. }
                    | QuestObjective::Unseal { .. }
                    | QuestObjective::Decide { .. }
                    | QuestObjective::Herd { .. }
                    | QuestObjective::Watch { .. }
                    | QuestObjective::Lay { .. }
                    | QuestObjective::EscortTo { .. }
                    | QuestObjective::Recruit { .. }
                    | QuestObjective::Sight { .. }
                    | QuestObjective::Ambush { .. }
                    | QuestObjective::Crossing { .. } => {
                        assert_eq!(view.text, format!("{} (done)", view.label));
                    }
                }
            }
        }
        let wolves = QuestObjective::Kill {
            template: content.creature_templates()[0].id,
            count: 6,
        };
        let view = objective_view(wolves, 9, &content);
        assert_eq!(
            view.text,
            format!("{} slain: 6/6", content.creature_templates()[0].name)
        );
        assert!(view.done);
    }

    #[test]
    fn standing_follows_the_sheet_and_the_prerequisite() {
        let content = content();
        let quest = content
            .quests()
            .iter()
            .find(|quest| quest.prerequisite.is_some() && quest.giver != quest.ender)
            .expect("a chained quest with separate giver and ender");
        let prerequisite = quest.prerequisite.unwrap();
        let (giver, ender) = (quest.giver, quest.ender);
        let locked = sheet(&[], Vec::new());
        assert_eq!(standing(&locked, quest, giver), QuestStanding::Locked);
        let unlocked = sheet(&[prerequisite], Vec::new());
        assert_eq!(standing(&unlocked, quest, giver), QuestStanding::Offered);
        assert_eq!(
            standing(&unlocked, quest, ender),
            QuestStanding::OfferedElsewhere
        );
        let accepted = sheet(&[prerequisite], vec![entry(quest.id, &[])]);
        assert_eq!(standing(&accepted, quest, ender), QuestStanding::TurnIn);
        assert_eq!(standing(&accepted, quest, giver), QuestStanding::InProgress);
        let done = sheet(&[prerequisite, quest.id], Vec::new());
        assert_eq!(standing(&done, quest, ender), QuestStanding::Completed);

        // The dialog offers it once unlocked and takes it back where it ends.
        assert!(
            !dialog_view(&locked, &content, giver, false)
                .offered
                .contains(&quest.id)
        );
        assert!(
            dialog_view(&unlocked, &content, giver, false)
                .offered
                .contains(&quest.id)
        );
        let ending = dialog_view(&accepted, &content, ender, false);
        assert_eq!(
            ending
                .in_progress
                .iter()
                .map(|view| view.quest)
                .collect::<Vec<_>>(),
            [quest.id]
        );
        let full: Vec<_> = quest.objectives.iter().map(|o| o.required()).collect();
        let ready = sheet(&[prerequisite], vec![entry(quest.id, &full)]);
        let ready = dialog_view(&ready, &content, ender, false);
        assert_eq!(ready.ready.len(), 1);
        assert!(ready.in_progress.is_empty());
    }

    #[test]
    fn a_woken_resident_greets_at_night_and_offers_only_turn_ins() {
        let content = content();
        let home = content
            .private_homes()
            .iter()
            .find(|home| content.quests().iter().any(|quest| quest.giver == home.npc))
            .expect("a private home whose resident gives quests");
        let empty = sheet(&[], Vec::new());
        let day = dialog_view(&empty, &content, home.npc, false);
        assert_eq!(day.greeting, None);
        let night = dialog_view(&empty, &content, home.npc, true);
        assert_eq!(night.greeting.as_deref(), Some(home.woken_line.as_str()));
        assert!(night.offered.is_empty());
    }

    #[test]
    fn the_tracker_lists_each_quest_and_its_objectives() {
        let content = content();
        let quest = &content.quests()[0];
        let full: Vec<_> = quest.objectives.iter().map(|o| o.required()).collect();
        let rows = tracker_rows(&sheet(&[], vec![entry(quest.id, &full)]), &content);
        assert_eq!(rows.len(), 1 + quest.objectives.len());
        assert_eq!(
            rows[0].text,
            format!(
                "{} — return to {}",
                quest.name,
                content.npc(quest.ender).unwrap().name
            )
        );
        assert!(rows[1].text.starts_with("  ✓ ") && rows[1].done);
        assert_eq!(rows[1].objective, Some(0));
        let started = tracker_rows(&sheet(&[], vec![entry(quest.id, &[])]), &content);
        assert_eq!(started[0].text, quest.name);
        assert!(started[1].text.starts_with("  · ") && !started[1].done);
    }

    #[test]
    fn talk_prefers_the_selected_quest_npc_then_the_nearest_and_hints_reach() {
        let content = content();
        let quest_npcs: Vec<NpcId> = {
            let mut npcs: Vec<_> = content
                .quests()
                .iter()
                .flat_map(|quest| [quest.giver, quest.ender])
                .collect();
            npcs.sort();
            npcs.dedup();
            npcs
        };
        let (near, far) = (quest_npcs[0], quest_npcs[1]);
        let other = content
            .npcs()
            .iter()
            .map(|npc| npc.id)
            .find(|npc| !quest_npcs.contains(npc))
            .expect("an NPC without quests");
        let unit = |kind, id, x, z| EntitySnapshot {
            kind,
            id,
            appearance: 0,
            position: [x, 90, z],
            velocity: [0; 3],
            facing: 0,
            level: 1,
            health_percent: 100,
            flags: EntityFlags::default(),
        };
        let mut snapshot = crate::test_support::snapshot(7, 1);
        snapshot.entities = vec![
            unit(EntityKind::Player, 7, 0, 0),
            unit(EntityKind::Npc, far.get(), 100, 400),
            unit(EntityKind::Npc, near.get(), 300, 0),
            unit(EntityKind::Npc, other.get(), 50, 0),
        ];
        assert_eq!(
            dialog_npc(&snapshot, &content),
            Some(QuestNpc {
                npc: near,
                in_reach: true
            })
        );
        snapshot.viewer.target = Some(EntityRef::Npc(far));
        assert_eq!(
            dialog_npc(&snapshot, &content).map(|npc| npc.npc),
            Some(far)
        );
        // An explicitly selected non-quest or missing NPC never redirects Talk.
        for selected in [other, NpcId::new(999)] {
            snapshot.viewer.target = Some(EntityRef::Npc(selected));
            assert_eq!(dialog_npc(&snapshot, &content), None);
        }
        snapshot.viewer.target = None;
        snapshot.entities = vec![
            unit(EntityKind::Player, 7, 0, 0),
            unit(EntityKind::Npc, near.get(), 600, 0),
        ];
        assert_eq!(
            dialog_npc(&snapshot, &content).map(|npc| npc.in_reach),
            Some(false)
        );
    }

    /// Interact inspects the nearest evidence in reach that an active quest still needs (#1019):
    /// not evidence already read, nor any quest's evidence out of reach.
    #[test]
    fn interact_inspects_only_open_evidence_in_reach() {
        use greyhaven_vale::quests::{self, CUT_SACKS, REDBRAND_MARKER};
        let content = content();
        let centre = |spot: EvidenceId| {
            let [x, y, z] = content.evidence_spot(spot).unwrap().at;
            [x + 20, y + mmorpg_core::PLAYER_HALF_EXTENTS_UNITS[1], z]
        };
        let open = [entry(quests::CUT_SACKS_RED_RAGS, &[0, 0, 0])];
        let read = [entry(quests::CUT_SACKS_RED_RAGS, &[1, 0, 0])];
        assert_eq!(
            evidence_in_reach(&content, &open, centre(CUT_SACKS)),
            Some(CUT_SACKS)
        );
        // The marker stands beside the sacks: once they are read, Interact reads the marker.
        assert_eq!(
            evidence_in_reach(&content, &read, centre(CUT_SACKS)),
            Some(REDBRAND_MARKER)
        );
        assert_eq!(evidence_in_reach(&content, &[], centre(CUT_SACKS)), None);
        let [x, y, z] = centre(CUT_SACKS);
        assert_eq!(evidence_in_reach(&content, &open, [x + 2_000, y, z]), None);
        assert_eq!(evidence_in_reach(&content, &open, [x, y + 400, z]), None);
    }

    /// Interact lifts the player's own set-down load first, else the open delivery's source in
    /// reach (#1077); nothing while the arms are full or the delivery is done.
    #[test]
    fn interact_lifts_an_open_deliverys_load_in_reach() {
        use greyhaven_vale::quests::{CART_SACK, THE_LAST_SACK};
        use mmorpg_core::{LoadKind, LoadView, ZoneCommand};
        let content = content();
        let [x, y, z] = content.load_spot(CART_SACK).unwrap().at;
        let centre = [x + 150, y + mmorpg_core::PLAYER_HALF_EXTENTS_UNITS[1], z];
        let open = [entry(THE_LAST_SACK, &[0])];
        let lift = |entries: &[QuestEntry], at, carrying, loads: &[LoadView]| {
            lift_in_reach(&content, entries, at, carrying, loads)
        };
        assert_eq!(
            lift(&open, centre, false, &[]),
            Some(ZoneCommand::Lift {
                source: Some(CART_SACK)
            })
        );
        assert_eq!(lift(&open, centre, true, &[]), None);
        assert_eq!(
            lift(&[entry(THE_LAST_SACK, &[1])], centre, false, &[]),
            None
        );
        assert_eq!(lift(&[], centre, false, &[]), None);
        assert_eq!(lift(&open, [x + 2_000, centre[1], z], false, &[]), None);
        let set_down = |own| LoadView {
            kind: LoadKind::Sack,
            own,
            at: [x + 100, y, z],
            facing: 0,
        };
        assert_eq!(
            lift(&open, centre, false, &[set_down(false), set_down(true)]),
            Some(ZoneCommand::Lift { source: None })
        );
        assert_eq!(
            lift(&[], centre, false, &[set_down(false)]),
            None,
            "another player's load is theirs"
        );
    }
}
