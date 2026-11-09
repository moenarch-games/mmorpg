//! World traps and triggers (#300, Atlas 5.2.i300): a shared authoritative core that authored
//! traps use now and a Rogue's player-placed traps (#302) can reuse.
//!
//! A [`Trap`] is plain data: a [`TrapTrigger`] (a pressure plate, or always live), a
//! [`TrapEffect`] (falling rocks, a swinging blade, spikes) and the damage it deals. The zone
//! keeps one [`TrapRuntime`] per trap: the tick its plate fired and, per unit, the tick from
//! which it may hit that unit again. Both are canonical ([`CanonicalTrap`]), so a snapshot,
//! a replay or a handoff reproduces every activation and never deals a hit twice.
//!
//! Everything else is a pure function of the zone tick. A blade's pose is
//! [`Blade::pose_at`], a schedule like a moving platform's that needs no state. Falling rocks
//! are [`Rockfall::rock_at`], a fixed fall from the fired tick. Clients derive the same motion
//! from content, the projection's tick and a fired trap's age ([`TrapView`]).
//!
//! Every outcome is an exact overlap with the unit's own capsule, never its bound: a blade's
//! head is an oriented box ([`Blade`]), a rock a sphere, a spike field or plate a volume at the
//! unit's feet. Blades are damage-only volumes and rocks fall along their fixed line (the owner
//! decided, 2026-11-07), so a trap never moves or blocks a unit; solid rotating bodies are later
//! work upstream. Traps hit players, creatures and minions alike; props and NPCs are left alone.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::entity::{EntityRef, body_id};
use crate::physics::{RigidBody, capsule_dimensions};
use crate::{ZoneError, ZoneEvent, ZoneSimulation, trig};

/// Traps a zone's content may author.
pub const MAX_TRAPS: usize = 32;
/// Fired traps one player's projection names, nearest first.
pub const MAX_VISIBLE_TRAPS: usize = 4;
/// Units a trap remembers having hit recently.
pub const MAX_TRAP_HITS: usize = 32;
/// Rocks one fall can drop.
pub const MAX_ROCKS: usize = 8;

/// Rocks fall at this acceleration (milliunits per tick², about 9.8 m/s²).
const ROCK_GRAVITY_MILLIUNITS: i64 = 1_090;
/// How far below a volume's floor feet still count (a unit standing a little sunken).
const FLOOR_MARGIN_UNITS: f32 = 8.0;

/// A trap's stable authored ID.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TrapId(u8);

impl TrapId {
    #[must_use]
    pub const fn new(value: u8) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

/// A volume over the ground: a rectangle in XZ from `floor` up to `height` above it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TriggerVolume {
    /// Centre of the rectangle in XZ units.
    pub centre: [i32; 2],
    /// Half widths in XZ units.
    pub half: [i32; 2],
    /// Ground height under the volume.
    pub floor: i32,
    /// How far above the floor feet still count: a jump clears it.
    pub height: i32,
}

impl TriggerVolume {
    /// Whether feet at `feet` stand in the volume.
    #[must_use]
    pub fn covers(&self, feet: [f32; 3]) -> bool {
        #[allow(clippy::cast_precision_loss)]
        let centre = self.centre.map(|value| value as f32);
        #[allow(clippy::cast_precision_loss)]
        let half = self.half.map(|value| value as f32);
        #[allow(clippy::cast_precision_loss)]
        let (floor, height) = (self.floor as f32, self.height as f32);
        (feet[0] - centre[0]).abs() <= half[0]
            && (feet[2] - centre[1]).abs() <= half[1]
            && feet[1] >= floor - FLOOR_MARGIN_UNITS
            && feet[1] <= floor + height
    }

    fn is_valid(&self) -> bool {
        let limit = crate::MAX_CONTENT_COORDINATE_UNITS;
        self.half.iter().all(|&half| half > 0)
            && self.height > 0
            && self.centre.iter().all(|&axis| axis.abs() <= limit)
            && self.floor.abs() <= limit
    }
}

