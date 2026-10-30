//! Headless autopilot for the two-host Seam Test journey (#145): it drives a claimed
//! character through the real session the window uses, so the smoke exercises the
//! client's own redirect following.
//!
//! Starting in a Seam Test zone, the character walks to the zone's waystone (unlocking it),
//! then across the seam into the neighbouring zone and on to that zone's stone, then
//! teleports back to the first stone. Each zone change is a redirect notice the session
//! follows to the other host.

use crate::{
    ClientError,
    network::{ClientSession, SessionError},
};
use mmorpg_core::{
    EntityKind, ZoneCommand, ZoneId, ZoneSnapshot, content_for_zone, trig, waystone,
};
use mmorpg_protocol::RedirectNotice;

/// Close enough to a waystone to unlock and use it (reach is 8 m).
const STONE_REACH_UNITS: i64 = 500;
/// How far an arrival may be from where the character crossed or the stone it chose.
const ARRIVAL_TOLERANCE_UNITS: i32 = 400;
/// Snapshots to reach any one goal before the journey fails (20 Hz: one minute).
const MAX_SNAPSHOTS: u32 = 1_200;

/// What the journey went through.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Journey {
    /// The zone walked from and the zone walked into through the seam.
    pub walked: (ZoneId, ZoneId),
    /// The zone teleported from and the zone of the stone arrived at.
    pub teleported: (ZoneId, ZoneId),
    /// Feet XZ after each arrival, in the arrival zone's units.
    pub arrivals: [[i32; 2]; 2],
}

fn feet(snapshot: &ZoneSnapshot) -> Result<[i32; 2], ClientError> {
    let own = snapshot
        .entities
        .iter()
        .find(|entity| entity.kind == EntityKind::Player && entity.id == snapshot.viewer_id)
        .ok_or("the projection lacks the player's own unit")?;
    Ok([own.position[0], own.position[2]])
}

fn distance(a: [i32; 2], b: [i32; 2]) -> i64 {
    let [dx, dz] = [i64::from(b[0] - a[0]), i64::from(b[1] - a[1])];
    (dx * dx + dz * dz).isqrt()
}

/// What one snapshot told the journey.
enum Step {
    Projection(Box<ZoneSnapshot>),
    Redirect(Box<RedirectNotice>),
}

async fn step(session: &ClientSession) -> Result<Step, ClientError> {
    match session.receive_snapshot().await {
        Ok(snapshot) => Ok(Step::Projection(Box::new(snapshot))),
        Err(SessionError::Redirected(notice)) => Ok(Step::Redirect(notice)),
        Err(error) => Err(error.into()),
    }
}

/// Walks toward `target` until within `reach`, or until redirected when `reach` is `None`.
async fn walk(
    session: &mut ClientSession,
    target: [i32; 2],
    reach: Option<i64>,
) -> Result<Option<Box<RedirectNotice>>, ClientError> {
    for _ in 0..MAX_SNAPSHOTS {
        let snapshot = match step(session).await? {
            Step::Projection(snapshot) => snapshot,
            Step::Redirect(notice) if reach.is_none() => return Ok(Some(notice)),
            Step::Redirect(_) => return Err("redirected before reaching the goal".into()),
        };
        let at = feet(&snapshot)?;
        if reach.is_some_and(|reach| distance(at, target) <= reach) {
            session.send_move(0, 0, 0)?;
            return Ok(None);
        }
        let facing = trig::yaw_from_vector(target[0] - at[0], target[1] - at[1]).unwrap_or(0);
        session.send_move(1, 0, facing)?;
    }
    Err(format!(
        "did not reach {target:?} in zone {}",
        session.zone_id().get()
    )
    .into())
}

/// Follows `notice` and returns the first live position in its destination.
async fn arrive(
    session: &mut ClientSession,
    notice: &RedirectNotice,
) -> Result<[i32; 2], ClientError> {
    session.follow(notice).await?;
    for _ in 0..MAX_SNAPSHOTS {
        if let Step::Projection(snapshot) = step(session).await? {
            if snapshot.zone_id == notice.destination {
                return feet(&snapshot);
            }
        } else {
            return Err("redirected again before arriving".into());
        }
    }
    Err("the character never went live at the destination".into())
}

/// Runs the journey from the session's Seam Test zone; the session holds a claimed
/// character.
pub async fn seam_test_journey(session: &mut ClientSession) -> Result<Journey, ClientError> {
    let origin = session.zone_id();
    let content = content_for_zone(origin);
    let home = *content
        .waystones()
        .first()
        .ok_or("the starting zone has no waystone")?;
    let home_site = waystone(home).ok_or("unknown waystone")?;
    let seam = *content
        .handoff_corridors()
        .first()
        .ok_or("the starting zone has no seam")?;
    let crossing = [
        (seam.crossing.min[0] + seam.crossing.max[0]) / 2,
        (seam.crossing.min[1] + seam.crossing.max[1]) / 2,
    ];

    // Unlock the home stone, then walk through the seam.
    walk(session, home_site.stone, Some(STONE_REACH_UNITS)).await?;
    let notice = walk(session, crossing, None)
        .await?
        .ok_or("the seam did not redirect")?;
    let walked = (notice.zone_id, notice.destination);
    let crossed = arrive(session, &notice).await?;
    let expected = [crossing[0] + seam.offset[0], crossing[1] + seam.offset[2]];
    if (crossed[0] - expected[0]).abs() > ARRIVAL_TOLERANCE_UNITS * 4 {
        return Err(format!("the walk continued at {crossed:?}, far from {expected:?}").into());
    }

    // Unlock the far stone, then travel home by waystone.
    let far = *content_for_zone(seam.neighbor)
        .waystones()
        .first()
        .ok_or("the neighbouring zone has no waystone")?;
    let far_site = waystone(far).ok_or("unknown waystone")?;
    walk(session, far_site.stone, Some(STONE_REACH_UNITS)).await?;
    let mut waited = 0;
    let notice = loop {
        match step(session).await? {
            // Once a second, in case a datagram was lost; the departure holds meanwhile.
            Step::Projection(_) if waited % 20 == 0 => {
                session.send_command(ZoneCommand::UseWaystone { waystone: home })?;
            }
            Step::Projection(_) if waited > MAX_SNAPSHOTS => {
                return Err("waystone travel never redirected".into());
            }
            Step::Projection(_) => {}
            Step::Redirect(notice) => break notice,
        }
        waited += 1;
    };
    let teleported = (notice.zone_id, notice.destination);
    let landed = arrive(session, &notice).await?;
    if distance(landed, home_site.arrival) > i64::from(ARRIVAL_TOLERANCE_UNITS) {
        return Err(format!("the traveller landed at {landed:?}, not at the stone").into());
    }
    Ok(Journey {
        walked,
        teleported,
        arrivals: [crossed, landed],
    })
}
