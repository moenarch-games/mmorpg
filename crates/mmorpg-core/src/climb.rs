//! Climbing climbable surfaces and free ledge mantling (#490, Atlas 5.1.40).
//!
//! **Surfaces.** Zone content marks which collision is climbable ([`ClimbSurfaces`]): static
//! colliders by ID and stretches of the terrain by XZ rectangle, each with its [`Grip`] (rough
//! rock, timber, or the rungs of a ladder, rope or vine). Everything else, buildings and towers
//! included, is not. A player who jumps at a climbable face that continues above their hands,
//! or who reaches one in the air while pressing forward, holds on and climbs: forward and back
//! move up and down along the face, strafing moves along it, jumping lets go. There is no
//! stamina and no height limit; climbing is just slow. Off the marked surface (round a corner
//! onto a sheer stretch, say) the climber cannot hold on and drops.
//!
//! **Mantling** needs no marking: any ledge the physics finds reachable will do. Jumping at a
//! ledge whose top is within [`MANTLE_REACH_UNITS`] of the feet (or reaching one in the air
//! while pressing forward), or climbing to the top of a face, pulls the player up onto it. The
//! zone requires a steep face in front, a walkable top below the reach, nothing solid above
//! the reach between the body and the top (a taller wall), and a free body path: straight up
//! to the top and across onto it. Rapier's character controller then moves the body along
//! that path like any other move, so contact is resolved as always and nothing teleports.
//!
//! The zone decides every transition on the positions the last physics step left
//! ([`decide`]); the state is canonical ([`Traversal`]), so a restored zone continues a climb
//! or a mantle exactly. A climbing or mantling body is weightless: the zone gives it its
//! vertical velocity, as for swimmers.

use crate::physics::{BodyId, Vec3i, World};
use crate::{PLAYER_HALF_EXTENTS_UNITS, XzBounds, ZoneError, trig};

/// Climbing up or down moves the body this fast along the face (0.9 m/s).
pub const CLIMB_SPEED_UNITS_PER_SECOND: i32 = 90;
/// Climbing sideways moves the body this fast along the face (0.75 m/s).
pub const CLIMB_SIDE_SPEED_UNITS_PER_SECOND: i32 = 75;
/// A climber presses into the face this fast, so contact holds it there.
const CLIMB_PRESS_UNITS_PER_SECOND: i32 = 60;
/// A face this far beyond the body's surface is within reach (0.6 m).
pub const CLIMB_REACH_UNITS: i32 = 60;
/// A ledge whose top is at most this far above the feet can be grabbed (2.2 m): standing, or at
/// the top of a jump (which adds about 1.2 m).
pub const MANTLE_REACH_UNITS: i32 = 220;
/// A ledge must stand at least this far above the feet to be mantled.
pub const MANTLE_MIN_UNITS: i32 = 20;
/// A mantle pulls the body up this fast (3 m/s) …
pub const MANTLE_RISE_UNITS_PER_SECOND: i32 = 300;
/// … then across onto the ledge this fast (2.1 m/s).
pub const MANTLE_FORWARD_UNITS_PER_SECOND: i32 = 210;
/// A mantle that has not landed after this long (2 s) is abandoned: the player drops.
pub const MANTLE_MAX_TICKS: u16 = crate::ticks_from_ms(2_000);
/// After letting go, a player cannot grab a face again for this long (0.5 s), unless they land.
pub const RELEASE_TICKS: u16 = crate::ticks_from_ms(500);
/// At most this many climbable colliders …
pub const MAX_CLIMBABLE_COLLIDERS: usize = crate::MAX_STATIC_COLLIDERS;
/// … and terrain stretches per zone.
pub const MAX_CLIMBABLE_STRETCHES: usize = 64;

