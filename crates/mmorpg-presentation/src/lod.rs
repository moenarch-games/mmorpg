//! Levels of detail from an asset-tooling descriptor's `lodSelection` metadata (the
//! greyhaven-nature format, asset-tooling #449, which the fortress kit #445 shares): per asset a
//! bounding sphere and levels finest first, each used while the sphere's projected size
//! (radius over distance × tan(fovY / 2), the diameter's fraction of the viewport height) is at
//! least its `minScreenSize`.
//!
//! [`Lods`] picks each placement's level per frame and cross-fades over [`FADE_SECONDS`] at
//! every switch, for the nature bundle (#49) and the composition kits (#599) alike. It culls
//! a placement wholly outside the camera's side planes by the descriptor's `bounds` box (the
//! drawn art, decoration included), or by the bounding sphere where the asset has none.
//!
//! The selection is also the sun's shadow-caster set: the sun cascades can draw only what it
//! keeps. While the sun casts shadows ([`Lods::set_sun`]), the side-plane test sweeps each box
//! down the sunlight until it has fallen its own height, so a tree beside or behind the camera
//! whose shadow reaches into view stays (#1038); the cascades cull those casters per view.
use serde_json::Value;

/// The one selection metric the descriptors use.
pub const METRIC: &str = "projected-bounding-sphere-height-fraction";

/// The lowest sun elevation, as its sine (10°), a shadow-caster sweep is measured for: a lower
/// sun's shadow, over five times as long as its caster is tall, is cut there.
pub const SHADOW_SWEEP_MIN_SINE: f64 = 0.173_648_177_666_930_35;
/// The sun direction is kept in steps of this much per component (under a degree), so a
/// slowly turning sun re-selects a resting camera's placements only once a step.
const SUN_STEP: f64 = 1. / 64.;

/// A switch between two levels of detail cross-fades over this long.
pub const FADE_SECONDS: f64 = 0.35;
/// A level switches only once the screen size is this fraction past its threshold, so a
/// placement standing at a boundary does not flicker between levels.
pub const HYSTERESIS: f64 = 0.08;

/// One entry's levels of detail, from the descriptor's `lods` and `bounds`.
#[derive(Clone, Debug, PartialEq)]
pub struct EntryLods {
    pub entry: String,
    /// Bounding-sphere centre in the entry's frame and its radius, metres.
    pub centre: [f64; 3],
    pub radius: f64,
    /// `(variant, minScreenSize)` from the finest level to the coarsest.
    pub levels: Vec<(String, f64)>,
    /// The drawn art's box `[min, max]` in the entry's frame, metres, when the descriptor
    /// states it.
    pub bounds: Option<[[f64; 3]; 2]>,
}

/// A JSON array of exactly `N` numbers, as the bundles' descriptors write points and spans.
pub(crate) fn numbers<const N: usize>(value: &Value) -> Option<[f64; N]> {
    let value = value.as_array().filter(|c| c.len() == N)?;
    let mut out = [0.; N];
    for (axis, coordinate) in value.iter().enumerate() {
        out[axis] = coordinate.as_f64()?;
    }
    Some(out)
}

/// Every asset's levels in a descriptor with `lodSelection`, or, in a descriptor without it (a
/// kit that ships only `default`, like the settlement props and the sandstone kit), each asset's
/// one level ([`single_level_lods`]).
///
/// # Errors
///
/// For an unknown metric or an asset without a key, bounding sphere or levels.
pub fn descriptor_lods(descriptor: &Value) -> Result<Vec<EntryLods>, String> {
    if descriptor.get("lodSelection").is_none() {
        return single_level_lods(descriptor);
    }
    let metric = descriptor["lodSelection"]["metric"].as_str();
    if metric != Some(METRIC) {
        return Err(format!("unknown LOD metric {metric:?}"));
    }
    let assets = descriptor["assets"]
        .as_array()
        .ok_or("the descriptor lists no assets")?;
    assets
        .iter()
        .map(|asset| {
            let entry = asset["key"].as_str().ok_or("an asset without key")?;
            let lods = &asset["lods"];
            let sphere = &lods["boundingSphere"];
            let centre = numbers::<3>(&sphere["center"])
                .ok_or_else(|| format!("{entry} has no bounding sphere"))?;
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
            let bounds = numbers::<3>(&asset["bounds"]["min"])
                .zip(numbers::<3>(&asset["bounds"]["max"]))
                .map(|(min, max)| [min, max]);
            Ok(EntryLods {
                entry: entry.to_owned(),
                centre,
                radius,
                levels,
                bounds,
            })
        })
        .collect()
}

