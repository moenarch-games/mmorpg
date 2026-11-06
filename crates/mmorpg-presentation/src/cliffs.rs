//! Fractured rock skins and cave surfaces over authoritative collision, using the existing
//! terrain bundle's rock PBR material. Shallow recesses stay inside the solid proxy; walkable
//! tops, entrances and the shelf keep their authored shape. No renderer or collision rules.
use std::sync::Arc;

use mmorpg_scenery::{Prop, PropKind, Scenery};

use crate::{
    Draw, Group,
    batching::{Cull, material, matrix, mix, rgb, shade},
    geometry::{Geometry, ID, V, hash, norm},
    ground::{self, Ground, Layer},
    terrain::metres,
};

fn colored_face(g: &mut Geometry, points: &[V], color: u32) {
    let first = g.p.len();
    g.face(points);
    g.colors.extend(std::iter::repeat_n(
        rgb(color).map(f64::from),
        g.p.len() - first,
    ));
}

fn rock(prop: &Prop) -> Geometry {
    let [hx, hy, hz] = prop.half_extents.map(metres);
    let [x, y, z] = prop.position.map(metres);
    let h = hy * 2.;
    let seed = hash(&[x, z]);
    let mut g = Geometry::default();
    // Each side has real shallow fractures rather than three giant stacked slabs.
    for side in 0..4 {
        let along_x = side < 2;
        let half = if along_x { hx } else { hz };
        let cols = ((half * 2. / 2.5).ceil() as usize).clamp(1, 48);
        let rows = ((h / 1.2).ceil() as usize).clamp(1, 20);
        let at = |c: usize, r: usize| {
            let edge = c == 0 || c == cols || r == 0 || r == rows;
            let along = -half + half * 2. * c as f64 / cols as f64;
            let height = h * r as f64 / rows as f64;
            let recess = if edge {
                0.
            } else {
                hash(&[seed, c as f64, r as f64, side as f64])
                    * if height < 2.2 { 0.025 } else { 0.08 }
            };
            let along = along
                + if c == 0 || c == cols {
                    0.
                } else {
                    (hash(&[seed, c as f64, r as f64, 9.]) - 0.5) * 0.18
                };
            let height = height
                + if r == 0 || r == rows {
                    0.
                } else {
                    (hash(&[seed, r as f64, 7.]) - 0.5) * 0.16
                };
            match side {
                0 => [x + along, y + height, z + hz - recess],
                1 => [x - along, y + height, z - hz + recess],
                2 => [x + hx - recess, y + height, z - along],
                _ => [x - hx + recess, y + height, z + along],
            }
        };
        for r in 0..rows {
            for c in 0..cols {
                let band = hash(&[seed, r as f64, 4.]);
                let color = mix(
                    0x666760,
                    0x989183,
                    band * 0.75 + 0.15 * hash(&[seed, c as f64, r as f64]),
                );
                let a = at(c, r);
                let b = at(c + 1, r);
                let d = at(c, r + 1);
                let e = at(c + 1, r + 1);
                colored_face(&mut g, &[a, b, d], color);
                colored_face(&mut g, &[b, e, d], shade(color, 0.96));
            }
        }
    }
    colored_face(
        &mut g,
        &[
            [x - hx, y + h, z - hz],
            [x - hx, y + h, z + hz],
            [x + hx, y + h, z + hz],
            [x + hx, y + h, z - hz],
        ],
        0x797d63,
    );
    ground::project_rock(&mut g);
    g
}

fn group(id: String, geometry: Geometry) -> Group {
    let min = std::array::from_fn(|axis| {
        geometry
            .p
            .iter()
            .map(|v| v[axis * 2])
            .fold(f64::INFINITY, f64::min)
    });
    let max = std::array::from_fn(|axis| {
        geometry
            .p
            .iter()
            .map(|v| v[axis * 2])
            .fold(f64::NEG_INFINITY, f64::max)
    });
    let mut material = material(0xffffff, false);
    material.ground = Some(Ground::near(Layer::Rock, false));
    Group {
        draws: vec![Draw {
            id: id.clone(),
            resource_key: format!("scenery:{id}"),
            geometry: Arc::new(geometry),
            model: matrix(ID),
            material,
        }],
        cull: Cull::Far,
        instanced: false,
        fallback: None,
        min,
        max,
    }
}

/// Muted strata and moss on exposed landscape rock; cave floors and ceilings stay stone.
pub(crate) fn landscape_color(points: [V; 3], id: u32) -> u32 {
    let centre: V = std::array::from_fn(|axis| points.iter().map(|p| p[axis]).sum::<f64>() / 3.);
    let [a, b, c] = points;
    let u: V = std::array::from_fn(|i| b[i] - a[i]);
    let v: V = std::array::from_fn(|i| c[i] - a[i]);
    let up = norm([
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ])[1];
    let band = hash(&[f64::from(id), (centre[1] / 1.8).floor()]);
    let stone = mix(0x68675f, 0x969082, 0.25 + 0.5 * band);
    let growth = ((up - 0.45) / 0.55).clamp(0., 1.)
        * (0.25 + 0.55 * crate::terrain::noise(centre[0], centre[2], 5., 539.));
    mix(stone, 0x667447, growth)
}

