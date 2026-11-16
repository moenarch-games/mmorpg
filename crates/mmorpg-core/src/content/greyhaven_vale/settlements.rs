//! Settlements and landmarks across the Greyhaven region (#818): the places beyond the Vale's
//! heart that the region's roads lead to. Barrowdown is a hamlet on a knoll of the King's Road
//! Downs, beside an old barrow ring; Fenford sits by the Fenwater's ford; a fisher's lodge stands
//! on Westmere Tarn's eastern shore; hunters camp on the Southwood Trail; miners work the Delving
//! Heights; a watchtower guards the Greyfang Road; and a ring of standing stones crowns the Crown
//! Foothills.
//!
//! Each settlement's yard is a level pad of the designed land ([`super::design`]), and its
//! buildings reuse the Vale's visuals. Three of them hold a waystone of the network
//! ([`crate::WAYSTONES`]).

use super::{Road, ids, on_terrain};
use crate::{AreaId, StaticCollider, XzBounds};

/// What a settlement structure looks like.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Building {
    House,
    Farmhouse,
    Barn,
    Windmill,
    Well,
    /// A keep-style tower.
    Tower,
    Tent,
    Campfire,
    Crate,
    Barrel,
    Cart,
    /// A drying rack or a pen's rail.
    Fence,
    /// A standing stone.
    Stone,
    /// A waystone of the network.
    Waystone,
    MineEntrance,
    /// Bare rock round a mine's mouth.
    Rock,
}

/// One settlement: its name, its level yard `(min, max)` and its structures
/// `(kind, centre, footprint size, height)`, in units.
pub struct Settlement {
    pub name: &'static str,
    pub yard: ([i32; 2], [i32; 2]),
    pub structures: &'static [(Building, [i32; 2], [i32; 2], i32)],
}

use Building::{
    Barn, Barrel, Campfire, Cart, Crate, Farmhouse, Fence, House, MineEntrance, Rock, Stone, Tent,
    Tower, Waystone, Well, Windmill,
};

/// The settlements, in collider order.
pub const SETTLEMENTS: &[Settlement] = &[
    Settlement {
        name: "Barrowdown",
        yard: ([50_000, 62_000], [58_000, 70_000]),
        structures: &[
            (House, [51_600, 63_600], [800, 600], 500),
            (House, [51_600, 66_600], [800, 600], 500),
            (House, [56_600, 63_600], [800, 600], 500),
            // The Barrow Rest, the hamlet's alehouse.
            (Farmhouse, [56_400, 67_400], [800, 700], 500),
            (Barn, [52_000, 69_000], [1_200, 1_600], 700),
            (Well, [54_000, 64_800], [200, 200], 100),
            (Waystone, [55_000, 64_800], [100, 100], 250),
            (Barrel, [55_600, 67_900], [60, 60], 90),
            (Barrel, [55_480, 68_020], [60, 60], 90),
            (Cart, [53_400, 67_400], [220, 120], 120),
        ],
    },
    Settlement {
        name: "Barrowdown Ring",
        yard: ([45_600, 58_600], [48_400, 61_400]),
        structures: &[
            (Stone, [47_000, 59_000], [80, 60], 240),
            (Stone, [47_700, 59_300], [80, 60], 220),
            (Stone, [48_000, 60_000], [80, 60], 260),
            (Stone, [47_700, 60_700], [80, 60], 230),
            (Stone, [47_000, 61_000], [80, 60], 250),
            (Stone, [46_300, 60_700], [80, 60], 210),
            (Stone, [46_000, 60_000], [80, 60], 240),
            (Stone, [46_300, 59_300], [80, 60], 200),
        ],
    },
    Settlement {
        name: "Fenford",
        yard: ([99_000, 18_000], [107_000, 24_400]),
        structures: &[
            (House, [100_200, 19_200], [800, 600], 500),
            (House, [100_200, 22_400], [800, 600], 500),
            (House, [103_600, 19_000], [800, 600], 500),
            // The Ford Inn.
            (Farmhouse, [106_000, 20_000], [800, 700], 500),
            (Windmill, [101_800, 23_800], [500, 500], 800),
            (Well, [103_200, 21_800], [200, 200], 100),
            (Waystone, [104_400, 22_600], [100, 100], 250),
            (Crate, [105_200, 23_200], [100, 100], 100),
            (Crate, [105_340, 23_200], [100, 100], 100),
            (Cart, [104_200, 19_400], [220, 120], 120),
        ],
    },
    Settlement {
        name: "Westmere Lodge",
        yard: ([-69_000, 2_400], [-63_000, 9_000]),
        structures: &[
            (House, [-64_400, 3_800], [900, 700], 500),
            (Tent, [-67_400, 7_600], [300, 300], 220),
            (Campfire, [-66_600, 5_600], [120, 120], 30),
            (Fence, [-68_000, 4_200], [20, 300], 120),
            (Barrel, [-63_600, 5_000], [60, 60], 90),
            (Crate, [-63_700, 5_200], [100, 100], 100),
            (Waystone, [-65_200, 8_400], [100, 100], 250),
        ],
    },
    Settlement {
        name: "Hunters' Camp",
        yard: ([-50_400, 77_200], [-45_400, 81_600]),
        structures: &[
            (House, [-46_400, 80_600], [800, 600], 500),
            (Tent, [-49_400, 80_400], [300, 300], 220),
            (Tent, [-48_600, 81_000], [300, 300], 220),
            (Campfire, [-48_200, 79_600], [120, 120], 30),
            (Fence, [-49_800, 78_400], [300, 20], 140),
            (Fence, [-49_800, 78_000], [300, 20], 140),
            (Crate, [-46_800, 79_400], [100, 100], 100),
        ],
    },
    Settlement {
        name: "Delving Workings",
        yard: ([103_600, -78_000], [112_000, -63_600]),
        structures: &[
            (Rock, [110_000, -76_000], [1_800, 800], 900),
            (MineEntrance, [110_000, -75_550], [600, 100], 420),
            (Tower, [105_000, -73_500], [800, 800], 1_400),
            (Tent, [107_000, -71_500], [300, 300], 220),
            (Tent, [108_400, -71_800], [300, 300], 220),
            (Campfire, [107_600, -70_200], [120, 120], 30),
            (Crate, [111_400, -68_600], [100, 100], 100),
            (Crate, [111_540, -68_600], [100, 100], 100),
            (Crate, [111_470, -68_480], [100, 100], 100),
            (Cart, [109_000, -68_800], [220, 120], 120),
            (Barrel, [106_200, -69_400], [60, 60], 90),
        ],
    },
    Settlement {
        name: "Greyfang Watch",
        yard: ([11_200, -88_400], [13_600, -86_000]),
        structures: &[
            (Tower, [12_400, -87_600], [800, 800], 1_800),
            (Campfire, [12_400, -86_400], [120, 120], 30),
            (Crate, [11_600, -86_400], [100, 100], 100),
        ],
    },
    Settlement {
        name: "Crown Stones",
        yard: ([-91_400, -47_900], [-88_600, -45_100]),
        structures: &[
            (Stone, [-90_000, -47_500], [80, 60], 260),
            (Stone, [-89_000, -47_100], [80, 60], 240),
            (Stone, [-88_700, -46_400], [80, 60], 280),
            (Stone, [-89_100, -45_600], [80, 60], 230),
            (Stone, [-90_000, -45_300], [80, 60], 270),
            (Stone, [-90_900, -45_600], [80, 60], 220),
            (Stone, [-91_300, -46_400], [80, 60], 250),
            (Stone, [-90_900, -47_100], [80, 60], 240),
        ],
    },
];

