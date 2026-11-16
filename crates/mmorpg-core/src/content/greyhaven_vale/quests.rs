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
//!
//! The quests themselves are content (#862): `content/greyhaven/quests.json` defines every
//! quest's text, giver, ender, prerequisite, objectives and rewards by the stable IDs below and
//! the zone's NPC, creature, item and area IDs. The zone validates them when it binds them.

use serde::Deserialize;

use crate::{
    AreaId, CreatureTemplateId, ItemId, NpcId, Quest, QuestId, QuestObjective, QuestRewards,
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

/// The farmland hamlets' quests (#859).
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

const SOURCE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../content/greyhaven/quests.json"
));

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    version: u32,
    quests: Vec<QuestSource>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QuestSource {
    id: u32,
    name: String,
    text: String,
    giver: u32,
    ender: u32,
    #[serde(default)]
    prerequisite: Option<u32>,
    objectives: Vec<ObjectiveSource>,
    rewards: RewardsSource,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
enum ObjectiveSource {
    Kill { template: u32, count: u8 },
    Collect { item: u32, count: u8, source: u32 },
    Talk { npc: u32 },
    Explore { area: u32 },
    Retrieve { item: u32, area: u32 },
    DefeatThorn,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RewardsSource {
    experience: u32,
    copper: u32,
    choices: Vec<u32>,
}

impl From<ObjectiveSource> for QuestObjective {
    fn from(source: ObjectiveSource) -> Self {
        match source {
            ObjectiveSource::Kill { template, count } => Self::Kill {
                template: CreatureTemplateId::new(template),
                count,
            },
            ObjectiveSource::Collect {
                item,
                count,
                source,
            } => Self::Collect {
                item: ItemId::new(item),
                count,
                source: CreatureTemplateId::new(source),
            },
            ObjectiveSource::Talk { npc } => Self::Talk {
                npc: NpcId::new(npc),
            },
            ObjectiveSource::Explore { area } => Self::Explore {
                area: AreaId::new(area),
            },
            ObjectiveSource::Retrieve { item, area } => Self::Retrieve {
                item: ItemId::new(item),
                area: AreaId::new(area),
            },
            ObjectiveSource::DefeatThorn => Self::DefeatThorn,
        }
    }
}

fn parse(source: &str) -> Result<Vec<Quest>, String> {
    let source: Source = serde_json::from_str(source).map_err(|error| error.to_string())?;
    if source.version != 1 {
        return Err(format!("unknown quests version {}", source.version));
    }
    Ok(source
        .quests
        .into_iter()
        .map(|quest| Quest {
            id: QuestId::new(quest.id),
            name: quest.name,
            text: quest.text,
            giver: NpcId::new(quest.giver),
            ender: NpcId::new(quest.ender),
            prerequisite: quest.prerequisite.map(QuestId::new),
            objectives: quest.objectives.into_iter().map(Into::into).collect(),
            rewards: QuestRewards {
                experience: quest.rewards.experience,
                copper: quest.rewards.copper,
                choices: quest.rewards.choices.into_iter().map(ItemId::new).collect(),
            },
        })
        .collect())
}

/// The chain as `content/greyhaven/quests.json` lists it; the zone orders and validates it.
///
/// # Panics
///
/// When the committed quest file does not parse.
#[must_use]
pub fn quests() -> Vec<Quest> {
    parse(SOURCE).unwrap_or_else(|error| panic!("content/greyhaven/quests.json: {error}"))
}
