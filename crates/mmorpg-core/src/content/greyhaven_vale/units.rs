//! Greyhaven Vale's creatures, NPCs and graveyard (content revision 3).
//!
//! Numbers follow the design contract's starting table in
//! `docs/STARTER_ZONE.md`. Spawn points sit on corners of the Wolfrun Woods
//! trunk grid (always clear of trunks) or in open ground of their subzone,
//! and every creature's wander area stays more than the interest radius away
//! from the hub spawn plaza. Tests validate both.

use crate::content::{
    CreatureBehaviour, CreatureFamily, CreatureSpawn, CreatureTemplate, Npc, NpcRole,
};
use crate::{CreatureId, CreatureTemplateId, NpcId, SeatId};

/// Creature templates by ID.
pub mod templates {
    use crate::CreatureTemplateId;

    pub const TIMBER_WOLF: CreatureTemplateId = CreatureTemplateId::new(1);
    pub const YOUNG_BOAR: CreatureTemplateId = CreatureTemplateId::new(2);
    pub const GRAIN_RAT: CreatureTemplateId = CreatureTemplateId::new(3);
    pub const FIELD_MARAUDER: CreatureTemplateId = CreatureTemplateId::new(4);
    pub const MIREFIN_LURKER: CreatureTemplateId = CreatureTemplateId::new(5);
    pub const REDBRAND_BANDIT: CreatureTemplateId = CreatureTemplateId::new(6);
    pub const GARRICK_REDBRAND: CreatureTemplateId = CreatureTemplateId::new(7);
    /// The Greyfang outriders, who shoot crossbow bolts (#286).
    pub const REDBRAND_CROSSBOWMAN: CreatureTemplateId = CreatureTemplateId::new(8);
    /// Breakable props (#288).
    pub const CRATE: CreatureTemplateId = CreatureTemplateId::new(9);
    pub const BARREL: CreatureTemplateId = CreatureTemplateId::new(10);
    pub const EXPLOSIVE_BARREL: CreatureTemplateId = CreatureTemplateId::new(11);
    pub const FENCE: CreatureTemplateId = CreatureTemplateId::new(12);
    /// A large creature (#297) that body-blocks, shoves and stomps.
    pub const HILL_OGRE: CreatureTemplateId = CreatureTemplateId::new(13);
    /// A movable crate (#58) that knockbacks and blasts slide.
    pub const HEAVY_CRATE: CreatureTemplateId = CreatureTemplateId::new(14);
    /// The rolling boulder trap above Redbrand Hollow (#299).
    pub const BOULDER: CreatureTemplateId = CreatureTemplateId::new(17);
    /// The Greyfang cavern's heavy shield-bearers (#160): slow overhead smashes, and arrows
    /// and bolts at their front stop on their shields.
    pub const REDBRAND_BRUTE: CreatureTemplateId = CreatureTemplateId::new(15);
    /// The beasts of #889: a bear in the deep woods, spiders in the caves and lurkers in the
    /// marsh, biting with the wildlife release's wind-ups.
    pub const BROWN_BEAR: CreatureTemplateId = CreatureTemplateId::new(18);
    pub const CAVE_SPIDER: CreatureTemplateId = CreatureTemplateId::new(19);
    pub const MARSH_LURKER: CreatureTemplateId = CreatureTemplateId::new(20);
    /// Old Scarback, the elite bear that holds the Wolfrun hunter's cache (#913).
    pub const OLD_SCARBACK: CreatureTemplateId = CreatureTemplateId::new(21);
    /// The Southwood lynx (#989): an ambusher that stalks in and pounces.
    pub const FOREST_LYNX: CreatureTemplateId = CreatureTemplateId::new(22);
    /// The Eastwater Fen adder (#990): an ambusher that coils, hisses and strikes close.
    pub const MARSH_ADDER: CreatureTemplateId = CreatureTemplateId::new(23);
    /// The Greyfang cave bat (#995): a flier that roosts on the cave roof and dives.
    pub const CAVE_BAT: CreatureTemplateId = CreatureTemplateId::new(24);
    /// The Fenwater river crayfish (#996): an armoured bank ambusher whose front turns blows.
    pub const RIVER_CRAYFISH: CreatureTemplateId = CreatureTemplateId::new(25);
    /// The Southwood stag (#997): neutral but territorial; it charges a player who comes close.
    pub const SOUTHWOOD_STAG: CreatureTemplateId = CreatureTemplateId::new(26);
    /// The courier at the east gate once he drops his act (#1019).
    pub const REDBRAND_COURIER: CreatureTemplateId = CreatureTemplateId::new(27);
}

