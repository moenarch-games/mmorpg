//! Named areas of zone content. Clients read area names and outlines from here instead of
//! inventing their own, and "explore" objectives and discovery decide membership with the
//! same table. Areas never affect movement or collision. Each content revision publishes its
//! table, such as `greyhaven_vale::areas()`.
//!
//! An area is a polygon in XZ units: an outer ring and any holes, even-odd. Greyhaven's are
//! traced from its geography and tile the zone (#1063); a rectangle is the simplest polygon.
//! [`ZoneAreas::area_at`] runs for every player every tick (discovery, explore objectives),
//! so the table keeps a grid index: a cell no border crosses names its area outright, and a
//! cell a border crosses tests only its candidates' edges in that cell's row band.

use crate::ZoneError;

/// Longest area name in bytes.
pub const MAX_AREA_NAME_BYTES: usize = 64;

/// Side of an index cell, in units (16 m).
const INDEX_CELL: i32 = 1_600;

/// Stable identifier of a named area within one content revision.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AreaId(u32);

impl AreaId {
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// A named XZ polygon in simulation units.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Area {
    id: AreaId,
    name: String,
    rings: Vec<Vec<[i32; 2]>>,
    min_xz: [i32; 2],
    max_xz: [i32; 2],
    centre: [i32; 2],
    radius: i32,
}

fn checked_name(name: String) -> Result<String, ZoneError> {
    if name.trim().is_empty() || name.len() > MAX_AREA_NAME_BYTES {
        return Err(ZoneError::new("area name must be 1..=64 bytes of text"));
    }
    Ok(name)
}

impl Area {
    /// A rectangle with inclusive bounds.
    pub fn new(
        id: AreaId,
        name: impl Into<String>,
        min_xz: [i32; 2],
        max_xz: [i32; 2],
    ) -> Result<Self, ZoneError> {
        let name = checked_name(name.into())?;
        if min_xz[0] > max_xz[0] || min_xz[1] > max_xz[1] {
            return Err(ZoneError::new("area bounds must not be inverted"));
        }
        // Inclusive integer bounds are the half-open polygon `[min, max + 1)`.
        let [x0, z0] = min_xz;
        let [x1, z1] = max_xz.map(|value| value.saturating_add(1));
        Ok(Self {
            id,
            name,
            rings: vec![vec![[x0, z0], [x1, z0], [x1, z1], [x0, z1]]],
            min_xz,
            max_xz,
            centre: [x0 + (max_xz[0] - x0) / 2, z0 + (max_xz[1] - z0) / 2],
            radius: (max_xz[0] - x0).min(max_xz[1] - z0) / 2,
        })
    }

    /// A polygon: its outer ring first, then any holes, each at least three open vertices.
    /// `centre` is a point well inside it (a label's place) and `radius` how far that point
    /// lies from its edge.
    pub fn polygon(
        id: AreaId,
        name: impl Into<String>,
        rings: Vec<Vec<[i32; 2]>>,
        centre: [i32; 2],
        radius: i32,
    ) -> Result<Self, ZoneError> {
        let name = checked_name(name.into())?;
        if rings.is_empty() || rings.iter().any(|ring| ring.len() < 3) {
            return Err(ZoneError::new(
                "an area needs rings of at least three vertices",
            ));
        }
        let mut min_xz = [i32::MAX; 2];
        let mut max_xz = [i32::MIN; 2];
        for point in rings.iter().flatten() {
            for axis in 0..2 {
                min_xz[axis] = min_xz[axis].min(point[axis]);
                max_xz[axis] = max_xz[axis].max(point[axis]);
            }
        }
        let area = Self {
            id,
            name,
            rings,
            min_xz,
            max_xz,
            centre,
            radius,
        };
        if radius < 0 || !area.contains(centre[0], centre[1]) {
            return Err(ZoneError::new("an area's centre must lie inside it"));
        }
        Ok(area)
    }

