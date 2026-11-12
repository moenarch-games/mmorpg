//! The grounded walk and jump step (#749): the per-tick rule that turns a character's held
//! intent into the controller velocity it asks the physics world for.
//!
//! - Horizontal: the forward and strafe signs pick one of eight directions around the yaw,
//!   moved at the forward speed, or the backward speed whenever the intent has a backward
//!   component. No intent stands still.
//! - Vertical: a jump sets the jump speed only while [`World::grounded`]; otherwise the body
//!   keeps the vertical velocity the last step realized. Gravity is the world's own (given to
//!   [`World::new`]) and applies when the world steps.
//!
//! Every value is integer: speeds in subunits per tick ([`sim_foundation::subunits_per_tick`]),
//! yaw and directions in [`sim_foundation::trig`] steps. Games layer their own rules (auras,
//! surfaces, sprinting, air control) over [`walk_velocity`] and [`jump_velocity`], or use
//! [`step`] as is.

use sim_foundation::physics::{BodyId, PhysicsError, RigidBody, Vec3i, World};
use sim_foundation::trig::{self, YAW_EIGHTH_TURN, YAW_QUARTER_TURN};

use crate::{
    BACKPEDAL_SPEED_SUBUNITS_PER_TICK, JUMP_VELOCITY_SUBUNITS_PER_TICK, RUN_SPEED_SUBUNITS_PER_TICK,
};

/// What a character holds this tick.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Intent {
    /// Positive forward, negative backward (only the sign counts).
    pub forward: i8,
    /// Positive right, negative left (only the sign counts).
    pub strafe: i8,
    /// The yaw the character faces ([`trig`] steps; yaw 0 faces +Z).
    pub yaw: u16,
    /// A jump is asked for this tick.
    pub jump: bool,
}

/// Speeds the step moves at, in subunits per tick.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Speeds {
    /// Forward, strafe and forward-diagonal intent.
    pub forward: i32,
    /// Any intent with a backward component.
    pub backward: i32,
    /// The upward velocity a grounded jump sets.
    pub jump: i32,
}

impl Speeds {
    /// The run, backpedal and jump speeds this crate defines.
    pub const RUN: Self = Self {
        forward: RUN_SPEED_SUBUNITS_PER_TICK,
        backward: BACKPEDAL_SPEED_SUBUNITS_PER_TICK,
        jump: JUMP_VELOCITY_SUBUNITS_PER_TICK,
    };
}

/// The yaw offset of a forward/strafe combination: one of eight multiples of 45°, or `None`
/// without intent. The character's right is `direction(yaw - 90°)`.
#[must_use]
pub const fn move_offset(forward: i8, strafe: i8) -> Option<u16> {
    const FORWARD: u16 = 0;
    const LEFT: u16 = YAW_QUARTER_TURN;
    const BACKWARD: u16 = 2 * YAW_QUARTER_TURN;
    const RIGHT: u16 = 3 * YAW_QUARTER_TURN;
    Some(match (forward.signum(), strafe.signum()) {
        (0, 0) => return None,
        (1, 0) => FORWARD,
        (1, -1) => FORWARD + YAW_EIGHTH_TURN,
        (0, -1) => LEFT,
        (-1, -1) => LEFT + YAW_EIGHTH_TURN,
        (-1, 0) => BACKWARD,
        (-1, 1) => BACKWARD + YAW_EIGHTH_TURN,
        (0, 1) => RIGHT,
        _ => RIGHT + YAW_EIGHTH_TURN,
    })
}

/// The yaw `intent` moves along: its yaw plus [`move_offset`], or `None` without intent.
#[must_use]
pub const fn move_yaw(intent: Intent) -> Option<u16> {
    match move_offset(intent.forward, intent.strafe) {
        Some(offset) => Some(intent.yaw.wrapping_add(offset)),
        None => None,
    }
}

