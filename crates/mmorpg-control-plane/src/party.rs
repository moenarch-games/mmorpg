//! Party membership directory (#404, ADR 0008): the one owner of who belongs to which party.
//!
//! Membership is cross-zone, so it lives beside the zone directory rather than in any zone.
//! The directory never touches gameplay: zones consume a versioned [`PartyState`] view per
//! character, through trusted host inputs the party service submits, and keep every kill,
//! reward and loot decision. Each operation is fenced by the issuing zone's lease
//! ([`PartyAuthority::transact`]) and refusals leave the directory untouched: a duplicate
//! or stale operation fails closed.
//!
//! Members are [`CharacterId`]s, independent of sessions and zones, so a handoff changes only
//! where a member is shown as present, never its membership. The party keeps join order,
//! so its first member is the leader and the longest-standing member succeeds a leader who
//! leaves.

use std::collections::BTreeMap;
use std::fmt;

use mmorpg_core::{
    CharacterId, MAX_PARTY_MEMBERS, PartyInvite, PartyMember, PartyRefusal, PartyRoster,
    PartyState, PlayerId, ZoneId,
};

use crate::{ControlPlaneError, ZoneDirectory, ZoneLease};

/// An unanswered invite lapses after one minute.
pub const PARTY_INVITE_TTL_MS: u64 = 60_000;
/// A member offline for this long has left the party (a brief gap such as a handoff never
/// reaches it).
pub const PARTY_OFFLINE_EXPIRY_MS: u64 = 300_000;

#[derive(Clone, Debug, Eq, PartialEq)]
struct Party {
    id: u64,
    version: u32,
    /// Join order; the first member leads.
    members: Vec<CharacterId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Invite {
    id: u64,
    inviter: CharacterId,
    invitee: CharacterId,
    expires_at_ms: u64,
}

/// Where a character is in play, as its zone last reported.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Presence {
    zone: ZoneId,
    player: PlayerId,
    name: String,
    offline_since_ms: Option<u64>,
}

/// One character currently bound to a zone, as that zone reports it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PartyRosterEntry {
    pub character: CharacterId,
    pub player: PlayerId,
    pub name: String,
}

/// Whom an invite addresses.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InviteTarget {
    Character(CharacterId),
    Name(String),
}

/// The authoritative party state: no gameplay, only membership and where members are.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PartyDirectory {
    next_id: u64,
    next_revision: u64,
    parties: BTreeMap<u64, Party>,
    membership: BTreeMap<CharacterId, u64>,
    invites: BTreeMap<u64, Invite>,
    presence: BTreeMap<CharacterId, Presence>,
    /// The content and revision of each character's last issued view.
    views: BTreeMap<CharacterId, (u64, PartyState)>,
}

impl PartyDirectory {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn party_of(&self, character: &CharacterId) -> Option<u64> {
        self.membership.get(character).copied()
    }

    fn online(&self, character: &CharacterId) -> bool {
        self.presence
            .get(character)
            .is_some_and(|presence| presence.offline_since_ms.is_none())
    }

    fn pending_invite_for(&self, character: &CharacterId) -> Option<u64> {
        self.invites
            .values()
            .find(|invite| invite.invitee == *character)
            .map(|invite| invite.id)
    }

    fn resolve(&self, target: &InviteTarget) -> Result<CharacterId, PartyRefusal> {
        match target {
            InviteTarget::Character(character) => Ok(character.clone()),
            InviteTarget::Name(name) => {
                let wanted = name.trim().to_lowercase();
                let mut found = self
                    .presence
                    .iter()
                    .filter(|(_, presence)| {
                        presence.offline_since_ms.is_none()
                            && presence.name.to_lowercase() == wanted
                    })
                    .map(|(character, _)| character);
                match (found.next(), found.next()) {
                    (Some(character), None) => Ok(character.clone()),
                    (Some(_), Some(_)) => Err(PartyRefusal::AmbiguousName),
                    (None, _) => Err(PartyRefusal::NotFound),
                }
            }
        }
    }

