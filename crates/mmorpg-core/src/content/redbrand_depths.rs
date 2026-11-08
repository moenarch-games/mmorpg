//! The Redbrand Depths (#404 part 2): a shared authored dungeon whose every run is one private
//! zone. An entrance hall, three escalating chambers, a checkpoint alcove between the first two
//! and the chamber of Captain Ulric Thorn, the run's boss, who appears once the others are clear.
//!
//! Stand-in rock boxes, like the public cavern; authored assets come later. Zone IDs
//! [`ZONE_IDS`] are instance slots: a host runs a pool of them and a run binds one (the run
//! directory in `mmorpg-control-plane` owns which). One content revision serves every party
//! size; the size only selects creatures ([`crate::DepthsSpawn::party`]).

use std::sync::{Arc, LazyLock};

use super::greyhaven_vale::{self, quests, units};
use crate::{
    Area, AreaId, CreatureSpawn, DepthsChamber, DepthsSite, DepthsSpawn, HandoffCorridor,
    STANDARD_GRAVITY, SpawnGrid, StaticCollider, XzBounds, ZoneAreas, ZoneContent, ZoneDefinition,
    ZoneId,
};

/// The instance-slot zone IDs a host may run; content for each is identical.
pub const ZONE_IDS: core::ops::Range<u32> = 20_000..20_032;

/// Whether `zone` is an instance slot of the depths.
#[must_use]
pub const fn is_depths_zone(zone: ZoneId) -> bool {
    zone.get() >= ZONE_IDS.start && zone.get() < ZONE_IDS.end
}

const FLOOR_TOP: i32 = 0;
const WALL_THICKNESS: i32 = 100;
const WALL_HEIGHT: i32 = 400;
const PASSAGE_HALF_WIDTH: i32 = 300;
/// Facing east, along the length of the dungeon.
const EAST: u16 = 16_384;
const WEST: u16 = 49_152;
const WANDER_RADIUS: i32 = 300;

/// Body centre of an arriving character: just inside the entrance hall, on its axis.
pub const ARRIVAL: [i32; 3] = [-8_800, FLOOR_TOP + crate::PLAYER_HALF_EXTENTS_UNITS[1], 0];
/// An arriving character faces into the hall.
pub const ARRIVAL_FACING: u16 = EAST;
/// Feet XZ where released players stand before the checkpoint unlocks: by the entrance.
pub const ENTRANCE: [i32; 2] = [-8_500, 900];
/// Feet XZ of the checkpoint, in the alcove between the first and second chambers.
pub const CHECKPOINT: [i32; 2] = [1_000, 0];
/// The zone's graveyard, by the entrance.
pub const GRAVEYARD: [i32; 2] = [-8_500, 900];

/// An interior room, `(min, max)` XZ in units, open floor between walls.
#[derive(Clone, Copy)]
struct Room {
    min: [i32; 2],
    max: [i32; 2],
}

const ENTRANCE_HALL: Room = Room {
    min: [-10_000, -1_100],
    max: [-4_200, 1_100],
};
const CHAMBER_1: Room = Room {
    min: [-3_800, -1_600],
    max: [0, 1_600],
};
const CHECKPOINT_ROOM: Room = Room {
    min: [400, -800],
    max: [1_600, 800],
};
const CHAMBER_2: Room = Room {
    min: [2_000, -1_800],
    max: [5_800, 1_800],
};
const CHAMBER_3: Room = Room {
    min: [6_200, -2_200],
    max: [10_400, 2_200],
};
const BOSS_CHAMBER: Room = Room {
    min: [10_800, -2_400],
    max: [14_800, 2_400],
};

