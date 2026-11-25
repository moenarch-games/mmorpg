//! Greyhaven's people and their days (#819, Atlas 5.1.45–5.1.47). Each resident works at its
//! post by day, walks home at dusk and back at dawn ([`crate::NpcRoutine`]), and runs errands
//! at set hours ([`crate::Errand`]): to the well, the alehouse, the market, the fields, the
//! fishing shore or the woods. The Outpost's residents (NPCs 10–20) live in its houses; the
//! region's settlements (#818) and the farmland's hamlets (#424) have their own people
//! (NPCs 22–41).
//!
//! Every point here is an open spot: a doorstep a little in front of a house, a well's side, a
//! road's line. Placement is a pure function of the zone tick, so no state needs saving.

use crate::{Errand, HOUR_TICKS, Npc, NpcId, NpcRole, NpcRoutine};

const SOUTH: u16 = 0;
const EAST: u16 = 16_384;
const NORTH: u16 = 32_768;
const WEST: u16 = 49_152;

/// `(id, name, level, feet XZ, facing)` of the region's new residents.
#[rustfmt::skip]
const NEW: [(u32, &str, u8, [i32; 2], u16); 21] = [
    // Barrowdown (#818).
    (22, "Alewife Gwen Barrow", 4, [55_800, 67_400], EAST),
    (23, "Shepherd Corin", 3, [53_000, 70_600], SOUTH),
    (24, "Old Maud", 5, [48_800, 60_600], WEST),
    (25, "Farmer Tobin Hale", 3, [53_400, 68_000], WEST),
    (26, "Pip", 1, [54_400, 64_200], SOUTH),
    // Fenford (#818).
    (27, "Innkeeper Hob Ferris", 4, [105_300, 20_000], EAST),
    (28, "Miller Wynne", 3, [101_800, 23_150], SOUTH),
    (29, "Ferryman Osk", 4, [107_000, 26_000], SOUTH),
    (30, "Netter Ilsa", 2, [104_000, 21_200], WEST),
    // Westmere Lodge (#818).
    (31, "Fisher Brann", 3, [-69_600, 5_000], WEST),
    // The hunters' camp in Southwood (#818).
    (32, "Huntsman Edric Vale", 6, [-48_200, 79_250], SOUTH),
    (33, "Trapper Sela", 4, [-49_000, 78_900], WEST),
    // Greyfang Watch (#818).
    (34, "Watchman Garth", 8, [13_200, -88_400], NORTH),
    (35, "Watchman Pell", 8, [11_600, -86_000], EAST),
    // The farmland's hamlets (#424).
    (36, "Goodwife Ena", 2, [-8_800, 35_300], EAST),
    (37, "Ploughman Garr", 3, [-12_800, 32_000], WEST),
    (38, "Old Bertil", 4, [12_000, 46_100], EAST),
    (39, "Shepherd Lise", 2, [15_200, 47_200], EAST),
    (40, "Miller Hanne", 3, [-4_350, 56_150], SOUTH),
    (41, "Farmhand Jory", 2, [-6_400, 54_000], WEST),
    // Fenford's trader (#826).
    (42, "Trader Maren", 4, [102_000, 20_600], EAST),
];

/// The region's new residents.
pub(super) fn npcs() -> Vec<Npc> {
    NEW.iter()
        .map(|&(id, name, level, position, facing)| Npc {
            id: NpcId::new(id),
            name: name.to_owned(),
            // The region's quest givers (#820) and the hamlets' (#859, Old Bertil, Old Maud, Miller
            // Wynne, Ferryman Osk and Netter Ilsa since #1019), then Greyfang Watch's other
            // watchman.
            role: match id {
                22 | 24 | 27..=32 | 34 | 36 | 38 | 39 | 40 => NpcRole::QuestGiver,
                35 => NpcRole::Guard,
                42 => NpcRole::Vendor,
                _ => NpcRole::Resident,
            },
            level,
            position,
            facing,
        })
        .collect()
}

/// `tenths` of an hour after midnight, in ticks.
const fn at(tenths: u32) -> u32 {
    tenths * HOUR_TICKS / 10
}

/// An errand leaving at `leave` (tenths of an hour), walking `path`, staying `stay` tenths of an
/// hour facing `facing`.
fn errand(leave: u32, path: &[[i32; 2]], stay: u32, facing: u16) -> Errand {
    Errand {
        leave: at(leave),
        path: path.to_vec(),
        stay: at(stay),
        facing,
        seat: None,
    }
}

/// A routine home along `home` (ending on the doorstep), facing `facing` there, with errands.
fn routine(npc: u32, home: &[[i32; 2]], facing: u16, errands: Vec<Errand>) -> NpcRoutine {
    NpcRoutine {
        npc: NpcId::new(npc),
        path: home.to_vec(),
        home_facing: facing,
        door: None,
        home_seat: None,
        work_seat: None,
        errands,
    }
}

