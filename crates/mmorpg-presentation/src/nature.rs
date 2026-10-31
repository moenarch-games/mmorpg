//! Greyhaven's trees and rocks from asset-tooling's `greyhaven-nature` bundle (#49).
//!
//! Every scenery tree and every rock that is not climbing rock becomes one placed bundle
//! entry: oaks and birches the broadleaf, pines the conifer, rocks one of the three rocks. A
//! collider rock's entry is the one its zone collider collides as
//! ([`mmorpg_core::RockShape`]), fitted into the collider's box exactly like the proxy, so the
//! drawn rock and its collision agree. While a client does not draw the bundle, the static
//! scene draws the procedural trees and rocks ([`crate::Kit::Nature`] fallback groups).
//!
//! [`Lods`] picks each placement's level of detail from the bundle's `lodSelection` metadata
//! (projected bounding-sphere diameter over viewport height) and cross-fades over
//! [`FADE_SECONDS`] at every switch. Presentation only.
use mmorpg_core::{Grip, ROCK_PROXIES, RockShape, UNITS_PER_METRE};
use mmorpg_scenery::{Prop, PropKind, Scenery, TerrainGrid, TreeVariant};

use crate::{
    Cull,
    batching::matrix,
    geometry::{compose, ry, scale, translate},
};

pub const BROADLEAF: &str = "greyhaven.nature.tree.broadleaf";
pub const CONIFER: &str = "greyhaven.nature.tree.conifer";
/// The bundle's entries, sorted.
pub const ENTRIES: [&str; 5] = [
    "greyhaven.nature.rock.1",
    "greyhaven.nature.rock.2",
    "greyhaven.nature.rock.3",
    BROADLEAF,
    CONIFER,
];

/// A switch between two levels of detail cross-fades over this long.
pub const FADE_SECONDS: f64 = 0.35;
/// A level switches only once the screen size is this fraction past its threshold, so a
/// placement standing at a boundary does not flicker between levels.
pub const HYSTERESIS: f64 = 0.08;

/// One placed bundle entry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    /// The bundle entry, one of [`ENTRIES`].
    pub entry: &'static str,
    /// Column-major entry-to-world matrix, metres.
    pub model: [f32; 16],
    /// The largest axis scale of `model`, which scales the entry's bounding sphere.
    pub scale: f64,
    /// Beyond its class's reach the placement is hidden, like the procedural prop.
    pub cull: Cull,
}

fn metres(units: i32) -> f64 {
    f64::from(units) / f64::from(UNITS_PER_METRE)
}

fn yaw(p: &Prop) -> f64 {
    f64::from(p.yaw) / 65_536. * std::f64::consts::TAU
}

/// Whether the bundle draws `p` (the procedural model is its fallback): every tree, and every
/// rock that is not climbing rock, whose worn handholds (#512) stay on the procedural rock.
#[must_use]
pub fn draws(s: &Scenery, p: &Prop) -> bool {
    match p.kind {
        PropKind::Tree(_) => true,
        PropKind::Rock(_) => !p
            .collider
            .and_then(|id| s.climbing().collider(id))
            .is_some_and(|grip| grip == Grip::Rock),
        _ => false,
    }
}

/// The rock `p` is drawn as: its collider's proxy, or for a rock without one, a stable pick.
fn rock_shape(s: &Scenery, p: &Prop) -> RockShape {
    p.collider
        .and_then(|id| s.rock(id))
        .unwrap_or_else(|| RockShape {
            proxy: u8::try_from(
                (u64::from(p.position[0].unsigned_abs()) * 31
                    + u64::from(p.position[2].unsigned_abs()))
                    % 3,
            )
            .unwrap_or(0),
            quarters: 0,
        })
}

