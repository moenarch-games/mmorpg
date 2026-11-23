//! A hunter's outing (#991, Atlas 5.1.49, 5.1.54, 5.4.15): a living-world encounter that runs
//! on the zone's clock whether or not anyone watches.
//!
//! - **Outing**: every day at its authored hour the hunter leaves its post, walks its path to the
//!   hunting ground and stalks there for a while. Its routine NPC's placement is a pure function
//!   of the tick and the outcome below, like any routine (#158, #819), so recovery and replay
//!   place it exactly. A player's arrival starts nothing.
//! - **Day roll**: whether the day's game turns on the hunter is a hash of the content seed and
//!   the day's number, drawn from nothing else, on [`CONFRONTATION_DAYS`] of days. It is fixed
//!   before the day begins and never rolled when someone looks.
//! - **Confrontation**: on such a day the zone stands the outing's reserved beast, an ordinary
//!   creature with its ordinary combat, threat and leash, at the hunting ground when the hunter
//!   arrives. Players may fight it or walk on; nothing asks them to.
//! - **Outcome**: when the stalk ends, the hunt succeeded on a quiet day, or on a confrontation
//!   day when the beast died while the hunter stood its ground: the hunter walks home with its
//!   catch ([`crate::PostureView::Carrying`]) and dresses it at its post for
//!   [`DRESS_TICKS`]. Otherwise it was hurt: it limps home at half pace and rests bandaged at its
//!   post until dusk ([`crate::PostureView::Hurt`], shown at [`INJURED_HEALTH_PERCENT`]). The
//!   outcome follows from the tick and the beast's canonical life alone, so the outing needs no
//!   state of its own.
//! - **Withdrawal**: a beast still standing outside its day's stalk leaves the world once it is
//!   out of combat, and it never respawns on its own.

use std::sync::Arc;

use crate::creature::Life;
use crate::routine::{DAWN, DUSK, HOUR_TICKS, NpcPlacement, along, legs, time_of_day, walk_ticks};
use crate::snapshot::CreatureAi;
use crate::{CreatureId, NpcId, PostureView, ZoneError, ZoneSimulation};

/// One of two days the game turns on the hunter.
pub const CONFRONTATION_DAYS: [u64; 2] = [1, 2];
/// A hurt hunter's projected health.
pub const INJURED_HEALTH_PERCENT: u8 = 35;
/// A successful hunter carries its catch at its post for an hour after it is back.
pub const DRESS_TICKS: u32 = HOUR_TICKS;
/// Keeps the hunt's roll independent of the raid's.
const CONFRONTATION_SALT: u64 = 0x4a7e_d0c5_91f3_2b60;

/// An authored outing: at `leave` (ticks since midnight) `hunter` walks `path` from its post to
/// the hunting ground at the path's end and stalks there `stalk` ticks facing `facing`, where a
/// confrontation day stands `beast`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HuntOuting {
    pub hunter: NpcId,
    pub leave: u32,
    pub path: Vec<[i32; 2]>,
    pub facing: u16,
    pub stalk: u32,
    pub beast: CreatureId,
}

/// How a day's outing ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HuntOutcome {
    /// The hunter brings its catch home.
    Catch,
    /// The beast hurt the hunter, who retreats to camp.
    Injured,
}

/// Whether the day containing `tick` is a confrontation day.
#[must_use]
pub const fn confrontation_day(seed: u64, tick: u64) -> bool {
    let day = (tick + crate::ZONE_START_TIME as u64) / crate::DAY_TICKS as u64;
    crate::raid::mix(seed ^ day ^ CONFRONTATION_SALT) % CONFRONTATION_DAYS[1]
        < CONFRONTATION_DAYS[0]
}

impl HuntOuting {
    /// Ticks the walk from `work` to the hunting ground takes.
    fn walk(&self, work: [i32; 2]) -> u32 {
        let total: i64 = legs(work, &self.path).iter().map(|leg| leg.2).sum();
        u32::try_from(walk_ticks(total)).unwrap_or(u32::MAX)
    }

    /// When the hunter of a post at `work` reaches the hunting ground (ticks since midnight).
    #[must_use]
    pub fn arrival(&self, work: [i32; 2]) -> u32 {
        self.leave.saturating_add(self.walk(work))
    }

