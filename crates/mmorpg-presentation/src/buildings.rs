//! Walk-in buildings (#158): where both clients draw asset-tooling's walk-in building bundles,
//! the Greyhaven inn (`greyhaven-inn-v1`) and Marshal Elden Greywatch's private home
//! (`greyhaven-marshal-house`), and how their doors and interior look follow the zone.
//!
//! Each building's entries are authored in one frame (origin at the footprint's base centre,
//! door on +Z) that the zone places at its content origin, turned by quarter turns about +Y and
//! lifted by the presentation relief like every unit. The door leaf hangs on its hinge and
//! eases toward the replicated state over [`SWING_SECONDS`]; the roof and ceiling are hidden
//! while the viewer stands inside the footprint. Presentation only: the zone's door bodies and
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
const OPEN_ANGLE: f64 = std::f64::consts::FRAC_PI_2;

/// One walk-in building's bundle and placement.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spec {
    /// The bundle directory under `web/public/buildings` and its entry prefix.
    pub bundle: &'static str,
    /// The entries the clients draw, in draw order: shell, furniture, ceiling, roof, door (the
    /// collision entry is not drawn). The ceiling and roof hide while the viewer is inside.
    pub entries: [&'static str; 5],
    /// The door this building presents.
    pub door: DoorId,
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
    pub const fn door_entry(&self) -> &'static str {
        self.entries[4]
    }

    fn hidden_inside(&self, key: &str) -> bool {
        key == self.entries[2] || key == self.entries[3]
    }
}

/// The Greyhaven inn, unrotated at [`inn::ORIGIN`].
pub const INN: Spec = Spec {
    bundle: "greyhaven.inn",
    entries: [
        "greyhaven.inn.shell",
        "greyhaven.inn.furniture",
        "greyhaven.inn.ceiling",
        "greyhaven.inn.roof",
        "greyhaven.inn.door",
    ],
    door: inn::DOOR,
    origin: inn::ORIGIN,
    quarters: 0,
    footprint: inn::FOOTPRINT_HALF_EXTENTS,
    hinge: [-0.6, 0., 4.53],
};

/// Elden's house on house 103, turned half a turn so its door faces the square.
pub const MARSHAL_HOUSE: Spec = Spec {
    bundle: "greyhaven.marshal-house",
    entries: [
        "greyhaven.marshal-house.shell",
        "greyhaven.marshal-house.furniture",
        "greyhaven.marshal-house.ceiling",
        "greyhaven.marshal-house.roof",
        "greyhaven.marshal-house.door",
    ],
    door: marshal_house::DOOR,
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
        }
    }

    /// Every walk-in building on `scenery`, in [`BUILDINGS`] order.
    #[must_use]
    pub fn all(scenery: &Scenery) -> Vec<Self> {
        BUILDINGS
            .iter()
            .map(|&spec| Self::new(spec, scenery))
            .collect()
    }

    #[must_use]
    pub const fn spec(&self) -> &Spec {
        &self.spec
    }

    /// Whether feet at `position` (metres, zone frame) stand inside the footprint.
    #[must_use]
    pub fn inside(&self, position: [f64; 3]) -> bool {
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
    ) -> [Part; 5] {
        let goal = if open { OPEN_ANGLE } else { 0. };
        if !self.settled || reduced_motion {
            self.angle = goal;
            self.settled = true;
        } else {
            let step = OPEN_ANGLE * (dt.max(0.) / SWING_SECONDS);
            self.angle += (goal - self.angle).clamp(-step, step);
        }
        let interior = viewer.is_some_and(|feet| self.inside(feet));
        let building = matrix(self.frame_transform());
        let door = matrix(self.leaf());
        let spec = self.spec;
        spec.entries.map(|key| Part {
            key,
            model: if key == spec.door_entry() {
                door
            } else {
                building
            },
            visible: !(interior && spec.hidden_inside(key)),
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

    #[test]
    fn the_door_eases_open_over_its_swing_and_hides_the_roof_inside() {
        let mut inn = building(INN);
        inn.frame(false, None, 0., false);
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