/// Each asset's one level (`default`, always drawn) with the sphere round its `bounds` box, for
/// a descriptor without `lodSelection`.
///
/// # Errors
///
/// When the descriptor lists no assets or an asset lacks a key or bounds.
pub fn single_level_lods(descriptor: &Value) -> Result<Vec<EntryLods>, String> {
    descriptor["assets"]
        .as_array()
        .ok_or("the descriptor lists no assets")?
        .iter()
        .map(|asset| {
            let entry = asset["key"].as_str().ok_or("an asset without key")?;
            let (min, max) = numbers::<3>(&asset["bounds"]["min"])
                .zip(numbers::<3>(&asset["bounds"]["max"]))
                .ok_or_else(|| format!("{entry} has no bounds"))?;
            let centre = std::array::from_fn(|axis| (min[axis] + max[axis]) / 2.);
            let radius = (0..3)
                .map(|axis| ((max[axis] - min[axis]) / 2.).powi(2))
                .sum::<f64>()
                .sqrt();
            Ok(EntryLods {
                entry: entry.to_owned(),
                centre,
                radius,
                levels: vec![("default".to_owned(), 0.)],
                bounds: Some([min, max]),
            })
        })
        .collect()
}

/// One placement [`Lods`] selects levels for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placed<'a> {
    /// The descriptor entry; an entry the descriptor lacks is never drawn.
    pub entry: &'a str,
    /// Column-major entry-to-world matrix, metres.
    pub model: [f32; 16],
    /// The largest axis scale of `model`, which scales the entry's bounding sphere.
    pub scale: f64,
    /// Camera distance (metres, flat, to the sphere's edge) beyond which it is hidden.
    pub reach: f64,
    /// Optional fixed origin-distance ceiling, independent of bounding sphere and scale.
    pub max_distance: Option<f64>,
}

/// One placed entry of a bundle drawn at fixed placements: the nature bundle's trees, rocks and
/// bushes (#49) and the climbing kit's parts (#621).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    /// The bundle entry.
    pub entry: &'static str,
    /// Column-major entry-to-world matrix, metres.
    pub model: [f32; 16],
    /// The largest axis scale of `model`, which scales the entry's bounding sphere.
    pub scale: f64,
    /// Beyond its class's reach the placement is hidden, like the procedural prop.
    pub cull: crate::Cull,
}

impl Placement {
    /// The placement as [`Lods`] selects its levels.
    #[must_use]
    pub fn placed(&self) -> Placed<'static> {
        Placed {
            entry: self.entry,
            model: self.model,
            scale: self.scale,
            reach: self.cull.reach(),
            max_distance: (self.cull == crate::Cull::Scatter).then_some(60.),
        }
    }
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

/// A placement in world space: its sphere for the level, its box for culling.
#[derive(Clone, Debug)]
struct Instance {
    lods: usize,
    centre: [f64; 3],
    radius: f64,
    reach: f64,
    max_distance: Option<f64>,
    origin: [f64; 3],
    /// The world box's centre and its three half axes (the sphere's when the entry states no
    /// bounds).
    box_centre: [f64; 3],
    half_axes: Option<[[f64; 3]; 3]>,
    /// The world box's height, which its shadow sweep falls.
    height: f64,
}

impl Instance {
    fn new(entries: &[EntryLods], placed: &Placed<'_>) -> Self {
        let lods = entries
            .iter()
            .position(|entry| entry.entry == placed.entry)
            .unwrap_or(usize::MAX);
        let m = placed.model.map(f64::from);
        let at = |[x, y, z]: [f64; 3]| {
            [
                m[0] * x + m[4] * y + m[8] * z + m[12],
                m[1] * x + m[5] * y + m[9] * z + m[13],
                m[2] * x + m[6] * y + m[10] * z + m[14],
            ]
        };
        let Some(entry) = entries.get(lods) else {
            return Self {
                lods,
                centre: [0.; 3],
                radius: 0.,
                reach: placed.reach,
                max_distance: placed.max_distance,
                origin: at([0.; 3]),
                box_centre: [0.; 3],
                half_axes: None,
                height: 0.,
            };
        };
        let centre = at(entry.centre);
        let (box_centre, half_axes) = entry.bounds.map_or((centre, None), |[min, max]| {
            let middle = std::array::from_fn(|axis| (min[axis] + max[axis]) / 2.);
            let half: [f64; 3] = std::array::from_fn(|axis| (max[axis] - min[axis]) / 2.);
            let axes = std::array::from_fn(|column| {
                std::array::from_fn(|row| m[column * 4 + row] * half[column])
            });
            (at(middle), Some(axes))
        });
        let radius = entry.radius * placed.scale;
        let height =
            2. * half_axes.map_or(radius, |axes| axes.iter().map(|axis| axis[1].abs()).sum());
        Self {
            lods,
            centre,
            radius,
            reach: placed.reach,
            max_distance: placed.max_distance,
            origin: at([0.; 3]),
            box_centre,
            half_axes,
            height,
        }
    }

