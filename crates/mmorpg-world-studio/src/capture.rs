//! Photographs the built game from above and writes region documents, layers and map tiles.
//!
//! Rendered layers are orthographic, north up, at noon, without units, fog or interface, stitched
//! from small tiles because scenery is culled by its distance from the camera. +X runs right and
//! +Z down: `world_x = min_x + pixel_x / pixels_per_metre`, and likewise for Z and rows.
use std::io::Cursor;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::Router;
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::emulation::{MediaFeature, SetEmulatedMediaParams};
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
use chromiumoxide::handler::viewport::Viewport;
use chromiumoxide::page::{Page, ScreenshotParams};
use futures_util::StreamExt;
use image::{ImageFormat, RgbImage, imageops};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use tower_http::services::ServeDir;

use crate::grid::{self, Bounds, Grid, UNITS_PER_METRE};
use crate::layers::{self, DataLayers, Shading};
use crate::ora;
use crate::tiles;

/// Small enough that a tile's corners stay inside the 30 m cull distance of the finest scenery.
const TILE_METRES: i32 = 40;
/// Enough frames for streamed terrain and level-of-detail fades to settle after the camera moves.
const SETTLE_FRAMES: u32 = 8;
const LOAD_TIMEOUT: Duration = Duration::from_secs(120);
const ART_TIMEOUT: Duration = Duration::from_secs(180);
const POLL: Duration = Duration::from_millis(100);
const HIDE_INTERFACE: &str =
    "* { visibility: hidden !important; } canvas#world { visibility: visible !important; }";
/// The debug presentation clock at noon: short shadows and even light to paint over.
const NOON_SECONDS: u32 = 0;
const LAYER_FILES: [&str; 6] = ["hillshade", "slope", "biome", "roads", "water", "locks"];

/// What to capture.
pub struct Request {
    pub root: PathBuf,
    pub out: PathBuf,
    pub only: Vec<String>,
    pub pixels_per_metre: u32,
    pub margin: f64,
    pub tiles: bool,
    pub tiles_only: bool,
    pub tiles_url: String,
    pub bounds: Option<[i32; 4]>,
    pub name: String,
    pub list: bool,
    pub target_dir: Option<PathBuf>,
}

pub type Log<'a> = &'a (dyn Fn(String) + Send + Sync);

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Area {
    id: u32,
    name: String,
    min_xz: [i32; 2],
    max_xz: [i32; 2],
}

fn io(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn slug(name: &str) -> String {
    let mut text = String::new();
    for character in name.to_lowercase().chars() {
        if character.is_ascii_alphanumeric() {
            text.push(character);
        } else if !text.ends_with('-') {
            text.push('-');
        }
    }
    text.trim_matches('-').to_owned()
}

fn bounds_of(min_xz: [i32; 2], max_xz: [i32; 2], margin: f64, ppm: u32) -> Bounds {
    // `max_xz` is inclusive in units; the rectangle ends one unit past it.
    let low = |value: i32| (f64::from(value) / UNITS_PER_METRE - margin).floor() as i32;
    let high = |value: i32| (f64::from(value + 1) / UNITS_PER_METRE + margin).ceil() as i32;
    Bounds {
        min_x: low(min_xz[0]),
        min_z: low(min_xz[1]),
        max_x: high(max_xz[0]),
        max_z: high(max_xz[1]),
        ppm,
    }
}

/// The Chromium to drive: Playwright's headless build when set up, otherwise a system browser.
fn chrome() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("WORLD_STUDIO_CHROME") {
        return Ok(path.into());
    }
    let newest = |prefix: &str, binary: &str| -> Option<PathBuf> {
        let cache = PathBuf::from(std::env::var_os("HOME")?).join(".cache/ms-playwright");
        std::fs::read_dir(cache)
            .ok()?
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let name = entry.file_name().into_string().ok()?;
                let revision = name.strip_prefix(prefix)?.parse::<u32>().ok()?;
                let path = entry.path().join(binary);
                path.is_file().then_some((revision, path))
            })
            .max_by_key(|(revision, _)| *revision)
            .map(|(_, path)| path)
    };
    newest(
        "chromium_headless_shell-",
        "chrome-headless-shell-linux64/chrome-headless-shell",
    )
    .or_else(|| newest("chromium-", "chrome-linux64/chrome"))
    .or_else(|| {
        let paths = std::env::var_os("PATH")?;
        std::env::split_paths(&paths)
            .flat_map(|directory| ["google-chrome", "chromium"].map(|name| directory.join(name)))
            .find(|path| path.is_file())
    })
    .ok_or_else(|| {
        "no Chromium found: run mise run setup:browser or set WORLD_STUDIO_CHROME".to_owned()
    })
}

