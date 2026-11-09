//! Brings a Krita-edited document back into the committed paint files.
//!
//! `paint height`: grey 128 is no change, one grey step is 10 cm, transparency fades towards no
//! change. `paint biome`: palette colours, transparent keeps the generated biome. Each terrain
//! sample covers one grid step and the averaged paint of that square decides it. Samples under
//! locks are skipped and reported. An import replaces what the document covers, so importing the
//! same document twice changes nothing.
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use image::RgbaImage;

use crate::brief::{BiomeReport, HeightReport, ImportReport, Rectangle};
use crate::grid::{self, Bounds, Grid, PixelRect, UNITS_PER_METRE};
use crate::ora;

const HEIGHT_NEUTRAL: i64 = 128;

/// `(row, column)`.
type Sample = (usize, usize);

fn covered_cells(grid: &Grid, bounds: Bounds) -> BTreeMap<Sample, PixelRect> {
    let (width, height) = bounds.size();
    let mut cells = BTreeMap::new();
    for row in 0..grid.rows {
        for column in 0..grid.columns {
            let rectangle = grid.cell_pixels(bounds, column, row);
            let [left, top, right, bottom] = rectangle;
            if left >= 0 && top >= 0 && right <= i64::from(width) && bottom <= i64::from(height) {
                cells.insert((row, column), rectangle);
            }
        }
    }
    cells
}

fn area([left, top, right, bottom]: PixelRect) -> i64 {
    (right - left) * (bottom - top)
}

/// Every covered sample's painted pixels: `(sample, red, green, blue, alpha)`.
fn painted<'a>(
    image: &'a RgbaImage,
    cells: &'a BTreeMap<Sample, PixelRect>,
) -> impl Iterator<Item = (Sample, [u8; 4])> + 'a {
    cells
        .iter()
        .flat_map(move |(sample, &[left, top, right, bottom])| {
            (top..bottom).flat_map(move |y| {
                (left..right).filter_map(move |x| {
                    let pixel = image.get_pixel(x as u32, y as u32).0;
                    (pixel[3] != 0).then_some((*sample, pixel))
                })
            })
        })
}

fn read_heights(
    image: &RgbaImage,
    cells: &BTreeMap<Sample, PixelRect>,
    step_units: i32,
) -> BTreeMap<Sample, i32> {
    let mut totals: BTreeMap<Sample, i64> = BTreeMap::new();
    for (sample, [grey, _, _, alpha]) in painted(image, cells) {
        *totals.entry(sample).or_default() += (i64::from(grey) - HEIGHT_NEUTRAL) * i64::from(alpha);
    }
    totals
        .into_iter()
        .filter_map(|(sample, total)| {
            let pixels = area(cells[&sample]);
            // Rounded half up in steps; a neutral square is simply absent.
            let steps = (2 * total + 255 * pixels).div_euclid(2 * 255 * pixels);
            (steps != 0).then_some((sample, steps as i32 * step_units))
        })
        .collect()
}

fn read_biomes(
    grid: &Grid,
    image: &RgbaImage,
    cells: &BTreeMap<Sample, PixelRect>,
) -> BTreeMap<Sample, i32> {
    let mut nearest: HashMap<[u8; 3], u8> = HashMap::new();
    let mut votes: BTreeMap<Sample, BTreeMap<u8, i64>> = BTreeMap::new();
    for (sample, [red, green, blue, alpha]) in painted(image, cells) {
        if alpha < 128 {
            continue;
        }
        let colour = [red, green, blue];
        let biome = *nearest.entry(colour).or_insert_with(|| {
            let distance = |entry: &&crate::grid::PaletteEntry| {
                entry
                    .color
                    .iter()
                    .zip(colour)
                    .map(|(&a, b)| (i32::from(a) - i32::from(b)).pow(2))
                    .sum::<i32>()
            };
            grid.palette
                .iter()
                .min_by_key(distance)
                .map_or(0, |entry| entry.id)
        });
        *votes.entry(sample).or_default().entry(biome).or_default() += 1;
    }
    votes
        .into_iter()
        .filter_map(|(sample, tally)| {
            let (biome, count) = tally
                .into_iter()
                .min_by_key(|&(biome, count)| (-count, biome))?;
            (2 * count > area(cells[&sample])).then_some((sample, i32::from(biome)))
        })
        .collect()
}