    /// Whether the placement, swept `lean[i]` metres along plane `i`'s normal per metre of its
    /// height (its shadow), lies wholly behind one of `planes`.
    fn outside(&self, planes: &[[f64; 4]; 4], lean: &[f64; 4]) -> bool {
        planes.iter().zip(lean).any(|(&[a, b, c, d], lean)| {
            let [x, y, z] = self.box_centre;
            let extent = self.half_axes.map_or(self.radius, |axes| {
                axes.iter()
                    .map(|[u, v, w]| (a * u + b * v + c * w).abs())
                    .sum()
            });
            a * x + b * y + c * z + d + lean * self.height < -extent
        })
    }
}

/// Each placement's level of detail and cross-fade, advanced once per frame.
#[derive(Clone, Debug)]
pub struct Lods {
    entries: Vec<EntryLods>,
    instances: Vec<Instance>,
    /// Placements a client switched off (#917: a gathered vein's full or spent look).
    hidden: Vec<bool>,
    states: Vec<State>,
    shown: Vec<Shown>,
    /// The previous update's result, kept to tell whether this one changed anything.
    before: Vec<Shown>,
    /// The camera of the last update, and whether a cross-fade was still running then: an
    /// update for the same camera while nothing fades or moves changes nothing and is skipped.
    view: Option<View>,
    fading: bool,
    changed: bool,
    /// The direction sunlight travels while the sun casts shadows, in [`SUN_STEP`]s.
    sun: Option<[f64; 3]>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct View {
    eye: [f64; 3],
    fov_y: f64,
    bias: f64,
    fade: bool,
    clip_from_world: Option<[f32; 16]>,
}

impl Lods {
    /// Levels for `placements` from the descriptor's `entries`.
    #[must_use]
    pub fn new<'a>(
        entries: Vec<EntryLods>,
        placements: impl IntoIterator<Item = Placed<'a>>,
    ) -> Self {
        let instances = placements
            .into_iter()
            .map(|placed| Instance::new(&entries, &placed))
            .collect::<Vec<_>>();
        let states = vec![
            State {
                level: None,
                previous: None,
                fade: 1.,
            };
            instances.len()
        ];
        let hidden = vec![false; instances.len()];
        Self {
            entries,
            instances,
            hidden,
            states,
            shown: Vec::new(),
            before: Vec::new(),
            view: None,
            fading: false,
            changed: false,
            sun: None,
        }
    }

    /// The entries' levels, as given.
    #[must_use]
    pub fn entries(&self) -> &[EntryLods] {
        &self.entries
    }

    /// Moves placement `index` (a lift piece following its platform) to `placed`, which keeps
    /// its entry; the next [`Self::update`] evaluates it again.
    pub fn place(&mut self, index: usize, placed: Placed<'_>) {
        let Some(instance) = self.instances.get_mut(index) else {
            return;
        };
        *instance = Instance::new(&self.entries, &placed);
        self.view = None;
    }

    /// Hides placement `index` (#917), or shows it again; the next [`Self::update`] evaluates
    /// every placement again.
    pub fn set_hidden(&mut self, index: usize, hidden: bool) {
        if let Some(slot) = self.hidden.get_mut(index)
            && *slot != hidden
        {
            *slot = hidden;
            self.view = None;
        }
    }