async fn eval<T: DeserializeOwned>(page: &Page, expression: &str) -> Result<T, String> {
    page.evaluate(expression)
        .await
        .map_err(|error| format!("{expression}: {error}"))?
        .into_value()
        .map_err(|error| format!("{expression}: {error}"))
}

async fn wait_until(page: &Page, condition: &str, timeout: Duration) -> Result<(), String> {
    let start = Instant::now();
    loop {
        if eval::<bool>(page, condition).await.unwrap_or(false) {
            return Ok(());
        }
        if start.elapsed() > timeout {
            return Err(format!("timed out waiting for {condition}"));
        }
        tokio::time::sleep(POLL).await;
    }
}

async fn frames(page: &Page, count: u32) -> Result<(), String> {
    let wait = format!(
        "new Promise(resolve => {{ let count = {count}; \
         function next() {{ if (--count <= 0) resolve(true); else requestAnimationFrame(next); }} \
         requestAnimationFrame(next); }})"
    );
    eval::<bool>(page, &wait).await.map(|_| ())
}

const ENTER_BUTTON: &str =
    "[...document.querySelectorAll('button')].find(b => /^Enter World/.test(b.textContent.trim()))";
const SHOWN: &str = "(el => !!el && el.getClientRects().length > 0)";

async fn enter_world(page: &Page, url: &str) -> Result<(), String> {
    page.goto(format!("{url}?debug")).await.map_err(io)?;
    let ready = format!("(b => !!b && !b.disabled)({ENTER_BUTTON})");
    wait_until(page, &ready, LOAD_TIMEOUT).await?;
    eval::<bool>(page, &format!("({ENTER_BUTTON}.click(), true)")).await?;
    let entered = format!(
        "!{SHOWN}(document.querySelector('#character-select')) && {SHOWN}(document.querySelector('.hud'))"
    );
    wait_until(page, &entered, LOAD_TIMEOUT).await?;
    let style = format!(
        "(s => {{ s.textContent = {}; document.head.append(s); return true; }})(document.createElement('style'))",
        serde_json::to_string(HIDE_INTERFACE).map_err(io)?
    );
    eval::<bool>(page, &style).await.map(|_| ())
}

async fn wait_for_art(page: &Page) -> Result<(), String> {
    eval::<bool>(page, "(window.__valeDebug.fetchAllArt(), true)").await?;
    let settled = "(() => { const d = window.__valeDebug, art = d.loadingReport().allArt; \
                   return art.loaded === art.total && !!d.nature()?.loaded && !!d.climbing()?.loaded && d.ground() !== null; })()";
    if wait_until(page, settled, ART_TIMEOUT).await.is_err() {
        let pending = "(d => JSON.stringify({art: d.loadingReport().allArt, nature: d.nature(), \
                       climbing: d.climbing(), ground: d.ground()?.tiles ?? null}))(window.__valeDebug)";
        let state: String = eval(page, pending).await?;
        return Err(format!("the game's art did not finish loading: {state}"));
    }
    Ok(())
}