/// One sample per line, ordered by row, then column, so that diffs stay reviewable.
fn write_cells(path: &Path, cells: &BTreeMap<Sample, i32>) -> Result<(), String> {
    let lines: Vec<String> = cells
        .iter()
        .map(|((row, column), value)| format!("    [{column}, {row}, {value}]"))
        .collect();
    let body = if lines.is_empty() {
        "[]".to_owned()
    } else {
        format!("[\n{}\n  ]", lines.join(",\n"))
    };
    let temporary = path.with_extension("json.tmp");
    std::fs::write(
        &temporary,
        format!("{{\n  \"version\": 1,\n  \"cells\": {body}\n}}\n"),
    )
    .and_then(|()| std::fs::rename(&temporary, path))
    .map_err(|error| format!("{}: {error}", path.display()))
}

fn committed(cells: &[[i32; 3]]) -> BTreeMap<Sample, i32> {
    cells
        .iter()
        .map(|&[column, row, value]| ((row as usize, column as usize), value))
        .collect()
}

fn changed(before: &BTreeMap<Sample, i32>, after: &BTreeMap<Sample, i32>) -> BTreeSet<Sample> {
    before
        .keys()
        .chain(after.keys())
        .filter(|key| before.get(*key) != after.get(*key))
        .copied()
        .collect()
}

/// What `layers` change over the committed paint, and the paint that results.
fn plan(
    grid: &Grid,
    layers: &ora::PaintLayers,
) -> (ImportReport, BTreeMap<Sample, i32>, BTreeMap<Sample, i32>) {
    let cells = covered_cells(grid, layers.bounds);
    let painted_height = read_heights(&layers.height, &cells, grid.height_step_units);
    let skipped = painted_height
        .keys()
        .filter(|&&(row, column)| grid.locked[grid.index(column, row)] != 0)
        .count();
    let height = committed(&grid.painted_height);
    let mut next_height: BTreeMap<_, _> = height
        .iter()
        .filter(|(key, _)| !cells.contains_key(*key))
        .map(|(k, v)| (*k, *v))
        .collect();
    next_height.extend(
        painted_height
            .iter()
            .filter(|&(&(row, column), _)| grid.locked[grid.index(column, row)] == 0)
            .map(|(k, v)| (*k, *v)),
    );
    let biome = committed(&grid.painted_biome);
    let mut next_biome: BTreeMap<_, _> = biome
        .iter()
        .filter(|(key, _)| !cells.contains_key(*key))
        .map(|(k, v)| (*k, *v))
        .collect();
    next_biome.extend(read_biomes(grid, &layers.biome, &cells));

    let height_changed = changed(&height, &next_height);
    let biome_changed = changed(&biome, &next_biome);
    let moved: Vec<f64> = height_changed
        .iter()
        .map(|key| {
            f64::from(
                next_height.get(key).copied().unwrap_or(0) - height.get(key).copied().unwrap_or(0),
            ) / UNITS_PER_METRE
        })
        .collect();
    let touched: Vec<Sample> = height_changed.union(&biome_changed).copied().collect();
    let changed_bounds = (!touched.is_empty()).then(|| {
        let edge = |pick: fn(usize, usize) -> usize, of: fn(&Sample) -> usize| {
            touched.iter().map(of).reduce(pick).unwrap_or(0)
        };
        let column = |sample: &Sample| sample.1;
        let row = |sample: &Sample| sample.0;
        let (min_x, min_z) = grid.sample_metres(edge(usize::min, column), edge(usize::min, row));
        let (max_x, max_z) = grid.sample_metres(edge(usize::max, column), edge(usize::max, row));
        Rectangle {
            min_x,
            min_z,
            max_x,
            max_z,
        }
    });
    let report = ImportReport {
        height: HeightReport {
            changed: height_changed.len() as u32,
            painted: next_height.len() as u32,
            skipped_locked: skipped as u32,
            lowest_metres: moved.iter().copied().reduce(f64::min).unwrap_or(0.),
            highest_metres: moved.iter().copied().reduce(f64::max).unwrap_or(0.),
        },
        biome: BiomeReport {
            changed: biome_changed.len() as u32,
            painted: next_biome.len() as u32,
        },
        changed_bounds,
    };
    (report, next_height, next_biome)
}

