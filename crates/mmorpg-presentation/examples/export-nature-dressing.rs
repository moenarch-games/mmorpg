//! Offline layout → the nature bundle's committed world dressing (#683).
//!
//! `cargo run --quiet --locked -p mmorpg-presentation --example export-nature-dressing [layout]
//! > content/greyhaven/dressing/nature.json`

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (_, scenery) = mmorpg_scenery::layout::exporter_input(std::env::args().nth(1))?;
    let placed: Vec<_> = mmorpg_presentation::nature::compose_all(&scenery)
        .iter()
        .map(mmorpg_presentation::nature::Composed::placed)
        .collect();
    println!(
        "{}",
        mmorpg_scenery::layout::dressing_json("greyhaven-nature", &placed)?
    );
    Ok(())
}
