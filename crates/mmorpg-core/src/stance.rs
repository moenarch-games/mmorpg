//! Movement-dependent Warden melee (#155, Atlas 5.2.28, 5.2.69–5.2.70).
//!
//! When a Warden melee action resolves, the zone reads the Warden's
//! movement context from its physics body and its landing state:
//!
//! | Stance | When | Damage dealt | Creature melee taken |
//! | --- | --- | --- | --- |
//! | Planted | grounded, standing still | 100% | 90% |
//! | Charging | grounded, running at the target | 120% | 100% |
//! | Moving | grounded, any other movement | 85% | 100% |
//! | Airborne | not grounded | 135% | 125% |
//! | Landing | the recovery window after touching down | — | 100% |
//!
//! Landing is recovery: auto-attack swings and queued melee abilities wait
//! and resolve when the window ends (Atlas 5.2.70), so no input fails. The
//! window lasts [`LANDING_RECOVERY_TICKS`], or [`OVERHEAD_LANDING_TICKS`]
//! after a melee action made while airborne. The melee actions are
//! auto-attack, Heroic Strike (ordinary and directed), Shield Bash and
//! Cleave; other classes and other abilities are unaffected. The values are
//! the owner's modest starting point, to be tuned after playtesting.

use crate::physics::RigidBody;

use crate::entity::body_id;
use crate::zone::{PlayerState, is_grounded};
use crate::{EntityRef, PlayerClass, PlayerId, ZoneError, ZoneSimulation, action};

/// Recovery after an ordinary landing: the action graph's `Landing`
/// recovery, 0.3 s.
pub const LANDING_RECOVERY_TICKS: u16 = action::recovery_ticks(action::ActionId::Landing);
/// Recovery after landing from a melee action made in the air: the graph's
/// `AttackedAirborne` landing modifier, 0.5 s.
pub const OVERHEAD_LANDING_TICKS: u16 = action::modified_recovery_ticks(
    action::ActionId::Landing,
    action::Condition::AttackedAirborne,
);
/// Horizontal speed (subunits per tick) from which a run at the target charges: about three
/// quarters of [`crate::RUN_SPEED_UNITS_PER_SECOND`] (4.5 m/s).
const CHARGE_SPEED_SUBUNITS_PER_TICK: i64 = crate::subunits_per_tick(450) as i64;
/// Below this horizontal speed (0.6 m/s, subunits per tick) a grounded Warden is planted.
const PLANTED_SPEED_SUBUNITS_PER_TICK: i64 = crate::subunits_per_tick(60) as i64;
/// A charge heads within acos(0.7) ≈ 45° of the target: cos² ≥ 49/100.
const CHARGE_COS_SQUARED: [i64; 2] = [49, 100];

/// Airborne and landing state of a player, as canonical snapshots carry it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LandingState {
    /// Not grounded after the last physics step.
    pub airborne: bool,
    /// Ticks of landing recovery left.
    pub recovery: u16,
    /// A melee action was made in the current airborne phase, or in the one
    /// the current landing recovery ends.
    pub attacked_airborne: bool,
}

impl LandingState {
    /// Consistent: recovery only on the ground, within the longest window,
    /// and an airborne attack only while airborne or during the lengthened
    /// recovery of the landing that followed it.
    #[must_use]
    pub const fn is_valid(self) -> bool {
        self.recovery <= OVERHEAD_LANDING_TICKS
            && !(self.airborne && self.recovery > 0)
            && (self.airborne || !self.attacked_airborne || self.recovery > 0)
    }
}

/// A Warden's movement context for melee.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MeleeStance {
    Planted,
    Charging,
    Moving,
    Airborne,
    Landing,
}

impl MeleeStance {
    /// Damage dealt by a melee action in this stance, in percent.
    #[must_use]
    pub const fn damage_percent(self) -> u32 {
        match self {
            Self::Planted | Self::Landing => 100,
            Self::Charging => 120,
            Self::Moving => 85,
            Self::Airborne => 135,
        }
    }

    /// Creature melee damage taken in this stance, in percent.
    #[must_use]
    pub const fn taken_percent(self) -> u32 {
        match self {
            Self::Planted => 90,
            Self::Charging | Self::Moving | Self::Landing => 100,
            Self::Airborne => 125,
        }
    }

    /// Wire code; 0 means no stance.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Planted => 1,
            Self::Charging => 2,
            Self::Moving => 3,
            Self::Airborne => 4,
            Self::Landing => 5,
        }
    }

    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Planted),
            2 => Some(Self::Charging),
            3 => Some(Self::Moving),
            4 => Some(Self::Airborne),
            5 => Some(Self::Landing),
            _ => None,
        }
    }
}

/// Scales `amount` by `percent`, rounding down, keeping a hit at least 1.
pub(crate) fn scale(amount: u32, percent: u32) -> u32 {
    if amount == 0 {
        return 0;
    }
    (u64::from(amount) * u64::from(percent) / 100)
        .clamp(1, u64::from(u32::MAX))
        .try_into()
        .unwrap_or(u32::MAX)
}

