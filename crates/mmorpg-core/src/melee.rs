//! The basic attack as movement-state strikes (#885, Atlas 5.2.i885).
//!
//! A pressed swing (#517) becomes one of [`StrikeVariant`]'s strikes, picked from the
//! striker's movement when it begins: airborne the jumping strike, running forward on the
//! ground the running strike, otherwise the next strike of the standing chain. The weapon's
//! [`WeaponFamily`] row ([`FamilyMelee`]) times, shapes and weights it. Every class strikes
//! this way; the Warden's stance (#155) still scales its damage on top.
//!
//! A strike runs on the same swing path as before, with the swing timer as its recovery:
//!
//! - **Wind-up.** The strike begins when the swing timer allows: observers see
//!   [`ZoneEvent::StrikeBegan`], and the swing timer runs for the strike's interval. A running
//!   strike's lunge carries the body forward through its wind-up ([`Self::lunge_velocity`]).
//! - **Contact.** On its contact tick the strike resolves toward the striker's facing at that
//!   moment, through the ordinary hit table: every attackable unit in the profile's reach, arc
//!   and height band, as an ordinary swing always did. A jumping strike lands on touchdown,
//!   never before its wind-up, in a small area ahead, and its landing recovery follows.
//! - **Chain.** A chain strike opens [`CHAIN_WINDOW_TICKS`] after its recovery in which the
//!   next press continues the chain; the finisher, a running and a jumping strike start it
//!   over.
//! - **Timing rewards (#928).** A jumping strike whose touchdown meets the end of its wind-up
//!   within [`LANDING_STRIKE_TOLERANCE_TICKS`] is a landing strike: its landing recovery is
//!   halved. A first or second chain strike that connects opens [`FOLLOW_UP_WINDOW_TICKS`] at
//!   its contact in which a fresh press begins the next chain strike at once, cutting the rest
//!   of the recovery; a press already waiting at the contact keeps the ordinary chain. The
//!   striker sees [`ZoneEvent::StrikeRewarded`] for either.
//!
//! The chain, its windows and a pending strike are canonical player state, so replay and
//! recovery continue mid-chain exactly. Outcomes stay health only: no knockback or hitstun.

use crate::physics::RigidBody;
use crate::zone::PlayerState;
use crate::{
    CHAIN_WINDOW_TICKS, EntityRef, FOLLOW_UP_WINDOW_TICKS, FamilyMelee,
    LANDING_STRIKE_RECOVERY_PERCENT, LANDING_STRIKE_TOLERANCE_TICKS, MAX_INTERVAL_TICKS,
    MeleeStance, PlayerClass, PlayerId, StrikeProfile, StrikeReward, StrikeVariant, WeaponFamily,
    ZoneError, ZoneEvent, ZoneSimulation,
};
use sim_combat::melee::scaled_damage;

/// Horizontal speed (subunits per tick) from which a forward-moving striker runs: three
/// quarters of [`crate::RUN_SPEED_UNITS_PER_SECOND`] (4.5 m/s), as the Warden's charge (#155).
const RUNNING_SPEED_SUBUNITS_PER_TICK: i64 = crate::subunits_per_tick(450) as i64;
/// A run heads within acos(0.7) ≈ 45° of the facing: cos² ≥ 49/100.
const RUNNING_COS_SQUARED: [i64; 2] = [49, 100];
/// A lunge stops once an attackable unit stands this close (XZ centre distance) in its arc.
const LUNGE_STOP_UNITS: i32 = 150;
/// A jumping strike that has not touched down this long (1.5 s) after its press lands in the air,
/// so a long fall never holds the weapon.
pub const JUMP_STRIKE_AIR_TICKS: u16 = crate::ticks_from_ms(1_500);

/// A strike between its press and its contact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PendingStrike {
    pub family: WeaponFamily,
    pub variant: StrikeVariant,
    /// Ticks of wind-up left. A jumping strike counts down [`JUMP_STRIKE_AIR_TICKS`] instead:
    /// it lands on touchdown once its wind-up has passed, and in the air when they run out.
    pub contact: u16,
    /// The Warden's stance when the strike began (#155), which scales its damage.
    pub stance: Option<MeleeStance>,
    /// Ticks a jumping strike has stood on the ground since its last touchdown (saturating),
    /// so its landing can be timed against its wind-up (#928); `0` while airborne.
    pub grounded: u8,
}