/// The landing column stands this far inside the face: the body's radius and 10 cm more.
const LEDGE_DEPTH_UNITS: f32 = 40.0;
/// A mantle lifts the body this far above the ledge before moving across onto it.
const LEDGE_CLEARANCE_UNITS: f32 = 5.0;
/// Faces are probed at these heights above the feet: low, the body's middle and the hands.
const PROBE_HEIGHTS: [f32; 3] = [25.0, 90.0, 170.0];
/// The body probe's height above the feet.
const BODY_HEIGHT: f32 = 90.0;
/// The hand probe looks for more face this far up along the face from the body probe.
const HAND_RISE_UNITS: f32 = 80.0;
/// A surface whose normal's upward part is below this is steeper than walkable (45°).
const WALKABLE_NORMAL_Y: f32 = std::f32::consts::FRAC_1_SQRT_2;
/// The contact offset the backends keep (see `physics.rs`).
const CONTACT_OFFSET: f32 = 0.25;

/// What a climbable surface is made of; each climbs the same for now.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Grip {
    /// Rough rock: crags and climbable cliff faces.
    Rock,
    /// Timber: palisades and fences.
    Timber,
    /// The rungs of a ladder, rope or vine.
    Rungs,
}

impl Grip {
    /// A stable code for the content revision.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Rock => 1,
            Self::Timber => 2,
            Self::Rungs => 3,
        }
    }
}

/// Which collision is climbable (#490): static colliders by ID, and stretches of the terrain
/// (or flat ground) by XZ rectangle, the first stretch containing a point deciding it; `None`
/// marks a sheer stretch. Everything else is not climbable.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ClimbSurfaces {
    colliders: Vec<(u32, Grip)>,
    terrain: Vec<(XzBounds, Option<Grip>)>,
}

impl ClimbSurfaces {
    /// Climbable `colliders` (any order, each ID once) and terrain `stretches` in priority order.
    pub fn new(
        mut colliders: Vec<(u32, Grip)>,
        terrain: Vec<(XzBounds, Option<Grip>)>,
    ) -> Result<Self, ZoneError> {
        colliders.sort_by_key(|&(id, _)| id);
        if colliders.len() > MAX_CLIMBABLE_COLLIDERS || terrain.len() > MAX_CLIMBABLE_STRETCHES {
            return Err(ZoneError::new("zone climbable surface capacity reached"));
        }
        if colliders.windows(2).any(|pair| pair[0].0 == pair[1].0) {
            return Err(ZoneError::new("duplicate climbable collider"));
        }
        if terrain
            .iter()
            .any(|(bounds, _)| bounds.min[0] >= bounds.max[0] || bounds.min[1] >= bounds.max[1])
        {
            return Err(ZoneError::new("empty climbable terrain stretch"));
        }
        Ok(Self { colliders, terrain })
    }

    /// Climbable colliders, ordered by ID.
    #[must_use]
    pub fn colliders(&self) -> &[(u32, Grip)] {
        &self.colliders
    }

    /// Terrain stretches in priority order.
    #[must_use]
    pub fn terrain(&self) -> &[(XzBounds, Option<Grip>)] {
        &self.terrain
    }

    /// The grip of collider `id`, if it is climbable.
    #[must_use]
    pub fn collider(&self, id: u32) -> Option<Grip> {
        self.colliders
            .binary_search_by_key(&id, |&(id, _)| id)
            .ok()
            .map(|index| self.colliders[index].1)
    }

    /// The grip of the terrain at `(x, z)`, if it is climbable there.
    #[must_use]
    pub fn terrain_at(&self, x: i32, z: i32) -> Option<Grip> {
        self.terrain
            .iter()
            .find(|(bounds, _)| bounds.contains(x, z))
            .and_then(|&(_, grip)| grip)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.colliders.is_empty() && self.terrain.is_empty()
    }
}

