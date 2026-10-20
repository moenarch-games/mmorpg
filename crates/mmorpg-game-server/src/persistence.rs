//! Production persistence composition (TECH-003/004/005). Storage never owns game rules.
mod checkpoints;
mod hosted;
#[cfg(feature = "postgres")]
pub(crate) mod postgres;
mod replication;

use mmorpg_characters::CharacterLease;
use mmorpg_core::CharacterRecord;
use std::{collections::BTreeSet, error::Error, fmt};

/// Stable entitlement identity, authored from durable world/character identities.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct GrantId(String);
impl GrantId {
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidGrantId> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 256
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_:./".contains(&byte))
        {
            return Err(InvalidGrantId);
        }
        Ok(Self(value))
    }
    pub(crate) fn for_reward(
        world: &str,
        zone: mmorpg_core::ZoneId,
        character: &mmorpg_core::CharacterId,
        key: mmorpg_core::RewardKey,
    ) -> Result<Self, InvalidGrantId> {
        use mmorpg_core::RewardKey;
        let suffix = match key {
            RewardKey::KillExperience { creature, life } => {
                format!("creature/{}/{life}/experience", creature.get())
            }
            RewardKey::KillObjective {
                creature,
                life,
                quest,
                objective,
            } => format!(
                "creature/{}/{life}/quest/{}/{objective}",
                creature.get(),
                quest.get()
            ),
            RewardKey::LootMoney { creature, life } => {
                format!("creature/{}/{life}/money", creature.get())
            }
            RewardKey::LootItem { creature, life } => {
                format!("creature/{}/{life}/item", creature.get())
            }
            RewardKey::LootQuestItem {
                creature,
                life,
                item,
            } => format!(
                "creature/{}/{life}/quest-item/{}",
                creature.get(),
                item.get()
            ),
            RewardKey::QuestCompletion { quest } => {
                return Self::new(format!(
                    "character/{}/quest/{}/complete",
                    character.as_str(),
                    quest.get()
                ));
            }
        };
        if key.world_owned() {
            Self::new(format!("world/{world}/zone/{}/{suffix}", zone.get()))
        } else {
            Self::new(format!(
                "character/{}/world/{world}/zone/{}/{suffix}",
                character.as_str(),
                zone.get()
            ))
        }
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidGrantId;
impl fmt::Display for InvalidGrantId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("invalid durable grant identity")
    }
}
impl Error for InvalidGrantId {}

/// Already-authorized core facts. No reward calculation occurs in storage.
#[derive(Clone)]
pub(crate) struct CharacterWrite {
    pub lease: CharacterLease,
    pub expected_revision: u64,
    pub record: CharacterRecord,
    pub grants: BTreeSet<GrantId>,
    pub release: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmorpg_core::{CharacterId, CreatureId, QuestId, RewardKey, ZoneId};
    #[test]
    fn grant_identity_separates_world_entitlements_and_personal_progression() {
        let first = CharacterId::new("first").unwrap();
        let second = CharacterId::new("second").unwrap();
        let creature = CreatureId::new(1);
        let money = RewardKey::LootMoney { creature, life: 2 };
        let xp = RewardKey::KillExperience { creature, life: 2 };
        let zone = ZoneId::new(1);
        assert_eq!(
            GrantId::for_reward("world-a", zone, &first, money).unwrap(),
            GrantId::for_reward("world-a", zone, &second, money).unwrap()
        );
        assert_ne!(
            GrantId::for_reward("world-a", zone, &first, xp).unwrap(),
            GrantId::for_reward("world-a", zone, &second, xp).unwrap()
        );
        let quest = RewardKey::QuestCompletion {
            quest: QuestId::new(1),
        };
        assert_eq!(
            GrantId::for_reward("world-a", zone, &first, quest).unwrap(),
            GrantId::for_reward("world-b", ZoneId::new(2), &first, quest).unwrap()
        );
    }
}