/// Default respawn time: 60 s.
pub const RESPAWN_TICKS: u32 = crate::long_ticks_from_ms(60_000);
/// Garrick Redbrand returns 2 min after his death (#276, Atlas 5.1.75); his corpse stays until then.
pub const GARRICK_RESPAWN_TICKS: u32 = crate::long_ticks_from_ms(120_000);
/// A broken prop stands again after 5 min, once its spot is clear (#288).
pub const PROP_RESPAWN_TICKS: u32 = crate::long_ticks_from_ms(300_000);
/// Idle creatures wander within 6 m of their spawn point.
pub const WANDER_RADIUS_UNITS: i32 = 600;
/// Stillwater Lake's lurkers (spawn IDs) wander only 2 m, so they stay on the level rim
/// above the basin's slope (#423; physics-engine #394 sinks units resting on slopes).
pub const LAKE_LURKERS: std::ops::Range<u32> = 300..308;
pub const LAKE_WANDER_RADIUS_UNITS: i32 = 200;
/// Wolf 108 at the den clearing where the Wolfrun Trail ends.
const DEN_WOLF: u32 = 108;
/// Released spirits return beside the graveyard, south of the gravestones'
/// western end and clear of houses.
pub const GRAVEYARD: [i32; 2] = [-2_000, 4_150];

const WOLF: [i32; 3] = [40, 45, 40];
const BOAR: [i32; 3] = [45, 45, 45];
/// As tall as the wildlife release's 0.3 m rat (#889), so it is drawn at its own size.
const RAT: [i32; 3] = [25, 15, 25];
/// The wildlife release's beasts (#889): a 1.3 m bear, a low, broad spider and a flat lurker.
const BEAR: [i32; 3] = [50, 65, 50];
const SPIDER: [i32; 3] = [40, 23, 40];
/// Old Scarback (#913): a broader, taller bear, still short of a large creature.
const SCARBACK: [i32; 3] = [60, 78, 60];
const MARSH_LURKER_BOX: [i32; 3] = [38, 15, 38];
/// The wildlife release's 0.8 m lynx (#989) on a square footprint between its width and length.
const LYNX: [i32; 3] = [30, 40, 30];
/// The wildlife release's marsh adder (#990) in its coil: 0.42 m across, 0.19 m tall.
const ADDER: [i32; 3] = [22, 10, 22];
/// The wildlife release's cave bat (#995) hovering: 0.38 m tall, its body a little wider.
const BAT: [i32; 3] = [30, 19, 30];
/// The wildlife release's river crayfish (#996), 0.67 m wide, 1.38 m long and 0.32 m tall, on a
/// square footprint between its width and length.
const CRAYFISH: [i32; 3] = [50, 16, 50];
/// The wildlife release's 2 m stag (#997) to its antler tips, on a square footprint between its
/// width and length.
const STAG: [i32; 3] = [45, 100, 45];
const HUMANOID: [i32; 3] = [30, 90, 30];
const LURKER: [i32; 3] = [35, 80, 35];
/// A 1.4 m rolling boulder (#299).
const BOULDER_BOX: [i32; 3] = [70, 70, 70];
const GARRICK: [i32; 3] = [40, 100, 40];
/// A broad-shouldered Redbrand, still smaller than a large creature (#160).
const BRUTE: [i32; 3] = [38, 96, 38];
const CRATE_BOX: [i32; 3] = [35, 35, 35];
const BARREL_BOX: [i32; 3] = [30, 45, 30];
/// A fence section runs along X at facing 0.
const FENCE_BOX: [i32; 3] = [110, 50, 8];
/// 1.4 m across and 2.8 m tall: a large creature (#297).
const OGRE: [i32; 3] = [70, 140, 70];

/// `(id, name, family, behaviour, levels, elite, health, health/level,
/// damage, damage/level, swing ticks, respawn ticks, half extents)`, durations authored in ms.
type TemplateRow = (
    CreatureTemplateId,
    &'static str,
    CreatureFamily,
    CreatureBehaviour,
    [u8; 2],
    bool,
    u32,
    u32,
    [u16; 2],
    u16,
    u16,
    u32,
    [i32; 3],
);

