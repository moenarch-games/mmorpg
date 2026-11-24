//! Offline: Greyhaven's remaining procedural barrels, crates, carts, signposts, lamps, waystones
//! and fences → committed world dressing from asset-tooling's `settlement.props` (#683).
//!
//! Each prop's collider keeps colliding unchanged; the kit prop stands over it as its
//! `fallbackCollider`, so the procedural model is only its stand-in while the props are not
//! drawn. Props another dressing file already stands over are left to it. A fence is tiled with
//! the two-metre fence module along its length, each module on the ground under it.
//!
//! `cargo run --quiet --locked -p mmorpg-presentation --example export-prop-dressing
//! > content/greyhaven/dressing/props.json`
use mmorpg_core::{greyhaven_vale, trig::YAW_QUARTER_TURN};
use mmorpg_presentation::world_dressing::{self, Placed};
use mmorpg_scenery::PropKind;

/// This file's placement IDs, skipped when reading which colliders are already dressed.
const PREFIX: &str = "prop-";
/// The fence module's length along its X, units.
const FENCE: i32 = 200;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let scenery = mmorpg_scenery::greyhaven_vale_scenery();
    let dressed: std::collections::BTreeSet<u32> = world_dressing::committed()
        .iter()
        .filter(|placed| !placed.id.starts_with(PREFIX))
        .flat_map(|placed| [placed.collider, placed.fallback_collider])
        .flatten()
        .collect();
    let mut placements = Vec::new();
    for p in &scenery.props {
        let Some(id) = p.collider else { continue };
        if dressed.contains(&id) || greyhaven_vale::fort::dresses(id) {
            continue;
        }
        let [hx, hy, hz] = p.half_extents;
        // The kit entry, its extra quarter turns and its scale (permille) to the collider box,
        // as the settlements' stand-ins (#851): a cart's and a lantern's length lie along their
        // own Z, the box's along X.
        let (name, quarters, scale) = match p.kind {
            PropKind::Barrel => ("barrel", 0, [1000, 1000, 1000]),
            PropKind::Crate => ("crate", 0, [1390, 1430, 1390]),
            PropKind::Cart => ("cart", 1, [900, 1000, 900]),
            PropKind::Signpost => ("signpost", 0, [1000, 1000, 1000]),
            PropKind::Lamp => ("lantern-post", 1, [1000, 1000, 1000]),
            PropKind::Waystone => {
                let height = u16::try_from(hy * 2 * 1000 / 240)?;
                ("standing-stone", 0, [490, height, 490])
            }
            PropKind::Fence => ("fence", 0, [1000, 1000, 1000]),
            _ => continue,
        };
        let at = |x: i32, z: i32| [x, scenery.surface_at(x, z), z];
        let place = |id_suffix: String, position: [i32; 3], yaw: u16, scale: [u16; 3]| Placed {
            id: format!("{PREFIX}{id}-{name}{id_suffix}"),
            asset: format!("settlement.props.{name}"),
            position,
            yaw,
            scale,
            collider: None,
            grip: None,
            fallback_collider: Some(id),
            wear: 0,
            ground_detail: false,
            cull: None,
        };
        let [x, _, z] = p.position;
        if p.kind != PropKind::Fence {
            let yaw = p.yaw.wrapping_add(quarters * YAW_QUARTER_TURN);
            placements.push(place(String::new(), at(x, z), yaw, scale));
            continue;
        }
        // A fence runs along its box's longer side, in modules stretched to fill it.
        let along_x = hx >= hz;
        let length = 2 * hx.max(hz);
        let count = (length + FENCE / 2) / FENCE;
        let count = count.max(1);
        let stretch = u16::try_from(length * 1000 / (count * FENCE))?;
        let yaw = if along_x {
            p.yaw
        } else {
            p.yaw.wrapping_add(YAW_QUARTER_TURN)
        };
        let turn = f64::from(p.yaw) / 65_536. * std::f64::consts::TAU;
        for module in 0..count {
            let offset = f64::from(-length / 2 + length * (2 * module + 1) / (2 * count));
            let (dx, dz) = if along_x { (offset, 0.) } else { (0., offset) };
            // The prop's yaw turns +Z toward +X.
            let mx = x + (dx * turn.cos() + dz * turn.sin()).round() as i32;
            let mz = z + (dz * turn.cos() - dx * turn.sin()).round() as i32;
            placements.push(place(
                format!("-{module}"),
                at(mx, mz),
                yaw,
                [stretch, 1000, 1000],
            ));
        }
    }
    println!("{{\n  \"version\": 1,\n  \"area\": \"greyhaven-props\",\n  \"placements\": [");
    for (index, p) in placements.iter().enumerate() {
        println!(
            "    {}{}",
            serde_json::to_string(p)?,
            if index + 1 == placements.len() {
                ""
            } else {
                ","
            }
        );
    }
    println!("  ]\n}}");
    Ok(())
}