/// Captain Ulric Thorn's creature ID in every run.
pub const THORN: crate::CreatureId = crate::CreatureId::new(30);
/// The template of Captain Ulric Thorn (#404 part 3), level 6 elite.
pub const THORN_TEMPLATE: crate::CreatureTemplateId = crate::CreatureTemplateId::new(16);
/// The Need/Greed window of his drops: 30 s, tick-based (Atlas 5.1.34).
pub const ROLL_TICKS: u32 = crate::long_ticks_from_ms(30_000);
/// Larger parties (three or more) face his second area attack in his last phase.
pub const FINAL_ATTACK_PARTY: u8 = 3;

/// A wall from `min` to `max` on the floor, `(min, max)` over XZ.
fn wall(id: &mut u32, min: [i32; 2], max: [i32; 2]) -> StaticCollider {
    let half = [
        (max[0] - min[0]) / 2,
        WALL_HEIGHT / 2,
        (max[1] - min[1]) / 2,
    ];
    let collider = StaticCollider {
        id: *id,
        position: [min[0] + half[0], FLOOR_TOP + half[1], min[1] + half[2]],
        half_extents: half,
    };
    *id += 1;
    collider
}

/// The four walls around `room`, each split around an optional door gap of [`PASSAGE_HALF_WIDTH`]
/// across the axis named by `(east, west)` (`z` of the gap on that wall, `None` closes it).
fn room_walls(
    id: &mut u32,
    out: &mut Vec<StaticCollider>,
    room: Room,
    west_door: Option<i32>,
    east_door: Option<i32>,
) {
    let (min, max) = (room.min, room.max);
    let t = WALL_THICKNESS;
    let north = |id: &mut u32| wall(id, [min[0] - t, min[1] - t], [max[0] + t, min[1]]);
    let south = |id: &mut u32| wall(id, [min[0] - t, max[1]], [max[0] + t, max[1] + t]);
    let side = |id: &mut u32, x: [i32; 2], door: Option<i32>, out: &mut Vec<_>| match door {
        None => {
            let collider = wall(id, [x[0], min[1] - t], [x[1], max[1] + t]);
            out.push(collider);
        }
        Some(z) => {
            let low = wall(id, [x[0], min[1] - t], [x[1], z - PASSAGE_HALF_WIDTH]);
            let high = wall(id, [x[0], z + PASSAGE_HALF_WIDTH], [x[1], max[1] + t]);
            out.push(low);
            out.push(high);
        }
    };
    let n = north(id);
    out.push(n);
    let s = south(id);
    out.push(s);
    side(id, [min[0] - t, min[0]], west_door, out);
    side(id, [max[0], max[0] + t], east_door, out);
}

/// Side walls of a straight passage along +X from `x0` to `x1`, centred on `z`.
fn passage(id: &mut u32, out: &mut Vec<StaticCollider>, x0: i32, x1: i32, z: i32) {
    let t = WALL_THICKNESS;
    let hw = PASSAGE_HALF_WIDTH;
    out.push(wall(id, [x0, z - hw - t], [x1, z - hw]));
    out.push(wall(id, [x0, z + hw], [x1, z + hw + t]));
}

fn colliders() -> Vec<StaticCollider> {
    let mut id = 1_u32;
    let mut out = vec![StaticCollider {
        id,
        position: [2_400, FLOOR_TOP - 50, 0],
        half_extents: [13_000, 50, 3_000],
    }];
    id += 1;
    // Entrance hall: an exit corridor runs along its west wall (no door gaps).
    room_walls(&mut id, &mut out, ENTRANCE_HALL, None, Some(0));
    passage(&mut id, &mut out, -4_200, -3_800, 0);
    room_walls(&mut id, &mut out, CHAMBER_1, Some(0), Some(0));
    passage(&mut id, &mut out, 0, 400, 0);
    room_walls(&mut id, &mut out, CHECKPOINT_ROOM, Some(0), Some(0));
    passage(&mut id, &mut out, 1_600, 2_000, 0);
    room_walls(&mut id, &mut out, CHAMBER_2, Some(0), Some(0));
    passage(&mut id, &mut out, 5_800, 6_200, 0);
    room_walls(&mut id, &mut out, CHAMBER_3, Some(0), Some(0));
    passage(&mut id, &mut out, 10_400, 10_800, 0);
    room_walls(&mut id, &mut out, BOSS_CHAMBER, Some(0), None);
    out
}