#[rustfmt::skip]
const TEMPLATES: [TemplateRow; 26] = {
    use CreatureBehaviour::{Aggressive, Neutral, Prop};
    use CreatureFamily::{Adder, Bat, Bear, Boar, Crayfish, Lurker, Lynx, Marauder, Mirefin, Ogre, Redbrand, Spider, Stag, Vermin, Wolf};
    use crate::PropKind;
    use templates::*;
    [
        (TIMBER_WOLF, "Timber Wolf", Wolf, Aggressive, [1, 2], false, 42, 14, [2, 4], 1, crate::ticks_from_ms(2_000), RESPAWN_TICKS, WOLF),
        (YOUNG_BOAR, "Young Boar", Boar, Neutral, [1, 2], false, 48, 14, [2, 4], 0, crate::ticks_from_ms(2_200), RESPAWN_TICKS, BOAR),
        (GRAIN_RAT, "Grain Rat", Vermin, Neutral, [1, 1], false, 30, 0, [1, 3], 0, crate::ticks_from_ms(1_600), RESPAWN_TICKS, RAT),
        (FIELD_MARAUDER, "Field Marauder", Marauder, Aggressive, [2, 3], false, 60, 16, [3, 6], 0, crate::ticks_from_ms(2_200), RESPAWN_TICKS, HUMANOID),
        (MIREFIN_LURKER, "Mirefin Lurker", Mirefin, Aggressive, [3, 3], false, 70, 0, [3, 6], 0, crate::ticks_from_ms(2_000), RESPAWN_TICKS, LURKER),
        (REDBRAND_BANDIT, "Redbrand Bandit", Redbrand, Aggressive, [3, 5], false, 75, 16, [4, 7], 0, crate::ticks_from_ms(2_200), RESPAWN_TICKS, HUMANOID),
        (GARRICK_REDBRAND, "Garrick Redbrand", Redbrand, Aggressive, [5, 5], true, 420, 0, [8, 14], 0, crate::ticks_from_ms(2_400), GARRICK_RESPAWN_TICKS, GARRICK),
        (REDBRAND_CROSSBOWMAN, "Redbrand Crossbowman", Redbrand, Aggressive, [3, 5], false, 70, 14, [3, 6], 0, crate::ticks_from_ms(2_200), RESPAWN_TICKS, HUMANOID),
        // Breakable props (#288): a few hits break them; they never swing.
        (CRATE, "Crate", CreatureFamily::Prop, Prop(PropKind::Crate), [1, 1], false, 12, 0, [1, 1], 0, crate::ticks_from_ms(2_000), PROP_RESPAWN_TICKS, CRATE_BOX),
        (BARREL, "Barrel", CreatureFamily::Prop, Prop(PropKind::Barrel), [1, 1], false, 15, 0, [1, 1], 0, crate::ticks_from_ms(2_000), PROP_RESPAWN_TICKS, BARREL_BOX),
        (EXPLOSIVE_BARREL, "Blasting Powder Barrel", CreatureFamily::Prop, Prop(PropKind::ExplosiveBarrel), [1, 1], false, 10, 0, [1, 1], 0, crate::ticks_from_ms(2_000), PROP_RESPAWN_TICKS, BARREL_BOX),
        (FENCE, "Fence", CreatureFamily::Prop, Prop(PropKind::Fence), [1, 1], false, 18, 0, [1, 1], 0, crate::ticks_from_ms(2_000), PROP_RESPAWN_TICKS, FENCE_BOX),
        // A large creature (#297): slow heavy swings, shoves and stomps.
        (HILL_OGRE, "Hill Ogre", Ogre, Aggressive, [5, 5], false, 300, 0, [9, 15], 1, crate::ticks_from_ms(2_800), RESPAWN_TICKS, OGRE),
        // A movable prop (#58): knockbacks slide it; it breaks like a crate.
        (HEAVY_CRATE, "Heavy Crate", CreatureFamily::Prop, Prop(PropKind::HeavyCrate), [1, 1], false, 30, 0, [1, 1], 0, crate::ticks_from_ms(2_000), PROP_RESPAWN_TICKS, CRATE_BOX),
        // The boulder trap (#299): unbreakable, its health only names it a prop.
        (BOULDER, "Loose Boulder", CreatureFamily::Prop, Prop(PropKind::Boulder), [1, 1], false, 1, 0, [1, 1], 0, crate::ticks_from_ms(2_000), PROP_RESPAWN_TICKS, BOULDER_BOX),
        // The Greyfang cavern's shield-bearer (#160): twice a level 5 bandit's health, heavier
        // and slower blows, and an Overhead Smash.
        (REDBRAND_BRUTE, "Redbrand Brute", Redbrand, Aggressive, [5, 5], false, 215, 0, [7, 12], 0, crate::ticks_from_ms(2_800), RESPAWN_TICKS, BRUTE),
        // The beasts of #889: the bear tough and slow, the spider fast and fragile, the lurker
        // a hard biter that waits for a player to come close.
        (BROWN_BEAR, "Brown Bear", Bear, Aggressive, [3, 4], false, 130, 20, [5, 9], 1, crate::ticks_from_ms(3_000), RESPAWN_TICKS, BEAR),
        (CAVE_SPIDER, "Cave Spider", Spider, Aggressive, [3, 4], false, 40, 10, [2, 4], 1, crate::ticks_from_ms(1_300), RESPAWN_TICKS, SPIDER),
        (MARSH_LURKER, "Marsh Lurker", Lurker, Aggressive, [2, 3], false, 60, 14, [3, 6], 1, crate::ticks_from_ms(2_400), RESPAWN_TICKS, MARSH_LURKER_BOX),
        // The hunter's cache's elite bear (#913): a level 5 elite in the starter woods, a risk for
        // a hunter not yet that strong, with the bear's slow, wound-up bites and Garrick's respawn.
        (OLD_SCARBACK, "Old Scarback", Bear, Aggressive, [5, 5], true, 360, 0, [8, 13], 0, crate::ticks_from_ms(3_000), GARRICK_RESPAWN_TICKS, SCARBACK),
        // The Southwood lynx (#989): Southwood's level, a light body and a hard, slow pounce.
        (FOREST_LYNX, "Forest Lynx", Lynx, Aggressive, [2, 3], false, 50, 12, [4, 7], 1, crate::ticks_from_ms(2_600), RESPAWN_TICKS, LYNX),
        // The Eastwater Fen adder (#990): the fen's level, a frail body and a quick, warned strike.
        (MARSH_ADDER, "Marsh Adder", Adder, Aggressive, [2, 3], false, 34, 10, [4, 7], 1, crate::ticks_from_ms(2_600), RESPAWN_TICKS, ADDER),
        // The Greyfang cave bat (#995): the cave mouth's level, a frail body and a warned dive.
        (CAVE_BAT, "Cave Bat", Bat, Aggressive, [3, 4], false, 36, 10, [3, 6], 1, crate::ticks_from_ms(2_600), RESPAWN_TICKS, BAT),
        // The Fenwater crayfish (#996): the crossing's level, a hard shell and a slow, heavy pinch.
        (RIVER_CRAYFISH, "River Crayfish", Crayfish, Aggressive, [3, 4], false, 70, 14, [5, 9], 1, crate::ticks_from_ms(3_000), RESPAWN_TICKS, CRAYFISH),
        // The Southwood stag (#997): Southwood's upper level, a strong body and a slow, warned charge.
        (SOUTHWOOD_STAG, "Southwood Stag", Stag, Neutral, [3, 4], false, 80, 16, [5, 9], 1, crate::ticks_from_ms(3_000), RESPAWN_TICKS, STAG),
        // The east gate's courier (#1019): a level 4 bandit's body; he stands only when unmasked.
        (REDBRAND_COURIER, "Redbrand Courier", Redbrand, Aggressive, [4, 4], false, 75, 16, [4, 7], 0, crate::ticks_from_ms(2_200), RESPAWN_TICKS, HUMANOID),
    ]
};

