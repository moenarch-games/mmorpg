//! Physical projectiles (#286, Atlas 5.2.71): arrows, crossbow bolts and magic missiles (#289).
//!
//! Under free aim (#517) a player's shot flies freely from where the player faces. It aims at a
//! unit (Atlas 5.2.i884): the selection when it lies in the aim cone (15° either side of the
//! facing, within the ability's range and in sight), otherwise the unit nearest the aim line in
//! that cone. The shot turns and pitches to meet that unit's body centre where it stands at
//! release, so short creatures and units uphill are hit; with nothing in the cone it leaves
//! along the facing. A free arrow, bolt or knife falls under gravity. A **tracking** shot (a
//! combo follow-up's, or a creature's) re-plans every tick the ballistic arc that lands on its
//! target's body centre after the remaining flight ticks, so it follows a moving target but
//! still flies a visible arc. Each tick a projectile sweeps the segment it travels with a
//! physics ray, so it never tunnels: walls, props, terrain and units in the way stop it, and a
//! Warden's raised shield turned toward it blocks it. The ability's effect lands only when the
//! shot reaches a valid victim. Spent projectiles stay stuck where they hit — in a wall for
//! [`STUCK_TICKS`], riding a unit for [`STUCK_IN_UNIT_TICKS`] — then vanish.
//!
//! A magic missile (Firebolt, Shadow Bolt, Smite, Wrath) ignores gravity: a free one flies in a
//! straight line at its aimed unit, or level along the facing, and can miss or be dodged; a
//! tracking one homes on its target, so only something in the way stops it.
//!
//! Aimed specials (#291) change what a meeting does: a Piercing Shot passes through up to
//! [`PIERCE_HITS`] enemies in a line, each later hit weaker ([`PIERCE_DAMAGE_PERCENT`]), and a
//! Ricochet Shot mirrors off walls, props and terrain up to [`RICOCHET_BOUNCES`] times about
//! the hit's surface normal. Walls stop a piercing shot; raised shields stop both.
//!
//! The Rogue's thrown weapons (#290) fly the same way. A throwing knife is a plain shot. A venom
//! flask and a bomb are lobbed in a high arc (onto the aimed unit within [`THROW_RANGE_UNITS`],
//! else along the facing): the flask shatters on the first thing it meets and splashes its
//! effect around that point; the bomb bounces off what it meets ([`BOMB_RESTITUTION`]), rolls
//! to rest under gravity and friction, and bursts when it touches a unit or after
//! [`BOMB_FUSE_TICKS`]. A burst stays visible for [`BURST_TICKS`] where it happened.
//!
//! Rapier gap noted for physics-engine parity: no continuous projectile body is used; a
//! per-tick ray sweep stands in for CCD on a thin, fast body. A bomb is likewise a swept point
//! with its own restitution and friction rather than a Rapier dynamic ball, so it bounces the
//! same on both backends and leaves no body in the world.

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
/// The aim cone of a free shot, as the cosine of its half-angle: the zone aims a shot at a unit
/// whose centre lies within [`crate::unit::AIM_CONE_HALF_WIDTH`] (15°) of the facing (#884).
/// Client aids such as the throw's landing marker use it.
pub const AIM_ASSIST_COS: f32 = 0.966;
/// A shielded creature (#160) blocks arrows and bolts arriving within this angle of its
/// facing; magic missiles pass its shield.
pub const CREATURE_SHIELD_HALF_ANGLE_DEGREES: f32 = 60.0;
/// A bounced shot restarts this far off the surface along its normal, units.
const BOUNCE_CLEARANCE_UNITS: f32 = 1.0;