/// `(creature, template, feet XZ, facing, party sizes, reinforcement after n fallen)`.
type SpawnRow = (
    u32,
    crate::CreatureTemplateId,
    [i32; 2],
    u16,
    [u8; 2],
    Option<u8>,
);

/// Levels 4–5 like the public cavern; Brutes stand at their one level.
const SPAWN_LEVELS: [u8; 2] = [4, 5];

fn chamber_table() -> Vec<(&'static str, Vec<SpawnRow>)> {
    use units::templates::{REDBRAND_BANDIT, REDBRAND_BRUTE, REDBRAND_CROSSBOWMAN};
    vec![
        (
            "The Lookouts' Gallery",
            vec![
                (1, REDBRAND_BANDIT, [-2_600, -700], EAST, [1, 5], None),
                (2, REDBRAND_BANDIT, [-2_600, 700], EAST, [1, 5], None),
                (3, REDBRAND_BANDIT, [-1_400, 0], EAST, [2, 5], None),
                (4, REDBRAND_CROSSBOWMAN, [-400, -800], WEST, [3, 5], None),
                (5, REDBRAND_BANDIT, [-1_400, -1_000], EAST, [4, 5], None),
                (6, REDBRAND_CROSSBOWMAN, [-400, 800], WEST, [5, 5], None),
            ],
        ),
        (
            "The Guard Room",
            vec![
                (7, REDBRAND_BRUTE, [3_500, 0], WEST, [1, 5], None),
                (8, REDBRAND_BANDIT, [4_400, -900], WEST, [1, 5], None),
                (9, REDBRAND_BANDIT, [4_400, 900], WEST, [2, 5], None),
                (10, REDBRAND_CROSSBOWMAN, [5_400, 0], WEST, [2, 5], None),
                (11, REDBRAND_BRUTE, [3_000, -1_100], EAST, [3, 5], None),
                (12, REDBRAND_BANDIT, [3_000, 1_100], EAST, [4, 5], None),
                // Reinforcements rush in once two of the room's defenders have fallen.
                (13, REDBRAND_BANDIT, [5_600, -1_500], WEST, [4, 5], Some(2)),
                (
                    14,
                    REDBRAND_CROSSBOWMAN,
                    [5_600, 1_500],
                    WEST,
                    [5, 5],
                    Some(2),
                ),
            ],
        ),
        (
            "The Captain's Guard",
            vec![
                (15, REDBRAND_BRUTE, [7_700, 0], WEST, [1, 5], None),
                (
                    16,
                    REDBRAND_CROSSBOWMAN,
                    [9_000, -1_400],
                    WEST,
                    [1, 5],
                    None,
                ),
                (17, REDBRAND_BANDIT, [7_700, -1_300], WEST, [1, 5], None),
                (18, REDBRAND_BANDIT, [7_700, 1_300], WEST, [2, 5], None),
                (19, REDBRAND_BRUTE, [8_900, 1_200], WEST, [2, 5], None),
                (20, REDBRAND_CROSSBOWMAN, [10_000, 0], WEST, [3, 5], None),
                (21, REDBRAND_BANDIT, [6_800, -1_400], EAST, [3, 5], Some(3)),
                (22, REDBRAND_BRUTE, [6_800, 1_400], EAST, [4, 5], Some(3)),
                (
                    23,
                    REDBRAND_CROSSBOWMAN,
                    [10_100, -1_900],
                    WEST,
                    [5, 5],
                    Some(4),
                ),
            ],
        ),
    ]
}

