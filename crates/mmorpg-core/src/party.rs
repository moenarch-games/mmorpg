//! Party membership as a zone consumes it (#404, ADR 0008).
//!
//! The party directory (`mmorpg-control-plane`) owns membership; a zone holds only the
//! versioned view of each of its players, set by trusted host inputs so replay and recovery
//! see it in order with commands. A zone never writes membership: it reads the view to share
//! kill rewards (Atlas 5.3.6, 5.4.5) and to project the viewer's party frame.

use std::collections::BTreeMap;

use crate::{PlayerId, ZoneError, ZoneSimulation};

/// A party holds at most five characters, the leader included (Atlas 5.4.7).
pub const MAX_PARTY_MEMBERS: usize = 5;
/// The longest member or inviter name in UTF-8 bytes.
pub const MAX_PARTY_NAME_BYTES: usize = 48;

/// One member of a party, as its zone sees it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PartyMember {
    pub name: String,
    /// The member's player in this zone; `None` when elsewhere or offline.
    pub player: Option<PlayerId>,
    pub online: bool,
}

/// A party a player belongs to. `members` is in join order, so the leader (always the
/// longest-standing member, as the directory promotes) comes first.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PartyRoster {
    pub party: u64,
    /// Advances on every membership change; kicking names the version it was decided on.
    pub version: u32,
    pub members: Vec<PartyMember>,
}

/// A pending invitation to the player.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PartyInvite {
    pub invite: u64,
    pub from: String,
}

/// A player's party standing: the view a zone holds and projects.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PartyState {
    /// Issued by the directory, increasing per character across zones; `0` is "never set".
    pub revision: u64,
    pub roster: Option<PartyRoster>,
    pub invite: Option<PartyInvite>,
}

impl PartyState {
    /// Fails closed on a view no directory should produce.
    pub fn validate(&self) -> Result<(), ZoneError> {
        let invalid = |reason: &str| Err(ZoneError::new(format!("party view: {reason}")));
        if self.revision == 0 {
            return invalid("the revision must be nonzero");
        }
        if self.roster.is_some() && self.invite.is_some() {
            return invalid("a party member has no pending invite");
        }
        if let Some(roster) = &self.roster {
            if !(2..=MAX_PARTY_MEMBERS).contains(&roster.members.len()) {
                return invalid("a party has two to five members");
            }
            let mut players = std::collections::BTreeSet::new();
            for member in &roster.members {
                if !valid_name(&member.name) {
                    return invalid("invalid member name");
                }
                if member.player.is_some_and(|player| !players.insert(player)) {
                    return invalid("a player appears twice");
                }
            }
        }
        if let Some(invite) = &self.invite
            && (invite.invite == 0 || !valid_name(&invite.from))
        {
            return invalid("invalid invite");
        }
        Ok(())
    }
}

/// Validated name text: nonempty, bounded and free of control characters.
#[must_use]
pub fn valid_name(name: &str) -> bool {
    !name.trim().is_empty()
        && name.len() <= MAX_PARTY_NAME_BYTES
        && name.chars().all(|character| !character.is_control())
}

/// Why the party directory refused an operation; the wire codes are stable.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum PartyRefusal {
    /// No such online character, or that name fits nobody.
    NotFound,
    /// Several online characters use that name.
    AmbiguousName,
    AlreadyInParty,
    /// You may not invite yourself.
    SelfInvite,
    /// That character already has a pending invite.
    InviteBusy,
    NotLeader,
    PartyFull,
    /// The invite expired, was answered or was withdrawn.
    InviteGone,
    NotInParty,
    /// The party changed since the action was decided.
    StaleParty,
    /// The character is not bound to this zone, or the zone's lease is no longer current.
    Unavailable,
}

