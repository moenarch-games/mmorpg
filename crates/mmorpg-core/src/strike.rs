//! Directed strikes (#154): the Warden's pointer-aimed execution mode of
//! Heroic Strike. The client submits facing and a high/middle/low,
//! left/centre/right aim; the zone resolves which eligible unit the sword's
//! physical sweep covers (Atlas 5.2.27, 5.2.53).
//!
//! **Coverage** is a horizontal arc and a vertical band:
//!
//! - the arc is centred on the strike facing turned 45° toward the aimed
//!   side and spans [`STRIKE_ARC_HALF_WIDTH`] either way; it reaches
//!   [`PLAYER_REACH_UNITS`] (XZ centre distance), the same melee reach as the
//!   selected-target strike;
//! - the band is a height range above the Warden's feet ([`StrikeHeight`]);
//!   a unit is covered when its physics body's vertical extent overlaps
//!   it, so a high slash passes over a wolf and a low one under nothing;
//! - static content colliders block the sweep through the Rapier physics world
//!   ray query that line of sight uses.
//!
//! The nearest covered living unit (ties in `EntityRef` order) takes the
//! ability's weapon-strike damage; there is no cleave. A sweep that covers
//! nobody still swings: it pays the shared Rage cost and starts the shared
//! global cooldown like the selected-target mode (one ability, Atlas 5.2.53),
//! and reports a whiff.

use crate::ability::{AbilityEffect, AbilityId, ids};
use crate::ai::distance_squared;
use crate::trig::{self, YAW_EIGHTH_TURN, YAW_QUARTER_TURN};
use crate::unit::PLAYER_REACH_UNITS;
use crate::{EntityRef, PLAYER_HALF_EXTENTS_UNITS, PlayerId, ZoneError, ZoneSimulation};

/// Half the horizontal arc one directed strike sweeps (30°).
pub const STRIKE_ARC_HALF_WIDTH: u16 = YAW_QUARTER_TURN / 3;

/// Which way the sword sweeps relative to the strike facing.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum StrikeSide {
    Left,
    Centre,
    Right,
}

impl StrikeSide {
    pub const ALL: [Self; 3] = [Self::Left, Self::Centre, Self::Right];

    /// Wire and canonical code: 0 left, 1 centre, 2 right.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Left => 0,
            Self::Centre => 1,
            Self::Right => 2,
        }
    }

    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Left),
            1 => Some(Self::Centre),
            2 => Some(Self::Right),
            _ => None,
        }
    }

    /// The arc's centre yaw for a strike facing `facing`; the character's
    /// right is `facing - 90°`, like strafing.
    #[must_use]
    pub const fn arc_centre(self, facing: u16) -> u16 {
        match self {
            Self::Left => facing.wrapping_add(YAW_EIGHTH_TURN),
            Self::Centre => facing,
            Self::Right => facing.wrapping_sub(YAW_EIGHTH_TURN),
        }
    }
}

/// How high the sword sweeps.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum StrikeHeight {
    Low,
    Middle,
    High,
}

impl StrikeHeight {
    pub const ALL: [Self; 3] = [Self::Low, Self::Middle, Self::High];

    /// Wire and canonical code: 0 low, 1 middle, 2 high.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Low => 0,
            Self::Middle => 1,
            Self::High => 2,
        }
    }

    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Low),
            1 => Some(Self::Middle),
            2 => Some(Self::High),
            _ => None,
        }
    }

    /// The inclusive height band above the striker's feet, in units, that
    /// the blade covers. A Warden stands 180 units tall.
    #[must_use]
    pub const fn band(self) -> [i32; 2] {
        match self {
            Self::Low => [0, 60],
            Self::Middle => [60, 130],
            Self::High => [130, 200],
        }
    }
}

/// The aimed direction of one directed strike.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct StrikeAim {
    pub side: StrikeSide,
    pub height: StrikeHeight,
}

impl StrikeAim {
    #[must_use]
    pub const fn new(side: StrikeSide, height: StrikeHeight) -> Self {
        Self { side, height }
    }
}

/// Whether `ability` has a directed execution mode: only Heroic Strike.
#[must_use]
pub const fn has_directed_mode(ability: AbilityId) -> bool {
    ability.get() == ids::HEROIC_STRIKE.get()
}