/// Stitches one image of `bounds` from square orthographic tiles.
async fn photograph(page: &Page, bounds: Bounds, ground_only: bool) -> Result<RgbImage, String> {
    let tile_pixels = TILE_METRES as u32 * bounds.ppm;
    let (width, height) = bounds.size();
    let tiles_across = |extent: u32| (extent / bounds.ppm).div_ceil(TILE_METRES as u32) as i32;
    let (columns, rows) = (tiles_across(width), tiles_across(height));
    let mut image = RgbImage::new(width, height);
    for row in 0..rows {
        for column in 0..columns {
            let min_x = bounds.min_x + column * TILE_METRES;
            let min_z = bounds.min_z + row * TILE_METRES;
            let plan = format!(
                "(window.__valeDebug.planView({{minX: {min_x}, minZ: {min_z}, maxX: {}, maxZ: {}, groundOnly: {ground_only}}}), true)",
                min_x + TILE_METRES,
                min_z + TILE_METRES
            );
            eval::<bool>(page, &plan).await?;
            frames(page, SETTLE_FRAMES).await?;
            let shot = page
                .screenshot(
                    ScreenshotParams::builder()
                        .format(CaptureScreenshotFormat::Png)
                        .build(),
                )
                .await
                .map_err(io)?;
            let tile = image::load(Cursor::new(shot), ImageFormat::Png)
                .map_err(io)?
                .to_rgb8();
            if tile.dimensions() != (tile_pixels, tile_pixels) {
                return Err(format!(
                    "expected a {tile_pixels}px tile, the browser gave {:?}",
                    tile.dimensions()
                ));
            }
            imageops::replace(
                &mut image,
                &tile,
                i64::from(column) * i64::from(tile_pixels),
                i64::from(row) * i64::from(tile_pixels),
            );
        }
    }
    Ok(image)
}

/// Serves the built game for as long as it is held.
struct GameServer {
    task: tokio::task::JoinHandle<()>,
    url: String,
}

impl Drop for GameServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn serve_game(root: &Path) -> Result<GameServer, String> {
    let dist = root.join("web/dist");
    if !dist.join("index.html").is_file() {
        return Err("build the game first: (cd web && bun run build)".to_owned());
    }
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(io)?;
    let port = listener.local_addr().map_err(io)?.port();
    let app = Router::new().nest_service("/mmorpg", ServeDir::new(dist));
    let task = tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, app).await {
            eprintln!("world-studio: the game server stopped: {error}");
        }
    });
    Ok(GameServer {
        task,
        url: format!("http://127.0.0.1:{port}/mmorpg/"),
    })
}

/// A launched browser: closed and its profile removed by `shut`, killed by drop on failure.
struct Headless {
    browser: Browser,
    pump: tokio::task::JoinHandle<()>,
    profile: PathBuf,
}

impl Headless {
    async fn launch(tile_pixels: u32) -> Result<Self, String> {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        let profile = std::env::temp_dir().join(format!("world-studio-chrome-{unique}"));
        let config = BrowserConfig::builder()
            .chrome_executable(chrome()?)
            .user_data_dir(&profile)
            // Matches Playwright: Ubuntu blocks the sandbox here, and only the local build is loaded.
            .no_sandbox()
            .arg("enable-gpu")
            .arg(("use-angle", "gl"))
            .arg("ignore-gpu-blocklist")
            .launch_timeout(LOAD_TIMEOUT)
            .request_timeout(LOAD_TIMEOUT)
            .viewport(Viewport {
                width: tile_pixels,
                height: tile_pixels,
                device_scale_factor: Some(1.),
                emulating_mobile: false,
                is_landscape: false,
                has_touch: false,
            })
            .build()?;
        let (browser, mut handler) = Browser::launch(config).await.map_err(io)?;
        // Protocol errors for frames the page dropped are not fatal; the pump must keep going.
        let pump = tokio::spawn(async move { while handler.next().await.is_some() {} });
        Ok(Self {
            browser,
            pump,
            profile,
        })
    }

    async fn page(&self) -> Result<Page, String> {
        let page = self.browser.new_page("about:blank").await.map_err(io)?;
        page.execute(
            SetEmulatedMediaParams::builder()
                .feature(MediaFeature::new("prefers-reduced-motion", "reduce"))
                .build(),
        )
        .await
        .map_err(io)?;
        Ok(page)
    }