/// Thorn's adds: `(creature, template, feet XZ, party sizes)`. The composition by party size is
/// two adds for one or two players, three for three or four, four for five (5.1.26).
fn thorn_adds() -> Vec<(u32, crate::CreatureTemplateId, [i32; 2], [u8; 2])> {
    use units::templates::{REDBRAND_BANDIT, REDBRAND_BRUTE, REDBRAND_CROSSBOWMAN};
    vec![
        (31, REDBRAND_BANDIT, [12_400, -700], [1, 5]),
        (32, REDBRAND_BANDIT, [12_400, 700], [1, 5]),
        (33, REDBRAND_CROSSBOWMAN, [11_400, -1_600], [3, 5]),
        (34, REDBRAND_BRUTE, [11_400, 1_600], [5, 5]),
    ]
}

fn spawns() -> Vec<CreatureSpawn> {
    let spawn = |id, template, position, facing, wander_radius| CreatureSpawn {
        id: crate::CreatureId::new(id),
        template,
        position,
        facing,
        wander_radius,
        levels: (template != units::templates::REDBRAND_BRUTE && template != THORN_TEMPLATE)
            .then_some(SPAWN_LEVELS),
    };
    chamber_table()
        .into_iter()
        .flat_map(|(_, rows)| rows)
        .map(|(id, template, position, facing, _, _)| {
            spawn(id, template, position, facing, WANDER_RADIUS)
        })
        .chain(std::iter::once(spawn(
            THORN.get(),
            THORN_TEMPLATE,
            [13_400, 0],
            WEST,
            0,
        )))
        .chain(
            thorn_adds()
                .into_iter()
                .map(|(id, template, position, _)| spawn(id, template, position, WEST, 0)),
        )
        .collect()
}

/// Captain Ulric Thorn: a veteran in the band's crimson, a little larger than his men.
fn thorn_template() -> crate::CreatureTemplate {
    use units::templates::GARRICK_REDBRAND;
    let mut thorn = units::creature_templates()
        .into_iter()
        .find(|template| template.id == GARRICK_REDBRAND)
        .expect("Garrick's template stands behind Thorn's");
    thorn.id = THORN_TEMPLATE;
    thorn.name = "Captain Ulric Thorn".to_owned();
    thorn.min_level = 6;
    thorn.max_level = 6;
    thorn.elite = true;
    thorn.health = 900;
    thorn.health_per_level = 0;
    thorn.damage = [6, 10];
    thorn.damage_per_level = 0;
    thorn.half_extents = [42, 104, 42];
    thorn
}

/// Thorn is scaled by adds and mechanics, not multipliers (5.1.26); these rules only make him
/// an ordinary boss to the shared encounter code (locking, contribution, evade resets).
fn thorn_rules() -> crate::BossRules {
    crate::BossRules {
        template: THORN_TEMPLATE,
        health_percent_per_extra: 0,
        damage_percent_per_extra: 0,
        engage_radius: 1,
        reaction_radius: 1,
        reaction_ticks: 1,
        rally_point: [13_400, 0],
    }
}

/// The authored encounter table of the depths.
#[must_use]
pub fn site() -> DepthsSite {
    DepthsSite {
        chambers: chamber_table()
            .into_iter()
            .map(|(name, rows)| DepthsChamber {
                name: name.to_owned(),
                spawns: rows
                    .into_iter()
                    .map(|(id, _, _, _, party, after_fallen)| DepthsSpawn {
                        creature: crate::CreatureId::new(id),
                        party,
                        after_fallen,
                    })
                    .collect(),
            })
            .collect(),
        entrance: ENTRANCE,
        checkpoint: CHECKPOINT,
        boss: Some(crate::DepthsBoss {
            creature: THORN,
            area: XzBounds {
                min: BOSS_CHAMBER.min,
                max: BOSS_CHAMBER.max,
            },
            adds: thorn_adds()
                .into_iter()
                .map(|(id, _, _, party)| DepthsSpawn {
                    creature: crate::CreatureId::new(id),
                    party,
                    after_fallen: None,
                })
                .collect(),
            pool: vec![
                crate::ItemId::new(15),
                crate::ItemId::new(16),
                crate::ItemId::new(17),
            ],
            drops_by_party: [1, 1, 2, 2, 2],
            roll_ticks: ROLL_TICKS,
            area_attack: crate::ability::ids::REAVERS_SWEEP,
            final_attack: crate::ability::ids::LAST_COMMAND,
            final_attack_party: FINAL_ATTACK_PARTY,
        }),
    }
}

