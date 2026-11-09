//! The XYZ raster pyramid the map shows. Its whole Web Mercator world is a square centred on
//! the zone's origin, so game metres map to map pixels without distortion.
use std::collections::BTreeMap;
use std::path::Path;

use image::imageops::{self, FilterType};
use image::{DynamicImage, RgbImage, RgbaImage};

use crate::grid::Bounds;
use crate::ora::png_bytes;

pub const WORLD_METRES: i64 = 2_048;
const TILE_PIXELS: u32 = 256;

type Level = BTreeMap<(i64, i64), RgbaImage>;

/// One 2x1-reduced tile: each result pixel is the alpha-weighted average of four.
fn halved(tile: &RgbaImage) -> RgbaImage {
    RgbaImage::from_fn(TILE_PIXELS / 2, TILE_PIXELS / 2, |x, y| {
        let mut alpha = 0u32;
        let mut colour = [0u32; 3];
        for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let [red, green, blue, a] = tile.get_pixel(x * 2 + dx, y * 2 + dy).0;
            let a = u32::from(a);
            alpha += a;
            for (total, channel) in colour.iter_mut().zip([red, green, blue]) {
                *total += u32::from(channel) * a;
            }
        }
        let share = |total: u32| (total + alpha / 2).checked_div(alpha).unwrap_or(0) as u8;
        image::Rgba([
            share(colour[0]),
            share(colour[1]),
            share(colour[2]),
            ((alpha + 2) / 4) as u8,
        ])
    })
}

fn save(out: &Path, zoom: u32, level: &Level) -> Result<(), String> {
    for ((x, y), tile) in level {
        let path = out
            .join("tiles")
            .join(zoom.to_string())
            .join(x.to_string())
            .join(format!("{y}.png"));
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        std::fs::write(&path, png_bytes(&DynamicImage::ImageRgba8(tile.clone()))?)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// Writes the world image as a pyramid plus a raster style for the map; returns the maximum zoom.
pub fn write(out: &Path, world: &RgbImage, bounds: Bounds, url: &str) -> Result<u32, String> {
    let ppm = f64::from(bounds.ppm);
    let max_zoom = ((WORLD_METRES as f64 * ppm / f64::from(TILE_PIXELS))
        .log2()
        .ceil())
    .max(0.) as u32;
    let tile_ppm = f64::from(TILE_PIXELS) * f64::from(2u32.pow(max_zoom)) / WORLD_METRES as f64;
    let image = DynamicImage::ImageRgb8(world.clone()).to_rgba8();
    let image = if (tile_ppm - ppm).abs() > f64::EPSILON {
        let scale = tile_ppm / ppm;
        imageops::resize(
            &image,
            (f64::from(image.width()) * scale).round() as u32,
            (f64::from(image.height()) * scale).round() as u32,
            FilterType::Triangle,
        )
    } else {
        image
    };
    let half = WORLD_METRES / 2;
    let side = i64::from(TILE_PIXELS);
    let left = ((i64::from(bounds.min_x) + half) as f64 * tile_ppm).round() as i64;
    let top = ((i64::from(bounds.min_z) + half) as f64 * tile_ppm).round() as i64;
    let mut level: Level = BTreeMap::new();
    for y in top.div_euclid(side)..=(top + i64::from(image.height()) - 1).div_euclid(side) {
        for x in left.div_euclid(side)..=(left + i64::from(image.width()) - 1).div_euclid(side) {
            let mut tile = RgbaImage::new(TILE_PIXELS, TILE_PIXELS);
            imageops::replace(&mut tile, &image, left - x * side, top - y * side);
            level.insert((x, y), tile);
        }
    }
    save(out, max_zoom, &level)?;
    for zoom in (0..max_zoom).rev() {
        let mut parents: Level = BTreeMap::new();
        for ((x, y), tile) in &level {
            let parent = parents
                .entry((x.div_euclid(2), y.div_euclid(2)))
                .or_insert_with(|| RgbaImage::new(TILE_PIXELS, TILE_PIXELS));
            let quarter = i64::from(TILE_PIXELS / 2);
            imageops::replace(
                parent,
                &halved(tile),
                x.rem_euclid(2) * quarter,
                y.rem_euclid(2) * quarter,
            );
        }
        save(out, zoom, &parents)?;
        level = parents;
    }
    let style = serde_json::json!({
        "version": 8,
        "sources": {"greyhaven": {
            "type": "raster", "tiles": [url], "tileSize": TILE_PIXELS,
            "minzoom": 0, "maxzoom": max_zoom, "attribution": "Greyhaven Vale",
        }},
        "layers": [{"id": "greyhaven", "type": "raster", "source": "greyhaven"}],
    });
    let text = serde_json::to_string_pretty(&style).map_err(|error| error.to_string())?;
    std::fs::write(out.join("tiles/style.json"), text + "\n").map_err(|error| error.to_string())?;
    Ok(max_zoom)
}

#[cfg(test)]
mod tests {
    use image::{Rgba, RgbaImage};

    use super::halved;

    #[test]
    fn halving_ignores_the_colour_of_transparent_pixels() {
        let mut tile = RgbaImage::new(256, 256);
        tile.put_pixel(0, 0, Rgba([200, 100, 0, 255]));
        tile.put_pixel(1, 0, Rgba([9, 9, 9, 0]));
        let reduced = halved(&tile);
        assert_eq!(reduced.dimensions(), (128, 128));
        assert_eq!(reduced.get_pixel(0, 0).0, [200, 100, 0, 64]);
        assert_eq!(reduced.get_pixel(5, 5).0, [0, 0, 0, 0]);
    }
}
