//! Physical projectiles (#286, Atlas 5.2.71): arrows, crossbow bolts and magic missiles (#289).
//!
//! A target-based shot **tracks** its target: every tick it re-plans the ballistic arc that
//! lands on the target's body centre after the remaining flight ticks, so it follows a moving
//! target but still flies a visible arc. A free shot (Aimed Shot without a target) leaves along
//! the shooter's facing and falls under gravity. Each tick a projectile sweeps the segment it
//! travels with a Rapier ray, so it never tunnels: walls, props, terrain and units in the way
//! stop it, and a Warden's raised shield turned toward it blocks it. The ability's effect lands
//! only when the shot reaches a valid victim. Spent projectiles stay stuck where they hit — in a
//! wall for [`STUCK_TICKS`], riding a unit for [`STUCK_IN_UNIT_TICKS`] — then vanish.
//!
//! A magic missile (Firebolt) ignores gravity: a target-based missile homes on its target in a
//! straight line, so only something in the way stops it; one cast without a target flies
//! straight along the facing and can miss or be dodged.
//!
//! Aimed specials (#291) change what a meeting does: a Piercing Shot passes through up to
//! [`PIERCE_HITS`] enemies in a line, each later hit weaker ([`PIERCE_DAMAGE_PERCENT`]), and a
//! Ricochet Shot mirrors off walls, props and terrain up to [`RICOCHET_BOUNCES`] times about
//! the hit's surface normal. Walls stop a piercing shot; raised shields stop both.
//!
//! Rapier gap noted for physics-engine parity: no continuous projectile body is used; a
//! per-tick ray sweep stands in for CCD on a thin, fast body.

use crate::ability::Shot;
use crate::entity::{EntityRef, body_id, entity_of};
use crate::physics::BodyId;
use crate::{AbilityId, ZoneError, ZoneEvent, ZoneSimulation};

/// A Piercing Shot strikes at most this many enemies (#291) …
pub const PIERCE_HITS: usize = 3;
/// … each for this share of the shot's damage, in hit order.
pub const PIERCE_DAMAGE_PERCENT: [u32; PIERCE_HITS] = [100, 70, 50];
/// A Ricochet Shot mirrors off surfaces at most this many times (#291).
pub const RICOCHET_BOUNCES: u8 = 2;
/// A bounced shot restarts this far off the surface along its normal, units.
const BOUNCE_CLEARANCE_UNITS: f32 = 1.0;

/// At most this many projectiles fly or stick in one zone; the oldest goes first.
pub const MAX_PROJECTILES: usize = 256;
/// At most this many projectiles reach one player projection, nearest first.
pub const MAX_VISIBLE_PROJECTILES: usize = 16;
/// Flight speed of an arrow (54 m/s) and of a crossbow bolt (45 m/s), units per tick.
pub const ARROW_SPEED_UNITS: f32 = 180.0;
pub const BOLT_SPEED_UNITS: f32 = 150.0;
/// Flight speed of a magic missile (30 m/s), units per tick: slow enough to sidestep.
pub const MISSILE_SPEED_UNITS: f32 = 100.0;
/// Gravity on a free projectile: 9.81 m/s² in units per tick².
pub const PROJECTILE_GRAVITY_UNITS: f32 = 981.0 / 900.0;
/// A projectile still flying after this many ticks (2 s) falls away.
pub const MAX_FLIGHT_TICKS: u16 = 60;
/// Ticks a spent projectile stays stuck in a wall or prop (3 s) …
pub const STUCK_TICKS: u16 = 90;
/// … or in a unit or its shield (1 s).
pub const STUCK_IN_UNIT_TICKS: u16 = 30;
/// Shots leave this far above the shooter's body centre (chest height). A level free shot
/// drops to the ground about 32 m out.
pub const LAUNCH_HEIGHT_UNITS: f32 = 20.0;
/// A free missile flies level this high above the caster's feet (#289), low enough to meet
/// wolf-sized creatures and humanoids alike.
pub const MISSILE_FLIGHT_HEIGHT_UNITS: f32 = 50.0;

/// What flies; the wire value is its code.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ProjectileKind {
    Arrow,
    Bolt,
    /// A magic missile (#289): no gravity.
    Missile,
}