/// What sets a trap off.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrapTrigger {
    /// Fires when a living unit's feet stand on the plate; it re-arms `rearm_ticks` after
    /// firing, whatever stands on it.
    Pressure {
        plate: TriggerVolume,
        rearm_ticks: u32,
    },
    /// Always live, on its effect's schedule or volume.
    Live,
}

/// One rock of a fall: where it hangs in XZ and how many ticks after firing it lets go.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RockDrop {
    pub at: [i32; 2],
    pub delay: u16,
}

/// Falling rocks: each drops from `start_y` to the floor at `floor`, a sphere of `radius`.
/// A rock hurts what it overlaps on the way down; once landed it is rubble and inert.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Rockfall {
    pub rocks: Vec<RockDrop>,
    pub start_y: i32,
    pub floor: i32,
    pub radius: i32,
    /// Ticks the rubble of a landed rock lies before it clears.
    pub rubble_ticks: u16,
}

/// A rock's motion at `age` ticks after the trap fired.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RockPose {
    pub centre: [i32; 3],
    /// Still falling: it can hit.
    pub falling: bool,
    /// Gone, its rubble cleared.
    pub cleared: bool,
}

impl Rockfall {
    /// Centre height of a landed rock.
    const fn resting_y(&self) -> i32 {
        self.floor + self.radius
    }

    /// Ticks after firing until `rock` lands.
    #[must_use]
    pub fn landing_age(&self, rock: &RockDrop) -> u32 {
        // Time to fall the drop under `ROCK_GRAVITY_MILLIUNITS`.
        let drop = i64::from((self.start_y - self.resting_y()).max(0));
        let doubled = u64::try_from(drop.saturating_mul(2_000)).unwrap_or(0);
        let ticks = trig::isqrt(doubled / ROCK_GRAVITY_MILLIUNITS.unsigned_abs());
        let ticks = u32::try_from(ticks).unwrap_or(u32::MAX);
        u32::from(rock.delay) + ticks + 1
    }

    /// Where `rock` is `age` ticks after firing.
    #[must_use]
    pub fn rock_at(&self, rock: &RockDrop, age: u32) -> RockPose {
        let [x, z] = rock.at;
        let resting = self.resting_y();
        if age <= u32::from(rock.delay) {
            return RockPose {
                centre: [x, self.start_y, z],
                falling: false,
                cleared: false,
            };
        }
        let fallen = i64::from(age - u32::from(rock.delay));
        // Whole units fallen: ½ g t².
        let drop = ROCK_GRAVITY_MILLIUNITS * fallen * fallen / 2_000;
        let y = i64::from(self.start_y) - drop;
        let falling = y > i64::from(resting);
        let cleared = !falling && age >= self.landing_age(rock) + u32::from(self.rubble_ticks);
        RockPose {
            centre: [
                x,
                i32::try_from(y.max(i64::from(resting))).unwrap_or(resting),
                z,
            ],
            falling,
            cleared,
        }
    }

    /// Ticks after firing until the last rock's rubble is gone.
    #[must_use]
    pub fn cleared_age(&self) -> u32 {
        self.rocks
            .iter()
            .map(|rock| self.landing_age(rock) + u32::from(self.rubble_ticks))
            .max()
            .unwrap_or(0)
    }
}

/// A pendulum blade: a head at the end of an arm, swinging about a horizontal hinge at
/// `pivot`, its angle a fixed function of the zone tick.
///
/// The hit volume is the head: an oriented box of `half_extents`
/// `[across the swing, along the arm, along the hinge]` centred `arm` units down the arm.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Blade {
    pub pivot: [i32; 3],
    pub arm: i32,
    /// `[across the swing, along the arm, along the hinge]`, units.
    pub half_extents: [i32; 3],
    /// The swing's horizontal direction, as a yaw.
    pub swing_yaw: u16,
    /// Greatest angle either side of straight down, as a yaw.
    pub amplitude: u16,
    /// Ticks of one full swing, there and back.
    pub period: u32,
    /// Ticks added to the zone tick, so blades can swing out of step.
    pub phase: u32,
}