    /// Records that `actor` invites `target` to its party, or to form one.
    pub fn invite(
        &mut self,
        actor: &CharacterId,
        target: &InviteTarget,
        now_ms: u64,
    ) -> Result<u64, PartyRefusal> {
        if !self.online(actor) {
            return Err(PartyRefusal::Unavailable);
        }
        let target = self.resolve(target)?;
        if target == *actor {
            return Err(PartyRefusal::SelfInvite);
        }
        if !self.online(&target) {
            return Err(PartyRefusal::NotFound);
        }
        if self.party_of(&target).is_some() {
            return Err(PartyRefusal::AlreadyInParty);
        }
        if self.pending_invite_for(&target).is_some() {
            return Err(PartyRefusal::InviteBusy);
        }
        if let Some(party) = self.party_of(actor).and_then(|id| self.parties.get(&id)) {
            if party.members.first() != Some(actor) {
                return Err(PartyRefusal::NotLeader);
            }
            let pending = self
                .invites
                .values()
                .filter(|invite| invite.inviter == *actor)
                .count();
            if party.members.len() + pending >= MAX_PARTY_MEMBERS {
                return Err(PartyRefusal::PartyFull);
            }
        }
        let id = self.fresh_id();
        self.invites.insert(
            id,
            Invite {
                id,
                inviter: actor.clone(),
                invitee: target,
                expires_at_ms: now_ms.saturating_add(PARTY_INVITE_TTL_MS),
            },
        );
        Ok(id)
    }

    fn fresh_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn drop_invites_of(&mut self, party: Option<u64>, character: &CharacterId) {
        self.invites.retain(|_, invite| {
            invite.invitee != *character && (invite.inviter != *character || party.is_none())
        });
    }

    /// `invitee` answers yes: it joins, the party forming around a solo inviter. A duplicate
    /// or lapsed answer is refused as gone.
    pub fn accept(
        &mut self,
        invitee: &CharacterId,
        invite_id: u64,
        now_ms: u64,
    ) -> Result<(), PartyRefusal> {
        let invite = self
            .invites
            .get(&invite_id)
            .filter(|invite| invite.invitee == *invitee && invite.expires_at_ms > now_ms)
            .ok_or(PartyRefusal::InviteGone)?
            .clone();
        if self.party_of(invitee).is_some() {
            return Err(PartyRefusal::AlreadyInParty);
        }
        match self.party_of(&invite.inviter) {
            Some(id) => {
                let party = self.parties.get_mut(&id).ok_or(PartyRefusal::InviteGone)?;
                if party.members.first() != Some(&invite.inviter) {
                    return Err(PartyRefusal::InviteGone);
                }
                if party.members.len() >= MAX_PARTY_MEMBERS {
                    return Err(PartyRefusal::PartyFull);
                }
                party.members.push(invitee.clone());
                party.version = party.version.wrapping_add(1);
                self.membership.insert(invitee.clone(), id);
                self.drop_invites_of(None, invitee);
            }
            None => {
                let id = self.fresh_id();
                self.parties.insert(
                    id,
                    Party {
                        id,
                        version: 1,
                        members: vec![invite.inviter.clone(), invitee.clone()],
                    },
                );
                self.membership.insert(invite.inviter.clone(), id);
                self.membership.insert(invitee.clone(), id);
                self.drop_invites_of(None, invitee);
                self.drop_invites_of(None, &invite.inviter);
            }
        }
        self.invites.remove(&invite_id);
        Ok(())
    }

    /// `invitee` answers no.
    pub fn decline(&mut self, invitee: &CharacterId, invite_id: u64) -> Result<(), PartyRefusal> {
        if self
            .invites
            .get(&invite_id)
            .is_none_or(|invite| invite.invitee != *invitee)
        {
            return Err(PartyRefusal::InviteGone);
        }
        self.invites.remove(&invite_id);
        Ok(())
    }

