//! Immutable starter progression rules. Award eligibility and mutable XP
//! belong to the authoritative zone; these queries never grant experience.

use crate::unit::MAX_UNIT_LEVEL;

/// Starter progression ends at level 10; creatures retain their own level bound.
pub const MAX_PLAYER_LEVEL: u8 = 10;
/// Moderate early pacing (#908): two kills reach level 2, then roughly 20–40
/// minutes of mixed Greyhaven play per level from level 6 (docs/PROGRESSION.md).
const NEXT_LEVEL_EXPERIENCE: [u32; 10] =
    [100, 300, 600, 1_000, 1_600, 2_000, 2_400, 2_800, 3_200, 0];

/// Current-level XP needed for the next level. The cap returns zero;
/// unsupported player levels return `None`. XP is not a lifetime total.
#[must_use]
pub fn experience_to_next_level(level: u8) -> Option<u32> {
    level
        .checked_sub(1)
        .and_then(|index| NEXT_LEVEL_EXPERIENCE.get(usize::from(index)))
        .copied()
}

/// Kill reward before tap/range/liveness eligibility checks.
///
/// Base XP is 45 + 5 × creature level. Each level below the player reduces
/// it by 20%; five or more levels below gives zero. Each level above adds
/// 10%, capped at +50%. Integer rewards round down. Capped players get zero.
/// Unsupported player/creature levels return `None`, not a fallback reward.
#[must_use]
pub fn kill_experience(player_level: u8, creature_level: u8) -> Option<u32> {
    experience_to_next_level(player_level)?;
    if !(1..=MAX_UNIT_LEVEL).contains(&creature_level) {
        return None;
    }
    if player_level == MAX_PLAYER_LEVEL {
        return Some(0);
    }
    let percent = if creature_level < player_level {
        100_u32.saturating_sub(20 * u32::from(player_level - creature_level))
    } else {
        100 + 10 * u32::from((creature_level - player_level).min(5))
    };
    Some((45 + 5 * u32::from(creature_level)) * percent / 100)
}

pub(crate) fn valid_progression(level: u8, experience: u32) -> bool {
    experience_to_next_level(level).is_some_and(|threshold| {
        if threshold == 0 {
            experience == 0
        } else {
            experience < threshold
        }
    })
}

impl crate::ZoneSimulation {
    /// Whether the admitted tapper is alive and within the interest radius
    /// of the corpse at `position`: it then earns the kill's experience and
    /// quest credit. Checked once by the alive-to-corpse transition.
    pub(crate) fn kill_credit_eligible(
        &self,
        player_id: crate::PlayerId,
        position: [i32; 3],
    ) -> Result<bool, crate::ZoneError> {
        let Some(player) = self.players.get(&player_id) else {
            return Ok(false);
        };
        if !player.is_alive() {
            return Ok(false);
        }
        let player_position = self.player_position(player_id)?;
        Ok(crate::reach::within_xz(
            [player_position.x, player_position.z],
            [position[0], position[2]],
            crate::INTEREST_RADIUS_UNITS,
        ))
    }

    /// Kill experience for an eligible tapper.
    pub(crate) fn reward_kill_experience(
        &mut self,
        player_id: crate::PlayerId,
        creature_level: u8,
    ) -> Result<(), crate::ZoneError> {
        let Some(player) = self.players.get(&player_id) else {
            return Ok(());
        };
        let reward = kill_experience(player.level, creature_level)
            .ok_or_else(|| crate::ZoneError::new("invalid progression level"))?;
        self.grant_experience(player_id, reward)
    }

    /// Adds experience, levelling up through the starter curve; the cap
    /// keeps zero experience. Current health and mana grow with their
    /// maximums; rage and focus keep a fixed maximum.
    pub(crate) fn grant_experience(
        &mut self,
        player_id: crate::PlayerId,
        amount: u32,
    ) -> Result<(), crate::ZoneError> {
        let Some(player) = self.players.get_mut(&player_id) else {
            return Ok(());
        };
        if player.level == MAX_PLAYER_LEVEL {
            return Ok(());
        }
        let old_level = player.level;
        player.experience = player.experience.saturating_add(amount);
        while player.level < MAX_PLAYER_LEVEL {
            let threshold = experience_to_next_level(player.level)
                .ok_or_else(|| crate::ZoneError::new("invalid progression level"))?;
            if player.experience < threshold {
                break;
            }
            player.experience -= threshold;
            player.level += 1;
        }
        if player.level == MAX_PLAYER_LEVEL {
            player.experience = 0;
        }
        // The maximum as it was, physique included (#633): the gain follows the final maximum.
        let old_max = crate::physique::max_health(old_level, &player.equipment, &player.physique);
        player.health += player.max_health() - old_max;
        if let Some(choice) = player.class {
            let kind = choice.class.resource();
            player.resource.value += kind.max(player.level) - kind.max(old_level);
        }
        let level = player.level;
        if level != old_level {
            self.level_companion(player_id, level);
        }
        Ok(())
    }
}

/// Experience for discovering a named area (#916): one and a half times the base of an
/// equal-level kill, `(45 + 5 × level) × 3 / 2` rounded down, so a detour to a new place pays
/// about what the fights on the way do. Capped players get zero; unsupported levels `None`.
#[must_use]
pub fn discovery_experience(player_level: u8) -> Option<u32> {
    experience_to_next_level(player_level)?;
    if player_level == MAX_PLAYER_LEVEL {
        return Some(0);
    }
    Some((45 + 5 * u32::from(player_level)) * 3 / 2)
}

impl crate::ZoneSimulation {
    /// Tick step after movement (#916): a living player inside one of the content's discovery
    /// areas it has not yet discovered discovers it and earns [`discovery_experience`] once.
    /// The discovered set is a durable character fact, so neither a replay, a reconnect nor a
    /// recovery pays an area twice; the receipt guards hosts that track rewards.
    pub(crate) fn discover_areas(&mut self) -> Result<(), crate::ZoneError> {
        let content = std::sync::Arc::clone(&self.content);
        if content.discoveries().is_empty() {
            return Ok(());
        }
        let ids: Vec<_> = self
            .players
            .iter()
            .filter(|(_, player)| player.is_alive())
            .map(|(&id, _)| id)
            .collect();
        for player_id in ids {
            let at = self.player_position(player_id)?;
            // The indexed lookup (#1063): areas tile the zone, so a player stands in one.
            if let Some(area) = content.areas().area_at(at.x, at.z) {
                if content.discoveries().binary_search(&area.id()).is_err() {
                    continue;
                }
                let key = crate::RewardKey::Discovery { area: area.id() };
                let Some(player) = self.players.get_mut(&player_id) else {
                    continue;
                };
                // A tick without room for the receipt leaves the area undiscovered; the
                // player discovers it in a later tick it still stands there (#1112).
                if player.discoveries.contains(&area.id())
                    || !crate::rewards::allowed(&self.rewards, player_id, key)
                    || !crate::rewards::fits(&self.rewards, 1)
                {
                    continue;
                }
                player.discoveries.insert(area.id());
                crate::rewards::record(&mut self.rewards, player_id, key)?;
                let reward = discovery_experience(player.level)
                    .ok_or_else(|| crate::ZoneError::new("invalid progression level"))?;
                self.grant_experience(player_id, reward)?;
            }
        }
        Ok(())
    }
}