type CavernRow = (u32, CreatureTemplateId, [i32; 2], u16, [u8; 2]);

/// The Greyfang cavern (#160) as `(spawn id, template, feet XZ, facing, levels)`: Redbrands
/// at levels 4–5 hold the outer cavern and the inner chamber, a Brute in each; the inner
/// Brute guards the sealed threshold, and Cave Spiders (#889) hold the mouth. They wander only [`CAVERN_WANDER_RADIUS_UNITS`].
#[rustfmt::skip]
const CAVERN: [CavernRow; 9] = {
    use templates::*;
    [
        (650, REDBRAND_BRUTE, [6_100, -13_700], 0, [5, 5]),
        (651, REDBRAND_BANDIT, [5_300, -13_500], 0, [4, 5]),
        (652, REDBRAND_CROSSBOWMAN, [5_100, -14_100], 16_384, [4, 5]),
        (653, REDBRAND_CROSSBOWMAN, [6_500, -14_100], 49_152, [4, 5]),
        (654, REDBRAND_BANDIT, [7_450, -13_450], 49_152, [4, 5]),
        (655, REDBRAND_BRUTE, [7_650, -13_800], 49_152, [5, 5]),
        // Cave Spiders (#889) in the north mouth's passage between its rocks.
        (656, CAVE_SPIDER, [6_280, -14_480], 32_768, [3, 4]),
        (657, CAVE_SPIDER, [6_520, -14_650], 32_768, [3, 4]),
        (658, CAVE_SPIDER, [6_300, -14_700], 32_768, [3, 3]),
    ]
};
/// Cavern Redbrands keep to their rock.
pub const CAVERN_WANDER_RADIUS_UNITS: i32 = 200;

/// Breakable props (#288) as `(spawn id, template, feet XZ, facing)`: crates and barrels by
/// Greyhaven's smithy and houses, crates, barrels and fences at Millbrook Farm, and powder
/// barrels in the Redbrand camp, set close enough to chain-react, and supplies inside its cave.
#[rustfmt::skip]
const PROPS: [(u32, CreatureTemplateId, [i32; 2], u16); 23] = {
    use templates::*;
    [
        // Greyhaven Outpost.
        (900, CRATE, [2_200, 1_000], 0),
        (901, CRATE, [2_310, 1_000], 2_048),
        (902, BARREL, [2_700, 1_050], 0),
        (903, BARREL, [1_750, 3_300], 0),
        (904, CRATE, [-1_900, 3_200], 4_096),
        // Millbrook Farm.
        (910, CRATE, [7_250, 3_450], 0),
        (911, CRATE, [7_250, 3_340], 0),
        (912, BARREL, [8_750, 3_450], 0),
        (913, BARREL, [6_700, 4_300], 0),
        (914, FENCE, [5_200, 3_600], 0),
        (915, FENCE, [5_430, 3_600], 0),
        (916, FENCE, [5_000, 3_730], 16_384),
        // Millbrook Farm: two heavy crates south of the barn that knockbacks slide (#58).
        (917, HEAVY_CRATE, [7_400, 3_200], 0),
        (918, HEAVY_CRATE, [7_550, 3_200], 0),
        // Redbrand Hollow: powder barrels by the campfire and beside the western tents.
        (920, EXPLOSIVE_BARREL, [300, -9_000], 0),
        (921, EXPLOSIVE_BARREL, [370, -9_000], 0),
        (922, EXPLOSIVE_BARREL, [335, -8_935], 0),
        (923, EXPLOSIVE_BARREL, [-800, -9_500], 0),
        (924, EXPLOSIVE_BARREL, [-870, -9_500], 0),
        // The boulder trap above the camp's south edge (#299): it rests on the rise north of the
        // watch post, and rolling down it sends the boulder across the bandits' camp.
        (925, BOULDER, [950, -7_420], 0),
        // Redbrand Cave (#719): a small supply row along the outer chamber's west wall.
        // Broken panels/staves meet the authored wall and floor, clear of the mouth,
        // south shelf and passage to the inner chamber. These barrels contain no powder.
        (926, CRATE, [4_900, -13_900], 0),
        (927, CRATE, [4_900, -13_800], 0),
        (928, BARREL, [4_900, -13_700], 0),
    ]
};