    /// When its stalk ends and it turns for home.
    #[must_use]
    pub fn end(&self, work: [i32; 2]) -> u32 {
        self.arrival(work).saturating_add(self.stalk)
    }

    /// Where the hunter of a post at `work` facing `work_facing` is at `time`, and its posture,
    /// while the outing runs; `outcome` is asked only once the stalk has ended.
    pub fn placement(
        &self,
        work: [i32; 2],
        work_facing: u16,
        time: u32,
        outcome: impl FnOnce() -> HuntOutcome,
    ) -> Option<(NpcPlacement, PostureView)> {
        let legs = legs(work, &self.path);
        let walk = i64::from(self.walk(work));
        let speed = crate::NPC_WALK_UNITS_PER_TICK;
        let at_work = NpcPlacement {
            position: work,
            facing: work_facing,
            velocity: [0, 0],
        };
        let since = i64::from(time.checked_sub(self.leave)?);
        if since < walk {
            return Some((along(&legs, since * speed, true), PostureView::Hunting));
        }
        let since = since - walk;
        if since < i64::from(self.stalk) {
            let ground = NpcPlacement {
                position: legs.last().map_or(work, |leg| leg.1),
                facing: self.facing,
                velocity: [0, 0],
            };
            return Some((ground, PostureView::Hunting));
        }
        let since = since - i64::from(self.stalk);
        match outcome() {
            HuntOutcome::Catch if since < walk => {
                Some((along(&legs, since * speed, false), PostureView::Carrying))
            }
            HuntOutcome::Catch => {
                (since < walk + i64::from(DRESS_TICKS)).then_some((at_work, PostureView::Carrying))
            }
            // Limping, it covers half the ground a tick.
            HuntOutcome::Injured if since < 2 * walk => {
                let mut limp = along(&legs, since * speed / 2, false);
                limp.velocity = limp.velocity.map(|axis| axis / 2);
                Some((limp, PostureView::Hurt))
            }
            HuntOutcome::Injured => (time < DUSK).then_some((at_work, PostureView::Hurt)),
        }
    }

    pub(crate) fn validate(&self, content: &crate::ZoneContent) -> Result<(), ZoneError> {
        let in_range = |point: &[i32; 2]| {
            point
                .iter()
                .all(|value| crate::content::within_content_range(*value))
        };
        let Some(npc) = content.npc(self.hunter) else {
            return Err(ZoneError::new("a hunt's hunter is not an npc"));
        };
        let Some(routine) = content.routine(self.hunter) else {
            return Err(ZoneError::new("a hunt's hunter has no routine"));
        };
        let walk = self.walk(npc.position);
        // The day's errands are done before it leaves, and the limp home ends before dusk.
        let errands_done = routine
            .errands
            .iter()
            .all(|errand| errand.leave.saturating_add(errand.span(npc.position)) <= self.leave);
        if self.path.is_empty()
            || !self.path.iter().all(in_range)
            || self.stalk == 0
            || self.leave < DAWN
            || self
                .end(npc.position)
                .saturating_add(walk.saturating_mul(2))
                >= DUSK
            || !errands_done
            || content.creature_spawn(self.beast).is_none()
            || content.is_raider(self.beast)
            || content.is_depths_creature(self.beast)
        {
            return Err(ZoneError::new("invalid hunt outing"));
        }
        Ok(())
    }
}

impl ZoneSimulation {
    /// How the outing whose stalk has ended by `now` turned out: a quiet day's catch, or on a
    /// confrontation day a catch only when the beast died during the stalk.
    fn hunt_outcome(&self, hunt: &HuntOuting, work: [i32; 2], now: u64) -> HuntOutcome {
        if !confrontation_day(self.rng_seed(), now) {
            return HuntOutcome::Catch;
        }
        let arrival = hunt.arrival(work);
        let Some(start) = time_of_day(now)
            .checked_sub(arrival)
            .and_then(|since| now.checked_sub(u64::from(since)))
        else {
            return HuntOutcome::Injured;
        };
        let stalk = start..start + u64::from(hunt.stalk);
        let slain = self
            .creatures
            .get(&hunt.beast)
            .is_some_and(|state| match state.life {
                Life::Corpse { died_at, .. } | Life::Despawned { died_at } => {
                    stalk.contains(&died_at)
                }
                Life::Alive => false,
            });
        if slain {
            HuntOutcome::Catch
        } else {
            HuntOutcome::Injured
        }
    }