/// A player's climb or mantle, canonical so a restored zone continues it exactly.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Traversal {
    /// Walking, falling or swimming as usual.
    #[default]
    Free,
    /// Let go of a face: moving freely, but no grab for `ticks` more ticks unless grounded.
    Released { ticks: u16 },
    /// Holding a climbable face; `wall` is the yaw facing it.
    Climbing { wall: u16 },
    /// Pulling up onto a ledge: the feet's goal on it, the yaw facing the ledge, the ticks the
    /// mantle has run and whether it has risen and moves across onto the ledge.
    Mantling {
        goal: [i32; 3],
        wall: u16,
        ticks: u16,
        across: bool,
    },
}

impl Traversal {
    /// Climbing or mantling: weightless, and the projection shows it.
    #[must_use]
    pub const fn holds(self) -> bool {
        matches!(self, Self::Climbing { .. } | Self::Mantling { .. })
    }

    /// The yaw a climbing or mantling player faces: toward the face.
    #[must_use]
    pub const fn wall(self) -> Option<u16> {
        match self {
            Self::Climbing { wall } | Self::Mantling { wall, .. } => Some(wall),
            Self::Free | Self::Released { .. } => None,
        }
    }

    /// Whether every field is within range.
    #[must_use]
    pub fn is_valid(self) -> bool {
        match self {
            Self::Free | Self::Climbing { .. } => true,
            Self::Released { ticks } => ticks <= RELEASE_TICKS,
            Self::Mantling { goal, ticks, .. } => {
                ticks <= MANTLE_MAX_TICKS
                    && goal
                        .iter()
                        .all(|&axis| crate::content::within_content_range(axis))
            }
        }
    }
}

/// What a player shows other players: the projection's traversal code.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TraversalView {
    #[default]
    Free,
    Climbing,
    Mantling,
}

impl TraversalView {
    #[must_use]
    pub const fn of(traversal: Traversal) -> Self {
        match traversal {
            Traversal::Free | Traversal::Released { .. } => Self::Free,
            Traversal::Climbing { .. } => Self::Climbing,
            Traversal::Mantling { .. } => Self::Mantling,
        }
    }

    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Free => 0,
            Self::Climbing => 1,
            Self::Mantling => 2,
        }
    }

    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Free),
            1 => Some(Self::Climbing),
            2 => Some(Self::Mantling),
            _ => None,
        }
    }
}

/// What the player asks for this tick.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Intent {
    pub(crate) forward: i32,
    pub(crate) strafe: i32,
    pub(crate) jump: bool,
    pub(crate) facing: u16,
    pub(crate) grounded: bool,
    /// Stunned: a climber hangs still, nobody grabs.
    pub(crate) stunned: bool,
}

/// The decision for one tick: the new state and, while it holds, the body's velocity
/// (subunits per tick); `None` moves the player as usual.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Step {
    pub(crate) traversal: Traversal,
    pub(crate) velocity: Option<Vec3i>,
}

impl Step {
    /// No climb or mantle: dead, swimming.
    pub(crate) const FREE: Self = Self::free(Traversal::Free);

    const fn free(traversal: Traversal) -> Self {
        Self {
            traversal,
            velocity: None,
        }
    }
}

/// The world the probes look at: the physics, the climbable surfaces and the ground's body
/// (the terrain shares its ID).
pub(crate) struct Probe<'a> {
    pub(crate) world: &'a World,
    pub(crate) surfaces: &'a ClimbSurfaces,
    pub(crate) ground: Option<BodyId>,
    /// The player's own body, which never obstructs it.
    pub(crate) body: BodyId,
}

/// A steep face a probe ray met.
#[derive(Clone, Copy, Debug)]
struct Face {
    body: BodyId,
    point: [f32; 3],
    /// The unit surface normal.
    normal: [f32; 3],
    /// The normal's horizontal part, a unit vector `(x, z)`.
    outward: [f32; 2],
}

/// Static content and terrain; never units or world limits' insides.
fn statics(body: BodyId) -> bool {
    body.0 < crate::entity::PLAYER_BODY_BASE
}