/// A blade's head at one tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BladePose {
    pub head: [f64; 3],
    /// From the pivot toward the head.
    pub arm: [f64; 3],
    /// Across the swing, perpendicular to the arm and the hinge.
    pub across: [f64; 3],
    /// Along the hinge.
    pub hinge: [f64; 3],
    /// The arm's angle from straight down, in radians, positive along the swing.
    pub angle: f64,
}

impl Blade {
    /// Yaw units of the blade's swing at zone `tick`, from straight down.
    #[must_use]
    pub fn angle_at(&self, tick: u64) -> i32 {
        let period = u64::from(self.period.max(1));
        let phased = (tick + u64::from(self.phase)) % period;
        // A whole circle per period, as a yaw (the remainder is below 2^16).
        let turn = u16::try_from(phased * 65_536 / period).unwrap_or(0);
        let swing = i64::from(self.amplitude) * i64::from(trig::sin(turn));
        i32::try_from(swing / i64::from(trig::TRIG_ONE)).unwrap_or(0)
    }

    /// The head's pose at zone `tick`.
    #[must_use]
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub fn pose_at(&self, tick: u64) -> BladePose {
        let one = f64::from(trig::TRIG_ONE);
        let (sx, sz) = trig::direction(self.swing_yaw);
        let swing = [f64::from(sx) / one, f64::from(sz) / one];
        let angle = self.angle_at(tick);
        let turned = angle.rem_euclid(65_536) as u16;
        let (sin, cos) = (
            f64::from(trig::sin(turned)) / one,
            f64::from(trig::cos(turned)) / one,
        );
        let arm = [sin * swing[0], -cos, sin * swing[1]];
        let across = [cos * swing[0], sin, cos * swing[1]];
        let hinge = [swing[1], 0.0, -swing[0]];
        let reach = f64::from(self.arm);
        let head = [
            f64::from(self.pivot[0]) + reach * arm[0],
            f64::from(self.pivot[1]) + reach * arm[1],
            f64::from(self.pivot[2]) + reach * arm[2],
        ];
        BladePose {
            head,
            arm,
            across,
            hinge,
            angle: f64::from(angle) / 65_536.0 * std::f64::consts::TAU,
        }
    }

    /// Greatest horizontal distance the head reaches from under the pivot.
    fn reach(&self) -> i32 {
        self.arm + self.half_extents.into_iter().max().unwrap_or(0)
    }

    fn is_valid(&self) -> bool {
        let limit = crate::MAX_CONTENT_COORDINATE_UNITS;
        self.arm > 0
            && self.period >= 2
            && self.half_extents.iter().all(|&half| half > 0)
            && self.pivot.iter().all(|&axis| axis.abs() <= limit)
    }
}

/// What a trap does when live.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TrapEffect {
    Rockfall(Rockfall),
    Blade(Blade),
    /// Spikes: hurts feet standing in the volume (a jump clears it).
    Spikes(TriggerVolume),
}

/// What a client draws a trap as. The wire and identity code is stable.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum TrapKind {
    Rockfall,
    Blade,
    Spikes,
}

/// One trap.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Trap {
    pub id: TrapId,
    pub trigger: TrapTrigger,
    pub effect: TrapEffect,
    /// Damage of one hit.
    pub damage: u16,
    /// Ticks before this trap may hit the same unit again.
    pub rehit_ticks: u16,
}

impl Trap {
    #[must_use]
    pub const fn kind(&self) -> TrapKind {
        match self.effect {
            TrapEffect::Rockfall(_) => TrapKind::Rockfall,
            TrapEffect::Blade(_) => TrapKind::Blade,
            TrapEffect::Spikes(_) => TrapKind::Spikes,
        }
    }