impl ZoneSimulation {
    /// Validates and resolves one directed strike of `ability`. Validation
    /// is the selected-target order (class, learned, alive, stunned,
    /// casting, cooldowns, resource) with "has a directed mode" after
    /// learned; coverage replaces the target and range checks.
    pub(crate) fn directed_strike(
        &mut self,
        player_id: PlayerId,
        ability: u8,
        facing: u16,
        aim: StrikeAim,
        now: u64,
    ) -> Result<crate::casting::Refusal, ZoneError> {
        self.update_player(player_id, |player| player.combo = None);
        let ability = match self.ready_ability(player_id, ability, true, false) {
            None => return Ok(None),
            Some(Err(code)) => return Ok(Some((code, None))),
            Some(Ok(ability)) => ability,
        };
        let source = EntityRef::Player(player_id);
        let global_cooldown = self.recovery_of(
            player_id,
            crate::action::ActionId::GlobalCooldown,
            ability.id,
        );
        let strike_recovery = self.recovery_of(
            player_id,
            crate::action::ActionId::DirectedSlash,
            ability.id,
        );
        self.update_player(player_id, |player| {
            player.global_cooldown = global_cooldown;
            player.facing = facing;
            // The sword gesture supersedes a held guard (Atlas 5.2.37) and
            // commits to its recovery (#156).
            player.guard.raised = None;
            player.strike_recovery = strike_recovery;
        });
        self.pay_and_cool_down(player_id, ability);
        let victim = self.strike_coverage(player_id, facing, aim)?;
        let target = victim;
        self.notify_units(
            [Some(source), target],
            crate::ZoneEvent::StrikeSwept {
                source,
                target,
                ability: ability.id,
                aim,
            },
        );
        if let (Some(victim), AbilityEffect::WeaponStrike { .. }) = (victim, ability.effect) {
            let amount = self.weapon_strike_amount(player_id, ability.effect);
            let amount = self.stance_damage(player_id, target, amount)?;
            let before = self.struck_health(victim);
            self.player_melee_hits_at(player_id, victim, amount, false, now, Some(aim.height))?;
            // Only positive health damage after absorbs opens the combo (#157).
            if ability.id == crate::ability::ids::HEROIC_STRIKE
                && self.struck_health(victim) < before
            {
                self.update_player(player_id, |player| {
                    player.combo = Some(crate::ComboOpening {
                        target: victim,
                        elapsed: 0,
                    });
                });
            }
        }
        Ok(None)
    }

    fn struck_health(&self, target: EntityRef) -> u32 {
        match target {
            EntityRef::Creature(id) => self.creatures.get(&id).map_or(0, |unit| unit.health),
            EntityRef::Player(id) => self.players.get(&id).map_or(0, |unit| unit.health),
            EntityRef::Npc(_) | EntityRef::Minion(_) => 0,
        }
    }

    /// The eligible living unit a sweep from the player's body covers: nearest
    /// first, then in `EntityRef` order; `None` is a whiff.
    pub(crate) fn strike_coverage(
        &self,
        player_id: PlayerId,
        facing: u16,
        aim: StrikeAim,
    ) -> Result<Option<EntityRef>, ZoneError> {
        let origin = self.player_position(player_id)?;
        let feet = origin.y - PLAYER_HALF_EXTENTS_UNITS[1];
        let [band_low, band_high] = aim.height.band();
        let (dir_x, dir_z) = trig::direction(aim.side.arc_centre(facing));
        let min_cos = i64::from(trig::cos(STRIKE_ARC_HALF_WIDTH));
        let reach = i64::from(PLAYER_REACH_UNITS).pow(2);
        let mut best: Option<(i64, EntityRef)> = None;
        for entity in self.interest.near(origin.x, origin.z, PLAYER_REACH_UNITS) {
            if !self.attackable_by_player(player_id, entity) {
                continue;
            }
            let body = self
                .world
                .body(crate::entity::body_id(entity))
                .ok_or_else(|| ZoneError::new("creature physics body is missing"))?;
            let (centre, half) = (body.position(), body.half_extents());
            let distance = distance_squared([origin.x, origin.z], [centre.x, centre.z]);
            if distance > reach || best.is_some_and(|best| (distance, entity) >= best) {
                continue;
            }
            // |v| cos(angle to the arc centre) >= |v| cos(half width), both
            // Q16; a creature centred on the striker is always in the arc.
            let (dx, dz) = (centre.x - origin.x, centre.z - origin.z);
            let along = i64::from(dx) * i64::from(dir_x) + i64::from(dz) * i64::from(dir_z);
            let length = i64::try_from(trig::xz_length(dx, dz))
                .map_err(|_| ZoneError::new("strike distance overflow"))?;
            if along < length * min_cos {
                continue;
            }
            let (bottom, top) = (centre.y - half.y - feet, centre.y + half.y - feet);
            if top < band_low || bottom > band_high {
                continue;
            }
            if !self.line_of_sight(EntityRef::Player(player_id), entity)? {
                continue;
            }
            best = Some((distance, entity));
        }
        Ok(best.map(|(_, entity)| entity))
    }
}