impl ProjectileKind {
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Arrow => 1,
            Self::Bolt => 2,
            Self::Missile => 3,
        }
    }

    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Arrow),
            2 => Some(Self::Bolt),
            3 => Some(Self::Missile),
            _ => None,
        }
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Arrow => "arrow",
            Self::Bolt => "bolt",
            Self::Missile => "missile",
        }
    }

    #[must_use]
    pub const fn speed(self) -> f32 {
        match self {
            Self::Arrow => ARROW_SPEED_UNITS,
            Self::Bolt => BOLT_SPEED_UNITS,
            Self::Missile => MISSILE_SPEED_UNITS,
        }
    }

    /// Downward acceleration in flight, units per tick²: arrows and bolts fall, missiles fly
    /// straight.
    #[must_use]
    pub const fn gravity(self) -> f32 {
        match self {
            Self::Arrow | Self::Bolt => PROJECTILE_GRAVITY_UNITS,
            Self::Missile => 0.0,
        }
    }
}

/// How a projectile moves this tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Flight {
    /// Tracks `target`, planned to arrive after `remaining` more ticks.
    Tracking { target: EntityRef, remaining: u16 },
    /// Flies freely under gravity.
    Free,
    /// Spent: stuck at `offset` from the unit it hit (or at `offset` in the world) for
    /// `remaining` more ticks.
    Stuck {
        unit: Option<EntityRef>,
        offset: [f32; 3],
        remaining: u16,
    },
}

/// One projectile; positions and velocities are exact units (per tick).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Projectile {
    pub id: u32,
    pub kind: ProjectileKind,
    pub owner: EntityRef,
    /// The ability whose effect lands on impact.
    pub ability: AbilityId,
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub flight: Flight,
    /// Ticks flown.
    pub age: u16,
    /// Units a piercing shot already struck and now passes (#291).
    pub struck: [Option<EntityRef>; PIERCE_HITS - 1],
    /// Surfaces a ricochet shot already bounced off (#291).
    pub bounces: u8,
}

impl Projectile {
    #[must_use]
    pub const fn is_stuck(&self) -> bool {
        matches!(self.flight, Flight::Stuck { .. })
    }

    /// How it treats what it meets: its ability's shot.
    #[must_use]
    pub fn shot(&self) -> Shot {
        crate::ability_by_id(self.ability).map_or(Shot::Plain, crate::Ability::shot)
    }

