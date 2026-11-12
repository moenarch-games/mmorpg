//! Greyhaven's highland crags dressed with asset-tooling's `crag-kit` (#668,
//! asset-tooling#480).
//!
//! Every highland crag box (`highlands::CRAGS`, a [`PropKind::Cliff`]) wears the kit's layout
//! rule (asset-tooling `docs/crag-kit.md#layout`, reference `cragLayout()`): each vertical face
//! is split into cells about 3 m square, each cell takes a face module scaled to fit, corner
//! columns stand on the vertical edges and caps run along the top. The family follows the
//! content's grip: a crag the climbing content marks rock ([`Grip::Rock`]) takes the
//! climbable modules (`grip: "rock"`, with strata and ledges), the sheer crags
//! (`SHEER_CRAGS`) the sheer ones (no grip). The Redbrand Hollow's cliffs are no longer boxes
//! but authored collision meshes with their own rock skin, so the box kit does not dress them.
//!
//! The box stays the collider: the modules are presentation over it. A climber on a dressed
//! climbable face is drawn out of the box by the module's face plane and the `Climb_*` clips'
//! `standOff` ([`Faces::stand_off`]), so the hands meet the rock rather than the collider
//! inside it. While a client does not draw the kit, the static scene draws the plain rock
//! boxes and their worn handholds ([`crate::Kit::Crag`] fallback groups). Presentation only.
//!
//! The kit's freestanding outcrops ([`OUTCROPS`]) are placed by the world dressing
//! ([`crate::world_dressing`], #683) through the same placements, levels and fallback.
use std::collections::BTreeMap;

use mmorpg_core::{Grip, UNITS_PER_METRE, greyhaven_vale::ids};
use mmorpg_scenery::{Prop, PropKind, Scenery};
use serde_json::Value;

use crate::{
    Cull,
    batching::matrix,
    geometry::{Affine, compose, ry, scale, translate},
};

pub use crate::lod::{EntryLods, Lods, Placement, Shown};

/// Every face, corner and cap module covers a cell this wide (m, `pitch`).
pub const PITCH: f64 = 3.;

pub const FACE_SHEER: [&str; 3] = [
    "crag.kit.face-sheer-1",
    "crag.kit.face-sheer-2",
    "crag.kit.face-sheer-3",
];
pub const FACE_CLIMB: [&str; 3] = [
    "crag.kit.face-climb-1",
    "crag.kit.face-climb-2",
    "crag.kit.face-climb-3",
];
pub const CORNER_SHEER: [&str; 2] = ["crag.kit.corner-sheer-1", "crag.kit.corner-sheer-2"];
pub const CORNER_CLIMB: [&str; 2] = ["crag.kit.corner-climb-1", "crag.kit.corner-climb-2"];
pub const CAPS: [&str; 2] = ["crag.kit.cap-1", "crag.kit.cap-2"];
pub const CAP_CORNER: &str = "crag.kit.cap-corner";
/// How far out of a dressed box face the climbing kit's ivy hangs (m), clear of the most
/// proud climbable rock (the descriptor's `relief.max`).
pub const IVY_STAND_OFF: f64 = 0.8;
/// The kit entries the layout places, sorted.
pub const ENTRIES: [&str; 13] = [
    "crag.kit.cap-1",
    "crag.kit.cap-2",
    CAP_CORNER,
    "crag.kit.corner-climb-1",
    "crag.kit.corner-climb-2",
    "crag.kit.corner-sheer-1",
    "crag.kit.corner-sheer-2",
    "crag.kit.face-climb-1",
    "crag.kit.face-climb-2",
    "crag.kit.face-climb-3",
    "crag.kit.face-sheer-1",
    "crag.kit.face-sheer-2",
    "crag.kit.face-sheer-3",
];
/// The freestanding outcrops the world dressing places (#683, [`crate::world_dressing`]),
/// sorted.
pub const OUTCROPS: [&str; 5] = [
    "crag.kit.outcrop-buttress-1",
    "crag.kit.outcrop-buttress-2",
    "crag.kit.outcrop-spire",
    "crag.kit.outcrop-talus-1",
    "crag.kit.outcrop-talus-2",
];
/// Every entry the clients load: the box modules, then the outcrops.
pub const KIT_ENTRIES: [&str; 18] = [
    ENTRIES[0],
    ENTRIES[1],
    ENTRIES[2],
    ENTRIES[3],
    ENTRIES[4],
    ENTRIES[5],
    ENTRIES[6],
    ENTRIES[7],
    ENTRIES[8],
    ENTRIES[9],
    ENTRIES[10],
    ENTRIES[11],
    ENTRIES[12],
    OUTCROPS[0],
    OUTCROPS[1],
    OUTCROPS[2],
    OUTCROPS[3],
    OUTCROPS[4],
];

