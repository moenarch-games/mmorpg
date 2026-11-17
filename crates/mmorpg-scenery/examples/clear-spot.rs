//! Offline authoring aid (#863): the nearest clear ground in Greyhaven for a new animal, prop or
//! spawn. Loads the content and scenery once, indexes every obstacle on a uniform grid and
//! answers in well under a second in release.
//!
//! `cargo run --release -p mmorpg-scenery --example clear-spot -- --near 52000,66000 --radius 300`
//!
//! Every coordinate and distance is in units (100 per metre), as content source writes them.
//! A spot is clear when nothing below lies within `--radius` of it: core colliders and static
//! meshes (bar the ground), props with colliders, the walk-in cottages' footprints, road
//! surfaces and water. Options:
//!
//! - `--near x,z` (required): where to search from.
//! - `--radius r` (default 200): the clearance round the spot.
//! - `--shore`: only spots 300–900 units from water.
//! - `--off-routes`: also clear of every resident's walk home and errand legs.
//! - `--avoid-fields`: also clear of the farmland's fields.
//! - `--indoors`: inside a walk-in house, to furnish it (#886): its footprint no longer blocks,
//!   only its walls and furniture (core colliders) do; with `--off-routes`, clear of the
//!   residents' walks in too.
//! - `--count n` (default 5): how many spots, each at least two radii from the others.
//! - `--max-distance d` (default 20000): how far from `--near` to look.
//! - `--step s` (default 50): the candidate grid spacing.
use mmorpg_core::greyhaven_vale::{self, brook, cottages, ids, terrain};
use mmorpg_scenery::{Biome, Scenery, TERRAIN_BOUNDS, greyhaven_vale_scenery};
use std::collections::HashMap;

/// Index cell size in units (10 m).
const CELL: i32 = 1_000;
/// Water's distance band for `--shore`.
const SHORE: (i64, i64) = (300, 900);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Layer {
    Obstacle,
    Road,
    Route,
    Field,
    Water,
}

#[derive(Clone, Copy)]
enum Shape {
    Rect {
        min: [i32; 2],
        max: [i32; 2],
    },
    Segment {
        a: [i32; 2],
        b: [i32; 2],
        half_width: i32,
    },
    Ellipse {
        centre: [i32; 2],
        radii: [i32; 2],
    },
}

impl Shape {
    fn bounds(self) -> ([i32; 2], [i32; 2]) {
        match self {
            Self::Rect { min, max } => (min, max),
            Self::Segment { a, b, half_width } => (
                [a[0].min(b[0]) - half_width, a[1].min(b[1]) - half_width],
                [a[0].max(b[0]) + half_width, a[1].max(b[1]) + half_width],
            ),
            Self::Ellipse { centre, radii } => (
                [centre[0] - radii[0], centre[1] - radii[1]],
                [centre[0] + radii[0], centre[1] + radii[1]],
            ),
        }
    }

    /// Distance from `p` to the shape's edge, zero or less inside it.
    fn distance(self, [x, z]: [i32; 2]) -> f64 {
        let (x, z) = (f64::from(x), f64::from(z));
        match self {
            Self::Rect { min, max } => {
                let dx = (f64::from(min[0]) - x).max(x - f64::from(max[0])).max(0.0);
                let dz = (f64::from(min[1]) - z).max(z - f64::from(max[1])).max(0.0);
                dx.hypot(dz)
            }
            Self::Segment { a, b, half_width } => {
                let (ax, az) = (f64::from(a[0]), f64::from(a[1]));
                let (bx, bz) = (f64::from(b[0]) - ax, f64::from(b[1]) - az);
                let length = bx * bx + bz * bz;
                let t = if length > 0.0 {
                    (((x - ax) * bx + (z - az) * bz) / length).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                (x - ax - t * bx).hypot(z - az - t * bz) - f64::from(half_width)
            }
            Self::Ellipse { centre, radii } => {
                // Along the ray from the centre: exact on the axes, close enough elsewhere.
                let (dx, dz) = (x - f64::from(centre[0]), z - f64::from(centre[1]));
                let k = (dx / f64::from(radii[0])).hypot(dz / f64::from(radii[1]));
                if k == 0.0 {
                    return -f64::from(radii[0].min(radii[1]));
                }
                dx.hypot(dz) * (1.0 - 1.0 / k)
            }
        }
    }
}

/// Every obstacle by the index cells its bounds cover.
struct Index {
    shapes: Vec<(Layer, Shape)>,
    cells: HashMap<(i32, i32), Vec<usize>>,
}

impl Index {
    fn insert(&mut self, layer: Layer, shape: Shape) {
        let index = self.shapes.len();
        self.shapes.push((layer, shape));
        let (min, max) = shape.bounds();
        for cx in min[0].div_euclid(CELL)..=max[0].div_euclid(CELL) {
            for cz in min[1].div_euclid(CELL)..=max[1].div_euclid(CELL) {
                self.cells.entry((cx, cz)).or_default().push(index);
            }
        }
    }