    /// Where the outing puts `npc` now, and its posture, if it is a hunter on its outing.
    pub(crate) fn hunt_placement(&self, npc: NpcId) -> Option<(NpcPlacement, PostureView)> {
        let hunt = self.content.hunt().filter(|hunt| hunt.hunter == npc)?;
        let definition = self.content.npc(npc)?;
        let time = time_of_day(self.tick);
        // An outing that left before the zone's first tick never ran.
        if self.tick < u64::from(time.checked_sub(hunt.leave)?) {
            return None;
        }
        hunt.placement(definition.position, definition.facing, time, || {
            self.hunt_outcome(hunt, definition.position, self.tick)
        })
    }

    /// Stands the beast when the hunter reaches the ground on a confrontation day, and lets a
    /// beast still standing outside its stalk leave once it is out of combat.
    pub(crate) fn advance_hunt(&mut self, now: u64) -> Result<(), ZoneError> {
        let content = Arc::clone(&self.content);
        let Some(hunt) = content.hunt() else {
            return Ok(());
        };
        let Some(hunter) = content.npc(hunt.hunter) else {
            return Ok(());
        };
        let time = time_of_day(now);
        let arrival = hunt.arrival(hunter.position);
        let stalking = (arrival..hunt.end(hunter.position)).contains(&time);
        let Some(state) = self.creatures.get(&hunt.beast) else {
            return Ok(());
        };
        if time == arrival
            && matches!(state.life, Life::Despawned { .. })
            && confrontation_day(self.rng_seed(), now)
        {
            self.spawn_creature(hunt.beast)?;
        } else if state.is_alive()
            && !stalking
            && state.ai != CreatureAi::Engaged
            && state.combat_timer == 0
        {
            self.retire_raider(hunt.beast, now)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{DRESS_TICKS, HuntOutcome, HuntOuting};
    use crate::routine::{DUSK, HOUR_TICKS, walk_ticks};
    use crate::{CreatureId, NpcId, PostureView};

    #[test]
    fn an_outing_walks_out_stalks_and_comes_home_by_its_outcome() {
        let hunt = HuntOuting {
            hunter: NpcId::new(1),
            leave: 14 * HOUR_TICKS,
            path: vec![[2_400, 0]],
            facing: 16_384,
            stalk: HOUR_TICKS,
            beast: CreatureId::new(1),
        };
        let walk = u32::try_from(walk_ticks(2_400)).unwrap();
        let end = hunt.end([0, 0]);
        let at = |time: u32, outcome| hunt.placement([0, 0], 0, time, || outcome);
        assert_eq!(at(hunt.leave - 1, HuntOutcome::Catch), None);
        let out = at(hunt.leave + walk / 2, HuntOutcome::Catch).unwrap();
        assert_eq!(out.1, PostureView::Hunting);
        assert!(out.0.velocity[0] > 0);
        let ground = at(end - 1, HuntOutcome::Injured).unwrap();
        assert_eq!(
            (ground.0.position, ground.1),
            ([2_400, 0], PostureView::Hunting)
        );
        // With its catch it walks home at pace and dresses it at its post for an hour.
        let home = at(end + walk / 2, HuntOutcome::Catch).unwrap();
        assert_eq!(home.1, PostureView::Carrying);
        assert_eq!(
            at(end + walk + 1, HuntOutcome::Catch).map(|(p, posture)| (p.position, posture)),
            Some(([0, 0], PostureView::Carrying))
        );
        assert_eq!(at(end + walk + DRESS_TICKS, HuntOutcome::Catch), None);
        // Hurt, it limps home at half pace and rests until dusk.
        let limp = at(end + walk / 2, HuntOutcome::Injured).unwrap();
        assert_eq!(limp.1, PostureView::Hurt);
        assert!(limp.0.position[0] > home.0.position[0]);
        assert!(limp.0.velocity[0] < 0 && limp.0.velocity[0] > home.0.velocity[0]);
        assert_eq!(
            at(DUSK - 1, HuntOutcome::Injured).map(|(p, posture)| (p.position, posture)),
            Some(([0, 0], PostureView::Hurt))
        );
        assert_eq!(at(DUSK, HuntOutcome::Injured), None);
    }
}
