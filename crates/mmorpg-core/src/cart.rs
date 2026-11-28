//! A merchant's cart journey (#992, Atlas 5.1.49, 5.1.54, 5.4.15): a public incident that runs
//! on the zone's clock whether or not anyone watches.
//!
//! - **Journey**: every day at its authored hour the merchant leaves its post with its handcart
//!   ([`crate::PostureView::Hauling`]), walks its road to the destination, trades there a while
//!   and walks home. Its placement is a pure function of the tick and the outcome below, like a
//!   routine's (#158, #819) or a hunter's outing (#991), so recovery and replay place it exactly.
//!   A player's arrival starts nothing.
//! - **Day roll**: whether bandits wait on the road that day is a hash of the content seed and
//!   the day's number, on [`AMBUSH_DAYS`] of days, fixed before the day begins.
//! - **Ambush**: on such a day the zone stands the journey's reserved bandits, ordinary
//!   creatures with their ordinary combat, threat and leash, round the robbery site as the cart
//!   reaches it. The merchant halts there under threat for [`HOLD_TICKS`] (projected at
//!   [`THREATENED_HEALTH_PERCENT`]).
//! - **Outcome**: if every bandit dies during the hold, the cart is **saved**: the merchant
//!   travels on from the moment the last one fell, its whole day shifted by the delay. Otherwise
//!   it is **robbed**: the merchant abandons the cart and limps home at half pace
//!   ([`crate::PostureView::Hurt`], at [`ROBBED_HEALTH_PERCENT`]) to rest until dusk, while the
//!   bandits stay at the site over the spoils for [`LINGER_TICKS`] and then leave once out of
//!   combat. A quiet day is a **safe arrival**. The outcome follows from the tick and the
//!   bandits' canonical lives alone.
//! - **Rewards**: a character who fights the bandits during the hold (it has threat on one of
//!   them: damage, or healing a defender) earns [`REWARD_EXPERIENCE`] and [`REWARD_COPPER`] once
//!   per incident, whatever the outcome. Standing nearby earns nothing. The once-only grant is
//!   the raid state's canonical `cart_rewarded` list (the zone's shared incident state, #530)
//!   plus the durable [`crate::RewardKey::CartDefence`] receipt, so recovery and reconnects never
//!   pay twice.
//!
//! A suspended zone's tick freezes, and the journey with it (#144).

use std::sync::Arc;

use crate::creature::Life;
use crate::routine::{DUSK, NpcPlacement, along, legs, time_of_day, walk_ticks};
use crate::{CreatureId, EntityRef, NpcId, PlayerId, PostureView, ZoneError, ZoneSimulation};

/// One of two days the bandits wait on the road.
pub const AMBUSH_DAYS: [u64; 2] = [1, 2];
/// The merchant holds at the robbery site for 2 min before it gives the cart up.
pub const HOLD_TICKS: u32 = crate::long_ticks_from_ms(2 * 60 * 1_000);
/// Robbers stay over the spoils for 1 min after the merchant flees.
pub const LINGER_TICKS: u32 = crate::long_ticks_from_ms(60 * 1_000);
/// A merchant under threat's projected health.
pub const THREATENED_HEALTH_PERCENT: u8 = 60;
/// A robbed merchant's projected health.
pub const ROBBED_HEALTH_PERCENT: u8 = 35;
/// What a defender earns once per incident: the night raid's participation reward (#531).
pub const REWARD_EXPERIENCE: u32 = crate::raid::REWARD_EXPERIENCE;
pub const REWARD_COPPER: u32 = crate::raid::REWARD_COPPER;
/// Keeps the cart's roll independent of the raid's and the hunt's.
const AMBUSH_SALT: u64 = 0x2c41_b8e7_06d9_5f13;

/// An authored journey: at `leave` (ticks since midnight) `merchant` walks `path` from its post
/// to the destination at the path's end, trades there `trade` ticks facing `facing` and walks
/// back. `path[ambush]` is the robbery site, where an ambush day stands `bandits`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CartRun {
    pub merchant: NpcId,
    pub leave: u32,
    pub path: Vec<[i32; 2]>,
    pub ambush: usize,
    pub facing: u16,
    pub trade: u32,
    pub bandits: Vec<CreatureId>,
}

