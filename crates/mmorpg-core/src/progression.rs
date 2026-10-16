//! Immutable starter progression rules. Award eligibility and mutable XP
//! belong to the authoritative zone; these queries never grant experience.

use crate::unit::MAX_UNIT_LEVEL;

/// Starter progression ends at level 10; creatures retain their own level bound.
pub const MAX_PLAYER_LEVEL: u8 = 10;
const NEXT_LEVEL_EXPERIENCE: [u32; 10] = [100, 200, 300, 400, 500, 600, 700, 800, 900, 0];

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
        let dx = i128::from(player_position.x) - i128::from(position[0]);
        let dz = i128::from(player_position.z) - i128::from(position[2]);
        let radius = i128::from(crate::INTEREST_RADIUS_UNITS);
        Ok(dx * dx + dz * dz <= radius * radius)
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
        player.health += crate::unit::player_max_health(player.level)
            - crate::unit::player_max_health(old_level);
        if let Some(choice) = player.class {
            let kind = choice.class.resource();
            player.resource.value += kind.max(player.level) - kind.max(old_level);
        }
        Ok(())
    }
}