    /// The XZ point and radius that bound everything the trap can touch.
    #[must_use]
    fn bound(&self) -> ([i32; 2], i32) {
        match &self.effect {
            TrapEffect::Blade(blade) => ([blade.pivot[0], blade.pivot[2]], blade.reach()),
            TrapEffect::Spikes(volume) => (volume.centre, volume.half[0].max(volume.half[1])),
            TrapEffect::Rockfall(fall) => {
                let [cx, cz] = fall.rocks.first().map_or([0, 0], |rock| rock.at);
                let extent = fall
                    .rocks
                    .iter()
                    .map(|rock| (rock.at[0] - cx).abs().max((rock.at[1] - cz).abs()))
                    .max()
                    .unwrap_or(0);
                let reach = match self.trigger {
                    TrapTrigger::Pressure { plate, .. } => extent
                        .max((plate.centre[0] - cx).abs() + plate.half[0])
                        .max((plate.centre[1] - cz).abs() + plate.half[1]),
                    TrapTrigger::Live => extent,
                };
                ([cx, cz], reach + fall.radius)
            }
        }
    }

    /// The point where a client's fired-trap view is centred.
    #[must_use]
    pub fn anchor(&self) -> [i32; 2] {
        match self.trigger {
            TrapTrigger::Pressure { plate, .. } => plate.centre,
            TrapTrigger::Live => self.bound().0,
        }
    }

    fn is_valid(&self) -> bool {
        let trigger_valid = match self.trigger {
            TrapTrigger::Pressure { plate, rearm_ticks } => plate.is_valid() && rearm_ticks > 0,
            TrapTrigger::Live => true,
        };
        let effect_valid = match &self.effect {
            TrapEffect::Rockfall(fall) => {
                !fall.rocks.is_empty()
                    && fall.rocks.len() <= MAX_ROCKS
                    && fall.radius > 0
                    && fall.start_y > fall.floor + fall.radius
                    && matches!(self.trigger, TrapTrigger::Pressure { rearm_ticks, .. } if rearm_ticks >= fall.cleared_age())
            }
            TrapEffect::Blade(blade) => blade.is_valid(),
            TrapEffect::Spikes(volume) => volume.is_valid(),
        };
        trigger_valid && effect_valid && self.damage > 0 && self.rehit_ticks > 0
    }
}

/// Validates a zone's authored traps: at most [`MAX_TRAPS`], in ID order, each sound.
pub(crate) fn validate(traps: &[Trap]) -> Result<(), ZoneError> {
    if traps.len() > MAX_TRAPS {
        return Err(ZoneError::new("zone trap capacity reached"));
    }
    if traps.windows(2).any(|pair| pair[0].id >= pair[1].id) {
        return Err(ZoneError::new("traps need ordered unique IDs"));
    }
    if traps.iter().any(|trap| !trap.is_valid()) {
        return Err(ZoneError::new("trap is invalid"));
    }
    Ok(())
}

/// The fired trap a viewer's projection names. Spikes and blades are drawn from content and
/// the tick alone, so only a fired pressure trap has one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TrapView {
    pub id: TrapId,
    /// Ticks since it fired.
    pub age: u16,
}

/// A trap's zone state.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TrapRuntime {
    /// The tick a pressure trap last fired, until it re-arms.
    pub fired_at: Option<u64>,
    /// Per unit, the tick from which it may be hit again.
    pub hits: BTreeMap<EntityRef, u64>,
}

/// One remembered hit in a canonical snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CanonicalTrapHit {
    pub unit: EntityRef,
    /// The tick from which this trap may hit the unit again.
    pub until: u64,
}

/// A trap's canonical state, in trap-ID order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalTrap {
    pub id: TrapId,
    pub fired_at: Option<u64>,
    /// In unit order.
    pub hits: Vec<CanonicalTrapHit>,
}

/// Squared distance from `point` to the box of `half` centred on the origin.
fn point_box_distance_squared(point: [f64; 3], half: [f64; 3]) -> f64 {
    (0..3)
        .map(|axis| {
            let outside = (point[axis].abs() - half[axis]).max(0.0);
            outside * outside
        })
        .sum()
}

