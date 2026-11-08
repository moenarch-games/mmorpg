//! Parties on a live host (#404, ADR 0008): the zone side of the party directory.
//!
//! The party service answers clients' reliable-control party requests against the
//! directory (`mmorpg_control_plane::party`) and keeps every zone's held views current:
//! each pass reconciles the characters bound to each zone against the directory and
//! submits changed views as trusted host inputs, so zones consume membership in order
//! with commands and never write it. The service holds no membership of its own and every
//! step is level-triggered from channel and directory state, so a handoff, reconnect or
//! host restart needs no party-specific hook: a character simply reports from its new
//! zone. Directory writes are fenced by the issuing zone's lease where it has one.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use game_server::{
    ControlContext, ControlServiceError, LiveMatchHost, MatchControlService, MatchId,
};
use mmorpg_control_plane::ZoneLease;
use mmorpg_control_plane::party::{InviteTarget, PartyAuthority, PartyDirectory, PartyRosterEntry};
use mmorpg_core::{CharacterId, PartyRefusal, PartyState, PlayerId, ZoneId};
use mmorpg_protocol::party::{
    PARTY_CONTROL, PartyRequest, decode_party_state, encode_party_reply, encode_party_state,
};

use crate::ZoneGameServerAdapter;

/// How often the service reconciles parties.
const PARTY_POLL: Duration = Duration::from_millis(100);

/// The host input tag carrying a player's party view: `[1, 9] · view`.
const PARTY_INPUT: u8 = 9;

/// The characters a zone holds and the view revision each one holds, as of its last tick.
#[derive(Clone, Default)]
pub struct PartyChannel(Arc<Mutex<Bound>>);

#[derive(Clone, Default, Eq, PartialEq)]
struct Bound(BTreeMap<PlayerId, (CharacterId, u64)>);

impl PartyChannel {
    fn snapshot(&self) -> BTreeMap<PlayerId, (CharacterId, u64)> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).0.clone()
    }

    fn character(&self, player: PlayerId) -> Option<CharacterId> {
        self.snapshot().remove(&player).map(|(character, _)| character)
    }

    fn publish(&self, bound: Bound) {
        let mut held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if *held != bound {
            *held = bound;
        }
    }
}

/// A character identity for a session that has none: hosts without character claims still
/// form parties among the characters playing there.
fn session_character(zone: ZoneId, player: PlayerId) -> Option<CharacterId> {
    CharacterId::new(format!("session-{}-{player}", zone.get())).ok()
}

impl ZoneGameServerAdapter {
    /// Attaches the host's party service; the zone publishes its bound characters every tick.
    pub fn attach_parties(&mut self, channel: PartyChannel) {
        self.parties = Some(channel);
    }

    /// The tick step: publishes the characters bound to live sessions.
    pub(crate) fn publish_parties(&self) {
        let Some(channel) = &self.parties else {
            return;
        };
        let bound = if self.requires_character_claims {
            self.characters
                .iter()
                .filter(|(player, _)| !self.parked_characters.contains_key(player))
                .map(|(player, character)| {
                    (*player, (character.clone(), self.zone.party_revision(*player)))
                })
                .collect()
        } else {
            self.zone
                .player_ids()
                .filter_map(|player| {
                    let character = self
                        .characters
                        .get(&player)
                        .cloned()
                        .or_else(|| session_character(self.zone.zone_id(), player))?;
                    Some((player, (character, self.zone.party_revision(player))))
                })
                .collect()
        };
        channel.publish(Bound(bound));
    }

    /// Applies a party view host input; `false` when `payload` is not one.
    pub(crate) fn apply_party_input(
        &mut self,
        player: PlayerId,
        payload: &[u8],
    ) -> Result<bool, game_server::SimulationError> {
        let [1, PARTY_INPUT, body @ ..] = payload else {
            return Ok(false);
        };
        let state = decode_party_state(body).map_err(crate::map_protocol_error)?;
        self.zone
            .apply_party_state(player, state)
            .map_err(crate::map_zone_error)?;
        Ok(true)
    }
}

fn party_input(state: &PartyState) -> Option<Vec<u8>> {
    let mut payload = vec![1, PARTY_INPUT];
    payload.extend_from_slice(&encode_party_state(state).ok()?);
    Some(payload)
}

