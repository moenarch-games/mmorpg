//! Cones, beams and chains (#292): Frost Breath, Lightning Beam and Chain Lightning.
//!
//! Each picks its victims from the zone's spatial index in `EntityRef` order and asks the static
//! world whether the caster (or, for a chain, the last victim) can see them: walls shield the
//! units behind them. A cone takes every visible hostile unit within its range and angle of the
//! caster's facing. A beam follows the caster's current facing every tick, pitched toward the
//! unit it aims at (#884); a physics ray finds the first wall, a raised shield turned toward the caster stops it at that player, and every
//! hostile unit closer along the line is struck. A chain jumps to the nearest unhit hostile unit
//! the last victim can see.
//!
//! Rapier gap noted for physics-engine parity: the beam is a ray plus a lateral distance test
//! against unit centres, not a swept volume query.

use crate::ability::scaled;
use crate::ai::distance_squared;
use crate::entity::PLAYER_BODY_BASE;
use crate::{EntityRef, PlayerId, ZoneError, ZoneEvent, ZoneSimulation};

/// A beam strikes units whose centre lies within this distance of its line, units.
const BEAM_HALF_WIDTH_UNITS: f32 = 70.0;

/// The unit vector of `facing` in XZ.
#[allow(clippy::cast_precision_loss)]
fn ahead(facing: u16) -> [f32; 2] {
    let (x, z) = crate::trig::direction(facing);
    let one = crate::trig::TRIG_ONE as f32;
    [x as f32 / one, z as f32 / one]
}

/// What one beam evaluation found: where it ends and whom it strikes, nearest first.
pub(crate) struct BeamTrace {
    pub end: [i32; 2],
    pub victims: Vec<EntityRef>,
    /// The player whose raised shield stopped it, if any.
    pub blocked_by: Option<PlayerId>,
}

impl ZoneSimulation {
    /// Whether `unit` is a living hostile of `attacker`: a creature or prop, or an opposing
    /// player under the PvP rules.
    pub(crate) fn hostile_to(&self, attacker: PlayerId, unit: EntityRef) -> bool {
        match unit {
            EntityRef::Player(id) => {
                id != attacker
                    && self.players_are_hostile(attacker, id)
                    && self
                        .players
                        .get(&id)
                        .is_some_and(|player| player.is_alive())
            }
            EntityRef::Creature(id) => self
                .creatures
                .get(&id)
                .is_some_and(|creature| creature.is_alive()),
            EntityRef::Npc(_) | EntityRef::Minion(_) => false,
        }
    }

    /// Frost Breath's victims: hostile units the caster can see within `range` and
    /// `half_angle` degrees of its facing, in `EntityRef` order.
    pub(crate) fn cone_victims(
        &self,
        player_id: PlayerId,
        range: i32,
        half_angle: u16,
    ) -> Result<Vec<EntityRef>, ZoneError> {
        let facing = self
            .players
            .get(&player_id)
            .ok_or_else(|| ZoneError::new("unknown player"))?
            .facing;
        self.units_in_cone(
            EntityRef::Player(player_id),
            facing,
            range,
            half_angle,
            |unit| self.hostile_to(player_id, unit),
        )
    }

    /// A creature's cone (the Redbrand Brute's Overhead Smash, #160): living players it can see
    /// within `range` and `half_angle` degrees of its facing, in `EntityRef` order.
    pub(crate) fn creature_cone_victims(
        &self,
        creature_id: crate::CreatureId,
        range: i32,
        half_angle: u16,
    ) -> Result<Vec<EntityRef>, ZoneError> {
        let facing = self
            .creatures
            .get(&creature_id)
            .ok_or_else(|| ZoneError::new("unknown creature"))?
            .facing;
        self.units_in_cone(
            EntityRef::Creature(creature_id),
            facing,
            range,
            half_angle,
            |unit| match unit {
                EntityRef::Player(id) => self
                    .players
                    .get(&id)
                    .is_some_and(|player| player.is_alive()),
                _ => false,
            },
        )
    }

    /// Units passing `wanted` that `caster` can see within `range` and `half_angle` degrees of
    /// `facing`, in `EntityRef` order.
    fn units_in_cone(
        &self,
        caster: EntityRef,
        facing: u16,
        range: i32,
        half_angle: u16,
        wanted: impl Fn(EntityRef) -> bool,
    ) -> Result<Vec<EntityRef>, ZoneError> {
        let forward = ahead(facing);
        let min_cos = libm::cosf(f32::from(half_angle).to_radians());
        let centre = self.unit_position(caster)?;
        let mut found = Vec::new();
        for unit in self.interest.near(centre[0], centre[2], range) {
            if !wanted(unit) {
                continue;
            }
            let position = self.unit_position(unit)?;
            if distance_squared([centre[0], centre[2]], [position[0], position[2]])
                > i64::from(range).pow(2)
            {
                continue;
            }
            #[allow(clippy::cast_precision_loss)]
            let delta = [
                (position[0] - centre[0]) as f32,
                (position[2] - centre[2]) as f32,
            ];
            let distance = libm::hypotf(delta[0], delta[1]);
            let along = delta[0] * forward[0] + delta[1] * forward[1];
            // A unit standing on the caster is in front of it.
            if (distance == 0.0 || along >= min_cos * distance)
                && self.line_of_sight(caster, unit)?
            {
                found.push(unit);
            }
        }
        Ok(found)
    }

