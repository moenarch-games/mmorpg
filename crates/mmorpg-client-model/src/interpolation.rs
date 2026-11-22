//! Snapshot interpolation both clients draw units with (#1058, Atlas 6.1.i1058): a bounded,
//! ordered history of received projections, sampled [`INTERPOLATION_DELAY_TICKS`] behind the
//! newest. Presentation only: it never extrapolates, runs physics or changes a record.
//!
//! Each pushed projection is indexed by entity once, so a sample walks the two bracketing
//! projections side by side: the work is linear in their entities ([`Sample::visited`]),
//! never a lookup per pair, and a warm sample allocates nothing.

use std::{borrow::Borrow, collections::VecDeque};

use mmorpg_core::{EntityRef, TICK_HZ, ZoneSnapshot};

/// How far behind the newest projection both clients draw units (owner, 2026-11-22): the
/// zone's hit rewind assumes the same delay ([`mmorpg_core::CLIENT_INTERPOLATION_TICKS`]).
pub const INTERPOLATION_DELAY_TICKS: u16 = mmorpg_core::CLIENT_INTERPOLATION_TICKS;
/// Projections kept; older ones are dropped first.
const CAPACITY: usize = 32;
/// After a longer gap the history restarts instead of interpolating a stale journey.
const STALL_TICKS: u64 = 30 * TICK_HZ as u64;
const YAW_STEPS: f64 = 65_536.;

/// Where one unit is drawn: its position in units and facing in yaw steps, both possibly
/// between ticks.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pose {
    pub position: [f64; 3],
    /// Yaw steps in `[0, 65 536)`: 0 faces +Z, turning toward +X.
    pub facing: f64,
}

impl Pose {
    /// The facing in radians within one turn.
    #[must_use]
    pub fn yaw_radians(&self) -> f64 {
        (self.facing / YAW_STEPS).rem_euclid(1.) * std::f64::consts::TAU
    }
}

/// One sample: the projection whose records are drawn and how many entity samples it visited.
/// The poses written beside it are parallel to `snapshot.entities`.
#[derive(Clone, Copy, Debug)]
pub struct Sample<'a> {
    pub snapshot: &'a ZoneSnapshot,
    /// Entity records of the bracketing projections walked: linear in projected entities.
    pub visited: usize,
}

struct Frame<S> {
    snapshot: S,
    /// Indices of `snapshot.entities` in entity order.
    order: Vec<u16>,
}

/// The bounded history of one projection stream, generic over how a client holds a
/// projection (owned natively, shared with the projection store in the browser).
pub struct SnapshotHistory<S> {
    frames: VecDeque<Frame<S>>,
    /// Entity orders of dropped frames, reused by the next push.
    spare: Vec<Vec<u16>>,
}

impl<S> Default for SnapshotHistory<S> {
    fn default() -> Self {
        Self {
            frames: VecDeque::with_capacity(CAPACITY + 1),
            spare: Vec::new(),
        }
    }
}

impl<S: Borrow<ZoneSnapshot>> SnapshotHistory<S> {
    /// Forgets the stream: call on reconnect, handoff or any authority change.
    pub fn reset(&mut self) {
        let spare = &mut self.spare;
        spare.extend(self.frames.drain(..).map(|frame| frame.order));
    }

    /// The newest projection.
    #[must_use]
    pub fn latest(&self) -> Option<&ZoneSnapshot> {
        self.frames.back().map(|frame| frame.snapshot.borrow())
    }

    /// Admits a projection newer than the newest; `false`, changing nothing, for a stale or
    /// repeated tick. A different zone, content revision or viewer starts a new stream, and so
    /// does a projection more than 30 s after the newest.
    pub fn push(&mut self, snapshot: S) -> bool {
        let projection = snapshot.borrow();
        if let Some(latest) = self.latest() {
            let same_stream = projection.zone_id == latest.zone_id
                && projection.content_revision == latest.content_revision
                && projection.viewer_id == latest.viewer_id;
            if same_stream && projection.tick <= latest.tick {
                return false;
            }
            if !same_stream || projection.tick - latest.tick > STALL_TICKS {
                self.reset();
            }
        }
        let mut order = self.spare.pop().unwrap_or_default();
        order.clear();
        order.extend(
            (0..projection.entities.len()).map(|index| u16::try_from(index).unwrap_or(u16::MAX)),
        );
        let entities = &projection.entities;
        order.sort_unstable_by_key(|&index| entities[usize::from(index)].entity());
        self.frames.push_back(Frame { snapshot, order });
        if self.frames.len() > CAPACITY
            && let Some(dropped) = self.frames.pop_front()
        {
            self.spare.push(dropped.order);
        }
        true
    }