    /// `actor` leaves. A leader's place passes to the longest-standing member; a party left
    /// with one member dissolves.
    pub fn leave(&mut self, actor: &CharacterId) -> Result<(), PartyRefusal> {
        let id = self.party_of(actor).ok_or(PartyRefusal::NotInParty)?;
        self.remove_member(id, actor)
    }

    /// The leader removes the member at roster index `member`, decided on `version`.
    pub fn kick(
        &mut self,
        actor: &CharacterId,
        member: u8,
        version: u32,
    ) -> Result<(), PartyRefusal> {
        let id = self.party_of(actor).ok_or(PartyRefusal::NotInParty)?;
        let party = self.parties.get(&id).ok_or(PartyRefusal::NotInParty)?;
        if party.version != version {
            return Err(PartyRefusal::StaleParty);
        }
        if party.members.first() != Some(actor) {
            return Err(PartyRefusal::NotLeader);
        }
        let target = party
            .members
            .get(usize::from(member))
            .filter(|target| *target != actor)
            .ok_or(PartyRefusal::StaleParty)?
            .clone();
        self.remove_member(id, &target)
    }

    fn remove_member(&mut self, id: u64, character: &CharacterId) -> Result<(), PartyRefusal> {
        let party = self.parties.get_mut(&id).ok_or(PartyRefusal::NotInParty)?;
        party.members.retain(|member| member != character);
        party.version = party.version.wrapping_add(1);
        self.membership.remove(character);
        self.drop_invites_of(Some(id), character);
        if self
            .parties
            .get(&id)
            .is_some_and(|party| party.members.len() < 2)
            && let Some(party) = self.parties.remove(&id)
        {
            for member in party.members {
                self.membership.remove(&member);
            }
        }
        Ok(())
    }

    /// Records which characters a zone holds: listed ones are present there, others last
    /// present there go offline. Levels out handoffs and logouts without any hook in them.
    pub fn sync_zone(&mut self, zone: ZoneId, roster: &[PartyRosterEntry], now_ms: u64) {
        for presence in self.presence.values_mut() {
            if presence.zone == zone && presence.offline_since_ms.is_none() {
                presence.offline_since_ms = Some(now_ms);
            }
        }
        for entry in roster {
            self.presence.insert(
                entry.character.clone(),
                Presence {
                    zone,
                    player: entry.player,
                    name: entry.name.clone(),
                    offline_since_ms: None,
                },
            );
        }
    }

    /// Lapses expired invites and members offline past [`PARTY_OFFLINE_EXPIRY_MS`], and forgets
    /// offline characters no party needs.
    pub fn sweep(&mut self, now_ms: u64) {
        self.invites
            .retain(|_, invite| invite.expires_at_ms > now_ms);
        let expired: Vec<_> = self
            .parties
            .values()
            .flat_map(|party| {
                party
                    .members
                    .iter()
                    .map(|member| (party.id, member.clone()))
            })
            .filter(|(_, member)| {
                self.presence
                    .get(member)
                    .and_then(|presence| presence.offline_since_ms)
                    .is_some_and(|since| now_ms.saturating_sub(since) >= PARTY_OFFLINE_EXPIRY_MS)
            })
            .collect();
        for (id, member) in expired {
            // Removal only fails for a party already gone, which needs nothing.
            let _ = self.remove_member(id, &member);
        }
        let membership = &self.membership;
        self.presence.retain(|character, presence| {
            presence.offline_since_ms.is_none() || membership.contains_key(character)
        });
    }

