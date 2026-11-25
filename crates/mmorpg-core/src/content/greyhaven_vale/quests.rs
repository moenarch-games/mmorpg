//! The Greyhaven chain (quest catalog 3): twelve quests from the outpost's
//! three quest givers, from the Wolfrun Woods to Garrick Redbrand and back, and sixteen more
//! through the region (#820).
//!
//! Marshal Elden Greywatch (NPC 1) sends new characters up the Wolfrun Trail
//! after an abandoned supply cart (#984) and later to the lake and the hollow;
//! Tanner Hilda Brook (NPC 2) takes back her cart's sacks, worries about her
//! missing farmhand and, on the side, wants pelts; Farmer Osric Mill (NPC 4)
//! asks for help against the marauders. Each quest unlocks the next.
//!
//! *The Wolfrun Cart* keeps the first quest's stable ID: characters who
//! turned in the old *Trouble in the Woods* still have it done, and saved
//! progress past its new counts is fitted on admission.
//!
//! The Marshal also sends players out into the region: to Gwen Barrow at Barrowdown, whose
//! chain leads on to Hob Ferris at Fenford; to Brann at Westmere Lodge, whose leads on to Edric
//! Vale's hunters in Southwood; and, once Garrick is gone, to Watchman Garth at Greyfang Watch,
//! whose chain retakes the Delving Workings from the Redbrands.
//!
//! Farmer Osric Mill sends players up the farm road to the farmland's hamlets (#859), where
//! Goodwife Ena, Miller Hanne and Shepherd Lise each need a hand.
//!
//! Back in the Core Vale (#1019), Hilda has a player read the signs her cart left: the cut sacks,
//! the bait and the Redbrand marker, the bait's trail to a courier's cache, and her own ledger
//! against Millbrook's delivery tally. Their inspect objectives name authored evidence spots.
//!
//! The quests themselves are content (#862): `content/greyhaven/quests.json` defines every
//! quest's text, giver, ender, prerequisite, objectives and rewards by the stable IDs below and
//! the zone's NPC, creature, item and area IDs. The zone validates them when it binds them.

use serde::Deserialize;

use crate::{
    AidChore, AreaId, Confrontation, CreatureId, CreatureTemplateId, DecisionOption, EvidenceId,
    EvidenceSpot, ItemId, LoadKind, LoadSpot, LoadSpotId, NpcAttitude, NpcDeparture, NpcId,
    PieceId, PlayerClass, Quest, QuestId, QuestObjective, QuestRewards, QuestSupply,
};
use strum::VariantArray;

pub const MARSHAL: NpcId = NpcId::new(1);
pub const TANNER: NpcId = NpcId::new(2);
pub const FARMER: NpcId = NpcId::new(4);

/// The quest item Timber Wolves drop for *Pelts for the Tanner*.
pub const WOLF_PELT: ItemId = ItemId::new(10);

/// The quest item Timber Wolves drop for *The Wolfrun Cart* (#984).
pub const SUPPLY_SACK: ItemId = ItemId::new(31);

/// The opening (#984): find the abandoned cart in the Wolfrun Woods and take back the sacks the
/// pack dragged off for the Tanner, who then gives the lead to Millbrook.
pub const THE_WOLFRUN_CART: QuestId = QuestId::new(1);
/// An optional side quest of the Tanner's since #984.
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

/// The Core Vale's evidence quests (#1019): Hilda sends a player back to the Wolfrun cart to
/// read its signs, after the bait's trail to a courier's cache, and through her own ledger to
/// the shipment Millbrook never received. ID 35 is the planned wounded guard's, which waits
/// for an escort rule.
pub const CUT_SACKS_RED_RAGS: QuestId = QuestId::new(33);
pub const THE_BAIT_TRAIL: QuestId = QuestId::new(34);
pub const THE_TANNERS_LEDGER: QuestId = QuestId::new(36);