    /// Samples the stream [`INTERPOLATION_DELAY_TICKS`] behind the newest projection,
    /// `elapsed_ticks` (time since it arrived, or the local clock's fraction of a tick) closer
    /// to it, never past it. Writes the poses of the returned projection's records into
    /// `poses`; `None` before the first projection.
    ///
    /// Units move between the two projections around that point and turn along the shorter
    /// arc; one only in the older appears and vanishes at authoritative ticks.
    pub fn sample(&self, elapsed_ticks: f64, poses: &mut Vec<Pose>) -> Option<Sample<'_>> {
        let delay = f64::from(INTERPOLATION_DELAY_TICKS);
        let elapsed = if elapsed_ticks.is_nan() {
            0.
        } else {
            elapsed_ticks.clamp(0., delay)
        };
        self.sample_behind(delay - elapsed, poses)
    }

    /// Samples `lag` ticks (non-negative) behind the newest projection.
    fn sample_behind(&self, lag: f64, poses: &mut Vec<Pose>) -> Option<Sample<'_>> {
        let newest = self.frames.back()?.snapshot.borrow().tick;
        // Subtract whole ticks before converting, preserving precision at large ticks.
        let age = |frame: &Frame<S>| (newest - frame.snapshot.borrow().tick) as f64;
        // The newest frame at least `lag` old, or the oldest when the history is shorter.
        let before = self
            .frames
            .iter()
            .rposition(|frame| age(frame) >= lag)
            .unwrap_or(0);
        let from = &self.frames[before];
        let records = &from.snapshot.borrow().entities;
        poses.clear();
        poses.extend(records.iter().map(|record| Pose {
            position: record.position.map(f64::from),
            facing: f64::from(record.facing),
        }));
        let mut visited = records.len();
        let after = self.frames.get(before + 1);
        if let Some(to) = after.filter(|to| age(from) > lag && age(to) < lag) {
            let alpha = (age(from) - lag) / (age(from) - age(to));
            visited += interpolate(from, to, alpha, poses);
        }
        Some(Sample {
            snapshot: from.snapshot.borrow(),
            visited,
        })
    }
}

