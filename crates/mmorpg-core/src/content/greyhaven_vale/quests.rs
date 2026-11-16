//! The Greyhaven chain (quest catalog 3): twelve quests from the outpost's
//! three quest givers, from the Wolfrun Woods to Garrick Redbrand and back, and sixteen more
//! through the region (#820).
//!
//! Marshal Elden Greywatch (NPC 1) sends new characters against the wolves
//! and later the lake and the hollow; Tanner Hilda Brook (NPC 2) wants
//! pelts and worries about her missing farmhand; Farmer Osric Mill (NPC 4)
//! asks for help against the marauders. Each quest unlocks the next.
//!
//! The Marshal also sends players out into the region: to Gwen Barrow at Barrowdown, whose
//! chain leads on to Hob Ferris at Fenford; to Brann at Westmere Lodge, whose leads on to Edric
//! Vale's hunters in Southwood; and, once Garrick is gone, to Watchman Garth at Greyfang Watch,
//! whose chain retakes the Delving Workings from the Redbrands.
//!
//! Farmer Osric Mill sends players up the farm road to the farmland's hamlets (#859), where
//! Goodwife Ena, Miller Hanne and Shepherd Lise each need a hand.

use super::settlements::DELVING_WORKINGS;
use super::shrine::SUNKEN_SHRINE;
use super::vale::{REDBRAND_HOLLOW, STILLWATER_LAKE};
use super::wilds::FENWATER_CROSSING;
use crate::{ItemId, NpcId, Quest, QuestId, QuestObjective, QuestRewards};