    /// The least distance from `p` to a shape on `layers` within `reach`, if any is that close.
    fn nearest(&self, p: [i32; 2], reach: i32, layers: &[Layer]) -> Option<f64> {
        let mut nearest: Option<f64> = None;
        for cx in (p[0] - reach).div_euclid(CELL)..=(p[0] + reach).div_euclid(CELL) {
            for cz in (p[1] - reach).div_euclid(CELL)..=(p[1] + reach).div_euclid(CELL) {
                for &index in self.cells.get(&(cx, cz)).into_iter().flatten() {
                    let (layer, shape) = self.shapes[index];
                    if !layers.contains(&layer) {
                        continue;
                    }
                    let distance = shape.distance(p);
                    if distance < f64::from(reach) && nearest.is_none_or(|n| distance < n) {
                        nearest = Some(distance);
                    }
                }
            }
        }
        nearest
    }
}

struct Query {
    near: [i32; 2],
    radius: i32,
    shore: bool,
    off_routes: bool,
    avoid_fields: bool,
    indoors: bool,
    count: usize,
    max_distance: i32,
    step: i32,
}

fn parse() -> Result<Query, String> {
    let mut query = Query {
        near: [0, 0],
        radius: 200,
        shore: false,
        off_routes: false,
        avoid_fields: false,
        indoors: false,
        count: 5,
        max_distance: 20_000,
        step: 50,
    };
    let mut near = None;
    let mut args = std::env::args().skip(1);
    let number = |value: Option<String>, name: &str| -> Result<i32, String> {
        value
            .ok_or(format!("{name} needs a value"))?
            .trim()
            .parse()
            .map_err(|error| format!("{name}: {error}"))
    };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--near" => {
                let value = args.next().ok_or("--near needs x,z")?;
                let (x, z) = value.split_once(',').ok_or("--near needs x,z")?;
                near = Some([
                    number(Some(x.to_owned()), "--near")?,
                    number(Some(z.to_owned()), "--near")?,
                ]);
            }
            "--radius" => query.radius = number(args.next(), "--radius")?,
            "--count" => {
                query.count = usize::try_from(number(args.next(), "--count")?)
                    .map_err(|error| format!("--count: {error}"))?;
            }
            "--max-distance" => query.max_distance = number(args.next(), "--max-distance")?,
            "--step" => query.step = number(args.next(), "--step")?.max(1),
            "--shore" => query.shore = true,
            "--off-routes" => query.off_routes = true,
            "--avoid-fields" => query.avoid_fields = true,
            "--indoors" => query.indoors = true,
            other => return Err(format!("unknown argument {other} (see the example's docs)")),
        }
    }
    query.near = near.ok_or("--near x,z is required")?;
    Ok(query)
}