/// Moves `poses` (of `from`'s records) `alpha` of the way toward `to`'s, walking both entity
/// orders once; returns how many of `to`'s records it visited.
fn interpolate<S: Borrow<ZoneSnapshot>>(
    from: &Frame<S>,
    to: &Frame<S>,
    alpha: f64,
    poses: &mut [Pose],
) -> usize {
    let (earlier, later) = (&from.snapshot.borrow().entities, &to.snapshot.borrow().entities);
    let key = |records: &[mmorpg_core::EntitySnapshot], index: u16| -> EntityRef {
        records[usize::from(index)].entity()
    };
    let mut next = to.order.iter().peekable();
    let mut visited = 0;
    for &index in &from.order {
        let entity = key(earlier, index);
        while next
            .next_if(|&&candidate| key(later, candidate) < entity)
            .is_some()
        {
            visited += 1;
        }
        let Some(&&matched) = next.peek().filter(|&&&candidate| key(later, candidate) == entity)
        else {
            continue;
        };
        let target = &later[usize::from(matched)];
        let pose = &mut poses[usize::from(index)];
        for (axis, value) in pose.position.iter_mut().enumerate() {
            *value += (f64::from(target.position[axis]) - *value) * alpha;
        }
        // The wrapping difference, read as signed, is the shorter arc.
        let start = earlier[usize::from(index)].facing;
        let arc = f64::from(target.facing.wrapping_sub(start).cast_signed());
        pose.facing = (f64::from(start) + arc * alpha).rem_euclid(YAW_STEPS);
    }
    visited + next.count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::snapshot;
    use mmorpg_core::{EntityFlags, EntityKind, EntitySnapshot, ZoneId};

    fn player(id: u32, x: i32, facing: u16) -> EntitySnapshot {
        EntitySnapshot {
            kind: EntityKind::Player,
            id,
            appearance: 0,
            position: [x, 90, 0],
            velocity: [0; 3],
            facing,
            level: 1,
            health_percent: 100,
            flags: EntityFlags::default(),
        }
    }

    fn projection(tick: u64, entities: Vec<EntitySnapshot>) -> ZoneSnapshot {
        ZoneSnapshot {
            entities,
            ..snapshot(1, tick)
        }
    }

    /// A projection with one player at 12 units per tick.
    fn moving(tick: u64) -> ZoneSnapshot {
        projection(tick, vec![player(1, tick as i32 * 12, 0)])
    }

    fn sample_behind(history: &SnapshotHistory<ZoneSnapshot>, lag: f64) -> Vec<Pose> {
        let mut poses = Vec::new();
        history.sample_behind(lag, &mut poses);
        poses
    }

    #[test]
    fn draws_two_ticks_behind_the_newest_and_closes_the_gap_while_it_stalls() {
        assert_eq!(INTERPOLATION_DELAY_TICKS, 2);
        let mut history = SnapshotHistory::default();
        for tick in 0..=10 {
            history.push(moving(tick));
        }
        let mut poses = Vec::new();
        let sample = history.sample(0., &mut poses).unwrap();
        assert_eq!(sample.snapshot.tick, 8);
        assert_eq!(poses[0].position[0], 96.);
        history.sample(0.5, &mut poses).unwrap();
        assert_eq!(poses[0].position[0], 102.);
        // A stalled stream reaches the newest projection and holds it.
        for elapsed in [2., 7., f64::INFINITY] {
            history.sample(elapsed, &mut poses).unwrap();
            assert_eq!(poses[0].position[0], 120., "{elapsed}");
        }
        history.sample(f64::NAN, &mut poses).unwrap();
        assert_eq!(poses[0].position[0], 96.);
    }

    #[test]
    fn interpolates_irregular_ticks_and_holds_during_packet_loss() {
        let mut history = SnapshotHistory::default();
        history.push(moving(10));
        history.push(moving(13));
        // Tick 11.5 of 10..13.
        assert_eq!(sample_behind(&history, 1.5)[0].position, [138., 90., 0.]);
        // Older than the history: the oldest projection.
        assert_eq!(sample_behind(&history, 90.)[0].position, [120., 90., 0.]);
        assert!(!history.push(moving(12)));
        assert!(!history.push(moving(13)));
        assert_eq!(sample_behind(&history, 0.)[0].position, [156., 90., 0.]);
    }

    #[test]
    fn interpolates_facing_along_the_shorter_arc() {
        let mut history = SnapshotHistory::default();
        history.push(projection(0, vec![player(1, 0, 65_000)]));
        history.push(projection(2, vec![player(1, 0, 500)]));
        assert_eq!(sample_behind(&history, 1.)[0].facing, 65_518.);
        assert_eq!(sample_behind(&history, 1.5)[0].facing, 65_259.);
        history.reset();
        history.push(projection(0, vec![player(1, 0, 500)]));
        history.push(projection(2, vec![player(1, 0, 65_000)]));
        assert_eq!(sample_behind(&history, 1.)[0].facing, 65_518.);
        assert_eq!(sample_behind(&history, 0.)[0].facing, 65_000.);
        let pose = Pose {
            position: [0.; 3],
            facing: 16_384.,
        };
        assert!((pose.yaw_radians() - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    }

    #[test]
    fn units_appear_and_vanish_at_authoritative_ticks_whatever_their_order() {
        let mut history = SnapshotHistory::default();
        history.push(projection(1, vec![player(2, 10, 0), player(1, 0, 0)]));
        history.push(projection(2, vec![player(3, 20, 0), player(1, 12, 0)]));
        let mut poses = Vec::new();
        let sample = history.sample_behind(0.5, &mut poses).unwrap();
        let ids: Vec<_> = sample.snapshot.entities.iter().map(|unit| unit.id).collect();
        assert_eq!(ids, [2, 1]);
        assert_eq!(poses[0].position[0], 10., "the vanishing unit holds");
        assert_eq!(poses[1].position[0], 6., "the shared unit moves");
        let sample = history.sample_behind(0., &mut poses).unwrap();
        let ids: Vec<_> = sample.snapshot.entities.iter().map(|unit| unit.id).collect();
        assert_eq!(ids, [3, 1]);
    }

    #[test]
    fn a_new_stream_never_blends_with_the_old_one() {
        let mut history = SnapshotHistory::default();
        history.push(moving(5));
        let other_zone = ZoneSnapshot {
            zone_id: ZoneId::new(2),
            ..moving(1)
        };
        assert!(history.push(other_zone));
        assert_eq!(history.latest().unwrap().zone_id, ZoneId::new(2));
        assert_eq!(sample_behind(&history, 2.)[0].position[0], 12.);
        let other_viewer = ZoneSnapshot {
            viewer_id: 9,
            ..moving(0)
        };
        assert!(history.push(other_viewer));
        history.reset();
        assert!(history.latest().is_none());
        assert!(history.sample(0., &mut Vec::new()).is_none());
    }

    #[test]
    fn bounds_history_handles_large_ticks_and_restarts_after_a_stall() {
        let mut history = SnapshotHistory::default();
        for tick in 0..40 {
            history.push(moving(tick));
        }
        // 32 kept: the oldest is tick 8.
        assert_eq!(sample_behind(&history, 1e9)[0].position[0], 8. * 12.);
        let stalled = 40 + 30 * u64::from(TICK_HZ) + 1;
        history.push(moving(stalled));
        assert_eq!(
            sample_behind(&history, 1.)[0].position[0],
            stalled as f64 * 12.
        );
        history.reset();
        let tick = u64::MAX - 1_615;
        history.push(projection(tick, vec![player(1, 0, 0)]));
        history.push(projection(tick + 2, vec![player(1, 24, 0)]));
        assert_eq!(sample_behind(&history, 1.)[0].position[0], 12.);
    }

    #[test]
    fn sampling_visits_each_record_of_the_bracketing_projections_once() {
        let mut history = SnapshotHistory::default();
        let crowd = |tick: u64, count: u32| {
            projection(
                tick,
                (0..count)
                    .rev()
                    .map(|id| player(id, tick as i32, 0))
                    .collect(),
            )
        };
        for count in [16, 64] {
            history.reset();
            history.push(crowd(1, count));
            history.push(crowd(2, count + 1));
            let mut poses = Vec::new();
            let sample = history.sample_behind(0.5, &mut poses).unwrap();
            assert_eq!(sample.visited, 2 * count as usize + 1);
            assert!(poses.iter().all(|pose| pose.position[0] == 1.5));
        }
    }
}