/// The Outpost's well, by the bridge over the brook.
const OUTPOST_WELL: [i32; 2] = [-400, 2_700];
/// In front of the inn's door.
const INN_FRONT: [i32; 2] = [1_450, 620];
/// Doorsteps on the square's west and east houses.
const WEST_HOUSE: [i32; 2] = [-2_400, 1_840];
const EAST_HOUSE: [i32; 2] = [2_400, 1_840];

/// Every resident's day as authored, its walk home ending outside.
#[allow(clippy::too_many_lines)]
fn table() -> Vec<NpcRoutine> {
    vec![
        // The Outpost (NPCs 10–20): homes along the square's edges.
        routine(
            10,
            &[WEST_HOUSE],
            NORTH,
            vec![errand(120, &[[-1_200, 2_300], OUTPOST_WELL], 8, EAST)],
        ),
        routine(
            11,
            &[EAST_HOUSE],
            NORTH,
            vec![errand(95, &[[1_000, 1_100], [200, -600]], 12, NORTH)],
        ),
        routine(
            12,
            &[[650, 3_400], [1_200, 3_350]],
            SOUTH,
            vec![errand(
                140,
                &[[650, 3_400], [1_350, 3_300], [1_600, 2_600]],
                10,
                EAST,
            )],
        ),
        routine(
            13,
            &[[-1_100, 3_380]],
            SOUTH,
            vec![errand(90, &[[0, 1_800], [500, 800], INN_FRONT], 15, NORTH)],
        ),
        routine(
            14,
            &[[1_760, 2_950], [1_760, 3_230], [2_400, 3_230]],
            SOUTH,
            vec![errand(110, &[[800, 2_600], OUTPOST_WELL], 6, WEST)],
        ),
        // The miller walks to Millbrook Farm's windmill for the working day, up the Millbrook
        // Climb.
        routine(
            15,
            &[[1_700, 2_000], [1_700, 800], [2_300, 720]],
            EAST,
            vec![errand(
                75,
                &[
                    [1_600, 2_000],
                    [3_200, 2_000],
                    [4_200, 2_000],
                    [4_200, -1_800],
                    [5_400, -1_800],
                    [5_400, 1_200],
                    [5_400, 2_800],
                    [6_400, 3_300],
                    [7_400, 3_500],
                    [8_100, 2_550],
                ],
                75,
                EAST,
            )],
        ),
        // The herbalist gathers by the South Road below the gate each morning.
        routine(
            16,
            &[[1_200, 3_350]],
            SOUTH,
            vec![errand(
                70,
                &[[500, 2_600], [0, 4_500], [0, 5_600], [-600, 6_400]],
                30,
                WEST,
            )],
        ),
        // The peddler greets travellers at the south gate, then calls at the inn.
        routine(
            17,
            &[[1_650, 2_850], [1_650, 3_230], [2_400, 3_230]],
            SOUTH,
            vec![
                errand(90, &[[500, 2_600], [0, 4_500]], 20, SOUTH),
                errand(150, &[[1_000, 1_800], INN_FRONT], 15, NORTH),
            ],
        ),
        routine(
            18,
            &[[-1_750, 3_150], [-1_100, 3_360]],
            SOUTH,
            vec![errand(130, &[[-1_000, 2_600], OUTPOST_WELL], 8, EAST)],
        ),
        // The porter waits for loads at the north gate, then at the east gate. Errands start
        // after 08:40, when the zone's first scripted visitors have left the square.
        routine(
            19,
            &[WEST_HOUSE],
            NORTH,
            vec![
                errand(90, &[[-1_200, 1_600], [0, 600], [0, -700]], 20, NORTH),
                errand(140, &[[0, 1_900], [2_700, 2_000]], 20, EAST),
            ],
        ),
        // The washer does the washing at the brook below the bridge.
        routine(
            20,
            &[[1_700, 3_260], [2_400, 3_250]],
            SOUTH,
            vec![errand(100, &[[700, 3_300], [650, 3_650]], 15, WEST)],
        ),
        // Farmer Osric Mill sleeps in Millbrook's farmhouse, behind his post (#886).
        routine(4, &[[6_200, 4_150]], SOUTH, Vec::new()),
        // Barrowdown.
        routine(
            22,
            &[[55_700, 66_800], [56_400, 66_800]],
            SOUTH,
            vec![errand(130, &[[54_400, 65_000]], 5, WEST)],
        ),
        routine(
            23,
            &[[53_000, 68_000], [51_600, 67_050]],
            NORTH,
            vec![errand(125, &[[54_000, 68_200], [55_400, 67_400]], 10, EAST)],
        ),
        routine(
            24,
            &[[49_000, 62_000], [50_600, 62_400], [51_600, 62_950]],
            SOUTH,
            Vec::new(),
        ),
        routine(
            25,
            &[[53_000, 67_800], [53_000, 66_000], [56_600, 64_250]],
            NORTH,
            vec![errand(120, &[[54_400, 65_400]], 6, EAST)],
        ),
        routine(
            26,
            &[[52_400, 63_000], [51_600, 62_950]],
            SOUTH,
            vec![
                errand(100, &[[50_000, 61_800], [49_200, 60_400]], 10, WEST),
                errand(150, &[[55_000, 66_800], [55_400, 67_000]], 8, EAST),
            ],
        ),
        // Fenford.
        // The innkeeper sleeps at the Ford Inn (#886).
        routine(27, &[[105_300, 20_600]], EAST, Vec::new()),
        // Maren keeps her stall by the square and shuts it for the night (#826).
        routine(42, &[[100_200, 21_950]], SOUTH, Vec::new()),
        routine(
            28,
            &[[100_750, 23_000], [100_750, 22_400]],
            EAST,
            vec![errand(120, &[[102_800, 21_400]], 6, EAST)],
        ),
        routine(
            29,
            &[[105_000, 23_800], [104_000, 20_600], [100_750, 19_200]],
            EAST,
            vec![errand(
                140,
                &[[105_000, 23_800], [105_300, 20_700]],
                10,
                EAST,
            )],
        ),
        routine(
            30,
            &[[103_600, 19_650]],
            NORTH,
            vec![errand(
                110,
                &[[105_000, 20_800], [105_300, 20_400]],
                10,
                EAST,
            )],
        ),
        // Westmere Lodge: the fisher eats at the fire at noon.
        routine(
            31,
            &[[-66_000, 4_800], [-64_400, 4_400]],
            NORTH,
            vec![errand(120, &[[-66_600, 5_000]], 6, SOUTH)],
        ),
        // The hunters' camp: the trapper walks her lines in the woods by day.
        routine(32, &[[-46_400, 79_950]], SOUTH, Vec::new()),
        routine(
            33,
            &[[-47_400, 80_000], [-46_400, 79_950]],
            SOUTH,
            vec![errand(
                85,
                &[[-49_000, 77_600], [-51_000, 76_000], [-53_600, 74_200]],
                30,
                WEST,
            )],
        ),
        // Greyfang Watch: Pell walks the road north and back twice a day.
        routine(
            35,
            &[[12_400, -86_000]],
            SOUTH,
            vec![
                errand(90, &[[14_600, -88_000], [16_000, -90_000]], 10, NORTH),
                errand(150, &[[14_600, -88_000], [16_000, -90_000]], 10, NORTH),
            ],
        ),
        // The farmland's hamlets. The goodwife, Old Bertil and the farmhand live in their
        // hamlets' farmhouses (#886).
        routine(
            36,
            &[[-8_800, 34_800], [-7_900, 34_800]],
            NORTH,
            vec![errand(
                130,
                &[[-8_800, 35_100], [-8_000, 35_100], [-7_350, 35_600]],
                8,
                SOUTH,
            )],
        ),
        routine(37, &[[-11_200, 32_800], [-9_600, 33_000]], WEST, Vec::new()),
        routine(
            38,
            &[[12_000, 45_600]],
            NORTH,
            vec![errand(
                120,
                &[[12_000, 45_800], [13_000, 45_800], [13_000, 44_200]],
                6,
                EAST,
            )],
        ),
        routine(
            39,
            &[[14_000, 46_400], [14_000, 46_000]],
            NORTH,
            vec![errand(125, &[[12_800, 46_500], [12_200, 46_500]], 6, WEST)],
        ),
        routine(
            40,
            &[[-3_400, 56_000], [-3_400, 55_500]],
            NORTH,
            vec![errand(120, &[[-5_000, 54_200], [-6_200, 53_600]], 6, WEST)],
        ),
        routine(
            41,
            &[[-5_600, 54_000]],
            NORTH,
            vec![errand(140, &[[-6_400, 55_600], [-4_700, 56_000]], 10, EAST)],
        ),
    ]
}

/// Every resident's day. A walk home that ends at a walk-in cottage's doorstep (#843) carries on
/// through the doorway to the bed, or for the cottage's second resident to the hearth.
pub(super) fn routines() -> Vec<NpcRoutine> {
    let mut routines = table();
    let mut housed: Vec<u32> = Vec::new();
    for routine in &mut routines {
        let Some(cottage) = routine
            .path
            .last()
            .and_then(|&end| super::cottages::home_of(end))
        else {
            continue;
        };
        let second = housed.contains(&cottage.house);
        if !second {
            housed.push(cottage.house);
        }
        routine.home_facing = cottage.walk_in(&mut routine.path, second);
    }
    routines
}
