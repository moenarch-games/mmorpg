//! Timed swings and clashes (#679, Atlas 5.2.i679, 5.2.i679-2): arena combat first.
//!
//! A zone whose content carries [`TimedCombat`] (the Battle Royale practice arena) runs a
//! directed strike as a **timed swing** instead of resolving it in the tick its command
//! arrives. The command begins the swing: the strike's ordinary validation, cost, cooldowns
//! and guard release apply at once, and observers see [`crate::ZoneEvent::SwingBegan`], so an
//! opponent sees the wind-up coming. Then:
//!
//! - **Startup** ([`SwingTiming::startup_ms`]): the wind-up. Nothing is struck yet.
//! - **Active** ([`SwingTiming::active_ms`]): the blade sweeps the strike's arc (the same arc,
//!   reach and height lane as an immediate directed strike), one equal slice per active tick,
//!   travelling toward the aimed side (a centre strike travels right to left). Each tick the
//!   nearest eligible unit its slice covers takes the ordinary weapon strike, physical cover
//!   still blocking; a swing strikes at most one unit (no cleave). A swing that strikes nobody
//!   reports its whiff on its last active tick.
//! - **Recovery**: the action graph's `DirectedSlash` recovery, committed when the active phase
//!   ends, exactly as after an immediate strike.
//!
//! Clashes resolve in the zone's present tick, never by rewind (#680), in stable order, before
//! any sweep strikes in that tick. Their starting rules are tunable data ([`ClashRules`]):
//!
//! - **Swing against swing**: two hostile swings both in their active phase, each one's whole
//!   arc covering the other swinger, clash. Neither deals damage; both go straight into
//!   recovery. Pairs are examined in player-ID order, and a swing clashes at most once.
//! - **Swing against projectile**: an active swing destroys a clashable hostile projectile
//!   whose path this tick passes through its whole arc and lane; the swing continues.
//! - **Projectile against projectile**: two clashable projectiles of opposing owners whose
//!   paths this tick come within their clash radii of each other both vanish. Pairs are
//!   examined in projectile-ID order.
//!
//! Clashable projectiles are plain arrows and free (non-homing) Firebolts, each with its own
//! clash radius. Bombs, flasks, piercing and ricochet shots stay out, and homing missiles need
//! their own rule. Health alone decides fights: a clash moves nobody (5.2.i679-2).
//!
//! The rules are part of the zone content, so its revision binds a recovered zone to them, and
//! a running swing is canonical player state: a zone restored mid-swing continues it exactly.
//! There is no second combat engine: the swing is the action graph's `DirectedSlash` running
//! its startup and active phases, and its strike is the ordinary directed strike.

use std::collections::BTreeSet;

use crate::ability::{Shot, ids};
use crate::entity::body_id;
use crate::projectile::{Flight, Projectile, ProjectileKind};
use crate::strike::{STRIKE_ARC_HALF_WIDTH, Sweep};
use crate::unit::PLAYER_REACH_UNITS;
use crate::{
    AbilityId, EntityRef, PLAYER_HALF_EXTENTS_UNITS, PlayerId, StrikeAim, StrikeSide, ZoneError,
    ZoneEvent, ZoneSimulation,
};

/// The longest a swing's startup or active phase may last (about one second, 63 ticks at
/// most), so each fits the six bits [`crate::ZoneEvent::SwingBegan`] gives it on the wire.
pub const MAX_SWING_PHASE_TICKS: u16 = 63;
/// The largest clash radius a projectile may have, in units.
pub const MAX_CLASH_RADIUS_UNITS: u16 = 200;
/// Points sampled along a projectile's path this tick when a sweep looks for it.
const PARRY_SAMPLES: u16 = 4;

/// How long a timed swing winds up and sweeps.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SwingTiming {
    /// The wind-up before the blade sweeps; may be zero.
    pub startup_ms: u32,
    /// The sweep across the strike's arc; at least one tick.
    pub active_ms: u32,
}

/// Which meetings clash, and how large clashable projectiles are (units).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ClashRules {
    /// Two hostile active swings that cover each other clash.
    pub swings: bool,
    /// Two clashable projectiles of opposing owners that meet clash.
    pub projectiles: bool,
    /// An active swing destroys a clashable hostile projectile its arc meets.
    pub parries: bool,
    /// A plain arrow's clash radius.
    pub arrow_radius_units: u16,
    /// A free Firebolt's clash radius.
    pub firebolt_radius_units: u16,
}