fn radius() -> f32 {
    #[allow(clippy::cast_precision_loss)]
    let radius = PLAYER_HALF_EXTENTS_UNITS[0].max(PLAYER_HALF_EXTENTS_UNITS[2]) as f32;
    radius
}

fn half_height() -> f32 {
    #[allow(clippy::cast_precision_loss)]
    let half = PLAYER_HALF_EXTENTS_UNITS[1] as f32;
    half
}

/// The unit XZ vector a yaw faces.
fn heading(yaw: u16) -> [f32; 2] {
    let (x, z) = trig::direction(yaw);
    #[allow(clippy::cast_precision_loss)]
    let one = trig::TRIG_ONE as f32;
    #[allow(clippy::cast_precision_loss)]
    [x as f32 / one, z as f32 / one]
}

/// The yaw facing into a face with outward normal `outward`.
fn yaw_into(outward: [f32; 2]) -> Option<u16> {
    #[allow(clippy::cast_possible_truncation)]
    let scaled = |value: f32| (-value * 65_536.0).round() as i32;
    trig::yaw_from_vector(scaled(outward[0]), scaled(outward[1]))
}

/// A velocity in units per tick as subunits per tick.
fn subunits(units_per_tick: [f32; 3]) -> Vec3i {
    #[allow(clippy::cast_precision_loss)]
    let scale = crate::physics::SUBUNITS_PER_UNIT as f32;
    #[allow(clippy::cast_possible_truncation)]
    let axis = |value: f32| (value * scale).round() as i32;
    Vec3i::new(
        axis(units_per_tick[0]),
        axis(units_per_tick[1]),
        axis(units_per_tick[2]),
    )
}

/// Units per second as units per tick.
fn per_tick(units_per_second: i32) -> f32 {
    #[allow(clippy::cast_precision_loss)]
    let units = units_per_second as f32;
    units / f32::from(crate::TICK_HZ)
}

/// `units_per_second` along `direction` (a unit or shorter vector), in subunits per tick.
fn velocity(direction: [f32; 3], units_per_second: i32) -> Vec3i {
    let speed = per_tick(units_per_second);
    subunits(direction.map(|axis| axis * speed))
}

/// The reach beyond the body's surface, in units.
fn reach() -> f32 {
    #[allow(clippy::cast_precision_loss)]
    let reach = CLIMB_REACH_UNITS as f32;
    reach
}