    /// The view of `character` as of now, its revision advanced when its content changed and
    /// always above `held`, the revision its zone holds.
    pub fn view(&mut self, character: &CharacterId, held: u64) -> PartyState {
        let mut state = self.compute_view(character);
        match self.views.get(character) {
            Some((revision, previous)) if *previous == state && *revision >= held => {
                state.revision = *revision;
            }
            None if state.roster.is_none() && state.invite.is_none() && held == 0 => {}
            _ => {
                self.next_revision = self.next_revision.max(held) + 1;
                self.views
                    .insert(character.clone(), (self.next_revision, state.clone()));
                state.revision = self.next_revision;
            }
        }
        state
    }

    fn compute_view(&self, character: &CharacterId) -> PartyState {
        let viewer_zone = self
            .presence
            .get(character)
            .and_then(|presence| presence.offline_since_ms.is_none().then_some(presence.zone));
        let roster = self
            .party_of(character)
            .and_then(|id| self.parties.get(&id))
            .map(|party| {
                let members = party
                    .members
                    .iter()
                    .map(|member| {
                        let presence = self.presence.get(member);
                        let online =
                            presence.is_some_and(|presence| presence.offline_since_ms.is_none());
                        PartyMember {
                            name: presence.map_or_else(
                                || "Unknown".to_owned(),
                                |presence| presence.name.clone(),
                            ),
                            player: presence
                                .filter(|presence| online && Some(presence.zone) == viewer_zone)
                                .map(|presence| presence.player),
                            online,
                        }
                    })
                    .collect();
                PartyRoster {
                    party: party.id,
                    version: party.version,
                    members,
                }
            });
        let invite = roster
            .is_none()
            .then(|| self.pending_invite_for(character))
            .flatten()
            .and_then(|id| {
                let invite = self.invites.get(&id)?;
                let from = self.presence.get(&invite.inviter)?.name.clone();
                Some(PartyInvite { invite: id, from })
            });
        PartyState {
            revision: 0,
            roster,
            invite,
        }
    }

    /// The directory as stored by the shared store.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.next_id.to_be_bytes());
        out.extend_from_slice(&self.next_revision.to_be_bytes());
        out.extend_from_slice(&(self.parties.len() as u32).to_be_bytes());
        for party in self.parties.values() {
            out.extend_from_slice(&party.id.to_be_bytes());
            out.extend_from_slice(&party.version.to_be_bytes());
            out.push(party.members.len() as u8);
            for member in &party.members {
                encode_text(&mut out, member.as_str());
            }
        }
        out.extend_from_slice(&(self.invites.len() as u32).to_be_bytes());
        for invite in self.invites.values() {
            out.extend_from_slice(&invite.id.to_be_bytes());
            encode_text(&mut out, invite.inviter.as_str());
            encode_text(&mut out, invite.invitee.as_str());
            out.extend_from_slice(&invite.expires_at_ms.to_be_bytes());
        }
        out.extend_from_slice(&(self.presence.len() as u32).to_be_bytes());
        for (character, presence) in &self.presence {
            encode_text(&mut out, character.as_str());
            out.extend_from_slice(&presence.zone.get().to_be_bytes());
            out.extend_from_slice(&presence.player.to_be_bytes());
            encode_text(&mut out, &presence.name);
            out.extend_from_slice(&presence.offline_since_ms.unwrap_or(u64::MAX).to_be_bytes());
        }
        out.extend_from_slice(&(self.views.len() as u32).to_be_bytes());
        for (character, (revision, state)) in &self.views {
            encode_text(&mut out, character.as_str());
            out.extend_from_slice(&revision.to_be_bytes());
            encode_view(&mut out, state);
        }
        out
    }

    /// Reads a stored directory; malformed bytes fail closed.
    pub fn decode(bytes: &[u8]) -> Result<Self, PartyStoreError> {
        let mut reader = Reader { bytes, offset: 0 };
        let mut directory = Self {
            next_id: reader.u64()?,
            next_revision: reader.u64()?,
            ..Self::default()
        };
        for _ in 0..reader.u32()? {
            let id = reader.u64()?;
            let version = reader.u32()?;
            let count = reader.u8()?;
            let mut members = Vec::with_capacity(usize::from(count));
            for _ in 0..count {
                let member = reader.character()?;
                directory.membership.insert(member.clone(), id);
                members.push(member);
            }
            directory.parties.insert(
                id,
                Party {
                    id,
                    version,
                    members,
                },
            );
        }
        for _ in 0..reader.u32()? {
            let id = reader.u64()?;
            let invite = Invite {
                id,
                inviter: reader.character()?,
                invitee: reader.character()?,
                expires_at_ms: reader.u64()?,
            };
            directory.invites.insert(id, invite);
        }
        for _ in 0..reader.u32()? {
            let character = reader.character()?;
            let zone = ZoneId::new(reader.u32()?);
            let player = reader.u32()?;
            let name = reader.text()?;
            let offline = reader.u64()?;
            directory.presence.insert(
                character,
                Presence {
                    zone,
                    player,
                    name,
                    offline_since_ms: (offline != u64::MAX).then_some(offline),
                },
            );
        }
        for _ in 0..reader.u32()? {
            let character = reader.character()?;
            let revision = reader.u64()?;
            let state = decode_view(&mut reader)?;
            directory.views.insert(character, (revision, state));
        }
        if reader.offset != bytes.len() {
            return Err(PartyStoreError::corrupt());
        }
        Ok(directory)
    }
}

