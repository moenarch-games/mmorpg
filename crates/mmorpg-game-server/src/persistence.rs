//! Production persistence composition (TECH-003/004/005). Storage never owns game rules.
#[cfg(feature = "postgres")]
pub(crate) mod postgres;

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
pub(crate) struct CharacterWrite {
    pub lease: CharacterLease,
    pub expected_revision: u64,
    pub record: CharacterRecord,
    pub grants: BTreeSet<GrantId>,
}
