//! Summoned terrain (#293, Atlas 5.2.i293): a temporary, destroyable obstacle a player raises
//! about four metres in front of itself along its facing. Arcanist Ice Wall and Earth Pillar
//! use it; the Druid's roots and stone walls reuse it through #304.
//!
//! - **Collision**: each obstacle is an exact [`RigidBody::turned_box`] in the static collider
//!   namespace ([`crate::entity::SUMMONED_BODY_BASE`]), so character movement, forced moves,
//!   line of sight, projectiles and navigation meet it through the same static-geometry filters
//!   as authored walls.
//! - **Placement** (owner decision 2026-10-26, Atlas 5.2.i293-2): it stands
//!   [`SUMMON_DISTANCE_UNITS`] along the caster's facing, on the ground found there. The cast is
//!   refused with [`crate::ErrorCode::NoRoom`], free of cost and cooldown, when no ground lies
//!   within reach of the caster's feet, when the way to the spot is blocked by static geometry
//!   ([`crate::ErrorCode::LineOfSight`]), or when the footprint would touch any unit or moving
//!   platform: an obstacle never embeds or traps what stands where it would rise.
//! - **Destruction**: a projectile that strikes it chips it by [`PROJECTILE_SUMMONED_DAMAGE`],
//!   a blast deals its falloff damage as for structure pieces (#305), and a melee swing that
//!   covers no unit chips the nearest obstacle inside the swing by [`SWING_SUMMONED_DAMAGE`].
//!   A destroyed or expired obstacle's body leaves the world at once.
//! - **State**: canonical snapshots carry every standing obstacle by ID, so replay, recovery
//!   and handoff of a zone reproduce it exactly.

use crate::entity::{PLAYER_BODY_BASE, SUMMONED_BODY_BASE, SUMMONED_BODY_SLOTS, body_id};
use crate::physics::{BodyId, RigidBody, Vec3i};
use crate::zone::{physics_error, vector};
use crate::{EntityRef, ErrorCode, PlayerId, ZoneError, ZoneSimulation};

/// Where an obstacle rises, along the caster's facing (4 m).
pub const SUMMON_DISTANCE_UNITS: i32 = 400;
/// How much of an obstacle stands below the ground it rises from, so a slope shows no gap.
const GROUND_SINK_UNITS: i32 = 40;
/// The ground probe starts this far above the caster's feet …
const GROUND_PROBE_UP_UNITS: i32 = 240;
/// … and looks this far below them.
const GROUND_PROBE_DOWN_UNITS: i32 = 300;
/// A unit counts as under an obstacle within this margin of its footprint.
const FOOTPRINT_MARGIN_UNITS: i64 = 8;
/// A projectile striking an obstacle chips this much off its health.
pub const PROJECTILE_SUMMONED_DAMAGE: u32 = 6;
/// A melee swing into an obstacle chips this much off its health.
pub const SWING_SUMMONED_DAMAGE: u32 = 8;
/// Each player keeps at most one obstacle of each kind; a zone holds at most this many.
pub const MAX_SUMMONED_TERRAIN: usize = crate::MAX_PLAYERS_PER_ZONE * 2;
/// At most this many obstacles reach one player projection, nearest first.
pub const MAX_VISIBLE_SUMMONED: usize = 4;

/// What was summoned.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, strum::VariantArray)]
pub enum SummonedTerrainKind {
    /// A 4 m wide, 3 m tall sheet of ice across the caster's facing.
    IceWall,
    /// A 1.8 m square, 3.6 m tall pillar of rock.
    EarthPillar,
}

