//! The Greyhaven chain (quest catalog 1): nine quests from the outpost's
//! three quest givers, from the Wolfrun Woods to Garrick Redbrand and back.
//!
//! Marshal Elden Greywatch (NPC 1) sends new characters against the wolves
//! and later the lake and the hollow; Tanner Hilda Brook (NPC 2) wants
//! pelts and worries about her missing farmhand; Farmer Osric Mill (NPC 4)
//! asks for help against the marauders. Each quest unlocks the next.

use super::{REDBRAND_HOLLOW, STILLWATER_LAKE};
use crate::{ItemId, NpcId, Quest, QuestId, QuestObjective, QuestRewards};

use super::units::templates::{
    FIELD_MARAUDER, GARRICK_REDBRAND, MIREFIN_LURKER, REDBRAND_BANDIT, TIMBER_WOLF,
};

pub const MARSHAL: NpcId = NpcId::new(1);
pub const TANNER: NpcId = NpcId::new(2);
pub const FARMER: NpcId = NpcId::new(4);

/// The quest item Timber Wolves drop for *Pelts for the Tanner*.
pub const WOLF_PELT: ItemId = ItemId::new(10);

pub const TROUBLE_IN_THE_WOODS: QuestId = QuestId::new(1);
pub const PELTS_FOR_THE_TANNER: QuestId = QuestId::new(2);
pub const THE_MISSING_FARMHAND: QuestId = QuestId::new(3);
pub const MARAUDERS_IN_THE_FIELDS: QuestId = QuestId::new(4);
pub const SCOUT_THE_LAKE: QuestId = QuestId::new(5);
pub const MIREFIN_MENACE: QuestId = QuestId::new(6);
pub const INTO_REDBRAND_HOLLOW: QuestId = QuestId::new(7);
pub const GARRICK_REDBRAND_QUEST: QuestId = QuestId::new(8);
pub const RETURN_TO_GREYHAVEN: QuestId = QuestId::new(9);

/// `(id, name, text, giver, ender, prerequisite, objectives, experience,
/// copper, reward choices)`.
type QuestRow = (
    QuestId,
    &'static str,
    &'static str,
    NpcId,
    NpcId,
    Option<QuestId>,
    &'static [QuestObjective],
    u32,
    u32,
    &'static [u16],
);

const fn kill(template: crate::CreatureTemplateId, count: u8) -> QuestObjective {
    QuestObjective::Kill { template, count }
}

#[rustfmt::skip]
const QUESTS: [QuestRow; 9] = [
    (
        TROUBLE_IN_THE_WOODS, "Trouble in the Woods",
        "Timber wolves prowl the Wolfrun Trail west of the gate. Thin the pack: slay six of them.",
        MARSHAL, MARSHAL, None, &[kill(TIMBER_WOLF, 6)], 150, 10, &[],
    ),
    (
        PELTS_FOR_THE_TANNER, "Pelts for the Tanner",
        "The Marshal's wolves left good fur behind. Bring me five wolf pelts from the Wolfrun Woods.",
        TANNER, TANNER, Some(TROUBLE_IN_THE_WOODS),
        &[QuestObjective::Collect { item: WOLF_PELT, count: 5, source: TIMBER_WOLF }],
        200, 15, &[9, 6],
    ),
    (
        THE_MISSING_FARMHAND, "The Missing Farmhand",
        "My farmhand never came back from Millbrook. Follow the Millbrook Road east and ask Farmer Osric Mill.",
        TANNER, FARMER, Some(PELTS_FOR_THE_TANNER),
        &[QuestObjective::Talk { npc: FARMER }], 100, 5, &[],
    ),
    (
        MARAUDERS_IN_THE_FIELDS, "Marauders in the Fields",
        "Marauders took my fields, and my farmhand ran from them. Drive them off: defeat eight Field Marauders.",
        FARMER, FARMER, Some(THE_MISSING_FARMHAND), &[kill(FIELD_MARAUDER, 8)], 300, 25, &[8, 5],
    ),
    (
        SCOUT_THE_LAKE, "Scout the Lake",
        "Something stirs at Stillwater Lake, south-east of the outpost. Scout it, then report to the Marshal.",
        FARMER, MARSHAL, Some(MARAUDERS_IN_THE_FIELDS),
        &[QuestObjective::Explore { area: STILLWATER_LAKE }], 150, 10, &[],
    ),
    (
        MIREFIN_MENACE, "Mirefin Menace",
        "Mirefin lurkers foul the lake's shallows. Slay six of them.",
        MARSHAL, MARSHAL, Some(SCOUT_THE_LAKE), &[kill(MIREFIN_LURKER, 6)], 350, 30, &[],
    ),
    (
        INTO_REDBRAND_HOLLOW, "Into Redbrand Hollow",
        "The Redbrand bandits camp in the hollow under the northern cliffs. Break their camp: defeat ten of them.",
        MARSHAL, MARSHAL, Some(MIREFIN_MENACE),
        &[QuestObjective::Explore { area: REDBRAND_HOLLOW }, kill(REDBRAND_BANDIT, 10)],
        450, 40, &[7, 3],
    ),
    (
        GARRICK_REDBRAND_QUEST, "Garrick Redbrand",
        "Garrick Redbrand holds the mine entrance at the back of the hollow. End his raids.",
        MARSHAL, MARSHAL, Some(INTO_REDBRAND_HOLLOW), &[kill(GARRICK_REDBRAND, 1)], 600, 60, &[],
    ),
    (
        RETURN_TO_GREYHAVEN, "Return to Greyhaven",
        "The vale is safe again. Tell Tanner Hilda Brook that Greyhaven stands, and take her thanks.",
        MARSHAL, TANNER, Some(GARRICK_REDBRAND_QUEST),
        &[QuestObjective::Talk { npc: TANNER }], 300, 100, &[4, 3, 5],
    ),
];

/// The chain in quest-ID order.
#[must_use]
pub fn quests() -> Vec<Quest> {
    QUESTS
        .iter()
        .map(
            |&(
                id,
                name,
                text,
                giver,
                ender,
                prerequisite,
                objectives,
                experience,
                copper,
                choices,
            )| {
                Quest {
                    id,
                    name: name.to_owned(),
                    text: text.to_owned(),
                    giver,
                    ender,
                    prerequisite,
                    objectives: objectives.to_vec(),
                    rewards: QuestRewards {
                        experience,
                        copper,
                        choices: choices.iter().map(|&item| ItemId::new(item)).collect(),
                    },
                }
            },
        )
        .collect()
}