/// A crag's module family.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Family {
    /// Smooth fracture planes, no grip.
    Sheer,
    /// Strata, ledges and cracks: `grip: "rock"`.
    Climb,
}

impl Family {
    const fn faces(self) -> &'static [&'static str; 3] {
        match self {
            Self::Sheer => &FACE_SHEER,
            Self::Climb => &FACE_CLIMB,
        }
    }

    const fn corners(self) -> &'static [&'static str; 2] {
        match self {
            Self::Sheer => &CORNER_SHEER,
            Self::Climb => &CORNER_CLIMB,
        }
    }

    /// The descriptor's `climb.grip` of the family's faces and corners.
    #[must_use]
    pub const fn grip(self) -> Option<Grip> {
        match self {
            Self::Sheer => None,
            Self::Climb => Some(Grip::Rock),
        }
    }
}

/// One dressed box: its collider, family and size (metres, base centre and full extents).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CragBox {
    pub collider: u32,
    pub family: Family,
    pub base: [f64; 3],
    pub size: [f64; 3],
}

fn metres(units: i32) -> f64 {
    f64::from(units) / f64::from(UNITS_PER_METRE)
}

/// Whether the kit dresses `p`: a highland crag drawn as a plain box (an authored cliff mesh
/// keeps its own skin).
#[must_use]
pub fn dresses(s: &Scenery, p: &Prop) -> bool {
    p.kind == PropKind::Cliff
        && p.yaw == 0
        && p.collider.is_some_and(|id| {
            ids::HIGHLAND_CRAGS.contains(&id)
                && !s.collision_surfaces.iter().any(|mesh| mesh.id == id)
        })
}

/// Every box the kit dresses, in prop order.
#[must_use]
pub fn boxes(s: &Scenery) -> Vec<CragBox> {
    s.props
        .iter()
        .filter(|p| dresses(s, p))
        .filter_map(|p| {
            let collider = p.collider?;
            let family = if s.climbing().collider(collider) == Some(Grip::Rock) {
                Family::Climb
            } else {
                Family::Sheer
            };
            let [hx, hy, hz] = p.half_extents.map(metres);
            Some(CragBox {
                collider,
                family,
                base: p.position.map(metres),
                size: [hx * 2., hy * 2., hz * 2.],
            })
        })
        .collect()
}

/// A stable hash of the crag and a cell, for picking variants (splitmix64).
fn pick(keys: &[u64]) -> u64 {
    keys.iter().fold(0x9e37_79b9_7f4a_7c15, |h, &k| {
        let mut z = h ^ k.wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    })
}

/// One face of a box: its centre on XZ (from the box's base centre), the turn about +Y that
/// faces a module's +Z out of it and its length.
struct Face {
    centre: [f64; 2],
    rotation: f64,
    length: f64,
}

fn faces(size: [f64; 3]) -> [Face; 4] {
    use std::f64::consts::{FRAC_PI_2, PI};
    let [w, _, d] = size;
    [
        Face {
            centre: [0., d / 2.],
            rotation: 0.,
            length: w,
        },
        Face {
            centre: [w / 2., 0.],
            rotation: FRAC_PI_2,
            length: d,
        },
        Face {
            centre: [0., -d / 2.],
            rotation: PI,
            length: w,
        },
        Face {
            centre: [-w / 2., 0.],
            rotation: -FRAC_PI_2,
            length: d,
        },
    ]
}

fn cells(length: f64) -> (usize, f64) {
    let n = (length / PITCH).round().max(1.);
    (n as usize, length / (PITCH * n))
}

/// One module the layout places, in the box's frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Module {
    pub entry: &'static str,
    /// Face index (0 +Z, 1 +X, 2 −Z, 3 −X), and the cell's column and row on it (caps and
    /// cap-corners take the row past the top, corners the column past the end).
    pub face: usize,
    pub column: usize,
    pub row: usize,
    /// Metres along the face from its centre (looking at it from outside, +along to the
    /// right) and up from the box's base.
    pub along: f64,
    pub up: f64,
    pub scale: [f64; 3],
}