impl SummonedTerrainKind {
    /// Stable wire and canonical code.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::IceWall => 0,
            Self::EarthPillar => 1,
        }
    }

    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::IceWall),
            1 => Some(Self::EarthPillar),
            _ => None,
        }
    }

    /// The obstacle's half extents, in its own frame (`x` across the facing, `z` along it).
    #[must_use]
    pub const fn half_extents(self) -> [i32; 3] {
        match self {
            Self::IceWall => [200, 150, 30],
            Self::EarthPillar => [90, 180, 90],
        }
    }

    /// Health the obstacle rises with.
    #[must_use]
    pub const fn max_health(self) -> u32 {
        match self {
            Self::IceWall => 60,
            Self::EarthPillar => 90,
        }
    }

    /// Ticks it stands before it melts or crumbles.
    #[must_use]
    pub const fn duration_ticks(self) -> u32 {
        match self {
            Self::IceWall => crate::long_ticks_from_ms(12_000),
            Self::EarthPillar => crate::long_ticks_from_ms(15_000),
        }
    }
}

/// One standing obstacle (canonical state).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SummonedTerrain {
    /// The zone's summoning order; a body's slot derives from it.
    pub id: u32,
    pub kind: SummonedTerrainKind,
    /// The player that raised it, for the one-of-each-kind rule.
    pub caster: PlayerId,
    /// The body's centre, in units.
    pub centre: [i32; 3],
    /// The yaw it faces (its `z` axis, the caster's facing when raised).
    pub facing: u16,
    pub health: u32,
    /// The tick it melts away on.
    pub expires_at: u64,
}

impl SummonedTerrain {
    fn half(self) -> [i32; 3] {
        self.kind.half_extents()
    }

    const fn body(self) -> BodyId {
        BodyId(SUMMONED_BODY_BASE + (self.id as u64) % SUMMONED_BODY_SLOTS)
    }

    fn rigid_body(self) -> RigidBody {
        RigidBody::turned_box(
            self.body(),
            vector(self.centre),
            vector(self.half()),
            self.facing,
        )
    }

    /// World XZ of a point given in the obstacle's own frame (centre at the origin).
    fn to_world(self, local: [i64; 2]) -> [i64; 2] {
        let (sin, cos) = crate::trig::direction(self.facing);
        let (sin, cos) = (i64::from(sin), i64::from(cos));
        let one = i64::from(crate::trig::TRIG_ONE);
        [
            i64::from(self.centre[0]) + (local[0] * cos + local[1] * sin) / one,
            i64::from(self.centre[2]) + (local[1] * cos - local[0] * sin) / one,
        ]
    }

    /// `(x, z)` relative to the centre, in the obstacle's own frame.
    fn to_local(self, offset: [i64; 2]) -> [i64; 2] {
        let (sin, cos) = crate::trig::direction(self.facing);
        let (sin, cos) = (i64::from(sin), i64::from(cos));
        let one = i64::from(crate::trig::TRIG_ONE);
        [
            (offset[0] * cos - offset[1] * sin) / one,
            (offset[0] * sin + offset[1] * cos) / one,
        ]
    }

    /// The point of its footprint nearest `(x, z)`, world XZ, and that point's squared distance.
    fn nearest_on_footprint(self, x: i32, z: i32) -> ([i64; 2], i64) {
        let [hx, _, hz] = self.half().map(i64::from);
        let [lx, lz] = self.to_local([
            i64::from(x) - i64::from(self.centre[0]),
            i64::from(z) - i64::from(self.centre[2]),
        ]);
        let nearest = [lx.clamp(-hx, hx), lz.clamp(-hz, hz)];
        let (dx, dz) = (lx - nearest[0], lz - nearest[1]);
        (self.to_world(nearest), dx * dx + dz * dz)
    }

    /// The nearest point of the whole box to `point` (units), and its squared distance.
    fn nearest_point(self, point: [i32; 3]) -> ([i32; 3], i64) {
        let ([x, z], xz) = self.nearest_on_footprint(point[0], point[2]);
        let [_, hy, _] = self.half().map(i64::from);
        let y = i64::from(point[1]).clamp(
            i64::from(self.centre[1]) - hy,
            i64::from(self.centre[1]) + hy,
        );
        let dy = y - i64::from(point[1]);
        (
            [
                i32::try_from(x).unwrap_or(point[0]),
                i32::try_from(y).unwrap_or(point[1]),
                i32::try_from(z).unwrap_or(point[2]),
            ],
            xz + dy * dy,
        )
    }