/// `p`'s placement on `s` (drawn terrain `g`), when the bundle draws it.
pub(crate) fn placement(s: &Scenery, g: &TerrainGrid, p: &Prop) -> Option<Placement> {
    if !draws(s, p) {
        return None;
    }
    let x = metres(p.position[0]);
    let z = metres(p.position[2]);
    let seed = crate::props::seed(p);
    let (entry, frame, largest, cull) = match p.kind {
        PropKind::Tree(variant) => {
            let background = p.collider.is_none();
            // Fitted to the procedural tree's height: about 7.5 m in the woods.
            let grow = if background {
                f64::from(p.scale_permille) / 1000.
            } else {
                0.85 + seed * 0.4
            };
            let (entry, size) = match variant {
                TreeVariant::Pine => (CONIFER, 0.86 * grow),
                TreeVariant::Oak | TreeVariant::Birch => (BROADLEAF, 0.8 * grow),
            };
            let at = compose(&[
                translate(x, crate::terrain::surface(g, x, z), z),
                ry(yaw(p)),
                scale(size, size, size),
            ]);
            let cull = if background { Cull::Always } else { Cull::Far };
            (entry, at, size, cull)
        }
        PropKind::Rock(_) => {
            let shape = rock_shape(s, p);
            let proxy = &ROCK_PROXIES[usize::from(shape.proxy)];
            let [hx, hy, hz] = p.half_extents.map(metres);
            let centre: [f64; 3] =
                std::array::from_fn(|axis| metres(proxy.min[axis] + proxy.max[axis]) / 2.);
            let span: [f64; 3] =
                std::array::from_fn(|axis| metres(proxy.max[axis] - proxy.min[axis]) / 2.);
            let fit = [hx / span[0], hy / span[1], hz / span[2]];
            // A collider rock stands on its body's bottom, where its proxy collides.
            let base = if p.collider.is_some() {
                metres(p.position[1])
            } else {
                crate::terrain::surface(g, x, z)
            };
            let at = compose(&[
                translate(x, base + hy, z),
                ry(yaw(p)),
                scale(fit[0], fit[1], fit[2]),
                translate(-centre[0], -centre[1], -centre[2]),
            ]);
            let cull = if p.collider.is_some() {
                Cull::Always
            } else {
                Cull::Mid
            };
            (shape.entry(), at, fit[0].max(fit[1]).max(fit[2]), cull)
        }
        _ => return None,
    };
    Some(Placement {
        entry,
        model: matrix(frame),
        scale: largest,
        cull,
    })
}

/// Every placed tree and rock the bundle draws on `s`, in prop order.
#[must_use]
pub fn placements(s: &Scenery) -> Vec<Placement> {
    let g = s.terrain_grid(crate::TERRAIN_STEP_UNITS);
    s.props.iter().filter_map(|p| placement(s, &g, p)).collect()
}

/// One entry's levels of detail, from the bundle descriptor's `lods`.
#[derive(Clone, Debug, PartialEq)]
pub struct EntryLods {
    pub entry: String,
    /// Bounding-sphere centre in the entry's frame and its radius, metres.
    pub centre: [f64; 3],
    pub radius: f64,
    /// `(variant, minScreenSize)` from the finest level to the coarsest.
    pub levels: Vec<(String, f64)>,
}

/// Every entry's levels from the bundle's `nature-descriptor` attachment JSON.
pub fn descriptor_lods(json: &str) -> Result<Vec<EntryLods>, String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|error| format!("nature descriptor: {error}"))?;
    let metric = value["lodSelection"]["metric"].as_str();
    if metric != Some("projected-bounding-sphere-height-fraction") {
        return Err(format!("unknown nature LOD metric {metric:?}"));
    }
    let assets = value["assets"]
        .as_array()
        .ok_or("the nature descriptor lists no assets")?;
    assets
        .iter()
        .map(|asset| {
            let entry = asset["key"].as_str().ok_or("a nature asset without key")?;
            let lods = &asset["lods"];
            let sphere = &lods["boundingSphere"];
            let centre = sphere["center"]
                .as_array()
                .filter(|c| c.len() == 3)
                .ok_or_else(|| format!("{entry} has no bounding sphere"))?;
            let centre = std::array::from_fn(|axis| centre[axis].as_f64().unwrap_or(0.));
            let radius = sphere["radius"]
                .as_f64()
                .ok_or_else(|| format!("{entry} has no bounding radius"))?;
            let levels = lods["levels"]
                .as_array()
                .ok_or_else(|| format!("{entry} has no levels"))?
                .iter()
                .map(|level| {
                    Some((
                        level["variant"].as_str()?.to_owned(),
                        level["minScreenSize"].as_f64()?,
                    ))
                })
                .collect::<Option<Vec<_>>>()
                .filter(|levels| !levels.is_empty())
                .ok_or_else(|| format!("{entry} has malformed levels"))?;
            Ok(EntryLods {
                entry: entry.to_owned(),
                centre,
                radius,
                levels,
            })
        })
        .collect()
}

/// One drawn level of a placement this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shown {
    /// Index into the placements [`Lods`] was built from.
    pub placement: usize,
    /// Index into the entry's levels.
    pub level: usize,
    /// `1` settled. While cross-fading, the incoming level draws `t` in `(0, 1)`, a dithered
    /// fraction `t` of its pixels, and the outgoing one `-t`, the complementary pixels.
    pub fade: f32,
}

#[derive(Clone, Copy, Debug)]
struct State {
    level: Option<usize>,
    previous: Option<usize>,
    /// Progress of the cross-fade from `previous` to `level`, 0 to 1.
    fade: f64,
}

#[derive(Clone, Debug)]
struct Instance {
    lods: usize,
    centre: [f64; 3],
    radius: f64,
    reach: f64,
}

/// Each placement's level of detail and cross-fade, advanced once per frame.
#[derive(Clone, Debug)]
pub struct Lods {
    entries: Vec<EntryLods>,
    instances: Vec<Instance>,
    states: Vec<State>,
    shown: Vec<Shown>,
}

