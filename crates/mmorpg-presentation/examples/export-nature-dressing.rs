//! Offline layout → the nature bundle's committed world dressing (#683).
//!
//! `cargo run --quiet --locked -p mmorpg-presentation --example export-nature-dressing [layout]
//! > content/greyhaven/dressing/nature.json`
use mmorpg_core::greyhaven_vale;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = match std::env::args().nth(1) {
        Some(path) => std::fs::read_to_string(path)?,
        None => greyhaven_vale::layout::SOURCE.to_owned(),
    };
    let definition = greyhaven_vale::layout::compile(&source)?;
    let scenery = mmorpg_scenery::layout::compose(&source, &definition)?;
    let placed: Vec<_> = mmorpg_presentation::nature::compose_all(&scenery)
        .iter()
        .map(mmorpg_presentation::nature::Composed::placed)
        .collect();
    println!("{{\n  \"version\": 1,\n  \"area\": \"greyhaven-nature\",\n  \"placements\": [");
    for (index, p) in placed.iter().enumerate() {
        println!(
            "    {}{}",
            serde_json::to_string(p)?,
            if index + 1 == placed.len() { "" } else { "," }
        );
    }
    println!("  ]\n}}");
    Ok(())
}