    /// Whether a unit body of `half` centred on `at` stands within its footprint.
    fn touches_unit(self, at: Vec3i, half: Vec3i) -> bool {
        let [hx, hy, hz] = self.half();
        if at.y - half.y > self.centre[1] + hy || at.y + half.y < self.centre[1] - hy {
            return false;
        }
        let radius = i64::from(half.x.max(half.z)) + FOOTPRINT_MARGIN_UNITS;
        let reach = hx.max(hz) + half.x.max(half.z) + 16;
        if (at.x - self.centre[0]).abs() > reach || (at.z - self.centre[2]).abs() > reach {
            return false;
        }
        self.nearest_on_footprint(at.x, at.z).1 <= radius * radius
    }
}

/// An obstacle as a player sees it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SummonedView {
    pub id: u32,
    pub kind: SummonedTerrainKind,
    pub centre: [i32; 3],
    pub facing: u16,
    /// Health left, rounded up so a standing obstacle never shows 0.
    pub health_percent: u8,
}

/// Where a cast would raise its obstacle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Placement {
    centre: [i32; 3],
    facing: u16,
}

impl ZoneSimulation {
    /// Whether `body` is static collision below any summoned band: what an obstacle may stand on.
    const fn terrain_ground(body: BodyId) -> bool {
        body.0 < SUMMONED_BODY_BASE
    }

    /// The obstacle a static body belongs to, if it is one.
    pub(crate) fn summoned_of_body(&self, body: BodyId) -> Option<u32> {
        if !(SUMMONED_BODY_BASE..PLAYER_BODY_BASE).contains(&body.0) {
            return None;
        }
        self.summoned_terrain
            .values()
            .find(|obstacle| obstacle.body() == body)
            .map(|obstacle| obstacle.id)
    }

    /// The static ground below `(x, z)` near feet at `feet_y`, if any stands in reach.
    fn ground_near(&self, x: i32, z: i32, feet_y: i32) -> Option<i32> {
        let top = feet_y + GROUND_PROBE_UP_UNITS;
        let probe = GROUND_PROBE_UP_UNITS + GROUND_PROBE_DOWN_UNITS;
        #[allow(clippy::cast_precision_loss)]
        let hit = self.world.cast_ray(
            [x as f32, top as f32, z as f32],
            [0.0, -(probe as f32), 0.0],
            Self::terrain_ground,
        )?;
        if hit.fraction <= 0.0 {
            return None;
        }
        #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
        Some(top - (hit.fraction * probe as f32).round() as i32)
    }