    #[must_use]
    pub const fn id(&self) -> AreaId {
        self.id
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The outer ring, then any holes; open (the last vertex joins the first).
    #[must_use]
    pub fn rings(&self) -> &[Vec<[i32; 2]>] {
        &self.rings
    }

    /// The bounding box's least corner, inclusive.
    #[must_use]
    pub const fn min_xz(&self) -> [i32; 2] {
        self.min_xz
    }

    /// The bounding box's greatest corner, inclusive.
    #[must_use]
    pub const fn max_xz(&self) -> [i32; 2] {
        self.max_xz
    }

    /// A point well inside the area, where its name belongs.
    #[must_use]
    pub const fn centre(&self) -> [i32; 2] {
        self.centre
    }

    /// How far [`Self::centre`] lies from the area's edge, in units.
    #[must_use]
    pub const fn radius(&self) -> i32 {
        self.radius
    }

    /// Even-odd containment over every ring. This tests every edge; [`ZoneAreas::area_at`]
    /// is the indexed lookup.
    #[must_use]
    pub fn contains(&self, x: i32, z: i32) -> bool {
        if x < self.min_xz[0] || x > self.max_xz[0] || z < self.min_xz[1] || z > self.max_xz[1] {
            return false;
        }
        let mut inside = false;
        for ring in &self.rings {
            for index in 0..ring.len() {
                inside ^= crosses(ring[index], ring[(index + 1) % ring.len()], x, z);
            }
        }
        inside
    }
}

/// Whether edge `a`→`b` crosses the ray from `(x, z)` towards +x, half-open in z so that
/// polygons sharing an edge never both claim a point on it.
pub(crate) fn crosses(a: [i32; 2], b: [i32; 2], x: i32, z: i32) -> bool {
    if (a[1] > z) == (b[1] > z) {
        return false;
    }
    let (dz, dx) = (i64::from(b[1] - a[1]), i64::from(b[0] - a[0]));
    let lhs = (i64::from(x) - i64::from(a[0])) * dz;
    let rhs = (i64::from(z) - i64::from(a[1])) * dx;
    (dz > 0 && lhs < rhs) || (dz < 0 && lhs > rhs)
}

/// One index cell: no area, one whole area, or a border cell's candidates.
#[derive(Clone, Copy, Debug)]
enum Cell {
    Outside,
    Whole(u16),
    /// `start..end` in [`AreaIndex::candidates`].
    Border(u32, u32),
}

/// The lookup grid over every area's bounding box.
#[derive(Clone, Debug, Default)]
struct AreaIndex {
    origin: [i32; 2],
    columns: i32,
    rows: i32,
    cells: Vec<Cell>,
    candidates: Vec<u16>,
    /// Per row band and area, `start..end` in `edges`: the area's edges whose z range meets
    /// the band.
    bands: Vec<(u32, u32)>,
    edges: Vec<[[i32; 2]; 2]>,
}

impl AreaIndex {
    #[allow(clippy::cast_possible_truncation)]
    fn new(areas: &[Area]) -> Self {
        if areas.is_empty() {
            return Self::default();
        }
        let min = areas.iter().fold([i32::MAX; 2], |m, a| {
            [m[0].min(a.min_xz[0]), m[1].min(a.min_xz[1])]
        });
        let max = areas.iter().fold([i32::MIN; 2], |m, a| {
            [m[0].max(a.max_xz[0]), m[1].max(a.max_xz[1])]
        });
        let span = |axis: usize| (max[axis] - min[axis]) / INDEX_CELL + 1;
        let (columns, rows) = (span(0), span(1));
        let cell_of = |value: i32, axis: usize| (value - min[axis]).div_euclid(INDEX_CELL);
        let band_range = |lo: i32, hi: i32| {
            (
                cell_of(lo, 1).clamp(0, rows - 1),
                cell_of(hi, 1).clamp(0, rows - 1),
            )
        };
        let n = areas.len();
        let mut bands: Vec<Vec<[[i32; 2]; 2]>> = vec![Vec::new(); rows as usize * n];
        let mut touched: Vec<Vec<u16>> = vec![Vec::new(); (columns * rows) as usize];
        for (index, area) in areas.iter().enumerate() {
            for ring in &area.rings {
                for k in 0..ring.len() {
                    let (a, b) = (ring[k], ring[(k + 1) % ring.len()]);
                    let (z0, z1) = (a[1].min(b[1]), a[1].max(b[1]));
                    let (x0, x1) = (a[0].min(b[0]), a[0].max(b[0]));
                    // An edge only ever counts for z in [z0, z1).
                    if z0 < z1 {
                        let (r0, r1) = band_range(z0, z1 - 1);
                        for row in r0..=r1 {
                            bands[row as usize * n + index].push([a, b]);
                        }
                    }
                    // Every cell the edge's box meets, borders included, holds a candidate.
                    let (r0, r1) = band_range(z0 - 1, z1 + 1);
                    let c0 = cell_of(x0 - 1, 0).clamp(0, columns - 1);
                    let c1 = cell_of(x1 + 1, 0).clamp(0, columns - 1);
                    for row in r0..=r1 {
                        for column in c0..=c1 {
                            let list = &mut touched[(row * columns + column) as usize];
                            if list.last() != Some(&(index as u16)) {
                                list.push(index as u16);
                            }
                        }
                    }
                }
            }
        }
        let mut edges = Vec::new();
        let bands = bands
            .into_iter()
            .map(|list| {
                let start = edges.len() as u32;
                edges.extend(list);
                (start, edges.len() as u32)
            })
            .collect::<Vec<_>>();
        let mut index = Self {
            origin: min,
            columns,
            rows,
            cells: Vec::with_capacity(touched.len()),
            candidates: Vec::new(),
            bands,
            edges,
        };
        for row in 0..rows {
            // A run of cells no border meets lies in one area: test its first cell only.
            let mut run: Option<Cell> = None;
            for column in 0..columns {
                let list = &mut touched[(row * columns + column) as usize];
                if list.is_empty() {
                    let cell = *run.get_or_insert_with(|| {
                        let x = min[0] + column * INDEX_CELL + INDEX_CELL / 2;
                        let z = min[1] + row * INDEX_CELL + INDEX_CELL / 2;
                        (0..n)
                            .find(|&area| index.band_contains(row, area, x, z, &mut 0))
                            .map_or(Cell::Outside, |area| Cell::Whole(area as u16))
                    });
                    index.cells.push(cell);
                } else {
                    run = None;
                    list.sort_unstable();
                    list.dedup();
                    let start = index.candidates.len() as u32;
                    index.candidates.extend_from_slice(list);
                    index
                        .cells
                        .push(Cell::Border(start, index.candidates.len() as u32));
                }
            }
        }
        index
    }

