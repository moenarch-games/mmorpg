//! Walk-in buildings (#158): where both clients draw asset-tooling's walk-in building bundles,
//! the Greyhaven inn (`greyhaven-inn-v1`) and Marshal Elden Greywatch's private home
//! (`greyhaven-marshal-house`), and how their doors and interior look follow the zone.
//!
//! Each building's entries are authored in one frame (origin at the footprint's base centre,
//! door on +Z) that the zone places at its content origin, turned by quarter turns about +Y and
//! lifted by the presentation relief like every unit. The door leaf hangs on its hinge and
//! eases toward the replicated state over [`SWING_SECONDS`]; the roof and ceiling are hidden
//! while the viewer stands inside the room. Presentation only: the zone's door bodies and
//! building colliders decide everything.
use mmorpg_core::{
    DoorId, UNITS_PER_METRE,
    greyhaven_vale::{inn, marshal_house},
};
use mmorpg_scenery::Scenery;

use crate::{
    batching::matrix,
    geometry::{compose, ry, translate},
};

/// A full swing between closed and open.
pub const SWING_SECONDS: f64 = 0.4;
/// A flipped door waits this long before its leaf swings: `Door_Push` and `Door_Pull`'s
/// `doorPushSeconds[0]`, when the palm meets the door (#619).
pub const CONTACT_SECONDS: f64 = 0.228;
const OPEN_ANGLE: f64 = std::f64::consts::FRAC_PI_2;

/// One walk-in building's bundle and placement.
#[derive(Clone, Debug, PartialEq)]
pub struct Spec {
    /// The bundle directory under `web/public/buildings` and its entry prefix.
    pub bundle: &'static str,
    /// The entries clients draw, in draw order; a building can have any number of parts.
    pub entries: &'static [&'static str],
    /// Entries hidden from an indoor viewer.
    pub hidden: &'static [&'static str],
    /// Authored room and camera obstacles; absent on the older footprint-based buildings.
    pub interior: Option<Interior>,
    /// The door this building presents.
    pub door: Option<DoorId>,
    /// The frame's origin in the zone, units.
    origin: [i32; 3],
    /// Quarter turns about +Y (+Z toward +X) from the frame to the zone.
    quarters: u8,
    /// The footprint's half extents in the frame, units.
    footprint: [i32; 3],
    /// The descriptor's hinge mount in the frame, metres; the leaf opens inward about +Y.
    hinge: [f64; 3],
}

impl Spec {
    /// The door leaf entry.
    #[must_use]
    pub fn door_entry(&self) -> Option<&'static str> {
        self.door.and_then(|_| self.entries.last().copied())
    }

    fn hidden_inside(&self, key: &str) -> bool {
        self.hidden.contains(&key)
    }
}

/// A descriptor box in the building's authored frame, metres.
#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
pub struct Volume {
    pub center: [f64; 3],
    pub size: [f64; 3],
}

impl Volume {
    fn contains(&self, p: [f64; 3]) -> bool {
        (0..3).all(|i| (p[i] - self.center[i]).abs() <= self.size[i] / 2. + 1e-6)
    }
}

/// Real room bounds and authored camera obstacles, independent of gameplay collision.
#[derive(Clone, Debug, PartialEq)]
pub struct Interior {
    pub volume: Volume,
    pub obstacles: Vec<Volume>,
}

impl Spec {
    /// A placed two-part mausoleum, from the monuments kit's actual descriptor attachment.
    /// This does not place it in any zone; placement belongs to #683.
    pub fn mausoleum(descriptor: &str, origin: [i32; 3], quarters: u8) -> Result<Self, String> {
        #[derive(serde::Deserialize)]
        struct Room {
            roof: String,
            volume: Volume,
        }
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Asset {
            key: String,
            interior: Option<Room>,
            #[serde(default)]
            collision_proxies: Vec<Volume>,
        }
        #[derive(serde::Deserialize)]
        struct Descriptor {
            assets: Vec<Asset>,
        }
        let descriptor: Descriptor = serde_json::from_str(descriptor).map_err(|e| e.to_string())?;
        let body = descriptor
            .assets
            .into_iter()
            .find(|a| a.key == "monuments.kit.mausoleum-walk-in")
            .ok_or("missing walk-in mausoleum")?;
        let room = body.interior.ok_or("missing mausoleum interior")?;
        if room.roof != "mausoleum-walk-in-roof" {
            return Err("unexpected mausoleum roof".into());
        }
        if std::iter::once(&room.volume)
            .chain(&body.collision_proxies)
            .any(|v| {
                !v.center.iter().all(|x| x.is_finite())
                    || !v.size.iter().all(|x| x.is_finite() && *x > 0.)
            })
        {
            return Err("invalid mausoleum volume".into());
        }
        Ok(Self {
            bundle: "monuments.kit",
            entries: &[
                "monuments.kit.mausoleum-walk-in",
                "monuments.kit.mausoleum-walk-in-roof",
            ],
            hidden: &["monuments.kit.mausoleum-walk-in-roof"],
            door: None,
            interior: Some(Interior {
                volume: room.volume,
                obstacles: body.collision_proxies,
            }),
            origin,
            quarters: quarters % 4,
            footprint: [0; 3],
            hinge: [0.; 3],
        })
    }
}

