//! Greyhaven's wildlife dens (#912, owner choice 2026-11-17: hybrid ecosystem).
//!
//! The Wolfrun Den lies deep in the woods' western half, more than 20 m from the trail-end
//! clearing, so the den wolf (108) there stays a new hunter's lone first fight. Three of the
//! woods' wolves form its pack: they rest at the den, then roam out to the boar grounds north and
//! south of it in turn, coming home between outings, and run down a boar grazing near the spot
//! they reach. The pack keeps its existing spawns; only where it rests and roams is new.

use crate::{CreatureId, WildlifeDen};

/// The den, west of the trail-end clearing.
pub const WOLFRUN_DEN: [i32; 2] = [-10_800, 300];
/// Wolves 105, 109 and 113; 106 nearby stays a lone wolf to hunt.
const PACK: [u32; 3] = [109, 105, 113];
/// Beside boars 123 and 120, off the trail and paths and away from the trail-end clearing.
const OUTINGS: [[i32; 2]; 2] = [[-10_900, 2_900], [-11_000, -1_000]];
const PREY: [u32; 2] = [123, 120];

pub(super) fn dens() -> Vec<WildlifeDen> {
    vec![WildlifeDen {
        name: "Wolfrun Den".into(),
        den: WOLFRUN_DEN,
        pack: PACK.into_iter().map(CreatureId::new).collect(),
        outings: OUTINGS.to_vec(),
        prey: PREY.into_iter().map(CreatureId::new).collect(),
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The den and every outing spot lie in Wolfrun Woods, away from the trail-end clearing;
    /// the den stands off the roads and the outing spots more than 10 m from every road.
    #[test]
    fn the_wolfrun_den_keeps_to_the_woods_and_off_the_roads() {
        let (_, _, woods) = super::super::vale::AREAS
            .iter()
            .find(|(id, ..)| *id == super::super::vale::WOLFRUN_WOODS)
            .expect("Wolfrun Woods is an area");
        for den in dens() {
            for [x, z] in den.anchors() {
                // Clear of the trail-end clearing's lone first fight by more than 20 m.
                let [cx, cz] = super::super::woods::CLEARING_CENTRE;
                let (dx, dz) = (i64::from(x - cx), i64::from(z - cz));
                assert!(
                    dx * dx + dz * dz > 2_000_i64.pow(2),
                    "[{x}, {z}] is by the clearing"
                );
                assert!(
                    (woods.min[0]..woods.max[0]).contains(&x),
                    "{x} lies outside the woods"
                );
                assert!(
                    (woods.min[1]..woods.max[1]).contains(&z),
                    "{z} lies outside the woods"
                );
            }
            assert!(super::super::road_distance(den.den[0], den.den[1]) >= 600);
            for [x, z] in &den.outings {
                assert!(
                    super::super::road_distance(*x, *z) >= 1_000,
                    "[{x}, {z}] lies within 10 m of a road"
                );
            }
        }
        assert_eq!(super::super::content().dens(), dens().as_slice());
    }
}