/// How a day's journey turned out.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CartOutcome {
    /// No bandits waited: the cart arrives safely.
    Safe,
    /// Bandits waited and every one of them fell during the hold, the last at this tick.
    Saved(u64),
    /// The bandits held the road: the merchant fled and the cart is lost.
    Robbed,
    /// The hold is still under way.
    Held,
}

/// Whether the day containing `tick` is an ambush day.
#[must_use]
pub const fn ambush_day(seed: u64, tick: u64) -> bool {
    crate::routine::day_roll(seed, tick, AMBUSH_SALT, AMBUSH_DAYS)
}

/// The incident a day's ambush is, for its receipts: the day's number, from 1.
#[must_use]
pub fn incident(tick: u64) -> u32 {
    u32::try_from(crate::routine::zone_day(tick) + 1).unwrap_or(u32::MAX)
}

impl CartRun {
    fn walk(&self, work: [i32; 2]) -> i64 {
        walk_ticks(legs(work, &self.path).iter().map(|leg| leg.2).sum())
    }

    fn to_ambush(&self, work: [i32; 2]) -> i64 {
        let legs = legs(work, &self.path);
        walk_ticks(legs.iter().take(self.ambush + 1).map(|leg| leg.2).sum())
    }

    /// When the cart of a post at `work` reaches the robbery site (ticks since midnight).
    #[must_use]
    pub fn arrival(&self, work: [i32; 2]) -> u32 {
        self.leave
            .saturating_add(u32::try_from(self.to_ambush(work)).unwrap_or(u32::MAX))
    }

    /// When a safe journey of a post at `work` is home again (ticks since midnight).
    #[must_use]
    pub fn end(&self, work: [i32; 2]) -> u32 {
        let span = 2 * self.walk(work) + i64::from(self.trade);
        self.leave
            .saturating_add(u32::try_from(span).unwrap_or(u32::MAX))
    }

    /// Where the merchant of a post at `work` facing `work_facing` is at `time`, its posture and
    /// projected health, while the journey runs; `outcome` is asked once the cart has reached
    /// the robbery site.
    pub fn placement(
        &self,
        work: [i32; 2],
        work_facing: u16,
        time: u32,
        outcome: impl FnOnce() -> CartOutcome,
    ) -> Option<(NpcPlacement, PostureView, u8)> {
        let legs = legs(work, &self.path);
        let walk = self.walk(work);
        let to_ambush = self.to_ambush(work);
        let speed = crate::NPC_WALK_UNITS_PER_TICK;
        let since = i64::from(time.checked_sub(self.leave)?);
        let hauling = |since: i64| {
            if since < walk {
                return Some(along(&legs, since * speed, true));
            }
            let since = since - walk;
            if since < i64::from(self.trade) {
                return Some(NpcPlacement {
                    position: legs.last().map_or(work, |leg| leg.1),
                    facing: self.facing,
                    velocity: [0, 0],
                });
            }
            let since = since - i64::from(self.trade);
            (since < walk).then(|| along(&legs, since * speed, false))
        };
        let haul = |since| hauling(since).map(|placement| (placement, PostureView::Hauling, 100));
        if since < to_ambush {
            return haul(since);
        }
        let held = since - to_ambush;
        match outcome() {
            CartOutcome::Safe => haul(since),
            // The merchant travels on from the tick the last bandit fell.
            CartOutcome::Saved(delay) => {
                haul(since - i64::try_from(delay).unwrap_or(i64::MAX).min(held))
            }
            CartOutcome::Held => {
                // Exactly at the site, facing up the road.
                let site: i64 = legs.iter().take(self.ambush + 1).map(|leg| leg.2).sum();
                let mut halt = along(&legs, site, true);
                halt.velocity = [0, 0];
                Some((halt, PostureView::Hauling, THREATENED_HEALTH_PERCENT))
            }
            CartOutcome::Robbed => {
                let back = held - i64::from(HOLD_TICKS);
                let out = &legs[..=self.ambush.min(legs.len().saturating_sub(1))];
                // Limping, it covers half the ground a tick.
                if back < 2 * to_ambush {
                    let limp = crate::routine::limp_along(out, back);
                    return Some((limp, PostureView::Hurt, ROBBED_HEALTH_PERCENT));
                }
                let home = NpcPlacement {
                    position: work,
                    facing: work_facing,
                    velocity: [0, 0],
                };
                (time < DUSK).then_some((home, PostureView::Hurt, ROBBED_HEALTH_PERCENT))
            }
        }
    }

