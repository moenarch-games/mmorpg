//! Night raids (#530, child 1 of #275; Atlas 5.1.49, 5.1.53–5.1.58, 5.1.63–5.1.68): Redbrand
//! raiders attack Millbrook Farm's barn at night.
//!
//! - **Trigger** (5.1.49, 5.1.58, 5.1.68): at [`TRIGGER_TIME`] each night the zone commits an
//!   incident against its one authored target, but only when the target is **eligible** and the
//!   night's roll succeeds. The roll is a hash of the content seed and the night's number, so it
//!   draws nothing from the zone RNG, and [`TRIGGER_NIGHTS`] of nights are raid nights.
//!   Eligibility means no incident is running, the barn is fully restored, and
//!   [`COOLDOWN_TICKS`] have passed since the last incident resolved. The target is committed
//!   with the incident and never changes.
//! - **Assault** (5.1.55, 5.1.56, 5.1.67): [`RAID_PHASES`] phases of [`PHASE_TICKS`] each. Each
//!   phase begins by committing one wave of raiders at the authored staging slots on the farm's
//!   outskirts, preferring slots that no player is near. The wave's size comes from the
//!   participation snapshot taken at that boundary: players who fought this incident's raiders
//!   and are still alive near the barn. The raiders walk their authored routes to the barn and
//!   hit it on their swing timer. They fight players who come close (ordinary aggro, assist and
//!   leash). Later participation never rescales a raider that is already standing.
//! - **Outcome** (5.1.56): when the barn's integrity reaches zero, the raid is lost. When the
//!   barn still stands at the end of the last phase, it is defended. Either way the incident
//!   resolves once.
//! - **Retreat** (5.1.63–5.1.66): the surviving raiders withdraw as a group along the authored
//!   retreat route to Redbrand Hollow, defending themselves without restarting the attack.
//!   There they join the camp's ordinary defence (aggro and assist, around their own camp spot)
//!   for [`CAMP_STAY_TICKS`], then leave the world. Later incidents commit fresh raider lives,
//!   never camp survivors. A raider that died stays gone.
//! - **Recovery and cooldown** (5.1.68): the barn's integrity grows back by one point every
//!   [`RECOVERY_TICKS_PER_POINT`] after resolution. Both conditions of the next eligibility
//!   follow from the resolution tick and the integrity alone.
//! - **Off-screen** (5.1.54): the single zone simulates every creature every tick whether or
//!   not anyone watches, so an unwatched raid advances and resolves exactly as a watched one.
//! - **Signals** (5.1.53): players within [`SIGNAL_RADIUS_UNITS`] of the barn receive a
//!   [`RaidView`]. It carries the phase, the barn's integrity, and an approximate disturbance
//!   circle around the farm for the minimap. Clients show smoke over a burning or damaged barn.
//!   No region-wide announcement is sent.
//!
//! - **Aftermath** (#531, 5.1.50): a lost raid opens the site's authored aid tasks: breaches in
//!   the barn's walls, and gate guards who answered the alarm and now lie injured by the farmhouse
//!   instead of standing at their gates (projected at [`INJURED_HEALTH_PERCENT`]). The tasks close
//!   on their own, in authored order, as the barn's integrity grows back; the last one closes when
//!   the barn is whole again, so the farm returns to normal in finite time without any help.
//! - **Recovery aid** (#531, 5.1.51): a player within [`AID_REACH_UNITS`] of an open task's spot
//!   can tend that guard or mend that breach ([`crate::ZoneCommand::RaidAid`]). The task closes
//!   once, for whoever got there first, and the shared barn regains [`AID_RESTORE_POINTS`]. Aid
//!   speeds the restoration only; the cooldown (5.1.68) still runs from the resolution tick.
//! - **Participation rewards** (#531, 5.1.52): a character earns [`REWARD_EXPERIENCE`] and
//!   [`REWARD_COPPER`] once per incident, the first time it either fights the incident's raiders
//!   (it has threat on one of them: damage, or healing a defender) or has an aid action
//!   accepted. Standing nearby earns nothing, and there is no ranking. The once-only grant is the
//!   zone's `rewarded` list plus the durable [`crate::RewardKey::RaidParticipation`] receipt.
//!
//! - **Forewarning** (#532, 5.1.61, 5.1.62): from [`FOREWARNING_TIME`] until the nightly check,
//!   on every raid night and on [`FALSE_ALARM_NIGHTS`] of the other nights, a scouting party's
//!   tracks lie on the site's approach while the target is eligible. Guards and residents speak of
//!   them, so the signs are uncertain: about one evening in five with tracks passes without a raid.
//!   A player within [`TRACKS_REACH_UNITS`] of the tracks investigates them, and may then report
//!   them to the site's lookout guard. Neither cancels nor postpones anything: the nightly check
//!   decides exactly as it would without them.
//! - **Preparation** (#532, 5.1.59, 5.1.60, 5.1.69): a report the lookout accepts posts the
//!   guards' watch, and once warned the guards ask for help bracing the objective at its authored
//!   brace spots. Each accepted preparation takes [`PREPARED_HIT_PERCENT`] off every raider's hit
//!   on the objective; all of them together still let an unopposed raid bring it down. The
//!   readiness belongs to the coming incident only: it retires when that incident resolves,
//!   whatever the outcome, and expires at the nightly check when no incident commits. Each step
//!   is accepted once per readiness, so repeats change nothing. The actor gets a short
//!   [`RaidNotice`], and players near the target get one when a prepared incident resolves, told
//!   truthfully whether the barn held.
//!
//! The raid state, the participants and every raider's march are canonical. A suspended zone's
//! tick freezes, and its raid, recovery and cooldown freeze with it.

use std::sync::Arc;

use crate::creature::Life;
use crate::entity::body_id;
use crate::routine::{HOUR_TICKS, time_of_day};
use crate::snapshot::CreatureAi;
use crate::{
    CreatureId, EntityRef, ErrorCode, NpcId, PlayerId, ZoneError, ZoneEvent, ZoneSimulation,
};

