//! Shared offline validation for the Blender authoring export and committed layout.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("expected a layout JSON path")?;
    let source = std::fs::read_to_string(path)?;
    let definition = mmorpg_core::greyhaven_vale::layout::compile(&source)?;
    mmorpg_scenery::layout::compose(&source, &definition)?;
    mmorpg_core::greyhaven_vale::content_with_definition(definition)?;
    println!("Layout accepted by authoritative content and presentation owners");
    Ok(())
}