/// Timed swings and clashes for one zone's content (#679).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TimedCombat {
    pub swing: SwingTiming,
    pub clashes: ClashRules,
}

impl TimedCombat {
    /// The arena's starting values (5.2.i679), to tune in play: a 0.3 s wind-up, a 0.15 s
    /// sweep, every clash on, arrows 15 cm and Firebolts 25 cm across.
    pub const ARENA: Self = Self {
        swing: SwingTiming {
            startup_ms: 300,
            active_ms: 150,
        },
        clashes: ClashRules {
            swings: true,
            projectiles: true,
            parries: true,
            arrow_radius_units: 15,
            firebolt_radius_units: 25,
        },
    };

    /// The wind-up in ticks.
    #[must_use]
    pub const fn startup_ticks(&self) -> u16 {
        crate::ticks_from_ms(self.swing.startup_ms)
    }

    /// The sweep in ticks.
    #[must_use]
    pub const fn active_ticks(&self) -> u16 {
        crate::ticks_from_ms(self.swing.active_ms)
    }

    pub(crate) fn validate(&self) -> Result<(), ZoneError> {
        let ms = (MAX_SWING_PHASE_TICKS as u32 * 1_000) / crate::TICK_HZ as u32;
        if self.swing.startup_ms > ms
            || self.swing.active_ms > ms
            || self.startup_ticks() > MAX_SWING_PHASE_TICKS
            || self.active_ticks() == 0
            || self.active_ticks() > MAX_SWING_PHASE_TICKS
        {
            return Err(ZoneError::new("timed swing phases are out of range"));
        }
        if self.clashes.arrow_radius_units > MAX_CLASH_RADIUS_UNITS
            || self.clashes.firebolt_radius_units > MAX_CLASH_RADIUS_UNITS
        {
            return Err(ZoneError::new("projectile clash radius is out of range"));
        }
        Ok(())
    }

    /// The clash radius of `projectile` while it flies, if it is clashable: a plain arrow or a
    /// free Firebolt.
    fn clash_radius(&self, projectile: &Projectile) -> Option<f32> {
        if projectile.is_stuck() || projectile.is_burst() || projectile.shot() != Shot::Plain {
            return None;
        }
        let radius = match projectile.kind {
            ProjectileKind::Arrow => self.clashes.arrow_radius_units,
            ProjectileKind::Missile
                if projectile.ability == ids::FIREBOLT && projectile.flight == Flight::Free =>
            {
                self.clashes.firebolt_radius_units
            }
            _ => return None,
        };
        Some(f32::from(radius))
    }
}

/// What met in a clash.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, strum::VariantArray)]
pub enum ClashKind {
    /// Two swings: neither struck, both recover.
    Swings,
    /// Two projectiles: both vanished.
    Projectiles,
    /// A swing destroyed a projectile and swung on.
    Parry,
}

impl ClashKind {
    /// Wire code: 0 swings, 1 projectiles, 2 parry.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Swings => 0,
            Self::Projectiles => 1,
            Self::Parry => 2,
        }
    }

    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Swings),
            1 => Some(Self::Projectiles),
            2 => Some(Self::Parry),
            _ => None,
        }
    }
}

/// The phase a running swing is in for its next tick.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SwingPhase {
    Startup,
    Active,
}

/// A running timed swing; canonical player state (#679).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Swing {
    pub ability: AbilityId,
    /// The facing it was aimed from; turning afterwards does not move the blade.
    pub facing: u16,
    pub aim: StrikeAim,
    /// Ticks run since it began.
    pub elapsed: u16,
    /// Its content's wind-up and sweep, in ticks.
    pub startup: u16,
    pub active: u16,
    /// Whether it already struck a unit; it strikes at most one.
    pub struck: bool,
    /// Ticks its sweep looks back at bodies (#680); 0 without hit rewind.
    pub rewind: u16,
}

impl Swing {
    /// Whether it is a well-formed swing that has not ended.
    #[must_use]
    pub const fn is_valid(&self) -> bool {
        crate::has_directed_mode(self.ability)
            && self.active > 0
            && self.active <= MAX_SWING_PHASE_TICKS
            && self.startup <= MAX_SWING_PHASE_TICKS
            && self.elapsed < self.startup + self.active
            && self.rewind <= crate::ticks_from_ms(crate::MAX_HIT_REWIND_MS)
    }

