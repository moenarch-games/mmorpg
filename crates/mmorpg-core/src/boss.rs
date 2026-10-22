//! Shared outdoor boss encounters (#276, Atlas 5.1.72–5.1.80): Garrick Redbrand.
//!
//! - **Scaling** (5.1.76): the first engagement of a resting boss snapshots the players actually
//!   fighting it (the puller and anyone already in a threat table of the boss's family nearby)
//!   and locks a health/damage profile for the whole attempt. Joins, leaves and deaths never
//!   rescale it.
//! - **Contribution** (5.1.74): damage to the boss or healing a player it threatens makes a
//!   player a contributor. Every contributor with the kill quest active earns the kill
//!   objective, across parties; kill XP and loot stay with the first tap (5.3.6).
//! - **Reset** (5.1.77–5.1.78): the ordinary evade clears the attempt; the boss is immune while
//!   it runs home and only rests at full, unscaled health.
//! - **Camp reaction** (5.1.79): an actual death sends idle family members nearby toward an
//!   authored rally point for a while; then they walk home and resume.
//! - **Respawn** (5.1.75): the corpse stays at the camp until the short respawn, so a defeated
//!   boss is visible to anyone who arrives.

use crate::{CreatureTemplateId, PlayerId};

/// At most this many participants scale a boss.
pub const MAX_SCALING_PARTICIPANTS: usize = 5;
/// At most this many players are remembered per attempt (participants or contributors).
pub const MAX_ENCOUNTER_PLAYERS: usize = 64;

/// Content rules for one boss template.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BossRules {
    pub template: CreatureTemplateId,
    /// Extra maximum health per participant beyond the first, in percent.
    pub health_percent_per_extra: u16,
    /// Extra damage per participant beyond the first, in percent.
    pub damage_percent_per_extra: u16,
    /// Players already fighting the boss's family within this XZ radius join the snapshot.
    pub engage_radius: i32,
    /// Idle family members within this XZ radius of the death react.
    pub reaction_radius: i32,
    /// How long they hold the rally before walking home.
    pub reaction_ticks: u16,
    /// Where they rally, as XZ units.
    pub rally_point: [i32; 2],
}

impl BossRules {
    /// Health or damage percent for `participants` locked players.
    #[must_use]
    pub fn percent(participants: usize, per_extra: u16) -> u32 {
        let extra = participants.clamp(1, MAX_SCALING_PARTICIPANTS) - 1;
        100 + u32::from(per_extra) * extra as u32
    }
}

/// The attempt-scoped state of a fighting boss.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BossEncounter {
    /// Locked at combat start; sorted, unique.
    pub participants: Vec<PlayerId>,
    /// Players who meaningfully contributed; sorted, unique.
    pub contributors: Vec<PlayerId>,
}

impl BossEncounter {
    pub(crate) fn contribute(&mut self, player: PlayerId) {
        if let Err(index) = self.contributors.binary_search(&player)
            && self.contributors.len() < MAX_ENCOUNTER_PLAYERS
        {
            self.contributors.insert(index, player);
        }
    }

    /// Maximum health of a boss with `base` unscaled health under this attempt's profile.
    pub(crate) fn scale_health(&self, rules: &BossRules, base: u32) -> u32 {
        scale(
            base,
            BossRules::percent(self.participants.len(), rules.health_percent_per_extra),
        )
    }

    pub(crate) fn scale_damage(&self, rules: &BossRules, amount: u32) -> u32 {
        scale(
            amount,
            BossRules::percent(self.participants.len(), rules.damage_percent_per_extra),
        )
    }
}

fn scale(value: u32, percent: u32) -> u32 {
    u32::try_from(u64::from(value) * u64::from(percent) / 100).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaling_grows_per_extra_participant_and_caps_at_five() {
        assert_eq!(BossRules::percent(0, 60), 100);
        assert_eq!(BossRules::percent(1, 60), 100);
        assert_eq!(BossRules::percent(2, 60), 160);
        assert_eq!(BossRules::percent(5, 60), 340);
        assert_eq!(BossRules::percent(9, 60), 340);
    }
}