impl PendingStrike {
    /// The countdown the strike begins with: its wind-up, or a jumping strike's longest flight.
    #[must_use]
    pub const fn countdown(family: WeaponFamily, variant: StrikeVariant) -> u16 {
        match variant {
            StrikeVariant::Jumping => JUMP_STRIKE_AIR_TICKS,
            _ => FamilyMelee::of(family).strike(variant).contact_ticks,
        }
    }

    /// Whether the strike lands this tick, its countdown already advanced: on contact, or a
    /// jumping strike on the ground after its wind-up.
    const fn lands(self, airborne: bool) -> bool {
        if self.contact == 0 {
            return true;
        }
        let StrikeVariant::Jumping = self.variant else {
            return false;
        };
        let flown = JUMP_STRIKE_AIR_TICKS - self.contact;
        !airborne && flown >= FamilyMelee::of(self.family).jumping.contact_ticks
    }

    /// Whether a jumping strike landing now is a timed landing strike (#928): it touched down
    /// within [`LANDING_STRIKE_TOLERANCE_TICKS`] either side of the end of its wind-up. A
    /// touchdown before it waited `grounded - 1` ticks; one after it is late by the flight
    /// beyond the wind-up. A strike that ran out of air never times.
    const fn timed_landing(self) -> bool {
        if self.grounded == 0 {
            return false;
        }
        let flown = JUMP_STRIKE_AIR_TICKS - self.contact;
        let late = flown.saturating_sub(FamilyMelee::of(self.family).jumping.contact_ticks);
        (self.grounded as u16) <= LANDING_STRIKE_TOLERANCE_TICKS + 1
            && late <= LANDING_STRIKE_TOLERANCE_TICKS
    }
}

/// A player's basic-strike state, as canonical snapshots carry it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MeleeState {
    /// The chain strike a standing press makes next, `0` for the first.
    pub chain: u8,
    /// Ticks left in which a standing press continues the chain; at zero it starts over.
    pub window: u16,
    /// Ticks left in which a fresh press follows up a standing hit at once (#928).
    pub follow_up: u16,
    pub pending: Option<PendingStrike>,
}

impl MeleeState {
    /// Within range: a known chain strike, a window no longer than the longest interval and
    /// its window, a follow-up window only within a running chain and between strikes, and a
    /// pending strike no further from its contact than its own wind-up.
    #[must_use]
    pub const fn is_valid(self) -> bool {
        if self.follow_up > FOLLOW_UP_WINDOW_TICKS
            || (self.follow_up > 0 && (self.chain == 0 || self.pending.is_some()))
        {
            return false;
        }
        let pending_ok = match self.pending {
            None => true,
            Some(pending) => {
                pending.contact <= PendingStrike::countdown(pending.family, pending.variant)
            }
        };
        self.chain < crate::CHAIN_LENGTH
            && self.window <= MAX_INTERVAL_TICKS + CHAIN_WINDOW_TICKS
            && (self.window > 0 || self.chain == 0)
            && pending_ok
    }
}

/// The melee family a player strikes with: the main-hand weapon's, else the class's own
/// weapon (the Warden's sword, the Ranger's bow, the Rogue's daggers, the Cleric's mace,
/// the other casters' staves), else bare hands.
/// A strike of `family` as the main-hand weapon shapes it: its find's trait (#918) while the
/// weapon is of that family.
fn strike_profile(
    state: &PlayerState,
    family: WeaponFamily,
    variant: StrikeVariant,
) -> StrikeProfile {
    let main_hand = state.equipment.item(crate::EquipmentSlot::MainHand);
    crate::shaped_strike(family, variant, main_hand)
}

pub(crate) fn weapon_family(state: &PlayerState) -> WeaponFamily {
    let weapon = state.equipment.slots()[usize::from(crate::EquipmentSlot::MainHand.index())]
        .and_then(crate::item_template)
        .and_then(|template| template.weapon);
    weapon.unwrap_or(match state.class.map(|choice| choice.class) {
        None => WeaponFamily::Unarmed,
        Some(PlayerClass::Warden) => WeaponFamily::Sword,
        Some(PlayerClass::Ranger) => WeaponFamily::Bow,
        Some(PlayerClass::Rogue) => WeaponFamily::Dagger,
        Some(PlayerClass::Cleric) => WeaponFamily::Mace,
        Some(PlayerClass::Arcanist | PlayerClass::Necromancer | PlayerClass::Druid) => {
            WeaponFamily::Staff
        }
    })
}