    pub(crate) fn validate(&self, content: &crate::ZoneContent) -> Result<(), ZoneError> {
        let work = crate::routine::outing_post(
            content,
            self.merchant,
            self.leave,
            &self.path,
            "cart run",
        )?;
        // The longest day, a saved cart whose last bandit fell as the hold ended, is home before
        // dusk, and so is a robbed merchant's limp.
        let mut bandits = self.bandits.clone();
        bandits.sort_unstable();
        bandits.dedup();
        if self.path.is_empty()
            || self.ambush + 1 >= self.path.len()
            || self.end(work).saturating_add(HOLD_TICKS) >= DUSK
            || self.bandits.is_empty()
            || bandits.len() != self.bandits.len()
            || self.bandits.iter().any(|&bandit| {
                content.creature_spawn(bandit).is_none()
                    || content.is_raider(bandit)
                    || content.is_depths_creature(bandit)
                    || content.is_hunt_beast(bandit)
            })
        {
            return Err(ZoneError::new("invalid cart run"));
        }
        Ok(())
    }
}

impl ZoneSimulation {
    /// The oldest cart incident that can still pay (#1128): a day's ambush pays defenders only
    /// during its hold, so from the coming tick on the day's own incident still can until the
    /// hold has ended, and a quiet day's never does. A zone without a cart pays none, so its
    /// floor is the coming tick's day plus one.
    pub(crate) fn cart_floor(&self) -> u32 {
        let next = self.tick.saturating_add(1);
        let day = incident(next);
        let holding = ambush_day(self.rng_seed(), next)
            && self.content.cart().is_some_and(|run| {
                self.content.npc(run.merchant).is_some_and(|merchant| {
                    time_of_day(next) < run.arrival(merchant.position).saturating_add(HOLD_TICKS)
                })
            });
        if holding { day } else { day.saturating_add(1) }
    }

    /// How the journey that reached the robbery site by `now` is going.
    fn cart_outcome(&self, run: &CartRun, work: [i32; 2], now: u64) -> CartOutcome {
        if !ambush_day(self.rng_seed(), now) {
            return CartOutcome::Safe;
        }
        let Some(start) = crate::routine::incident_start(now, run.arrival(work)) else {
            return CartOutcome::Held;
        };
        let hold = start..start + u64::from(HOLD_TICKS);
        let fell = run.bandits.iter().map(|bandit| {
            self.creatures
                .get(bandit)
                .and_then(|state| state.died_within(&hold))
        });
        match fell.collect::<Option<Vec<_>>>() {
            Some(deaths) => CartOutcome::Saved(deaths.into_iter().max().unwrap_or(start) - start),
            None if now < hold.end => CartOutcome::Held,
            None => CartOutcome::Robbed,
        }
    }

    /// Where the journey puts `npc` now, its posture and projected health, if it is a merchant
    /// on its journey.
    pub(crate) fn cart_placement(&self, npc: NpcId) -> Option<(NpcPlacement, PostureView, u8)> {
        let run = self.content.cart().filter(|run| run.merchant == npc)?;
        let definition = self.content.npc(npc)?;
        let time = time_of_day(self.tick);
        // A journey that left before the zone's first tick never ran.
        if self.tick < u64::from(time.checked_sub(run.leave)?) {
            return None;
        }
        run.placement(definition.position, definition.facing, time, || {
            self.cart_outcome(run, definition.position, self.tick)
        })
    }