    /// One Lightning Beam evaluation along the caster's current facing, up to `length` units. The
    /// beam pitches up toward the body centre of the unit it aims at (#884): the selection when
    /// it stands in the beam's line, else the nearest unit there, so rising ground before a unit
    /// uphill does not swallow it.
    pub(crate) fn trace_beam(
        &self,
        player_id: PlayerId,
        length: i32,
    ) -> Result<BeamTrace, ZoneError> {
        let caster = EntityRef::Player(player_id);
        let (facing, selection) = self
            .players
            .get(&player_id)
            .map(|player| (player.facing, player.target))
            .ok_or_else(|| ZoneError::new("unknown player"))?;
        let forward = ahead(facing);
        let origin = self.unit_position(caster)?;
        #[allow(clippy::cast_precision_loss)]
        let from = [origin[0] as f32, origin[1] as f32, origin[2] as f32];
        #[allow(clippy::cast_precision_loss)]
        let length = length as f32;
        // (along the beam, rise per unit along it, unit) of every hostile unit in its line.
        let mut met: Vec<(f32, f32, EntityRef)> = Vec::new();
        #[allow(clippy::cast_possible_truncation)]
        let mid = [
            (from[0] + forward[0] * length / 2.0).round() as i32,
            (from[2] + forward[1] * length / 2.0).round() as i32,
        ];
        #[allow(clippy::cast_possible_truncation)]
        let radius = (length / 2.0 + BEAM_HALF_WIDTH_UNITS).ceil() as i32;
        for unit in self.interest.near(mid[0], mid[1], radius) {
            if !self.hostile_to(player_id, unit) {
                continue;
            }
            let position = self.unit_position(unit)?;
            #[allow(clippy::cast_precision_loss)]
            let centre = self
                .unit_centre(unit)
                .unwrap_or(position.map(|axis| axis as f32));
            let delta = [
                centre[0] - from[0],
                centre[1] - from[1],
                centre[2] - from[2],
            ];
            let along = delta[0] * forward[0] + delta[2] * forward[1];
            let lateral = (delta[0] * forward[1] - delta[2] * forward[0]).abs();
            if along > 0.0 && along <= length && lateral <= BEAM_HALF_WIDTH_UNITS {
                met.push((along, delta[1] / along, unit));
            }
        }
        met.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.2.cmp(&b.2)));
        // Only upward: a level ray from the body centre already clears flat or falling ground, and
        // one pitched down at a short unit would meet the ground beyond it.
        let slope = met
            .iter()
            .find(|&&(_, _, unit)| Some(unit) == selection)
            .or_else(|| met.first())
            .map_or(0.0, |&(_, slope, _)| slope.max(0.0));
        let mut reach = self
            .world
            .cast_ray(
                from,
                [forward[0] * length, slope * length, forward[1] * length],
                |body| body.0 < PLAYER_BODY_BASE,
            )
            .map_or(length, |hit| hit.fraction * length);
        let mut victims = Vec::new();
        let mut blocked_by = None;
        for (along, _, unit) in met {
            if along > reach {
                break;
            }
            if let EntityRef::Player(id) = unit
                && self.shield_covers(id, from)?.is_some()
            {
                reach = along;
                blocked_by = Some(id);
                break;
            }
            victims.push(unit);
        }
        #[allow(clippy::cast_possible_truncation)]
        let end = [
            (from[0] + forward[0] * reach).round() as i32,
            (from[2] + forward[1] * reach).round() as i32,
        ];
        Ok(BeamTrace {
            end,
            victims,
            blocked_by,
        })
    }

    /// One beam pulse: the trace's victims take `damage` + `per_level`; a raised shield that
    /// stopped it reports the block.
    pub(crate) fn beam_pulse(
        &mut self,
        player_id: PlayerId,
        trace: &BeamTrace,
        damage: [u16; 2],
        per_level: u16,
        level: u8,
        now: u64,
    ) -> Result<(), ZoneError> {
        let bonus = self
            .players
            .get(&player_id)
            .map_or(0, |player| u32::from(player.damage_bonus()));
        for &victim in &trace.victims {
            let amount = self
                .stream(victim)
                .inclusive(u32::from(damage[0]), u32::from(damage[1]))
                + u32::from(scaled(0, per_level, level))
                + bonus;
            self.player_damages_unit(player_id, victim, amount, false, now)?;
        }
        if let Some(defender) = trace.blocked_by {
            let source = EntityRef::Player(player_id);
            #[allow(clippy::cast_precision_loss)]
            let from = self
                .unit_position(source)
                .map(|p| [p[0] as f32, p[1] as f32, p[2] as f32])?;
            if let Some(lane) = self.shield_covers(defender, from)? {
                self.guard_feedback(
                    defender,
                    ZoneEvent::Blocked {
                        source,
                        target: EntityRef::Player(defender),
                        lane,
                        amount: 0,
                    },
                );
            }
        }
        Ok(())
    }

    /// Chain Lightning's victims: `first`, then up to `jumps` times the nearest unhit hostile
    /// unit within `radius` that the last victim can see (ties to the lower `EntityRef`).
    pub(crate) fn chain_victims(
        &self,
        player_id: PlayerId,
        first: EntityRef,
        jumps: u8,
        radius: i32,
    ) -> Result<Vec<EntityRef>, ZoneError> {
        let mut victims = vec![first];
        let mut current = first;
        for _ in 0..jumps {
            let here = self.unit_position(current)?;
            let mut best: Option<(i64, EntityRef)> = None;
            for unit in self.interest.near(here[0], here[2], radius) {
                if victims.contains(&unit) || !self.hostile_to(player_id, unit) {
                    continue;
                }
                let position = self.unit_position(unit)?;
                let distance = distance_squared([here[0], here[2]], [position[0], position[2]]);
                if distance <= i64::from(radius).pow(2)
                    && best.is_none_or(|(near, _)| distance < near)
                    && self.line_of_sight(current, unit)?
                {
                    best = Some((distance, unit));
                }
            }
            let Some((_, next)) = best else {
                break;
            };
            victims.push(next);
            current = next;
        }
        Ok(victims)
    }
}
