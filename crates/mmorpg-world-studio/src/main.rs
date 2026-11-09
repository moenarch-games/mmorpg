//! Greyhaven World Studio: pick a place on the game's map, paint it in Krita, bring the paint back
//! and hand the consequences to an agent (`docs/WORLD_STUDIO.md`). One program: the web app, and
//! the same capture and import as commands.
mod brief;
mod capture;
mod grid;
mod import;
mod layers;
mod ora;
mod server;
mod studio;
mod tiles;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};

#[derive(Parser)]
#[command(about = "Greyhaven's map, Krita and terrain workflow")]
struct Cli {
    /// The mmorpg checkout to work on.
    #[arg(long, global = true, default_value = ".")]
    repo: PathBuf,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run the web app (the default).
    Serve {
        #[arg(long, default_value_t = server::DEFAULT_PORT)]
        port: u16,
    },
    /// Photograph areas of the zone into layered Krita documents and map tiles.
    Capture(CaptureArgs),
    /// Bring a painted document's height and biome layers into content/greyhaven/paint/.
    Import {
        document: PathBuf,
        /// Report the changes without writing them.
        #[arg(long)]
        check: bool,
        /// Print the report as JSON.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Args)]
struct CaptureArgs {
    /// Output folder [default: artifacts/regions in the checkout].
    #[arg(long)]
    out: Option<PathBuf>,
    /// Area name or part of it; repeatable.
    #[arg(long)]
    only: Vec<String>,
    #[arg(long, default_value_t = 8)]
    pixels_per_metre: u32,
    /// Metres of surroundings around each area.
    #[arg(long, default_value_t = 8.)]
    margin: f64,
    /// Also write an XYZ raster pyramid of the whole zone for the map.
    #[arg(long)]
    tiles: bool,
    /// Write the pyramid and no region documents.
    #[arg(long)]
    tiles_only: bool,
    /// URL template written into tiles/style.json.
    #[arg(long, default_value = "/regions/tiles/{z}/{x}/{y}.png")]
    tiles_url: String,
    /// Capture this rectangle in metres instead of the named areas.
    #[arg(long, num_args = 4, allow_hyphen_values = true, value_names = ["MIN_X", "MIN_Z", "MAX_X", "MAX_Z"])]
    bounds: Option<Vec<i32>>,
    /// Document name for --bounds.
    #[arg(long, default_value = "selection")]
    name: String,
    /// Print the areas without capturing them.
    #[arg(long)]
    list: bool,
}

fn run_capture(args: CaptureArgs, repo: &Path) -> capture::Request {
    capture::Request {
        root: repo.to_path_buf(),
        out: args.out.unwrap_or_else(|| repo.join("artifacts/regions")),
        only: args.only,
        pixels_per_metre: args.pixels_per_metre,
        margin: args.margin,
        tiles: args.tiles,
        tiles_only: args.tiles_only,
        tiles_url: args.tiles_url,
        bounds: args
            .bounds
            .and_then(|bounds| <[i32; 4]>::try_from(bounds).ok()),
        name: args.name,
        list: args.list,
        target_dir: None,
    }
}

async fn run(cli: Cli) -> Result<(), String> {
    let repo = std::path::absolute(&cli.repo).map_err(|error| error.to_string())?;
    if !repo.join("crates/mmorpg-world-studio/Cargo.toml").is_file() {
        return Err(format!(
            "{} is not the mmorpg repository: pass --repo",
            repo.display()
        ));
    }
    match cli.command.unwrap_or(Command::Serve {
        port: server::DEFAULT_PORT,
    }) {
        Command::Serve { port } => server::serve(repo, port).await,
        Command::Capture(args) => {
            capture::run(run_capture(args, &repo), &|line| eprintln!("{line}")).await
        }
        Command::Import {
            document,
            check,
            json,
        } => {
            let report = import::run(&repo, &document, check, None).await?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string(&report).map_err(|error| error.to_string())?
                );
            } else {
                println!(
                    "height: {} samples change, {} painted in the zone",
                    report.height.changed, report.height.painted
                );
                println!(
                    "biome: {} samples change, {} painted in the zone",
                    report.biome.changed, report.biome.painted
                );
                if report.height.skipped_locked > 0 {
                    println!(
                        "height: skipped {} locked samples (buildings, water, the Outpost)",
                        report.height.skipped_locked
                    );
                }
                if !check {
                    println!(
                        "Rebuild the clients (cd web && bun run build) and capture again to see the result."
                    );
                }
            }
            Ok(())
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    match run(Cli::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