impl Lods {
    /// Levels for `placements` from the descriptor's `entries`; an entry the descriptor
    /// lacks is never drawn.
    #[must_use]
    pub fn new(entries: Vec<EntryLods>, placements: &[Placement]) -> Self {
        let instances = placements
            .iter()
            .map(|placement| {
                let lods = entries
                    .iter()
                    .position(|entry| entry.entry == placement.entry)
                    .unwrap_or(usize::MAX);
                let (centre, radius) = entries.get(lods).map_or(([0.; 3], 0.), |entry| {
                    let m = placement.model.map(f64::from);
                    let [x, y, z] = entry.centre;
                    (
                        [
                            m[0] * x + m[4] * y + m[8] * z + m[12],
                            m[1] * x + m[5] * y + m[9] * z + m[13],
                            m[2] * x + m[6] * y + m[10] * z + m[14],
                        ],
                        entry.radius * placement.scale,
                    )
                });
                Instance {
                    lods,
                    centre,
                    radius,
                    reach: placement.cull.reach(),
                }
            })
            .collect::<Vec<_>>();
        let states = vec![
            State {
                level: None,
                previous: None,
                fade: 1.,
            };
            instances.len()
        ];
        Self {
            entries,
            instances,
            states,
            shown: Vec::new(),
        }
    }

    /// The entries' levels, as given.
    #[must_use]
    pub fn entries(&self) -> &[EntryLods] {
        &self.entries
    }

    /// Advances every placement by `seconds` for a camera at `eye` (metres) with vertical
    /// field of view `fov_y` (radians). `bias` multiplies every `minScreenSize` (a model-detail
    /// setting: larger switches to coarser levels sooner); without `fade` (reduced motion)
    /// switches are immediate. With the camera's column-major `clip_from_world` matrix,
    /// placements wholly outside its side planes are not drawn. Returns what to draw.
    pub fn update(
        &mut self,
        eye: [f64; 3],
        fov_y: f64,
        seconds: f64,
        bias: f64,
        fade: bool,
        clip_from_world: Option<[f32; 16]>,
    ) -> &[Shown] {
        let planes = clip_from_world.map(side_planes);
        let tan = (fov_y / 2.).tan().max(1e-6);
        let step = if fade {
            (seconds / FADE_SECONDS).max(0.)
        } else {
            1.
        };
        self.shown.clear();
        for (index, (instance, state)) in self.instances.iter().zip(&mut self.states).enumerate() {
            let Some(entry) = self.entries.get(instance.lods) else {
                continue;
            };
            let [dx, dy, dz] = std::array::from_fn(|axis| instance.centre[axis] - eye[axis]);
            let flat = dx.hypot(dz);
            let target = if flat - instance.radius > instance.reach {
                None
            } else {
                let distance = (flat.hypot(dy)).max(1e-3);
                let size = instance.radius / (distance * tan);
                let first = |margin: f64| {
                    entry
                        .levels
                        .iter()
                        .position(|(_, min)| min * bias * margin <= size)
                        .unwrap_or(entry.levels.len() - 1)
                };
                Some(match state.level {
                    None => first(1.),
                    Some(level) => {
                        let finer = first(1. + HYSTERESIS);
                        let coarser = first(1. - HYSTERESIS);
                        if finer < level {
                            finer
                        } else if coarser > level {
                            coarser
                        } else {
                            level
                        }
                    }
                })
            };
            if target != state.level {
                // Hidden beyond reach, or shown from hiding, the placement pops like the
                // procedural prop did; between two levels it cross-fades.
                let switch = target.is_some() && state.level.is_some();
                state.previous = if switch { state.level } else { None };
                state.fade = if switch && fade { 0. } else { 1. };
                state.level = target;
            } else {
                state.fade = (state.fade + step).min(1.);
            }
            let Some(level) = state.level else {
                continue;
            };
            if planes.as_ref().is_some_and(|planes| {
                planes.iter().any(|[a, b, c, d]| {
                    let [x, y, z] = instance.centre;
                    a * x + b * y + c * z + d < -instance.radius
                })
            }) {
                continue;
            }
            match state.previous.filter(|_| state.fade < 1.) {
                Some(previous) => {
                    let t = state.fade.clamp(0.01, 0.99) as f32;
                    self.shown.push(Shown {
                        placement: index,
                        level,
                        fade: t,
                    });
                    self.shown.push(Shown {
                        placement: index,
                        level: previous,
                        fade: -t,
                    });
                }
                None => self.shown.push(Shown {
                    placement: index,
                    level,
                    fade: 1.,
                }),
            }
        }
        &self.shown
    }
}