    /// The placement of `kind` in front of `player_id`, or why it is refused.
    fn summoned_placement(
        &self,
        player_id: PlayerId,
        kind: SummonedTerrainKind,
    ) -> Result<Result<Placement, ErrorCode>, ZoneError> {
        let player = self
            .players
            .get(&player_id)
            .ok_or_else(|| ZoneError::new("unknown player"))?;
        let facing = player.facing;
        let centre = self.player_position(player_id)?;
        let body = self
            .world
            .body(body_id(EntityRef::Player(player_id)))
            .ok_or_else(|| ZoneError::new("player physics body is missing"))?;
        let feet = centre.y - body.half_extents().y;
        let (sin, cos) = crate::trig::direction(facing);
        let along = |value: i32| {
            let q16 = i64::from(value) * i64::from(SUMMON_DISTANCE_UNITS);
            i32::try_from(q16 / i64::from(crate::trig::TRIG_ONE)).unwrap_or(0)
        };
        let at = Vec3i::new(centre.x + along(sin), centre.y, centre.z + along(cos));
        // Authored geometry on the way to the spot: walls and doors, but not the ground or
        // another obstacle.
        if self.world.segment_blocked(
            centre,
            Vec3i::new(at.x - centre.x, 0, at.z - centre.z),
            |body| Self::terrain_ground(body) && Some(body) != self.ground_body(),
        ) {
            return Ok(Err(ErrorCode::LineOfSight));
        }
        let [hx, hy, hz] = kind.half_extents();
        // The ground under the middle and both ends of the width; the lowest decides.
        let across = (along_rotation(hx, cos), -along_rotation(hx, sin));
        let (right_x, right_z) = (at.x + across.0, at.z + across.1);
        let (left_x, left_z) = (at.x - across.0, at.z - across.1);
        let Some(mut bottom) = self.ground_near(at.x, at.z, feet) else {
            return Ok(Err(ErrorCode::NoRoom));
        };
        if (bottom - feet).abs() > GROUND_PROBE_UP_UNITS {
            return Ok(Err(ErrorCode::NoRoom));
        }
        for (x, z) in [(right_x, right_z), (left_x, left_z)] {
            if let Some(ground) = self.ground_near(x, z, feet) {
                bottom = bottom.min(ground);
            }
        }
        let placement = Placement {
            centre: [at.x, bottom - GROUND_SINK_UNITS + hy, at.z],
            facing,
        };
        let candidate = SummonedTerrain {
            id: 0,
            kind,
            caster: player_id,
            centre: placement.centre,
            facing,
            health: 1,
            expires_at: 0,
        };
        let reach = hx.max(hz) + 400;
        for unit in self.interest.near(at.x, at.z, reach) {
            let Some(unit_body) = self.world.body(body_id(unit)) else {
                continue;
            };
            if candidate.touches_unit(unit_body.position(), unit_body.half_extents()) {
                return Ok(Err(ErrorCode::NoRoom));
            }
        }
        // A moving platform's path (the mill hoist's shaft) stays clear.
        let bound = candidate.rigid_body().half_extents();
        #[allow(clippy::cast_precision_loss)]
        let aabb_centre = [
            placement.centre[0] as f32,
            placement.centre[1] as f32,
            placement.centre[2] as f32,
        ];
        #[allow(clippy::cast_precision_loss)]
        let aabb_half = [bound.x as f32, bound.y as f32, bound.z as f32];
        let overlapped = self
            .world
            .overlap_box(aabb_centre, aabb_half)
            .map_err(physics_error)?;
        if overlapped
            .iter()
            .any(|other| self.world.body(*other).is_some_and(RigidBody::is_platform))
        {
            return Ok(Err(ErrorCode::NoRoom));
        }
        Ok(Ok(placement))
    }

    /// Whether `kind` can rise for `player_id` now; a refused cast costs nothing.
    pub(crate) fn check_summon_terrain(
        &self,
        player_id: PlayerId,
        kind: SummonedTerrainKind,
    ) -> Result<Option<ErrorCode>, ZoneError> {
        let others = self
            .summoned_terrain
            .values()
            .filter(|obstacle| !(obstacle.caster == player_id && obstacle.kind == kind))
            .count();
        if others >= MAX_SUMMONED_TERRAIN {
            return Ok(Some(ErrorCode::NoRoom));
        }
        Ok(self.summoned_placement(player_id, kind)?.err())
    }

    /// Raises `kind` for `player_id` at tick `now`, replacing that caster's earlier one of the
    /// same kind.
    pub(crate) fn raise_summoned_terrain(
        &mut self,
        player_id: PlayerId,
        kind: SummonedTerrainKind,
        now: u64,
    ) -> Result<(), ZoneError> {
        let Ok(placement) = self.summoned_placement(player_id, kind)? else {
            return Ok(());
        };
        let earlier: Vec<u32> = self
            .summoned_terrain
            .values()
            .filter(|obstacle| obstacle.caster == player_id && obstacle.kind == kind)
            .map(|obstacle| obstacle.id)
            .collect();
        for id in earlier {
            self.remove_summoned(id);
        }
        let id = self.next_summoned_terrain;
        let obstacle = SummonedTerrain {
            id,
            kind,
            caster: player_id,
            centre: placement.centre,
            facing: placement.facing,
            health: kind.max_health(),
            expires_at: now.saturating_add(u64::from(kind.duration_ticks())),
        };
        // Slots repeat only after this many summons; an older one can still stand only
        // pathologically, and then this one does not rise.
        if self.world.body(obstacle.body()).is_none() {
            self.next_summoned_terrain = id.wrapping_add(1);
            self.world
                .add_body(obstacle.rigid_body())
                .map_err(physics_error)?;
            self.summoned_terrain.insert(id, obstacle);
            self.invalidate_navigation();
        }
        Ok(())
    }

