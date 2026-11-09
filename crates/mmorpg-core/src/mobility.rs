//! Movement spells that are not forced moves (#294): the Arcanist's Blink. Heroic Leap and
//! Grappling Hook live in [`crate::forced`].
//!
//! **Blink rule.** Blink goes straight along the caster's facing for its distance, or as far as
//! its capsule (raised to the same knee height a charge uses) can sweep: walls, closed doors,
//! props, moving platforms and summoned terrain end the way, as they end a charge, and the sweep
//! never crosses them. The terrain ridges a stride cannot clear end it too. From there it
//! steps back until it finds a landing: ground within [`BLINK_RISE_UNITS`] above or
//! [`BLINK_DROP_UNITS`] below the caster's feet, with room for the caster's body (no
//! wall, prop or other unit). A cast with no landing at least [`BLINK_MIN_UNITS`] away is
//! refused, free of cost and cooldown ([`crate::ErrorCode::NoLanding`]).

use crate::entity::{PLAYER_BODY_BASE, body_id};
use crate::forced::CHARGE_CLEARANCE_UNITS;
use crate::physics::{BodyId, Vec3i};
use crate::zone::physics_error;
use crate::{EntityRef, PlayerId, ZoneError, ZoneSimulation};

/// The shortest blink worth its cost: below this the cast is refused (2 m).
pub const BLINK_MIN_UNITS: i32 = 200;
/// A landing may rise this far above the caster's feet (1.2 m) …
pub const BLINK_RISE_UNITS: i32 = 120;
/// … or lie this far below them (3 m).
pub const BLINK_DROP_UNITS: i32 = 300;
/// The sweep keeps this margin off whatever stopped it, so the body is not left touching a wall.
const BLINK_WALL_MARGIN_UNITS: f32 = 16.0;
/// Each retry for a landing steps this much closer (0.5 m).
const BLINK_BACK_STEP_UNITS: i32 = 50;
/// The body stands a hair above the ground it lands on, as a restored body does.
const LANDING_RAISE_UNITS: i32 = 1;

impl ZoneSimulation {
    /// Where the body of `player_id` would land after a blink of `distance` units, if anywhere.
    pub(crate) fn blink_destination(
        &self,
        player_id: PlayerId,
        distance: i32,
    ) -> Result<Option<Vec3i>, ZoneError> {
        let unit = EntityRef::Player(player_id);
        let player = self
            .players
            .get(&player_id)
            .ok_or_else(|| ZoneError::new("unknown player"))?;
        let body = self
            .world
            .body(body_id(unit))
            .ok_or_else(|| ZoneError::new("player physics body is missing"))?;
        let half = body.half_extents();
        let here = body.exact_position();
        let centre = body.position();
        let feet = centre.y - half.y;
        let (sin, cos) = crate::trig::direction(player.facing);
        let one = f64::from(crate::trig::TRIG_ONE);
        let direction = [f64::from(sin) / one, f64::from(cos) / one];
        // Everything but the ground blocks the way; a charge's filter (#57).
        let ground = self.ground_body();
        let blocks = |body: BodyId| body.0 < PLAYER_BODY_BASE && Some(body) != ground;
        #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
        let sweep = [
            (direction[0] * f64::from(distance)) as f32,
            0.0_f32,
            (direction[1] * f64::from(distance)) as f32,
        ];
        let lift = CHARGE_CLEARANCE_UNITS / 2;
        #[allow(clippy::cast_precision_loss)]
        let raised = [here[0], here[1] + lift as f32, here[2]];
        let wall = self.world.cast_shape(
            raised,
            Vec3i::new(half.x, half.y - lift, half.z),
            sweep,
            blocks,
        );
        // The ground higher than a stride along the way (a ridge or cliff) ends it as well.
        #[allow(clippy::cast_precision_loss)]
        let stride = [here[0], (feet + BLINK_RISE_UNITS) as f32, here[2]];
        let ridge = self
            .world
            .cast_ray(stride, sweep, |body| Some(body) == ground);
        #[allow(clippy::cast_precision_loss)]
        let distance_f = distance as f32;
        let mut reach = distance_f;
        for hit in [wall, ridge].into_iter().flatten() {
            reach = reach.min(hit.fraction * distance_f - BLINK_WALL_MARGIN_UNITS);
        }
        #[allow(clippy::cast_possible_truncation)]
        let mut travel = reach.floor() as i32;
        while travel >= BLINK_MIN_UNITS {
            let along = |value: f64| (value * f64::from(travel)).round();
            #[allow(clippy::cast_possible_truncation)]
            let (x, z) = (
                centre.x + along(direction[0]) as i32,
                centre.z + along(direction[1]) as i32,
            );
            if let Some(landing) = self.landing_below(x, z, feet) {
                let target = Vec3i::new(x, landing + half.y + LANDING_RAISE_UNITS, z);
                if crate::zone::within_world_limits(target)
                    && self.stands_clear([target.x, target.y, target.z], [half.x, half.y, half.z])
                {
                    return Ok(Some(target));
                }
            }
            travel -= BLINK_BACK_STEP_UNITS;
        }
        Ok(None)
    }

    /// Ground under `(x, z)` near feet at `feet`, in reach of a blink's landing.
    fn landing_below(&self, x: i32, z: i32, feet: i32) -> Option<i32> {
        let top = feet + BLINK_RISE_UNITS;
        let reach = BLINK_RISE_UNITS + BLINK_DROP_UNITS;
        #[allow(clippy::cast_precision_loss)]
        let hit = self.world.cast_ray(
            [x as f32, top as f32, z as f32],
            [0.0, -(reach as f32), 0.0],
            Self::terrain_ground,
        )?;
        if hit.fraction <= 0.0 {
            return None;
        }
        #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
        Some(top - (hit.fraction * reach as f32).round() as i32)
    }

    /// Teleports `player_id` by a blink of `distance` units; nothing happens when no landing
    /// remains (the zone refused it earlier in the same tick in every other case).
    pub(crate) fn blink(&mut self, player_id: PlayerId, distance: i32) -> Result<(), ZoneError> {
        let Some(target) = self.blink_destination(player_id, distance)? else {
            return Ok(());
        };
        let unit = EntityRef::Player(player_id);
        let body = body_id(unit);
        // A blink breaks whatever carried the body: a forced move, a roll, a climb or mantle.
        self.end_forced_move_of(unit);
        let climbing = self.players.get_mut(&player_id).is_some_and(|player| {
            player.roll = player.roll.ended();
            player.repairing = None;
            let held = player.traversal.holds();
            if held {
                player.traversal = crate::Traversal::Released {
                    ticks: crate::climb::RELEASE_TICKS,
                };
            }
            held
        });
        if climbing {
            self.world
                .set_weightless(body, false)
                .map_err(physics_error)?;
        }
        self.world
            .set_position(body, target)
            .map_err(physics_error)?;
        self.world
            .set_velocity(body, Vec3i::ZERO)
            .map_err(physics_error)?;
        self.place_in_interest(unit, target.x, target.z);
        Ok(())
    }

    /// Whether `player_id` can take off: on the ground and out of deep water.
    pub(crate) fn leap_can_start(&self, player_id: PlayerId) -> Result<bool, ZoneError> {
        let body = self
            .world
            .body(body_id(EntityRef::Player(player_id)))
            .ok_or_else(|| ZoneError::new("player physics body is missing"))?;
        let centre = body.position();
        let feet = [centre.x, centre.y - body.half_extents().y, centre.z];
        Ok(self.world.grounded(body.id())
            && crate::water::swim_depth(self.content.waters(), feet).is_none())
    }
}