/// Squared distance from the segment `from`→`to` to the box of `half` centred on the origin.
///
/// Along the segment the squared distance is convex and piecewise quadratic, with breaks where
/// an axis crosses a face plane. The minimum over each piece has a closed form, so the result
/// is exact; only basic arithmetic takes part, and a fixed-size sort, so it is deterministic.
fn segment_box_distance_squared(from: [f64; 3], to: [f64; 3], half: [f64; 3]) -> f64 {
    let delta = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    // At most the two ends and two face crossings per axis.
    let mut breaks = [0.0_f64; 8];
    breaks[1] = 1.0;
    let mut count = 2;
    for axis in 0..3 {
        if delta[axis] == 0.0 {
            continue;
        }
        for face in [-half[axis], half[axis]] {
            let t = (face - from[axis]) / delta[axis];
            if t > 0.0 && t < 1.0 {
                breaks[count] = t;
                count += 1;
            }
        }
    }
    let breaks = &mut breaks[..count];
    breaks.sort_by(f64::total_cmp);
    let at = |t: f64| {
        [
            from[0] + t * delta[0],
            from[1] + t * delta[1],
            from[2] + t * delta[2],
        ]
    };
    let mut best = f64::INFINITY;
    for pair in breaks.windows(2) {
        let (start, end) = (pair[0], pair[1]);
        let mid = at(f64::midpoint(start, end));
        // The outside faces on this piece, and the quadratic they sum to.
        let (mut quadratic, mut linear) = (0.0, 0.0);
        for axis in 0..3 {
            let outside = if mid[axis] > half[axis] {
                half[axis]
            } else if mid[axis] < -half[axis] {
                -half[axis]
            } else {
                continue;
            };
            quadratic += delta[axis] * delta[axis];
            linear += delta[axis] * (from[axis] - outside);
        }
        let vertex = if quadratic > 0.0 {
            (-linear / quadratic).clamp(start, end)
        } else {
            start
        };
        best = best.min(point_box_distance_squared(at(vertex), half));
    }
    best
}

/// A unit a trap can hit: its capsule.
struct TrapTarget {
    unit: EntityRef,
    centre: [f32; 3],
    half_segment: f32,
    radius: f32,
    /// The capsule's bottom.
    feet: [f32; 3],
}

impl TrapTarget {
    /// Whether the sphere of `radius` at `centre` touches the capsule.
    fn touches_sphere(&self, centre: [i32; 3], radius: i32) -> bool {
        let [ux, uy, uz] = self.centre.map(f64::from);
        let [cx, cy, cz] = centre.map(f64::from);
        let segment = f64::from(self.half_segment);
        let closest_y = cy.clamp(uy - segment, uy + segment);
        let (dx, dy, dz) = (cx - ux, cy - closest_y, cz - uz);
        let reach = f64::from(radius) + f64::from(self.radius);
        dx * dx + dy * dy + dz * dz <= reach * reach
    }

    /// Whether a blade's head at `pose` touches the capsule.
    fn touches_blade(&self, blade: &Blade, pose: &BladePose) -> bool {
        let [ux, uy, uz] = self.centre.map(f64::from);
        let segment = f64::from(self.half_segment);
        let half = blade.half_extents.map(f64::from);
        // The capsule axis in the head's frame.
        let local = |point: [f64; 3]| {
            let relative = [
                point[0] - pose.head[0],
                point[1] - pose.head[1],
                point[2] - pose.head[2],
            ];
            let dot = |axis: [f64; 3]| {
                axis[0] * relative[0] + axis[1] * relative[1] + axis[2] * relative[2]
            };
            [dot(pose.across), dot(pose.arm), dot(pose.hinge)]
        };
        let bottom = local([ux, uy - segment, uz]);
        let top = local([ux, uy + segment, uz]);
        let reach = f64::from(self.radius);
        segment_box_distance_squared(bottom, top, half) <= reach * reach
    }
}

impl ZoneSimulation {
    /// Resets every authored trap's runtime state: armed, with no remembered hits.
    pub(crate) fn fresh_traps(content: &crate::ZoneContent) -> BTreeMap<TrapId, TrapRuntime> {
        content
            .traps()
            .iter()
            .map(|trap| (trap.id, TrapRuntime::default()))
            .collect()
    }