fn encode_view(out: &mut Vec<u8>, state: &PartyState) {
    out.push(u8::from(state.roster.is_some()));
    if let Some(roster) = &state.roster {
        out.extend_from_slice(&roster.party.to_be_bytes());
        out.extend_from_slice(&roster.version.to_be_bytes());
        out.push(roster.members.len() as u8);
        for member in &roster.members {
            encode_text(out, &member.name);
            out.extend_from_slice(&member.player.unwrap_or(u32::MAX).to_be_bytes());
            out.push(u8::from(member.online));
        }
    }
    out.push(u8::from(state.invite.is_some()));
    if let Some(invite) = &state.invite {
        out.extend_from_slice(&invite.invite.to_be_bytes());
        encode_text(out, &invite.from);
    }
}

fn decode_view(reader: &mut Reader<'_>) -> Result<PartyState, PartyStoreError> {
    let roster = if reader.u8()? != 0 {
        let party = reader.u64()?;
        let version = reader.u32()?;
        let count = reader.u8()?;
        let mut members = Vec::with_capacity(usize::from(count));
        for _ in 0..count {
            let name = reader.text()?;
            let player = reader.u32()?;
            let online = reader.u8()? != 0;
            members.push(PartyMember {
                name,
                player: (player != u32::MAX).then_some(player),
                online,
            });
        }
        Some(PartyRoster {
            party,
            version,
            members,
        })
    } else {
        None
    };
    let invite = if reader.u8()? != 0 {
        let invite = reader.u64()?;
        let from = reader.text()?;
        Some(PartyInvite { invite, from })
    } else {
        None
    };
    Ok(PartyState {
        revision: 0,
        roster,
        invite,
    })
}

fn encode_text(out: &mut Vec<u8>, text: &str) {
    out.push(text.len() as u8);
    out.extend_from_slice(text.as_bytes());
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl Reader<'_> {
    fn take(&mut self, count: usize) -> Result<&[u8], PartyStoreError> {
        let end = self
            .offset
            .checked_add(count)
            .ok_or_else(PartyStoreError::corrupt)?;
        let slice = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(PartyStoreError::corrupt)?;
        self.offset = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, PartyStoreError> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, PartyStoreError> {
        Ok(u32::from_be_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| PartyStoreError::corrupt())?,
        ))
    }

    fn u64(&mut self) -> Result<u64, PartyStoreError> {
        Ok(u64::from_be_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| PartyStoreError::corrupt())?,
        ))
    }

    fn text(&mut self) -> Result<String, PartyStoreError> {
        let length = usize::from(self.u8()?);
        std::str::from_utf8(self.take(length)?)
            .map(str::to_owned)
            .map_err(|_| PartyStoreError::corrupt())
    }

    fn character(&mut self) -> Result<CharacterId, PartyStoreError> {
        CharacterId::new(self.text()?).map_err(|_| PartyStoreError::corrupt())
    }
}