/// The horizontal `[x, z]` velocity `intent` asks for: `speed × direction(move yaw)`, zero
/// without intent. `None` only when a speed scaled by the direction overflows `i32`.
#[must_use]
pub fn walk_velocity(intent: Intent, speeds: Speeds) -> Option<[i32; 2]> {
    let Some(yaw) = move_yaw(intent) else {
        return Some([0, 0]);
    };
    let speed = if intent.forward < 0 {
        speeds.backward
    } else {
        speeds.forward
    };
    let (x, z) = trig::direction(yaw);
    Some([
        trig::checked_scale(speed, x)?,
        trig::checked_scale(speed, z)?,
    ])
}

/// The upward velocity a jump sets: `jump_speed` when `jump` is asked for while `grounded`,
/// otherwise `None` (the body keeps its vertical velocity).
#[must_use]
pub const fn jump_velocity(grounded: bool, jump: bool, jump_speed: i32) -> Option<i32> {
    if jump && grounded {
        Some(jump_speed)
    } else {
        None
    }
}

/// One tick's walk and jump for `body`, before the world steps: sets its controller velocity to
/// [`walk_velocity`] horizontally and [`jump_velocity`] (or its realized vertical velocity)
/// vertically. Returns whether it jumped. An overflowing walk velocity stands still.
pub fn step(
    world: &mut World,
    body: BodyId,
    intent: Intent,
    speeds: Speeds,
) -> Result<bool, PhysicsError> {
    let current = world
        .body(body)
        .map(RigidBody::velocity)
        .unwrap_or_default();
    let jump = jump_velocity(world.grounded(body), intent.jump, speeds.jump);
    let [x, z] = walk_velocity(intent, speeds).unwrap_or_default();
    world.set_velocity(body, Vec3i::new(x, jump.unwrap_or(current.y), z))?;
    Ok(jump.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_foundation::physics::SUBUNITS_PER_UNIT;

    const BODY: BodyId = BodyId(10);

    fn world(feet: i32) -> World {
        let mut world = World::new(Vec3i::new(0, -SUBUNITS_PER_UNIT, 0));
        world.add_body(RigidBody::floor(BodyId(1), 0)).unwrap();
        world
            .add_body(RigidBody::dynamic(
                BODY,
                Vec3i::new(0, feet + 90, 0),
                Vec3i::ZERO,
                Vec3i::new(30, 90, 30),
            ))
            .unwrap();
        world
    }

    fn intent(forward: i8, strafe: i8, jump: bool) -> Intent {
        Intent {
            forward,
            strafe,
            yaw: 0,
            jump,
        }
    }

    #[test]
    fn eight_directions_at_forward_or_backward_speed() {
        let speeds = Speeds::RUN;
        assert_eq!(walk_velocity(intent(0, 0, false), speeds), Some([0, 0]));
        // Yaw 0 faces +Z; the character's right is -X.
        assert_eq!(
            walk_velocity(intent(1, 0, false), speeds),
            Some([0, speeds.forward])
        );
        assert_eq!(
            walk_velocity(intent(0, 1, false), speeds),
            Some([-speeds.forward, 0])
        );
        assert_eq!(
            walk_velocity(intent(-1, 0, false), speeds),
            Some([0, -speeds.backward])
        );
        let [x, z] = walk_velocity(intent(-1, -1, false), speeds).unwrap();
        assert!(x > 0 && z < 0 && x == -z && x < speeds.backward);
        assert_eq!(move_offset(5, -3), move_offset(1, -1));
    }

    #[test]
    fn jumps_only_from_the_ground() {
        let mut world = world(0);
        for _ in 0..4 {
            world.step().unwrap();
        }
        assert!(world.grounded(BODY));
        assert!(step(&mut world, BODY, intent(1, 0, true), Speeds::RUN).unwrap());
        let velocity = world.body(BODY).unwrap().velocity();
        assert_eq!(
            velocity,
            Vec3i::new(0, Speeds::RUN.jump, Speeds::RUN.forward)
        );

        let mut world = world_in_air();
        assert!(!world.grounded(BODY));
        assert!(!step(&mut world, BODY, intent(0, 0, true), Speeds::RUN).unwrap());
        let velocity = world.body(BODY).unwrap().velocity();
        assert!(velocity.y < 0, "keeps falling: {velocity:?}");
    }

    fn world_in_air() -> World {
        let mut world = world(400);
        world.step().unwrap();
        world.step().unwrap();
        world
    }
}