impl Probe<'_> {
    /// The grip of the surface a ray met at `point` on `body`.
    fn grip(&self, body: BodyId, point: [f32; 3]) -> Option<Grip> {
        #[allow(clippy::cast_possible_truncation)]
        let at = |value: f32| value.round() as i32;
        if Some(body) == self.ground {
            return self.surfaces.terrain_at(at(point[0]), at(point[2]));
        }
        u32::try_from(body.0)
            .ok()
            .and_then(|id| self.surfaces.collider(id))
    }

    /// The steep static face met by a ray from `origin` along `direction` (XZ unit vector) for
    /// `length` units.
    fn face(&self, origin: [f32; 3], direction: [f32; 3], length: f32) -> Option<Face> {
        let translation = direction.map(|axis| axis * length);
        let hit = self.world.cast_ray(origin, translation, statics)?;
        let normal = hit.normal;
        let horizontal = normal[0].hypot(normal[2]);
        // A ray starting inside geometry, a walkable surface, an overhang's underside or a
        // face turned away is no face to hold or mantle.
        if hit.fraction <= 0.0
            || normal[1].abs() >= WALKABLE_NORMAL_Y
            || horizontal <= 0.0
            || normal[0] * direction[0] + normal[2] * direction[2] >= 0.0
        {
            return None;
        }
        Some(Face {
            body: hit.body,
            point: [0, 1, 2].map(|axis| origin[axis] + translation[axis] * hit.fraction),
            normal,
            outward: [normal[0] / horizontal, normal[2] / horizontal],
        })
    }

    /// The face in front of a body centred on `centre`, facing `ahead`, at `height` above
    /// its feet.
    fn face_ahead(&self, centre: [f32; 3], ahead: [f32; 2], height: f32) -> Option<Face> {
        let feet = centre[1] - half_height();
        self.face(
            [centre[0], feet + height, centre[2]],
            [ahead[0], 0.0, ahead[1]],
            radius() + CONTACT_OFFSET + reach(),
        )
    }

    /// More face above `face`: a ray back into it from [`HAND_RISE_UNITS`] up along it.
    fn face_above(&self, face: &Face) -> Option<Face> {
        let up = up_along(face.normal);
        let back = radius() + 10.0;
        let origin = [0, 1, 2].map(|axis| {
            let outward = [face.outward[0], 0.0, face.outward[1]][axis];
            face.point[axis] + outward * back + up[axis] * HAND_RISE_UNITS
        });
        self.face(
            origin,
            [-face.outward[0], 0.0, -face.outward[1]],
            back + reach(),
        )
    }

    /// A climbable face in front of the body with more of it above: one to grab.
    fn grabbable(&self, centre: [f32; 3], ahead: [f32; 2]) -> Option<Face> {
        let face = self.face_ahead(centre, ahead, BODY_HEIGHT)?;
        self.grip(face.body, face.point)?;
        let above = self.face_above(&face)?;
        self.grip(above.body, above.point)?;
        Some(face)
    }

    /// A reachable ledge in front of a body centred on `centre`, facing `ahead`: the feet's
    /// goal on it and the yaw facing it. See the module documentation for what is checked.
    fn ledge(&self, centre: [f32; 3], ahead: [f32; 2]) -> Option<([i32; 3], u16)> {
        let feet = centre[1] - half_height();
        let face = PROBE_HEIGHTS
            .iter()
            .filter_map(|&height| self.face_ahead(centre, ahead, height))
            .min_by(|a, b| {
                let distance =
                    |face: &Face| (face.point[0] - centre[0]).hypot(face.point[2] - centre[2]);
                distance(a).total_cmp(&distance(b))
            })?;
        let column = [
            face.point[0] - face.outward[0] * LEDGE_DEPTH_UNITS,
            face.point[2] - face.outward[1] * LEDGE_DEPTH_UNITS,
        ];
        #[allow(clippy::cast_precision_loss)]
        let (reach, low) = (MANTLE_REACH_UNITS as f32, MANTLE_MIN_UNITS as f32);
        let top = feet + reach + 10.0;
        // Nothing solid at the top of the reach between the body and the ledge: a taller
        // wall's ledge is out of reach.
        let across = [column[0] - centre[0], 0.0, column[1] - centre[2]];
        if self
            .world
            .cast_ray([centre[0], top, centre[2]], across, statics)
            .is_some()
        {
            return None;
        }
        // A walkable top within reach.
        let drop = top - (feet + low);
        let hit = self
            .world
            .cast_ray([column[0], top, column[1]], [0.0, -drop, 0.0], statics)?;
        if hit.fraction <= 0.0 || hit.normal[1] < WALKABLE_NORMAL_Y {
            return None;
        }
        let ledge = top - drop * hit.fraction;
        // A free body path: straight up above the ledge, then across onto it.
        let raised = ledge + half_height() + LEDGE_CLEARANCE_UNITS;
        let half = Vec3i::new(
            PLAYER_HALF_EXTENTS_UNITS[0],
            PLAYER_HALF_EXTENTS_UNITS[1],
            PLAYER_HALF_EXTENTS_UNITS[2],
        );
        let others = |body: BodyId| body != self.body;
        // The sweep starts a unit up, off the ground the body rests on.
        let start = [centre[0], centre[1] + 1.0, centre[2]];
        if raised > start[1]
            && self
                .world
                .cast_shape(start, half, [0.0, raised - start[1], 0.0], others)
                .is_some()
        {
            return None;
        }
        let lifted = [centre[0], raised.max(centre[1]), centre[2]];
        if self
            .world
            .cast_shape(lifted, half, across, others)
            .is_some()
        {
            return None;
        }
        #[allow(clippy::cast_possible_truncation)]
        let at = |value: f32| value.round() as i32;
        Some((
            [at(column[0]), at(ledge), at(column[1])],
            yaw_into(face.outward)?,
        ))
    }
}