impl PartyRefusal {
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::NotFound => 1,
            Self::AmbiguousName => 2,
            Self::AlreadyInParty => 3,
            Self::SelfInvite => 4,
            Self::InviteBusy => 5,
            Self::NotLeader => 6,
            Self::PartyFull => 7,
            Self::InviteGone => 8,
            Self::NotInParty => 9,
            Self::StaleParty => 10,
            Self::Unavailable => 11,
        }
    }

    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        Some(match code {
            1 => Self::NotFound,
            2 => Self::AmbiguousName,
            3 => Self::AlreadyInParty,
            4 => Self::SelfInvite,
            5 => Self::InviteBusy,
            6 => Self::NotLeader,
            7 => Self::PartyFull,
            8 => Self::InviteGone,
            9 => Self::NotInParty,
            10 => Self::StaleParty,
            11 => Self::Unavailable,
            _ => return None,
        })
    }

    /// The sentence shown to the player.
    #[must_use]
    pub const fn message(self) -> &'static str {
        match self {
            Self::NotFound => "No player by that name is online.",
            Self::AmbiguousName => "Several players use that name; select the one you mean.",
            Self::AlreadyInParty => "That player is already in a party.",
            Self::SelfInvite => "You cannot invite yourself.",
            Self::InviteBusy => "That player already has a pending invite.",
            Self::NotLeader => "Only the party leader can do that.",
            Self::PartyFull => "The party is full.",
            Self::InviteGone => "That invite is no longer open.",
            Self::NotInParty => "You are not in a party.",
            Self::StaleParty => "The party changed; try again.",
            Self::Unavailable => "Parties are not available here right now.",
        }
    }
}

/// The party views of a zone's players.
pub(crate) type Parties = BTreeMap<PlayerId, PartyState>;

impl ZoneSimulation {
    /// Applies a directory view to `player_id`. An unknown player or a malformed view is an
    /// error; a view at or below the held revision is stale and ignored (`Ok(false)`).
    pub fn apply_party_state(
        &mut self,
        player_id: PlayerId,
        state: PartyState,
    ) -> Result<bool, ZoneError> {
        if !self.players.contains_key(&player_id) {
            return Err(ZoneError::new("unknown player"));
        }
        state.validate()?;
        if self.party_revision(player_id) >= state.revision {
            return Ok(false);
        }
        self.parties.insert(player_id, state);
        Ok(true)
    }

    /// The directory revision of the view `player_id` holds; `0` for none.
    #[must_use]
    pub fn party_revision(&self, player_id: PlayerId) -> u64 {
        self.parties
            .get(&player_id)
            .map_or(0, |state| state.revision)
    }

    /// The zone's player IDs, ascending.
    pub fn player_ids(&self) -> impl Iterator<Item = PlayerId> + '_ {
        self.players.keys().copied()
    }

    #[must_use]
    pub fn party_state(&self, player_id: PlayerId) -> Option<&PartyState> {
        self.parties.get(&player_id)
    }

    fn roster_of(&self, player_id: PlayerId) -> Option<&PartyRoster> {
        self.parties.get(&player_id)?.roster.as_ref()
    }

    /// Whether two distinct players each hold a view of the same party.
    pub(crate) fn share_party(&self, left: PlayerId, right: PlayerId) -> bool {
        left != right
            && self
                .roster_of(left)
                .zip(self.roster_of(right))
                .is_some_and(|(left, right)| left.party == right.party)
    }

    /// `player_id`'s party members in this zone, the player excluded, in ascending ID order.
    /// Membership takes both views' word, so a view not yet updated cannot share a reward
    /// with someone who has left.
    pub(crate) fn party_mates(&self, player_id: PlayerId) -> Vec<PlayerId> {
        let Some(roster) = self.roster_of(player_id) else {
            return Vec::new();
        };
        let mut mates: Vec<_> = roster
            .members
            .iter()
            .filter_map(|member| member.player)
            .filter(|mate| self.share_party(player_id, *mate) && self.players.contains_key(mate))
            .collect();
        mates.sort_unstable();
        mates
    }
}