    /// Living units near `centre` a trap may act on: players, creatures and minions, in
    /// `EntityRef` order. Props, NPCs, bosses and evading creatures are left out.
    fn trap_targets(&self, centre: [i32; 2], reach: i32) -> Result<Vec<TrapTarget>, ZoneError> {
        let mut targets = Vec::new();
        for unit in self.interest.near(centre[0], centre[1], reach + 100) {
            let alive = match unit {
                EntityRef::Player(id) => self.players.get(&id).is_some_and(|p| p.is_alive()),
                EntityRef::Creature(id) => {
                    if self.is_boss(id)? {
                        continue;
                    }
                    self.creatures.get(&id).is_some_and(|creature| {
                        creature.is_alive()
                            && !matches!(creature.ai, crate::CreatureAi::Evading { .. })
                    }) && !self.is_prop(unit)
                }
                EntityRef::Minion(id) => self.minions.contains_key(&id),
                EntityRef::Npc(_) => false,
            };
            if !alive {
                continue;
            }
            let Some(body) = self.world.body(body_id(unit)) else {
                continue;
            };
            targets.push(Self::trap_target(unit, body));
        }
        Ok(targets)
    }

    fn trap_target(unit: EntityRef, body: &RigidBody) -> TrapTarget {
        let half_extents = body.half_extents();
        let (half_segment, radius) = capsule_dimensions(half_extents);
        let centre = body.exact_position();
        #[allow(clippy::cast_precision_loss)]
        let feet = [centre[0], centre[1] - half_extents.y as f32, centre[2]];
        TrapTarget {
            unit,
            centre,
            half_segment,
            radius,
            feet,
        }
    }

    /// Step 6 of the tick: every trap in ID order re-arms, fires and hits the units it now
    /// touches, each at most once per its re-hit window.
    pub(crate) fn advance_traps(&mut self, now: u64) -> Result<(), ZoneError> {
        let content = Arc::clone(&self.content);
        for trap in content.traps() {
            let Some(state) = self.traps.get_mut(&trap.id) else {
                continue;
            };
            state.hits.retain(|_, until| *until > now);
            if let TrapTrigger::Pressure { rearm_ticks, .. } = trap.trigger
                && state
                    .fired_at
                    .is_some_and(|fired| now >= fired + u64::from(rearm_ticks))
            {
                state.fired_at = None;
            }
            let (centre, reach) = trap.bound();
            let targets = self.trap_targets(centre, reach)?;
            if let TrapTrigger::Pressure { plate, .. } = trap.trigger
                && self
                    .traps
                    .get(&trap.id)
                    .is_some_and(|state| state.fired_at.is_none())
                && targets.iter().any(|target| plate.covers(target.feet))
                && let Some(state) = self.traps.get_mut(&trap.id)
            {
                state.fired_at = Some(now);
            }
            self.trap_strikes(trap, &targets, now)?;
        }
        Ok(())
    }

    /// Applies `trap`'s effect to the `targets` it touches this tick.
    fn trap_strikes(
        &mut self,
        trap: &Trap,
        targets: &[TrapTarget],
        now: u64,
    ) -> Result<(), ZoneError> {
        let fired = self.traps.get(&trap.id).and_then(|state| state.fired_at);
        let touched = |target: &TrapTarget| -> bool {
            match &trap.effect {
                TrapEffect::Spikes(volume) => volume.covers(target.feet),
                TrapEffect::Blade(blade) => {
                    let pose = blade.pose_at(now);
                    target.touches_blade(blade, &pose)
                }
                TrapEffect::Rockfall(fall) => fired.is_some_and(|tick| {
                    let age = u32::try_from(now.saturating_sub(tick)).unwrap_or(u32::MAX);
                    fall.rocks.iter().any(|rock| {
                        let pose = fall.rock_at(rock, age);
                        pose.falling && target.touches_sphere(pose.centre, fall.radius)
                    })
                }),
            }
        };
        let victims: Vec<EntityRef> = targets
            .iter()
            .filter(|target| touched(target))
            .map(|target| target.unit)
            .collect();
        for unit in victims {
            let Some(state) = self.traps.get_mut(&trap.id) else {
                continue;
            };
            if state.hits.contains_key(&unit) || (state.hits.len() >= MAX_TRAP_HITS) {
                continue;
            }
            state.hits.insert(unit, now + u64::from(trap.rehit_ticks));
            self.notify_trap_hit(trap.id, unit, trap.damage);
            self.trap_damages(unit, u32::from(trap.damage), now)?;
        }
        Ok(())
    }