/// The left, right, bottom and top planes `[a, b, c, d]` of a clip-from-world matrix
/// (Gribb–Hartmann), normalized so `a x + b y + c z + d` is the signed distance inside.
fn side_planes(m: [f32; 16]) -> [[f64; 4]; 4] {
    let m = m.map(f64::from);
    let row = |r: usize| [m[r], m[4 + r], m[8 + r], m[12 + r]];
    let (x, y, w) = (row(0), row(1), row(3));
    let plane = |p: [f64; 4], sign: f64, q: [f64; 4]| {
        let v: [f64; 4] = std::array::from_fn(|i| p[i] + sign * q[i]);
        let length = v[0].hypot(v[1]).hypot(v[2]).max(1e-9);
        v.map(|value| value / length)
    };
    [
        plane(w, 1., x),
        plane(w, -1., x),
        plane(w, 1., y),
        plane(w, -1., y),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> EntryLods {
        EntryLods {
            entry: BROADLEAF.into(),
            centre: [0., 4., 0.],
            radius: 5.7,
            levels: vec![
                ("default".into(), 0.5),
                ("lod1".into(), 0.12),
                ("lod2".into(), 0.),
            ],
        }
    }

    fn at(x: f32) -> Placement {
        let mut model = [0.; 16];
        model[0] = 1.;
        model[5] = 1.;
        model[10] = 1.;
        model[15] = 1.;
        model[12] = x;
        Placement {
            entry: BROADLEAF,
            model,
            scale: 1.,
            cull: Cull::Far,
        }
    }

    #[test]
    fn levels_follow_the_projected_size_and_cross_fade() {
        let fov = 60f64.to_radians();
        let mut lods = Lods::new(vec![tree()], &[at(10.), at(50.), at(200.), at(900.)]);
        let shown = lods.update([0., 4., 0.], fov, 0., 1., true, None).to_vec();
        assert_eq!(
            shown
                .iter()
                .map(|s| (s.placement, s.level, s.fade))
                .collect::<Vec<_>>(),
            [(0, 0, 1.), (1, 1, 1.), (2, 2, 1.)],
            "near, mid and far levels; beyond the far reach hidden"
        );
        // The near tree is now 30 m away: it fades from the detailed to the mid level.
        let mut lods = Lods::new(vec![tree()], &[at(10.)]);
        lods.update([0., 4., 0.], fov, 0., 1., true, None);
        let fading = lods
            .update([-20., 4., 0.], fov, 0.1, 1., true, None)
            .to_vec();
        assert_eq!(fading.len(), 2);
        assert_eq!((fading[0].level, fading[1].level), (1, 0));
        assert!(fading[0].fade > 0. && fading[0].fade < 1.);
        assert_eq!(fading[1].fade, -fading[0].fade);
        let settled = lods
            .update([-20., 4., 0.], fov, 1., 1., true, None)
            .to_vec();
        assert_eq!(settled.len(), 1);
        assert_eq!((settled[0].level, settled[0].fade), (1, 1.));
        // Just past the switch point, hysteresis holds the level.
        let boundary = 5.7 / (0.12 * (fov / 2.).tan());
        lods.update([10. - boundary * 1.02, 4., 0.], fov, 1., 1., true, None);
        let held = lods.update([10. - boundary * 1.02, 4., 0.], fov, 1., 1., true, None);
        assert_eq!(held[0].level, 1);
    }

    #[test]
    fn every_tree_and_unclimbable_rock_is_placed_as_its_collider_rock() {
        let scenery = mmorpg_scenery::greyhaven_vale_scenery();
        let placements = placements(&scenery);
        let drawn: Vec<_> = scenery
            .props
            .iter()
            .filter(|p| draws(&scenery, p))
            .collect();
        assert_eq!(placements.len(), drawn.len());
        assert!(drawn.iter().any(|p| matches!(p.kind, PropKind::Rock(_))));
        let mut hulls = 0;
        for (prop, placement) in drawn.iter().zip(&placements) {
            if let Some(shape) = prop.collider.and_then(|id| scenery.rock(id)) {
                assert_eq!(placement.entry, shape.entry());
                hulls += 1;
            }
        }
        assert!(
            hulls > 300,
            "the loose rocks collide as their drawn rock: {hulls}"
        );
    }

    #[test]
    fn the_descriptor_parses_its_lod_levels() {
        let json = r#"{"lodSelection":{"metric":"projected-bounding-sphere-height-fraction"},
          "assets":[{"key":"k","lods":{"boundingSphere":{"center":[0,1,0],"radius":2},
          "levels":[{"variant":"default","minScreenSize":0.3},{"variant":"lod1","minScreenSize":0}]}}]}"#;
        let lods = descriptor_lods(json).unwrap();
        assert_eq!(
            lods[0].levels,
            [("default".into(), 0.3), ("lod1".into(), 0.)]
        );
        assert_eq!(lods[0].radius, 2.);
    }
}