/// The unit vector up along a face with unit normal `normal`.
fn up_along(normal: [f32; 3]) -> [f32; 3] {
    let up = [
        -normal[1] * normal[0],
        1.0 - normal[1] * normal[1],
        -normal[1] * normal[2],
    ];
    let length = (up[0] * up[0] + up[1] * up[1] + up[2] * up[2]).sqrt();
    if length <= 0.0 {
        return [0.0, 1.0, 0.0];
    }
    up.map(|axis| axis / length)
}

/// The tick's climb or mantle decision for a living player whose body is centred on `centre`
/// (game units) after the last physics step.
pub(crate) fn decide(
    probe: &Probe<'_>,
    centre: [f32; 3],
    current: Traversal,
    intent: Intent,
) -> Step {
    match current {
        Traversal::Mantling {
            goal,
            wall,
            ticks,
            across,
        } => mantle(centre, goal, wall, ticks, across, intent),
        Traversal::Climbing { wall } => climb(probe, centre, wall, intent),
        Traversal::Released { ticks } if ticks > 0 && !intent.grounded => {
            Step::free(Traversal::Released { ticks: ticks - 1 })
        }
        Traversal::Free | Traversal::Released { .. } => grab(probe, centre, intent),
    }
}

/// Free movement: a jump from the ground, or pressing forward in the air, at a ledge mantles
/// and at a climbable face climbs.
fn grab(probe: &Probe<'_>, centre: [f32; 3], intent: Intent) -> Step {
    let reaching = (intent.jump && intent.grounded) || (!intent.grounded && intent.forward > 0);
    if !reaching || intent.stunned {
        return Step::free(Traversal::Free);
    }
    let ahead = heading(intent.facing);
    if let Some((goal, wall)) = probe.ledge(centre, ahead) {
        return start_mantle(centre, goal, wall);
    }
    if let Some(face) = probe.grabbable(centre, ahead)
        && let Some(wall) = yaw_into(face.outward)
    {
        return Step {
            traversal: Traversal::Climbing { wall },
            velocity: Some(climb_velocity(&face, wall, 0, 0)),
        };
    }
    Step::free(Traversal::Free)
}

fn start_mantle(centre: [f32; 3], goal: [i32; 3], wall: u16) -> Step {
    let traversal = Traversal::Mantling {
        goal,
        wall,
        ticks: 0,
        across: false,
    };
    Step {
        traversal,
        velocity: Some(mantle_velocity(centre, goal, false)),
    }
}

/// Holding a face: move along it, top out onto a ledge, step off at the bottom or let go.
fn climb(probe: &Probe<'_>, centre: [f32; 3], wall: u16, intent: Intent) -> Step {
    if intent.jump {
        return Step::free(Traversal::Released {
            ticks: RELEASE_TICKS,
        });
    }
    let ahead = heading(wall);
    let Some(face) = probe
        .face_ahead(centre, ahead, BODY_HEIGHT)
        .filter(|face| probe.grip(face.body, face.point).is_some())
    else {
        // The face ended or turned sheer: top out onto it if possible, otherwise drop.
        if intent.forward > 0
            && !intent.stunned
            && let Some((goal, wall)) = probe.ledge(centre, ahead)
        {
            return start_mantle(centre, goal, wall);
        }
        return Step::free(Traversal::Released {
            ticks: RELEASE_TICKS,
        });
    };
    let wall = yaw_into(face.outward).unwrap_or(wall);
    if intent.stunned {
        return Step {
            traversal: Traversal::Climbing { wall },
            velocity: Some(climb_velocity(&face, wall, 0, 0)),
        };
    }
    if intent.forward < 0 && intent.grounded {
        return Step::free(Traversal::Free);
    }
    let mut forward = intent.forward;
    if forward > 0 {
        match probe.face_above(&face) {
            // Sheer above: no higher.
            Some(above) if probe.grip(above.body, above.point).is_none() => forward = 0,
            Some(_) => {}
            // Near the top: pull up onto it once the ledge is in reach.
            None => {
                if let Some((goal, wall)) = probe.ledge(centre, ahead) {
                    return start_mantle(centre, goal, wall);
                }
            }
        }
    }
    Step {
        traversal: Traversal::Climbing { wall },
        velocity: Some(climb_velocity(&face, wall, forward, intent.strafe)),
    }
}

