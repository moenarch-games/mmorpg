//! Floating and drifting props (#667): which water a dynamic piece is in, how buoyant its kind
//! is and the current there. The zone derives a [`PieceMedium`] for every debris piece, sliding
//! prop and rolling boulder before each physics step from its canonical position and the
//! content's [`crate::WaterVolume`]s; the medium is never stored, so replay and canonical
//! recovery stay exact. Physics Engine applies the buoyancy and the drag toward the current
//! (physics-engine #525).
//!
//! Wood floats at the depth its density gives; stone sinks. Pieces drift with a channel's
//! current along its centre line ([`crate::WaterChannel::flow_at`]) and settle where a bank or
//! obstacle stops them, as any piece settles once slow.

use crate::physics::{DebrisBody, PieceMedium, SUBUNITS_PER_UNIT};
use crate::{PropKind, WaterVolume};

/// Fresh water (kg/m³).
pub const WATER_DENSITY: f32 = 1_000.0;

/// A prop kind's density (kg/m³), for how it floats (#667). Tuning, not approved numbers:
/// wood floats (a crate's thin boards highest, a loaded heavy crate lowest), an explosive
/// barrel's powder keeps it barely afloat, a boulder sinks.
#[must_use]
pub const fn density(kind: PropKind) -> f32 {
    match kind {
        PropKind::Crate => 500.0,
        PropKind::Fence => 550.0,
        PropKind::Barrel => 650.0,
        PropKind::HeavyCrate => 700.0,
        PropKind::ExplosiveBarrel => 850.0,
        PropKind::Boulder => 2_600.0,
    }
}

/// The water around a piece of `kind`, if its centre lies over a volume and its body reaches
/// below that volume's surface: the surface, the kind's buoyancy there and the current (a
/// channel's flow at the piece; lakes and pools are still).
#[must_use]
pub fn piece_medium(
    waters: &[WaterVolume],
    kind: PropKind,
    piece: &DebrisBody,
) -> Option<PieceMedium> {
    let [x, y, z] = crate::physics::to_units(piece.position);
    let water = waters.iter().find(|water| water.contains(x, z))?;
    let half = piece.half_extents;
    // The farthest any point of the piece reaches from its centre, whatever its turn.
    let reach = half.x + half.y + half.z;
    if y - reach > water.surface {
        return None;
    }
    let [fx, fz] = water
        .channel
        .as_ref()
        .map_or([0, 0], |channel| channel.flow_at(x, z));
    #[allow(clippy::cast_precision_loss)]
    let per_tick = |subunits: i32| subunits as f32 / SUBUNITS_PER_UNIT as f32;
    #[allow(clippy::cast_precision_loss)]
    Some(PieceMedium {
        surface: water.surface as f32,
        buoyancy: WATER_DENSITY / density(kind),
        flow: [per_tick(fx), 0.0, per_tick(fz)],
    })
}

impl crate::ZoneSimulation {
    /// The media of `pieces` of the matching `kinds`, or none at all when the zone has no water.
    pub(crate) fn piece_media(
        &self,
        pieces: &[DebrisBody],
        kinds: impl IntoIterator<Item = PropKind>,
    ) -> Vec<Option<PieceMedium>> {
        let waters = self.content().waters();
        if waters.is_empty() {
            return Vec::new();
        }
        pieces
            .iter()
            .zip(kinds)
            .map(|(piece, kind)| piece_medium(waters, kind, piece))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wood_floats_and_stone_sinks() {
        for kind in [
            PropKind::Crate,
            PropKind::Fence,
            PropKind::Barrel,
            PropKind::HeavyCrate,
            PropKind::ExplosiveBarrel,
        ] {
            assert!(density(kind) < WATER_DENSITY, "{kind:?}");
        }
        assert!(density(PropKind::Boulder) > WATER_DENSITY);
    }
}