    /// Stands the bandits when the cart reaches the robbery site on an ambush day, rewards the
    /// players fighting them during the hold, and lets bandits still standing after the spoils
    /// leave once out of combat.
    pub(crate) fn advance_cart(&mut self, now: u64) -> Result<(), ZoneError> {
        let content = Arc::clone(&self.content);
        let Some(run) = content.cart() else {
            return Ok(());
        };
        let Some(merchant) = content.npc(run.merchant) else {
            return Ok(());
        };
        let time = time_of_day(now);
        let arrival = run.arrival(merchant.position);
        let ambush = ambush_day(self.rng_seed(), now);
        let holding = ambush && (arrival..arrival.saturating_add(HOLD_TICKS)).contains(&time);
        let standing =
            ambush && (arrival..arrival.saturating_add(HOLD_TICKS + LINGER_TICKS)).contains(&time);
        if ambush && time == arrival {
            self.raid.cart_rewarded.clear();
            for &bandit in &run.bandits {
                if self
                    .creatures
                    .get(&bandit)
                    .is_some_and(|state| matches!(state.life, Life::Despawned { .. }))
                {
                    self.spawn_creature(bandit)?;
                }
            }
        }
        let mut defenders = Vec::new();
        for &bandit in &run.bandits {
            let Some(state) = self.creatures.get(&bandit) else {
                continue;
            };
            if !state.is_alive() {
                // A bandit felled this tick took its threat table with it: its tapper fought it.
                if holding
                    && matches!(state.life, Life::Corpse { died_at, .. } if died_at == now)
                    && let Some(player) = state.tapped_by
                {
                    defenders.push(player);
                }
                continue;
            }
            if holding {
                defenders.extend(state.threat.iter().filter_map(|entry| match entry.entity {
                    EntityRef::Player(player) if entry.threat > 0 => Some(player),
                    _ => None,
                }));
            } else if !standing && state.out_of_fight() {
                self.retire_raider(bandit, now)?;
            }
        }
        defenders.sort_unstable();
        defenders.dedup();
        for player in defenders {
            self.reward_cart_defender(player, incident(now))?;
        }
        Ok(())
    }

    /// Grants the defence reward for `incident` to `player_id` unless it already has it: once
    /// per incident and character, in the zone and in the durable receipts.
    fn reward_cart_defender(
        &mut self,
        player_id: PlayerId,
        incident: u32,
    ) -> Result<(), ZoneError> {
        let payout = crate::rewards::RewardPayout {
            experience: REWARD_EXPERIENCE,
            copper: REWARD_COPPER,
        };
        self.pay_incident_once(
            |zone| &mut zone.raid.cart_rewarded,
            player_id,
            crate::RewardKey::CartDefence { incident },
            payout,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{CartOutcome, CartRun, HOLD_TICKS, ROBBED_HEALTH_PERCENT};
    use crate::routine::{DUSK, HOUR_TICKS, walk_ticks};
    use crate::{CreatureId, NpcId, PostureView};

    #[test]
    fn a_journey_halts_at_the_site_and_ends_by_its_outcome() {
        let run = CartRun {
            merchant: NpcId::new(1),
            leave: 13 * HOUR_TICKS,
            path: vec![[1_200, 0], [2_400, 0]],
            ambush: 0,
            facing: 16_384,
            trade: HOUR_TICKS,
            bandits: vec![CreatureId::new(1)],
        };
        let half = u32::try_from(walk_ticks(1_200)).unwrap();
        let arrival = run.arrival([0, 0]);
        assert_eq!(arrival, run.leave + half);
        let at = |time: u32, outcome| run.placement([0, 0], 0, time, || outcome);
        assert_eq!(at(run.leave - 1, CartOutcome::Safe), None);
        // Hauling out; on a quiet day it passes the site without a halt.
        let out = at(run.leave + half / 2, CartOutcome::Held).unwrap();
        assert_eq!((out.1, out.2), (PostureView::Hauling, 100));
        assert!(out.0.velocity[0] > 0);
        assert!(at(arrival + 5, CartOutcome::Safe).unwrap().0.position[0] > 1_200);
        // Held under threat at the site.
        let held = at(arrival + 5, CartOutcome::Held).unwrap();
        assert_eq!((held.0.position, held.0.velocity), ([1_200, 0], [0, 0]));
        assert_eq!(held.2, super::THREATENED_HEALTH_PERCENT);
        // Saved after 10 ticks: it travels on 10 ticks behind a safe day.
        assert_eq!(
            at(arrival + 30, CartOutcome::Saved(10)),
            at(arrival + 20, CartOutcome::Safe)
        );
        // Robbed: it limps back without the cart and rests until dusk.
        let limp = at(arrival + HOLD_TICKS + 4, CartOutcome::Robbed).unwrap();
        assert_eq!((limp.1, limp.2), (PostureView::Hurt, ROBBED_HEALTH_PERCENT));
        assert!(limp.0.position[0] < 1_200 && limp.0.velocity[0] < 0);
        let home = at(DUSK - 1, CartOutcome::Robbed).unwrap();
        assert_eq!((home.0.position, home.1), ([0, 0], PostureView::Hurt));
        assert_eq!(at(DUSK, CartOutcome::Robbed), None);
        assert_eq!(at(run.end([0, 0]), CartOutcome::Safe), None);
    }
}
