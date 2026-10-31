//! Breakable props and their debris (#288).
//!
//! Crates, barrels and fences are creature-table units with the
//! [`crate::CreatureBehaviour::Prop`] behaviour: targetable, with a little health, damaged by every swing, ability, projectile and
//! blast that reaches them like any creature. They stand on fixed boxes, never move, fight or
//! engage, take no forced movement and give no experience or loot. A broken prop vanishes at
//! once and throws its pieces as short-lived dynamic Rapier bodies ([`DebrisBody`]) that fly,
//! settle and despawn after [`DEBRIS_TICKS`]. It stands again in place after its template's
//! respawn time (5 min in Greyhaven Vale), once no unit stands on its spot. An explosive
//! barrel explodes like Fire Blast as it breaks, hurting and knocking back nearby players,
//! creatures and other props, so barrels chain-react.

use crate::creature::Life;
use crate::entity::{EntityRef, body_id, is_unit_body};
use crate::physics::{Aabb, DebrisBody, Vec3i};
use crate::snapshot::CreatureAi;
use crate::{CreatureId, PlayerId, PropKind, ZoneError, ZoneEvent, ZoneSimulation};

/// A piece of debris lives this long (5 s), flying and then lying where it settled.
pub const DEBRIS_TICKS: u16 = crate::ticks_from_ms(5_000);
/// At most this many pieces exist in one zone; the oldest goes first.
pub const MAX_DEBRIS: usize = 192;
/// At most this many pieces reach one player projection, nearest first.
pub const MAX_VISIBLE_DEBRIS: usize = 16;
/// A piece may settle once it has flown this many ticks.
const SETTLE_AFTER_TICKS: u16 = crate::ticks_from_ms(267);
/// Launch speeds (units) and spins (radians) below are tuned per 1/30 s; this turns them into
/// per-tick values at [`crate::TICK_HZ`].
#[allow(clippy::cast_precision_loss)]
const PER_TUNED_TICK: f32 = 30.0 / crate::TICK_HZ as f32;
/// An explosive barrel's blast: Fire Blast's numbers at a level-3 caster (#287).
pub const EXPLOSION_DAMAGE: [u16; 2] = [12, 16];
pub const EXPLOSION_RADIUS_UNITS: i32 = 500;
pub const EXPLOSION_KNOCKBACK_UNITS: i32 = 600;

/// One piece of a prop: its centre and half extents in the prop's own frame (facing 0, units
/// from the prop's body centre) and its own turn about the vertical axis.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DebrisPiece {
    pub offset: [i32; 3],
    pub half_extents: [i32; 3],
    pub yaw: u16,
}

const fn piece(offset: [i32; 3], half_extents: [i32; 3], yaw: u16) -> DebrisPiece {
    DebrisPiece {
        offset,
        half_extents,
        yaw,
    }
}

/// A crate (35 cm half extents) breaks into its six panels.
const CRATE_PIECES: [DebrisPiece; 6] = [
    piece([0, 32, 0], [33, 3, 33], 0),
    piece([0, -32, 0], [33, 3, 33], 0),
    piece([32, 0, 0], [3, 29, 33], 0),
    piece([-32, 0, 0], [3, 29, 33], 0),
    piece([0, 0, 32], [29, 29, 3], 0),
    piece([0, 0, -32], [29, 29, 3], 0),
];
/// A barrel (30 cm radius, 45 cm half height) breaks into six staves and its two lids.
const BARREL_PIECES: [DebrisPiece; 8] = [
    piece([0, 0, 27], [9, 40, 3], 0),
    piece([23, 0, 13], [9, 40, 3], 10_923),
    piece([23, 0, -13], [9, 40, 3], 21_845),
    piece([0, 0, -27], [9, 40, 3], 32_768),
    piece([-23, 0, -13], [9, 40, 3], 43_691),
    piece([-23, 0, 13], [9, 40, 3], 54_613),
    piece([0, 42, 0], [22, 3, 22], 0),
    piece([0, -42, 0], [22, 3, 22], 0),
];
/// A fence section (2.2 m long, 1 m tall) breaks into two posts and three rails.
const FENCE_PIECES: [DebrisPiece; 5] = [
    piece([-100, 0, 0], [6, 50, 6], 0),
    piece([100, 0, 0], [6, 50, 6], 0),
    piece([0, 25, 0], [92, 5, 3], 0),
    piece([0, 0, 0], [92, 5, 3], 0),
    piece([0, -25, 0], [92, 5, 3], 0),
];

