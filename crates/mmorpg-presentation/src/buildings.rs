//! Walk-in buildings (#158): where both clients draw asset-tooling's Greyhaven inn bundle
//! (`greyhaven-inn-v1`) and how its door and interior look follow the zone.
//!
//! The inn's entries are authored in one frame (origin at the footprint's base centre, door on
//! +Z) that the zone places unrotated at [`inn::ORIGIN`], lifted by the presentation relief
//! like every unit. The door leaf hangs on its hinge and eases toward the replicated state
//! over [`SWING_SECONDS`]; the roof and ceiling are hidden while the viewer stands inside the
//! footprint. Presentation only: the zone's door body and inn colliders decide everything.
use mmorpg_core::{DoorId, UNITS_PER_METRE, greyhaven_vale::inn};
use mmorpg_scenery::Scenery;

use crate::{
    batching::matrix,
    geometry::{compose, ry, translate},
};

/// The inn bundle's entries the clients draw, in draw order (the collision entry is not drawn).
pub const INN_ENTRIES: [&str; 5] = [
    "greyhaven.inn.shell",
    "greyhaven.inn.furniture",
    "greyhaven.inn.ceiling",
    "greyhaven.inn.roof",
    "greyhaven.inn.door",
];
/// The door this building presents.
pub const INN_DOOR: DoorId = inn::DOOR;
/// A full swing between closed and open.
pub const SWING_SECONDS: f64 = 0.4;
/// The descriptor's hinge mount in the inn frame, metres; the leaf opens inward about +Y.
const HINGE: [f64; 3] = [-0.6, 0., 4.53];
const OPEN_ANGLE: f64 = std::f64::consts::FRAC_PI_2;

/// One drawn inn entry this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Part {
    pub key: &'static str,
    /// Column-major entry-to-world matrix, metres.
    pub model: [f32; 16],
    pub visible: bool,
}

/// The inn's presented door swing and interior state.
#[derive(Clone, Debug)]
pub struct Inn {
    origin: [f64; 3],
    angle: f64,
    settled: bool,
}

fn metres(units: i32) -> f64 {
    f64::from(units) / f64::from(UNITS_PER_METRE)
}

impl Inn {
    /// The inn on `scenery`'s relief, door closed until the first frame says otherwise.
    #[must_use]
    pub fn new(scenery: &Scenery) -> Self {
        let [x, y, z] = inn::ORIGIN;
        Self {
            origin: [metres(x), metres(y + scenery.height_at(x, z)), metres(z)],
            angle: 0.,
            settled: false,
        }
    }

    /// Whether feet at `position` (metres, zone frame) stand inside the footprint.
    #[must_use]
    pub fn inside(&self, position: [f64; 3]) -> bool {
        let [hx, _, hz] = inn::FOOTPRINT_HALF_EXTENTS.map(metres);
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
        let [x, y, z] = self.origin;
        let building = matrix(translate(x, y, z));
        let door = matrix(self.leaf());
        INN_ENTRIES.map(|key| Part {
            key,
            model: if key == "greyhaven.inn.door" {
                door
            } else {
                building
            },
            visible: !(interior && matches!(key, "greyhaven.inn.roof" | "greyhaven.inn.ceiling")),
        })
    }

    fn leaf(&self) -> crate::geometry::Affine {
        let [x, y, z] = self.origin;
        compose(&[
            translate(x + HINGE[0], y + HINGE[1], z + HINGE[2]),
            ry(self.angle),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inn() -> Inn {
        Inn::new(&mmorpg_scenery::greyhaven_vale_scenery())
    }

    #[test]
    fn the_door_eases_open_over_its_swing_and_hides_the_roof_inside() {
        let mut inn = inn();
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
}