/// The layout rule (asset-tooling `docs/crag-kit.md#layout`) for one box: a face module of
/// the box's family on every cell of every vertical face, scaled to fit, never the variant of
/// its left or lower neighbour; a corner column on each vertical edge; a cap on every top cell
/// and a cap-corner on every top corner. The modules are not mirrored: a negative scale would
/// turn their faces inside out for the clients' instanced, back-face-culled draws.
#[must_use]
pub fn layout(b: &CragBox) -> Vec<Module> {
    let mut out = Vec::new();
    let height = b.size[1];
    let (rows, sy) = cells(height);
    let seed = u64::from(b.collider);
    let variants = b.family.faces();
    for (f, face) in faces(b.size).iter().enumerate() {
        let (columns, sx) = cells(face.length);
        let mut below = vec![""; columns];
        for j in 0..rows {
            let mut left = "";
            for (i, under) in below.iter_mut().enumerate() {
                let options: Vec<&str> = variants
                    .iter()
                    .copied()
                    .filter(|v| *v != left && v != under)
                    .collect();
                let index = pick(&[seed, 1, f as u64, i as u64, j as u64]) as usize;
                let entry = options[index % options.len()];
                left = entry;
                *under = entry;
                out.push(Module {
                    entry,
                    face: f,
                    column: i,
                    row: j,
                    along: -face.length / 2. + PITCH * sx * (i as f64 + 0.5),
                    up: PITCH * sy * j as f64,
                    scale: [sx, sy, 1.],
                });
            }
            // The corner on this face's right-hand end, looking at the face from outside.
            let corners = b.family.corners();
            out.push(Module {
                entry: corners[pick(&[seed, 2, f as u64, j as u64]) as usize % corners.len()],
                face: f,
                column: columns,
                row: j,
                along: face.length / 2.,
                up: PITCH * sy * j as f64,
                scale: [1., sy, 1.],
            });
        }
        for i in 0..columns {
            out.push(Module {
                entry: CAPS[pick(&[seed, 3, f as u64, i as u64]) as usize % CAPS.len()],
                face: f,
                column: i,
                row: rows,
                along: -face.length / 2. + PITCH * sx * (i as f64 + 0.5),
                up: height,
                scale: [sx, 1., 1.],
            });
        }
        out.push(Module {
            entry: CAP_CORNER,
            face: f,
            column: columns,
            row: rows,
            along: face.length / 2.,
            up: height,
            scale: [1., 1., 1.],
        });
    }
    out
}

/// A module's frame in the world.
fn frame(b: &CragBox, m: &Module) -> Affine {
    let face = &faces(b.size)[m.face];
    let (c, s) = (face.rotation.cos(), face.rotation.sin());
    compose(&[
        translate(
            b.base[0] + face.centre[0] + m.along * c,
            b.base[1] + m.up,
            b.base[2] + face.centre[1] - m.along * s,
        ),
        ry(face.rotation),
        scale(m.scale[0], m.scale[1], m.scale[2]),
    ])
}

/// Every crag kit module placed on `s`, box by box in prop order, then the outcrops the world
/// dressing places (#683). Only Greyhaven Vale has them.
#[must_use]
pub fn placements(s: &Scenery) -> Vec<Placement> {
    let mut placements: Vec<_> = boxes(s)
        .iter()
        .flat_map(|b| {
            layout(b).into_iter().map(move |m| Placement {
                entry: m.entry,
                model: matrix(frame(b, &m)),
                scale: m.scale.into_iter().fold(1., f64::max),
                cull: Cull::Far,
            })
        })
        .collect();
    placements.extend(crate::world_dressing::placements(s, crate::Kit::Crag));
    placements
}

/// A module's `climb` metadata from the descriptor, in the climbing kit's shape.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Climb {
    /// `grip`: `"rock"` or none.
    pub grip: Option<Grip>,
    /// The face plane's offset out of the box face (m, the mean relief over the cell); a
    /// corner's is its arms' and a cap's 0.
    pub face: f64,
    /// The `Climb_*` clips' distance from the grip plane to the pelvis (m).
    pub stand_off: f64,
    /// How far the rock stands proud of the box face (m): least, mean and most over the cell.
    pub relief: [f64; 3],
}

