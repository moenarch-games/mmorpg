//! The data layers of a document, rasterised from the terrain grid.
use image::{GrayImage, ImageBuffer, Luma, Pixel, Rgb, RgbImage, Rgba, RgbaImage};

use crate::grid::{Bounds, Grid, PixelRect};
use crate::ora::{Layer, PAINT_BIOME, PAINT_HEIGHT};

const UNPAINTABLE_COLOUR: [u8; 3] = [90, 90, 90];
const LOCK_COLOUR: [u8; 4] = [230, 40, 40, 110];
const HEIGHT_NEUTRAL: i32 = 128;
const NO_PAINT: [u8; 4] = [0, 0, 0, 0];
/// A sun in the north-west, 45 degrees up, as maps are conventionally shaded.
const SUN: [f64; 3] = [-0.5, std::f64::consts::FRAC_1_SQRT_2, -0.5];
/// Steeper ground than this reads as white in the slope layer (degrees).
const FULL_SLOPE_DEGREES: f64 = 45.;

fn fill<P: Pixel>(
    image: &mut ImageBuffer<P, Vec<P::Subpixel>>,
    [left, top, right, bottom]: PixelRect,
    colour: P,
) {
    let (width, height) = (i64::from(image.width()), i64::from(image.height()));
    for y in top.max(0)..bottom.min(height) {
        for x in left.max(0)..right.min(width) {
            image.put_pixel(x as u32, y as u32, colour);
        }
    }
}

/// A document-sized image giving each sample's square `colour_of(column, row)`.
fn rasterise<P: Pixel<Subpixel = u8>>(
    grid: &Grid,
    bounds: Bounds,
    colour_of: impl Fn(usize, usize) -> P,
) -> ImageBuffer<P, Vec<u8>> {
    let (width, height) = bounds.size();
    let mut image = ImageBuffer::new(width, height);
    let (left, top, right, bottom) = grid.covered(bounds);
    for row in top..bottom {
        for column in left..right {
            fill(
                &mut image,
                grid.cell_pixels(bounds, column, row),
                colour_of(column, row),
            );
        }
    }
    image
}

fn byte(value: f64) -> u8 {
    value.round_ties_even().clamp(0., 255.) as u8
}

/// Per sample, the hillshade (0..=255) and the slope in degrees of the physical ground.
pub struct Shading {
    shade: Vec<u8>,
    slope: Vec<f64>,
}

impl Shading {
    pub fn of(grid: &Grid) -> Self {
        let span = f64::from(2 * grid.step);
        let at = |column: i64, row: i64| {
            let row = row.clamp(0, grid.rows as i64 - 1) as usize;
            let column = column.clamp(0, grid.columns as i64 - 1) as usize;
            f64::from(grid.heights[grid.index(column, row)])
        };
        let mut shade = vec![0; grid.heights.len()];
        let mut slope = vec![0.; grid.heights.len()];
        for row in 0..grid.rows {
            for column in 0..grid.columns {
                let (c, r) = (column as i64, row as i64);
                let dx = (at(c + 1, r) - at(c - 1, r)) / span;
                let dz = (at(c, r + 1) - at(c, r - 1)) / span;
                let length = (dx * dx + dz * dz + 1.).sqrt();
                let light = (-dx * SUN[0] + SUN[1] - dz * SUN[2]) / length;
                let index = grid.index(column, row);
                shade[index] = byte(light * 255.);
                slope[index] = dx.hypot(dz).atan().to_degrees();
            }
        }
        Self { shade, slope }
    }
}

/// Reference layers of one document.
pub struct DataLayers {
    pub height: GrayImage,
    pub hillshade: GrayImage,
    pub slope: GrayImage,
    pub biome: RgbImage,
    pub roads: GrayImage,
    pub water: GrayImage,
    pub locks: RgbaImage,
}