/// The Greyhaven inn, unrotated at [`inn::ORIGIN`].
pub const INN: Spec = Spec {
    bundle: "greyhaven.inn",
    entries: &[
        "greyhaven.inn.shell",
        "greyhaven.inn.furniture",
        "greyhaven.inn.ceiling",
        "greyhaven.inn.roof",
        "greyhaven.inn.door",
    ],
    hidden: &["greyhaven.inn.ceiling", "greyhaven.inn.roof"],
    interior: None,
    door: Some(inn::DOOR),
    origin: inn::ORIGIN,
    quarters: 0,
    footprint: inn::FOOTPRINT_HALF_EXTENTS,
    hinge: [-0.6, 0., 4.53],
};

/// Elden's house on house 103, turned half a turn so its door faces the square.
pub const MARSHAL_HOUSE: Spec = Spec {
    bundle: "greyhaven.marshal-house",
    entries: &[
        "greyhaven.marshal-house.shell",
        "greyhaven.marshal-house.furniture",
        "greyhaven.marshal-house.ceiling",
        "greyhaven.marshal-house.roof",
        "greyhaven.marshal-house.door",
    ],
    hidden: &[
        "greyhaven.marshal-house.ceiling",
        "greyhaven.marshal-house.roof",
    ],
    interior: None,
    door: Some(marshal_house::DOOR),
    origin: marshal_house::ORIGIN,
    quarters: 2,
    footprint: marshal_house::FOOTPRINT_HALF_EXTENTS,
    hinge: [-0.58, 0.01, 2.88],
};

/// Every walk-in building, in the order clients present them.
pub const BUILDINGS: [Spec; 2] = [INN, MARSHAL_HOUSE];

/// One drawn building entry this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Part {
    pub key: &'static str,
    /// Column-major entry-to-world matrix, metres.
    pub model: [f32; 16],
    pub visible: bool,
}

/// One building's presented door swing and interior state.
#[derive(Clone, Debug)]
pub struct WalkIn {
    spec: Spec,
    origin: [f64; 3],
    angle: f64,
    settled: bool,
    /// The door state last seen and how long until its leaf starts to swing (#619).
    open: bool,
    hold: f64,
    camera_ratio: f64,
}

fn metres(units: i32) -> f64 {
    f64::from(units) / f64::from(UNITS_PER_METRE)
}

impl WalkIn {
    /// `spec`'s building on `scenery`'s relief, door closed until the first frame says otherwise.
    #[must_use]
    pub fn new(spec: Spec, scenery: &Scenery) -> Self {
        let [x, y, z] = spec.origin;
        Self {
            spec,
            origin: [metres(x), metres(y + scenery.height_at(x, z)), metres(z)],
            angle: 0.,
            settled: false,
            open: false,
            hold: 0.,
            camera_ratio: 1.,
        }
    }

    /// Every walk-in building on `scenery`, in [`BUILDINGS`] order; none outside Greyhaven
    /// Vale.
    #[must_use]
    pub fn all(scenery: &Scenery) -> Vec<Self> {
        if !scenery.greyhaven() {
            return Vec::new();
        }
        BUILDINGS
            .iter()
            .map(|spec| Self::new(spec.clone(), scenery))
            .collect()
    }

    #[must_use]
    pub const fn spec(&self) -> &Spec {
        &self.spec
    }

    /// Whether feet at `position` (metres, zone frame) stand inside the authored room,
    /// or the legacy footprint when no room descriptor exists.
    #[must_use]
    pub fn inside(&self, position: [f64; 3]) -> bool {
        if let Some(interior) = &self.spec.interior {
            return interior.volume.contains(self.local(position));
        }
        let [mut hx, _, mut hz] = self.spec.footprint.map(metres);
        if self.spec.quarters % 2 == 1 {
            std::mem::swap(&mut hx, &mut hz);
        }
        (position[0] - self.origin[0]).abs() < hx && (position[2] - self.origin[2]).abs() < hz
    }