fn build(scenery: &Scenery, indoors: bool) -> Index {
    let mut index = Index {
        shapes: Vec::new(),
        cells: HashMap::new(),
    };
    let rect = |centre: [i32; 2], half: [i32; 2]| Shape::Rect {
        min: [centre[0] - half[0], centre[1] - half[1]],
        max: [centre[0] + half[0], centre[1] + half[1]],
    };
    let definition = greyhaven_vale::greyhaven_vale_definition();
    let huge = |half: [i32; 3]| half[0] > 20_000 && half[2] > 20_000;
    for collider in definition.colliders() {
        if collider.id != ids::GROUND && !huge(collider.half_extents) {
            let [x, _, z] = collider.position;
            let [hx, _, hz] = collider.half_extents;
            index.insert(Layer::Obstacle, rect([x, z], [hx, hz]));
        }
    }
    for mesh in definition.static_meshes() {
        if let Ok(bounds) = mesh.bounds()
            && !huge(bounds.half_extents)
        {
            let [x, _, z] = bounds.position;
            let [hx, _, hz] = bounds.half_extents;
            index.insert(Layer::Obstacle, rect([x, z], [hx, hz]));
        }
    }
    for prop in scenery.props.iter().filter(|prop| prop.collider.is_some()) {
        let footprint = prop.footprint();
        index.insert(
            Layer::Obstacle,
            Shape::Rect {
                min: footprint.min,
                max: footprint.max,
            },
        );
    }
    for cottage in cottages::cottages().into_iter().filter(|_| !indoors) {
        let [hx, hz] = cottage.plan.half();
        let corners = [[-hx, -hz], [hx, hz]].map(|corner| cottage.spot(corner));
        index.insert(
            Layer::Obstacle,
            Shape::Rect {
                min: [
                    corners[0][0].min(corners[1][0]),
                    corners[0][1].min(corners[1][1]),
                ],
                max: [
                    corners[0][0].max(corners[1][0]),
                    corners[0][1].max(corners[1][1]),
                ],
            },
        );
    }
    for road in &scenery.roads {
        for pair in road.points.windows(2) {
            index.insert(
                Layer::Road,
                Shape::Segment {
                    a: pair[0],
                    b: pair[1],
                    half_width: road.half_width,
                },
            );
        }
    }
    for water in &scenery.water {
        index.insert(
            Layer::Water,
            Shape::Ellipse {
                centre: water.centre,
                radii: water.radii,
            },
        );
    }
    // The brook's channels (#891), narrower than the terrain grid's lake-bed cells below.
    let channels = [
        (brook::village(), brook::HALF_WIDTH),
        (brook::inflow(), brook::UPPER_HALF_WIDTH),
        (brook::outlet(), brook::UPPER_HALF_WIDTH),
    ];
    for (points, half_width) in channels {
        for pair in points.windows(2) {
            index.insert(
                Layer::Water,
                Shape::Segment {
                    a: pair[0],
                    b: pair[1],
                    half_width,
                },
            );
        }
    }
    // Lake beds the ellipses miss (painted water), at the terrain grid's step.
    let half = terrain::STEP / 2;
    for row in 0..i32::from(terrain::ROWS) {
        for column in 0..i32::from(terrain::COLUMNS) {
            let x = terrain::ORIGIN[0] + column * terrain::STEP;
            let z = terrain::ORIGIN[1] + row * terrain::STEP;
            if scenery.biome_of(x, z) == Biome::LakeBed {
                index.insert(Layer::Water, rect([x, z], [half, half]));
            }
        }
    }
    for field in scenery.farmland() {
        index.insert(
            Layer::Field,
            Shape::Rect {
                min: field.min,
                max: field.max,
            },
        );
    }
    let content = greyhaven_vale::content();
    let posts: HashMap<_, _> = content
        .npcs()
        .iter()
        .map(|npc| (npc.id, npc.position))
        .collect();
    for routine in content.routines() {
        let Some(&post) = posts.get(&routine.npc) else {
            continue;
        };
        let legs = std::iter::once(&routine.path).chain(routine.errands.iter().map(|e| &e.path));
        for leg in legs {
            let points: Vec<_> = std::iter::once(post).chain(leg.iter().copied()).collect();
            for pair in points.windows(2) {
                index.insert(
                    Layer::Route,
                    Shape::Segment {
                        a: pair[0],
                        b: pair[1],
                        half_width: 0,
                    },
                );
            }
        }
    }
    index
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let query = parse()?;
    let started = std::time::Instant::now();
    let scenery = greyhaven_vale_scenery();
    let index = build(&scenery, query.indoors);
    let built = started.elapsed();

    let mut blocking = vec![Layer::Obstacle, Layer::Road, Layer::Water];
    if query.off_routes {
        blocking.push(Layer::Route);
    }
    if query.avoid_fields {
        blocking.push(Layer::Field);
    }
    let reach = query.max_distance / query.step;
    let mut candidates: Vec<[i32; 2]> = (-reach..=reach)
        .flat_map(|i| (-reach..=reach).map(move |j| [i, j]))
        .filter(|[i, j]| i64::from(*i).pow(2) + i64::from(*j).pow(2) <= i64::from(reach).pow(2))
        .map(|[i, j]| {
            [
                query.near[0] + i * query.step,
                query.near[1] + j * query.step,
            ]
        })
        .collect();
    let squared =
        |p: [i32; 2], q: [i32; 2]| i64::from(p[0] - q[0]).pow(2) + i64::from(p[1] - q[1]).pow(2);
    candidates.sort_by_key(|&p| (squared(p, query.near), p));

    let bounds = TERRAIN_BOUNDS;
    let spacing = i64::from(query.radius * 2).pow(2);
    let mut found: Vec<([i32; 2], Option<f64>)> = Vec::new();
    for p in candidates {
        if found.len() >= query.count {
            break;
        }
        if !bounds.contains(p[0], p[1])
            || found.iter().any(|&(q, _)| squared(p, q) < spacing)
            || index.nearest(p, query.radius, &blocking).is_some()
        {
            continue;
        }
        // Rivers are their own line index in core content, as far as their banks reach.
        let river = greyhaven_vale::design::river_distance(p[0], p[1]);
        if river < i64::from(query.radius) {
            continue;
        }
        let river = (river <= SHORE.1).then_some(river as f64);
        let water = match (index.nearest(p, SHORE.1 as i32, &[Layer::Water]), river) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        if query.shore && !water.is_some_and(|d| d >= SHORE.0 as f64 && d <= SHORE.1 as f64) {
            continue;
        }
        found.push((p, water));
    }

    println!(
        "index: {} shapes in {} cells, built in {:.2?}; searched in {:.2?}",
        index.shapes.len(),
        index.cells.len(),
        built,
        started.elapsed() - built
    );
    if found.is_empty() {
        println!("no clear spot within {} units", query.max_distance);
        std::process::exit(1);
    }
    for ([x, z], water) in found {
        let distance = (squared([x, z], query.near) as f64).sqrt();
        let water = water.map_or(String::new(), |d| format!(", water {d:.0}"));
        println!(
            "[{x}, {z}]  ({:.1} m, {:.1} m)  ground {}  distance {distance:.0}{water}",
            f64::from(x) / 100.0,
            f64::from(z) / 100.0,
            scenery.ground_at(x, z),
        );
    }
    Ok(())
}