/// Every module's `climb` metadata from the bundle's `crag-descriptor` attachment JSON.
///
/// # Errors
///
/// For malformed JSON or climb metadata, or a module whose grip is not its family's.
pub fn descriptor_climbs(json: &str) -> Result<BTreeMap<String, Climb>, String> {
    let value: Value =
        serde_json::from_str(json).map_err(|error| format!("crag descriptor: {error}"))?;
    let assets = value["assets"]
        .as_array()
        .ok_or("the crag descriptor lists no assets")?;
    let mut out = BTreeMap::new();
    for asset in assets {
        let Some(key) = asset["key"].as_str() else {
            return Err("a crag module without key".into());
        };
        if !ENTRIES.contains(&key) {
            continue;
        }
        let climb = &asset["climb"];
        let grip = match climb["grip"].as_str() {
            None if climb["grip"].is_null() => None,
            Some("rock") => Some(Grip::Rock),
            other => return Err(format!("{key} has grip {other:?}")),
        };
        let family = if FACE_CLIMB.contains(&key) || CORNER_CLIMB.contains(&key) {
            Family::Climb
        } else {
            Family::Sheer
        };
        if grip != family.grip() {
            return Err(format!("{key} has grip {grip:?}, not its family's"));
        }
        let face = climb["face"]["z"]
            .as_f64()
            .or_else(|| climb["faces"][0]["z"].as_f64())
            .unwrap_or(0.);
        let stand_off = climb["standOff"]
            .as_f64()
            .ok_or_else(|| format!("{key} has no standOff"))?;
        let relief = &climb["relief"];
        let mean = relief["mean"].as_f64().unwrap_or(face);
        out.insert(
            key.to_owned(),
            Climb {
                grip,
                face,
                stand_off,
                relief: [
                    relief["min"].as_f64().unwrap_or(mean),
                    mean,
                    relief["max"].as_f64().unwrap_or(mean),
                ],
            },
        );
    }
    if let Some(missing) = ENTRIES.iter().find(|entry| !out.contains_key(**entry)) {
        return Err(format!("the crag descriptor lacks {missing}"));
    }
    Ok(out)
}

/// Every placed module's and outcrop's levels from the bundle's `crag-descriptor` attachment
/// JSON ([`KIT_ENTRIES`]); the macro texture is not placed.
///
/// # Errors
///
/// For malformed JSON or levels of detail.
pub fn descriptor_lods(json: &str) -> Result<Vec<EntryLods>, String> {
    let mut value: Value =
        serde_json::from_str(json).map_err(|error| format!("crag descriptor: {error}"))?;
    // The outcrops are listed apart from the modules, in the same shape.
    let outcrops = value["outcrops"]["entries"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if let Some(assets) = value["assets"].as_array_mut() {
        assets.extend(outcrops);
        assets.retain(|asset| {
            asset["key"]
                .as_str()
                .is_some_and(|key| KIT_ENTRIES.contains(&key))
        });
    }
    crate::lod::descriptor_lods(&value)
}

/// One climbable face cell of a dressed box, where a climber's hands meet the module's rock.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cell {
    /// The box face's outward normal on XZ, and the face's plane: `normal · p = plane`.
    pub normal: [f64; 2],
    pub plane: f64,
    /// The cell's centre on XZ on the box face, its half width along it, and its bottom and
    /// top (m).
    pub centre: [f64; 2],
    pub half_width: f64,
    pub bottom: f64,
    pub top: f64,
    /// How far out of the box face the drawn grip plane stands, less how far the clips' pelvis
    /// already stands off the collider: the drawn climber's push (m).
    pub push: f64,
}

/// The dressed climbable faces, cell by cell, which a climber's drawn body follows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Faces {
    cells: Vec<Cell>,
}

impl Faces {
    /// The climbable cells of every dressed box on `s`, with the descriptor's climb metadata;
    /// `radius` is a climber's body radius (m), at which the collider holds its centre off a
    /// face.
    #[must_use]
    pub fn new(s: &Scenery, climbs: &BTreeMap<String, Climb>, radius: f64) -> Self {
        let mut cells = Vec::new();
        for b in boxes(s).iter().filter(|b| b.family == Family::Climb) {
            let faces = faces(b.size);
            for m in layout(b) {
                let Some(climb) = climbs.get(m.entry).filter(|c| c.grip == Some(Grip::Rock)) else {
                    continue;
                };
                if !FACE_CLIMB.contains(&m.entry) {
                    continue;
                }
                let face = &faces[m.face];
                let (c, s) = (face.rotation.cos(), face.rotation.sin());
                let normal = [s, c];
                let centre = [
                    b.base[0] + face.centre[0] + m.along * c,
                    b.base[2] + face.centre[1] - m.along * s,
                ];
                cells.push(Cell {
                    normal,
                    plane: normal[0] * centre[0] + normal[1] * centre[1],
                    centre,
                    half_width: PITCH * m.scale[0] / 2.,
                    bottom: b.base[1] + m.up,
                    top: b.base[1] + m.up + PITCH * m.scale[1],
                    push: (climb.face + climb.stand_off - radius).max(0.),
                });
            }
        }
        Self { cells }
    }