/// Small recesses on the brook's western outcrops. Boundary vertices stay on the
/// authored surface; interior chips sit at most six centimetres inside it. Physics
/// continues to use the original simplified rock and its exact footprint.
fn weathered_bank_face(g: &mut Geometry, points: [V; 3], id: u32, depth: u8) {
    let edge = |a: V, b: V| (0..3).map(|axis| (a[axis] - b[axis]).powi(2)).sum::<f64>();
    let [a, b, c] = points;
    let u: V = std::array::from_fn(|axis| b[axis] - a[axis]);
    let v: V = std::array::from_fn(|axis| c[axis] - a[axis]);
    let normal = norm([
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ]);
    if normal[1].abs() > 0.65 {
        colored_face(g, &points, landscape_color(points, id));
        return;
    }

    if depth < 5 && edge(a, b).max(edge(b, c)).max(edge(c, a)) > 1.4 * 1.4 {
        let mid = |a: V, b: V| std::array::from_fn(|axis| (a[axis] + b[axis]) * 0.5);
        let (ab, bc, ca) = (mid(a, b), mid(b, c), mid(c, a));
        for face in [[a, ab, ca], [ab, b, bc], [ca, bc, c], [ab, bc, ca]] {
            weathered_bank_face(g, face, id, depth + 1);
        }
        return;
    }
    let centre: V = std::array::from_fn(|axis| (a[axis] + b[axis] + c[axis]) / 3.);
    let chip = 0.01 + 0.05 * hash(&[centre[0] * 17., centre[1] * 13., centre[2] * 19.]);
    let inset: V = std::array::from_fn(|axis| centre[axis] - normal[axis] * chip);
    let strata = crate::terrain::noise(centre[0] * 0.2 + centre[2] * 0.13, centre[1], 0.48, 51397.);
    let stone = landscape_color(points, id);
    for face in [[a, b, inset], [b, c, inset], [c, a, inset]] {
        colored_face(g, &face, shade(stone, 0.88 + strata * 0.20));
    }
}

/// Interior roof faces remain one-sided static rock while the exterior shell cuts away.
/// The existing backface culling hides these from an overhead camera automatically.
pub(crate) fn ceiling_face([a, b, c]: [V; 3]) -> bool {
    (b[2] - a[2]) * (c[0] - a[0]) - (b[0] - a[0]) * (c[2] - a[2]) < -1e-9
}

pub(crate) fn groups(scenery: &Scenery) -> Vec<Group> {
    let cliffs = scenery
        .props
        .iter()
        .filter(|p| {
            p.kind == PropKind::Cliff
                && !scenery
                    .collision_surfaces
                    .iter()
                    .any(|mesh| Some(mesh.id) == p.collider)
        })
        .enumerate()
        .map(|(i, p)| group(format!("rock-face-{i}"), rock(p)));
    let surfaces = scenery
        .collision_surfaces
        .iter()
        .filter(|mesh| {
            // The shrine kit already supplies its vaulted interior ceiling. Its authored
            // exterior is animated below; a second static underside would overlap the kit.
            !mmorpg_core::greyhaven_vale::fort::dresses(mesh.id)
                && !mmorpg_core::greyhaven_vale::ids::SHRINE_CAVE_ROOF.contains(&mesh.id)
        })
        .map(|mesh| {
            let mut g = Geometry::default();
            let roof = scenery
                .props
                .iter()
                .any(|p| p.kind == PropKind::CaveRoof && p.collider == Some(mesh.id));
            for triangle in &mesh.triangles {
                let points = triangle.map(|index| {
                    let v = mesh.vertices[index as usize];
                    std::array::from_fn(|axis| metres(v[axis] + mesh.position[axis]))
                });
                // Root caps sit within the solid roof; omitting them from the visible skin
                // also keeps an overhead cutaway free of floating ceiling decorations.
                let ceiling_detail =
                    mmorpg_core::greyhaven_vale::ids::CAVE_CEILING_DETAILS.contains(&mesh.id);
                if (roof || ceiling_detail) && !ceiling_face(points) {
                    continue;
                }
                let landscape = (400..=406).contains(&mesh.id)
                    || (65030..=65039).contains(&mesh.id)
                    || mmorpg_core::greyhaven_vale::fort::SHORE_ROCKS.contains(&mesh.id)
                    || mmorpg_core::greyhaven_vale::fort::BASIN_ROCKS.contains(&mesh.id);
                let color = if landscape {
                    landscape_color(points, mesh.id)
                } else {
                    0x817b70
                };
                if (65094..=65096).contains(&mesh.id) {
                    weathered_bank_face(&mut g, points, mesh.id, 0);
                } else {
                    colored_face(&mut g, &points, color);
                }
            }
            ground::project_rock(&mut g);
            group(format!("cave-surface-{}", mesh.id), g)
        });
    cliffs.chain(surfaces).collect()
}
