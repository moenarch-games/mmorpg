//! Private homes and urgent night visits (#158, Atlas 5.1.47/5.1.48).
//!
//! A [`PrivateHome`] is a routine NPC's house behind one content door. Only a character with a
//! quest from or for the resident (active or turned in) may move that door; everyone else is
//! refused with [`crate::ErrorCode::PrivateHome`]. The door closes itself
//! [`PRIVATE_DOOR_CLOSE_TICKS`] after it last moved, whenever its leaf is clear.
//!
//! At night the resident sleeps at its routine's home, and using the closed door knocks
//! instead. A knock from a character with an urgent matter (a quest ending at the resident whose
//! objectives are complete) wakes it: it walks the home's wake path to the doorway, opens the
//! door and answers turn-ins there. After [`WAKE_IDLE_TICKS`] without a quest request addressed
//! to it, it closes the door and walks back to bed. Other knocks get no answer
//! ([`crate::ErrorCode::NoAnswer`]). The resident offers no new quests at night
//! ([`crate::ErrorCode::NotAtNight`]). Its placement remains a pure function of the tick and the
//! canonical [`NpcWake`], so replay and recovery place it exactly; dawn ends any wake.

use crate::{
    DoorId, EntityRef, ErrorCode, NpcId, NpcPlacement, PlayerId, ZoneError, ZoneSimulation,
};

/// A woken resident goes back to bed after this long without a quest request (60 s).
pub const WAKE_IDLE_TICKS: u64 = crate::long_ticks_from_ms(60_000) as u64;
/// A private home's door closes itself this long after it last moved (8 s).
pub const PRIVATE_DOOR_CLOSE_TICKS: u64 = crate::ticks_from_ms(8_000) as u64;
/// Waypoints a wake path may have.
pub const MAX_WAKE_WAYPOINTS: usize = 4;

/// A routine NPC's private home.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrivateHome {
    pub npc: NpcId,
    pub door: DoorId,
    /// From the routine's home (exclusive) to the doorway spot just inside the door, where a
    /// woken resident answers.
    pub wake_path: Vec<[i32; 2]>,
    /// The resident's facing in the doorway, out through the door.
    pub doorway_facing: u16,
    /// What the woken resident says before the quest dialogue (clients present it).
    pub woken_line: String,
}

/// A woken resident: the tick it was knocked awake and the tick of the last quest request
/// addressed to it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NpcWake {
    pub npc: NpcId,
    pub knocked: u64,
    pub idle_since: u64,
}

fn walk_ticks(legs: &[([i32; 2], [i32; 2], i64)]) -> u64 {
    let total: i64 = legs.iter().map(|leg| leg.2).sum();
    u64::try_from(total.div_euclid(crate::NPC_WALK_UNITS_PER_TICK) + 1).unwrap_or(u64::MAX)
}

impl NpcWake {
    /// The tick the resident starts back to bed: idle long enough, and not before it reached
    /// the doorway.
    fn return_tick(&self, walk: u64) -> u64 {
        (self.idle_since + WAKE_IDLE_TICKS).max(self.knocked + walk)
    }
}

impl ZoneSimulation {
    /// Whether `player_id` has a quest from or for `npc`, active or turned in.
    fn welcome_in(&self, player_id: PlayerId, npc: NpcId) -> bool {
        let Some(player) = self.players.get(&player_id) else {
            return false;
        };
        self.content.quests().iter().any(|quest| {
            (quest.giver == npc || quest.ender == npc)
                && (player.quests.entry(quest.id).is_some() || player.quests.is_completed(quest.id))
        })
    }

    /// Whether `player_id` can turn in a quest at `npc` now.
    fn urgent_for(&self, player_id: PlayerId, npc: NpcId) -> bool {
        let Some(player) = self.players.get(&player_id) else {
            return false;
        };
        player.quests.entries.iter().any(|entry| {
            self.content.quest(entry.quest).is_some_and(|quest| {
                quest.ender == npc && crate::quest::objectives_done(quest, entry, &player.inventory)
            })
        })
    }

    fn is_night_now(&self) -> bool {
        crate::is_night(crate::time_of_day(self.tick))
    }

    /// Whether `npc` lives in a private home and so offers no new quests now.
    pub(crate) fn resting(&self, npc: NpcId) -> bool {
        self.content.private_home(npc).is_some() && self.is_night_now()
    }

    /// A quest request addressed to a woken resident keeps it at the door.
    pub(crate) fn keep_awake(&mut self, npc: NpcId) {
        let tick = self.tick;
        if let Some(wake) = self.wakes.get_mut(&npc) {
            wake.idle_since = tick;
        }
    }

