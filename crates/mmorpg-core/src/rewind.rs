//! Capped server rewind of attack-against-body hits (#680, Atlas 5.9.i680): a netcode option
//! an arena match may run, in the style of favor-the-shooter.
//!
//! Content with [`HitRewind`] (and [`crate::TimedCombat`]) gives every timed swing a rewind
//! the zone fixes when the swing begins, from its own knowledge only: the ticks completed
//! since the tick the client had observed when it sent the strike (the #415 lateness, which
//! the zone measures and validates itself), plus the [`CLIENT_INTERPOLATION_TICKS`] every
//! client draws other units behind the newest projection, capped at the match's
//! [`HitRewind::max_ticks`]. A client never names an earlier time.
//!
//! While the swing sweeps, each body it might strike is tested where it stood that many
//! ticks earlier (the zone's position history), from the swinger's present position and
//! against the present static cover. Everything else stays in the present: eligibility,
//! damage, deaths, and above all **clashes**, which compare two swings launched from
//! different views of the past and so always resolve in the zone's present tick.
//!
//! The history holds each player's and living creature's centre for the last
//! `max_ticks` ticks. It is canonical state, so a zone restored from a snapshot rewinds
//! exactly as the original would. Zones without the option keep no history.

use std::collections::VecDeque;

use crate::{EntityRef, ZoneError, ZoneSimulation};

/// The longest rewind any match may allow; within what the #415 late-input grace (150 ms)
/// plus the interpolation can ask for.
pub const MAX_HIT_REWIND_MS: u32 = 200;

/// How far behind the newest projection clients draw other units: the 67 ms interpolation
/// delay of the browser and native clients, in ticks.
pub const CLIENT_INTERPOLATION_TICKS: u16 = crate::ticks_from_ms(67);

/// Rewind of attack-against-body hits for one match's content (#680).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct HitRewind {
    /// The most a hit is rewound, in milliseconds.
    pub max_ms: u32,
}

impl HitRewind {
    /// The arena trial's cap (#680): 150 ms, as long as the #415 late-input grace. Tuning,
    /// not a decision: at 50 ms round trip a hit is rewound fully (3 + 4 ticks), from about
    /// 85 ms on the cap is reached.
    pub const ARENA_TRIAL: Self = Self { max_ms: 150 };

    /// The cap in ticks.
    #[must_use]
    pub const fn max_ticks(&self) -> u16 {
        crate::ticks_from_ms(self.max_ms)
    }

    pub(crate) fn validate(&self) -> Result<(), ZoneError> {
        if self.max_ms > MAX_HIT_REWIND_MS || self.max_ticks() == 0 {
            return Err(ZoneError::new("hit rewind cap is out of range"));
        }
        Ok(())
    }

    /// The rewind of a strike whose command was `late` ticks late.
    #[must_use]
    pub const fn rewind_for(&self, late: u8) -> u16 {
        let wanted = late as u16 + CLIENT_INTERPOLATION_TICKS;
        let max = self.max_ticks();
        if wanted < max { wanted } else { max }
    }
}

/// Where every player and living creature stood at the end of one tick (#680), in
/// [`EntityRef`] order.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct RewindFrame {
    pub tick: u64,
    pub bodies: Vec<(EntityRef, [i32; 3])>,
}

impl ZoneSimulation {
    /// The zone's hit rewind, if its content has one (#680).
    #[must_use]
    pub fn hit_rewind(&self) -> Option<HitRewind> {
        self.content.hit_rewind()
    }

    /// The canonical position history, oldest first.
    pub(crate) fn canonical_rewind_history(&self) -> Vec<RewindFrame> {
        self.rewind_history.iter().cloned().collect()
    }

    /// Restores the position history of a snapshot taken at `tick`.
    pub(crate) fn restore_rewind_history(
        &mut self,
        frames: Vec<RewindFrame>,
        tick: u64,
    ) -> Result<(), ZoneError> {
        let max = self
            .content
            .hit_rewind()
            .map_or(0, |rewind| usize::from(rewind.max_ticks()));
        let ordered = frames.windows(2).all(|pair| pair[0].tick < pair[1].tick);
        let sorted = frames
            .iter()
            .all(|frame| frame.bodies.windows(2).all(|pair| pair[0].0 < pair[1].0));
        if frames.len() > max
            || !ordered
            || !sorted
            || frames
                .iter()
                .any(|frame| frame.tick > tick || tick - frame.tick >= max as u64)
        {
            return Err(ZoneError::new("hit rewind history is invalid"));
        }
        self.rewind_history = frames.into();
        Ok(())
    }

    /// Remembers where every body stands at the end of tick `now`, keeping the last
    /// `max_ticks` ticks; nothing without hit rewind.
    pub(crate) fn record_rewind_frame(&mut self, now: u64) -> Result<(), ZoneError> {
        let Some(rewind) = self.content.hit_rewind() else {
            return Ok(());
        };
        let mut bodies = Vec::with_capacity(self.players.len() + self.creatures.len());
        for &player_id in self.players.keys() {
            let position = self.player_position(player_id)?;
            bodies.push((
                EntityRef::Player(player_id),
                [position.x, position.y, position.z],
            ));
        }
        for (&id, creature) in &self.creatures {
            if let Some(position) = self.creature_position(id, creature)? {
                bodies.push((EntityRef::Creature(id), position));
            }
        }
        bodies.sort_unstable_by_key(|(entity, _)| *entity);
        self.rewind_history
            .push_back(RewindFrame { tick: now, bodies });
        while self.rewind_history.len() > usize::from(rewind.max_ticks()) {
            self.rewind_history.pop_front();
        }
        Ok(())
    }

    /// Where `entity` stood `rewind` ticks before the tick being advanced, if the history
    /// holds it; `None` for no rewind or an unknown position (the present then applies).
    pub(crate) fn rewound_centre(&self, entity: EntityRef, rewind: u16) -> Option<[i32; 3]> {
        if rewind == 0 {
            return None;
        }
        // The tick being advanced is `self.tick + 1`; frames end completed ticks.
        let tick = (self.tick + 1).checked_sub(u64::from(rewind))?;
        let frame = self
            .rewind_history
            .iter()
            .find(|frame| frame.tick == tick)?;
        frame
            .bodies
            .binary_search_by_key(&entity, |(body, _)| *body)
            .ok()
            .map(|index| frame.bodies[index].1)
    }
}

/// The history type the zone keeps.
pub(crate) type RewindHistory = VecDeque<RewindFrame>;
