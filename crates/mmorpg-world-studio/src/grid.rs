//! Greyhaven's terrain grid, and how a document's pixels line up with it.
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use serde::Deserialize;
use tokio::process::Command;

pub const UNITS_PER_METRE: f64 = 100.;
const GRID_TIMEOUT: Duration = Duration::from_secs(15 * 60);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Grid {
    pub origin: [i32; 2],
    pub step: i32,
    pub columns: usize,
    pub rows: usize,
    pub height_step_units: i32,
    pub heights: Vec<i32>,
    pub biomes: Vec<u8>,
    pub road_biome: u8,
    pub water_biomes: Vec<u8>,
    pub locked: Vec<u8>,
    pub palette: Vec<PaletteEntry>,
    pub painted_height: Vec<[i32; 3]>,
    pub painted_biome: Vec<[i32; 3]>,
}

#[derive(Clone, Deserialize, serde::Serialize)]
pub struct PaletteEntry {
    pub id: u8,
    pub name: String,
    pub color: [u8; 3],
}

/// The grid of the checkout at `root`, built from its own committed paint: a branch's worktree
/// answers for that branch, which code compiled into this binary could not.
pub async fn load(root: &Path, target_dir: Option<&Path>) -> Result<Grid, String> {
    let mut command = Command::new("cargo");
    command
        .args(["run", "-q", "--locked", "-p", "mmorpg-scenery", "--example"])
        .arg("export-world-layers")
        .current_dir(root)
        .stdin(Stdio::null())
        .kill_on_drop(true);
    if let Some(target) = target_dir {
        command.env("CARGO_TARGET_DIR", target);
    }
    let output = tokio::time::timeout(GRID_TIMEOUT, command.output())
        .await
        .map_err(|_| "reading the terrain grid timed out".to_owned())?
        .map_err(|error| format!("could not start cargo: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "reading the terrain grid failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("unreadable terrain grid: {error}"))
}

/// A document's world rectangle in whole metres, and its scale.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Bounds {
    pub min_x: i32,
    pub min_z: i32,
    pub max_x: i32,
    pub max_z: i32,
    pub ppm: u32,
}

impl Bounds {
    /// The document's size in pixels.
    pub fn size(self) -> (u32, u32) {
        (
            self.max_x.abs_diff(self.min_x) * self.ppm,
            self.max_z.abs_diff(self.min_z) * self.ppm,
        )
    }

    /// Krita drops unknown files on save but keeps layer names, so geometry lives in one.
    pub fn layer_name(self) -> String {
        format!(
            "{BOUNDS_PREFIX} {} {} {} {} {}",
            self.min_x, self.min_z, self.max_x, self.max_z, self.ppm
        )
    }

    pub fn parse(name: &str) -> Option<Self> {
        let mut values = name.strip_prefix(BOUNDS_PREFIX)?.split_whitespace();
        let mut next = || values.next()?.parse::<i32>().ok();
        let bounds = Self {
            min_x: next()?,
            min_z: next()?,
            max_x: next()?,
            max_z: next()?,
            ppm: u32::try_from(next()?).ok()?,
        };
        (bounds.ppm > 0 && bounds.max_x > bounds.min_x && bounds.max_z > bounds.min_z)
            .then_some(bounds)
    }
}

pub const BOUNDS_PREFIX: &str = "_bounds";

/// A pixel rectangle `[left, top, right, bottom)`, possibly reaching outside its document.
pub type PixelRect = [i64; 4];

impl Grid {
    pub fn index(&self, column: usize, row: usize) -> usize {
        row * self.columns + column
    }

    fn step_metres(&self) -> f64 {
        f64::from(self.step) / UNITS_PER_METRE
    }

    /// The pixel rectangle a terrain sample owns: the grid step centred on the sample.
    pub fn cell_pixels(&self, bounds: Bounds, column: usize, row: usize) -> PixelRect {
        let step = self.step_metres();
        let half = step / 2.;
        let ppm = f64::from(bounds.ppm);
        let x = f64::from(self.origin[0]) / UNITS_PER_METRE + column as f64 * step;
        let z = f64::from(self.origin[1]) / UNITS_PER_METRE + row as f64 * step;
        let left = ((x - half - f64::from(bounds.min_x)) * ppm).round_ties_even() as i64;
        let top = ((z - half - f64::from(bounds.min_z)) * ppm).round_ties_even() as i64;
        let side = (step * ppm).round_ties_even() as i64;
        [left, top, left + side, top + side]
    }

    /// The samples near the document as `(first column, first row, end column, end row)`.
    pub fn covered(&self, bounds: Bounds) -> (usize, usize, usize, usize) {
        let step = self.step_metres();
        let first = |value: i32, origin: i32| {
            ((f64::from(value) - f64::from(origin) / UNITS_PER_METRE) / step)
                .floor()
                .max(0.) as usize
        };
        let last = |value: i32, origin: i32| {
            (((f64::from(value) - f64::from(origin) / UNITS_PER_METRE) / step).ceil() + 1.).max(0.)
                as usize
        };
        (
            first(bounds.min_x, self.origin[0]),
            first(bounds.min_z, self.origin[1]),
            last(bounds.max_x, self.origin[0]).min(self.columns),
            last(bounds.max_z, self.origin[1]).min(self.rows),
        )
    }

    /// World XZ of a sample, in metres.
    pub fn sample_metres(&self, column: usize, row: usize) -> (f64, f64) {
        let step = self.step_metres();
        (
            f64::from(self.origin[0]) / UNITS_PER_METRE + column as f64 * step,
            f64::from(self.origin[1]) / UNITS_PER_METRE + row as f64 * step,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::Bounds;

    #[test]
    fn bounds_survive_a_layer_name_and_refuse_nonsense() {
        let bounds = Bounds {
            min_x: -43,
            min_z: -21,
            max_x: 43,
            max_z: 61,
            ppm: 8,
        };
        assert_eq!(Bounds::parse(&bounds.layer_name()), Some(bounds));
        assert_eq!(bounds.size(), (688, 656));
        for bad in [
            "_bounds 1 2 3",
            "_bounds 5 5 1 9 8",
            "_bounds 0 0 1 1 0",
            "beauty",
        ] {
            assert_eq!(Bounds::parse(bad), None, "{bad}");
        }
    }
}