/// One zone the service reconciles.
#[derive(Clone)]
pub struct PartyZone {
    pub zone: ZoneId,
    /// Fences this zone's directory writes when the host has a shared directory.
    pub lease: Option<ZoneLease>,
    pub channel: PartyChannel,
}

#[derive(Clone, Default)]
struct Session {
    epoch: u32,
    name: Option<String>,
}

/// The host's party service; see the module documentation.
pub struct PartyService {
    host: LiveMatchHost<ZoneGameServerAdapter>,
    runtime: tokio::runtime::Handle,
    authority: Mutex<Box<dyn PartyAuthority + Send>>,
    zones: BTreeMap<MatchId, PartyZone>,
    sessions: Mutex<BTreeMap<(MatchId, PlayerId), Session>>,
}

impl PartyService {
    #[must_use]
    pub fn new(
        host: LiveMatchHost<ZoneGameServerAdapter>,
        authority: Box<dyn PartyAuthority + Send>,
        zones: BTreeMap<MatchId, PartyZone>,
    ) -> Arc<Self> {
        Arc::new(Self {
            host,
            runtime: tokio::runtime::Handle::current(),
            authority: Mutex::new(authority),
            zones,
            sessions: Mutex::default(),
        })
    }

    fn with_session<R>(
        &self,
        body: impl FnOnce(&mut BTreeMap<(MatchId, PlayerId), Session>) -> R,
    ) -> R {
        body(&mut self.sessions.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// Remembers the session's connection epoch for its party views.
    fn track(&self, id: &MatchId, context: ControlContext) {
        self.with_session(|sessions| {
            sessions
                .entry((id.clone(), context.player_id))
                .or_default()
                .epoch = context.connection_epoch;
        });
    }

    fn transact(
        &self,
        zone: &PartyZone,
        body: &mut dyn FnMut(&mut PartyDirectory, u64),
    ) -> Result<(), String> {
        self.authority
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .transact(zone.lease.as_ref(), body)
            .map_err(|error| error.to_string())
    }

    /// Answers a party request from one connection.
    fn handle(
        &self,
        id: &MatchId,
        context: ControlContext,
        payload: &[u8],
    ) -> Result<Vec<u8>, ControlServiceError> {
        let request =
            PartyRequest::decode(payload).map_err(|error| ControlServiceError::new(error.to_string()))?;
        if let PartyRequest::Hello { name } = request {
            self.with_session(|sessions| {
                sessions.entry((id.clone(), context.player_id)).or_default().name = Some(name);
            });
            return Ok(encode_party_reply(Ok(())));
        }
        Ok(encode_party_reply(self.dispatch(id, context, request)))
    }

    fn dispatch(
        &self,
        id: &MatchId,
        context: ControlContext,
        request: PartyRequest,
    ) -> Result<(), PartyRefusal> {
        let zone = self.zones.get(id).ok_or(PartyRefusal::Unavailable)?;
        let actor = zone
            .channel
            .character(context.player_id)
            .ok_or(PartyRefusal::Unavailable)?;
        let invite_target = |name: String| {
            local_player_alias(&name)
                .and_then(|player| zone.channel.character(player))
                .map_or(InviteTarget::Name(name), InviteTarget::Character)
        };
        let target = match &request {
            PartyRequest::InvitePlayer { player } => Some(InviteTarget::Character(
                zone.channel.character(*player).ok_or(PartyRefusal::NotFound)?,
            )),
            PartyRequest::InviteName { name } => Some(invite_target(name.clone())),
            _ => None,
        };
        let mut outcome = Err(PartyRefusal::Unavailable);
        self.transact(zone, &mut |directory, now| {
            outcome = match &request {
                PartyRequest::InvitePlayer { .. } | PartyRequest::InviteName { .. } => target
                    .as_ref()
                    .map_or(Err(PartyRefusal::NotFound), |target| {
                        directory.invite(&actor, target, now).map(|_| ())
                    }),
                PartyRequest::Accept { invite } => directory.accept(&actor, *invite, now),
                PartyRequest::Decline { invite } => directory.decline(&actor, *invite),
                PartyRequest::Leave => directory.leave(&actor),
                PartyRequest::Kick { member, version } => directory.kick(&actor, *member, *version),
                PartyRequest::Hello { .. } => Ok(()),
            };
        })
        .map_err(|_| PartyRefusal::Unavailable)?;
        outcome
    }

    /// One reconciliation pass over every zone.
    pub fn reconcile(&self) {
        for (id, zone) in &self.zones {
            if let Err(error) = self.reconcile_zone(id, zone) {
                eprintln!("party reconcile of {} failed: {error}", id.as_str());
            }
        }
    }

    fn reconcile_zone(&self, id: &MatchId, zone: &PartyZone) -> Result<(), String> {
        let bound = zone.channel.snapshot();
        let roster: Vec<PartyRosterEntry> = self.with_session(|sessions| {
            bound
                .iter()
                .map(|(player, (character, _))| PartyRosterEntry {
                    character: character.clone(),
                    player: *player,
                    name: sessions
                        .get(&(id.clone(), *player))
                        .and_then(|session| session.name.clone())
                        .unwrap_or_else(|| format!("Player {player}")),
                })
                .collect()
        });
        let mut views = Vec::with_capacity(bound.len());
        self.transact(zone, &mut |directory, now| {
            directory.sync_zone(zone.zone, &roster, now);
            directory.sweep(now);
            views = bound
                .iter()
                .map(|(player, (character, held))| {
                    (*player, *held, directory.view(character, *held))
                })
                .collect();
        })?;
        for (player, held, view) in views {
            if view.revision == 0 || view.revision == held {
                continue;
            }
            let Some(epoch) = self.with_session(|sessions| {
                sessions.get(&(id.clone(), player)).map(|session| session.epoch)
            }) else {
                continue; // The session has not spoken over control yet.
            };
            let Some(input) = party_input(&view) else {
                continue;
            };
            if let Err(error) =
                self.runtime
                    .block_on(self.host.submit_host_input(id, player, epoch, &input))
            {
                eprintln!("party view for {player} in {} not applied: {error}", id.as_str());
                self.with_session(|sessions| sessions.remove(&(id.clone(), player)));
            }
        }
        Ok(())
    }

    /// Reconciles until the process ends.
    pub async fn run(self: Arc<Self>) {
        let mut interval = tokio::time::interval(PARTY_POLL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            if self.host.is_serving() {
                let service = Arc::clone(&self);
                if let Err(error) = tokio::task::spawn_blocking(move || service.reconcile()).await {
                    eprintln!("party reconcile stopped: {error}");
                }
            }
        }
    }
}

/// `"Player 7"` or `"#7"`: the label other players are shown under.
fn local_player_alias(name: &str) -> Option<PlayerId> {
    let name = name.trim();
    let digits = name
        .strip_prefix('#')
        .or_else(|| {
            name.get(..7)
                .filter(|prefix| prefix.eq_ignore_ascii_case("player "))
                .map(|_| &name[7..])
        })?
        .trim();
    digits.parse().ok()
}

/// Serves a zone host without durable characters: a client's session is its character. It
/// keeps the host's single-process party directory.
pub async fn serve_open_zone_host(
    zone_ids: Vec<ZoneId>,
    reconnect_grace_ticks: u64,
    recovery_directory: Option<std::path::PathBuf>,
    transport: game_server::MatchHostWebTransportConfig,
    status: game_server::MatchHostStatusConfig,
    shutdown: tokio::sync::mpsc::Receiver<()>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let validated = crate::build_zone_matches(zone_ids.iter().copied())?;
    let mut zones = BTreeMap::new();
    for zone_id in &zone_ids {
        zones.insert(
            crate::zone_match_id(*zone_id)?,
            PartyZone {
                zone: *zone_id,
                lease: None,
                channel: PartyChannel::default(),
            },
        );
    }
    let attached = zones.clone();
    let control = |parties: Arc<PartyService>| PartyControl {
        inner: game_server::RejectMatchControlService,
        parties,
    };
    if let Some(directory) = recovery_directory {
        let ids: Vec<_> = zones.keys().cloned().collect();
        let count = ids.len();
        let prepared = game_server::prepare_live_match_host_for_recovery(
            ids,
            move |id: &MatchId| -> Result<_, String> {
                let zone = attached.get(id).ok_or("unknown zone")?;
                let mut adapter = ZoneGameServerAdapter::with_content(
                    zone.zone,
                    mmorpg_core::content_for_zone(zone.zone),
                )
                .map_err(|error| error.to_string())?;
                adapter.attach_parties(zone.channel.clone());
                Ok(adapter)
            },
            count,
            reconnect_grace_ticks,
            game_server::MatchHostRecoveryConfig { directory },
        )
        .await?;
        let parties = PartyService::new(
            prepared.host(),
            Box::new(mmorpg_control_plane::party::LocalParties::new()),
            zones,
        );
        tokio::spawn(Arc::clone(&parties).run());
        game_server::serve_prepared_live_match_host_with_status_and_control_and_shutdown(
            prepared,
            control(parties),
            transport,
            status,
            shutdown,
        )
        .await?;
    } else {
        let mut host = game_server::MatchHost::new(validated.len())?;
        for (id, mut adapter) in validated {
            if let Some(zone) = attached.get(&id) {
                adapter.attach_parties(zone.channel.clone());
            }
            let runtime = game_server::MatchRuntime::new(adapter, reconnect_grace_ticks);
            host.insert(id, runtime)
                .map_err(|failure| failure.into_parts().0)?;
        }
        let host = LiveMatchHost::new(host);
        let parties = PartyService::new(
            host.clone(),
            Box::new(mmorpg_control_plane::party::LocalParties::new()),
            zones,
        );
        tokio::spawn(Arc::clone(&parties).run());
        game_server::serve_live_match_host_with_status_and_control_and_shutdown(
            host,
            control(parties),
            transport,
            status,
            shutdown,
        )
        .await?;
    }
    Ok(())
}

/// Answers party requests and passes every other control request to `inner`.
pub struct PartyControl<S> {
    pub inner: S,
    pub parties: Arc<PartyService>,
}

impl<S: MatchControlService> MatchControlService for PartyControl<S> {
    fn handle(
        &self,
        id: &MatchId,
        context: ControlContext,
        payload: &[u8],
    ) -> Result<Vec<u8>, ControlServiceError> {
        self.parties.track(id, context);
        if payload.first() == Some(&PARTY_CONTROL) {
            return self.parties.handle(id, context, payload);
        }
        self.inner.handle(id, context, payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use game_server::GameSimulation;
    use mmorpg_core::{PartyMember, PartyRoster};

    fn view(revision: u64) -> PartyState {
        PartyState {
            revision,
            roster: Some(PartyRoster {
                party: 1,
                version: 1,
                members: vec![
                    PartyMember {
                        name: "Aria".into(),
                        player: Some(1),
                        online: true,
                    },
                    PartyMember {
                        name: "Bren".into(),
                        player: None,
                        online: true,
                    },
                ],
            }),
            invite: None,
        }
    }

    #[test]
    fn host_inputs_set_a_sessions_view_and_stale_ones_change_nothing() {
        let mut adapter = ZoneGameServerAdapter::new(ZoneId::new(4));
        let channel = PartyChannel::default();
        adapter.attach_parties(channel.clone());
        adapter.add_player(1).unwrap();
        adapter.advance_tick().unwrap();
        // Without character claims a session stands in as its character.
        let bound = channel.snapshot();
        assert_eq!(bound[&1].0.as_str(), "session-4-1");
        assert_eq!(bound[&1].1, 0);

        adapter
            .apply_host_input(1, &party_input(&view(3)).unwrap())
            .unwrap();
        assert_eq!(adapter.zone().party_revision(1), 3);
        // An older view from before a handoff or restart does not win.
        adapter
            .apply_host_input(1, &party_input(&view(2)).unwrap())
            .unwrap();
        assert_eq!(adapter.zone().party_revision(1), 3);
        adapter.advance_tick().unwrap();
        assert_eq!(channel.snapshot()[&1].1, 3);
    }

    #[test]
    fn local_player_aliases_parse() {
        assert_eq!(local_player_alias("Player 7"), Some(7));
        assert_eq!(local_player_alias("#9"), Some(9));
        assert_eq!(local_player_alias(" player  12 "), Some(12));
        assert_eq!(local_player_alias("Aria"), None);
    }
}