    /// The private-home rules for a door use the player is within reach of: `Some` refuses it
    /// or settles it (a knock), `None` lets it move like any door.
    pub(crate) fn private_door_use(
        &mut self,
        player_id: PlayerId,
        id: DoorId,
        open: bool,
    ) -> Option<Option<(ErrorCode, Option<EntityRef>)>> {
        let home = self.content.home_behind(id)?.clone();
        let npc = home.npc;
        let refuse = |code| Some(Some((code, None)));
        if self.wakes.contains_key(&npc) {
            return refuse(ErrorCode::PrivateHome);
        }
        let closed = !self.doors.get(&id).is_some_and(|state| state.open);
        if self.is_night_now() && closed && open {
            if !self.asleep(npc) || !self.urgent_for(player_id, npc) {
                return refuse(ErrorCode::NoAnswer);
            }
            self.wakes.insert(
                npc,
                NpcWake {
                    npc,
                    knocked: self.tick,
                    idle_since: self.tick,
                },
            );
            return Some(None);
        }
        if !self.welcome_in(player_id, npc) {
            return refuse(ErrorCode::PrivateHome);
        }
        None
    }

    /// Whether `npc` is at its routine's home, not walking.
    fn asleep(&self, npc: NpcId) -> bool {
        let (Some(definition), Some(routine)) = (self.content.npc(npc), self.content.routine(npc))
        else {
            return false;
        };
        let placement = routine.placement(
            definition.position,
            definition.facing,
            crate::time_of_day(self.tick),
        );
        placement.velocity == [0, 0] && routine.path.last() == Some(&placement.position)
    }

    /// A woken resident's placement at the current tick, from its routine's home.
    pub(crate) fn wake_placement(&self, wake: &NpcWake, home: [i32; 2]) -> Option<NpcPlacement> {
        let private = self.content.private_home(wake.npc)?;
        let legs = crate::routine::legs(home, &private.wake_path);
        let walk = walk_ticks(&legs);
        let speed = crate::NPC_WALK_UNITS_PER_TICK;
        let total: i64 = legs.iter().map(|leg| leg.2).sum();
        let out = i64::try_from(self.tick.saturating_sub(wake.knocked)).unwrap_or(i64::MAX);
        if out.saturating_mul(speed) < total {
            return Some(crate::routine::along(&legs, out * speed, true));
        }
        let back_start = wake.return_tick(walk);
        if self.tick < back_start {
            return Some(NpcPlacement {
                position: legs.last().map_or(home, |leg| leg.1),
                facing: private.doorway_facing,
                velocity: [0, 0],
            });
        }
        let back = i64::try_from(self.tick - back_start).unwrap_or(i64::MAX);
        if back.saturating_mul(speed) < total {
            return Some(crate::routine::along(&legs, back * speed, false));
        }
        None
    }

    /// Tick step after the routine placements: woken residents open their door at the doorway,
    /// close it when they turn back and are forgotten in bed or at dawn; idle private doors
    /// close themselves.
    pub(crate) fn advance_private_homes(&mut self) -> Result<(), ZoneError> {
        let night = self.is_night_now();
        let homes = self.content.private_homes().to_vec();
        for home in homes {
            let Some(door) = self.content.door(home.door).copied() else {
                continue;
            };
            let state = self.doors.get(&door.id).copied().unwrap_or_default();
            let Some(wake) = self.wakes.get(&home.npc).copied() else {
                if state.open
                    && self.tick >= state.changed_tick + PRIVATE_DOOR_CLOSE_TICKS
                    && !self.leaf_blocked(&door, false)?
                {
                    self.set_door(&door, false, self.tick)?;
                }
                continue;
            };
            let bed = self
                .content
                .routine(home.npc)
                .and_then(|routine| routine.path.last().copied());
            let Some(bed) = bed.filter(|_| night) else {
                self.wakes.remove(&home.npc);
                continue;
            };
            let walk = walk_ticks(&crate::routine::legs(bed, &home.wake_path));
            if self.tick >= wake.return_tick(walk) {
                if state.open && !self.leaf_blocked(&door, false)? {
                    self.set_door(&door, false, self.tick)?;
                }
                if self.wake_placement(&wake, bed).is_none() {
                    self.wakes.remove(&home.npc);
                }
            } else if self.tick >= wake.knocked + walk && !state.open {
                self.try_open_for_npc(&door)?;
            }
        }
        Ok(())
    }

    /// Every woken resident, in NPC-ID order.
    #[must_use]
    pub(crate) fn canonical_wakes(&self) -> Vec<NpcWake> {
        self.wakes.values().copied().collect()
    }

    /// Restores woken residents from a snapshot taken at `tick`.
    pub(crate) fn restore_wakes(&mut self, wakes: &[NpcWake], tick: u64) -> Result<(), ZoneError> {
        if wakes.windows(2).any(|pair| pair[0].npc >= pair[1].npc)
            || wakes.iter().any(|wake| {
                self.content.private_home(wake.npc).is_none()
                    || wake.knocked > wake.idle_since
                    || wake.idle_since > tick
            })
        {
            return Err(ZoneError::new("snapshot npc wakes are invalid"));
        }
        self.wakes = wakes.iter().map(|wake| (wake.npc, *wake)).collect();
        Ok(())
    }
}