    fn remove_summoned(&mut self, id: u32) {
        if let Some(obstacle) = self.summoned_terrain.remove(&id) {
            self.world.remove_body(obstacle.body());
            self.invalidate_navigation();
        }
    }

    /// Chips `amount` off a standing obstacle; at zero it is destroyed.
    pub(crate) fn damage_summoned(&mut self, id: u32, amount: u32) {
        let Some(obstacle) = self.summoned_terrain.get_mut(&id) else {
            return;
        };
        obstacle.health = obstacle.health.saturating_sub(amount.max(1));
        if obstacle.health == 0 {
            self.remove_summoned(id);
        }
    }

    /// A blast at `centre` hurts every obstacle within `radius` that the centre can see,
    /// with its falloff to half at the edge, like a structure piece (#305).
    pub(crate) fn blast_summoned(
        &mut self,
        centre: [i32; 3],
        radius: i32,
        amount: u32,
    ) -> Result<(), ZoneError> {
        let radius = i64::from(radius.max(1));
        let reached: Vec<(u32, u32)> = self
            .summoned_terrain
            .values()
            .filter_map(|obstacle| {
                let (near, squared) = obstacle.nearest_point(centre);
                let distance = squared.isqrt();
                if distance > radius {
                    return None;
                }
                #[allow(clippy::cast_precision_loss)]
                let delta = [
                    (near[0] - centre[0]) as f32,
                    (near[1] - centre[1]) as f32,
                    (near[2] - centre[2]) as f32,
                ];
                let body = obstacle.body();
                #[allow(clippy::cast_precision_loss)]
                let hit = self
                    .world
                    .cast_ray(centre.map(|value| value as f32), delta, |other| {
                        other.0 < PLAYER_BODY_BASE
                    });
                if hit.is_some_and(|hit| hit.body != body && hit.fraction < 0.999) {
                    return None;
                }
                let share = 1_000 - 500 * distance / radius;
                let damage = u32::try_from(i64::from(amount) * share / 1_000).unwrap_or(0);
                Some((obstacle.id, damage))
            })
            .collect();
        for (id, damage) in reached {
            self.damage_summoned(id, damage);
        }
        Ok(())
    }

    /// A swing toward `facing` that covered no unit chips the nearest obstacle inside it.
    pub(crate) fn swing_at_summoned(
        &mut self,
        player_id: PlayerId,
        facing: u16,
    ) -> Result<(), ZoneError> {
        let origin = self.player_position(player_id)?;
        let (dir_x, dir_z) = crate::trig::direction(facing);
        let min_cos = i64::from(crate::trig::cos(crate::unit::SWING_ARC_HALF_WIDTH));
        let reach_squared = i64::from(crate::unit::PLAYER_REACH_UNITS).pow(2);
        let mut best: Option<(i64, u32)> = None;
        for obstacle in self.summoned_terrain.values() {
            let (nearest, squared) = obstacle.nearest_on_footprint(origin.x, origin.z);
            if squared > reach_squared || best.is_some_and(|(found, _)| found <= squared) {
                continue;
            }
            let (dx, dz) = (
                i32::try_from(nearest[0] - i64::from(origin.x)).unwrap_or(0),
                i32::try_from(nearest[1] - i64::from(origin.z)).unwrap_or(0),
            );
            let along = i64::from(dx) * i64::from(dir_x) + i64::from(dz) * i64::from(dir_z);
            let length = i64::try_from(crate::trig::xz_length(dx, dz))
                .map_err(|_| ZoneError::new("swing distance overflow"))?;
            if length == 0 || along >= length * min_cos {
                best = Some((squared, obstacle.id));
            }
        }
        if let Some((_, id)) = best {
            self.damage_summoned(id, SWING_SUMMONED_DAMAGE);
        }
        Ok(())
    }