/// The nightly check, at 22:00.
pub const TRIGGER_TIME: u32 = 22 * HOUR_TICKS;
/// Two of three eligible nights are raid nights.
pub const TRIGGER_NIGHTS: [u64; 2] = [2, 3];
/// Assault phases per incident.
pub const RAID_PHASES: u8 = 3;
/// One assault phase lasts 75 s.
pub const PHASE_TICKS: u32 = crate::long_ticks_from_ms(75_000);
/// Raiders each phase commits for one participant …
pub const WAVE_BASE: [u8; RAID_PHASES as usize] = [3, 3, 4];
/// … plus one more for each further participant …
pub const WAVE_PER_EXTRA_PARTICIPANT: u8 = 1;
/// … and at most this many.
pub const WAVE_MAX: u8 = 6;
/// The barn's full integrity: three unopposed waves bring it down in the last phase.
pub const OBJECTIVE_INTEGRITY: u32 = 2_400;
/// One integrity point grows back every 0.5 s, so a fully broken barn is restored in 20 min.
pub const RECOVERY_TICKS_PER_POINT: u32 = crate::long_ticks_from_ms(500);
/// A new incident against the same target waits 12 zone hours (1 real hour) after the last one
/// resolved.
pub const COOLDOWN_TICKS: u32 = 12 * HOUR_TICKS;
/// A raider within this horizontal distance of the barn's footprint hits it.
pub const OBJECTIVE_REACH_UNITS: i32 = 150;
/// A raider at the objective pushed farther than this from it closes in again.
const STAND_OFF_UNITS: i32 = 110;
/// A march waypoint counts as reached within this distance.
pub const WAYPOINT_RADIUS_UNITS: i32 = 100;
/// Players who fought the raid count as participants while they are within 60 m of the barn.
pub const PARTICIPANT_RADIUS_UNITS: i32 = 6_000;
/// A staging slot counts as watched while a player is within 30 m of it.
pub const STAGING_WATCH_RADIUS_UNITS: i32 = 3_000;
/// Players within 150 m of the barn see the raid's signals.
pub const SIGNAL_RADIUS_UNITS: i32 = 15_000;
/// The minimap's disturbance circle has a 35 m radius …
pub const AREA_RADIUS_UNITS: i32 = 3_500;
/// … centred this far from the barn's centre, so it shows the farm rather than the exact spot.
pub const AREA_OFFSET_UNITS: [i32; 2] = [-600, 400];
/// Survivors reinforce the camp for 20 min.
pub const CAMP_STAY_TICKS: u32 = crate::long_ticks_from_ms(20 * 60 * 1_000);
/// A retreating raider that has not reached the camp after 5 min leaves the world once no player
/// is near it.
pub const RETREAT_TIMEOUT_TICKS: u32 = crate::long_ticks_from_ms(5 * 60 * 1_000);
/// Most raiders a site may reserve.
pub const MAX_RAIDERS: usize = 24;
/// Most waypoints of one authored route.
pub const MAX_RAID_WAYPOINTS: usize = 24;
/// Most aid tasks a site may author: projections carry the open ones as five bits.
pub const MAX_AID_TASKS: usize = 5;
/// A player aids a task within 3 m of its spot.
pub const AID_REACH_UNITS: i32 = 300;
/// One accepted aid action gives the barn back 360 integrity points, 3 of its 20 min of
/// restoration.
pub const AID_RESTORE_POINTS: u32 = 360;
/// What a defender or helper earns once per incident: a little more than one level-4 kill.
pub const REWARD_EXPERIENCE: u32 = 120;
pub const REWARD_COPPER: u32 = 15;
/// An injured guard's projected health.
pub const INJURED_HEALTH_PERCENT: u8 = 30;
/// Forewarning signs lie on the approach from 19:00 until the nightly check (#532).
pub const FOREWARNING_TIME: u32 = 19 * HOUR_TICKS;
/// One of two nights without a raid still shows signs: a false alarm.
pub const FALSE_ALARM_NIGHTS: [u64; 2] = [1, 2];
/// A player investigates the tracks within 3 m of them.
pub const TRACKS_REACH_UNITS: i32 = 300;
/// A player braces the objective within 3 m of a brace spot.
pub const BRACE_REACH_UNITS: i32 = 300;
/// Most brace spots a site may author: readiness carries the warning and the braces in four bits.
pub const MAX_BRACES: usize = 3;
/// Each accepted preparation takes this share off every raider's hit on the objective.
pub const PREPARED_HIT_PERCENT: u32 = 10;
/// The readiness bit of an accepted report; brace `k` is bit `k + 1`.
pub const READINESS_WARNED: u8 = 1;

/// One preparation step (#532, 5.1.59): investigate the tracks, report them to the lookout, or
/// brace the objective at brace spot `k`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RaidPrep {
    Investigate,
    Report,
    Brace(u8),
}

/// A short raid acknowledgment for a player (#532, 5.1.60): one of its own preparation steps was
/// accepted, or a prepared incident resolved near it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RaidNotice {
    /// The player read the scouts' tracks.
    Tracks,
    /// The lookout accepted the player's report and posts the watch.
    Warned,
    /// The player braced the objective.
    Braced,
    /// A prepared incident ended with the objective standing.
    Held,
    /// A prepared incident still brought the objective down.
    Fell,
}

/// What one aid task asks of a helper (5.1.51).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AidKind {
    /// Tend the injured guard, who lies at the task's spot until tended or recovered.
    TendGuard(NpcId),
    /// Mend the breach in the objective at the task's spot.
    MendBreach,
}

/// One authored post-raid aid task: its kind and where a helper does it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AidTask {
    pub kind: AidKind,
    pub at: [i32; 2],
}

/// One way into the target: the reserved raider spawns standing at its outskirts staging slots,
/// and the route from there to the objective. The route's last point is where its raiders hit
/// the objective, and the n-th raider of the approach stands `n` × `spread` further along.
/// `exit` leads from the objective to the site's shared retreat route.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RaidApproach {
    pub raiders: Vec<CreatureId>,
    pub route: Vec<[i32; 2]>,
    pub spread: [i32; 2],
    pub exit: Vec<[i32; 2]>,
}

/// An authored raid target (5.1.57, 5.1.58): the objective's collider and footprint, the ways in,
/// and the retreat route, which ends at the camp the survivors reinforce.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RaidSite {
    pub name: String,
    pub objective: u32,
    /// Footprint as `[min, max]` XZ corners.
    pub footprint: [[i32; 2]; 2],
    pub approaches: Vec<RaidApproach>,
    pub retreat: Vec<[i32; 2]>,
    /// Survivors spread out from the camp point by this much each.
    pub camp_spread: [i32; 2],
    /// What a lost raid leaves to aid, in the order the tasks close on their own (#531).
    pub aid: Vec<AidTask>,
    /// Where the scouting party's tracks lie on a forewarned evening (#532).
    pub tracks: [i32; 2],
    /// The guard who takes reports of the tracks (#532).
    pub lookout: NpcId,
    /// Where helpers brace the objective once the guards are warned (#532).
    pub braces: Vec<[i32; 2]>,
}

impl RaidSite {
    /// Every aid task's bit.
    #[must_use]
    pub fn aid_mask(&self) -> u8 {
        u8::try_from((1_u16 << self.aid.len()) - 1).unwrap_or(u8::MAX)
    }

    /// Whether aid task `index` has closed on its own at `damage`: the tasks close one by one
    /// as the objective regains equal shares of its integrity, the last one once it is whole.
    #[must_use]
    pub fn aid_settled(&self, index: usize, damage: u32) -> bool {
        let count = u64::try_from(self.aid.len()).unwrap_or(u64::MAX);
        let index = u64::try_from(index).unwrap_or(u64::MAX);
        u64::from(damage) * count
            <= u64::from(OBJECTIVE_INTEGRITY) * count.saturating_sub(index + 1)
    }