/// The evidence those quests name (#1019): the Wolfrun cart scene's cut sacks, bait and marker
/// (#984), the bait trail's scraps, second rag and the courier's cache, Hilda's ledger and the
/// Millbrook delivery tally.
pub const CUT_SACKS: EvidenceId = EvidenceId::new(1);
pub const BAIT_RACK: EvidenceId = EvidenceId::new(2);
pub const REDBRAND_MARKER: EvidenceId = EvidenceId::new(3);
pub const BAIT_SCRAPS: EvidenceId = EvidenceId::new(4);
pub const SECOND_RAG: EvidenceId = EvidenceId::new(5);
pub const COURIERS_CACHE: EvidenceId = EvidenceId::new(6);
pub const HILDAS_LEDGER: EvidenceId = EvidenceId::new(7);
pub const DELIVERY_TALLY: EvidenceId = EvidenceId::new(8);

/// The Core Vale's tactical and social quests (#1019, slice 2): a wounded watchman's rescue, the
/// smith's loaned trial arms, the Hollow's boulder trap and watch post, a distrustful resident's
/// interview, the courier who turns on whoever shows him his pass, and a farmhand's medicine.
pub const A_GUARDS_LAST_WATCH: QuestId = QuestId::new(35);
pub const THE_SMITHS_TRIAL: QuestId = QuestId::new(37);
pub const STONE_ON_THE_ROAD: QuestId = QuestId::new(38);
pub const BREAKING_THE_WATCH_POST: QuestId = QuestId::new(39);
pub const A_QUIET_INTRODUCTION: QuestId = QuestId::new(40);
pub const THE_MAN_AT_THE_GATE: QuestId = QuestId::new(41);
pub const THE_MILLERS_MEDICINE: QuestId = QuestId::new(42);

/// The Core Vale's first delivery (#1077, #1019 slot 43): the last sack still on the Wolfrun
/// cart, carried in both arms to Farmer Osric Mill at Millbrook.
pub const THE_LAST_SACK: QuestId = QuestId::new(43);
/// The sack on the Wolfrun cart (#1077), a load source.
pub const CART_SACK: LoadSpotId = LoadSpotId::new(1);
/// Farmer Osric Mill, who takes it at Millbrook.
pub const OSRIC_MILL: NpcId = NpcId::new(4);

/// Millbrook's defence (#1019, slice 3a): Osric has the player raise two barricades with timber
/// from the farm's pile, hold the farm through a defence with his farmfolk standing, and help
/// the farm recover afterwards.
pub const BARRICADES_AT_MILLBROOK: QuestId = QuestId::new(44);
pub const HOLD_UNTIL_DAWN: QuestId = QuestId::new(45);
pub const AFTER_THE_RAID: QuestId = QuestId::new(46);

/// The farmland's delivery, repair and village stories (#1019, slice 4a): Millbrook's choked
/// sluice, a sack for Miller Hanne, Shepherd Lise's dispute with Old Bertil settled with evidence
/// or copper, her fold's hurdles, Bertil's hidden locket and bread for the tower's watch
/// ([`super::hamlet_stories`]). The flock's two, 53 and 55, are [`super::flock`]'s.
pub const THE_BROKEN_SLUICE: QuestId = QuestId::new(51);
pub const A_SACK_FOR_HANNE: QuestId = QuestId::new(52);
pub const A_SHEPHERDS_BARGAIN: QuestId = QuestId::new(54);
pub const A_FENCE_IN_NEED: QuestId = QuestId::new(56);
pub const BERTILS_HEIRLOOM: QuestId = QuestId::new(57);
pub const BREAD_FOR_THE_WATCH: QuestId = QuestId::new(58);
/// Old Bertil's locket, hidden under a stone at the old tower (#1019).
pub const BERTILS_LOCKET: ItemId = ItemId::new(43);