    /// Step 7: obstacles whose time is up leave the world, in ID order.
    pub(crate) fn advance_summoned(&mut self, now: u64) {
        let expired: Vec<u32> = self
            .summoned_terrain
            .values()
            .filter(|obstacle| obstacle.expires_at <= now)
            .map(|obstacle| obstacle.id)
            .collect();
        for id in expired {
            self.remove_summoned(id);
        }
    }

    /// The obstacles a viewer at `centre` sees, nearest first, ties by ID.
    pub(crate) fn summoned_views(&self, centre: [i32; 3]) -> Vec<SummonedView> {
        let radius = i64::from(crate::INTEREST_RADIUS_UNITS).pow(2);
        let mut near: Vec<(i64, SummonedView)> = self
            .summoned_terrain
            .values()
            .filter_map(|obstacle| {
                let (dx, dz) = (
                    i64::from(obstacle.centre[0] - centre[0]),
                    i64::from(obstacle.centre[2] - centre[2]),
                );
                let squared = dx * dx + dz * dz;
                (squared <= radius).then(|| {
                    (
                        squared,
                        SummonedView {
                            id: obstacle.id,
                            kind: obstacle.kind,
                            centre: obstacle.centre,
                            facing: obstacle.facing,
                            health_percent: crate::unit::health_percent(
                                obstacle.health,
                                obstacle.kind.max_health(),
                            ),
                        },
                    )
                })
            })
            .collect();
        near.sort_by_key(|(squared, view)| (*squared, view.id));
        near.into_iter()
            .take(MAX_VISIBLE_SUMMONED)
            .map(|(_, view)| view)
            .collect()
    }

    /// Canonical restore: ordered IDs below `next`, known bounds, and a body for each.
    pub(crate) fn restore_summoned(
        &mut self,
        obstacles: Vec<SummonedTerrain>,
        next: u32,
        tick: u64,
    ) -> Result<(), ZoneError> {
        let valid = obstacles.len() <= MAX_SUMMONED_TERRAIN
            && obstacles.windows(2).all(|pair| pair[0].id < pair[1].id)
            && obstacles.iter().all(|obstacle| {
                obstacle.id < next
                    && obstacle.health > 0
                    && obstacle.health <= obstacle.kind.max_health()
                    && obstacle.expires_at > tick
                    && obstacle.expires_at
                        <= tick.saturating_add(u64::from(obstacle.kind.duration_ticks()))
            });
        if !valid {
            return Err(ZoneError::new("snapshot summoned terrain is invalid"));
        }
        for obstacle in &obstacles {
            if obstacle
                .centre
                .iter()
                .any(|axis| axis.abs() > crate::MAX_CONTENT_COORDINATE_UNITS)
            {
                return Err(ZoneError::new("snapshot summoned terrain is out of range"));
            }
            self.world
                .add_body(obstacle.rigid_body())
                .map_err(physics_error)?;
        }
        self.summoned_terrain = obstacles
            .into_iter()
            .map(|obstacle| (obstacle.id, obstacle))
            .collect();
        self.next_summoned_terrain = next;
        self.invalidate_navigation();
        Ok(())
    }
}

/// `half × a` in Q16 back to whole units.
fn along_rotation(half: i32, a: i32) -> i32 {
    let q16 = i64::from(half) * i64::from(a);
    i32::try_from(q16 / i64::from(crate::trig::TRIG_ONE)).unwrap_or(0)
}