/// `(spawn id, template, feet XZ, facing)`. IDs group by subzone:
/// 100s Wolfrun Woods, 200s Millbrook Farm, 300s Stillwater Lake, 400s
/// Redbrand Hollow, 500s–800s the outer valley (#159).
#[rustfmt::skip]
const SPAWNS: [(u32, CreatureTemplateId, [i32; 2], u16); 75] = {
    use templates::*;
    [
        // Wolfrun Woods: timber wolves on trunk-grid corners. Wolf 108 guards
        // the den clearing where the Wolfrun Trail ends; the others keep
        // more than 12 m from the trail and from it.
        (100, TIMBER_WOLF, [-10_250, -2_700], 0),
        (101, TIMBER_WOLF, [-9_350, -3_150], 8_192),
        (102, TIMBER_WOLF, [-8_000, -2_700], 16_384),
        (103, TIMBER_WOLF, [-6_650, -3_150], 24_576),
        (104, TIMBER_WOLF, [-6_200, -2_700], 32_768),
        (105, TIMBER_WOLF, [-10_700, -1_800], 40_960),
        (106, TIMBER_WOLF, [-9_350, -1_350], 49_152),
        (107, TIMBER_WOLF, [-8_000, -1_800], 57_344),
        (108, TIMBER_WOLF, [-8_450, 450], 16_384),
        (109, TIMBER_WOLF, [-10_700, 1_800], 0),
        (110, TIMBER_WOLF, [-9_800, 3_150], 8_192),
        (111, TIMBER_WOLF, [-8_450, 3_600], 16_384),
        (112, TIMBER_WOLF, [-7_100, 3_150], 24_576),
        (113, TIMBER_WOLF, [-11_150, 4_050], 32_768),
        // Wolfrun Woods: young boars.
        (120, YOUNG_BOAR, [-11_150, -900], 0),
        (121, YOUNG_BOAR, [-9_800, 900], 16_384),
        (122, YOUNG_BOAR, [-7_550, -2_250], 32_768),
        (123, YOUNG_BOAR, [-10_250, 2_700], 49_152),
        (124, YOUNG_BOAR, [-7_100, 4_050], 8_192),
        // Stillwater now occupies the old northern clearing; retain this boar on dry woodland.
        (125, YOUNG_BOAR, [-10_900, -2_300], 24_576),
        // Millbrook Farm: grain rats around the barn and farmhouse.
        (200, GRAIN_RAT, [7_200, 4_000], 0),
        (201, GRAIN_RAT, [8_800, 4_400], 16_384),
        (202, GRAIN_RAT, [8_000, 3_400], 32_768),
        (203, GRAIN_RAT, [6_900, 4_700], 49_152),
        (204, GRAIN_RAT, [8_800, 2_700], 8_192),
        (205, GRAIN_RAT, [7_600, 5_400], 24_576),
        // Millbrook Farm: field marauders inside the fenced fields, 10 m apart.
        (220, FIELD_MARAUDER, [5_800, 6_400], 32_768),
        (221, FIELD_MARAUDER, [6_800, 6_200], 32_768),
        (222, FIELD_MARAUDER, [7_800, 6_400], 32_768),
        (223, FIELD_MARAUDER, [8_800, 6_200], 32_768),
        (224, FIELD_MARAUDER, [6_300, 7_300], 0),
        (225, FIELD_MARAUDER, [7_300, 7_200], 0),
        (226, FIELD_MARAUDER, [8_300, 7_300], 0),
        (227, FIELD_MARAUDER, [9_200, 7_200], 0),
        (228, FIELD_MARAUDER, [5_800, 8_000], 16_384),
        (229, FIELD_MARAUDER, [7_800, 8_000], 49_152),
        // Stillwater Lake: mirefin lurkers on the shallow rim, clear of the deep basin and the
        // Sunken Shrine's cave (#274), out on the level rim (#423).
        (300, MIREFIN_LURKER, [4_200, -4_310], 16_384),
        (301, MIREFIN_LURKER, [4_890, -4_050], 32_768),
        (302, MIREFIN_LURKER, [6_380, -4_120], 32_768),
        (303, MIREFIN_LURKER, [7_040, -4_470], 49_152),
        (304, MIREFIN_LURKER, [7_190, -6_400], 57_344),
        (305, MIREFIN_LURKER, [5_850, -7_000], 0),
        (306, MIREFIN_LURKER, [3_800, -5_800], 8_192),
        (307, MIREFIN_LURKER, [6_500, -6_830], 24_576),
        // Redbrand Hollow: the bandit camp between the cliffs.
        (400, REDBRAND_BANDIT, [-1_000, -8_200], 0),
        (401, REDBRAND_BANDIT, [1_000, -7_800], 0),
        (402, REDBRAND_BANDIT, [-500, -8_400], 0),
        (403, REDBRAND_BANDIT, [600, -8_500], 0),
        (404, REDBRAND_BANDIT, [-1_100, -9_000], 16_384),
        (405, REDBRAND_BANDIT, [1_000, -9_100], 49_152),
        (406, REDBRAND_BANDIT, [-400, -9_700], 0),
        (407, REDBRAND_BANDIT, [500, -9_800], 0),
        (408, REDBRAND_BANDIT, [-1_300, -10_000], 8_192),
        (409, REDBRAND_BANDIT, [1_300, -10_200], 57_344),
        (410, REDBRAND_BANDIT, [-2_000, -9_400], 16_384),
        (411, REDBRAND_BANDIT, [2_000, -8_200], 49_152),
        (412, REDBRAND_BANDIT, [0, -10_000], 0),
        (413, REDBRAND_BANDIT, [-500, -8_000], 16_384),
        // Garrick Redbrand holds the mine entrance.
        (450, GARRICK_REDBRAND, [0, -10_500], 0),
        // The outer valley (#159), on trunk-grid corners of its woods.
        // Hollowpine Reach: wolves and boars among the pines.
        (500, TIMBER_WOLF, [-18_600, -8_000], 0),
        (501, TIMBER_WOLF, [-17_250, -4_400], 16_384),
        (502, TIMBER_WOLF, [-19_050, 550], 32_768),
        (503, TIMBER_WOLF, [-16_800, 4_150], 49_152),
        (504, YOUNG_BOAR, [-18_150, 8_200], 8_192),
        (505, YOUNG_BOAR, [-16_800, -2_150], 40_960),
        // Greyfang Ridge: Redbrand outriders watching the standing stones.
        (600, REDBRAND_CROSSBOWMAN, [-1_350, -17_250], 0),
        (601, REDBRAND_CROSSBOWMAN, [1_350, -17_250], 0),
        (602, REDBRAND_CROSSBOWMAN, [0, -18_600], 32_768),
        // Southmeadow Downs: boars and a rat around the ruined farmstead.
        (700, YOUNG_BOAR, [3_600, 17_300], 16_384),
        (701, YOUNG_BOAR, [9_000, 16_850], 49_152),
        (702, GRAIN_RAT, [5_850, 17_300], 0),
        // A hill ogre (#297) squats east of the ruined farmstead.
        (710, HILL_OGRE, [7_450, 18_150], 32_768),
        // Eastwater Fen: mirefin lurkers in the wet ground.
        (800, MIREFIN_LURKER, [14_850, -7_100], 16_384),
        (801, MIREFIN_LURKER, [17_550, -4_400], 32_768),
        (802, MIREFIN_LURKER, [16_200, 2_350], 49_152),
    ]
};