/// The settlements' named areas, numbered below the wild sub-regions' so a settlement names
/// the ground it stands on (where areas overlap the lowest ID wins).
pub const BARROWDOWN: AreaId = AreaId::new(14);
pub const FENFORD: AreaId = AreaId::new(15);
pub const WESTMERE_LODGE: AreaId = AreaId::new(16);
pub const HUNTERS_CAMP: AreaId = AreaId::new(17);
pub const DELVING_WORKINGS: AreaId = AreaId::new(18);
pub const GREYFANG_WATCH: AreaId = AreaId::new(19);
pub const CROWN_STONES: AreaId = AreaId::new(20);

const fn around(yard: ([i32; 2], [i32; 2]), margin: i32) -> XzBounds {
    XzBounds {
        min: [yard.0[0] - margin, yard.0[1] - margin],
        max: [yard.1[0] + margin, yard.1[1] + margin],
    }
}

/// The settlements' named areas: each its yard and 10 m round it.
pub const AREAS: &[(AreaId, &str, XzBounds)] = &[
    (BARROWDOWN, "Barrowdown", around(SETTLEMENTS[0].yard, 1_000)),
    (FENFORD, "Fenford", around(SETTLEMENTS[2].yard, 1_000)),
    (
        WESTMERE_LODGE,
        "Westmere Lodge",
        around(SETTLEMENTS[3].yard, 1_000),
    ),
    (
        HUNTERS_CAMP,
        "Hunters' Camp",
        around(SETTLEMENTS[4].yard, 1_000),
    ),
    (
        DELVING_WORKINGS,
        "Delving Workings",
        around(SETTLEMENTS[5].yard, 1_000),
    ),
    (
        GREYFANG_WATCH,
        "Greyfang Watch",
        around(SETTLEMENTS[6].yard, 1_000),
    ),
    (
        CROWN_STONES,
        "Crown Stones",
        around(SETTLEMENTS[7].yard, 1_000),
    ),
];

/// The lanes into the settlements off the region's roads; designed roads lead past the rest.
pub const ROADS: &[Road] = &[];

pub(super) const REGION: super::Region = super::Region {
    areas: AREAS,
    roads: ROADS,
    colliders,
};

/// Every structure in collider order: `(kind, footprint min, footprint max, height)`.
pub fn structures() -> impl Iterator<Item = (Building, [i32; 2], [i32; 2], i32)> {
    SETTLEMENTS.iter().flat_map(|settlement| {
        settlement
            .structures
            .iter()
            .map(|&(kind, centre, size, height)| {
                let half = [size[0] / 2, size[1] / 2];
                (
                    kind,
                    [centre[0] - half[0], centre[1] - half[1]],
                    [centre[0] + half[0], centre[1] + half[1]],
                    height,
                )
            })
    })
}

/// The settlements' structures, standing on their yards; a walk-in cottage (#843) is its walls
/// and furniture rather than its house's box.
fn colliders() -> Vec<StaticCollider> {
    let mut colliders: Vec<_> = (*ids::SETTLEMENTS.start()..)
        .zip(structures())
        .filter(|(id, _)| !super::cottages::is_house(*id))
        .map(|(id, (_, min, max, height))| on_terrain(id, min, max, height))
        .collect();
    colliders.extend(super::cottages::colliders());
    colliders
}

/// The settlement structure a collider ID belongs to (#818).
#[must_use]
pub fn building(id: u32) -> Option<Building> {
    if !ids::SETTLEMENTS.contains(&id) {
        return None;
    }
    let index = usize::try_from(id - ids::SETTLEMENTS.start()).ok()?;
    structures().nth(index).map(|(kind, ..)| kind)
}