    /// The entries this frame: the door eased `dt` seconds toward `open` (snapped on the first
    /// frame and under reduced motion), the roof and ceiling hidden while `viewer` (feet,
    /// metres) is inside.
    pub fn frame(
        &mut self,
        open: bool,
        viewer: Option<[f64; 3]>,
        dt: f64,
        reduced_motion: bool,
    ) -> Vec<Part> {
        let goal = if open { OPEN_ANGLE } else { 0. };
        if open != self.open {
            self.open = open;
            self.hold = CONTACT_SECONDS;
        }
        if !self.settled || reduced_motion {
            self.angle = goal;
            self.settled = true;
            self.hold = 0.;
        } else {
            let waited = dt.max(0.).min(self.hold);
            self.hold -= waited;
            let step = OPEN_ANGLE * ((dt.max(0.) - waited) / SWING_SECONDS);
            self.angle += (goal - self.angle).clamp(-step, step);
        }
        let interior = viewer.is_some_and(|feet| self.inside(feet));
        let building = matrix(self.frame_transform());
        let door = matrix(self.leaf());
        let spec = &self.spec;
        spec.entries
            .iter()
            .map(|&key| Part {
                key,
                model: if Some(key) == spec.door_entry() {
                    door
                } else {
                    building
                },
                visible: !(interior && spec.hidden_inside(key)),
            })
            .collect()
    }

    fn local(&self, p: [f64; 3]) -> [f64; 3] {
        let [x, y, z] = std::array::from_fn(|i| p[i] - self.origin[i]);
        match self.spec.quarters % 4 {
            0 => [x, y, z],
            1 => [-z, y, x],
            2 => [-x, y, -z],
            _ => [z, y, -x],
        }
    }

    /// Pull in before an authored wall or furnishing, retaining orbit heading/pitch and zoom.
    /// Obstruction retracts immediately so no frame looks through stone; recovery eases out.
    /// Called outside too, to smoothly restore the user's chosen distance on leaving.
    pub fn camera(
        &mut self,
        viewer: [f64; 3],
        eye: [f64; 3],
        target: [f64; 3],
        dt: f64,
        reduced: bool,
    ) -> [f64; 3] {
        let mut safe: f64 = 1.;
        if self.inside(viewer)
            && let Some(room) = &self.spec.interior
        {
            let start = self.local(target);
            let end = self.local(eye);
            let ray: [f64; 3] = std::array::from_fn(|i| end[i] - start[i]);
            let length = ray.iter().map(|x| x * x).sum::<f64>().sqrt();
            for obstacle in &room.obstacles {
                if obstacle.contains(start) {
                    continue;
                }
                let mut enter: f64 = 0.;
                let mut exit: f64 = 1.;
                for i in 0..3 {
                    let low = obstacle.center[i] - obstacle.size[i] / 2.;
                    let high = obstacle.center[i] + obstacle.size[i] / 2.;
                    if ray[i].abs() < 1e-9 {
                        if start[i] < low || start[i] > high {
                            exit = -1.;
                        }
                    } else {
                        let a = (low - start[i]) / ray[i];
                        let b = (high - start[i]) / ray[i];
                        enter = enter.max(a.min(b));
                        exit = exit.min(a.max(b));
                    }
                }
                if enter <= exit {
                    safe = safe.min((enter - 0.3 / length.max(1e-6)).max(0.));
                }
            }
        }
        let blend = if reduced {
            1.
        } else {
            1. - (-12. * dt.max(0.)).exp()
        };
        self.camera_ratio = (self.camera_ratio + (safe - self.camera_ratio) * blend).min(safe);
        std::array::from_fn(|i| target[i] + (eye[i] - target[i]) * self.camera_ratio)
    }

    /// Interior sky occlusion / eye adaptation, following the player's feet.
    #[must_use]
    pub fn lighting(&self, viewer: [f64; 3]) -> f32 {
        if self.spec.interior.is_some() && self.inside(viewer) {
            1.
        } else {
            0.
        }
    }

    /// A warm steady lamp above the room, in the building's actual frame.
    #[must_use]
    pub fn light(&self) -> Option<crate::lights::PointLight> {
        let room = self.spec.interior.as_ref()?;
        let mut position = room.volume.center;
        position[1] += room.volume.size[1] * 0.25;
        Some(crate::lights::PointLight {
            position: crate::geometry::point(self.frame_transform(), position),
            lumens: 1800.,
            range: 8.,
            ..crate::lights::LAMP
        })
    }