/// Reads the paint in `document` against the checkout at `root`; writes it there unless `check`.
pub async fn run(
    root: &Path,
    document: &Path,
    check: bool,
    target_dir: Option<&Path>,
) -> Result<ImportReport, String> {
    let grid = grid::load(root, target_dir).await?;
    let document = document.to_path_buf();
    let paint: PathBuf = root.join("content/greyhaven/paint");
    tokio::task::spawn_blocking(move || {
        let layers = ora::read(&document)?;
        let (report, height, biome) = plan(&grid, &layers);
        if !check {
            write_cells(&paint.join("height.json"), &height)?;
            write_cells(&paint.join("biome.json"), &biome)?;
        }
        Ok(report)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use image::{Rgba, RgbaImage};

    use super::{covered_cells, plan};
    use crate::grid::{Bounds, Grid, PaletteEntry};
    use crate::ora::PaintLayers;

    const BOUNDS: Bounds = Bounds {
        min_x: 0,
        min_z: 0,
        max_x: 16,
        max_z: 16,
        ppm: 2,
    };

    fn grid(painted_height: Vec<[i32; 3]>, locked: Vec<u8>) -> Grid {
        Grid {
            origin: [0, 0],
            step: 400,
            columns: 5,
            rows: 5,
            height_step_units: 10,
            heights: vec![0; 25],
            biomes: vec![0; 25],
            road_biome: 5,
            water_biomes: vec![8],
            locked,
            palette: vec![
                PaletteEntry {
                    id: 0,
                    name: "meadow".into(),
                    color: [120, 200, 80],
                },
                PaletteEntry {
                    id: 11,
                    name: "rock".into(),
                    color: [120, 120, 125],
                },
            ],
            painted_height,
            painted_biome: vec![],
        }
    }

    fn fill(image: &mut RgbaImage, grid: &Grid, (column, row): (usize, usize), colour: [u8; 4]) {
        let [left, top, right, bottom] = grid.cell_pixels(BOUNDS, column, row);
        for y in top..bottom {
            for x in left..right {
                image.put_pixel(x as u32, y as u32, Rgba(colour));
            }
        }
    }

    fn blank() -> RgbaImage {
        RgbaImage::new(32, 32)
    }

    #[test]
    fn only_samples_wholly_inside_the_document_count() {
        let grid = grid(vec![], vec![0; 25]);
        let cells = covered_cells(&grid, BOUNDS);
        // Samples at 4, 8 and 12 m have their whole 4 m square inside a 16 m document.
        assert_eq!(cells.len(), 9);
        assert!(cells.contains_key(&(1, 1)) && !cells.contains_key(&(0, 1)));
    }

    #[test]
    fn solid_paint_moves_a_sample_and_a_lock_keeps_it() {
        let mut locked = vec![0; 25];
        locked[2 * 5 + 2] = 1;
        let grid = grid(vec![], locked);
        let mut height = blank();
        fill(&mut height, &grid, (1, 1), [138, 138, 138, 255]);
        fill(&mut height, &grid, (2, 2), [168, 168, 168, 255]);
        let mut biome = blank();
        fill(&mut biome, &grid, (3, 3), [120, 120, 125, 255]);
        let (report, next_height, next_biome) = plan(
            &grid,
            &PaintLayers {
                bounds: BOUNDS,
                height,
                biome,
            },
        );
        assert_eq!(next_height.len(), 1);
        assert_eq!(next_height[&(1, 1)], 100);
        assert_eq!(report.height.skipped_locked, 1);
        assert_eq!(next_biome[&(3, 3)], 11);
    }

    #[test]
    fn transparency_fades_towards_no_change() {
        let grid = grid(vec![], vec![0; 25]);
        let mut height = blank();
        fill(&mut height, &grid, (1, 1), [188, 188, 188, 128]);
        let (_, next_height, _) = plan(
            &grid,
            &PaintLayers {
                bounds: BOUNDS,
                height,
                biome: blank(),
            },
        );
        assert_eq!(next_height[&(1, 1)], 300);
    }

    #[test]
    fn importing_replaces_what_the_document_covers_and_keeps_the_rest() {
        // [1, 1] lies inside the document and is erased; [0, 0] lies outside and stays.
        let grid = grid(vec![[1, 1, 70], [0, 0, 20]], vec![0; 25]);
        let (report, next_height, _) = plan(
            &grid,
            &PaintLayers {
                bounds: BOUNDS,
                height: blank(),
                biome: blank(),
            },
        );
        assert_eq!(next_height.len(), 1);
        assert_eq!(next_height[&(0, 0)], 20);
        assert_eq!(report.height.changed, 1);
        assert_eq!(report.height.lowest_metres, -0.7);
    }
}
