//! Development-only Blender context for authoring world dressing at one Greyhaven site (#683):
//! the physical ground and the drawn surface on a 1 m grid, and the footprints of the
//! colliders and static meshes standing there, as JSON. No authoring data enters either runtime.
//!
//! `cargo run -p mmorpg-scenery --example export-site-context -- --centre x,z --half hx,hz`
//!
//! Coordinates are in units (100 per metre); the JSON goes to stdout.
use mmorpg_core::greyhaven_vale::{self, ids};
use serde_json::json;

const STEP: i32 = 100;

fn pair(value: &str) -> Result<[i32; 2], String> {
    let (a, b) = value
        .split_once(',')
        .ok_or_else(|| format!("{value}: expected x,z"))?;
    Ok([
        a.trim().parse().map_err(|_| format!("{a}: not a number"))?,
        b.trim().parse().map_err(|_| format!("{b}: not a number"))?,
    ])
}

fn main() -> Result<(), String> {
    let mut centre = None;
    let mut half = [3_000, 3_000];
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let value = args.next().ok_or_else(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--centre" => centre = Some(pair(&value)?),
            "--half" => half = pair(&value)?,
            _ => return Err(format!("unknown option {flag}")),
        }
    }
    let centre = centre.ok_or("--centre x,z is required")?;
    let [min_x, min_z] = [centre[0] - half[0], centre[1] - half[1]];
    let [columns, rows] = [half[0] * 2 / STEP + 1, half[1] * 2 / STEP + 1];
    let scenery = mmorpg_scenery::greyhaven_vale_scenery();
    let sample = |f: &dyn Fn(i32, i32) -> i32| -> Vec<i32> {
        (0..rows)
            .flat_map(|row| (0..columns).map(move |column| (column, row)))
            .map(|(column, row)| f(min_x + column * STEP, min_z + row * STEP))
            .collect()
    };
    let ground = sample(&|x, z| scenery.ground_at(x, z));
    let surface = sample(&|x, z| scenery.surface_at(x, z));

    let definition = greyhaven_vale::greyhaven_vale_definition();
    let inside = |position: [i32; 3], extents: [i32; 3]| {
        (position[0] - centre[0]).abs() < half[0] + extents[0]
            && (position[2] - centre[1]).abs() < half[1] + extents[2]
    };
    let mut bodies: Vec<_> = definition
        .colliders()
        .iter()
        .filter(|collider| collider.id != ids::GROUND)
        .map(|collider| (collider.id, collider.position, collider.half_extents))
        .chain(definition.static_meshes().iter().filter_map(|mesh| {
            let bounds = mesh.bounds().ok()?;
            Some((mesh.id, bounds.position, bounds.half_extents))
        }))
        .filter(|&(_, position, extents)| {
            inside(position, extents) && extents[0] < half[0] && extents[2] < half[1]
        })
        .map(|(id, position, extents)| json!({"id": id, "centre": position, "half": extents}))
        .collect();
    bodies.sort_by_key(|body| body["id"].as_u64());
    println!(
        "{}",
        json!({
            "origin": [min_x, min_z],
            "step": STEP,
            "columns": columns,
            "rows": rows,
            "ground": ground,
            "surface": surface,
            "bodies": bodies,
        })
    );
    Ok(())
}