/// The climber's velocity: up or down and sideways along the face, pressing into it.
fn climb_velocity(face: &Face, wall: u16, forward: i32, strafe: i32) -> Vec3i {
    let up = up_along(face.normal);
    // Strafing right turns three quarters from the facing, as on the ground.
    let side = heading(wall.wrapping_add(3 * trig::YAW_QUARTER_TURN));
    #[allow(clippy::cast_precision_loss)]
    let (forward, strafe) = (forward.signum() as f32, strafe.signum() as f32);
    let along = velocity(up.map(|axis| axis * forward), CLIMB_SPEED_UNITS_PER_SECOND);
    let sideways = velocity(
        [side[0] * strafe, 0.0, side[1] * strafe],
        CLIMB_SIDE_SPEED_UNITS_PER_SECOND,
    );
    let press = velocity(
        [-face.outward[0], 0.0, -face.outward[1]],
        CLIMB_PRESS_UNITS_PER_SECOND,
    );
    Vec3i::new(
        along.x + sideways.x + press.x,
        along.y,
        along.z + sideways.z + press.z,
    )
}

/// Pulling up: rise until the feet clear the ledge, then move across onto the goal.
fn mantle(
    centre: [f32; 3],
    goal: [i32; 3],
    wall: u16,
    ticks: u16,
    across: bool,
    intent: Intent,
) -> Step {
    if ticks >= MANTLE_MAX_TICKS {
        return Step::free(Traversal::Released {
            ticks: RELEASE_TICKS,
        });
    }
    let [x, y, z] = crate::physics::from_units(goal);
    let raised = y + half_height() + LEDGE_CLEARANCE_UNITS;
    // The rise ends once the feet clear the ledge by its clearance.
    let across = across || centre[1] >= raised - 0.5;
    if across && (x - centre[0]).hypot(z - centre[2]) < 1.0 {
        return Step::free(Traversal::Free);
    }
    let traversal = Traversal::Mantling {
        goal,
        wall,
        ticks: ticks + 1,
        across,
    };
    if intent.stunned {
        return Step {
            traversal,
            velocity: Some(Vec3i::ZERO),
        };
    }
    Step {
        traversal,
        velocity: Some(mantle_velocity(centre, goal, across)),
    }
}

fn mantle_velocity(centre: [f32; 3], goal: [i32; 3], across: bool) -> Vec3i {
    let [x, y, z] = crate::physics::from_units(goal);
    if !across {
        let raised = y + half_height() + LEDGE_CLEARANCE_UNITS;
        let rise = (raised - centre[1]).min(per_tick(MANTLE_RISE_UNITS_PER_SECOND));
        return subunits([0.0, rise.max(0.0), 0.0]);
    }
    let (dx, dz) = (x - centre[0], z - centre[2]);
    let across = dx.hypot(dz);
    if across <= 0.0 {
        return Vec3i::ZERO;
    }
    let step = across.min(per_tick(MANTLE_FORWARD_UNITS_PER_SECOND));
    subunits([dx / across * step, 0.0, dz / across * step])
}