/// A failed directory transaction: the zone's lease is not current or the store is unavailable.
/// A directory transaction applies nothing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PartyStoreError {
    Fenced,
    Backend(String),
}

impl PartyStoreError {
    #[must_use]
    pub fn corrupt() -> Self {
        Self::Backend("stored party directory is malformed".to_owned())
    }
}

impl fmt::Display for PartyStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fenced => formatter.write_str("the zone's lease is not current"),
            Self::Backend(cause) => write!(formatter, "party store failed: {cause}"),
        }
    }
}

impl std::error::Error for PartyStoreError {}

impl From<ControlPlaneError> for PartyStoreError {
    fn from(error: ControlPlaneError) -> Self {
        Self::Backend(error.to_string())
    }
}

/// A store of the one party directory. Each transaction checks the issuing zone's lease (when
/// it has one) and applies all or nothing; `body` runs under the store's exclusion.
pub trait PartyAuthority {
    fn transact(
        &mut self,
        issuer: Option<&ZoneLease>,
        body: &mut dyn FnMut(&mut PartyDirectory, u64),
    ) -> Result<(), PartyStoreError>;
}

/// The single-process directory for hosts without a shared store. Its zones are one
/// process's, so there is no lease to fence and a restart starts it empty.
#[derive(Debug)]
pub struct LocalParties {
    directory: PartyDirectory,
    started: std::time::Instant,
}

impl LocalParties {
    #[must_use]
    pub fn new() -> Self {
        Self {
            directory: PartyDirectory::new(),
            started: std::time::Instant::now(),
        }
    }
}

impl Default for LocalParties {
    fn default() -> Self {
        Self::new()
    }
}

impl PartyAuthority for LocalParties {
    fn transact(
        &mut self,
        _issuer: Option<&ZoneLease>,
        body: &mut dyn FnMut(&mut PartyDirectory, u64),
    ) -> Result<(), PartyStoreError> {
        let now = self.started.elapsed().as_millis();
        body(&mut self.directory, u64::try_from(now).unwrap_or(u64::MAX));
        Ok(())
    }
}

/// [`PartyAuthority`] over the in-memory zone directory, for tests and reference
/// composition: a stale or expired lease is fenced.
pub struct ReferenceParties<'a> {
    pub zones: &'a ZoneDirectory,
    pub parties: &'a mut PartyDirectory,
    pub now_tick: u64,
}