impl ZoneSimulation {
    /// Whether the grounded player runs forward: forward intent held, and the body moving at
    /// least at [`RUNNING_SPEED_SUBUNITS_PER_TICK`] within 45° of its facing.
    fn runs_forward(&self, player_id: PlayerId, state: &PlayerState) -> Result<bool, ZoneError> {
        if state.forward.get() <= 0 {
            return Ok(false);
        }
        let velocity = self.player_body(player_id).map(RigidBody::velocity)?;
        let (vx, vz) = (i64::from(velocity.x), i64::from(velocity.z));
        let speed_squared = vx * vx + vz * vz;
        if speed_squared < RUNNING_SPEED_SUBUNITS_PER_TICK.pow(2) {
            return Ok(false);
        }
        let (dx, dz) = crate::trig::direction(state.facing);
        let along = i128::from(vx * i64::from(dx) + vz * i64::from(dz));
        let one = i128::from(crate::trig::TRIG_ONE);
        let [numerator, denominator] = RUNNING_COS_SQUARED.map(i128::from);
        Ok(along > 0
            && along * along * denominator >= numerator * i128::from(speed_squared) * one * one)
    }

    /// Begins the pressed strike: picks it from the movement and the chain, starts the swing
    /// timer and the wind-up, and shows it to everyone watching.
    pub(crate) fn begin_strike(&mut self, player_id: PlayerId) -> Result<(), ZoneError> {
        let Some(state) = self.players.get(&player_id) else {
            return Ok(());
        };
        let airborne = state.landing.airborne && !state.swimming;
        let running = !airborne && self.runs_forward(player_id, state)?;
        let chain = if state.melee.window > 0 {
            state.melee.chain
        } else {
            0
        };
        let variant = StrikeVariant::select(airborne, running, chain);
        let family = weapon_family(state);
        let profile = strike_profile(state, family, variant);
        let interval = crate::aura::hasted_swing(profile.interval_ticks, &state.auras);
        let target = state.target;
        // The Warden's stance is read as the strike begins: a jump strike lands as an
        // airborne blow, and lengthens its landing (#155).
        let stance = self.melee_stance(player_id, target)?;
        self.update_player(player_id, |player| {
            if stance == Some(MeleeStance::Airborne) {
                player.landing.attacked_airborne = true;
            }
            player.swing_queued = false;
            // A jumping strike's recovery begins when it lands.
            player.swing_timer = if variant == StrikeVariant::Jumping {
                0
            } else {
                interval
            };
            player.melee = MeleeState {
                chain: variant.next_chain(),
                window: if variant.is_chain() && variant.next_chain() > 0 {
                    interval.saturating_add(CHAIN_WINDOW_TICKS)
                } else {
                    0
                },
                follow_up: 0,
                pending: Some(PendingStrike {
                    family,
                    variant,
                    contact: PendingStrike::countdown(family, variant),
                    stance,
                    grounded: 0,
                }),
            };
        });
        let source = EntityRef::Player(player_id);
        self.notify_observers(
            &[source],
            &std::collections::BTreeSet::new(),
            ZoneEvent::StrikeBegan {
                source,
                family,
                variant,
                contact: profile.contact_ticks,
            },
        );
        Ok(())
    }

    /// Advances the player's chain window and pending strike by one tick; `true` while a
    /// strike is still winding up, so no other may begin. Death drops it, as a stun
    /// interrupts it.
    pub(crate) fn advance_strike(
        &mut self,
        player_id: PlayerId,
        now: u64,
    ) -> Result<bool, ZoneError> {
        let Some(state) = self.players.get_mut(&player_id) else {
            return Ok(false);
        };
        let interrupted =
            !state.is_alive() || crate::aura::has_kind(&state.auras, crate::AuraKind::Stun);
        let airborne = state.landing.airborne;
        let melee = &mut state.melee;
        melee.window = melee.window.saturating_sub(1);
        melee.follow_up = melee.follow_up.saturating_sub(1);
        if melee.window == 0 {
            melee.chain = 0;
            melee.follow_up = 0;
        }
        let Some(mut pending) = melee.pending else {
            return Ok(false);
        };
        if interrupted {
            melee.pending = None;
            return Ok(false);
        }
        pending.contact = pending.contact.saturating_sub(1);
        pending.grounded = if airborne {
            0
        } else {
            pending.grounded.saturating_add(1)
        };
        if !pending.lands(airborne) {
            melee.pending = Some(pending);
            return Ok(true);
        }
        melee.pending = None;
        let mut landing_strike = None;
        if pending.variant == StrikeVariant::Jumping {
            let mut recovery =
                strike_profile(state, pending.family, StrikeVariant::Jumping).interval_ticks;
            // A timed landing strike halves its landing recovery (#928).
            if pending.timed_landing() {
                recovery =
                    (u32::from(recovery) * u32::from(LANDING_STRIKE_RECOVERY_PERCENT) / 100) as u16;
            }
            state.swing_timer = crate::aura::hasted_swing(recovery, &state.auras);
            landing_strike = pending.timed_landing().then_some(state.swing_timer);
        }
        self.strike_contact(player_id, pending, now)?;
        if let Some(ticks) = landing_strike {
            self.notify(
                player_id,
                ZoneEvent::StrikeRewarded {
                    source: EntityRef::Player(player_id),
                    reward: StrikeReward::LandingStrike,
                    ticks,
                },
            );
        }
        Ok(false)
    }