    /// The phase of its next tick.
    #[must_use]
    pub const fn phase(&self) -> SwingPhase {
        if self.elapsed < self.startup {
            SwingPhase::Startup
        } else {
            SwingPhase::Active
        }
    }

    /// Ticks left of its active phase, counting the next tick.
    #[must_use]
    pub const fn active_left(&self) -> u16 {
        (self.startup + self.active).saturating_sub(self.elapsed)
    }

    /// The active tick `elapsed` is, from 0, once it has run that many ticks.
    const fn active_tick(&self) -> Option<u16> {
        if self.elapsed > self.startup && self.elapsed <= self.startup + self.active {
            Some(self.elapsed - self.startup - 1)
        } else {
            None
        }
    }

    /// The whole arc and lane it sweeps.
    const fn whole(&self) -> Sweep {
        Sweep::full(self.facing, self.aim)
    }

    /// The slice of the arc active tick `tick` sweeps. Slices are contiguous and together are
    /// the whole arc; the blade travels toward the aimed side, a centre strike right to left.
    #[allow(clippy::cast_possible_truncation)]
    fn slice(&self, tick: u16) -> Sweep {
        let whole = self.whole();
        let total = 2 * u32::from(STRIKE_ARC_HALF_WIDTH);
        let active = u32::from(self.active.max(1));
        let from = u32::from(tick) * total / active;
        let to = (u32::from(tick) + 1) * total / active;
        let middle = ((from + to) / 2) as u16;
        let half_width = (to - from).div_ceil(2) as u16;
        let centre = match self.aim.side {
            StrikeSide::Right => whole
                .centre
                .wrapping_add(STRIKE_ARC_HALF_WIDTH)
                .wrapping_sub(middle),
            StrikeSide::Left | StrikeSide::Centre => whole
                .centre
                .wrapping_sub(STRIKE_ARC_HALF_WIDTH)
                .wrapping_add(middle),
        };
        Sweep {
            centre,
            half_width,
            height: whole.height,
        }
    }
}

impl ZoneSimulation {
    /// The zone's timed swing and clash rules, if its content has them (#679).
    #[must_use]
    pub fn timed_combat(&self) -> Option<TimedCombat> {
        self.content.timed_combat()
    }

    /// The running timed swing of `player_id`, if any (#679).
    #[must_use]
    pub fn swing_of(&self, player_id: PlayerId) -> Option<Swing> {
        self.players.get(&player_id).and_then(|player| player.swing)
    }

    /// A directed strike begins a timed swing: observers see its wind-up.
    pub(crate) fn begin_swing(
        &mut self,
        player_id: PlayerId,
        ability: AbilityId,
        facing: u16,
        aim: StrikeAim,
        timed: TimedCombat,
    ) {
        let swing = Swing {
            ability,
            facing,
            aim,
            elapsed: 0,
            startup: timed.startup_ticks(),
            active: timed.active_ticks(),
            struck: false,
            // Fixed now, from the lateness the zone measured itself (#680).
            rewind: self
                .content
                .hit_rewind()
                .map_or(0, |rewind| rewind.rewind_for(self.resolving_late)),
        };
        self.update_player(player_id, |player| player.swing = Some(swing));
        let source = EntityRef::Player(player_id);
        #[allow(clippy::cast_possible_truncation)]
        self.notify_units(
            [Some(source), None],
            ZoneEvent::SwingBegan {
                source,
                ability,
                aim,
                startup: swing.startup as u8,
                active: swing.active as u8,
            },
        );
    }