    fn notify_trap_hit(&mut self, trap: TrapId, target: EntityRef, amount: u16) {
        let refs = [target];
        self.notify_observers(
            &refs,
            &std::collections::BTreeSet::new(),
            ZoneEvent::TrapHit {
                trap,
                target,
                amount,
            },
        );
    }

    /// Damage from a trap: no credit and no threat. Players take it like other environmental
    /// damage; a creature's remaining rewards still go to the player who tapped it.
    fn trap_damages(&mut self, unit: EntityRef, amount: u32, now: u64) -> Result<(), ZoneError> {
        match unit {
            EntityRef::Player(id) => {
                self.apply_environmental_damage(id, amount)?;
            }
            EntityRef::Creature(id) => {
                let Some(creature) = self.creatures.get_mut(&id) else {
                    return Ok(());
                };
                let dealt = amount.min(creature.health);
                creature.health -= dealt;
                creature.combat_timer = crate::unit::COMBAT_LINGER_TICKS;
                if creature.health == 0 {
                    self.creature_dies(id, None, now)?;
                } else {
                    self.break_roots(unit);
                }
            }
            EntityRef::Minion(id) => {
                let Some(minion) = self.minions.get_mut(&id) else {
                    return Ok(());
                };
                let dealt = amount.min(minion.health);
                minion.health -= dealt;
                if minion.health == 0 {
                    self.minion_crumbles(id, None)?;
                }
            }
            EntityRef::Npc(_) => {}
        }
        Ok(())
    }

    /// The fired traps within interest of `centre`, nearest first.
    pub(crate) fn trap_views(&self, centre: [i32; 3]) -> Vec<TrapView> {
        let radius = i64::from(crate::INTEREST_RADIUS_UNITS).pow(2);
        let mut near: Vec<(i64, TrapView)> = self
            .content
            .traps()
            .iter()
            .filter_map(|trap| {
                let fired = self.traps.get(&trap.id)?.fired_at?;
                let [x, z] = trap.anchor();
                let (dx, dz) = (i64::from(x - centre[0]), i64::from(z - centre[2]));
                let squared = dx * dx + dz * dz;
                (squared <= radius).then(|| {
                    let age = self.tick.saturating_sub(fired);
                    (
                        squared,
                        TrapView {
                            id: trap.id,
                            age: u16::try_from(age).unwrap_or(u16::MAX),
                        },
                    )
                })
            })
            .collect();
        near.sort_by_key(|(squared, view)| (*squared, view.id));
        near.into_iter()
            .take(MAX_VISIBLE_TRAPS)
            .map(|(_, view)| view)
            .collect()
    }

    /// Every trap's canonical state, in trap-ID order.
    pub(crate) fn canonical_traps(&self) -> Vec<CanonicalTrap> {
        self.traps
            .iter()
            .map(|(&id, state)| CanonicalTrap {
                id,
                fired_at: state.fired_at,
                hits: state
                    .hits
                    .iter()
                    .map(|(&unit, &until)| CanonicalTrapHit { unit, until })
                    .collect(),
            })
            .collect()
    }