    /// The strike's contact: everything its profile covers toward the striker's facing takes
    /// the ordinary hit, scaled by the strike and the Warden's stance; a whiff still swings.
    fn strike_contact(
        &mut self,
        player_id: PlayerId,
        pending: PendingStrike,
        now: u64,
    ) -> Result<(), ZoneError> {
        let Some(state) = self.players.get(&player_id) else {
            return Ok(());
        };
        let row = FamilyMelee::of(pending.family);
        let profile = strike_profile(state, pending.family, pending.variant);
        let (facing, weapon) = (state.facing, state.melee_damage());
        let damage = scaled_damage(weapon, profile.damage_percent);
        let victims = self.swing_coverage(
            player_id,
            facing,
            profile.reach_units,
            profile.half_width,
            row.band,
        )?;
        if !victims.is_empty() {
            // A first or second chain strike that connects opens the follow-up (#928), unless
            // a press already waits: that one continues the chain after the recovery as before.
            let follow_up = pending.variant.is_chain()
                && pending.variant.next_chain() > 0
                && state.melee.chain > 0
                && !state.swing_queued;
            self.update_player(player_id, |player| {
                player.combat_timer = crate::unit::COMBAT_LINGER_TICKS;
                if follow_up {
                    player.melee.follow_up = FOLLOW_UP_WINDOW_TICKS;
                }
            });
            if follow_up {
                self.notify(
                    player_id,
                    ZoneEvent::StrikeRewarded {
                        source: EntityRef::Player(player_id),
                        reward: StrikeReward::FollowUp,
                        ticks: FOLLOW_UP_WINDOW_TICKS,
                    },
                );
            }
        }
        if victims.is_empty() {
            let source = EntityRef::Player(player_id);
            self.notify_observers(
                &[source],
                &std::collections::BTreeSet::new(),
                ZoneEvent::AttackPerformed {
                    source,
                    target: None,
                },
            );
            // A strike that covered no unit may still strike summoned terrain (#293).
            return self.swing_at_summoned(player_id, facing);
        }
        for target in victims {
            let before = self.struck_health(target);
            self.attack_unit(player_id, target, damage, pending.stance, now)?;
            // A landed sword blow banks practice against a real foe (#915).
            self.practise(
                player_id,
                pending.family,
                target,
                before,
                crate::BLOW_PRACTICE,
            );
        }
        Ok(())
    }

    /// The running strike's lunge for the movement step (subunits per tick, XZ): its distance
    /// over its wind-up along the facing, and none once an attackable unit stands within
    /// [`LUNGE_STOP_UNITS`] in its arc, so the lunge closes the gap instead of running past.
    /// `None` while no lunge runs.
    pub(crate) fn lunge_velocity(
        &self,
        player_id: PlayerId,
        state: &PlayerState,
    ) -> Result<Option<[i32; 2]>, ZoneError> {
        let Some(pending) = state
            .melee
            .pending
            .filter(|pending| pending.variant == StrikeVariant::Running && pending.contact > 0)
        else {
            return Ok(None);
        };
        let row = FamilyMelee::of(pending.family);
        let arrived = !self
            .swing_coverage(
                player_id,
                state.facing,
                LUNGE_STOP_UNITS,
                row.running.half_width,
                row.band,
            )?
            .is_empty();
        if arrived {
            return Ok(Some([0, 0]));
        }
        let speed = row.lunge_units * crate::physics::SUBUNITS_PER_UNIT
            / i32::from(row.running.contact_ticks);
        let (dx, dz) = crate::trig::direction(state.facing);
        Ok(Some([
            crate::trig::checked_scale(speed, dx).unwrap_or(0),
            crate::trig::checked_scale(speed, dz).unwrap_or(0),
        ]))
    }
}