impl DataLayers {
    pub fn of(grid: &Grid, shading: &Shading, bounds: Bounds) -> Self {
        let colour = |id: u8| {
            grid.palette
                .iter()
                .find(|entry| entry.id == id)
                .map_or(UNPAINTABLE_COLOUR, |entry| entry.color)
        };
        let (left, top, right, bottom) = grid.covered(bounds);
        let window = (top..bottom)
            .flat_map(|row| (left..right).map(move |column| (column, row)))
            .map(|(column, row)| grid.heights[grid.index(column, row)]);
        let low = window.clone().min().unwrap_or(0);
        let high = window.max().unwrap_or(0);
        let span = f64::from((high - low).max(1));
        let biome_at = |c: usize, r: usize| grid.biomes[grid.index(c, r)];
        let mask = |on: bool| Luma([if on { 255 } else { 0 }]);
        Self {
            height: rasterise(grid, bounds, |c, r| {
                Luma([byte(
                    f64::from(grid.heights[grid.index(c, r)] - low) * 255. / span,
                )])
            }),
            hillshade: rasterise(grid, bounds, |c, r| Luma([shading.shade[grid.index(c, r)]])),
            slope: rasterise(grid, bounds, |c, r| {
                Luma([byte(
                    shading.slope[grid.index(c, r)] * 255. / FULL_SLOPE_DEGREES,
                )])
            }),
            biome: rasterise(grid, bounds, |c, r| Rgb(colour(biome_at(c, r)))),
            roads: rasterise(grid, bounds, |c, r| mask(biome_at(c, r) == grid.road_biome)),
            water: rasterise(grid, bounds, |c, r| {
                mask(grid.water_biomes.contains(&biome_at(c, r)))
            }),
            locks: rasterise(grid, bounds, |c, r| {
                Rgba(if grid.locked[grid.index(c, r)] != 0 {
                    LOCK_COLOUR
                } else {
                    NO_PAINT
                })
            }),
        }
    }
}

/// The physical heights at one pixel per sample: 16-bit, in centimetres offset by 32768.
pub fn native_height(grid: &Grid, bounds: Bounds) -> ImageBuffer<Luma<u16>, Vec<u16>> {
    let (left, top, right, bottom) = grid.covered(bounds);
    ImageBuffer::from_fn((right - left) as u32, (bottom - top) as u32, |x, y| {
        let height = grid.heights[grid.index(left + x as usize, top + y as usize)];
        Luma([u16::try_from(height + 32_768).unwrap_or(0)])
    })
}

/// The committed paint inside the document, so that re-importing it changes nothing.
fn paint_layer(
    grid: &Grid,
    bounds: Bounds,
    cells: &[[i32; 3]],
    colour_of: impl Fn(i32) -> Rgba<u8>,
) -> RgbaImage {
    let (width, height) = bounds.size();
    let mut image = RgbaImage::new(width, height);
    for &[column, row, value] in cells {
        fill(
            &mut image,
            grid.cell_pixels(bounds, column as usize, row as usize),
            colour_of(value),
        );
    }
    image
}

/// A document's layers, bottom to top.
pub fn document(
    grid: &Grid,
    bounds: Bounds,
    beauty: RgbImage,
    ground: RgbImage,
    data: DataLayers,
) -> Vec<Layer> {
    let colour = |id: i32| {
        let entry = grid.palette.iter().find(|entry| i32::from(entry.id) == id);
        let [red, green, blue] = entry.map_or(UNPAINTABLE_COLOUR, |entry| entry.color);
        Rgba([red, green, blue, 255])
    };
    let grey = |delta: i32| {
        let value = (HEIGHT_NEUTRAL + delta / grid.height_step_units).clamp(0, 255) as u8;
        Rgba([value, value, value, 255])
    };
    let (width, height) = bounds.size();
    vec![
        Layer::new("beauty", beauty),
        Layer::new("ground", ground).hidden(),
        Layer::new("hillshade", data.hillshade)
            .hidden()
            .opacity(0.5),
        Layer::new("height", data.height).hidden(),
        Layer::new("slope", data.slope).hidden(),
        Layer::new("biome", data.biome).hidden().opacity(0.6),
        Layer::new("roads", data.roads).hidden().opacity(0.5),
        Layer::new("water", data.water).hidden().opacity(0.5),
        Layer::new("locks", data.locks),
        Layer::new(
            PAINT_BIOME,
            paint_layer(grid, bounds, &grid.painted_biome, colour),
        )
        .opacity(0.7),
        Layer::new(
            PAINT_HEIGHT,
            paint_layer(grid, bounds, &grid.painted_height, grey),
        )
        .opacity(0.7),
        Layer::new(bounds.layer_name(), RgbaImage::new(width, height)).hidden(),
    ]
}