fn is_warden(state: &PlayerState) -> bool {
    state
        .class
        .is_some_and(|choice| choice.class == PlayerClass::Warden)
}

impl ZoneSimulation {
    /// The living Warden's melee stance toward `target`; `None` for other
    /// players.
    pub(crate) fn melee_stance(
        &self,
        player_id: PlayerId,
        target: Option<EntityRef>,
    ) -> Result<Option<MeleeStance>, ZoneError> {
        let Some(state) = self.players.get(&player_id) else {
            return Ok(None);
        };
        if !is_warden(state) || !state.is_alive() {
            return Ok(None);
        }
        if state.landing.airborne {
            return Ok(Some(MeleeStance::Airborne));
        }
        if state.landing.recovery > 0 {
            return Ok(Some(MeleeStance::Landing));
        }
        let body = self.player_body(player_id)?;
        let velocity = body.velocity();
        let (vx, vz) = (i64::from(velocity.x), i64::from(velocity.z));
        let speed_squared = vx * vx + vz * vz;
        if speed_squared <= PLANTED_SPEED_SUBUNITS_PER_TICK.pow(2) {
            return Ok(Some(MeleeStance::Planted));
        }
        let toward = match target {
            Some(target) if speed_squared >= CHARGE_SPEED_SUBUNITS_PER_TICK.pow(2) => {
                let from = body.position();
                let to = self
                    .world
                    .body(body_id(target))
                    .map(RigidBody::position)
                    .unwrap_or(from);
                let (dx, dz) = (i64::from(to.x - from.x), i64::from(to.z - from.z));
                let along = vx * dx + vz * dz;
                let [numerator, denominator] = CHARGE_COS_SQUARED;
                along > 0
                    && along * along * denominator
                        >= numerator * speed_squared * (dx * dx + dz * dz)
            }
            _ => false,
        };
        Ok(Some(if toward {
            MeleeStance::Charging
        } else {
            MeleeStance::Moving
        }))
    }

    /// Scales melee damage dealt by the Warden's stance toward `target` and
    /// remembers an airborne action for the landing that follows.
    pub(crate) fn stance_damage(
        &mut self,
        player_id: PlayerId,
        target: Option<EntityRef>,
        amount: u32,
    ) -> Result<u32, ZoneError> {
        let Some(stance) = self.melee_stance(player_id, target)? else {
            return Ok(amount);
        };
        if stance == MeleeStance::Airborne {
            self.update_player(player_id, |player| player.landing.attacked_airborne = true);
        }
        Ok(scale(amount, stance.damage_percent()))
    }

    /// Scales creature melee damage taken by the Warden's stance.
    pub(crate) fn stance_exposure(
        &self,
        player_id: PlayerId,
        amount: u32,
    ) -> Result<u32, ZoneError> {
        Ok(match self.melee_stance(player_id, None)? {
            Some(stance) => scale(amount, stance.taken_percent()),
            None => amount,
        })
    }

    /// After the physics step: track every player's airborne state and
    /// landing recovery, the action graph's movement region (#413). Only
    /// Wardens' melee waits the recovery out; other classes' landings hold
    /// nothing unless an experimental graph authors it.
    pub(crate) fn update_landings(&mut self) -> Result<(), ZoneError> {
        let ids: Vec<_> = self.players.keys().copied().collect();
        for player_id in ids {
            let grounded = is_grounded(&self.world, self.player_body(player_id)?)?;
            self.update_player(player_id, |player| {
                let state = &mut player.landing;
                // A new airborne phase forgets the last one's attack.
                if !grounded && !state.airborne {
                    state.attacked_airborne = false;
                }
                state.recovery = if grounded && state.airborne {
                    if state.attacked_airborne {
                        OVERHEAD_LANDING_TICKS
                    } else {
                        LANDING_RECOVERY_TICKS
                    }
                } else if grounded {
                    state.recovery.saturating_sub(1)
                } else {
                    0
                };
                state.airborne = !grounded;
                // The airborne attack is remembered through the landing it
                // lengthened, for the edges out of that landing.
                if grounded && state.recovery == 0 {
                    state.attacked_airborne = false;
                }
            });
        }
        self.track_swimming()
    }

    /// Whether each player swims where the last physics step left it: the
    /// movement region's swim (#413). Derived state, so a restored zone
    /// calls it too.
    pub(crate) fn track_swimming(&mut self) -> Result<(), ZoneError> {
        let ids: Vec<_> = self.players.keys().copied().collect();
        for player_id in ids {
            let centre = self.player_body(player_id)?.position();
            let swims = self.player_water_mode(centre).swims();
            if let Some(player) = self.players.get_mut(&player_id) {
                player.swimming = swims;
            }
        }
        Ok(())
    }

    fn player_body(&self, player_id: PlayerId) -> Result<&RigidBody, ZoneError> {
        self.world
            .body(body_id(EntityRef::Player(player_id)))
            .ok_or_else(|| ZoneError::new("player physics body is missing"))
    }
}