    /// Every running swing advances one tick, in player-ID order: a dead or stunned swinger's
    /// swing ends, swings clash, sweeps parry projectiles, then each active slice strikes.
    pub(crate) fn advance_swings(&mut self, now: u64) -> Result<(), ZoneError> {
        let Some(timed) = self.content.timed_combat() else {
            return Ok(());
        };
        let swinging: Vec<PlayerId> = self
            .players
            .iter()
            .filter(|(_, player)| player.swing.is_some())
            .map(|(&id, _)| id)
            .collect();
        let mut active = Vec::new();
        for player_id in swinging {
            let Some(player) = self.players.get_mut(&player_id) else {
                continue;
            };
            let Some(mut swing) = player.swing else {
                continue;
            };
            if !player.is_alive() || crate::aura::has_kind(&player.auras, crate::AuraKind::Stun) {
                player.swing = None;
                continue;
            }
            swing.elapsed += 1;
            player.swing = Some(swing);
            if let Some(tick) = swing.active_tick() {
                active.push((player_id, tick));
            }
        }
        if timed.clashes.swings {
            let mut clashed = BTreeSet::new();
            for (index, &(first, _)) in active.iter().enumerate() {
                for &(second, _) in &active[index + 1..] {
                    if clashed.contains(&first) {
                        break;
                    }
                    if clashed.contains(&second) || !self.players_are_hostile(first, second) {
                        continue;
                    }
                    if self.swing_reaches(first, second)? && self.swing_reaches(second, first)? {
                        clashed.extend([first, second]);
                        self.clash_swings(first, second);
                    }
                }
            }
            active.retain(|(player_id, _)| !clashed.contains(player_id));
        }
        if timed.clashes.parries {
            for &(player_id, _) in &active {
                self.parry_projectiles(player_id, timed)?;
            }
        }
        for (player_id, tick) in active {
            let Some(player) = self.players.get(&player_id) else {
                continue;
            };
            let Some(mut swing) = player.swing else {
                continue;
            };
            // A blow earlier in this tick may have felled or stunned the swinger.
            if !player.is_alive() || crate::aura::has_kind(&player.auras, crate::AuraKind::Stun) {
                self.update_player(player_id, |player| player.swing = None);
                continue;
            }
            let last = tick + 1 >= swing.active;
            if !swing.struck {
                // Bodies only, rewound by the swing's own rewind (#680); clashes above stay
                // in the present.
                let victim = self.sweep_coverage(player_id, swing.slice(tick), swing.rewind)?;
                if victim.is_some() {
                    swing.struck = true;
                    self.update_player(player_id, |player| player.swing = Some(swing));
                }
                if victim.is_some() || last {
                    self.strike_resolves(player_id, swing.ability, victim, swing.aim, now)?;
                }
            }
            if last {
                self.end_swing(player_id, swing.ability);
            }
        }
        Ok(())
    }

    /// Whether `player_id`'s swing, over its whole arc and lane, covers `other`.
    fn swing_reaches(&self, player_id: PlayerId, other: PlayerId) -> Result<bool, ZoneError> {
        let Some(swing) = self.swing_of(player_id) else {
            return Ok(false);
        };
        // In the present, never rewound (#680).
        Ok(self
            .sweep_distance(player_id, swing.whole(), EntityRef::Player(other), 0)?
            .is_some())
    }

    /// The swing ends and commits to the graph's `DirectedSlash` recovery.
    fn end_swing(&mut self, player_id: PlayerId, ability: AbilityId) {
        let recovery = self.recovery_of(player_id, crate::action::ActionId::DirectedSlash, ability);
        self.update_player(player_id, |player| {
            player.swing = None;
            player.strike_recovery = recovery;
        });
    }

    /// Two swings met: neither strikes, both recover.
    fn clash_swings(&mut self, first: PlayerId, second: PlayerId) {
        for player_id in [first, second] {
            if let Some(swing) = self.swing_of(player_id) {
                self.end_swing(player_id, swing.ability);
            }
        }
        let (source, target) = (EntityRef::Player(first), EntityRef::Player(second));
        self.notify_units(
            [Some(source), Some(target)],
            ZoneEvent::Clashed {
                source,
                target,
                kind: ClashKind::Swings,
            },
        );
    }