/// The pieces a prop breaks into, in piece-index order.
#[must_use]
pub const fn debris_pieces(kind: PropKind) -> &'static [DebrisPiece] {
    match kind {
        PropKind::Crate | PropKind::HeavyCrate => &CRATE_PIECES,
        PropKind::Barrel | PropKind::ExplosiveBarrel => &BARREL_PIECES,
        PropKind::Fence => &FENCE_PIECES,
    }
}

/// One flying or settled piece (canonical state).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Debris {
    pub id: u32,
    pub kind: PropKind,
    /// Index into [`debris_pieces`] of `kind`.
    pub piece: u8,
    pub body: DebrisBody,
    pub age: u16,
}

/// A piece as a player sees it: whole-unit centre and its rotation as a quaternion in
/// 1/32,767 steps.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DebrisView {
    pub id: u16,
    pub kind: PropKind,
    pub piece: u8,
    pub position: [i32; 3],
    pub rotation: [i16; 4],
    /// Ticks since the break, saturating.
    pub age: u8,
}

/// Radians of a yaw step value (0 faces +Z, turning toward +X).
fn radians(yaw: u16) -> f32 {
    f32::from(yaw) / 65_536.0 * std::f32::consts::TAU
}

/// `[x, z]` turned by `yaw` about the vertical axis.
fn turned(yaw: u16, [x, z]: [f32; 2]) -> [f32; 2] {
    let (sin, cos) = radians(yaw).sin_cos();
    [x * cos + z * sin, z * cos - x * sin]
}

/// The quaternion of a turn by `yaw` about the vertical axis.
fn yaw_rotation(yaw: u16) -> [f32; 4] {
    let (sin, cos) = (radians(yaw) / 2.0).sin_cos();
    [0.0, sin, 0.0, cos]
}

impl ZoneSimulation {
    /// Whether `unit` is a breakable prop.
    pub(crate) fn is_prop(&self, unit: EntityRef) -> bool {
        let EntityRef::Creature(id) = unit else {
            return false;
        };
        self.content
            .creature_spawn(id)
            .and_then(|spawn| self.content.creature_template(spawn.template))
            .is_some_and(|template| template.prop().is_some())
    }

    /// Whether a unit body overlaps a prop's spot, which keeps it from standing again.
    pub(crate) fn prop_spot_taken(&self, id: CreatureId) -> Result<bool, ZoneError> {
        let (spawn, template) = Self::creature_content(&self.content, id)?;
        let [hx, hy, hz] = template.body_half_extents(spawn.facing);
        let centre = Vec3i::new(
            spawn.position[0],
            self.ground_y(spawn.position[0], spawn.position[1]) + hy,
            spawn.position[1],
        );
        let hits = self
            .world
            .overlap_query(Aabb::new(centre, Vec3i::new(hx, hy, hz)))
            .map_err(crate::zone::physics_error)?;
        Ok(hits.into_iter().any(is_unit_body))
    }

