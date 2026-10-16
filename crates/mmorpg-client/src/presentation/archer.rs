//! Normalize the checked-in asset-tooling OBJ with the pinned format loader.
use super::{kit::*, player::Look};
use crate::{
    ClientError,
    world::{DrawMaterial, SceneDraw, geometry::*},
};
use std::{
    collections::BTreeMap,
    sync::{Arc, OnceLock},
};
const OBJ: &[u8] = include_bytes!("../../../../web/assets/medieval-character-kit/archer.obj");
const MANIFEST: &str = include_str!("../../../../web/assets/medieval-character-kit/manifest.json");
const MATERIALS: &str =
    include_str!("../../../../web/assets/medieval-character-kit/materials.json");
struct Package {
    parts: Vec<(String, Arc<three_d_core::Mesh>, String)>,
    colors: BTreeMap<String, u32>,
    identity: String,
}
fn load() -> Result<Package, ClientError> {
    let manifest: serde_json::Value = serde_json::from_str(MANIFEST)?;
    let materials: serde_json::Value = serde_json::from_str(MATERIALS)?;
    if manifest["coordinateSystem"] != "right-handed-y-up"
        || manifest["unit"] != "millimeter"
        || manifest["consumerScaleToMeters"] != "0.001"
    {
        return Err("incompatible archer package coordinate system".into());
    }
    let entry = manifest["assets"]
        .as_array()
        .and_then(|a| a.iter().find(|a| a["id"] == "medieval.archer"))
        .ok_or("archer manifest entry missing")?;
    let identity = format!(
        "asset-tooling:{}",
        entry["sha256"].as_str().ok_or("archer digest missing")?
    );
    let neutral = materials["palettes"]
        .as_array()
        .and_then(|p| p.iter().find(|p| p["id"] == "neutral"))
        .ok_or("neutral palette missing")?;
    let mut colors = BTreeMap::new();
    for material in neutral["materials"]
        .as_array()
        .ok_or("palette materials missing")?
    {
        let id = material["id"].as_str().ok_or("material id missing")?;
        let channels = material["baseColorSrgb8"]
            .as_array()
            .ok_or("material color missing")?;
        if channels.len() != 4 {
            return Err("material color must be RGBA".into());
        }
        let mut c = 0;
        for channel in &channels[..3] {
            let v = channel
                .as_u64()
                .filter(|v| *v <= 255)
                .ok_or("invalid color channel")?;
            c = (c << 8) | v as u32;
        }
        colors.insert(id.to_owned(), c);
    }
    let asset = three_d_formats::load_obj(OBJ)?;
    let mut parts = Vec::new();
    let mut triangles = 0;
    let mut source_vertices = 0_u64;

    for part in asset.meshes() {
        let name = part.name().ok_or("archer part has no name")?.to_owned();
        let role = materials["bindings"]["archer"][&name]
            .as_str()
            .ok_or("archer material binding missing")?
            .to_owned();
        let mut g = Geometry::default();
        for primitive in part.primitives() {
            let mesh = primitive.mesh();
            source_vertices += mesh.vertices().len() as u64;
            for tri in mesh.indices().chunks_exact(3) {
                triangles += 1;
                let p: Vec<_> = tri
                    .iter()
                    .map(|i| {
                        let p = mesh.vertices()[*i as usize];
                        [f64::from(p.x), f64::from(p.y), f64::from(p.z)]
                    })
                    .collect();
                let u = std::array::from_fn::<_, 3, _>(|i| p[1][i] - p[0][i]);
                let v = std::array::from_fn::<_, 3, _>(|i| p[2][i] - p[0][i]);
                let cross = [
                    u[1] * v[2] - u[2] * v[1],
                    u[2] * v[0] - u[0] * v[2],
                    u[0] * v[1] - u[1] * v[0],
                ];
                if cross == [0.; 3] {
                    return Err("degenerate archer triangle".into());
                }
                let n = norm(cross);
                for p in p {
                    g.indices.push(g.p.len() as u32);
                    g.p.push(p.map(|x| x * 0.001));
                    g.n.push(n);
                }
            }
        }
        parts.push((name, Arc::new(g.mesh()), role));
    }
    let names: Vec<_> = parts
        .iter()
        .map(|(n, ..)| serde_json::Value::String(n.clone()))
        .collect();
    if entry["parts"] != serde_json::Value::Array(names)
        || entry["triangleCount"].as_u64() != Some(triangles)
        || entry["vertexCount"].as_u64() != Some(source_vertices)
    {
        return Err("archer topology differs from manifest".into());
    }
    Ok(Package {
        parts,
        colors,
        identity,
    })
}
fn package() -> Result<&'static Package, ClientError> {
    static PACKAGE: OnceLock<Result<Package, String>> = OnceLock::new();
    PACKAGE
        .get_or_init(|| load().map_err(|e| e.to_string()))
        .as_ref()
        .map_err(|e| e.clone().into())
}
pub(super) fn validate() -> Result<(), ClientError> {
    package().map(|_| ())
}
pub(super) fn nodes(
    _resources: &mut Resources,
    id: &str,
    root: Frame,
    l: Look,
    bob: f64,
) -> Vec<SceneDraw> {
    let package = package().expect("embedded archer package validated at presentation creation");
    let f = Frame {
        p: [root.p[0], root.p[1] + bob, root.p[2]],
        ..root
    };
    package
        .parts
        .iter()
        .map(|(name, mesh, role)| {
            let c = match role.as_str() {
                "cloth-primary" => l.body,
                "cloth-secondary" => l.chest,
                _ => package.colors[role],
            };
            SceneDraw {
                id: format!("{id}-{name}"),
                resource_key: format!("{}:{name}", package.identity),
                mesh: Arc::clone(mesh),
                model: model(f, [1.; 3]),
                material: DrawMaterial {
                    color: rgb(c),
                    emissive: [0.; 3],
                    opacity: 1.,
                    unlit: false,
                    double_sided: false,
                },
            }
        })
        .collect()
}