    /// An active swing destroys every clashable hostile projectile whose path this tick passes
    /// through its arc and lane, in projectile-ID order.
    fn parry_projectiles(
        &mut self,
        player_id: PlayerId,
        timed: TimedCombat,
    ) -> Result<(), ZoneError> {
        let Some(swing) = self.swing_of(player_id) else {
            return Ok(());
        };
        let swinger = EntityRef::Player(player_id);
        let origin = self.player_position(player_id)?;
        #[allow(clippy::cast_precision_loss)]
        let (x, z, feet) = (
            origin.x as f32,
            origin.z as f32,
            (origin.y - PLAYER_HALF_EXTENTS_UNITS[1]) as f32,
        );
        let sweep = swing.whole();
        let (dir_x, dir_z) = crate::trig::direction(sweep.centre);
        #[allow(clippy::cast_precision_loss)]
        let (dir_x, dir_z) = (
            dir_x as f32 / crate::trig::TRIG_ONE as f32,
            dir_z as f32 / crate::trig::TRIG_ONE as f32,
        );
        #[allow(clippy::cast_precision_loss)]
        let min_cos = crate::trig::cos(sweep.half_width) as f32 / crate::trig::TRIG_ONE as f32;
        let [low, high] = sweep.height.band();
        #[allow(clippy::cast_precision_loss)]
        let (low, high, reach) = (low as f32, high as f32, PLAYER_REACH_UNITS as f32);
        let mut parried = Vec::new();
        for projectile in self.projectiles.values() {
            let Some(radius) = timed.clash_radius(projectile) else {
                continue;
            };
            if !self.projectile_meets(projectile.owner, body_id(swinger)) {
                continue;
            }
            let meets = (0..=PARRY_SAMPLES).any(|step| {
                let t = f32::from(step) / f32::from(PARRY_SAMPLES);
                let [px, py, pz] = projectile.position;
                let [vx, vy, vz] = projectile.velocity;
                let (dx, dz) = (px + vx * t - x, pz + vz * t - z);
                let height = py + vy * t - feet;
                let distance = dx.hypot(dz);
                distance <= reach + radius
                    && (distance == 0.0 || dx * dir_x + dz * dir_z >= distance * min_cos)
                    && height >= low - radius
                    && height <= high + radius
            });
            if meets {
                parried.push((projectile.id, projectile.owner));
            }
        }
        for (id, owner) in parried {
            self.projectiles.remove(&id);
            self.notify_units(
                [Some(swinger), Some(owner)],
                ZoneEvent::Clashed {
                    source: swinger,
                    target: owner,
                    kind: ClashKind::Parry,
                },
            );
        }
        Ok(())
    }

    /// Clashable projectiles of opposing owners whose paths this tick come within their clash
    /// radii both vanish, pairs in projectile-ID order; each clashes at most once.
    pub(crate) fn clash_projectiles(&mut self) {
        let Some(timed) = self.content.timed_combat() else {
            return;
        };
        if !timed.clashes.projectiles {
            return;
        }
        let flying: Vec<(Projectile, f32)> = self
            .projectiles
            .values()
            .filter_map(|projectile| Some((*projectile, timed.clash_radius(projectile)?)))
            .collect();
        let mut gone = BTreeSet::new();
        let mut clashes = Vec::new();
        for (index, (first, first_radius)) in flying.iter().enumerate() {
            for (second, second_radius) in &flying[index + 1..] {
                if gone.contains(&first.id) {
                    break;
                }
                if gone.contains(&second.id)
                    || first.owner == second.owner
                    || !self.projectile_meets(first.owner, body_id(second.owner))
                {
                    continue;
                }
                if closest_approach(first, second) <= first_radius + second_radius {
                    gone.extend([first.id, second.id]);
                    clashes.push((first.owner, second.owner));
                }
            }
        }
        for id in gone {
            self.projectiles.remove(&id);
        }
        for (source, target) in clashes {
            self.notify_units(
                [Some(source), Some(target)],
                ZoneEvent::Clashed {
                    source,
                    target,
                    kind: ClashKind::Projectiles,
                },
            );
        }
    }
}

/// The closest two projectiles come while each flies its velocity this tick.
fn closest_approach(first: &Projectile, second: &Projectile) -> f32 {
    let offset: [f32; 3] = std::array::from_fn(|axis| first.position[axis] - second.position[axis]);
    let motion: [f32; 3] = std::array::from_fn(|axis| first.velocity[axis] - second.velocity[axis]);
    let speed = motion.iter().map(|axis| axis * axis).sum::<f32>();
    let t = if speed > 0.0 {
        (-(offset[0] * motion[0] + offset[1] * motion[1] + offset[2] * motion[2]) / speed)
            .clamp(0.0, 1.0)
    } else {
        0.0
    };
    let at: [f32; 3] = std::array::from_fn(|axis| offset[axis] + motion[axis] * t);
    (at[0] * at[0] + at[1] * at[1] + at[2] * at[2]).sqrt()
}
