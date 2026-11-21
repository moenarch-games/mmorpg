//! Weapon-family mastery (#915, Atlas 5.2.i915): practice with a weapon family in real
//! fights grows a durable proficiency that unlocks techniques, not raw damage. The sword is
//! the first family.
//!
//! - **Practice.** A sword blow earns practice only when it deals positive health damage to a
//!   living creature that is not a prop, a boulder or trivial (five or more levels below the
//!   striker: the kill would pay no experience, [`crate::kill_experience`]). A basic strike
//!   earns [`BLOW_PRACTICE`], a landed Heroic Strike, the family's technique,
//!   [`TECHNIQUE_PRACTICE`]. Misses, whiffs, evaded, absorbed and blocked blows, players and
//!   props earn nothing.
//! - **Anti-farm.** Practice is banked per foe life, at most [`PRACTICE_PER_FOE`], and becomes
//!   mastery only when that foe dies while the striker is alive and in kill-credit range
//!   ([`ZoneSimulation::kill_credit_eligible`]). A foe that is never killed, or a leash reset,
//!   pays nothing; a player banks at most [`MAX_PRACTICE`] foes at once, the oldest dropped.
//! - **Mastery.** Banked practice fills the next level's [`practice_to_next_level`]; the levels
//!   end at [`MAX_MASTERY_LEVEL`]. Level [`LEAPING_SLASH_LEVEL`] unlocks the action graph's
//!   `LeapingSlash` edge ([`crate::action::Condition::SwordMastery`]): a Warden's jumping
//!   strike lands into an immediate Heroic Strike. The basic strikes are whole from the first
//!   equip; mastery adds no damage, so classes, resources and future talents compose with it.
//!
//! Mastery is durable (the character record) and canonical; the banked practice is canonical
//! session state, so replay and recovery continue it exactly.

use crate::{CreatureId, EntityRef, PlayerId, WeaponFamily, ZoneError, ZoneSimulation};

/// Practice a landed basic sword strike earns.
pub const BLOW_PRACTICE: u8 = 1;
/// Practice a landed Heroic Strike earns: the family's technique, worth practising.
pub const TECHNIQUE_PRACTICE: u8 = 2;
/// The most practice one foe's life can bank.
pub const PRACTICE_PER_FOE: u8 = 6;
/// The most foes a player banks practice against at once.
pub const MAX_PRACTICE: usize = 4;
/// A creature this many or more levels below the striker is trivial.
pub const TRIVIAL_LEVELS_BELOW: u8 = 5;
/// The highest mastery level.
pub const MAX_MASTERY_LEVEL: u8 = 3;
/// The mastery level that unlocks the Leaping Slash.
pub const LEAPING_SLASH_LEVEL: u8 = 1;
/// Practice from each level to the next; the last level needs none.
const NEXT_LEVEL_PRACTICE: [u16; MAX_MASTERY_LEVEL as usize + 1] = [30, 80, 150, 0];

/// Practice needed from `level` to the next: zero at [`MAX_MASTERY_LEVEL`], `None` beyond.
#[must_use]
pub fn practice_to_next_level(level: u8) -> Option<u16> {
    NEXT_LEVEL_PRACTICE.get(usize::from(level)).copied()
}

/// One family's mastery: its level and the practice toward the next.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct Mastery {
    pub level: u8,
    pub progress: u16,
}

impl Mastery {
    /// Whether the level exists and the progress stays below its next level.
    #[must_use]
    pub fn is_valid(self) -> bool {
        practice_to_next_level(self.level).is_some_and(|next| {
            if next == 0 {
                self.progress == 0
            } else {
                self.progress < next
            }
        })
    }

    /// Adds `practice`, levelling up as often as it fills; at the top level it is spent.
    fn gain(&mut self, practice: u16) {
        let mut left = practice;
        while let Some(next) = practice_to_next_level(self.level).filter(|&next| next > 0) {
            let room = next - self.progress;
            if left < room {
                self.progress += left;
                return;
            }
            left -= room;
            self.level += 1;
            self.progress = 0;
        }
    }
}

/// A character's weapon-family masteries: the sword's for now.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct WeaponMastery {
    pub sword: Mastery,
}

impl WeaponMastery {
    #[must_use]
    pub fn is_valid(self) -> bool {
        self.sword.is_valid()
    }

    /// The mastery of `family`, if it has one.
    #[must_use]
    pub const fn of(self, family: WeaponFamily) -> Option<Mastery> {
        match family {
            WeaponFamily::Sword => Some(self.sword),
            _ => None,
        }
    }
}