    async fn shut(mut self) {
        // Best effort: the process is already being torn down, and Drop kills whatever remains.
        let _ = self.browser.close().await;
        let _ = self.browser.wait().await;
        self.pump.abort();
        let _ = std::fs::remove_dir_all(&self.profile);
    }
}

/// Writes one area's layers and Krita document.
#[allow(clippy::too_many_arguments)]
async fn write_region(
    page: &Page,
    grid: &Grid,
    shading: &Shading,
    out: &Path,
    name: &str,
    bounds: Bounds,
) -> Result<(), String> {
    let folder = out.join(name);
    std::fs::create_dir_all(&folder).map_err(io)?;
    let beauty = photograph(page, bounds, false).await?;
    let ground = photograph(page, bounds, true).await?;
    let data = DataLayers::of(grid, shading, bounds);
    beauty.save(folder.join("beauty.png")).map_err(io)?;
    ground.save(folder.join("ground.png")).map_err(io)?;
    layers::native_height(grid, bounds)
        .save(folder.join("height.png"))
        .map_err(io)?;
    for (name, saved) in LAYER_FILES.iter().zip([
        data.hillshade.save(folder.join("hillshade.png")),
        data.slope.save(folder.join("slope.png")),
        data.biome.save(folder.join("biome.png")),
        data.roads.save(folder.join("roads.png")),
        data.water.save(folder.join("water.png")),
        data.locks.save(folder.join("locks.png")),
    ]) {
        saved.map_err(|error| format!("{name}: {error}"))?;
    }
    let document = layers::document(grid, bounds, beauty, ground, data);
    ora::write(&out.join(format!("{name}.ora")), bounds, &document)
}

/// Runs a capture; with `list`, only prints the areas.
pub async fn run(request: Request, log: Log<'_>) -> Result<(), String> {
    let game = serve_game(&request.root).await?;
    let ppm = request.pixels_per_metre;
    let headless = Headless::launch(TILE_METRES as u32 * ppm).await?;
    let result = capture(&request, &game.url, &headless, log).await;
    headless.shut().await;
    result
}