/// The flock (#1019, slice 4b): Lise's missing ewe tracked and driven home, and a night watch
/// over her fold with bait and a snare laid at its hedges ([`super::flock`]).
/// The missing family and the granary (#1019, slice 4c): Goodwife Ena has a player follow the
/// Tollys' trail from their empty cottage, lead them home, muster defenders across the farmland
/// and hold her granary through a raid ([`super::granary`]).
pub const THE_EMPTY_COTTAGE: QuestId = QuestId::new(59);
pub const BRING_THEM_HOME: QuestId = QuestId::new(60);
pub const THE_HARVEST_MUSTER: QuestId = QuestId::new(61);
pub const THE_GRANARY_STAND: QuestId = QuestId::new(62);
/// The Tollys' trail (#1019): their emptied chest by the cottage door, a child's shoe by the west
/// fields' lane and their handcart's ruts by the hollow where they hide.
pub const TOLLYS_CHEST: EvidenceId = EvidenceId::new(18);
pub const CHILDS_SHOE: EvidenceId = EvidenceId::new(19);
pub const HANDCART_RUTS: EvidenceId = EvidenceId::new(20);
/// The muster's recruits (#1019): Ploughman Garr of the western hamlet, Farmhand Jory of the
/// southern one and Watchman Dunstan at the tower, who distrusts Necromancers unless the watch's
/// bread came from them or a trusted party mate speaks for them.
pub const PLOUGHMAN_GARR: NpcId = NpcId::new(37);
pub const WATCHMAN_DUNSTAN: NpcId = NpcId::new(49);

/// Barrowdown's ring and barrow (#1019, slice 5a): Old Maud's warning about her stones, the
/// ring's measure worked in an inscription's order, the grave robber talked round or fought for
/// the barrow's torc, and what Maud keeps from anyone showing a staff
/// ([`super::barrowdown`]).
pub const MAUDS_WARNING: QuestId = QuestId::new(63);
pub const THE_RINGS_MEASURE: QuestId = QuestId::new(64);
pub const THE_GRAVE_ROBBER: QuestId = QuestId::new(65);
pub const A_MATTER_OF_BELIEF: QuestId = QuestId::new(66);
/// The barrow's torc, handed over or taken from Cobb's body.
pub const BARROW_TORC: ItemId = ItemId::new(46);
/// Their evidence: the spade marks and the scattered offering at two of the ring's stones, the
/// lintel's inscription at its heart, and Cobb's turned earth west of the ring.
pub const SPADE_MARKS: EvidenceId = EvidenceId::new(21);
pub const SCATTERED_OFFERING: EvidenceId = EvidenceId::new(22);
pub const RING_INSCRIPTION: EvidenceId = EvidenceId::new(23);
pub const TURNED_EARTH: EvidenceId = EvidenceId::new(24);

/// The debt and the Downs road (#1019, slice 5b): Gwen's unpaid round settled between two
/// villagers, the Downs signal lit and the supply patrol watched from the lookout, the patrol
/// stopped before its camp for its orders, and the peace Maud sends Gwen once the torc and the
/// debt are settled ([`super::downs`]).
pub const THE_UNPAID_ALE: QuestId = QuestId::new(67);
pub const THE_ROADSIDE_SIGNAL: QuestId = QuestId::new(68);
pub const THE_DOWNS_AMBUSH: QuestId = QuestId::new(69);
pub const PEACE_AT_THE_REST: QuestId = QuestId::new(70);
/// The two villagers Gwen's slate names.
pub const SHEPHERD_CORIN: NpcId = NpcId::new(23);
pub const FARMER_TOBIN: NpcId = NpcId::new(25);
/// The signal's tinder, a supply, and the patrol's orders, from the packbearer's body.
pub const SIGNAL_TINDER: ItemId = ItemId::new(47);
pub const ORDER_POUCH: ItemId = ItemId::new(48);
/// The cold beacon by the lookout.
pub const DOWNS_BEACON: EvidenceId = EvidenceId::new(25);

