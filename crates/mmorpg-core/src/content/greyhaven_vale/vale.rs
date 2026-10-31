//! The Vale itself: the Outpost, Millbrook Farm, Stillwater Lake and Redbrand Hollow in the
//! basin of [`super::basin`], Wolfrun Woods on its rim, and the outer valley's ring of four
//! regions (#159). Its areas, roads and authored structures, in table order (#542).

use super::{Footprint, Road, cave, ids, inn, marshal_house, on_terrain};
use crate::{AreaId, StaticCollider, XzBounds};

pub const OUTPOST: AreaId = AreaId::new(1);
pub const WOLFRUN_WOODS: AreaId = AreaId::new(2);
pub const MILLBROOK_FARM: AreaId = AreaId::new(3);
pub const STILLWATER_LAKE: AreaId = AreaId::new(4);
pub const REDBRAND_HOLLOW: AreaId = AreaId::new(5);
pub const GREYFANG_RIDGE: AreaId = AreaId::new(6);
pub const SOUTHMEADOW_DOWNS: AreaId = AreaId::new(7);
pub const HOLLOWPINE_REACH: AreaId = AreaId::new(8);
pub const EASTWATER_FEN: AreaId = AreaId::new(9);

pub(super) const REGION: super::Region = super::Region {
    areas: AREAS,
    roads: ROADS,
    colliders,
};

/// The Vale's structures, the inn and Elden's house, and the Greyfang cave.
fn colliders() -> Vec<StaticCollider> {
    let mut colliders: Vec<_> = STRUCTURES
        .iter()
        // Elden's house collides through its walls and furniture (#158).
        .filter(|&&(id, ..)| id != ids::MARSHAL_HOUSE)
        // Structures out on the hills stand on the terrain (#159).
        .map(|&(id, min, max, height)| on_terrain(id, min, max, height))
        .collect();
    colliders.extend(inn::colliders(*ids::INN_PARTS.start()));
    colliders.extend(marshal_house::colliders(*ids::MARSHAL_HOUSE_PARTS.start()));
    colliders.extend(cave::GREYFANG.colliders(*ids::CAVE_WALLS.start()));
    colliders.push(cave::threshold(ids::CAVE_THRESHOLD));
    colliders
}

/// The Vale's named areas as `(id, name, half-open bounds)`. Bounds are disjoint.
pub const AREAS: &[(AreaId, &str, XzBounds)] = &[
    (
        OUTPOST,
        "Greyhaven Outpost",
        XzBounds {
            min: [-3_500, -1_300],
            max: [3_500, 5_300],
        },
    ),
    (
        WOLFRUN_WOODS,
        "Wolfrun Woods",
        XzBounds {
            min: [-11_900, -5_000],
            max: [-4_000, 5_000],
        },
    ),
    (
        MILLBROOK_FARM,
        "Millbrook Farm",
        XzBounds {
            min: [4_000, 1_300],
            max: [10_500, 9_000],
        },
    ),
    (
        STILLWATER_LAKE,
        "Stillwater Lake",
        XzBounds {
            min: [3_600, -7_800],
            max: [8_600, -3_200],
        },
    ),
    (
        REDBRAND_HOLLOW,
        "Redbrand Hollow",
        XzBounds {
            min: [-3_500, -11_900],
            max: [3_500, -7_000],
        },
    ),
    // The outer valley (#159): a ring of four regions around the original five.
    (
        GREYFANG_RIDGE,
        "Greyfang Ridge",
        XzBounds {
            min: [-20_800, -20_800],
            max: [20_800, -12_000],
        },
    ),
    (
        SOUTHMEADOW_DOWNS,
        "Southmeadow Downs",
        XzBounds {
            min: [-20_800, 12_000],
            max: [20_800, 20_800],
        },
    ),
    (
        HOLLOWPINE_REACH,
        "Hollowpine Reach",
        XzBounds {
            min: [-20_800, -12_000],
            max: [-12_000, 12_000],
        },
    ),
    (
        EASTWATER_FEN,
        "Eastwater Fen",
        XzBounds {
            min: [12_000, -12_000],
            max: [20_800, 12_000],
        },
    ),
];