    /// The injured guard `npc`'s shelter spot while its task is open in `aftermath`.
    #[must_use]
    pub fn injured_at(&self, npc: NpcId, aftermath: u8) -> Option<[i32; 2]> {
        self.aid.iter().enumerate().find_map(|(index, task)| {
            (task.kind == AidKind::TendGuard(npc) && aftermath & (1 << index) != 0)
                .then_some(task.at)
        })
    }

    /// Every readiness bit the site can set: the warning and one per brace spot.
    #[must_use]
    pub fn readiness_mask(&self) -> u8 {
        u8::try_from((1_u16 << (self.braces.len() + 1)) - 1).unwrap_or(u8::MAX)
    }

    /// The guards whose posts the aftermath can empty.
    pub fn aid_guards(&self) -> impl Iterator<Item = NpcId> + '_ {
        self.aid.iter().filter_map(|task| match task.kind {
            AidKind::TendGuard(npc) => Some(npc),
            AidKind::MendBreach => None,
        })
    }

    #[must_use]
    pub fn centre(&self) -> [i32; 2] {
        let [min, max] = self.footprint;
        [(min[0] + max[0]) / 2, (min[1] + max[1]) / 2]
    }

    /// The approximate disturbance circle clients draw on the minimap while an assault runs
    /// (5.1.53): centre and radius in units, around the farm rather than the exact objective.
    #[must_use]
    pub fn area(&self) -> ([i32; 2], i32) {
        let centre = self.centre();
        (
            [
                centre[0] + AREA_OFFSET_UNITS[0],
                centre[1] + AREA_OFFSET_UNITS[1],
            ],
            AREA_RADIUS_UNITS,
        )
    }

    /// The horizontal distance from `at` to the footprint, squared.
    fn distance_squared(&self, at: [i32; 2]) -> i64 {
        let [min, max] = self.footprint;
        let gap =
            |value: i32, low: i32, high: i32| i64::from((low - value).max(value - high).max(0));
        gap(at[0], min[0], max[0]).pow(2) + gap(at[1], min[1], max[1]).pow(2)
    }

    /// The approach and slot that reserve `raider`.
    #[must_use]
    pub fn slot_of(&self, raider: CreatureId) -> Option<(u8, u8)> {
        self.approaches
            .iter()
            .enumerate()
            .find_map(|(approach, way)| {
                let slot = way.raiders.iter().position(|&id| id == raider)?;
                Some((u8::try_from(approach).ok()?, u8::try_from(slot).ok()?))
            })
    }

    /// Every reserved raider.
    pub fn raiders(&self) -> impl Iterator<Item = CreatureId> + '_ {
        self.approaches
            .iter()
            .flat_map(|way| way.raiders.iter().copied())
    }

    /// Waypoint `index` of the march a raider of `approach`, `slot` follows on `leg`.
    #[must_use]
    pub fn waypoint(&self, leg: RaidLeg, approach: u8, slot: u8, index: u8) -> Option<[i32; 2]> {
        let index = usize::from(index);
        match leg {
            RaidLeg::Assault => {
                let way = self.approaches.get(usize::from(approach))?;
                let point = *way.route.get(index)?;
                Some(if index + 1 == way.route.len() {
                    offset_by(point, way.spread, slot)
                } else {
                    point
                })
            }
            RaidLeg::Retreat => {
                let exit = &self.approaches.get(usize::from(approach))?.exit;
                let point = match index.checked_sub(exit.len()) {
                    None => exit[index],
                    Some(shared) => *self.retreat.get(shared)?,
                };
                Some(if index + 1 == exit.len() + self.retreat.len() {
                    self.camp_spot(approach, slot)
                } else {
                    point
                })
            }
            RaidLeg::Camp => None,
        }
    }

    /// Waypoints of a raider of `approach` on `leg`.
    #[must_use]
    pub fn leg_len(&self, leg: RaidLeg, approach: u8) -> usize {
        let way = self.approaches.get(usize::from(approach));
        match leg {
            RaidLeg::Assault => way.map_or(0, |way| way.route.len()),
            RaidLeg::Retreat => way.map_or(0, |way| way.exit.len()) + self.retreat.len(),
            RaidLeg::Camp => 0,
        }
    }

    /// The camp spot a survivor of `approach`, `slot` holds.
    #[must_use]
    pub fn camp_spot(&self, approach: u8, slot: u8) -> [i32; 2] {
        let camp = self.retreat.last().copied().unwrap_or([0, 0]);
        let before: usize = self
            .approaches
            .iter()
            .take(usize::from(approach))
            .map(|way| way.raiders.len())
            .sum();
        let rank = u8::try_from(before + usize::from(slot)).unwrap_or(u8::MAX);
        let count = u8::try_from(self.raiders().count()).unwrap_or(u8::MAX);
        // Centred on the camp point.
        let centred = i32::from(rank) - i32::from(count / 2);
        [
            camp[0].saturating_add(self.camp_spread[0].saturating_mul(centred)),
            camp[1].saturating_add(self.camp_spread[1].saturating_mul(centred)),
        ]
    }

    pub(crate) fn validate(&self, content: &crate::ZoneContent) -> Result<(), ZoneError> {
        let in_range = |point: &[i32; 2]| {
            point
                .iter()
                .all(|value| crate::content::within_content_range(*value))
        };
        let raiders: Vec<CreatureId> = self.raiders().collect();
        let mut sorted = raiders.clone();
        sorted.sort();
        sorted.dedup();
        let [min, max] = self.footprint;
        if self.name.is_empty()
            || self.approaches.is_empty()
            || raiders.is_empty()
            || raiders.len() > MAX_RAIDERS
            || sorted.len() != raiders.len()
            || raiders
                .iter()
                .any(|&id| content.creature_spawn(id).is_none())
            || self.approaches.iter().any(|way| {
                way.route.is_empty()
                    || way.route.len() > MAX_RAID_WAYPOINTS
                    || !way.route.iter().all(in_range)
                    || way.exit.len() > MAX_RAID_WAYPOINTS
                    || !way.exit.iter().all(in_range)
            })
            || self.retreat.is_empty()
            || self.retreat.len() > MAX_RAID_WAYPOINTS
            || !self.retreat.iter().all(in_range)
            || min[0] >= max[0]
            || min[1] >= max[1]
            || !self.footprint.iter().all(in_range)
            || content
                .definition()
                .colliders()
                .iter()
                .all(|collider| collider.id != self.objective)
            || self.aid.len() > MAX_AID_TASKS
            || !self.aid.iter().all(|task| in_range(&task.at))
            || self.aid_guards().any(|npc| content.npc(npc).is_none())
            || !in_range(&self.tracks)
            || content.npc(self.lookout).is_none()
            || content.routine(self.lookout).is_some()
            || self.braces.len() > MAX_BRACES
            || !self.braces.iter().all(in_range)
            || {
                let mut guards: Vec<NpcId> = self.aid_guards().collect();
                let count = guards.len();
                guards.sort_unstable();
                guards.dedup();
                guards.len() != count || guards.iter().any(|&npc| content.routine(npc).is_some())
            }
        {
            return Err(ZoneError::new("invalid raid site"));
        }
        Ok(())
    }
}