/// The village and the river (#1019, slice 6a): Ferryman Osk's lost rope block found on the
/// ford's bed, the ford's three stakes surveyed for him, Miller Wynne's new brake wheel carried
/// from the cart and fitted, spring water carried from the alders to Hob Ferris, and a smuggler's
/// shipment found, its owner questioned and its token handed to the Watch or left for Osk
/// ([`super::fenford`]). 73, 76, 78 and 79 are the crossings under threat (slice 6b).
pub const THE_FERRYMANS_ROPE: QuestId = QuestId::new(71);
pub const THE_FORDS_RECKONING: QuestId = QuestId::new(72);
pub const THE_MISSING_MILL_WHEEL: QuestId = QuestId::new(74);
pub const WATER_FOR_THE_REST: QuestId = QuestId::new(75);
pub const THE_SMUGGLERS_TOKEN: QuestId = QuestId::new(77);
/// The rope's iron block from the ford's bed and the token from the hidden shipment, both worth
/// nothing to a vendor.
pub const ROPE_BLOCK: ItemId = ItemId::new(49);
pub const SMUGGLERS_TOKEN: ItemId = ItemId::new(50);
/// Their evidence: the frayed rope at Osk's post, the block's hiding place on the ford's bed, the
/// ford's north, middle and south stakes, and the shipment's hiding place upstream.
pub const FRAYED_ROPE: EvidenceId = EvidenceId::new(26);
pub const FORD_BED: EvidenceId = EvidenceId::new(27);
pub const FORD_STAKES: [EvidenceId; 3] = [
    EvidenceId::new(28),
    EvidenceId::new(29),
    EvidenceId::new(30),
];
pub const HIDDEN_SHIPMENT: EvidenceId = EvidenceId::new(31);
/// The ferryman and the miller of Fenford.
pub const FERRYMAN_OSK: NpcId = NpcId::new(29);
pub const MILLER_WYNNE: NpcId = NpcId::new(28);

pub const THE_MISSING_EWE: QuestId = QuestId::new(53);
pub const THE_HEDGES_AT_DUSK: QuestId = QuestId::new(55);
/// Their supplies: the bait and the snare, worth nothing to a vendor.
pub const TAINTED_MUTTON: ItemId = ItemId::new(44);
pub const WOLF_SNARE: ItemId = ItemId::new(45);

/// Their people: the wounded watchman on the Wolfrun terrace, the courier outside the east gate,
/// the distrustful resident, the herbalist, the farmhand and the smith.
pub const TAM_HOLLIS: NpcId = NpcId::new(44);
pub const COURIER: NpcId = NpcId::new(45);
pub const DAVIN_MOSS: NpcId = NpcId::new(13);
pub const HERBALIST: NpcId = NpcId::new(16);
pub const FARMHAND_JORY: NpcId = NpcId::new(41);
pub const SMITH: NpcId = NpcId::new(43);

/// Their supplies: the smith's three trial arms, the bandage, the draught and the toll pass.
pub const TRIAL_BLADE: ItemId = ItemId::new(36);
pub const TRIAL_STAFF: ItemId = ItemId::new(37);
pub const TRIAL_MACE: ItemId = ItemId::new(38);
pub const CLEAN_BANDAGE: ItemId = ItemId::new(39);
pub const FEVER_DRAUGHT: ItemId = ItemId::new(40);
pub const FORGED_TOLL_PASS: ItemId = ItemId::new(41);

/// The courier once unmasked: a reserved spawn a metre south of him, standing only while he is
/// hostile (#1019).
pub const COURIER_FOE: CreatureId = CreatureId::new(2_930);