    /// A prop's health reached zero: it vanishes, throws its pieces away from `killer` and, for
    /// an explosive barrel, explodes.
    pub(crate) fn prop_breaks(
        &mut self,
        id: CreatureId,
        kind: PropKind,
        killer: EntityRef,
        now: u64,
    ) -> Result<(), ZoneError> {
        let entity = EntityRef::Creature(id);
        let body = self
            .world
            .remove_body(body_id(entity))
            .ok_or_else(|| ZoneError::new("prop physics body is missing"))?;
        let centre = body.position();
        let facing = self.creatures.get(&id).map_or(0, |prop| prop.facing);
        if let Some(prop) = self.creatures.get_mut(&id) {
            prop.health = 0;
            prop.life = Life::Despawned { died_at: now };
            prop.ai = CreatureAi::Idle {
                timer: 0,
                destination: None,
            };
            prop.threat.clear();
            prop.swing_timer = 0;
            prop.combat_timer = 0;
            prop.tapped_by = None;
            prop.loot = None;
            prop.reset_abilities();
        }
        self.forced.remove(&entity);
        self.prop_motion.remove(&id);
        if self.interest.remove(entity) {
            self.interest_work.bucket_removes += 1;
        }
        for player in self.players.values_mut() {
            if player.target == Some(entity) {
                player.target = None;
            }
        }
        if let EntityRef::Player(player_id) = killer {
            self.notify(
                player_id,
                ZoneEvent::Died {
                    entity,
                    killer: Some(killer),
                },
            );
        }
        let from = self.unit_position(killer).ok();
        self.throw_debris(kind.debris(), centre, facing, from);
        if kind == PropKind::ExplosiveBarrel
            && let EntityRef::Player(player_id) = killer
        {
            self.explode_barrel(id, player_id, [centre.x, centre.y, centre.z], facing, now)?;
        }
        Ok(())
    }

    /// Spawns `kind`'s pieces around `centre`, flying outward and away from `from`.
    fn throw_debris(&mut self, kind: PropKind, centre: Vec3i, facing: u16, from: Option<[i32; 3]>) {
        let explosive = kind == PropKind::ExplosiveBarrel;
        let (outward, upward) = if explosive { (14.0, 12.0) } else { (4.0, 5.0) };
        #[allow(clippy::cast_precision_loss)]
        let away = from
            .map(|at| [(centre.x - at[0]) as f32, (centre.z - at[2]) as f32])
            .and_then(|[x, z]| {
                let length = x.hypot(z);
                (length > 0.0).then(|| [x / length * 3.0, z / length * 3.0])
            })
            .unwrap_or([0.0; 2]);
        for (index, piece) in debris_pieces(kind).iter().enumerate() {
            #[allow(clippy::cast_precision_loss)]
            let [ox, oy, oz] = piece.offset.map(|axis| axis as f32);
            let [x, z] = turned(facing, [ox, oz]);
            let length = x.hypot(z);
            let (dx, dz) = if length > 0.0 {
                (x / length, z / length)
            } else {
                // Lids and middle rails fly straight up.
                (0.0, 0.0)
            };
            #[allow(clippy::cast_precision_loss)]
            let i = index as f32;
            let lift = if oy > 0.0 { 1.4 } else { 1.0 };
            let body = DebrisBody {
                #[allow(clippy::cast_precision_loss)]
                position: [
                    centre.x as f32 + x,
                    centre.y as f32 + oy,
                    centre.z as f32 + z,
                ],
                rotation: yaw_rotation(facing.wrapping_add(piece.yaw)),
                velocity: [
                    dx * outward + away[0],
                    upward * lift + (i % 3.0),
                    dz * outward + away[1],
                ]
                .map(|axis| axis * PER_TUNED_TICK),
                spin: [
                    0.12 * ((i % 3.0) - 1.0) + 0.05,
                    0.08 * ((i % 2.0) * 2.0 - 1.0),
                    0.12 * (((i + 1.0) % 3.0) - 1.0),
                ]
                .map(|axis| axis * PER_TUNED_TICK),
                half_extents: Vec3i::new(
                    piece.half_extents[0],
                    piece.half_extents[1],
                    piece.half_extents[2],
                ),
                resting: false,
            };
            let id = self.next_debris;
            self.next_debris = self.next_debris.wrapping_add(1);
            if self.debris.len() >= MAX_DEBRIS
                && let Some((&oldest, _)) = self.debris.iter().next()
            {
                self.debris.remove(&oldest);
            }
            self.debris.insert(
                id,
                Debris {
                    id,
                    kind,
                    piece: u8::try_from(index).unwrap_or(u8::MAX),
                    body,
                    age: 0,
                },
            );
        }
    }