fn offset_by(point: [i32; 2], spread: [i32; 2], slot: u8) -> [i32; 2] {
    [
        point[0].saturating_add(spread[0].saturating_mul(i32::from(slot))),
        point[1].saturating_add(spread[1].saturating_mul(i32::from(slot))),
    ]
}

/// Which part of its incident a raider is in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RaidLeg {
    /// Marching to the objective and hitting it.
    Assault,
    /// Withdrawing to the camp.
    Retreat,
    /// Reinforcing the camp.
    Camp,
}

/// A creature committed to a raid: its incident, its march and the anchor it leashes to and
/// evades back to: where its last fight began on a march, or its camp spot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RaidMember {
    pub incident: u32,
    pub leg: RaidLeg,
    /// The next waypoint of its leg.
    pub waypoint: u8,
    pub anchor: [i32; 2],
    /// The tick its leg began.
    pub since: u64,
}

/// How an incident ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RaidOutcome {
    /// The objective stood through the last phase.
    Defended,
    /// The objective's integrity ran out.
    Lost,
}

/// A resolved incident.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RaidResolution {
    pub incident: u32,
    pub at: u64,
    pub outcome: RaidOutcome,
}

/// The running incident.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RaidIncident {
    pub id: u32,
    /// `1..=RAID_PHASES`.
    pub phase: u8,
    pub phase_started: u64,
    /// The participation snapshot each committed phase was sized for, in phase order.
    pub scaled_for: Vec<u8>,
    /// Players who fought this incident's raiders, by ID.
    pub participants: Vec<PlayerId>,
}

/// The zone's raid state (canonical).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RaidState {
    /// Incidents committed so far; the next one's ID is one more.
    pub incidents: u32,
    pub active: Option<RaidIncident>,
    /// Integrity the objective has lost, `0..=OBJECTIVE_INTEGRITY`.
    pub damage: u32,
    pub last: Option<RaidResolution>,
    /// The last lost incident's aid tasks still open, one bit per task in authored order
    /// (#531).
    pub aftermath: u8,
    /// Players already rewarded for the current incident (the running one, otherwise the last),
    /// by ID (#531).
    pub rewarded: Vec<PlayerId>,
    /// Players who read this evening's tracks, by ID (#532).
    pub scouted: Vec<PlayerId>,
    /// The coming or running incident's accepted preparations: [`READINESS_WARNED`], then one
    /// bit per brace spot (#532).
    pub readiness: u8,
}

impl RaidState {
    /// The incident rewards and aid currently count toward: the running one, otherwise the last.
    #[must_use]
    pub fn current_incident(&self) -> Option<u32> {
        self.active
            .as_ref()
            .map(|incident| incident.id)
            .or(self.last.map(|last| last.incident))
    }

    pub(crate) fn validate(
        &self,
        now: u64,
        aid_mask: u8,
        readiness_mask: u8,
    ) -> Result<(), ZoneError> {
        let forewarning_ok = self.scouted.len() <= crate::MAX_PLAYERS_PER_ZONE
            && self.scouted.windows(2).all(|pair| pair[0] < pair[1])
            && self.readiness & !readiness_mask == 0
            // Bracing follows the warning.
            && (self.readiness == 0 || self.readiness & READINESS_WARNED != 0);
        if !forewarning_ok {
            return Err(ZoneError::new("raid readiness is out of range"));
        }
        let aftermath_ok = self.aftermath & !aid_mask == 0
            && (self.aftermath == 0
                || (self.active.is_none()
                    && self.damage > 0
                    && self
                        .last
                        .is_some_and(|last| last.outcome == RaidOutcome::Lost)));
        let rewarded_ok = self.rewarded.len() <= crate::MAX_PLAYERS_PER_ZONE
            && self.rewarded.windows(2).all(|pair| pair[0] < pair[1])
            && (self.rewarded.is_empty() || self.current_incident().is_some());
        if !aftermath_ok || !rewarded_ok {
            return Err(ZoneError::new("raid aftermath is out of range"));
        }
        let active_ok = self.active.as_ref().is_none_or(|incident| {
            incident.id == self.incidents
                && (1..=RAID_PHASES).contains(&incident.phase)
                && incident.phase_started <= now
                && incident.scaled_for.len() == usize::from(incident.phase)
                && incident.participants.len() <= crate::MAX_PLAYERS_PER_ZONE
                && incident
                    .participants
                    .windows(2)
                    .all(|pair| pair[0] < pair[1])
                && self.last.is_none_or(|last| last.incident < incident.id)
        });
        let last_ok = self.last.is_none_or(|last| {
            last.at <= now && last.incident >= 1 && last.incident <= self.incidents
        });
        if !active_ok || !last_ok || self.damage > OBJECTIVE_INTEGRITY {
            return Err(ZoneError::new("raid state is out of range"));
        }
        Ok(())
    }
}

/// What a player near the target sees of the raid (5.1.53).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RaidView {
    /// The assault phase, `1..=RAID_PHASES`, or 0 once it resolved and the barn is still
    /// recovering.
    pub phase: u8,
    /// The objective's integrity in percent, rounded up while any is left.
    pub integrity: u8,
    /// The aid tasks still open after a lost raid, one bit per authored task (#531).
    pub aid: u8,
    /// Whether the scouts' tracks lie on the approach this evening (#532); only with phase 0.
    pub signs: bool,
    /// The coming or running incident's accepted preparations (#532): [`READINESS_WARNED`], then
    /// one bit per brace spot. Never with aid.
    pub readiness: u8,
    /// Whether the viewer read this evening's tracks (#532); only with signs.
    pub scouted: bool,
}