use super::units::templates::{
    FIELD_MARAUDER, GARRICK_REDBRAND, GRAIN_RAT, HILL_OGRE, MIREFIN_LURKER, REDBRAND_BANDIT,
    REDBRAND_BRUTE, REDBRAND_CROSSBOWMAN, TIMBER_WOLF, YOUNG_BOAR,
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
/// A side quest beside *Mirefin Menace* (#274): dive for the relic in the Sunken Shrine.
pub const THE_SUNKEN_SHRINE: QuestId = QuestId::new(10);

/// After Garrick (#160): break the Redbrands holding the Greyfang cavern and find the sealed
/// way deeper.
pub const DEEPER_INTO_THE_HOLLOW: QuestId = QuestId::new(11);

/// After Garrick (#160): the follow-on story that sends a character after Captain Ulric Thorn in
/// the party-private Redbrand Depths (5.1.25). Its objective is the character's durable Thorn
/// victory, so a victory won before Garrick's quest still counts (5.1.37).
pub const THE_CAPTAIN_BELOW: QuestId = QuestId::new(12);

/// The region's quests (#820).
pub const WORD_FROM_THE_DOWNS: QuestId = QuestId::new(13);
pub const RAIDERS_ON_THE_DOWNS: QuestId = QuestId::new(14);
pub const GRAIN_FOR_THE_BARROW_REST: QuestId = QuestId::new(15);
pub const THE_BARROW_RING: QuestId = QuestId::new(16);
pub const DOWN_TO_THE_FORD: QuestId = QuestId::new(17);
pub const TUSKS_AND_TALLOW: QuestId = QuestId::new(18);
pub const THE_FENWATER_RAIDERS: QuestId = QuestId::new(19);
pub const THE_LODGE_ON_THE_TARN: QuestId = QuestId::new(20);
pub const SCALES_OF_WESTMERE: QuestId = QuestId::new(21);
pub const WOLVES_AT_THE_WATER: QuestId = QuestId::new(22);
pub const THE_HUNTERS_CAMP: QuestId = QuestId::new(23);
pub const SOUTHWOOD_HUNT: QuestId = QuestId::new(24);
pub const THE_GREYFANG_WATCH: QuestId = QuestId::new(25);
pub const ORDERS_ON_THE_ROAD: QuestId = QuestId::new(26);
pub const THE_DELVING_WORKINGS: QuestId = QuestId::new(27);
pub const OGRES_OF_THE_HEIGHTS: QuestId = QuestId::new(28);

/// The farmland hamlets' quests (#859); 32 is the quest log's last.
pub const THE_HAMLETS: QuestId = QuestId::new(29);
pub const RATS_IN_THE_GRANARY: QuestId = QuestId::new(30);
pub const BOARS_IN_THE_BARLEY: QuestId = QuestId::new(31);
pub const THE_SHEPHERDS_WOLVES: QuestId = QuestId::new(32);

/// The region's quest givers (#820): Barrowdown's alewife, Fenford's innkeeper, Westmere
/// Lodge's fisher, the hunters' huntsman and Greyfang Watch's watchman; and Old Maud, whom a
/// quest sends players to talk to.
pub const ALEWIFE: NpcId = NpcId::new(22);
pub const OLD_MAUD: NpcId = NpcId::new(24);
pub const INNKEEPER: NpcId = NpcId::new(27);
pub const FISHER: NpcId = NpcId::new(31);
pub const HUNTSMAN: NpcId = NpcId::new(32);
pub const WATCHMAN: NpcId = NpcId::new(34);
/// The hamlets' quest givers (#859): the western hamlet's goodwife, the southern hamlet's miller
/// and the eastern hamlet's shepherd.
pub const GOODWIFE: NpcId = NpcId::new(36);
pub const SHEPHERD: NpcId = NpcId::new(39);
pub const MILLER: NpcId = NpcId::new(40);

/// The region's quest items (#820).
pub const BOAR_TUSK: ItemId = ItemId::new(19);
pub const STOLEN_GRAIN_SACK: ItemId = ItemId::new(20);
pub const LURKER_SCALE: ItemId = ItemId::new(21);
pub const REDBRAND_ORDERS: ItemId = ItemId::new(22);

/// The relic on the Sunken Shrine's altar (#274).
pub const SUNKEN_RELIC: ItemId = ItemId::new(11);

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
const QUESTS: [QuestRow; 32] = [
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
    (
        THE_SUNKEN_SHRINE, "The Sunken Shrine",
        "Old stones lie at the bottom of Stillwater Lake, and a flooded cave runs east from them. Dive down, bring back the relic from its altar, and mind your breath.",
        MARSHAL, MARSHAL, Some(SCOUT_THE_LAKE),
        &[QuestObjective::Retrieve { item: SUNKEN_RELIC, area: SUNKEN_SHRINE }], 300, 25, &[],
    ),
    (
        DEEPER_INTO_THE_HOLLOW, "Deeper into the Hollow",
        "Garrick is gone, but his men still hold the Greyfang cave east of the hollow, and shield-bearing brutes guard a sealed way deeper. Break two Redbrand Brutes and see what they guard.",
        MARSHAL, MARSHAL, Some(RETURN_TO_GREYHAVEN),
        &[kill(REDBRAND_BRUTE, 2)], 700, 70, &[12, 13, 14],
    ),
    (
        THE_CAPTAIN_BELOW, "The Captain Below",
        "Garrick's ledger names the man he answered to: Captain Ulric Thorn, who rules the Redbrands from the depths under the Greyfang cave. Take friends you trust and end him, or tell me it is already done.",
        MARSHAL, MARSHAL, Some(GARRICK_REDBRAND_QUEST),
        &[QuestObjective::DefeatThorn], 900, 120, &[18],
    ),
    // The region's chains (#820): the Downs and the ford, the tarn and Southwood, the Greyfang
    // Road and the Delving Workings.
    (
        WORD_FROM_THE_DOWNS, "Word from the Downs",
        "Barrowdown on the King's Road Downs has sent no grain this season. Take the Fen Road and the Fenwater Link south and ask Gwen Barrow at the Barrow Rest what is wrong.",
        MARSHAL, ALEWIFE, Some(TROUBLE_IN_THE_WOODS),
        &[QuestObjective::Talk { npc: ALEWIFE }], 120, 5, &[],
    ),
    (
        RAIDERS_ON_THE_DOWNS, "Raiders on the Downs",
        "Marauders camp in the hollow south of the knoll and raid our carts on the King's Road. Drive them off: defeat six Field Marauders.",
        ALEWIFE, ALEWIFE, Some(WORD_FROM_THE_DOWNS), &[kill(FIELD_MARAUDER, 6)], 350, 30, &[],
    ),
    (
        GRAIN_FOR_THE_BARROW_REST, "Grain for the Barrow Rest",
        "They carried off our grain in sacks. Bring four of them back from the marauders, and the Barrow Rest will brew again.",
        ALEWIFE, ALEWIFE, Some(RAIDERS_ON_THE_DOWNS),
        &[QuestObjective::Collect { item: STOLEN_GRAIN_SACK, count: 4, source: FIELD_MARAUDER }],
        300, 20, &[7, 8],
    ),
    (
        THE_BARROW_RING, "The Barrow Ring",
        "Old Maud keeps the barrow ring west of the hamlet and swears the stones are restless. Humour an old woman: go and hear her out.",
        ALEWIFE, ALEWIFE, Some(RAIDERS_ON_THE_DOWNS),
        &[QuestObjective::Talk { npc: OLD_MAUD }], 150, 10, &[],
    ),
    (
        DOWN_TO_THE_FORD, "Down to the Ford",
        "Hob Ferris at the Ford Inn buys our ale and sells us news. Follow the Fen Road east to Fenford and tell him the Barrow Rest is brewing again.",
        ALEWIFE, INNKEEPER, Some(GRAIN_FOR_THE_BARROW_REST),
        &[QuestObjective::Talk { npc: INNKEEPER }], 150, 10, &[],
    ),
    (
        TUSKS_AND_TALLOW, "Tusks and Tallow",
        "The boars by the Fenwater grow fat and bold. Bring me five tusks for the carver, and the tallow pays your supper.",
        INNKEEPER, INNKEEPER, Some(DOWN_TO_THE_FORD),
        &[QuestObjective::Collect { item: BOAR_TUSK, count: 5, source: YOUNG_BOAR }],
        300, 25, &[],
    ),
    (
        THE_FENWATER_RAIDERS, "The Fenwater Raiders",
        "Marauders watch the ford from the meadows across the Fenwater. Walk the crossing and break them: defeat four.",
        INNKEEPER, INNKEEPER, Some(DOWN_TO_THE_FORD),
        &[QuestObjective::Explore { area: FENWATER_CROSSING }, kill(FIELD_MARAUDER, 4)],
        350, 30, &[5, 3],
    ),
    (
        THE_LODGE_ON_THE_TARN, "The Lodge on the Tarn",
        "The fisher at Westmere Lodge sends word of trouble on the tarn. Follow the Westmere Road west from the Wolfrun Climb and find Brann.",
        MARSHAL, FISHER, Some(SCOUT_THE_LAKE),
        &[QuestObjective::Talk { npc: FISHER }], 150, 10, &[],
    ),
    (
        SCALES_OF_WESTMERE, "Scales of Westmere",
        "Lurkers crept up the beck into the tarn and foul my nets. Bring me four of their scales so the lodge believes it.",
        FISHER, FISHER, Some(THE_LODGE_ON_THE_TARN),
        &[QuestObjective::Collect { item: LURKER_SCALE, count: 4, source: MIREFIN_LURKER }],
        350, 25, &[6, 9],
    ),
    (
        WOLVES_AT_THE_WATER, "Wolves at the Water",
        "Wolf packs come down through Westmere Woods to drink at dusk. Thin them: slay eight timber wolves.",
        FISHER, FISHER, Some(THE_LODGE_ON_THE_TARN), &[kill(TIMBER_WOLF, 8)], 300, 20, &[],
    ),
    (
        THE_HUNTERS_CAMP, "The Hunters' Camp",
        "Edric Vale's hunters camp on the Southwood Trail. He pays for good hands. Take the Westwood Track south through the woods and find him.",
        FISHER, HUNTSMAN, Some(WOLVES_AT_THE_WATER),
        &[QuestObjective::Talk { npc: HUNTSMAN }], 150, 10, &[],
    ),
    (
        SOUTHWOOD_HUNT, "Southwood Hunt",
        "The camp needs meat and the trails need clearing. Bring down six boars and four wolves in Southwood.",
        HUNTSMAN, HUNTSMAN, Some(THE_HUNTERS_CAMP),
        &[kill(YOUNG_BOAR, 6), kill(TIMBER_WOLF, 4)], 400, 30, &[5, 3, 4],
    ),
    (
        THE_GREYFANG_WATCH, "The Greyfang Watch",
        "With Garrick gone the Redbrands still ride the Greyfang Road. Watchman Garth holds the tower below the mountains; go up through the Highland Pass and report to him.",
        MARSHAL, WATCHMAN, Some(GARRICK_REDBRAND_QUEST),
        &[QuestObjective::Talk { npc: WATCHMAN }], 200, 15, &[],
    ),
    (
        ORDERS_ON_THE_ROAD, "Orders on the Road",
        "Their crossbowmen carry written orders. Take three from them on the Greyfang Road, and we will know where they are bound.",
        WATCHMAN, WATCHMAN, Some(THE_GREYFANG_WATCH),
        &[QuestObjective::Collect { item: REDBRAND_ORDERS, count: 3, source: REDBRAND_CROSSBOWMAN }],
        500, 40, &[],
    ),
    (
        THE_DELVING_WORKINGS, "The Delving Workings",
        "The orders name the Delving Workings east of the lookout: the Redbrands have taken the miners' camp. Go there by the Delving Track and break them: defeat six bandits.",
        WATCHMAN, WATCHMAN, Some(ORDERS_ON_THE_ROAD),
        &[QuestObjective::Explore { area: DELVING_WORKINGS }, kill(REDBRAND_BANDIT, 6)],
        650, 60, &[12, 13, 14],
    ),
    (
        OGRES_OF_THE_HEIGHTS, "Ogres of the Heights",
        "Hill ogres roam the Delving Heights and the miners will not go back while they do. Fell two of them.",
        WATCHMAN, WATCHMAN, Some(THE_DELVING_WORKINGS), &[kill(HILL_OGRE, 2)], 800, 80, &[],
    ),
    // The farmland hamlets (#859).
    (
        THE_HAMLETS, "The Hamlets",
        "With the marauders gone the hamlets up the farm road can sow again, but they are short of hands. Take the road north past the fields and find Goodwife Ena in the western hamlet.",
        FARMER, GOODWIFE, Some(MARAUDERS_IN_THE_FIELDS),
        &[QuestObjective::Talk { npc: GOODWIFE }], 120, 5, &[],
    ),
    (
        RATS_IN_THE_GRANARY, "Rats in the Granary",
        "Rats have got into the barn and the haystack, and what they leave the damp takes. Kill eight of them before they eat the seed corn.",
        GOODWIFE, GOODWIFE, Some(THE_HAMLETS), &[kill(GRAIN_RAT, 8)], 250, 15, &[],
    ),
    (
        BOARS_IN_THE_BARLEY, "Boars in the Barley",
        "Miller Hanne down at the southern hamlet says boars root up the barley west of her mill every night. Bring down five of them, then tell her the fields are safe.",
        GOODWIFE, MILLER, Some(RATS_IN_THE_GRANARY), &[kill(YOUNG_BOAR, 5)], 300, 20, &[8, 5],
    ),
    (
        THE_SHEPHERDS_WOLVES, "The Shepherd's Wolves",
        "A wolf pack hunts east of the eastern hamlet's pastures, and Shepherd Lise has lost two ewes. Slay five wolves and let her know her flock can graze again.",
        MILLER, SHEPHERD, Some(BOARS_IN_THE_BARLEY), &[kill(TIMBER_WOLF, 5)], 350, 25, &[6, 9],
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