    fn frame_transform(&self) -> crate::geometry::Affine {
        let [x, y, z] = self.origin;
        compose(&[
            translate(x, y, z),
            ry(f64::from(self.spec.quarters) * std::f64::consts::FRAC_PI_2),
        ])
    }

    fn leaf(&self) -> crate::geometry::Affine {
        let [hx, hy, hz] = self.spec.hinge;
        compose(&[
            self.frame_transform(),
            translate(hx, hy, hz),
            ry(self.angle),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn building(spec: Spec) -> WalkIn {
        WalkIn::new(spec, &mmorpg_scenery::greyhaven_vale_scenery())
    }

    // Actual monuments release descriptor excerpt; both authored pieces, no substitute geometry.
    const MONUMENTS: &str = include_str!("fixtures/monuments-interior.json");

    #[test]
    fn mausoleum_room_hides_only_the_roof_and_clears_the_camera_with_a_lamp() {
        let mut room = building(Spec::mausoleum(MONUMENTS, [0; 3], 0).unwrap());
        let feet = [1.5, 0.28, -0.6];
        assert!(room.inside(feet));
        assert!(!room.inside([0., 0.28, 2.8])); // porch, still on the stepped footprint
        assert!(!room.inside([1.5, 4., -0.6])); // above the room
        assert_eq!(room.spec().door, None);
        let parts = room.frame(false, Some(feet), 0., false);
        assert_eq!(parts.len(), 2);
        assert!(parts[0].visible);
        assert!(!parts[1].visible);
        assert_eq!(parts[0].model, parts[1].model); // pieces share their authored frame
        let target = [1.5, 1.5, -0.6];
        let eye = [1.5, 6., -12.];
        let close = room.camera(feet, eye, target, 1. / 60., false);
        assert!(close[2] > -3.09); // in front of the back wall's inner face
        assert!(close[2] < target[2]);
        let leaving = room.camera([10., 0., 0.], eye, target, 1. / 60., false);
        assert!(leaving[2] < close[2] && leaving[2] > eye[2]);
        assert_eq!(room.camera([10., 0., 0.], eye, target, 0., true), eye);
        assert_eq!(room.lighting(feet), 1.);
        assert_eq!(room.lighting([0., 0.28, 2.8]), 0.);
        let light = room.light().unwrap();
        assert!(room.inside(light.position));
        assert!(light.lumens > 0. && light.range >= 5.);
        let mut turned = building(Spec::mausoleum(MONUMENTS, [1000, 200, 1000], 1).unwrap());
        let floor = turned.origin[1] + 0.28;
        assert!(turned.inside([9.4, floor, 8.5]));
        assert!(!turned.inside([12.8, floor, 10.]));
        assert!(!turned.frame(false, Some([9.4, floor, 8.5]), 0., false)[1].visible);
        assert_eq!(turned.light().unwrap().position[0], 9.4);
    }

    #[test]
    fn the_door_eases_open_over_its_swing_and_hides_the_roof_inside() {
        let mut inn = building(INN);
        inn.frame(false, None, 0., false);
        // The leaf waits for the palm to meet it (#619).
        inn.frame(true, None, CONTACT_SECONDS, false);
        assert_eq!(inn.angle, 0.);
        inn.frame(true, None, SWING_SECONDS / 2., false);
        assert!((inn.angle - OPEN_ANGLE / 2.).abs() < 1e-9);
        inn.frame(true, None, SWING_SECONDS, false);
        assert_eq!(inn.angle, OPEN_ANGLE);
        let inside = inn.frame(true, Some([15., 0., -1.]), 0., false);
        assert!(inside.iter().all(|part| part.visible
            != matches!(part.key, "greyhaven.inn.roof" | "greyhaven.inn.ceiling")));
        assert!(
            inn.frame(true, Some([15., 0., 6.]), 0., false)
                .iter()
                .all(|part| part.visible)
        );
    }

    #[test]
    fn eldens_house_turns_its_door_toward_the_square() {
        let mut house = building(MARSHAL_HOUSE);
        let parts = house.frame(false, Some([-24., 0., 37.]), 0., false);
        assert!(parts.iter().all(|part| part.visible
            != matches!(
                part.key,
                "greyhaven.marshal-house.roof" | "greyhaven.marshal-house.ceiling"
            )));
        // The closed leaf's hinge sits on the north (−Z) front, east of the doorway's centre.
        let door = parts[4].model;
        assert!((f64::from(door[12]) - (-24. + 0.58)).abs() < 1e-4);
        assert!((f64::from(door[14]) - (36. - 2.88)).abs() < 1e-4);
    }
}