/// `splitmix64`: a well-mixed hash of the night's number and the content seed.
const fn mix(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

/// A body velocity (subunits per tick) and, when it turns, its new facing.
pub(crate) type Steering = ([i32; 2], Option<u16>);

/// Whether the night containing `tick` is a raid night.
#[must_use]
pub const fn raid_night(seed: u64, tick: u64) -> bool {
    let night = (tick + crate::ZONE_START_TIME as u64) / crate::DAY_TICKS as u64;
    mix(seed ^ night) % TRIGGER_NIGHTS[1] < TRIGGER_NIGHTS[0]
}

/// Whether the evening containing `tick` shows forewarning signs: every raid night, and a false
/// alarm on [`FALSE_ALARM_NIGHTS`] of the others.
#[must_use]
pub const fn signs_night(seed: u64, tick: u64) -> bool {
    let night = (tick + crate::ZONE_START_TIME as u64) / crate::DAY_TICKS as u64;
    raid_night(seed, tick)
        || mix(seed ^ night ^ FALSE_ALARM_SALT) % FALSE_ALARM_NIGHTS[1] < FALSE_ALARM_NIGHTS[0]
}

/// Keeps the false-alarm roll independent of the raid roll.
const FALSE_ALARM_SALT: u64 = 0x5ca7_7e2d_7aac_5000;

/// A raider's hit on the objective under `readiness`: [`PREPARED_HIT_PERCENT`] less per accepted
/// preparation, rounded up.
#[must_use]
pub const fn prepared_hit(amount: u32, readiness: u8) -> u32 {
    (amount * (100 - PREPARED_HIT_PERCENT * readiness.count_ones())).div_ceil(100)
}

/// The raiders a phase commits for `participants`.
#[must_use]
pub fn wave_size(phase: u8, participants: u8) -> u8 {
    let base = WAVE_BASE
        .get(usize::from(phase.saturating_sub(1)))
        .copied()
        .unwrap_or(0);
    base.saturating_add(WAVE_PER_EXTRA_PARTICIPANT.saturating_mul(participants.saturating_sub(1)))
        .min(WAVE_MAX)
}

impl ZoneSimulation {
    /// The zone's raid state.
    #[must_use]
    pub const fn raid(&self) -> &RaidState {
        &self.raid
    }

    /// Whether a new incident may be committed at `now` (5.1.68): none is running, the objective
    /// is restored, and the cooldown since the last one has passed.
    #[must_use]
    pub fn raid_eligible(&self, now: u64) -> bool {
        self.content.raid().is_some()
            && self.raid.active.is_none()
            && self.raid.damage == 0
            && self.raid.aftermath == 0
            && self
                .raid
                .last
                .is_none_or(|last| now >= last.at.saturating_add(u64::from(COOLDOWN_TICKS)))
    }

    /// Whether the scouts' tracks lie on the approach at `now` (#532): a forewarned evening
    /// before the nightly check, while the target is eligible.
    #[must_use]
    pub fn forewarned(&self, now: u64) -> bool {
        (FOREWARNING_TIME..TRIGGER_TIME).contains(&time_of_day(now))
            && self.raid_eligible(now)
            && signs_night(self.content.rng_seed(), now)
    }

    /// Commits a new incident at the current tick when the target is eligible, whatever the
    /// time of day or the night's roll: the nightly trigger's commitment, for tests and tools.
    /// Returns whether it committed one.
    pub fn begin_raid(&mut self) -> Result<bool, ZoneError> {
        let now = self.tick;
        if !self.raid_eligible(now) {
            return Ok(false);
        }
        self.commit_incident(now)?;
        Ok(true)
    }

    /// A recovered creature's raid membership (#530) belongs to a living reserved raider of a
    /// committed incident: on the assault only for the running one, with waypoints on its
    /// routes and its anchor in range.
    pub(crate) fn validate_raid_member(
        &self,
        record: &crate::CanonicalCreatureSnapshot,
    ) -> Result<(), ZoneError> {
        let Some(member) = record.raid else {
            return Ok(());
        };
        let valid = self.content.raid().is_some_and(|site| {
            let legs = match member.leg {
                RaidLeg::Assault => {
                    site.slot_of(record.creature_id)
                        .is_some_and(|(approach, _)| {
                            usize::from(member.waypoint)
                                <= site.approaches[usize::from(approach)].route.len()
                        })
                        && self
                            .raid
                            .active
                            .as_ref()
                            .is_some_and(|incident| incident.id == member.incident)
                }
                RaidLeg::Retreat => {
                    site.slot_of(record.creature_id)
                        .is_some_and(|(approach, _)| {
                            usize::from(member.waypoint) <= site.leg_len(RaidLeg::Retreat, approach)
                        })
                }
                RaidLeg::Camp => member.waypoint == 0,
            };
            legs && record.life == crate::CreatureLife::Alive
                && site.slot_of(record.creature_id).is_some()
                && (1..=self.raid.incidents).contains(&member.incident)
                && member.since <= self.tick
                && member
                    .anchor
                    .iter()
                    .all(|value| crate::content::within_content_range(*value))
        });
        if !valid {
            return Err(ZoneError::new("creature raid membership is out of range"));
        }
        Ok(())
    }

    /// A creature's raid membership.
    #[must_use]
    pub fn raid_member(&self, creature: CreatureId) -> Option<RaidMember> {
        self.creatures.get(&creature).and_then(|state| state.raid)
    }

    /// Where a creature leashes, evades and wanders: a raider's anchor, otherwise its spawn.
    pub(crate) fn creature_home(&self, id: CreatureId, spawn: &crate::CreatureSpawn) -> [i32; 2] {
        self.creatures
            .get(&id)
            .and_then(|creature| creature.raid)
            .map_or(spawn.position, |member| member.anchor)
    }

    fn commit_incident(&mut self, now: u64) -> Result<(), ZoneError> {
        let id = self
            .raid
            .incidents
            .checked_add(1)
            .ok_or_else(|| ZoneError::new("raid incident identities exhausted"))?;
        self.raid.incidents = id;
        self.raid.rewarded.clear();
        // The evening's tracks are moot once the raid is on; its readiness stays for it.
        self.raid.scouted.clear();
        self.raid.active = Some(RaidIncident {
            id,
            phase: 1,
            phase_started: now,
            scaled_for: Vec::new(),
            participants: Vec::new(),
        });
        self.commit_wave(now)
    }

    /// Takes the participation snapshot, records it with the phase and stands the wave up.
    fn commit_wave(&mut self, now: u64) -> Result<(), ZoneError> {
        let content = Arc::clone(&self.content);
        let Some(site) = content.raid() else {
            return Ok(());
        };
        let participants = self.participation(site)?;
        let Some(incident) = self.raid.active.as_mut() else {
            return Ok(());
        };
        incident.scaled_for.push(participants);
        let (id, phase) = (incident.id, incident.phase);
        let mut wanted = wave_size(phase, participants.max(1));
        // Unwatched approaches first, then watched ones; within each in authored order.
        let mut ways: Vec<(bool, usize)> = Vec::with_capacity(site.approaches.len());
        for (index, way) in site.approaches.iter().enumerate() {
            let staging = way
                .raiders
                .first()
                .and_then(|&raider| self.content.creature_spawn(raider))
                .map_or([0, 0], |spawn| spawn.position);
            let watched = self
                .interest
                .near(staging[0], staging[1], STAGING_WATCH_RADIUS_UNITS)
                .into_iter()
                .any(|entity| matches!(entity, EntityRef::Player(_)));
            ways.push((watched, index));
        }
        ways.sort_unstable();
        // Round-robin over the approaches, each standing up its free slots in order.
        let free: Vec<Vec<CreatureId>> = ways
            .iter()
            .map(|&(_, index)| {
                site.approaches[index]
                    .raiders
                    .iter()
                    .copied()
                    .filter(|raider| {
                        self.creatures
                            .get(raider)
                            .is_none_or(|state| matches!(state.life, Life::Despawned { .. }))
                    })
                    .collect()
            })
            .collect();
        let mut round = 0;
        while wanted > 0 {
            let mut stood = false;
            for list in &free {
                if wanted == 0 {
                    break;
                }
                if let Some(&raider) = list.get(round) {
                    self.spawn_creature(raider)?;
                    let anchor = self
                        .content
                        .creature_spawn(raider)
                        .map_or([0, 0], |spawn| spawn.position);
                    if let Some(state) = self.creatures.get_mut(&raider) {
                        state.raid = Some(RaidMember {
                            incident: id,
                            leg: RaidLeg::Assault,
                            waypoint: 0,
                            anchor,
                            since: now,
                        });
                    }
                    wanted -= 1;
                    stood = true;
                }
            }
            if !stood {
                break;
            }
            round += 1;
        }
        Ok(())
    }

    /// The phase-boundary participation snapshot (5.1.55): recorded participants who are alive
    /// and near the objective.
    fn participation(&self, site: &RaidSite) -> Result<u8, ZoneError> {
        let Some(incident) = &self.raid.active else {
            return Ok(0);
        };
        let mut count = 0_u8;
        for &player_id in &incident.participants {
            let Some(player) = self.players.get(&player_id) else {
                continue;
            };
            if !player.is_alive() {
                continue;
            }
            let at = self.player_position(player_id)?;
            if site.distance_squared([at.x, at.z]) <= i64::from(PARTICIPANT_RADIUS_UNITS).pow(2) {
                count = count.saturating_add(1);
            }
        }
        Ok(count)
    }

    /// Tick step 7b: the raid's bookkeeping, after combat resolved the tick's blows.
    pub(crate) fn advance_raid(&mut self, now: u64) -> Result<(), ZoneError> {
        let content = Arc::clone(&self.content);
        let Some(site) = content.raid() else {
            return Ok(());
        };
        let members: Vec<(CreatureId, RaidMember)> = self
            .creatures
            .iter()
            .filter(|(_, state)| state.is_alive())
            .filter_map(|(&id, state)| state.raid.map(|member| (id, member)))
            .collect();
        if self.raid.active.is_some() {
            self.record_participants(&members)?;
            self.hit_objective(site, &members)?;
            self.advance_phase(site, now)?;
        } else if self.raid.damage > 0
            && let Some(last) = self.raid.last
            && now > last.at
            && (now - last.at).is_multiple_of(u64::from(RECOVERY_TICKS_PER_POINT))
        {
            self.raid.damage -= 1;
            self.settle_aftermath(site)?;
        }
        self.advance_marches(site, now)?;
        if time_of_day(now) == TRIGGER_TIME {
            if self.raid_eligible(now) && raid_night(self.content.rng_seed(), now) {
                self.commit_incident(now)?;
            }
            // Without a raid, the evening's tracks and preparations expire (5.1.69).
            self.raid.scouted.clear();
            if self.raid.active.is_none() {
                self.raid.readiness = 0;
            }
        }
        Ok(())
    }

    /// Players on the threat tables of this incident's living raiders take part in it; those
    /// who caused threat (damage, or healing a defender) earn the participation reward.
    fn record_participants(
        &mut self,
        members: &[(CreatureId, RaidMember)],
    ) -> Result<(), ZoneError> {
        let Some(incident) = self.raid.active.as_mut() else {
            return Ok(());
        };
        let mut defenders = Vec::new();
        for (id, member) in members {
            if member.incident != incident.id {
                continue;
            }
            let Some(state) = self.creatures.get(id) else {
                continue;
            };
            for entry in &state.threat {
                let EntityRef::Player(player) = entry.entity else {
                    continue;
                };
                if let Err(at) = incident.participants.binary_search(&player)
                    && incident.participants.len() < crate::MAX_PLAYERS_PER_ZONE
                {
                    incident.participants.insert(at, player);
                }
                if entry.threat > 0 {
                    defenders.push(player);
                }
            }
        }
        let incident = incident.id;
        defenders.sort_unstable();
        defenders.dedup();
        for player in defenders {
            self.reward_participant(player, incident)?;
        }
        Ok(())
    }

    /// Grants the participation reward for `incident` to `player_id` unless it already has it
    /// (5.1.52): once per incident and character, in the zone and in the durable receipts.
    fn reward_participant(&mut self, player_id: PlayerId, incident: u32) -> Result<(), ZoneError> {
        let key = crate::RewardKey::RaidParticipation { incident };
        let Err(at) = self.raid.rewarded.binary_search(&player_id) else {
            return Ok(());
        };
        if self.raid.rewarded.len() >= crate::MAX_PLAYERS_PER_ZONE
            || !crate::rewards::allowed(&self.rewards, player_id, key)
            || !self.players.contains_key(&player_id)
        {
            return Ok(());
        }
        self.raid.rewarded.insert(at, player_id);
        crate::rewards::record(&mut self.rewards, player_id, key)?;
        if let Some(player) = self.players.get_mut(&player_id) {
            player.copper = player.copper.saturating_add(REWARD_COPPER);
        }
        self.grant_experience(player_id, REWARD_EXPERIENCE)
    }

    /// Closes the aid tasks the objective's restoration has reached; a guard whose task
    /// closes returns to its post.
    fn settle_aftermath(&mut self, site: &RaidSite) -> Result<(), ZoneError> {
        let before = self.raid.aftermath;
        for index in 0..site.aid.len() {
            if site.aid_settled(index, self.raid.damage) {
                self.raid.aftermath &= !(1 << index);
            }
        }
        if self.raid.aftermath != before {
            self.place_npcs(false)?;
        }
        Ok(())
    }

    /// The `RaidAid` intent (5.1.51): tends or mends open aid task `task` within reach. The task
    /// closes, the barn regains [`AID_RESTORE_POINTS`], and the helper earns the incident's
    /// participation reward once.
    pub(crate) fn aid_raid(
        &mut self,
        player_id: PlayerId,
        task: u8,
    ) -> Result<Option<(ErrorCode, Option<EntityRef>)>, ZoneError> {
        let content = Arc::clone(&self.content);
        let (Some(site), Some(incident)) = (content.raid(), self.raid.current_incident()) else {
            return Ok(Some((ErrorCode::InvalidTarget, None)));
        };
        let Some(spot) = site.aid.get(usize::from(task)).map(|task| task.at) else {
            return Ok(Some((ErrorCode::InvalidTarget, None)));
        };
        if self.raid.aftermath & (1 << task) == 0 {
            return Ok(Some((ErrorCode::InvalidTarget, None)));
        }
        let at = self.player_position(player_id)?;
        if crate::ai::distance_squared([at.x, at.z], spot) > i64::from(AID_REACH_UNITS).pow(2) {
            return Ok(Some((ErrorCode::OutOfRange, None)));
        }
        self.raid.aftermath &= !(1 << task);
        self.raid.damage = self.raid.damage.saturating_sub(AID_RESTORE_POINTS);
        self.settle_aftermath(site)?;
        self.place_npcs(false)?;
        self.reward_participant(player_id, incident)?;
        Ok(None)
    }

    /// The `RaidPrepare` intent (5.1.59–5.1.62): one preparation step on a forewarned evening.
    /// Investigating needs the tracks in reach; a report needs the lookout in reach and this
    /// evening's tracks read; a brace needs the guards warned and its spot in reach. Each step
    /// counts once: a repeat is refused. No step earns XP or money.
    pub(crate) fn prepare_raid(
        &mut self,
        player_id: PlayerId,
        prep: RaidPrep,
    ) -> Result<Option<(ErrorCode, Option<EntityRef>)>, ZoneError> {
        let content = Arc::clone(&self.content);
        let Some(site) = content.raid() else {
            return Ok(Some((ErrorCode::InvalidTarget, None)));
        };
        if !self.forewarned(self.tick) {
            return Ok(Some((ErrorCode::InvalidTarget, None)));
        }
        let at = self.player_position(player_id)?;
        let here = [at.x, at.z];
        let within = |spot: [i32; 2], reach: i32| {
            crate::ai::distance_squared(here, spot) <= i64::from(reach).pow(2)
        };
        let notice = match prep {
            RaidPrep::Investigate => {
                let Err(index) = self.raid.scouted.binary_search(&player_id) else {
                    return Ok(Some((ErrorCode::InvalidTarget, None)));
                };
                if !within(site.tracks, TRACKS_REACH_UNITS) {
                    return Ok(Some((ErrorCode::OutOfRange, None)));
                }
                if self.raid.scouted.len() >= crate::MAX_PLAYERS_PER_ZONE {
                    return Ok(Some((ErrorCode::InvalidTarget, None)));
                }
                self.raid.scouted.insert(index, player_id);
                RaidNotice::Tracks
            }
            RaidPrep::Report => {
                let lookout = EntityRef::Npc(site.lookout);
                if self.raid.readiness & READINESS_WARNED != 0
                    || self.raid.scouted.binary_search(&player_id).is_err()
                {
                    return Ok(Some((ErrorCode::InvalidTarget, Some(lookout))));
                }
                let post = self.npc_placement(site.lookout).map(|place| place.position);
                if !post.is_some_and(|post| within(post, crate::quest::QUEST_REACH_UNITS)) {
                    return Ok(Some((ErrorCode::OutOfRange, Some(lookout))));
                }
                self.raid.readiness |= READINESS_WARNED;
                RaidNotice::Warned
            }
            RaidPrep::Brace(index) => {
                let Some(&spot) = site.braces.get(usize::from(index)) else {
                    return Ok(Some((ErrorCode::InvalidTarget, None)));
                };
                let bit = 1 << (index + 1);
                if self.raid.readiness & READINESS_WARNED == 0 || self.raid.readiness & bit != 0 {
                    return Ok(Some((ErrorCode::InvalidTarget, None)));
                }
                if !within(spot, BRACE_REACH_UNITS) {
                    return Ok(Some((ErrorCode::OutOfRange, None)));
                }
                self.raid.readiness |= bit;
                RaidNotice::Braced
            }
        };
        self.notify(player_id, ZoneEvent::Raid { notice });
        Ok(None)
    }

    /// Idle raiders at the objective hit it on their swing timer, for their damage range's
    /// midpoint.
    fn hit_objective(
        &mut self,
        site: &RaidSite,
        members: &[(CreatureId, RaidMember)],
    ) -> Result<(), ZoneError> {
        let content = Arc::clone(&self.content);
        for &(id, member) in members {
            if member.leg != RaidLeg::Assault {
                continue;
            }
            let (_, template) = Self::creature_content(&content, id)?;
            let Some(state) = self.creatures.get(&id) else {
                continue;
            };
            if !matches!(state.ai, CreatureAi::Idle { .. })
                || state.swing_timer > 0
                || state.cast.is_some()
                || crate::aura::has_kind(&state.auras, crate::AuraKind::Stun)
            {
                continue;
            }
            let at = self.unit_position(EntityRef::Creature(id))?;
            if site.distance_squared([at[0], at[2]]) > i64::from(OBJECTIVE_REACH_UNITS).pow(2) {
                continue;
            }
            let [low, high] = template.damage_at(state.level);
            let amount = prepared_hit(
                (u32::from(low) + u32::from(high)).div_ceil(2),
                self.raid.readiness,
            );
            if let Some(state) = self.creatures.get_mut(&id) {
                state.swing_timer = template.swing_ticks;
            }
            self.raid.damage = (self.raid.damage + amount).min(OBJECTIVE_INTEGRITY);
            let source = EntityRef::Creature(id);
            self.notify_observers(
                &[source],
                &std::collections::BTreeSet::new(),
                ZoneEvent::AttackPerformed {
                    source,
                    target: None,
                },
            );
        }
        Ok(())
    }

    /// Loss when the integrity ran out; otherwise the next phase, or a defence after the last.
    fn advance_phase(&mut self, site: &RaidSite, now: u64) -> Result<(), ZoneError> {
        let Some(incident) = &self.raid.active else {
            return Ok(());
        };
        if self.raid.damage >= OBJECTIVE_INTEGRITY {
            self.resolve(now, RaidOutcome::Lost)?;
            // The farm's aftermath (5.1.50): breaches open and the guards lie injured.
            self.raid.aftermath = site.aid_mask();
            return self.place_npcs(false);
        }
        if now
            < incident
                .phase_started
                .saturating_add(u64::from(PHASE_TICKS))
        {
            return Ok(());
        }
        if incident.phase >= RAID_PHASES {
            return self.resolve(now, RaidOutcome::Defended);
        }
        if let Some(incident) = self.raid.active.as_mut() {
            incident.phase += 1;
            incident.phase_started = now;
        }
        self.commit_wave(now)
    }

    /// Resolves the running incident once; its living raiders withdraw.
    fn resolve(&mut self, now: u64, outcome: RaidOutcome) -> Result<(), ZoneError> {
        let Some(incident) = self.raid.active.take() else {
            return Ok(());
        };
        self.raid.last = Some(RaidResolution {
            incident: incident.id,
            at: now,
            outcome,
        });
        // The preparations served this incident and retire with it (5.1.69); nearby players hear
        // truthfully what came of them (5.1.60).
        if self.raid.readiness != 0 {
            self.raid.readiness = 0;
            let notice = match outcome {
                RaidOutcome::Defended => RaidNotice::Held,
                RaidOutcome::Lost => RaidNotice::Fell,
            };
            if let Some([x, z]) = self.content.raid().map(RaidSite::centre) {
                let near: Vec<PlayerId> = self
                    .interest
                    .near(x, z, SIGNAL_RADIUS_UNITS)
                    .into_iter()
                    .filter_map(|entity| match entity {
                        EntityRef::Player(id) => Some(id),
                        _ => None,
                    })
                    .collect();
                for player in near {
                    self.notify(player, ZoneEvent::Raid { notice });
                }
            }
        }
        for state in self.creatures.values_mut() {
            if let Some(member) = state.raid.as_mut()
                && member.incident == incident.id
                && member.leg == RaidLeg::Assault
            {
                member.leg = RaidLeg::Retreat;
                member.waypoint = 0;
                member.since = now;
            }
        }
        Ok(())
    }

    /// Arrivals at the camp, the end of a camp stay and stuck retreats.
    fn advance_marches(&mut self, site: &RaidSite, now: u64) -> Result<(), ZoneError> {
        let ids: Vec<CreatureId> = self
            .creatures
            .iter()
            .filter(|(_, state)| state.is_alive() && state.raid.is_some())
            .map(|(&id, _)| id)
            .collect();
        for id in ids {
            let Some(state) = self.creatures.get(&id) else {
                continue;
            };
            let (Some(member), idle) = (state.raid, matches!(state.ai, CreatureAi::Idle { .. }))
            else {
                continue;
            };
            let elapsed = now.saturating_sub(member.since);
            let retire = match member.leg {
                RaidLeg::Assault => false,
                RaidLeg::Retreat => {
                    let (approach, slot) = site.slot_of(id).unwrap_or((0, 0));
                    if usize::from(member.waypoint) >= site.leg_len(RaidLeg::Retreat, approach) {
                        if let Some(state) = self.creatures.get_mut(&id)
                            && let Some(member) = state.raid.as_mut()
                        {
                            member.leg = RaidLeg::Camp;
                            member.waypoint = 0;
                            member.anchor = site.camp_spot(approach, slot);
                            member.since = now;
                        }
                        false
                    } else {
                        idle && elapsed >= u64::from(RETREAT_TIMEOUT_TICKS) && !self.watched(id)?
                    }
                }
                RaidLeg::Camp => idle && elapsed >= u64::from(CAMP_STAY_TICKS),
            };
            if retire {
                self.retire_raider(id, now)?;
            }
        }
        Ok(())
    }

    fn watched(&self, id: CreatureId) -> Result<bool, ZoneError> {
        let at = self.unit_position(EntityRef::Creature(id))?;
        Ok(self
            .interest
            .near(at[0], at[2], crate::INTEREST_RADIUS_UNITS)
            .into_iter()
            .any(|entity| matches!(entity, EntityRef::Player(_))))
    }

    /// A raider leaves the world without a corpse and stays gone until a later incident
    /// commits its slot again.
    fn retire_raider(&mut self, id: CreatureId, now: u64) -> Result<(), ZoneError> {
        let entity = EntityRef::Creature(id);
        self.world.remove_body(body_id(entity));
        if self.interest.remove(entity) {
            self.interest_work.bucket_removes += 1;
        }
        for player in self.players.values_mut() {
            if player.target == Some(entity) {
                player.target = None;
            }
        }
        if let Some(state) = self.creatures.get_mut(&id) {
            state.life = Life::Despawned { died_at: now };
            state.health = 0;
            state.ai = CreatureAi::Idle {
                timer: 0,
                destination: None,
            };
            state.threat.clear();
            state.swing_timer = 0;
            state.combat_timer = 0;
            state.tapped_by = None;
            state.loot = None;
            state.cast = None;
            state.auras.clear();
            state.encounter = None;
            state.raid = None;
            state.reset_abilities();
        }
        Ok(())
    }

    /// Step 2 for a marching raider (idle on its assault or retreat leg): the velocity and facing
    /// toward its next waypoint, around static geometry; standing at the objective, it faces
    /// it. `None` for any other creature.
    pub(crate) fn march(
        &mut self,
        id: CreatureId,
        centre: [f32; 3],
        half: crate::physics::Vec3i,
    ) -> Result<Option<Steering>, ZoneError> {
        let Some(member) = self.creatures.get(&id).and_then(|state| state.raid) else {
            return Ok(None);
        };
        let content = Arc::clone(&self.content);
        let (Some(site), Some(state)) = (content.raid(), self.creatures.get(&id)) else {
            return Ok(None);
        };
        if member.leg == RaidLeg::Camp || !matches!(state.ai, CreatureAi::Idle { .. }) {
            return Ok(None);
        }
        let (approach, slot) = site.slot_of(id).unwrap_or((0, 0));
        #[allow(clippy::cast_possible_truncation)]
        let here = [centre[0].round() as i32, centre[2].round() as i32];
        let mut waypoint = member.waypoint;
        let mut goal = site.waypoint(member.leg, approach, slot, waypoint);
        let legs = site.leg_len(member.leg, approach);
        // Waypoints on the way pass loosely; the last one, its own spot, exactly.
        let radius = |index: u8| {
            if usize::from(index) + 1 >= legs {
                crate::ARRIVAL_RADIUS_UNITS
            } else {
                WAYPOINT_RADIUS_UNITS
            }
        };
        while let Some(point) = goal
            && crate::ai::distance_squared(here, point) <= i64::from(radius(waypoint)).pow(2)
        {
            waypoint = waypoint.saturating_add(1);
            goal = site.waypoint(member.leg, approach, slot, waypoint);
        }
        if let Some(state) = self.creatures.get_mut(&id)
            && let Some(member) = state.raid.as_mut()
        {
            member.waypoint = waypoint;
            // A fight on the march leashes to where it began.
            member.anchor = here;
        }
        let Some(goal) = goal else {
            if member.leg != RaidLeg::Assault {
                // Arrived at the camp; the bookkeeping turns it into a stay.
                return Ok(Some(([0, 0], None)));
            }
            // At the objective: stand and face it, closing in again when pushed out of reach.
            let [min, max] = site.footprint;
            let nearest = [here[0].clamp(min[0], max[0]), here[1].clamp(min[1], max[1])];
            let (dx, dz) = crate::ai::offset(here, nearest);
            let velocity = if site.distance_squared(here) > i64::from(STAND_OFF_UNITS).pow(2) {
                crate::ai::steer(dx, dz, crate::CREATURE_WALK_SPEED_SUBUNITS_PER_TICK)
            } else {
                [0, 0]
            };
            return Ok(Some((velocity, crate::trig::yaw_from_vector(dx, dz))));
        };
        let toward = self.route_waypoint(centre, half, goal, 0)?.unwrap_or(goal);
        let (dx, dz) = crate::ai::offset(here, toward);
        let speed = match member.leg {
            RaidLeg::Assault => crate::CREATURE_WALK_SPEED_SUBUNITS_PER_TICK,
            _ => crate::CREATURE_RUN_SPEED_SUBUNITS_PER_TICK,
        };
        Ok(Some((
            crate::ai::steer(dx, dz, speed),
            crate::trig::yaw_from_vector(dx, dz),
        )))
    }

    /// The raid's signals for `viewer` at `at` (5.1.53, 5.1.61).
    pub(crate) fn raid_view(&self, viewer: PlayerId, at: [i32; 2]) -> Option<RaidView> {
        let site = self.content.raid()?;
        let phase = self
            .raid
            .active
            .as_ref()
            .map_or(0, |incident| incident.phase);
        let signs = phase == 0 && self.forewarned(self.tick);
        if (phase == 0 && self.raid.damage == 0 && !signs)
            || site.distance_squared(at) > i64::from(SIGNAL_RADIUS_UNITS).pow(2)
        {
            return None;
        }
        let left = OBJECTIVE_INTEGRITY - self.raid.damage;
        let integrity = u8::try_from((left * 100).div_ceil(OBJECTIVE_INTEGRITY)).unwrap_or(100);
        Some(RaidView {
            phase,
            integrity,
            aid: self.raid.aftermath,
            signs,
            readiness: self.raid.readiness,
            scouted: signs && self.raid.scouted.binary_search(&viewer).is_ok(),
        })
    }
}