/// The region's roads, in table order.
pub const ROADS: &[Road] = &[
    Road {
        name: "Hollow Road",
        // Up the Hollow Climb (#423): the ledge under the Hollow's cliff, the landing, the
        // cutting.
        points: &[
            [0, 1_200],
            [0, -1_000],
            [0, -4_000],
            [0, -6_200],
            [-3_600, -6_200],
            [-3_600, -7_000],
            [0, -7_000],
            [0, -8_600],
        ],
    },
    Road {
        name: "Lakeshore Path",
        // Up the Lakeshore Climb (#423): the west cliff's ledge, the landing, the cutting.
        points: &[
            [0, -2_800],
            [1_400, -2_800],
            [1_400, -6_200],
            [2_600, -6_200],
            [2_600, -3_200],
            [3_400, -3_200],
            [3_400, -4_900],
        ],
    },
    Road {
        // Along the terrace's rim from the farmyard to the lake's south shore (#423).
        name: "Stillwater Rim Trail",
        points: &[[6_400, 3_300], [6_400, -2_400], [5_600, -3_600]],
    },
    Road {
        name: "Millbrook Road",
        // Up the Millbrook Climb (#423): the cliff ledge, the hairpin landing, the cutting.
        points: &[
            [1_600, 2_000],
            [3_200, 2_000],
            [4_200, 2_000],
            [4_200, -1_800],
            [5_400, -1_800],
            [5_400, 1_200],
            [5_400, 2_800],
            [6_400, 3_300],
        ],
    },
    Road {
        // The farm's footpath down the cliff to the valley floor and the South Road (#423).
        name: "Millbrook Steps",
        points: &[
            [5_400, 2_800],
            [4_800, 3_400],
            [4_200, 3_400],
            [4_200, 8_000],
            [0, 8_000],
        ],
    },
    Road {
        name: "Wolfrun Trail",
        // Up the Wolfrun Climb (#423): the cliff ledge, the hairpin landing, the cutting.
        points: &[
            [-1_600, 2_000],
            [-3_200, 2_000],
            [-4_200, 2_000],
            [-4_200, -1_800],
            [-5_400, -1_800],
            [-5_400, 1_200],
            [-6_600, 300],
            [-7_600, 0],
        ],
    },
    Road {
        // Along the rim from Wolfrun Woods to Redbrand Hollow's camp (#423).
        name: "Wolfrun Ridge Path",
        points: &[[-6_600, 300], [-6_600, -7_700], [0, -7_700]],
    },
    Road {
        name: "South Road",
        // Up the South Climb (#423) onto the rim, across the Vale Ring Road and up the ridge's
        // notch onto the farmland's plateau (#424).
        points: &[
            [0, 2_800],
            [0, 5_000],
            [0, 9_400],
            [-3_600, 9_400],
            [-3_600, 10_200],
            [0, 10_200],
            [0, 15_000],
            [0, 23_200],
            [2_800, 23_200],
            [2_800, 25_200],
        ],
    },
    // The outer valley (#159): a ring road through the four outer regions and
    // two trails that join the farm and the woods to it without the outpost.
    Road {
        name: "Vale Ring Road",
        points: &[
            [-15_000, -15_000],
            [15_000, -15_000],
            [15_000, 15_000],
            [-15_000, 15_000],
            [-15_000, -15_000],
        ],
    },
    Road {
        name: "Fen Track",
        // From the top of the Millbrook Climb down the terrace's eastern slope (#423).
        points: &[[5_400, 1_200], [11_000, -1_000], [15_000, -3_000]],
    },
    Road {
        // From the top of the Wolfrun Climb along the rim (#423).
        name: "Downs Trail",
        points: &[[-5_400, 1_200], [-5_400, 6_000], [-15_000, 6_000]],
    },
];

