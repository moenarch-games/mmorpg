//! Lures (#1019, *The River's Teeth*): a quest's supplied bait cast at an authored spot draws the
//! creatures of one kind near it round to face it, so their sides and backs are open.
//!
//! - **Spots** ([`Lure`]): an evidence spot, the bait item and the creature template it draws.
//! - **Casting**: [`crate::ZoneCommand::Inspect`] at a lure spot no quest objective needs, with the
//!   bait in the bag, lays it there for [`LURE_TICKS`]; the bait stays in the bag, so the same line
//!   is cast again once the last cast is spent. Without it the cast is refused with
//!   [`crate::ErrorCode::MissingMaterials`]. Casting at a spot whose lure still lies there changes
//!   nothing.
//! - **The draw**: a living, unstunned creature of the lure's template within [`LURE_RADIUS_UNITS`]
//!   of a lying lure holds its ground and turns toward it at the bounded engaged rate
//!   ([`crate::CREATURE_TURN_PER_TICK`]), in a fight or out of one. It still strikes whoever is in
//!   its reach; only its facing follows the bait.
//!
//! Lying lures are canonical zone state, so snapshots, replay and recovery continue them; none
//! lying stores nothing.

use crate::evidence::EvidenceId;
use crate::{CreatureId, CreatureTemplateId, ErrorCode, ItemId, PlayerId, ZoneError};

/// A lure draws creatures within 6 m of its spot.
pub const LURE_RADIUS_UNITS: i32 = 600;
/// A cast lure lies this long (20 s) before the bait is spent.
pub const LURE_TICKS: u32 = crate::long_ticks_from_ms(20_000);
/// Most lure spots a zone may hold.
pub const MAX_LURES: usize = 8;

/// A lure spot (#1019): bait `item` cast at evidence spot `spot` draws creatures of `template`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Lure {
    pub spot: EvidenceId,
    pub item: ItemId,
    pub template: CreatureTemplateId,
}

/// A lure lying at its spot (#1019), canonical.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LureState {
    pub spot: EvidenceId,
    /// The tick the bait is spent.
    pub until: u64,
}

impl crate::ZoneSimulation {
    /// The lure lying at `spot` (#1019), if any.
    #[must_use]
    pub fn lure(&self, spot: EvidenceId) -> Option<&LureState> {
        self.lures.get(&spot)
    }

    /// Casts `lure` at its spot for `player_id` (#1019): see the module documentation. The caller
    /// has checked the player stands within the spot's reach.
    pub(crate) fn cast_lure(
        &mut self,
        player_id: PlayerId,
        lure: Lure,
    ) -> Option<(ErrorCode, Option<crate::EntityRef>)> {
        if !self
            .players
            .get(&player_id)
            .is_some_and(|player| player.inventory.count(lure.item) > 0)
        {
            return Some((ErrorCode::MissingMaterials, None));
        }
        let until = self.tick.saturating_add(u64::from(LURE_TICKS));
        self.lures.entry(lure.spot).or_insert(LureState {
            spot: lure.spot,
            until,
        });
        None
    }

    /// The facing toward the nearest lying lure drawing creature `id` standing at `here` (#1019),
    /// if one does.
    pub(crate) fn lure_bearing(
        &self,
        id: CreatureId,
        template: CreatureTemplateId,
        here: [i32; 2],
    ) -> Option<u16> {
        // Most ticks no lure lies anywhere: every creature's question ends here.
        if self.lures.is_empty() {
            return None;
        }
        let creature = self.creatures.get(&id)?;
        if !creature.is_alive() || crate::aura::has_kind(&creature.auras, crate::AuraKind::Stun) {
            return None;
        }
        self.lures
            .keys()
            .filter_map(|&spot| {
                let lure = self.content.lure(spot)?;
                let at = self.content.evidence_spot(spot)?.at;
                let (dx, dz) = (at[0] - here[0], at[2] - here[1]);
                let far = i64::from(dx).pow(2) + i64::from(dz).pow(2);
                (lure.template == template && far <= i64::from(LURE_RADIUS_UNITS).pow(2))
                    .then_some((far, dx, dz))
            })
            .min_by_key(|&(far, ..)| far)
            .and_then(|(_, dx, dz)| crate::trig::yaw_from_vector(dx, dz))
    }

    /// Tick step (#1019): spent lures leave their spots.
    pub(crate) fn advance_lures(&mut self, now: u64) {
        if !self.lures.is_empty() {
            self.lures.retain(|_, lure| now < lure.until);
        }
    }

    /// Canonical lying lures in spot order.
    pub(crate) fn canonical_lures(&self) -> Vec<LureState> {
        self.lures.values().copied().collect()
    }

    /// Restores the lying lures from a snapshot taken at `tick`.
    pub(crate) fn restore_lures(
        &mut self,
        lures: &[LureState],
        tick: u64,
    ) -> Result<(), ZoneError> {
        if lures.windows(2).any(|pair| pair[0].spot >= pair[1].spot)
            || lures.iter().any(|lure| {
                self.content.lure(lure.spot).is_none()
                    || lure.until > tick.saturating_add(u64::from(LURE_TICKS))
            })
        {
            return Err(ZoneError::new("snapshot lures are invalid"));
        }
        self.lures = lures.iter().map(|lure| (lure.spot, *lure)).collect();
        Ok(())
    }
}