impl PartyAuthority for ReferenceParties<'_> {
    fn transact(
        &mut self,
        issuer: Option<&ZoneLease>,
        body: &mut dyn FnMut(&mut PartyDirectory, u64),
    ) -> Result<(), PartyStoreError> {
        if let Some(lease) = issuer {
            self.zones
                .ensure_current(lease, self.now_tick)
                .map_err(|_| PartyStoreError::Fenced)?;
        }
        body(self.parties, self.now_tick);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn character(name: &str) -> CharacterId {
        CharacterId::new(name).unwrap()
    }

    /// `name` online in `zone` under player ID `player`.
    fn present(directory: &mut PartyDirectory, zone: u32, entries: &[(&str, PlayerId)], now: u64) {
        let roster: Vec<_> = entries
            .iter()
            .map(|(name, player)| PartyRosterEntry {
                character: character(name),
                player: *player,
                name: (*name).to_owned(),
            })
            .collect();
        directory.sync_zone(ZoneId::new(zone), &roster, now);
    }

    fn pair() -> (PartyDirectory, u64) {
        let mut directory = PartyDirectory::new();
        present(&mut directory, 1, &[("aria", 1), ("bren", 2)], 0);
        let invite = directory
            .invite(
                &character("aria"),
                &InviteTarget::Character(character("bren")),
                0,
            )
            .unwrap();
        (directory, invite)
    }

    fn roster_of(directory: &mut PartyDirectory, who: &str) -> Option<PartyRoster> {
        directory.view(&character(who), 0).roster
    }

    #[test]
    fn invite_accept_forms_a_party_with_the_inviter_as_leader() {
        let (mut directory, invite) = pair();
        assert_eq!(
            directory.view(&character("bren"), 0).invite,
            Some(PartyInvite {
                invite,
                from: "aria".into()
            })
        );
        directory.accept(&character("bren"), invite, 1).unwrap();
        let roster = roster_of(&mut directory, "bren").unwrap();
        assert_eq!(
            roster
                .members
                .iter()
                .map(|m| m.name.as_str())
                .collect::<Vec<_>>(),
            ["aria", "bren"]
        );
        assert_eq!(roster.version, 1);
        // A solo player has no party (Atlas 5.4.7): leaving dissolves a party of two.
        directory.leave(&character("bren")).unwrap();
        assert!(roster_of(&mut directory, "aria").is_none());
    }

    #[test]
    fn leader_leaving_promotes_the_longest_standing_member() {
        let (mut directory, invite) = pair();
        directory.accept(&character("bren"), invite, 1).unwrap();
        present(
            &mut directory,
            1,
            &[("aria", 1), ("bren", 2), ("cara", 3)],
            2,
        );
        let second = directory
            .invite(
                &character("aria"),
                &InviteTarget::Character(character("cara")),
                2,
            )
            .unwrap();
        directory.accept(&character("cara"), second, 3).unwrap();
        directory.leave(&character("aria")).unwrap();
        let roster = roster_of(&mut directory, "cara").unwrap();
        assert_eq!(roster.members[0].name, "bren");
        assert_eq!(roster.members.len(), 2);
    }

    #[test]
    fn membership_survives_a_handoff_and_shows_the_member_elsewhere() {
        let (mut directory, invite) = pair();
        directory.accept(&character("bren"), invite, 1).unwrap();
        // Bren crosses to zone 2: the source lists it no more, the destination does.
        present(&mut directory, 1, &[("aria", 1)], 2);
        present(&mut directory, 2, &[("bren", 9)], 3);
        let roster = roster_of(&mut directory, "aria").unwrap();
        assert_eq!(roster.members[1].name, "bren");
        assert_eq!(
            roster.members[1].player, None,
            "elsewhere has no local player"
        );
        assert!(roster.members[1].online);
        // A later fenced operation still names the same party.
        directory.leave(&character("bren")).unwrap();
        assert!(roster_of(&mut directory, "aria").is_none());
    }

    #[test]
    fn duplicate_and_stale_operations_fail_closed() {
        let (mut directory, invite) = pair();
        directory.accept(&character("bren"), invite, 1).unwrap();
        // The same accept again is gone, and the party is unchanged.
        let before = directory.clone();
        assert_eq!(
            directory.accept(&character("bren"), invite, 2),
            Err(PartyRefusal::InviteGone)
        );
        // A kick decided on an older roster is refused.
        assert_eq!(
            directory.kick(&character("aria"), 1, 0),
            Err(PartyRefusal::StaleParty)
        );
        // Only the leader kicks.
        assert_eq!(
            directory.kick(&character("bren"), 0, 1),
            Err(PartyRefusal::NotLeader)
        );
        assert_eq!(directory, before);
        directory.leave(&character("bren")).unwrap();
        assert_eq!(
            directory.leave(&character("bren")),
            Err(PartyRefusal::NotInParty)
        );
    }

    #[test]
    fn a_fenced_zone_changes_nothing() {
        let mut zones = ZoneDirectory::new(100).unwrap();
        let mut hosts = crate::HostRegistry::new(1_000).unwrap();
        let host = crate::HostId::new("host").unwrap();
        hosts.register(host.clone(), 0).unwrap();
        let lease = zones
            .assign(ZoneId::new(1), host.clone(), &hosts, 0)
            .unwrap();
        let mut directory = PartyDirectory::new();
        {
            let mut parties = ReferenceParties {
                zones: &zones,
                parties: &mut directory,
                now_tick: 1,
            };
            parties
                .transact(Some(&lease), &mut |directory, now| {
                    present(directory, 1, &[("aria", 1), ("bren", 2)], now);
                })
                .unwrap();
        }
        zones
            .reassign(ZoneId::new(1), lease.epoch, host, &hosts, 2)
            .unwrap();
        let mut parties = ReferenceParties {
            zones: &zones,
            parties: &mut directory,
            now_tick: 3,
        };
        let before = parties.parties.clone();
        let result = parties.transact(Some(&lease), &mut |_directory, _now| {
            panic!("a fenced transaction must not run")
        });
        assert_eq!(result, Err(PartyStoreError::Fenced));
        assert_eq!(*parties.parties, before);
    }

    #[test]
    fn five_is_full_and_invites_lapse() {
        let mut directory = PartyDirectory::new();
        let names = ["aria", "bren", "cara", "dara", "edda", "finn"];
        let entries: Vec<_> = names
            .iter()
            .enumerate()
            .map(|(i, n)| (*n, i as u32 + 1))
            .collect();
        present(&mut directory, 1, &entries, 0);
        let leader = character("aria");
        for name in &names[1..5] {
            let invite = directory
                .invite(&leader, &InviteTarget::Name((*name).to_owned()), 0)
                .unwrap();
            directory.accept(&character(name), invite, 0).unwrap();
        }
        assert_eq!(
            directory.invite(&leader, &InviteTarget::Name("finn".into()), 0),
            Err(PartyRefusal::PartyFull)
        );
        // An unanswered invite lapses.
        let mut directory = PartyDirectory::new();
        present(&mut directory, 1, &[("aria", 1), ("bren", 2)], 0);
        let invite = directory
            .invite(
                &character("aria"),
                &InviteTarget::Character(character("bren")),
                0,
            )
            .unwrap();
        directory.sweep(PARTY_INVITE_TTL_MS);
        assert_eq!(
            directory.accept(&character("bren"), invite, PARTY_INVITE_TTL_MS),
            Err(PartyRefusal::InviteGone)
        );
    }

    #[test]
    fn view_revisions_advance_only_with_content_and_stay_above_the_held_one() {
        let (mut directory, invite) = pair();
        directory.accept(&character("bren"), invite, 1).unwrap();
        let first = directory.view(&character("bren"), 0);
        let again = directory.view(&character("bren"), first.revision);
        assert_eq!(first, again, "unchanged content keeps its revision");
        present(&mut directory, 1, &[("aria", 1)], 2);
        let changed = directory.view(&character("bren"), first.revision);
        assert!(changed.revision > first.revision);
        // A zone holding a later revision from before a restart still gets a newer one.
        let mut fresh = PartyDirectory::new();
        present(&mut fresh, 1, &[("aria", 1)], 0);
        let cleared = fresh.view(&character("aria"), 500);
        assert!(cleared.revision > 500 && cleared.roster.is_none());
    }

    #[test]
    fn the_directory_round_trips_for_the_shared_store() {
        let (mut directory, invite) = pair();
        directory.accept(&character("bren"), invite, 1).unwrap();
        directory.view(&character("bren"), 0);
        let bytes = directory.encode();
        let restored = PartyDirectory::decode(&bytes).unwrap();
        assert_eq!(restored, directory);
        assert!(PartyDirectory::decode(&bytes[..bytes.len() - 1]).is_err());
    }
}