    /// The barrel's blast, credited to the player who broke it: Fire Blast's falloff damage and
    /// knockback to every player, creature and prop within reach that the centre can see.
    fn explode_barrel(
        &mut self,
        barrel: CreatureId,
        breaker: PlayerId,
        centre: [i32; 3],
        facing: u16,
        now: u64,
    ) -> Result<(), ZoneError> {
        let (x, z) = crate::trig::direction(facing);
        self.detonate(
            crate::casting::Detonation {
                attacker: breaker,
                source: Some(barrel),
                centre,
                radius: EXPLOSION_RADIUS_UNITS,
                damage: EXPLOSION_DAMAGE,
                bonus: 0,
                knockback: EXPLOSION_KNOCKBACK_UNITS,
                fallback: [i64::from(x), i64::from(z)],
            },
            now,
        )
    }

    /// After the physics step: pieces age, fly under Rapier and settle; old ones vanish.
    pub(crate) fn advance_debris(&mut self) {
        self.debris.retain(|_, piece| {
            piece.age += 1;
            piece.age < DEBRIS_TICKS
        });
        if self.debris.is_empty() {
            return;
        }
        let mut bodies: Vec<_> = self.debris.values().map(|piece| piece.body).collect();
        let settle: Vec<_> = self
            .debris
            .values()
            .map(|piece| piece.age >= SETTLE_AFTER_TICKS)
            .collect();
        self.world.step_debris(&mut bodies, &settle);
        for (piece, body) in self.debris.values_mut().zip(bodies) {
            piece.body = body;
        }
    }

    /// The pieces a viewer at `centre` sees, nearest first.
    pub(crate) fn debris_views(&self, centre: [i32; 3]) -> Vec<DebrisView> {
        let radius = f64::from(crate::INTEREST_RADIUS_UNITS);
        let mut near: Vec<_> = self
            .debris
            .values()
            .filter_map(|piece| {
                let [x, y, z] = crate::physics::to_units(piece.body.position);
                let distance = f64::from(x - centre[0]).hypot(f64::from(z - centre[2]));
                #[allow(clippy::cast_possible_truncation)]
                let rotation = piece
                    .body
                    .rotation
                    .map(|axis| (axis.clamp(-1.0, 1.0) * 32_767.0).round() as i16);
                (distance <= radius).then_some((
                    distance,
                    DebrisView {
                        #[allow(clippy::cast_possible_truncation)]
                        id: piece.id as u16,
                        kind: piece.kind,
                        piece: piece.piece,
                        position: [x, y, z],
                        rotation,
                        age: u8::try_from(piece.age).unwrap_or(u8::MAX),
                    },
                ))
            })
            .collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.id.cmp(&b.1.id)));
        near.into_iter()
            .take(MAX_VISIBLE_DEBRIS)
            .map(|(_, view)| view)
            .collect()
    }

    /// Canonical debris restore: ordered IDs, known pieces with their table extents, valid
    /// bodies and ages.
    pub(crate) fn restore_debris(
        &mut self,
        debris: Vec<Debris>,
        next: u32,
    ) -> Result<(), ZoneError> {
        let valid = debris.len() <= MAX_DEBRIS
            && debris.windows(2).all(|pair| pair[0].id < pair[1].id)
            && debris.iter().all(|piece| {
                debris_pieces(piece.kind)
                    .get(usize::from(piece.piece))
                    .is_some_and(|table| {
                        let [x, y, z] = table.half_extents;
                        piece.body.half_extents == Vec3i::new(x, y, z)
                    })
                    && piece.body.is_valid()
                    && piece.age < DEBRIS_TICKS
                    && (!piece.body.resting
                        || (piece.body.velocity == [0.0; 3] && piece.body.spin == [0.0; 3]))
            });
        if !valid {
            return Err(ZoneError::new("snapshot debris is invalid"));
        }
        self.debris = debris.into_iter().map(|piece| (piece.id, piece)).collect();
        self.next_debris = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use strum::VariantArray;

    #[test]
    fn pieces_turn_with_the_prop() {
        let [x, z] = turned(16_384, [100.0, 0.0]);
        assert!(x.abs() < 1.0e-3 && (z + 100.0).abs() < 1.0e-3, "{x} {z}");
        let [x, z] = turned(16_384, [0.0, 100.0]);
        assert!((x - 100.0).abs() < 1.0e-3 && z.abs() < 1.0e-3, "{x} {z}");
        for &kind in PropKind::VARIANTS {
            assert!(!debris_pieces(kind).is_empty());
        }
    }
}
