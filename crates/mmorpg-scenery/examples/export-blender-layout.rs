//! Development-only Blender scene context from the real Greyhaven content and scenery.
//! No authoring data or Blender dependency enters either runtime.
use mmorpg_core::greyhaven_vale;
use mmorpg_scenery::PropKind;
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = greyhaven_vale::layout::SOURCE;
    let definition = greyhaven_vale::layout::compile(source)?;
    greyhaven_vale::content_with_definition(definition.clone())?;
    let scenery = mmorpg_scenery::layout::compose(source, &definition)?;
    let decorations = mmorpg_scenery::layout::decorations(source)?;
    let region = greyhaven_vale::layout::REGION;

    let house = |collider: &mmorpg_core::StaticCollider| {
        json!({
            "id": collider.id,
            "position": [collider.position[0], collider.position[2]],
            "feet": collider.position[1] - collider.half_extents[1],
            "half": collider.half_extents,
        })
    };
    let editable = |id: u32| (103..=106).contains(&id) || (60_000..65_000).contains(&id);
    let base_houses: Vec<_> = greyhaven_vale::base_definition()
        .colliders()
        .iter()
        .filter(|collider| (103..=106).contains(&collider.id))
        .map(&house)
        .collect();
    let houses: Vec<_> = definition
        .colliders()
        .iter()
        .filter(|collider| editable(collider.id))
        .map(house)
        .collect();

    // These are visual editing guides, not independently persisted game geometry.
    let context: Vec<_> = scenery
        .props
        .iter()
        .enumerate()
        .filter(|(_, prop)| {
            let [x, _, z] = prop.position;
            x >= region.min[0] - 1_000
                && x <= region.max[0] + 1_000
                && z >= region.min[1] - 1_000
                && z <= region.max[1] + 1_000
                && !matches!(prop.kind, PropKind::GrassTuft | PropKind::Flowers)
                && !prop.collider.is_some_and(editable)
                && !decorations.iter().any(|entry| {
                    entry.kind == prop.kind.name()
                        && entry.position == [prop.position[0], prop.position[2]]
                })
        })
        .map(|(index, prop)| {
            json!({
                "name": format!("context-{}-{index}", prop.kind.name()),
                "kind": prop.kind.name(),
                "position": prop.position,
                "half": prop.half_extents,
                "yaw": prop.yaw,
            })
        })
        .collect();
    let step = 200;
    let columns = ((region.max[0] - region.min[0]) / step + 1) as usize;
    let rows = ((region.max[1] - region.min[1]) / step + 1) as usize;
    let mut heights = Vec::with_capacity(rows * columns);
    for row in 0..rows {
        for column in 0..columns {
            heights.push(scenery.surface_at(
                region.min[0] + column as i32 * step,
                region.min[1] + row as i32 * step,
            ));
        }
    }
    let roads: Vec<_> = scenery
        .roads
        .iter()
        .map(|road| json!({"name": road.name, "points": road.points, "half": road.half_width}))
        .collect();

    println!(
        "{}",
        serde_json::to_string(&json!({
            "region": [region.min, region.max],
            "houses": houses,
            "base_houses": base_houses,
            "decoration": decorations,
            "context": context,
            "terrain": {
                "origin": region.min,
                "step": step,
                "columns": columns,
                "rows": rows,
                "heights": heights,
            },
            "roads": roads,
        }))?
    );
    Ok(())
}