#[rustfmt::skip]
pub const STRUCTURES: &[Footprint] = &[
    // Greyhaven Outpost: keep, inn, smithy, houses, well and waystone.
    (ids::KEEP, [-2_600, -500], [-800, 700], 1_000),
    (ids::SMITHY, [2_400, 100], [3_000, 800], 450),
    (103, [-2_800, 3_300], [-2_000, 3_900], 500),
    (104, [-1_500, 3_500], [-700, 4_100], 500),
    (105, [800, 3_500], [1_600, 4_100], 500),
    (106, [2_000, 3_300], [2_800, 3_900], 500),
    (ids::WELL, [400, 3_000], [600, 3_200], 100),
    (ids::WAYSTONE, [-600, 3_000], [-500, 3_100], 250),
    // Palisade: north, south, west and east sides with a 6 m gap on each road.
    (110, [-3_230, -1_030], [-300, -970], 300),
    (111, [300, -1_030], [3_230, -970], 300),
    (112, [-3_230, 4_970], [-300, 5_030], 300),
    (113, [300, 4_970], [3_230, 5_030], 300),
    (114, [-3_230, -970], [-3_170, 1_700], 300),
    (115, [-3_230, 2_300], [-3_170, 4_970], 300),
    (116, [3_170, -970], [3_230, 1_700], 300),
    (117, [3_170, 2_300], [3_230, 4_970], 300),
    // 0.8 m gate posts stand inside each gap, leaving a 4.4 m clear opening.
    (120, [-300, -1_040], [-220, -960], 400),
    (121, [220, -1_040], [300, -960], 400),
    (122, [-300, 4_960], [-220, 5_040], 400),
    (123, [220, 4_960], [300, 5_040], 400),
    (124, [-3_240, 1_700], [-3_160, 1_780], 400),
    (125, [-3_240, 2_220], [-3_160, 2_300], 400),
    (126, [3_160, 1_700], [3_240, 1_780], 400),
    (127, [3_160, 2_220], [3_240, 2_300], 400),
    // Graveyard plot: two rows of gravestones.
    (130, [-2_830, 4_390], [-2_770, 4_410], 100),
    (131, [-2_530, 4_390], [-2_470, 4_410], 100),
    (132, [-2_230, 4_390], [-2_170, 4_410], 100),
    (133, [-1_930, 4_390], [-1_870, 4_410], 100),
    (134, [-2_830, 4_690], [-2_770, 4_710], 100),
    (135, [-2_530, 4_690], [-2_470, 4_710], 100),
    (136, [-2_230, 4_690], [-2_170, 4_710], 100),
    (137, [-1_930, 4_690], [-1_870, 4_710], 100),
    // Millbrook Farm: barn, farmhouse, windmill base and field fences.
    (ids::BARN, [7_400, 3_600], [8_600, 5_200], 700),
    (ids::FARMHOUSE, [5_800, 4_300], [6_600, 5_000], 500),
    (ids::WINDMILL, [8_550, 1_850], [9_050, 2_350], 800),
    // The mill's loading dock (#59), 1.5 m high, west of the windmill.
    (ids::MILL_DOCK, [8_100, 1_800], [8_400, 2_100], 150),
    (210, [5_000, 5_590], [7_000, 5_610], 110),
    (211, [7_400, 5_590], [9_600, 5_610], 110),
    (212, [5_000, 8_390], [9_600, 8_410], 110),
    (213, [4_990, 5_610], [5_010, 8_390], 110),
    (214, [9_590, 5_610], [9_610, 8_390], 110),
    // Stillwater Lake: rocks outside the walkable shallow water.
    // North of the Sunken Shrine's cave (#274).
    (300, [7_700, -4_680], [7_900, -4_520], 150),
    (301, [7_080, -7_390], [7_320, -7_210], 180),
    (302, [4_920, -7_470], [5_080, -7_330], 100),
    (303, [3_500, -6_800], [3_700, -6_600], 160),
    (304, [6_710, -3_780], [6_890, -3_620], 120),
    (305, [4_330, -3_760], [4_470, -3_640], 90),
    // Redbrand Hollow: cliffs under the north wall, the mine entrance, the camp.
    // Their southern faces stand back for the Wolfrun Ridge Path (#423).
    (400, [-3_200, -12_000], [-2_200, -8_000], 1_200),
    (401, [-2_200, -8_400], [-1_600, -8_000], 800),
    (402, [2_200, -12_000], [3_200, -7_400], 1_200),
    (403, [1_600, -9_600], [2_200, -8_600], 900),
    (404, [-2_200, -12_000], [2_200, -11_000], 1_400),
    (405, [-2_200, -11_000], [-800, -10_400], 1_000),
    (406, [800, -11_000], [2_200, -10_500], 1_000),
    (ids::MINE_ENTRANCE, [-300, -11_000], [300, -10_900], 420),
    (420, [-1_700, -9_600], [-1_400, -9_300], 220),
    (421, [1_100, -9_900], [1_400, -9_600], 220),
    (422, [-1_500, -8_600], [-1_200, -8_300], 220),
    (430, [500, -10_500], [600, -10_400], 100),
    (431, [620, -10_500], [720, -10_400], 100),
    (432, [560, -10_380], [660, -10_280], 100),
    (433, [-700, -10_300], [-600, -10_200], 100),
    (ids::CAMPFIRE, [-60, -9_260], [60, -9_140], 30),
    // Greyfang Ridge: seven standing stones in an 8 m ring (#159).
    (500, [-40, -15_730], [40, -15_670], 250),
    (501, [590, -16_030], [670, -15_970], 230),
    (502, [740, -16_710], [820, -16_650], 260),
    (503, [310, -17_250], [390, -17_190], 240),
    (504, [-390, -17_250], [-310, -17_190], 250),
    (505, [-820, -16_710], [-740, -16_650], 220),
    (506, [-670, -16_030], [-590, -15_970], 260),
    // Southmeadow Downs: a ruined farmstead inside its old fence (#159).
    (510, [5_000, 16_000], [5_800, 16_600], 300),
    (511, [6_200, 16_200], [6_800, 16_700], 250),
    (512, [4_700, 15_800], [7_000, 15_820], 110),
    (513, [4_700, 15_820], [4_720, 17_000], 110),
    (514, [6_980, 15_820], [7_000, 17_000], 110),
    // Eastwater Fen: the old watchtower (#159).
    (ids::WATCHTOWER, [16_500, -6_200], [16_900, -5_800], 1_400),
    // Hollowpine Reach: boulders beside the ring road (#159).
    (530, [-15_700, -9_200], [-15_480, -9_000], 160),
    (531, [-14_600, -4_300], [-14_420, -4_140], 120),
    (532, [-15_620, 1_800], [-15_420, 1_960], 140),
    (533, [-14_640, 9_400], [-14_460, 9_580], 110),
    (534, [-15_800, 12_600], [-15_560, 12_820], 180),
    (535, [-14_560, -12_800], [-14_400, -12_660], 100),
    (536, [-15_660, 4_400], [-15_500, 4_540], 90),
    (537, [-14_620, -1_200], [-14_440, -1_040], 130),
];