    /// The direction sunlight travels (any length, downwards) while the sun casts shadows, or
    /// `None` (#1038): with it, [`Self::update`] keeps a placement outside the view whose
    /// shadow may fall inside. Kept in [`SUN_STEP`]s; a new step evaluates every placement again.
    pub fn set_sun(&mut self, direction: Option<[f64; 3]>) {
        let sun = direction
            .filter(|d| d.iter().all(|c| c.is_finite()) && d.iter().any(|&c| c != 0.))
            .map(|d| {
                let length = d[0].hypot(d[1]).hypot(d[2]);
                d.map(|c| (c / length / SUN_STEP).round() * SUN_STEP)
            });
        if sun != self.sun {
            self.sun = sun;
            self.view = None;
        }
    }

    /// Advances every placement by `seconds` for a camera at `eye` (metres) with vertical
    /// field of view `fov_y` (radians). `bias` multiplies every `minScreenSize` (a model-detail
    /// setting: larger switches to coarser levels sooner); without `fade` (reduced motion)
    /// switches are immediate. With the camera's column-major `clip_from_world` matrix,
    /// placements wholly outside its side planes are not drawn, unless the sun
    /// ([`Self::set_sun`]) casts their shadow inside. Returns what to draw. For the
    /// same camera while nothing fades or moved it returns the last result without evaluating
    /// any placement; [`Self::changed`] tells whether the result changed.
    pub fn update(
        &mut self,
        eye: [f64; 3],
        fov_y: f64,
        seconds: f64,
        bias: f64,
        fade: bool,
        clip_from_world: Option<[f32; 16]>,
    ) -> &[Shown] {
        let view = View {
            eye,
            fov_y,
            bias,
            fade,
            clip_from_world,
        };
        if !self.fading && self.view == Some(view) {
            self.changed = false;
            return &self.shown;
        }
        self.view = Some(view);
        let planes = clip_from_world.map(side_planes);
        // Per side plane, how far into it a box's shadow reaches per metre of the box's height:
        // swept down the sunlight until it has fallen that height.
        let lean = match (&planes, self.sun) {
            (Some(planes), Some(sun)) => {
                let length = sun[0].hypot(sun[1]).hypot(sun[2]);
                let fall = (-sun[1] / length).max(SHADOW_SWEEP_MIN_SINE);
                planes.map(|[a, b, c, _]| {
                    ((a * sun[0] + b * sun[1] + c * sun[2]) / length / fall).max(0.)
                })
            }
            _ => [0.; 4],
        };
        let tan = (fov_y / 2.).tan().max(1e-6);
        let step = if fade {
            (seconds / FADE_SECONDS).max(0.)
        } else {
            1.
        };
        std::mem::swap(&mut self.shown, &mut self.before);
        self.shown.clear();
        for (index, (instance, state)) in self.instances.iter().zip(&mut self.states).enumerate() {
            let Some(entry) = self.entries.get(instance.lods) else {
                continue;
            };
            let [dx, dy, dz] = std::array::from_fn(|axis| instance.centre[axis] - eye[axis]);
            // `sqrt` rather than `hypot`: wasm's `hypot` is a slow software routine, and this runs for every
            // placement every frame the camera moves; these distances never near overflow.
            let flat = (dx * dx + dz * dz).sqrt();
            let beyond_max = instance.max_distance.is_some_and(|limit| {
                let (ox, oz) = (instance.origin[0] - eye[0], instance.origin[2] - eye[2]);
                (ox * ox + oz * oz).sqrt() > limit
            });
            let target =
                if self.hidden[index] || flat - instance.radius > instance.reach || beyond_max {
                    None
                } else {
                    let distance = (flat * flat + dy * dy).sqrt().max(1e-3);
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
            if planes
                .as_ref()
                .is_some_and(|planes| instance.outside(planes, &lean))
            {
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
        self.fading = self.shown.iter().any(|shown| shown.fade < 1.);
        self.changed = self.shown != self.before;
        &self.shown
    }

    /// What the last [`Self::update`] returned.
    #[must_use]
    pub fn shown(&self) -> &[Shown] {
        &self.shown
    }

    /// Whether the last [`Self::update`] changed what is drawn.
    #[must_use]
    pub fn changed(&self) -> bool {
        self.changed
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
            entry: "tree".into(),
            centre: [0., 4., 0.],
            radius: 5.7,
            levels: vec![
                ("default".into(), 0.5),
                ("lod1".into(), 0.12),
                ("lod2".into(), 0.),
            ],
            bounds: None,
        }
    }

    fn translated(x: f32, z: f32) -> [f32; 16] {
        let mut model = [0.; 16];
        model[0] = 1.;
        model[5] = 1.;
        model[10] = 1.;
        model[15] = 1.;
        model[12] = x;
        model[14] = z;
        model
    }

    fn at(x: f32) -> Placed<'static> {
        Placed {
            entry: "tree",
            model: translated(x, 0.),
            scale: 1.,
            reach: 420.,
            max_distance: None,
        }
    }

    #[test]
    fn ground_scatter_stops_at_sixty_metres_independent_of_scale_and_bounds() {
        let mut placement = at(0.);
        placement.scale = 5.;
        placement.max_distance = Some(60.);
        let mut lods = Lods::new(vec![tree()], [placement]);
        assert!(
            !lods
                .update([59., 4., 0.], 1., 0., 1., false, None)
                .is_empty()
        );
        assert!(
            lods.update([61., 4., 0.], 1., 0., 1., false, None)
                .is_empty()
        );
    }

    #[test]
    fn levels_follow_the_projected_size_and_cross_fade() {
        let fov = 60f64.to_radians();
        let mut lods = Lods::new(vec![tree()], [at(10.), at(50.), at(200.), at(900.)]);
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
        let mut lods = Lods::new(vec![tree()], [at(10.)]);
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
    fn an_unchanged_camera_changes_nothing_once_the_fades_settle() {
        let fov = 60f64.to_radians();
        let mut lods = Lods::new(vec![tree()], [at(10.), at(50.)]);
        lods.update([0., 4., 0.], fov, 0.016, 1., true, None);
        assert!(lods.changed());
        lods.update([0., 4., 0.], fov, 0.016, 1., true, None);
        assert!(!lods.changed(), "same camera, nothing fading");
        // Moving switches the near tree; its fade runs on while the camera rests.
        lods.update([-20., 4., 0.], fov, 0.016, 1., true, None);
        assert!(lods.changed());
        let fading = lods
            .update([-20., 4., 0.], fov, 0.1, 1., true, None)
            .to_vec();
        assert!(lods.changed() && fading.iter().any(|s| s.fade < 1.));
        lods.update([-20., 4., 0.], fov, 1., 1., true, None);
        assert!(lods.changed(), "the fade settles");
        let settled = lods
            .update([-20., 4., 0.], fov, 1., 1., true, None)
            .to_vec();
        assert!(!lods.changed());
        assert!(settled.iter().all(|s| s.fade == 1.));
        // A placement moved under a resting camera is evaluated again.
        lods.place(0, at(-19.));
        let near = lods
            .update([-20., 4., 0.], fov, 1., 1., true, None)
            .to_vec();
        assert!(lods.changed());
        assert_eq!((near[0].level, near[1].level), (0, 1), "fading back in");
    }

    #[test]
    fn a_long_wall_is_culled_by_its_box_not_its_sphere() {
        // An 80 m wall along x, 4 m deep: its sphere reaches 40 m to either side, its box 2 m.
        let wall = EntryLods {
            entry: "wall".into(),
            centre: [0., 5., 0.],
            radius: 40.,
            levels: vec![("default".into(), 0.)],
            bounds: Some([[-40., 0., -2.], [40., 10., 2.]]),
        };
        let sphere = EntryLods {
            bounds: None,
            ..wall.clone()
        };
        // Looking down -z from the origin with a 60° frustum, the wall turned a quarter about
        // +Y (running along z) and centred 50 m off to the right: its box lies wholly outside
        // the right plane, its sphere reaches back inside.
        let turned = |x: f32| {
            let mut model = translated(x, -20.);
            model[0] = 0.;
            model[2] = -1.;
            model[8] = 1.;
            model[10] = 0.;
            model
        };
        let fov = 60f64.to_radians();
        let projection = looking_down_minus_z(fov);
        let placed = Placed {
            entry: "wall",
            model: turned(50.),
            scale: 1.,
            reach: f64::INFINITY,
            max_distance: None,
        };
        let mut boxed = Lods::new(vec![wall], [placed]);
        let mut round = Lods::new(vec![sphere], [placed]);
        let eye = [0., 5., 0.];
        assert!(
            boxed
                .update(eye, fov, 0., 1., false, Some(projection))
                .is_empty()
        );
        assert_eq!(
            round
                .update(eye, fov, 0., 1., false, Some(projection))
                .len(),
            1
        );
    }

    /// The clip-from-world matrix of a square camera at the origin looking down -z.
    fn looking_down_minus_z(fov: f64) -> [f32; 16] {
        let f = 1. / (fov / 2.).tan();
        let (near, far) = (0.1, 1000.);
        let mut m = [0_f32; 16];
        m[0] = f as f32;
        m[5] = f as f32;
        m[10] = ((far + near) / (near - far)) as f32;
        m[11] = -1.;
        m[14] = (2. * far * near / (near - far)) as f32;
        m
    }

    #[test]
    fn a_tree_off_screen_stays_while_its_shadow_falls_in_view() {
        // A 12 m tree whose box stands 24 m right of a 60° view looking down -z, 20 m ahead:
        // the right edge there is 11.5 m out, so the whole tree is off screen. Its shadow under a
        // sun 45° high in the east (light travelling -x) reaches back to x = 9, into view (#1038).
        let tree = EntryLods {
            bounds: Some([[-3., 0., -3.], [3., 12., 3.]]),
            ..tree()
        };
        let placed = Placed {
            model: translated(24., -20.),
            ..at(0.)
        };
        let fov = 60f64.to_radians();
        let clip = Some(looking_down_minus_z(fov));
        let mut lods = Lods::new(vec![tree], [placed]);
        let drawn = |lods: &mut Lods, sun: Option<[f64; 3]>| {
            lods.set_sun(sun);
            lods.update([0., 0., 0.], fov, 0., 1., false, clip).len()
        };
        assert_eq!(
            drawn(&mut lods, None),
            0,
            "without sun shadows the side planes cull it"
        );
        assert_eq!(
            drawn(&mut lods, Some([-1., -1., 0.])),
            1,
            "its shadow falls into view"
        );
        assert_eq!(
            drawn(&mut lods, Some([1., -1., 0.])),
            0,
            "its shadow falls away from the view"
        );
        // A sun at noon drops the shadow at its foot, off screen too.
        assert_eq!(drawn(&mut lods, Some([0., -1., 0.])), 0);
        // A low sun's longer shadow reaches a tree further out; one past its sweep is culled.
        let far = Placed {
            model: translated(40., -20.),
            ..placed
        };
        lods.place(0, far);
        assert_eq!(
            drawn(&mut lods, Some([-1., -1., 0.])),
            0,
            "a 45° shadow falls short"
        );
        assert_eq!(
            drawn(&mut lods, Some([-1., -0.4, 0.])),
            1,
            "a 22° shadow reaches in"
        );
    }

    #[test]
    fn a_sun_turning_within_a_step_leaves_a_resting_camera_alone() {
        let fov = 60f64.to_radians();
        let clip = Some(looking_down_minus_z(fov));
        let ahead = Placed {
            model: translated(0., -20.),
            ..at(0.)
        };
        let mut lods = Lods::new(vec![tree()], [ahead]);
        lods.set_sun(Some([-0.5, -0.8, 0.3]));
        lods.update([0., 4., 0.], fov, 0., 1., false, clip);
        assert!(lods.changed());
        // Within a step the resting camera's selection is kept, not evaluated again.
        lods.set_sun(Some([-0.501, -0.8, 0.3]));
        assert!(lods.view.is_some());
        // A sun of any length is one direction.
        lods.set_sun(Some([-5., -8., 3.]));
        assert!(lods.view.is_some());
        lods.set_sun(Some([-0.6, -0.8, 0.3]));
        assert!(
            lods.view.is_none(),
            "a new step evaluates every placement again"
        );
        lods.update([0., 4., 0.], fov, 0., 1., false, clip);
        assert!(!lods.changed());
    }

    #[test]
    fn the_descriptor_parses_its_lod_levels() {
        let json: Value = serde_json::from_str(
            r#"{"lodSelection":{"metric":"projected-bounding-sphere-height-fraction"},
          "assets":[{"key":"k","bounds":{"min":[-1,0,-1],"max":[1,2,1]},
          "lods":{"boundingSphere":{"center":[0,1,0],"radius":2},
          "levels":[{"variant":"default","minScreenSize":0.3},{"variant":"lod1","minScreenSize":0}]}}]}"#,
        )
        .unwrap();
        let lods = descriptor_lods(&json).unwrap();
        assert_eq!(
            lods[0].levels,
            [("default".into(), 0.3), ("lod1".into(), 0.)]
        );
        assert_eq!(lods[0].centre, [0., 1., 0.]);
        assert_eq!(lods[0].bounds, Some([[-1., 0., -1.], [1., 2., 1.]]));
        assert!(descriptor_lods(&serde_json::json!({"lodSelection": {"metric": "x"}})).is_err());
    }
}