/// At most this many projectiles fly or stick in one zone; the oldest goes first.
pub const MAX_PROJECTILES: usize = 256;
/// At most this many projectiles reach one player projection, nearest first.
pub const MAX_VISIBLE_PROJECTILES: usize = 16;
/// Flight speed of an arrow (54 m/s) and of a crossbow bolt (45 m/s), units per tick.
pub const ARROW_SPEED_UNITS: f32 = crate::f32_units_per_tick(5_400.0);
pub const BOLT_SPEED_UNITS: f32 = crate::f32_units_per_tick(4_500.0);
/// Flight speed of a magic missile (30 m/s), units per tick: slow enough to sidestep.
pub const MISSILE_SPEED_UNITS: f32 = crate::f32_units_per_tick(3_000.0);
/// Gravity on a free projectile: 9.81 m/s² in units per tick².
pub const PROJECTILE_GRAVITY_UNITS: f32 = crate::f32_units_per_tick_squared(981.0);
/// A projectile still flying after this many ticks (2 s) falls away.
pub const MAX_FLIGHT_TICKS: u16 = crate::ticks_from_ms(2_000);
/// Ticks a spent projectile stays stuck in a wall or prop (3 s) …
pub const STUCK_TICKS: u16 = crate::ticks_from_ms(3_000);
/// … or in a unit or its shield (1 s).
pub const STUCK_IN_UNIT_TICKS: u16 = crate::ticks_from_ms(1_000);
/// Shots leave this far above the shooter's body centre (chest height). A level free shot
/// drops to the ground about 32 m out.
pub const LAUNCH_HEIGHT_UNITS: f32 = 20.0;
/// A free missile leaves this high above the caster's feet (#289): it flies straight at the
/// aimed unit's body centre (#884), or level along the facing with nothing to aim at.
pub const MISSILE_FLIGHT_HEIGHT_UNITS: f32 = 50.0;
/// Flight speed of a throwing knife (36 m/s), units per tick (#290).
pub const KNIFE_SPEED_UNITS: f32 = crate::f32_units_per_tick(3_600.0);
/// A flask or bomb leaves at this horizontal speed (10 m/s) …
pub const THROW_SPEED_UNITS: f32 = crate::f32_units_per_tick(1_000.0);
/// … and, with nothing to aim at, this upward speed (5 m/s): it lands about 12 m out.
pub const THROW_RISE_UNITS: f32 = crate::f32_units_per_tick(500.0);
/// A throw arcs onto the nearest hostile straight ahead within this distance (15 m).
pub const THROW_RANGE_UNITS: i32 = 1_500;
/// A thrown flask still flying after this many ticks (3 s) falls away.
pub const THROWN_FLIGHT_TICKS: u16 = crate::ticks_from_ms(3_000);
/// A bomb bursts this long after the throw (2.5 s) unless a unit sets it off sooner.
pub const BOMB_FUSE_TICKS: u16 = crate::ticks_from_ms(2_500);
/// A bomb keeps this share of its speed into a surface as it bounces off …
pub const BOMB_RESTITUTION: f32 = 0.4;
/// … and this share of its speed along the surface on a bounce, or while rolling.
const BOMB_BOUNCE_FRICTION: f32 = 0.6;
const BOMB_ROLL_FRICTION: f32 = 0.88;
/// A rebound slower than this (0.6 m/s) settles the bomb onto the surface to roll.
const BOMB_REST_SPEED_UNITS: f32 = crate::f32_units_per_tick(60.0);
/// A burst (#290) stays visible this long (0.3 s) …
pub const BURST_TICKS: u16 = crate::ticks_from_ms(300);
/// … and spreads its effect from this far above the point it happened at.
const BURST_LIFT_UNITS: f32 = 15.0;

/// What flies; the wire value is its code.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ProjectileKind {
    Arrow,
    Bolt,
    /// A magic missile (#289): no gravity.
    Missile,
    /// A Necromancer's shadow missile (#301): flies like [`Self::Missile`], drawn in shadow.
    Shadow,
    /// A Cleric's holy missile (#303): flies like [`Self::Missile`], drawn in gold.
    Holy,
    /// A Druid's nature missile (#304): flies like [`Self::Missile`], drawn in green.
    Nature,
    /// A Rogue's throwing knife (#290): flies like an arrow.
    Knife,
    /// A Rogue's venom flask (#290): lobbed, shatters on impact.
    Flask,
    /// A Rogue's bomb (#290): lobbed, bounces and rolls, bursts on its fuse.
    Bomb,
}