/// The four gate guards share one name.
const GUARD: &str = "Greyhaven Guard";

/// `(id, name, role, level, feet XZ, facing)`.
type NpcRow = (u32, &'static str, NpcRole, u8, [i32; 2], u16);

#[rustfmt::skip]
const NPCS: [NpcRow; 24] = [
    (1, "Marshal Elden Greywatch", NpcRole::QuestGiver, 10, [-2_200, 900], 0),
    (2, "Tanner Hilda Brook", NpcRole::QuestGiver, 5, [-2_400, 3_150], 32_768),
    // Inside the inn, behind its bar, facing the room (#158).
    (3, "Innkeeper Bram Tolliver", NpcRole::Vendor, 5, [1_920, -380], 0),
    (4, "Farmer Osric Mill", NpcRole::QuestGiver, 4, [6_200, 4_150], 32_768),
    (5, "Brother Aldous", NpcRole::SpiritHealer, 10, [-1_650, 4_550], 49_152),
    // Guards stand inside the palisade beside the north, south, west and
    // east gates, facing out. The western guard now watches the mill towpath.
    (6, GUARD, NpcRole::Guard, 10, [-500, -800], 32_768),
    (7, GUARD, NpcRole::Guard, 10, [500, 4_800], 0),
    (8, GUARD, NpcRole::Guard, 10, [-2_620, 2_250], 49_152),
    // The east guard stands a metre south of the market square's front crate (#577).
    (9, GUARD, NpcRole::Guard, 10, [2_900, 2_600], 16_384),
    (10,"Weaver Lysa Reed",NpcRole::Resident,2,[-2100,1850],0),
    (11,"Carpenter Owen Alder",NpcRole::Resident,3,[2150,1850],0),
    (12,"Mara Wells",NpcRole::Resident,2,[700,4000],32768),
    (13,"Davin Moss",NpcRole::Resident,2,[-1000,2900],0),
    // Small conversations and stall customers at the edges of the clear spawn square.
    (14,"Baker Anwen",NpcRole::Resident,3,[1900,2800],16384),
    (15,"Miller Tomas",NpcRole::Resident,3,[2050,2850],49152),
    // The herbalist gives *The Miller's Medicine* (#1019).
    (16,"Herbalist Nessa",NpcRole::QuestGiver,3,[1750,2450],16384),
    (17,"Peddler Rowan",NpcRole::Resident,3,[1850,2350],49152),
    (18,"Fisher Ada",NpcRole::Resident,2,[-1750,2400],49152),
    (19,"Porter Ben",NpcRole::Resident,2,[-1900,2500],16384),
    (20,"Washer Elin",NpcRole::Resident,2,[1400,3100],49152),
    // On the market bench by day (#619): the stand spot of seat 1.
    (21,"Apprentice Finn",NpcRole::Resident,2,[2410,2520],0),
    // In the smithy's back yard by his bench, facing the alley from the square (#917).
    (43, "Smith Wulfric Hale", NpcRole::Vendor, 6, [2_300, -250], 0),
    // Core Vale B (#1019): the wounded watchman on the Wolfrun terrace at the top of the climb,
    // and the courier loitering outside the east gate, facing it.
    (44, "Guardsman Tam Hollis", NpcRole::Guard, 6, [-5_600, 1_450], 32_768),
    (45, "Courier Wat Ferrow", NpcRole::Resident, 4, [3_650, 2_300], 49_152),
];

pub(crate) fn creature_templates() -> Vec<CreatureTemplate> {
    TEMPLATES
        .iter()
        .map(
            |&(
                id,
                name,
                family,
                behaviour,
                [min_level, max_level],
                elite,
                health,
                health_per_level,
                damage,
                damage_per_level,
                swing_ticks,
                respawn_ticks,
                half_extents,
            )| CreatureTemplate {
                id,
                name: name.to_owned(),
                family,
                behaviour,
                min_level,
                max_level,
                elite,
                health,
                health_per_level,
                damage,
                damage_per_level,
                swing_ticks,
                respawn_ticks,
                half_extents,
            },
        )
        .collect()
}

pub(super) fn creature_spawns() -> Vec<CreatureSpawn> {
    SPAWNS
        .iter()
        .map(|&(id, template, position, facing)| CreatureSpawn {
            id: CreatureId::new(id),
            template,
            position: if LAKE_LURKERS.contains(&id) {
                super::lake::place(position)
            } else {
                position
            },
            facing: if LAKE_LURKERS.contains(&id) {
                facing.wrapping_add(32768)
            } else {
                facing
            },
            // Stillwater Lake's lurkers keep to its narrow level rim (#423).
            wander_radius: if LAKE_LURKERS.contains(&id) {
                LAKE_WANDER_RADIUS_UNITS
            } else if super::powder_watch::holds_post(id) {
                // The Redbrand powder watch keeps its posts by the powder (#954).
                super::powder_watch::POST_WANDER_RADIUS_UNITS
            } else {
                WANDER_RADIUS_UNITS
            },
            // The den wolf, a new hunter's first fight on the Wolfrun Trail, is always level 1 (#861).
            levels: (id == DEN_WOLF).then_some([1, 1]),
        })
        .chain(
            CAVERN
                .iter()
                .map(|&(id, template, position, facing, levels)| CreatureSpawn {
                    id: CreatureId::new(id),
                    template,
                    position,
                    facing,
                    wander_radius: CAVERN_WANDER_RADIUS_UNITS,
                    levels: Some(levels),
                }),
        )
        .chain(
            PROPS
                .iter()
                .map(|&(id, template, position, facing)| CreatureSpawn {
                    id: CreatureId::new(id),
                    template,
                    position,
                    facing,
                    wander_radius: 0,
                    levels: None,
                }),
        )
        .chain(super::raid::raider_spawns())
        // Bandit Highlands garrison (#502).
        .chain(super::highland_bandits::spawns())
        // Marsh-Lake Wetlands creatures (#506).
        .chain(super::marsh_creatures::spawns())
        // The new sub-regions' creatures (#816).
        .chain(super::wilds::spawns())
        // The farmland hamlets' creatures (#859).
        .chain(super::hamlet_creatures::spawns())
        // The Greyfang Approach's Redbrand patrol (#911).
        .chain(super::redbrand_patrol::spawns())
        // The Rimewood scouts, an Arcanist's group-control fight (#933).
        .chain(super::rimewood::spawns())
        // The Eastwater Fen wolf pack (#932).
        .chain(super::fen_pack::spawns())
        // The optional discoveries' guards (#913).
        .chain(super::discoveries::spawns())
        // The Rimewood signal post, a target-priority fight (#952).
        .chain(super::signal_post::spawns())
        // Trapper Sela's hunt's bear (#991).
        .chain(super::hunt::spawns())
        // Peddler Rowan's cart robbers (#992).
        .chain(super::cart::spawns())
        // The Southwood lynxes (#989).
        .chain(super::lynx::spawns())
        // The Eastwater Fen adders (#990).
        .chain(super::adder::spawns())
        // The Greyfang cave bats (#995).
        .chain(super::bat::spawns())
        // The Fenwater river crayfish (#996).
        .chain(super::crayfish::spawns())
        // The Southwood stags (#997).
        .chain(super::stag::spawns())
        // The Core Vale's confrontation foes (#1019).
        .chain(super::quests::spawns())
        .collect()
}

pub(super) fn npcs() -> Vec<Npc> {
    NPCS.iter()
        .map(|&(id, name, role, level, position, facing)| Npc {
            id: NpcId::new(id),
            name: name.to_owned(),
            role,
            level,
            position,
            facing,
        })
        .chain(super::residents::npcs())
        .collect()
}

/// Creature abilities of this revision.
#[must_use]
pub fn creature_abilities() -> Vec<(CreatureTemplateId, crate::AbilityId)> {
    vec![
        (
            templates::GARRICK_REDBRAND,
            crate::ability::ids::REDBRAND_WHIRL,
        ),
        (templates::MIREFIN_LURKER, crate::ability::ids::MUCK_BOLT),
        (
            templates::REDBRAND_BANDIT,
            crate::ability::ids::CRUDE_BANDAGE,
        ),
        (
            templates::REDBRAND_CROSSBOWMAN,
            crate::ability::ids::CROSSBOW_BOLT,
        ),
        (templates::HILL_OGRE, crate::ability::ids::STOMP),
        (
            templates::REDBRAND_BRUTE,
            crate::ability::ids::OVERHEAD_SMASH,
        ),
    ]
}

/// The Redbrand Brute's shield stops arrows, bolts and melee blows at its front (#160, #953).
#[must_use]
pub fn shielded() -> Vec<CreatureTemplateId> {
    vec![templates::REDBRAND_BRUTE]
}

/// Greyhaven's shielded templates: the Brute ([`shielded`]) and the River Crayfish, whose
/// shell and claws turn blows at its front the same way (#996).
#[must_use]
pub fn greyhaven_shielded() -> Vec<CreatureTemplateId> {
    let mut shields = shielded();
    shields.push(templates::RIVER_CRAYFISH);
    shields
}

/// Garrick Redbrand is the shared outdoor boss (#276): +60% health and +10% damage per extra
/// participant (up to five), and his camp rallies at the mine mouth for 20 s when he falls.
#[must_use]
pub fn bosses() -> Vec<crate::BossRules> {
    vec![crate::BossRules {
        template: templates::GARRICK_REDBRAND,
        health_percent_per_extra: 60,
        damage_percent_per_extra: 10,
        engage_radius: 2_500,
        reaction_radius: 3_000,
        reaction_ticks: crate::ticks_from_ms(20_000),
        rally_point: [0, -10_100],
    }]
}

/// Innkeeper Bram Tolliver (NPC 3) sells the starter stock.
pub(crate) fn vendors() -> Vec<(NpcId, crate::VendorStock)> {
    vec![
        (NpcId::new(3), crate::starter_vendor_stock()),
        // Trader Maren at Fenford (#826).
        (NpcId::new(42), crate::fenford_trader_stock()),
        // Smith Wulfric Hale at the Outpost smithy makes the boar-spear (#917).
        (NpcId::new(43), crate::smith_stock()),
    ]
}

/// Tanner Hilda Brook (NPC 2) works by her tannery and spends the night in the inn's common
/// room, crossing the square and opening the inn door on the way (#158, Atlas 5.1.45). Her
/// quests stay available wherever she is. Marshal Elden Greywatch (NPC 1) walks home to house
/// 103 at dusk and sleeps by his bed until dawn (#158, Atlas 5.1.48). Hilda takes a chair at the
/// inn's table for the night; Apprentice Finn (NPC 21) sits on the market bench by day (#619).
pub(crate) fn routines() -> Vec<crate::NpcRoutine> {
    let mut routines = vec![
        crate::NpcRoutine {
            npc: NpcId::new(1),
            path: super::marshal_house::path(),
            home_facing: super::marshal_house::BEDSIDE_FACING,
            door: Some(super::marshal_house::DOOR),
            home_seat: None,
            work_seat: None,
            errands: Vec::new(),
        },
        crate::NpcRoutine {
            npc: NpcId::new(2),
            path: vec![
                [-1_500, 2_000],
                [1_500, 1_000],
                [1_500, 620],
                [1_500, 350],
                [1_600, 100],
                [1_302, 10],
            ],
            home_facing: 32_768,
            door: Some(super::inn::DOOR),
            home_seat: Some(SeatId::new(3)),
            work_seat: None,
            errands: Vec::new(),
        },
        crate::NpcRoutine {
            npc: NpcId::new(21),
            path: vec![[2_300, 2_700]],
            home_facing: 16_384,
            door: None,
            home_seat: None,
            work_seat: Some(SeatId::new(1)),
            errands: Vec::new(),
        },
    ];
    // Everyone else's day (#819).
    routines.extend(super::residents::routines());
    routines
}

/// The authored seats (#619): the market bench's two places, then two chairs at the inn's third
/// table. Stable IDs; add new ones at the end.
pub(crate) fn seats() -> Vec<crate::Seat> {
    use super::{inn, market};
    let [x, z] = market::BENCH;
    let bench = [(1, -1), (2, 1)].map(|(id, side)| crate::Seat {
        id: SeatId::new(id),
        stand: [
            x + side * market::BENCH_SEAT_OFFSET,
            z + market::BENCH_STAND_Z,
        ],
        root: [
            x + side * market::BENCH_SEAT_OFFSET,
            z + market::BENCH_ROOT_Z,
        ],
        facing: 0,
    });
    let chairs = inn::SEATS.map(|(id, chair)| crate::Seat {
        id: SeatId::new(id),
        stand: [inn::ORIGIN[0] + chair, inn::ORIGIN[2] + inn::SEAT_STAND_Z],
        root: [inn::ORIGIN[0] + chair, inn::ORIGIN[2] + inn::SEAT_ROOT_Z],
        facing: 32_768,
    });
    bench.into_iter().chain(chairs).collect()
}

/// Bram (NPC 3) closes his shop at night (#158, Atlas 5.1.47), and so does Trader Maren at Fenford
/// (NPC 42, #826).
pub(crate) fn closed_at_night() -> Vec<NpcId> {
    vec![NpcId::new(3), NpcId::new(42)]
}