    fn passes(&self, unit: EntityRef) -> bool {
        self.struck.contains(&Some(unit))
    }
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// What a viewer sees of a projectile: where it is and where it points.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ProjectileView {
    /// The zone's projectile ID, truncated; presentation identity only.
    pub id: u16,
    pub kind: ProjectileKind,
    /// Whole units.
    pub position: [i32; 3],
    /// Heading (the facing convention) and pitch (positive up), in 1/65 536 turns.
    pub yaw: u16,
    pub pitch: i16,
    pub stuck: bool,
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale(a: [f32; 3], by: f32) -> [f32; 3] {
    a.map(|axis| axis * by)
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn turns(radians: f32) -> i32 {
    (radians / std::f32::consts::TAU * 65_536.0).round() as i32
}

/// The heading and pitch of `velocity`, in the facing convention (yaw 0 is +Z, a quarter
/// turn is +X).
#[must_use]
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn orientation(velocity: [f32; 3]) -> (u16, i16) {
    let [x, y, z] = velocity;
    let yaw = turns(x.atan2(z)).rem_euclid(65_536) as u16;
    let pitch = turns(y.atan2(x.hypot(z))).clamp(-16_384, 16_384) as i16;
    (yaw, pitch)
}

/// The velocity that covers `delta` in `ticks` ticks under `gravity` (units per tick²).
fn ballistic(delta: [f32; 3], ticks: u16, gravity: f32) -> [f32; 3] {
    let t = f32::from(ticks.max(1));
    [
        delta[0] / t,
        delta[1] / t + gravity * (t - 1.0) / 2.0,
        delta[2] / t,
    ]
}

/// A missile's step toward a target `delta` away: straight at it at `speed`, or onto it when
/// closer than one step.
fn homing(delta: [f32; 3], speed: f32) -> [f32; 3] {
    let distance = delta[0].hypot(delta[1]).hypot(delta[2]);
    if distance <= speed {
        // The centre lies inside the body, so the sweep meets its surface on the way.
        return delta;
    }
    scale(delta, speed / distance)
}

impl ZoneSimulation {
    /// A unit's physics body centre in exact units.
    pub(crate) fn unit_centre(&self, unit: EntityRef) -> Option<[f32; 3]> {
        self.world
            .body(body_id(unit))
            .map(crate::physics::RigidBody::exact_position)
    }

    pub(crate) fn launch_point(&self, owner: EntityRef) -> Result<[f32; 3], ZoneError> {
        let centre = self
            .unit_centre(owner)
            .ok_or_else(|| ZoneError::new("shooter physics body is missing"))?;
        Ok(add(centre, [0.0, LAUNCH_HEIGHT_UNITS, 0.0]))
    }

    fn add_projectile(&mut self, mut projectile: Projectile) {
        projectile.id = self.next_projectile;
        self.next_projectile = self.next_projectile.wrapping_add(1);
        if self.projectiles.len() >= MAX_PROJECTILES
            && let Some((&oldest, _)) = self.projectiles.iter().next()
        {
            self.projectiles.remove(&oldest);
        }
        self.projectiles.insert(projectile.id, projectile);
    }

    /// `owner` shoots `kind` at `target`; `ability` lands on impact.
    pub(crate) fn shoot_at(
        &mut self,
        owner: EntityRef,
        target: EntityRef,
        kind: ProjectileKind,
        ability: AbilityId,
    ) -> Result<(), ZoneError> {
        let from = self.launch_point(owner)?;
        let to = self
            .unit_centre(target)
            .ok_or_else(|| ZoneError::new("target physics body is missing"))?;
        let delta = sub(to, from);
        let distance = delta[0].hypot(delta[1]).hypot(delta[2]);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let remaining = (distance / kind.speed())
            .ceil()
            .clamp(1.0, f32::from(MAX_FLIGHT_TICKS)) as u16;
        self.add_projectile(Projectile {
            id: 0,
            kind,
            owner,
            ability,
            position: from,
            velocity: if kind == ProjectileKind::Missile {
                homing(delta, kind.speed())
            } else {
                ballistic(delta, remaining, kind.gravity())
            },
            flight: Flight::Tracking { target, remaining },
            age: 0,
            struck: [None; PIERCE_HITS - 1],
            bounces: 0,
        });
        Ok(())
    }

    /// `owner` shoots `kind` freely along `facing`; `ability` lands on whatever it hits.
    pub(crate) fn shoot_free(
        &mut self,
        owner: EntityRef,
        facing: u16,
        kind: ProjectileKind,
        ability: AbilityId,
    ) -> Result<(), ZoneError> {
        let from = if kind == ProjectileKind::Missile {
            let body = self
                .world
                .body(body_id(owner))
                .ok_or_else(|| ZoneError::new("shooter physics body is missing"))?;
            let [x, y, z] = body.exact_position();
            #[allow(clippy::cast_precision_loss)]
            let feet = y - body.half_extents().y as f32;
            [x, feet + MISSILE_FLIGHT_HEIGHT_UNITS, z]
        } else {
            self.launch_point(owner)?
        };
        let (x, z) = crate::trig::direction(facing);
        #[allow(clippy::cast_precision_loss)]
        let unit = |value: i32| value as f32 / crate::trig::TRIG_ONE as f32;
        let speed = kind.speed();
        self.add_projectile(Projectile {
            id: 0,
            kind,
            owner,
            ability,
            position: from,
            velocity: [unit(x) * speed, 0.0, unit(z) * speed],
            flight: Flight::Free,
            age: 0,
            struck: [None; PIERCE_HITS - 1],
            bounces: 0,
        });
        Ok(())
    }

    /// Every projectile flies, sticks or vanishes, in ID order.
    pub(crate) fn advance_projectiles(&mut self, now: u64) -> Result<(), ZoneError> {
        let ids: Vec<_> = self.projectiles.keys().copied().collect();
        for id in ids {
            let Some(mut projectile) = self.projectiles.get(&id).copied() else {
                continue;
            };
            if let Flight::Stuck {
                unit,
                offset,
                remaining,
            } = projectile.flight
            {
                if remaining <= 1 {
                    self.projectiles.remove(&id);
                    continue;
                }
                projectile.flight = Flight::Stuck {
                    unit,
                    offset,
                    remaining: remaining - 1,
                };
                if let Some(unit) = unit {
                    match self.unit_centre(unit) {
                        Some(centre) => projectile.position = add(centre, offset),
                        None => {
                            self.projectiles.remove(&id);
                            continue;
                        }
                    }
                }
                self.projectiles.insert(id, projectile);
                continue;
            }
            projectile.age += 1;
            if projectile.age > MAX_FLIGHT_TICKS {
                self.projectiles.remove(&id);
                continue;
            }
            if let Flight::Tracking { target, remaining } = projectile.flight {
                match self.unit_centre(target).filter(|_| self.unit_alive(target)) {
                    Some(centre) => {
                        let delta = sub(centre, projectile.position);
                        projectile.velocity = if projectile.kind == ProjectileKind::Missile {
                            homing(delta, projectile.kind.speed())
                        } else {
                            ballistic(delta, remaining, projectile.kind.gravity())
                        };
                        projectile.flight = Flight::Tracking {
                            target,
                            remaining: remaining.saturating_sub(1).max(1),
                        };
                    }
                    // The target is gone: the shot flies on as it was.
                    None => projectile.flight = Flight::Free,
                }
            }
            let segment = projectile.velocity;
            let hit = self.world.cast_ray(projectile.position, segment, |body| {
                self.projectile_meets(projectile.owner, body)
                    && entity_of(body).is_none_or(|unit| !projectile.passes(unit))
            });
            let Some(hit) = hit else {
                projectile.position = add(projectile.position, segment);
                if projectile.flight == Flight::Free {
                    projectile.velocity[1] -= projectile.kind.gravity();
                }
                self.projectiles.insert(id, projectile);
                continue;
            };
            let point = add(projectile.position, scale(segment, hit.fraction));
            projectile.position = point;
            let unit = entity_of(hit.body);
            let shot = projectile.shot();
            // A ricochet mirrors off a surface (not a unit) while it has bounces left.
            if unit.is_none() && shot == Shot::Ricochet && projectile.bounces < RICOCHET_BOUNCES {
                let n = hit.normal;
                let along = dot(projectile.velocity, n);
                projectile.velocity = sub(projectile.velocity, scale(n, 2.0 * along));
                projectile.position = add(point, scale(n, BOUNCE_CLEARANCE_UNITS));
                projectile.bounces += 1;
                self.projectiles.insert(id, projectile);
                if let Some(piece) = self.piece_of_body(hit.body) {
                    self.damage_piece(piece, crate::PROJECTILE_PIECE_DAMAGE, now)?;
                }
                continue;
            }
            // A piercing shot passes each enemy it lands on until its last hit.
            if let Some(victim) = unit
                && shot == Shot::Pierce
            {
                let struck = projectile.struck.iter().flatten().count();
                let percent = PIERCE_DAMAGE_PERCENT[struck];
                let landed = self.projectile_strikes(projectile, victim, percent, now)?;
                if landed && struck + 1 < PIERCE_HITS {
                    projectile.struck[struck] = Some(victim);
                    self.projectiles.insert(id, projectile);
                    continue;
                }
                projectile.flight = match self.unit_centre(victim) {
                    Some(centre) => Flight::Stuck {
                        unit: Some(victim),
                        offset: sub(point, centre),
                        remaining: STUCK_IN_UNIT_TICKS,
                    },
                    None => Flight::Stuck {
                        unit: None,
                        offset: point,
                        remaining: STUCK_TICKS,
                    },
                };
                self.projectiles.insert(id, projectile);
                continue;
            }
            projectile.flight = match unit.and_then(|unit| Some((unit, self.unit_centre(unit)?))) {
                Some((unit, centre)) => Flight::Stuck {
                    unit: Some(unit),
                    offset: sub(point, centre),
                    remaining: STUCK_IN_UNIT_TICKS,
                },
                None => Flight::Stuck {
                    unit: None,
                    offset: point,
                    remaining: STUCK_TICKS,
                },
            };
            self.projectiles.insert(id, projectile);
            if let Some(unit) = unit {
                self.projectile_strikes(projectile, unit, 100, now)?;
            } else if let Some(piece) = self.piece_of_body(hit.body) {
                // A shot into a destructible piece chips it (#305).
                self.damage_piece(piece, crate::PROJECTILE_PIECE_DAMAGE, now)?;
            }
        }
        Ok(())
    }

    /// Whether a body stops a shot by `owner`: everything but its shooter, the shooter's own side
    /// (safe players' shots pass players; creatures' shots pass creatures) and corpses.
    pub(crate) fn projectile_meets(&self, owner: EntityRef, body: BodyId) -> bool {
        if body == body_id(owner) {
            return false;
        }
        match entity_of(body) {
            None => true,
            Some(unit) => {
                let opposing = match (owner, unit) {
                    (EntityRef::Player(attacker), EntityRef::Player(victim)) => {
                        self.players_are_hostile(attacker, victim)
                    }
                    _ => std::mem::discriminant(&unit) != std::mem::discriminant(&owner),
                };
                opposing && self.unit_alive(unit)
            }
        }
    }

    fn unit_alive(&self, unit: EntityRef) -> bool {
        match unit {
            EntityRef::Player(id) => self
                .players
                .get(&id)
                .is_some_and(|player| player.is_alive()),
            EntityRef::Creature(id) => self
                .creatures
                .get(&id)
                .is_some_and(|creature| creature.is_alive()),
            EntityRef::Npc(_) => true,
        }
    }

    /// `projectile` reached `unit`: a raised shield turned toward it blocks it; otherwise its
    /// ability lands on a valid victim (the tracked target, or anything a free shot meets) at
    /// `percent` of its damage. Returns whether it landed on a hostile victim.
    fn projectile_strikes(
        &mut self,
        projectile: Projectile,
        unit: EntityRef,
        percent: u32,
        now: u64,
    ) -> Result<bool, ZoneError> {
        if let (EntityRef::Player(attacker), EntityRef::Player(victim)) = (projectile.owner, unit)
            && !self.players_are_hostile(attacker, victim)
        {
            return Ok(false);
        }
        let ability = crate::ability_by_id(projectile.ability)
            .ok_or_else(|| ZoneError::new("projectile names an unknown ability"))?;
        let from = sub(projectile.position, projectile.velocity);
        if let EntityRef::Player(player_id) = unit
            && let Some(lane) = self.shield_covers(player_id, from)?
        {
            self.update_player(player_id, |player| {
                player.combat_timer = crate::unit::COMBAT_LINGER_TICKS;
            });
            self.guard_feedback(
                player_id,
                ZoneEvent::Blocked {
                    source: projectile.owner,
                    target: unit,
                    lane,
                    amount: 0,
                },
            );
            return Ok(false);
        }
        match (projectile.owner, unit) {
            (EntityRef::Player(player_id), EntityRef::Creature(_) | EntityRef::Player(_)) => {
                if self.players.contains_key(&player_id) {
                    self.apply_player_effect_scaled(player_id, ability, Some(unit), percent, now)?;
                }
            }
            (EntityRef::Creature(creature_id), EntityRef::Player(player_id)) => {
                if self
                    .creatures
                    .get(&creature_id)
                    .is_some_and(|creature| creature.is_alive())
                {
                    self.creature_effect_lands(creature_id, player_id, ability)?;
                }
            }
            // Cover: an NPC in the way takes the shot without effect.
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// The projectiles a viewer at `centre` sees, nearest first.
    pub(crate) fn projectile_views(&self, centre: [i32; 3]) -> Vec<ProjectileView> {
        let radius = f64::from(crate::INTEREST_RADIUS_UNITS);
        let mut near: Vec<_> = self
            .projectiles
            .values()
            .filter_map(|projectile| {
                let [x, y, z] = crate::physics::to_units(projectile.position);
                let distance = (f64::from(x - centre[0])).hypot(f64::from(z - centre[2]));
                (distance <= radius).then(|| {
                    let (yaw, pitch) = orientation(projectile.velocity);
                    (
                        distance,
                        ProjectileView {
                            #[allow(clippy::cast_possible_truncation)]
                            id: projectile.id as u16,
                            kind: projectile.kind,
                            position: [x, y, z],
                            yaw,
                            pitch,
                            stuck: projectile.is_stuck(),
                        },
                    )
                })
            })
            .collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.id.cmp(&b.1.id)));
        near.into_iter()
            .take(MAX_VISIBLE_PROJECTILES)
            .map(|(_, view)| view)
            .collect()
    }
}