    /// Restores trap state: every authored trap once, in ID order, with hits in unit order.
    pub(crate) fn restore_traps(&mut self, traps: &[CanonicalTrap]) -> Result<(), ZoneError> {
        if traps.len() != self.traps.len()
            || !traps
                .iter()
                .zip(self.traps.keys())
                .all(|(record, id)| record.id == *id && record.hits.len() <= MAX_TRAP_HITS)
        {
            return Err(ZoneError::new("snapshot traps do not match the zone"));
        }
        for record in traps {
            if record
                .hits
                .windows(2)
                .any(|pair| pair[0].unit >= pair[1].unit)
            {
                return Err(ZoneError::new("snapshot trap hits are out of order"));
            }
            let state = self
                .traps
                .get_mut(&record.id)
                .ok_or_else(|| ZoneError::new("snapshot traps do not match the zone"))?;
            state.fired_at = record.fired_at;
            state.hits = record
                .hits
                .iter()
                .map(|hit| (hit.unit, hit.until))
                .collect();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn segment_distance_to_a_box_is_exact() {
        let half = [1.0, 2.0, 3.0];
        // Inside, and straight along a face.
        assert!(close(
            segment_box_distance_squared([0.0; 3], [0.5, 0.5, 0.5], half),
            0.0
        ));
        assert!(close(
            segment_box_distance_squared([3.0, 0.0, 0.0], [3.0, 1.0, 0.0], half),
            4.0
        ));
        // A segment that passes beside a corner, closest at the corner's level.
        let d = segment_box_distance_squared([2.0, 3.0, 0.0], [2.0, 5.0, 0.0], half);
        assert!(close(d, 2.0), "{d}");
        // A diagonal segment that clips the box.
        let clip = segment_box_distance_squared([2.0, 0.0, 0.0], [0.0, 3.0, 0.0], half);
        assert!(close(clip, 0.0), "{clip}");
    }

    #[test]
    fn a_blade_is_a_pure_function_of_the_tick() {
        let blade = Blade {
            pivot: [0, 400, 0],
            arm: 300,
            half_extents: [20, 60, 70],
            swing_yaw: 0,
            amplitude: 7_000,
            period: 90,
            phase: 0,
        };
        assert_eq!(blade.angle_at(0), 0);
        assert_eq!(blade.angle_at(90), 0);
        assert_eq!(blade.angle_at(22), -blade.angle_at(67));
        assert!(blade.angle_at(22) > blade.angle_at(11));
        let pose = blade.pose_at(0);
        assert!(close(pose.head[1], 100.0));
        let tilted = blade.pose_at(23);
        assert!(tilted.head[2] > 150.0, "{:?}", tilted.head);
        assert!(close(
            tilted.arm[0] * tilted.across[0]
                + tilted.arm[1] * tilted.across[1]
                + tilted.arm[2] * tilted.across[2],
            0.0
        ));
    }

    #[test]
    fn a_rock_waits_then_falls_then_lies_cleared() {
        let fall = Rockfall {
            rocks: vec![RockDrop {
                at: [0, 0],
                delay: 6,
            }],
            start_y: 400,
            floor: 0,
            radius: 25,
            rubble_ticks: 30,
        };
        let rock = fall.rocks[0];
        assert_eq!(fall.rock_at(&rock, 0).centre[1], 400);
        assert!(!fall.rock_at(&rock, 6).falling);
        assert!(fall.rock_at(&rock, 10).falling);
        let landed = fall.landing_age(&rock) + 1;
        let pose = fall.rock_at(&rock, landed);
        assert!(!pose.falling && !pose.cleared);
        assert_eq!(pose.centre[1], 25);
        assert!(fall.rock_at(&rock, landed + 40).cleared);
    }

    #[test]
    fn validation_rejects_unsound_traps() {
        let spikes = Trap {
            id: TrapId::new(1),
            trigger: TrapTrigger::Live,
            effect: TrapEffect::Spikes(TriggerVolume {
                centre: [0, 0],
                half: [100, 100],
                floor: 0,
                height: 30,
            }),
            damage: 5,
            rehit_ticks: 20,
        };
        assert!(validate(std::slice::from_ref(&spikes)).is_ok());
        assert!(validate(&[spikes.clone(), spikes.clone()]).is_err());
        let mut bad = spikes;
        bad.damage = 0;
        assert!(validate(&[bad]).is_err());
    }
}
