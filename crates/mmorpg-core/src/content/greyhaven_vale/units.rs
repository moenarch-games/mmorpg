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
use crate::{CreatureId, CreatureTemplateId, NpcId};

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
}

/// Default respawn time: 60 s.
pub const RESPAWN_TICKS: u32 = 1_800;
/// Garrick Redbrand respawns after 5 min.
pub const GARRICK_RESPAWN_TICKS: u32 = 9_000;
/// Idle creatures wander within 6 m of their spawn point.
pub const WANDER_RADIUS_UNITS: i32 = 600;
/// Released spirits return beside the graveyard, south of the gravestones'
/// western end and clear of houses.
pub const GRAVEYARD: [i32; 2] = [-2_000, 4_150];

const WOLF: [i32; 3] = [40, 45, 40];
const BOAR: [i32; 3] = [45, 45, 45];
const RAT: [i32; 3] = [25, 20, 25];
const HUMANOID: [i32; 3] = [30, 90, 30];
const LURKER: [i32; 3] = [35, 80, 35];
const GARRICK: [i32; 3] = [40, 100, 40];

/// `(id, name, family, behaviour, levels, elite, health, health/level,
/// damage, damage/level, swing ticks, respawn ticks, half extents)`.
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
const TEMPLATES: [TemplateRow; 7] = {
    use CreatureBehaviour::{Aggressive, Neutral};
    use CreatureFamily::{Boar, Marauder, Mirefin, Redbrand, Vermin, Wolf};
    use templates::*;
    [
        (TIMBER_WOLF, "Timber Wolf", Wolf, Aggressive, [1, 2], false, 42, 14, [2, 4], 1, 60, RESPAWN_TICKS, WOLF),
        (YOUNG_BOAR, "Young Boar", Boar, Neutral, [1, 2], false, 48, 14, [2, 4], 0, 66, RESPAWN_TICKS, BOAR),
        (GRAIN_RAT, "Grain Rat", Vermin, Neutral, [1, 1], false, 30, 0, [1, 3], 0, 48, RESPAWN_TICKS, RAT),
        (FIELD_MARAUDER, "Field Marauder", Marauder, Aggressive, [2, 3], false, 60, 16, [3, 6], 0, 66, RESPAWN_TICKS, HUMANOID),
        (MIREFIN_LURKER, "Mirefin Lurker", Mirefin, Aggressive, [3, 3], false, 70, 0, [3, 6], 0, 60, RESPAWN_TICKS, LURKER),
        (REDBRAND_BANDIT, "Redbrand Bandit", Redbrand, Aggressive, [3, 5], false, 75, 16, [4, 7], 0, 66, RESPAWN_TICKS, HUMANOID),
        (GARRICK_REDBRAND, "Garrick Redbrand", Redbrand, Aggressive, [5, 5], true, 420, 0, [8, 14], 0, 72, GARRICK_RESPAWN_TICKS, GARRICK),
    ]
};

/// `(spawn id, template, feet XZ, facing)`. IDs group by subzone:
/// 100s Wolfrun Woods, 200s Millbrook Farm, 300s Stillwater Lake, 400s
/// Redbrand Hollow.
#[rustfmt::skip]
const SPAWNS: [(u32, CreatureTemplateId, [i32; 2], u16); 59] = {
    use templates::*;
    [
        // Wolfrun Woods: timber wolves on trunk-grid corners. Wolf 108 guards
        // the den clearing where the Wolfrun Trail ends; the others keep
        // more than 12 m from the trail and from it.
        (100, TIMBER_WOLF, [-10_250, -3_600], 0),
        (101, TIMBER_WOLF, [-9_350, -3_150], 8_192),
        (102, TIMBER_WOLF, [-8_000, -3_600], 16_384),
        (103, TIMBER_WOLF, [-6_650, -3_150], 24_576),
        (104, TIMBER_WOLF, [-5_300, -3_600], 32_768),
        (105, TIMBER_WOLF, [-10_700, -1_800], 40_960),
        (106, TIMBER_WOLF, [-9_350, -1_350], 49_152),
        (107, TIMBER_WOLF, [-6_200, -1_800], 57_344),
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
        (125, YOUNG_BOAR, [-9_350, -4_050], 24_576),
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
        // Stillwater Lake: mirefin lurkers in and around the shallow water.
        (300, MIREFIN_LURKER, [4_200, -5_200], 16_384),
        (301, MIREFIN_LURKER, [5_000, -4_300], 32_768),
        (302, MIREFIN_LURKER, [6_200, -4_400], 32_768),
        (303, MIREFIN_LURKER, [7_200, -5_000], 49_152),
        (304, MIREFIN_LURKER, [7_000, -6_300], 57_344),
        (305, MIREFIN_LURKER, [5_800, -6_800], 0),
        (306, MIREFIN_LURKER, [4_600, -6_300], 8_192),
        (307, MIREFIN_LURKER, [5_500, -5_500], 24_576),
        // Redbrand Hollow: the bandit camp between the cliffs.
        (400, REDBRAND_BANDIT, [-1_000, -7_800], 0),
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
        (413, REDBRAND_BANDIT, [-1_500, -7_700], 16_384),
        // Garrick Redbrand holds the mine entrance.
        (450, GARRICK_REDBRAND, [0, -10_500], 0),
    ]
};

/// The four gate guards share one name.
const GUARD: &str = "Greyhaven Guard";

/// `(id, name, role, level, feet XZ, facing)`.
type NpcRow = (u32, &'static str, NpcRole, u8, [i32; 2], u16);

#[rustfmt::skip]
const NPCS: [NpcRow; 9] = [
    (1, "Marshal Elden Greywatch", NpcRole::QuestGiver, 10, [-2_200, 900], 0),
    (2, "Tanner Hilda Brook", NpcRole::QuestGiver, 5, [-2_400, 3_150], 32_768),
    (3, "Innkeeper Bram Tolliver", NpcRole::Vendor, 5, [1_500, 700], 0),
    (4, "Farmer Osric Mill", NpcRole::QuestGiver, 4, [6_200, 4_150], 32_768),
    (5, "Brother Aldous", NpcRole::SpiritHealer, 10, [-1_650, 4_550], 49_152),
    // Guards stand inside the palisade beside the north, south, west and
    // east gates, facing out.
    (6, GUARD, NpcRole::Guard, 10, [-500, -800], 32_768),
    (7, GUARD, NpcRole::Guard, 10, [500, 4_800], 0),
    (8, GUARD, NpcRole::Guard, 10, [-2_900, 1_500], 49_152),
    (9, GUARD, NpcRole::Guard, 10, [2_900, 2_500], 16_384),
];

pub(super) fn creature_templates() -> Vec<CreatureTemplate> {
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
            position,
            facing,
            wander_radius: WANDER_RADIUS_UNITS,
        })
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
        .collect()
}

/// Creature abilities of this revision: Garrick's kit belongs to the quests
/// and boss step.
#[must_use]
pub fn creature_abilities() -> Vec<(CreatureTemplateId, crate::AbilityId)> {
    vec![
        (templates::MIREFIN_LURKER, crate::ability::ids::MUCK_BOLT),
        (
            templates::REDBRAND_BANDIT,
            crate::ability::ids::CRUDE_BANDAGE,
        ),
    ]
}

/// Innkeeper Bram Tolliver (NPC 3) sells the starter stock.
pub(crate) fn vendors() -> Vec<(NpcId, crate::VendorStock)> {
    vec![(NpcId::new(3), crate::starter_vendor_stock())]
}