    /// Even-odd containment in `area` from its edges in `row`'s band, counting edge tests.
    fn band_contains(&self, row: i32, area: usize, x: i32, z: i32, work: &mut usize) -> bool {
        let areas = self.bands.len() / self.rows.max(1) as usize;
        let (start, end) = self.bands[row as usize * areas + area];
        let edges = &self.edges[start as usize..end as usize];
        *work += edges.len();
        edges
            .iter()
            .fold(false, |inside, [a, b]| inside ^ crosses(*a, *b, x, z))
    }

    /// The index of the area at `(x, z)`, adding the edge tests made to `work`.
    fn locate(&self, x: i32, z: i32, work: &mut usize) -> Option<usize> {
        if self.cells.is_empty() || x < self.origin[0] || z < self.origin[1] {
            return None;
        }
        let column = (x - self.origin[0]) / INDEX_CELL;
        let row = (z - self.origin[1]) / INDEX_CELL;
        if column >= self.columns || row >= self.rows {
            return None;
        }
        match self.cells[(row * self.columns + column) as usize] {
            Cell::Outside => None,
            Cell::Whole(area) => Some(usize::from(area)),
            Cell::Border(start, end) => self.candidates[start as usize..end as usize]
                .iter()
                .map(|&area| usize::from(area))
                .find(|&area| self.band_contains(row, area, x, z, work)),
        }
    }
}

/// The ordered area table of one content revision.
#[derive(Clone, Debug, Default)]
pub struct ZoneAreas {
    areas: Vec<Area>,
    index: AreaIndex,
}

impl PartialEq for ZoneAreas {
    fn eq(&self, other: &Self) -> bool {
        self.areas == other.areas
    }
}

impl Eq for ZoneAreas {}

impl ZoneAreas {
    /// Validates unique IDs and names, orders the table by ID and indexes it.
    pub fn new(mut areas: Vec<Area>) -> Result<Self, ZoneError> {
        areas.sort_by_key(Area::id);
        if areas.windows(2).any(|pair| pair[0].id == pair[1].id) {
            return Err(ZoneError::new("duplicate area id"));
        }
        if areas.len() > usize::from(u16::MAX) {
            return Err(ZoneError::new("too many areas"));
        }
        let mut names = areas.iter().map(Area::name).collect::<Vec<_>>();
        names.sort_unstable();
        if names.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(ZoneError::new("duplicate area name"));
        }
        let index = AreaIndex::new(&areas);
        Ok(Self { areas, index })
    }

    #[must_use]
    pub fn areas(&self) -> &[Area] {
        &self.areas
    }

    /// The area containing `(x, z)`. Where areas overlap, the lowest ID wins.
    #[must_use]
    pub fn area_at(&self, x: i32, z: i32) -> Option<&Area> {
        self.area_at_counted(x, z).0
    }