    /// Whether no face is dressed (the kit is not drawn, or no box climbs).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// How far (XZ, metres) to draw a climber whose feet are at `feet` out of the dressed
    /// face it holds, so its hands meet the module's rock; `None` off every dressed face.
    #[must_use]
    pub fn stand_off(&self, feet: [f64; 3], radius: f64) -> Option<[f64; 2]> {
        self.cells
            .iter()
            .filter(|cell| {
                let out = cell.normal[0] * feet[0] + cell.normal[1] * feet[2] - cell.plane;
                let side = [-cell.normal[1], cell.normal[0]];
                let along =
                    side[0] * (feet[0] - cell.centre[0]) + side[1] * (feet[2] - cell.centre[1]);
                (-0.05..=radius + 0.5).contains(&out)
                    && along.abs() <= cell.half_width
                    && (cell.bottom - 2.5..cell.top).contains(&feet[1])
            })
            .map(|cell| cell.normal.map(|n| n * cell.push))
            .next()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmorpg_core::greyhaven_vale::{SHEER_CRAGS, highlands};

    #[test]
    fn the_kit_loads_every_outcrop_the_dressing_places() {
        let s = mmorpg_scenery::greyhaven_vale_scenery();
        let assets = crate::world_dressing::assets(&s, crate::Kit::Crag);
        assert!(!assets.is_empty());
        assert!(
            assets.iter().all(|asset| OUTCROPS.contains(asset)),
            "{assets:?}"
        );
        let outcrops = placements(&s)
            .into_iter()
            .filter(|p| OUTCROPS.contains(&p.entry))
            .count();
        assert_eq!(
            outcrops,
            crate::world_dressing::placements(&s, crate::Kit::Crag).len()
        );
    }

    #[test]
    fn every_crag_is_dressed_sheer_where_the_content_is_sheer() {
        let scenery = mmorpg_scenery::greyhaven_vale_scenery();
        let crags = boxes(&scenery);
        assert_eq!(crags.len(), highlands::CRAGS.len());
        for (index, crag) in crags.iter().enumerate() {
            assert_eq!(crag.collider, ids::HIGHLAND_CRAGS.start() + index as u32);
            let sheer = SHEER_CRAGS.contains(&index);
            assert_eq!(crag.family == Family::Sheer, sheer, "crag {index}");
        }
    }

    #[test]
    fn the_layout_covers_every_face_with_fitted_cells_and_no_repeated_neighbour() {
        let scenery = mmorpg_scenery::greyhaven_vale_scenery();
        for b in boxes(&scenery) {
            let modules = layout(&b);
            let (rows, sy) = cells(b.size[1]);
            assert!((0.7..=1.3).contains(&sy), "{b:?}");
            for (f, face) in faces(b.size).iter().enumerate() {
                let (columns, sx) = cells(face.length);
                assert!((0.6..=1.4).contains(&sx), "{b:?}");
                let on: Vec<_> = modules
                    .iter()
                    .filter(|m| m.face == f && b.family.faces().contains(&m.entry))
                    .collect();
                assert_eq!(on.len(), rows * columns);
                for m in &on {
                    let neighbour = |c: usize, r: usize| {
                        on.iter()
                            .find(|n| n.column == c && n.row == r)
                            .map(|n| n.entry)
                    };
                    if m.column > 0 {
                        assert_ne!(neighbour(m.column - 1, m.row), Some(m.entry));
                    }
                    if m.row > 0 {
                        assert_ne!(neighbour(m.column, m.row - 1), Some(m.entry));
                    }
                }
                // The cells span the face exactly.
                let width: f64 = on
                    .iter()
                    .filter(|m| m.row == 0)
                    .map(|m| PITCH * m.scale[0])
                    .sum();
                assert!((width - face.length).abs() < 1e-9);
                let corners = modules
                    .iter()
                    .filter(|m| m.face == f && b.family.corners().contains(&m.entry))
                    .count();
                assert_eq!(corners, rows);
                assert_eq!(
                    modules
                        .iter()
                        .filter(|m| m.face == f && CAPS.contains(&m.entry))
                        .count(),
                    columns
                );
            }
            assert_eq!(modules.iter().filter(|m| m.entry == CAP_CORNER).count(), 4);
        }
    }

    #[test]
    fn a_module_stands_on_its_box_face_facing_out() {
        let scenery = mmorpg_scenery::greyhaven_vale_scenery();
        let b = boxes(&scenery)[0];
        let centre = [b.base[0], b.base[2]];
        for m in layout(&b) {
            let f = frame(&b, &m);
            let out = crate::geometry::point(f, [0., 0., 1.]);
            let normal = [out[0] - f.t[0], out[2] - f.t[2]];
            let offset = [f.t[0] - centre[0], f.t[2] - centre[1]];
            // The origin is on the box face (or its edge), and +Z points out of the box.
            let reach = normal[0] * offset[0] + normal[1] * offset[1];
            let half = if normal[0].abs() > 0.5 {
                b.size[0]
            } else {
                b.size[2]
            } / 2.;
            assert!((reach - half).abs() < 1e-9, "{m:?}");
            assert!(f.t[1] >= b.base[1] - 1e-9 && f.t[1] <= b.base[1] + b.size[1] + 1e-9);
        }
    }

    #[test]
    fn a_climber_on_a_dressed_face_is_pushed_out_to_the_rock() {
        let scenery = mmorpg_scenery::greyhaven_vale_scenery();
        let climbs: BTreeMap<String, Climb> = ENTRIES
            .iter()
            .map(|entry| {
                let climb = FACE_CLIMB.contains(entry) || CORNER_CLIMB.contains(entry);
                (
                    (*entry).to_owned(),
                    Climb {
                        grip: climb.then_some(Grip::Rock),
                        face: 0.5,
                        stand_off: 0.36,
                        relief: [0.2, 0.5, 0.7],
                    },
                )
            })
            .collect();
        let faces = Faces::new(&scenery, &climbs, 0.3);
        assert!(!faces.is_empty());
        let b = boxes(&scenery)
            .into_iter()
            .find(|b| b.family == Family::Climb)
            .unwrap();
        // On the +Z face, half way up, the climber's centre a body radius off the box.
        let feet = [
            b.base[0],
            b.base[1] + b.size[1] / 2.,
            b.base[2] + b.size[2] / 2. + 0.3,
        ];
        let push = faces.stand_off(feet, 0.3).unwrap();
        assert!(push[0].abs() < 1e-9 && (push[1] - 0.56).abs() < 1e-9);
        // Away from the face, nothing moves; nor does anything on a sheer crag.
        assert_eq!(faces.stand_off([feet[0], feet[1], feet[2] + 3.], 0.3), None);
        let sheer = boxes(&scenery)
            .into_iter()
            .find(|b| b.family == Family::Sheer)
            .unwrap();
        let at = [
            sheer.base[0],
            sheer.base[1] + 2.,
            sheer.base[2] + sheer.size[2] / 2. + 0.3,
        ];
        assert_eq!(faces.stand_off(at, 0.3), None);
    }

    #[test]
    fn the_fetched_descriptor_maps_every_module_and_its_levels() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../web/public/scenery/crag.kit"
        );
        let Ok(manifest) = std::fs::read_to_string(format!("{path}/manifest.json")) else {
            eprintln!("the crag kit is not fetched; skipping");
            return;
        };
        let manifest: Value = serde_json::from_str(&manifest).unwrap();
        let attachment = manifest["assets"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|asset| asset["attachments"].as_array().unwrap())
            .find(|a| a["name"] == "crag-descriptor")
            .unwrap();
        let json =
            std::fs::read_to_string(format!("{path}/{}", attachment["path"].as_str().unwrap()))
                .unwrap();
        let climbs = descriptor_climbs(&json).unwrap();
        assert!(FACE_CLIMB.iter().all(|k| climbs[*k].face > 0.));
        assert!(climbs.values().all(|c| (c.stand_off - 0.36).abs() < 1e-9));
        let lods = descriptor_lods(&json).unwrap();
        assert_eq!(lods.len(), KIT_ENTRIES.len());
        assert!(lods.iter().all(|l| l.levels.len() == 3));
        // The ivy on dressed climbable faces stands clear of the rock.
        let most = FACE_CLIMB
            .iter()
            .chain(&CORNER_CLIMB)
            .map(|k| climbs[*k].relief[2])
            .fold(0., f64::max);
        assert!(IVY_STAND_OFF >= most, "{most}");
    }
}