/// Practice banked against one foe's life, paid when it dies.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Practice {
    pub creature: CreatureId,
    /// The creature's life ([`crate::RewardKey::creature_life`]'s generation).
    pub life: u64,
    /// 1 through [`PRACTICE_PER_FOE`].
    pub points: u8,
}

/// Whether `practice` fits a player: at most [`MAX_PRACTICE`] foes, each once and in range.
#[must_use]
pub fn valid_practice(practice: &[Practice]) -> bool {
    practice.len() <= MAX_PRACTICE
        && practice.iter().enumerate().all(|(index, entry)| {
            (1..=PRACTICE_PER_FOE).contains(&entry.points)
                && entry.life > 0
                && practice[..index]
                    .iter()
                    .all(|other| (other.creature, other.life) != (entry.creature, entry.life))
        })
}

impl ZoneSimulation {
    /// A sword blow of the player against `target`, which had `before` health, banks
    /// `points` of practice if it was eligible (see the module docs).
    pub(crate) fn practise(
        &mut self,
        player_id: PlayerId,
        family: WeaponFamily,
        target: EntityRef,
        before: u32,
        points: u8,
    ) {
        if family != WeaponFamily::Sword {
            return;
        }
        let EntityRef::Creature(creature_id) = target else {
            return;
        };
        let Some(creature) = self.creatures.get(&creature_id) else {
            return;
        };
        let Some(player) = self.players.get(&player_id) else {
            return;
        };
        // The blow must have taken health; a killing blow leaves a corpse of the same life.
        let landed = creature.health < before
            && matches!(
                creature.life,
                crate::creature::Life::Alive | crate::creature::Life::Corpse { .. }
            );
        let trivial = creature.level.saturating_add(TRIVIAL_LEVELS_BELOW) <= player.level;
        if !landed
            || trivial
            || !player.is_alive()
            || self.is_prop(target)
            || self.is_boulder(creature_id)
        {
            return;
        }
        let life = creature.generation;
        self.update_player(player_id, |player| {
            match player
                .practice
                .iter_mut()
                .find(|entry| (entry.creature, entry.life) == (creature_id, life))
            {
                Some(entry) => {
                    entry.points = entry.points.saturating_add(points).min(PRACTICE_PER_FOE);
                }
                None => {
                    if player.practice.len() >= MAX_PRACTICE {
                        player.practice.remove(0);
                    }
                    player.practice.push(Practice {
                        creature: creature_id,
                        life,
                        points: points.min(PRACTICE_PER_FOE),
                    });
                }
            }
        });
    }

    /// Pays every banked practice whose foe has died (to a striker alive and in kill-credit
    /// range), and drops practice against a life that is over without a kill or long gone.
    pub(crate) fn settle_practice(&mut self) -> Result<(), ZoneError> {
        let player_ids: Vec<PlayerId> = self
            .players
            .iter()
            .filter(|(_, player)| !player.practice.is_empty())
            .map(|(&id, _)| id)
            .collect();
        for player_id in player_ids {
            let Some(practice) = self.players.get(&player_id).map(|p| p.practice.clone()) else {
                continue;
            };
            let mut kept = Vec::with_capacity(practice.len());
            let mut earned = 0_u16;
            for entry in practice {
                let Some(creature) = self.creatures.get(&entry.creature) else {
                    continue;
                };
                if creature.generation != entry.life {
                    continue;
                }
                match creature.life {
                    crate::creature::Life::Alive => kept.push(entry),
                    crate::creature::Life::Corpse { position, .. } => {
                        if self.kill_credit_eligible(player_id, position)? {
                            earned += u16::from(entry.points);
                        }
                    }
                    crate::creature::Life::Despawned { .. } => {}
                }
            }
            self.update_player(player_id, |player| {
                player.practice = kept;
                player.mastery.sword.gain(earned);
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn practice_levels_up_and_stops_at_the_top() {
        let mut mastery = Mastery::default();
        mastery.gain(29);
        assert_eq!(
            mastery,
            Mastery {
                level: 0,
                progress: 29
            }
        );
        mastery.gain(3);
        assert_eq!(
            mastery,
            Mastery {
                level: 1,
                progress: 2
            }
        );
        mastery.gain(1_000);
        assert_eq!(
            mastery,
            Mastery {
                level: MAX_MASTERY_LEVEL,
                progress: 0
            }
        );
        assert!(mastery.is_valid());
        assert!(
            !Mastery {
                level: 0,
                progress: 30
            }
            .is_valid()
        );
        assert!(
            !Mastery {
                level: 4,
                progress: 0
            }
            .is_valid()
        );
    }
}