/// Whether `template` is one a depths spawn can use: only the Redbrands appear.
fn is_depths_template(template: crate::CreatureTemplateId) -> bool {
    use units::templates::{REDBRAND_BANDIT, REDBRAND_BRUTE, REDBRAND_CROSSBOWMAN};
    matches!(
        template,
        REDBRAND_BANDIT | REDBRAND_BRUTE | REDBRAND_CROSSBOWMAN
    )
}

/// The exit corridor along the entrance hall's west wall (#404): crossing it returns a
/// character to the public cave's north mouth, through the same fenced transfer as a seam.
const fn exit_corridor() -> HandoffCorridor {
    HandoffCorridor {
        neighbor: crate::GREYHAVEN_ZONE,
        corridor: XzBounds {
            min: [ENTRANCE_HALL.min[0], -600],
            max: [-9_000, 600],
        },
        crossing: XzBounds {
            min: [ENTRANCE_HALL.min[0], -150],
            max: [-9_800, 150],
        },
        offset: [
            greyhaven_vale::cave::EXIT_OFFSET[0],
            greyhaven_vale::cave::GREYFANG.floor_y - FLOOR_TOP,
            greyhaven_vale::cave::EXIT_OFFSET[1],
        ],
    }
}

static CONTENT: LazyLock<Arc<ZoneContent>> =
    LazyLock::new(|| build().expect("built-in Redbrand Depths content is valid"));

fn build() -> Result<Arc<ZoneContent>, crate::ZoneError> {
    let definition = ZoneDefinition::with_spawn_grid(
        STANDARD_GRAVITY,
        SpawnGrid {
            origin: [-8_400, -750],
            columns: 32,
            spacing: 100,
        },
        colliders(),
    )?;
    let mut templates: Vec<_> = units::creature_templates()
        .into_iter()
        .filter(|template| is_depths_template(template.id))
        .collect();
    templates.push(thorn_template());
    let content = ZoneContent::new(
        definition,
        ZoneAreas::new(vec![Area::new(
            AreaId::new(1),
            "Redbrand Depths",
            [-10_000, -2_400],
            [14_800, 2_400],
        )?])?,
        templates,
        spawns(),
        Vec::new(),
        GRAVEYARD,
    )?;
    // Thorn's corpse has no ordinary loot: his shared Need/Greed pool is the reward (#404).
    let tables = content
        .creature_templates()
        .iter()
        .filter(|template| template.id != THORN_TEMPLATE)
        .map(|template| {
            (
                template.id,
                crate::loot_table(template.id)
                    .expect("Redbrand templates have authored loot tables")
                    .clone(),
            )
        })
        .collect();
    let abilities = units::creature_abilities()
        .into_iter()
        .filter(|(template, _)| is_depths_template(*template))
        .collect();
    Ok(Arc::new(
        content
            .with_loot_tables(tables)?
            .with_creature_abilities(abilities)?
            .with_shielded(units::shielded())?
            .with_bosses(vec![thorn_rules()])?
            .with_carried_quests(quests::quests())
            .with_handoff_corridors(vec![exit_corridor()])?
            .with_depths_site(site())?,
    ))
}

/// The depths content, one revision for every party size and slot.
#[must_use]
pub fn content() -> Arc<ZoneContent> {
    Arc::clone(&CONTENT)
}