    /// The most polygon edges any one [`Self::area_at`] can test: the busiest border cell's
    /// candidates' edges in its row band.
    #[must_use]
    pub fn lookup_work_bound(&self) -> usize {
        let index = &self.index;
        let areas = index.bands.len() / index.rows.max(1) as usize;
        let mut most = 0;
        for (cell, kind) in index.cells.iter().enumerate() {
            if let Cell::Border(start, end) = *kind {
                let row = cell / index.columns.max(1) as usize;
                let work = index.candidates[start as usize..end as usize]
                    .iter()
                    .map(|&area| {
                        let (a, b) = index.bands[row * areas + usize::from(area)];
                        (b - a) as usize
                    })
                    .sum();
                most = most.max(work);
            }
        }
        most
    }

    /// [`Self::area_at`] and the polygon edges it tested: the deterministic work counter of
    /// the `area-lookup` operation in `.performance/contract.json`.
    #[must_use]
    pub fn area_at_counted(&self, x: i32, z: i32) -> (Option<&Area>, usize) {
        let mut work = 0;
        let area = self
            .index
            .locate(x, z, &mut work)
            .map(|index| &self.areas[index]);
        (area, work)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(id: u32, name: &str, min_xz: [i32; 2], max_xz: [i32; 2]) -> Area {
        Area::new(AreaId::new(id), name, min_xz, max_xz).unwrap()
    }

    #[test]
    fn lookup_is_inclusive_and_prefers_the_lowest_id() {
        let areas = ZoneAreas::new(vec![
            area(2, "Inner", [0, 0], [10, 10]),
            area(1, "Outer", [-5, -5], [5, 5]),
        ])
        .unwrap();
        assert_eq!(areas.areas()[0].name(), "Outer", "ordered by id");
        assert_eq!(areas.area_at(5, 5).map(Area::name), Some("Outer"));
        assert_eq!(areas.area_at(6, 10).map(Area::name), Some("Inner"));
        assert_eq!(areas.area_at(11, 0), None);
        assert_eq!(areas.area_at(-6, 0), None);
    }

    #[test]
    fn indexed_lookup_matches_testing_every_area() {
        // An L-shaped area with a square hole that holds a second area, spanning many cells.
        let l = Area::polygon(
            AreaId::new(1),
            "Ell",
            vec![
                vec![
                    [0, 0],
                    [9_000, 0],
                    [9_000, 3_000],
                    [3_000, 3_000],
                    [3_000, 9_000],
                    [0, 9_000],
                ],
                vec![[500, 500], [2_500, 500], [2_500, 2_500], [500, 2_500]],
            ],
            [1_500, 6_000],
            1_000,
        )
        .unwrap();
        let hole = Area::polygon(
            AreaId::new(2),
            "Hole",
            vec![vec![[500, 500], [2_500, 500], [2_500, 2_500], [500, 2_500]]],
            [1_500, 1_500],
            1_000,
        )
        .unwrap();
        let areas = ZoneAreas::new(vec![l, hole]).unwrap();
        for z in (-200..9_400).step_by(97) {
            for x in (-200..9_400).step_by(89) {
                let oracle = areas
                    .areas()
                    .iter()
                    .find(|a| a.contains(x, z))
                    .map(Area::id);
                assert_eq!(areas.area_at(x, z).map(Area::id), oracle, "at ({x}, {z})");
            }
        }
        // A shared edge belongs to exactly one side.
        assert_eq!(areas.area_at(2_500, 1_000).map(Area::name), Some("Ell"));
        assert_eq!(areas.area_at(500, 1_000).map(Area::name), Some("Hole"));
    }

    #[test]
    fn invalid_areas_fail_closed() {
        assert!(Area::new(AreaId::new(1), " ", [0, 0], [1, 1]).is_err());
        assert!(Area::new(AreaId::new(1), "x".repeat(65), [0, 0], [1, 1]).is_err());
        assert!(Area::new(AreaId::new(1), "Inverted", [2, 0], [1, 1]).is_err());
        let triangle = vec![vec![[0, 0], [100, 0], [0, 100]]];
        assert!(Area::polygon(AreaId::new(1), "Out", triangle.clone(), [90, 90], 0).is_err());
        assert!(
            Area::polygon(
                AreaId::new(1),
                "Line",
                vec![vec![[0, 0], [1, 1]]],
                [0, 0],
                0
            )
            .is_err()
        );
        assert!(Area::polygon(AreaId::new(1), "In", triangle, [10, 10], 5).is_ok());
        assert!(
            ZoneAreas::new(vec![
                area(1, "A", [0, 0], [1, 1]),
                area(1, "B", [0, 0], [1, 1])
            ])
            .is_err()
        );
        assert!(
            ZoneAreas::new(vec![
                area(1, "A", [0, 0], [1, 1]),
                area(2, "A", [0, 0], [1, 1])
            ])
            .is_err()
        );
    }
}