async fn capture(
    request: &Request,
    url: &str,
    headless: &Headless,
    log: Log<'_>,
) -> Result<(), String> {
    let ppm = request.pixels_per_metre;
    let page = headless.page().await?;
    enter_world(&page, url).await?;
    let areas: Vec<Area> = eval(&page, "window.__valeDebug.areas()").await?;
    let mut margin = request.margin;
    let wanted: Vec<Area> = if request.tiles_only {
        Vec::new()
    } else if let Some([min_x, min_z, max_x, max_z]) = request.bounds {
        margin = 0.;
        let unit = UNITS_PER_METRE as i32;
        vec![Area {
            id: 0,
            name: request.name.clone(),
            min_xz: [min_x * unit, min_z * unit],
            max_xz: [max_x * unit - 1, max_z * unit - 1],
        }]
    } else {
        let matches = |area: &Area| {
            let name = area.name.to_lowercase();
            request.only.is_empty()
                || request
                    .only
                    .iter()
                    .any(|term| name.contains(&term.to_lowercase()))
        };
        areas.iter().filter(|area| matches(area)).cloned().collect()
    };
    if request.list {
        for area in &wanted {
            let bounds = bounds_of(area.min_xz, area.max_xz, margin, ppm);
            println!(
                "{:>4} {}: {} x {} m",
                area.id,
                area.name,
                bounds.max_x - bounds.min_x,
                bounds.max_z - bounds.min_z
            );
        }
        return Ok(());
    }
    if wanted.is_empty() && !request.tiles_only {
        let names: Vec<_> = areas.iter().map(|area| area.name.as_str()).collect();
        return Err(format!(
            "no area matches {:?}; the zone has: {}",
            request.only,
            names.join(", ")
        ));
    }
    let grid = grid::load(&request.root, request.target_dir.as_deref()).await?;
    let shading = Shading::of(&grid);
    wait_for_art(&page).await?;
    eval::<bool>(
        &page,
        &format!("(window.__valeDebug.setPresentationTime({NOON_SECONDS}), true)"),
    )
    .await?;
    std::fs::create_dir_all(&request.out).map_err(io)?;
    let mut entries = Vec::new();
    for area in &wanted {
        let bounds = bounds_of(area.min_xz, area.max_xz, margin, ppm);
        let name = slug(&area.name);
        log(format!(
            "{} -> {}.ora",
            area.name,
            request.out.join(&name).display()
        ));
        write_region(&page, &grid, &shading, &request.out, &name, bounds).await?;
        let (width, height) = bounds.size();
        entries.push(serde_json::json!({
            "id": area.id, "name": area.name, "slug": name, "document": format!("{name}.ora"),
            "pixels_per_metre": ppm, "size_pixels": [width, height],
            "bounds_metres": {"min_x": bounds.min_x, "min_z": bounds.min_z, "max_x": bounds.max_x, "max_z": bounds.max_z},
        }));
    }
    let mut manifest = serde_json::json!({
        "pixels_per_metre": ppm,
        "axes": {"right": "+x", "down": "+z"},
        "paint": {"neutral_grey": 128, "height_step_units": grid.height_step_units, "biomes": grid.palette, "bounds_layer": grid::BOUNDS_PREFIX},
        "regions": entries,
    });
    if request.tiles || request.tiles_only {
        // One rectangle over every area, so the map has no holes between them.
        let edge = |pick: fn(i32, i32) -> i32, of: fn(&Area) -> [i32; 2], axis: usize| {
            areas
                .iter()
                .map(|area| of(area)[axis])
                .reduce(pick)
                .unwrap_or(0)
        };
        let world = bounds_of(
            [
                edge(i32::min, |a| a.min_xz, 0),
                edge(i32::min, |a| a.min_xz, 1),
            ],
            [
                edge(i32::max, |a| a.max_xz, 0),
                edge(i32::max, |a| a.max_xz, 1),
            ],
            request.margin,
            ppm,
        );
        log(format!(
            "The whole zone -> {}",
            request.out.join("tiles").display()
        ));
        let image = photograph(&page, world, false).await?;
        std::fs::create_dir_all(request.out.join("world")).map_err(io)?;
        image
            .save(request.out.join("world/beauty.png"))
            .map_err(io)?;
        manifest["tiles"] = serde_json::json!({
            "max_zoom": tiles::write(&request.out, &image, world, &request.tiles_url)?,
            "world_metres": tiles::WORLD_METRES,
            "geographic": "lng = x / half_world * 180; lat = degrees(atan(sinh(-z / half_world * pi)))",
        });
    }
    let text = serde_json::to_string_pretty(&manifest).map_err(io)?;
    std::fs::write(request.out.join("manifest.json"), text + "\n").map_err(io)
}

#[cfg(test)]
mod tests {
    use super::{bounds_of, slug};
    use crate::grid::Bounds;

    #[test]
    fn names_become_file_names() {
        assert_eq!(slug("Marsh-Lake Wetlands"), "marsh-lake-wetlands");
        assert_eq!(slug("  Greyhaven  Outpost!"), "greyhaven-outpost");
    }

    #[test]
    fn inclusive_unit_bounds_snap_outward_to_whole_metres() {
        assert_eq!(
            bounds_of([-3_500, -1_300], [3_499, 5_299], 8., 8),
            Bounds {
                min_x: -43,
                min_z: -21,
                max_x: 43,
                max_z: 61,
                ppm: 8
            },
        );
        assert_eq!(
            bounds_of([-15_800, 9_800], [-9_401, 16_199], 0., 8),
            Bounds {
                min_x: -158,
                min_z: 98,
                max_x: -94,
                max_z: 162,
                ppm: 8
            },
        );
    }
}