/// The confrontation foes' spawns (#1019): reserved off the world until their NPC turns.
pub(super) fn spawns() -> Vec<crate::content::CreatureSpawn> {
    vec![crate::content::CreatureSpawn {
        id: COURIER_FOE,
        template: super::units::templates::REDBRAND_COURIER,
        position: [3_650, 2_200],
        facing: 49_152,
        wander_radius: 0,
        levels: None,
    }]
}

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
    /// The evidence the quests' inspect objectives name (#1019).
    #[serde(default)]
    evidence: Vec<EvidenceSource>,
    /// The load sources the quests' deliveries name (#1077).
    #[serde(default)]
    loads: Vec<LoadSource>,
    /// How NPCs regard interviewers (#1019).
    #[serde(default)]
    attitudes: Vec<AttitudeSource>,
    /// NPCs who turn hostile when shown evidence (#1019).
    #[serde(default)]
    confrontations: Vec<ConfrontationSource>,
    /// NPCs who leave a character's world after its quest decision (#1019).
    #[serde(default)]
    departures: Vec<DepartureSource>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DepartureSource {
    npc: u32,
    quest: u32,
    outcome: u8,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AttitudeSource {
    npc: u32,
    /// Class names, as in `warden`.
    distrusts: Vec<String>,
    /// Distrusts a staff shown in the main hand (#1019).
    #[serde(default)]
    arcane: bool,
    sponsor: bool,
    #[serde(default)]
    recommendation: Option<u32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfrontationSource {
    npc: u32,
    /// Absent for an NPC only a decision's fight option turns (#1019).
    #[serde(default)]
    evidence: Option<u32>,
    foe: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SupplySource {
    item: u32,
    /// Class names; every class when absent.
    #[serde(default)]
    classes: Vec<String>,
}

fn classes(names: Vec<String>) -> Result<Vec<PlayerClass>, String> {
    names
        .into_iter()
        .map(|name| {
            PlayerClass::VARIANTS
                .iter()
                .copied()
                .find(|class| class.name() == name)
                .ok_or_else(|| format!("unknown class {name}"))
        })
        .collect()
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
    /// What the giver hands over on accepting (#1019).
    #[serde(default)]
    supplies: Vec<SupplySource>,
    /// Further quests to turn in first (#1019).
    #[serde(default)]
    requires: Vec<u32>,
    /// A decide objective's options (#1019): a label, or a label and the copper it costs.
    #[serde(default)]
    decision: Vec<OptionSource>,
}

/// A decision option (#1019): free as a plain label, or with its copper, its turn-in reward, the
/// item the NPC hands over or a fight.
#[derive(Deserialize)]
#[serde(untagged)]
enum OptionSource {
    Free(String),
    Full(FullOptionSource),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FullOptionSource {
    label: String,
    #[serde(default)]
    copper: u32,
    #[serde(default)]
    reward: u32,
    #[serde(default)]
    hands: Option<u32>,
    #[serde(default)]
    fight: bool,
}

impl From<OptionSource> for DecisionOption {
    fn from(source: OptionSource) -> Self {
        match source {
            OptionSource::Free(label) => Self::free(label),
            OptionSource::Full(option) => Self {
                label: option.label,
                copper: option.copper,
                reward: option.reward,
                hands: option.hands.map(ItemId::new),
                fight: option.fight,
            },
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
enum ObjectiveSource {
    Kill {
        template: u32,
        count: u8,
    },
    Collect {
        item: u32,
        count: u8,
        source: u32,
    },
    Talk {
        npc: u32,
    },
    Explore {
        area: u32,
    },
    Retrieve {
        item: u32,
        area: u32,
    },
    DefeatThorn,
    Inspect {
        spot: u32,
    },
    UseItem {
        item: u32,
        npc: u32,
    },
    Escort {
        npc: u32,
        area: u32,
    },
    Interview {
        npc: u32,
    },
    DefeatWithLoan {
        template: u32,
        count: u8,
    },
    Crush {
        template: u32,
        count: u8,
    },
    Demolish {
        piece: u32,
    },
    DeliverLoad {
        source: u32,
        npc: u32,
    },
    Build {
        piece: u32,
    },
    Defend {
        npc: u32,
    },
    Aid {
        chore: ChoreSource,
        count: u8,
    },
    /// A hidden find (#1019) is a transcription of an item from a hidden spot.
    #[serde(alias = "find")]
    Transcribe {
        spot: u32,
        item: u32,
    },
    Unseal {
        piece: u32,
    },
    Decide {
        npc: u32,
    },
    Herd {
        npc: u32,
    },
    Watch {
        npc: u32,
    },
    Lay {
        spot: u32,
        item: u32,
    },
    EscortTo {
        npc: u32,
        to: u32,
    },
    Recruit {
        npc: u32,
    },
    WorkInOrder {
        sequence: u32,
        count: u8,
    },
    Sight {
        npc: u32,
    },
    Ambush {
        npc: u32,
    },
}

/// An aid objective's chore (#1019).
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum ChoreSource {
    Mend,
    Tend,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoadSource {
    id: u32,
    name: String,
    kind: String,
    at: [i32; 3],
    reach: i32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EvidenceSource {
    id: u32,
    name: String,
    at: [i32; 3],
    reach: i32,
    /// A hidden spot's landmark directions (#1019).
    #[serde(default)]
    directions: Option<String>,
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
            ObjectiveSource::Inspect { spot } => Self::Inspect {
                spot: EvidenceId::new(spot),
            },
            ObjectiveSource::UseItem { item, npc } => Self::UseItem {
                item: ItemId::new(item),
                npc: NpcId::new(npc),
            },
            ObjectiveSource::Escort { npc, area } => Self::Escort {
                npc: NpcId::new(npc),
                area: AreaId::new(area),
            },
            ObjectiveSource::Interview { npc } => Self::Interview {
                npc: NpcId::new(npc),
            },
            ObjectiveSource::DefeatWithLoan { template, count } => Self::DefeatWithLoan {
                template: CreatureTemplateId::new(template),
                count,
            },
            ObjectiveSource::Crush { template, count } => Self::Crush {
                template: CreatureTemplateId::new(template),
                count,
            },
            ObjectiveSource::Demolish { piece } => Self::Demolish {
                piece: PieceId::new(piece),
            },
            ObjectiveSource::DeliverLoad { source, npc } => Self::DeliverLoad {
                source: LoadSpotId::new(source),
                npc: NpcId::new(npc),
            },
            ObjectiveSource::Build { piece } => Self::Build {
                piece: PieceId::new(piece),
            },
            ObjectiveSource::Defend { npc } => Self::Defend {
                npc: NpcId::new(npc),
            },
            ObjectiveSource::Aid { chore, count } => Self::Aid {
                chore: match chore {
                    ChoreSource::Mend => AidChore::Mend,
                    ChoreSource::Tend => AidChore::Tend,
                },
                count,
            },
            ObjectiveSource::Transcribe { spot, item } => Self::Transcribe {
                spot: EvidenceId::new(spot),
                item: ItemId::new(item),
            },
            ObjectiveSource::Unseal { piece } => Self::Unseal {
                piece: PieceId::new(piece),
            },
            ObjectiveSource::Decide { npc } => Self::Decide {
                npc: NpcId::new(npc),
            },
            // The flock (#1019).
            ObjectiveSource::Herd { npc } => Self::Herd {
                npc: NpcId::new(npc),
            },
            ObjectiveSource::Watch { npc } => Self::Watch {
                npc: NpcId::new(npc),
            },
            ObjectiveSource::Lay { spot, item } => Self::Lay {
                spot: EvidenceId::new(spot),
                item: ItemId::new(item),
            },
            // The missing family and the muster (#1019).
            ObjectiveSource::EscortTo { npc, to } => Self::EscortTo {
                npc: NpcId::new(npc),
                to: NpcId::new(to),
            },
            ObjectiveSource::Recruit { npc } => Self::Recruit {
                npc: NpcId::new(npc),
            },
            // The ring's measure (#1019).
            ObjectiveSource::WorkInOrder { sequence, count } => Self::WorkInOrder {
                sequence: crate::SequenceId::new(sequence),
                count,
            },
            // The Downs road's supply patrol (#1019).
            ObjectiveSource::Sight { npc } => Self::Sight {
                npc: NpcId::new(npc),
            },
            ObjectiveSource::Ambush { npc } => Self::Ambush {
                npc: NpcId::new(npc),
            },
        }
    }
}

/// Everything `content/greyhaven/quests.json` defines.
struct Parsed {
    quests: Vec<Quest>,
    evidence: Vec<EvidenceSpot>,
    loads: Vec<LoadSpot>,
    attitudes: Vec<NpcAttitude>,
    confrontations: Vec<Confrontation>,
    departures: Vec<NpcDeparture>,
}

fn parse(source: &str) -> Result<Parsed, String> {
    let source: Source = serde_json::from_str(source).map_err(|error| error.to_string())?;
    if source.version != 1 {
        return Err(format!("unknown quests version {}", source.version));
    }
    let quests = source
        .quests
        .into_iter()
        .map(|quest| {
            Ok(Quest {
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
                supplies: quest
                    .supplies
                    .into_iter()
                    .map(|supply| {
                        Ok(QuestSupply {
                            item: ItemId::new(supply.item),
                            classes: classes(supply.classes)?,
                        })
                    })
                    .collect::<Result<_, String>>()?,
                requires: quest.requires.into_iter().map(QuestId::new).collect(),
                decision: quest.decision.into_iter().map(Into::into).collect(),
            })
        })
        .collect::<Result<_, String>>()?;
    let attitudes = source
        .attitudes
        .into_iter()
        .map(|attitude| {
            Ok(NpcAttitude {
                npc: NpcId::new(attitude.npc),
                distrusts: classes(attitude.distrusts)?,
                arcane: attitude.arcane,
                sponsor: attitude.sponsor,
                recommendation: attitude.recommendation.map(QuestId::new),
            })
        })
        .collect::<Result<_, String>>()?;
    let confrontations = source
        .confrontations
        .into_iter()
        .map(|confrontation| Confrontation {
            npc: NpcId::new(confrontation.npc),
            evidence: confrontation.evidence.map(ItemId::new),
            foe: CreatureId::new(confrontation.foe),
        })
        .collect();
    let evidence = source
        .evidence
        .into_iter()
        .map(|spot| EvidenceSpot {
            id: EvidenceId::new(spot.id),
            name: spot.name,
            at: spot.at,
            reach: spot.reach,
            directions: spot.directions,
        })
        .collect();
    let loads = source
        .loads
        .into_iter()
        .map(|spot| {
            Ok(LoadSpot {
                id: LoadSpotId::new(spot.id),
                kind: LoadKind::from_name(&spot.kind)
                    .ok_or_else(|| format!("unknown load kind {}", spot.kind))?,
                name: spot.name,
                at: spot.at,
                reach: spot.reach,
            })
        })
        .collect::<Result<_, String>>()?;
    let departures = source
        .departures
        .into_iter()
        .map(|departure| NpcDeparture {
            npc: NpcId::new(departure.npc),
            quest: QuestId::new(departure.quest),
            outcome: departure.outcome,
        })
        .collect();
    Ok(Parsed {
        quests,
        evidence,
        loads,
        attitudes,
        confrontations,
        departures,
    })
}

fn source() -> Parsed {
    parse(SOURCE).unwrap_or_else(|error| panic!("content/greyhaven/quests.json: {error}"))
}

/// The chain as `content/greyhaven/quests.json` lists it; the zone orders and validates it.
///
/// # Panics
///
/// When the committed quest file does not parse.
#[must_use]
pub fn quests() -> Vec<Quest> {
    source().quests
}

/// The evidence the quests' inspect objectives name (#1019), as `content/greyhaven/quests.json`
/// lists it; the world dressing draws each piece at its spot.
///
/// # Panics
///
/// When the committed quest file does not parse.
#[must_use]
pub fn evidence() -> Vec<EvidenceSpot> {
    source().evidence
}

/// The load sources the quests' deliveries name (#1077), as `content/greyhaven/quests.json`
/// lists them.
///
/// # Panics
///
/// When the committed quest file does not parse.
#[must_use]
pub fn loads() -> Vec<LoadSpot> {
    source().loads
}

/// How the quests' NPCs regard interviewers (#1019), as `content/greyhaven/quests.json` lists it.
///
/// # Panics
///
/// When the committed quest file does not parse.
#[must_use]
pub fn attitudes() -> Vec<NpcAttitude> {
    source().attitudes
}

/// The NPCs who turn hostile when shown evidence (#1019), as `content/greyhaven/quests.json`
/// lists them.
///
/// # Panics
///
/// When the committed quest file does not parse.
#[must_use]
pub fn confrontations() -> Vec<Confrontation> {
    source().confrontations
}

/// The NPCs who leave a character's world after its quest decision (#1019), as
/// `content/greyhaven/quests.json` lists them.
///
/// # Panics
///
/// When the committed quest file does not parse.
#[must_use]
pub fn departures() -> Vec<NpcDeparture> {
    source().departures
}