impl ProjectileKind {
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Arrow => 1,
            Self::Bolt => 2,
            Self::Missile => 3,
            Self::Shadow => 4,
            Self::Holy => 5,
            Self::Nature => 6,
            Self::Knife => 7,
            Self::Flask => 8,
            Self::Bomb => 9,
        }
    }

    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Arrow),
            2 => Some(Self::Bolt),
            3 => Some(Self::Missile),
            4 => Some(Self::Shadow),
            5 => Some(Self::Holy),
            6 => Some(Self::Nature),
            7 => Some(Self::Knife),
            8 => Some(Self::Flask),
            9 => Some(Self::Bomb),
            _ => None,
        }
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Arrow => "arrow",
            Self::Bolt => "bolt",
            Self::Missile => "missile",
            Self::Shadow => "shadow",
            Self::Holy => "holy",
            Self::Nature => "nature",
            Self::Knife => "knife",
            Self::Flask => "flask",
            Self::Bomb => "bomb",
        }
    }

    /// Whether it is lobbed by hand in a high arc (#290): a flask or a bomb.
    #[must_use]
    pub const fn is_thrown(self) -> bool {
        matches!(self, Self::Flask | Self::Bomb)
    }

    /// Whether it is a spell missile: no gravity, homing and flying at body height.
    #[must_use]
    pub const fn is_magic(self) -> bool {
        matches!(
            self,
            Self::Missile | Self::Shadow | Self::Holy | Self::Nature
        )
    }

    #[must_use]
    pub const fn speed(self) -> f32 {
        match self {
            Self::Arrow => ARROW_SPEED_UNITS,
            Self::Bolt => BOLT_SPEED_UNITS,
            Self::Missile | Self::Shadow | Self::Holy | Self::Nature => MISSILE_SPEED_UNITS,
            Self::Knife => KNIFE_SPEED_UNITS,
            Self::Flask | Self::Bomb => THROW_SPEED_UNITS,
        }
    }

    /// Downward acceleration in flight, units per tick²: arrows, bolts and thrown weapons fall,
    /// missiles fly straight.
    #[must_use]
    pub const fn gravity(self) -> f32 {
        match self {
            Self::Arrow | Self::Bolt | Self::Knife | Self::Flask | Self::Bomb => {
                PROJECTILE_GRAVITY_UNITS
            }
            Self::Missile | Self::Shadow | Self::Holy | Self::Nature => 0.0,
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
    /// A flask or bomb burst here (#290); it shows for `remaining` more ticks.
    Burst { remaining: u16 },
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

    #[must_use]
    pub const fn is_burst(&self) -> bool {
        matches!(self.flight, Flight::Burst { .. })
    }

    /// Ticks it may fly before it falls away: longer for a lobbed flask (#290).
    fn max_flight(&self) -> u16 {
        if self.kind.is_thrown() {
            THROWN_FLIGHT_TICKS
        } else {
            MAX_FLIGHT_TICKS
        }
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

/// The horizontal length of `delta`; plain `sqrt`, correctly rounded on every target (#884).
fn xz_length(delta: [f32; 3]) -> f32 {
    (delta[0] * delta[0] + delta[2] * delta[2]).sqrt()
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
    /// A flask or bomb bursting where it is (#290).
    pub burst: bool,
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
    let yaw = turns(libm::atan2f(x, z)).rem_euclid(65_536) as u16;
    let pitch = turns(libm::atan2f(y, libm::hypotf(x, z))).clamp(-16_384, 16_384) as i16;
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
    let distance = libm::hypotf(libm::hypotf(delta[0], delta[1]), delta[2]);
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
        let distance = libm::hypotf(libm::hypotf(delta[0], delta[1]), delta[2]);
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
            velocity: if kind.is_magic() {
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

    /// `owner` shoots `kind` freely along `facing`; `ability` lands on whatever it hits. With an
    /// `aim` unit (the selection inside the aim cone, else the nearest unit in front, #884) the
    /// shot turns and pitches to meet that unit's body centre where it stands now: an arrow,
    /// bolt or knife arcs through it, a missile flies straight at it, and a throw lobs onto it
    /// within [`THROW_RANGE_UNITS`]. It still flies freely, so a unit that steps aside dodges it.
    pub(crate) fn shoot_free(
        &mut self,
        owner: EntityRef,
        facing: u16,
        kind: ProjectileKind,
        ability: AbilityId,
        aim: Option<EntityRef>,
    ) -> Result<(), ZoneError> {
        let from = if kind.is_magic() {
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
        let ahead = [unit(x), unit(z)];
        #[allow(clippy::cast_precision_loss)]
        let throw_range = THROW_RANGE_UNITS as f32;
        let delta = aim
            .and_then(|unit| self.unit_centre(unit))
            .map(|centre| sub(centre, from))
            .filter(|delta| !kind.is_thrown() || xz_length(*delta) <= throw_range);
        let velocity = match delta {
            Some(delta) if kind.is_magic() => {
                let distance = dot(delta, delta).sqrt();
                if distance > 0.0 {
                    scale(delta, speed / distance)
                } else {
                    [ahead[0] * speed, 0.0, ahead[1] * speed]
                }
            }
            Some(delta) => {
                // Level along the bearing at the kind's speed, rising to pass through the centre
                // after the ticks it takes to get there.
                let distance = xz_length(delta);
                let bearing = if distance > 0.0 {
                    [delta[0] / distance, delta[2] / distance]
                } else {
                    ahead
                };
                let ticks = (distance / speed).max(1.0);
                let rise = delta[1] / ticks + kind.gravity() * (ticks - 1.0) / 2.0;
                [bearing[0] * speed, rise, bearing[1] * speed]
            }
            // Nothing to aim at: a throw is lobbed high, anything else leaves level.
            None => {
                let rise = if kind.is_thrown() {
                    THROW_RISE_UNITS
                } else {
                    0.0
                };
                [ahead[0] * speed, rise, ahead[1] * speed]
            }
        };
        self.add_projectile(Projectile {
            id: 0,
            kind,
            owner,
            ability,
            position: from,
            velocity,
            flight: Flight::Free,
            age: 0,
            struck: [None; PIERCE_HITS - 1],
            bounces: 0,
        });
        Ok(())
    }

    /// The segment this projectile will actually travel, including this tick's tracking.
    /// Clashes and body collisions must use the same present-time trajectory.
    pub(crate) fn projectile_velocity(&self, projectile: &Projectile) -> [f32; 3] {
        if let Flight::Tracking { target, remaining } = projectile.flight
            && let Some(centre) = self.unit_centre(target).filter(|_| self.unit_alive(target))
        {
            let delta = sub(centre, projectile.position);
            if projectile.kind.is_magic() {
                homing(delta, projectile.kind.speed())
            } else {
                ballistic(delta, remaining, projectile.kind.gravity())
            }
        } else {
            projectile.velocity
        }
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
            if let Flight::Burst { remaining } = projectile.flight {
                if remaining <= 1 {
                    self.projectiles.remove(&id);
                } else {
                    projectile.flight = Flight::Burst {
                        remaining: remaining - 1,
                    };
                    self.projectiles.insert(id, projectile);
                }
                continue;
            }
            projectile.age += 1;
            let shot = projectile.shot();
            // A bomb's fuse runs out wherever it lies (#290).
            if shot == Shot::Bounce && projectile.age >= BOMB_FUSE_TICKS {
                self.burst(projectile, now)?;
                continue;
            }
            if projectile.age > projectile.max_flight() {
                self.projectiles.remove(&id);
                continue;
            }
            if let Flight::Tracking { target, remaining } = projectile.flight {
                match self.unit_centre(target).filter(|_| self.unit_alive(target)) {
                    Some(_) => {
                        projectile.velocity = self.projectile_velocity(&projectile);
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
            // A flask shatters on whatever it meets; a bomb bursts on a unit (#290).
            if shot == Shot::Shatter || (shot == Shot::Bounce && unit.is_some()) {
                projectile.position = add(point, scale(hit.normal, BOUNCE_CLEARANCE_UNITS));
                self.burst(projectile, now)?;
                continue;
            }
            // A bomb bounces off a surface, losing speed into it and along it; a weak rebound
            // settles it onto the surface, where it rolls on under gravity until friction stops
            // it (#290).
            if shot == Shot::Bounce {
                let n = hit.normal;
                let into = dot(projectile.velocity, n);
                let along = sub(projectile.velocity, scale(n, into));
                let rebound = -into * BOMB_RESTITUTION;
                let (rebound, friction) = if rebound < BOMB_REST_SPEED_UNITS {
                    (0.0, BOMB_ROLL_FRICTION)
                } else {
                    (rebound, BOMB_BOUNCE_FRICTION)
                };
                projectile.velocity = add(scale(along, friction), scale(n, rebound));
                projectile.position = add(point, scale(n, BOUNCE_CLEARANCE_UNITS));
                self.projectiles.insert(id, projectile);
                continue;
            }
            // A ricochet mirrors off a surface (not a unit) while it has bounces left.
            if unit.is_none() && shot == Shot::Ricochet && projectile.bounces < RICOCHET_BOUNCES {
                let n = hit.normal;
                let along = dot(projectile.velocity, n);
                projectile.velocity = sub(projectile.velocity, scale(n, 2.0 * along));
                projectile.position = add(point, scale(n, BOUNCE_CLEARANCE_UNITS));
                projectile.bounces += 1;
                self.projectiles.insert(id, projectile);
                if let Some(piece) = self.piece_of_body(hit.body) {
                    self.damage_piece(
                        piece,
                        crate::PROJECTILE_PIECE_DAMAGE,
                        now,
                        match projectile.owner {
                            crate::EntityRef::Player(player) => Some(player),
                            _ => None,
                        },
                    )?;
                }
                if let Some(obstacle) = self.summoned_of_body(hit.body) {
                    self.damage_summoned(obstacle, crate::PROJECTILE_SUMMONED_DAMAGE);
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
                self.damage_piece(
                    piece,
                    crate::PROJECTILE_PIECE_DAMAGE,
                    now,
                    match projectile.owner {
                        crate::EntityRef::Player(player) => Some(player),
                        _ => None,
                    },
                )?;
            } else if let Some(obstacle) = self.summoned_of_body(hit.body) {
                // A shot into summoned terrain (#293) chips it.
                self.damage_summoned(obstacle, crate::PROJECTILE_SUMMONED_DAMAGE);
            }
        }
        Ok(())
    }

    /// A flask or bomb bursts where it is (#290): it stays visible there for [`BURST_TICKS`] and
    /// its ability's splash or blast spreads from just above that point.
    fn burst(&mut self, mut projectile: Projectile, now: u64) -> Result<(), ZoneError> {
        projectile.velocity = [0.0; 3];
        projectile.flight = Flight::Burst {
            remaining: BURST_TICKS,
        };
        self.projectiles.insert(projectile.id, projectile);
        let EntityRef::Player(player_id) = projectile.owner else {
            return Ok(());
        };
        let ability = crate::ability_by_id(projectile.ability)
            .ok_or_else(|| ZoneError::new("projectile names an unknown ability"))?;
        let centre =
            crate::physics::to_units(add(projectile.position, [0.0, BURST_LIFT_UNITS, 0.0]));
        self.player_burst(player_id, ability, centre, now)
    }

    /// Whether a body stops a shot by `owner`: everything but its shooter, the shooter's own side
    /// (safe players' shots pass players; creatures' shots pass creatures; players' shots pass
    /// minions, #295) and corpses.
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
                    // Minions fight creatures only: players' shots pass them.
                    (EntityRef::Player(_), EntityRef::Minion(_)) => false,
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
            EntityRef::Minion(id) => self.minions.contains_key(&id),
        }
    }

    /// Whether a living shielded creature (the Redbrand Brute, #160) faces a shot arriving from
    /// `from`: within [`CREATURE_SHIELD_HALF_ANGLE_DEGREES`] of its facing. A stunned creature
    /// drops its guard.
    fn creature_shield_covers(
        &self,
        creature_id: crate::CreatureId,
        from: [f32; 3],
    ) -> Result<bool, ZoneError> {
        let Some(creature) = self.creatures.get(&creature_id) else {
            return Ok(false);
        };
        let (spawn, _) = Self::creature_content(&self.content, creature_id)?;
        if !self.content.is_shielded(spawn.template)
            || !creature.is_alive()
            || crate::aura::has_kind(&creature.auras, crate::AuraKind::Stun)
        {
            return Ok(false);
        }
        let Some(centre) = self.unit_centre(EntityRef::Creature(creature_id)) else {
            return Ok(false);
        };
        let (x, z) = crate::trig::direction(creature.facing);
        #[allow(clippy::cast_precision_loss)]
        let forward = [x as f32, z as f32];
        let delta = [from[0] - centre[0], from[2] - centre[2]];
        let along = delta[0] * forward[0] + delta[1] * forward[1];
        let length = libm::hypotf(delta[0], delta[1]) * libm::hypotf(forward[0], forward[1]);
        Ok(length > 0.0
            && along >= length * libm::cosf(CREATURE_SHIELD_HALF_ANGLE_DEGREES.to_radians()))
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
        if let EntityRef::Creature(creature_id) = unit
            && !projectile.kind.is_magic()
            && self.creature_shield_covers(creature_id, from)?
        {
            self.notify_units(
                [Some(projectile.owner), Some(unit)],
                ZoneEvent::Blocked {
                    source: projectile.owner,
                    target: unit,
                    lane: crate::StrikeHeight::Middle,
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
            // A minion in the way (#295) takes the shot meant for a player.
            (
                EntityRef::Creature(creature_id),
                victim @ (EntityRef::Player(_) | EntityRef::Minion(_)),
            ) => {
                if self
                    .creatures
                    .get(&creature_id)
                    .is_some_and(|creature| creature.is_alive())
                {
                    self.creature_effect_lands(creature_id, victim, ability)?;
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
                let distance = libm::hypot(f64::from(x - centre[0]), f64::from(z - centre[2]));
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
                            burst: projectile.is_burst(),
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
