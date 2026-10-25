//! MMO client adapter over the pinned session protocol and WebTransport.
//! Reconnect tokens stay private to this in-memory session; server authority owns
//! their rotation, grace period, and connection epochs.

use crate::ClientError;
use game_server::{
    BrowserRoutePrefix, MatchId, ReconnectToken, SnapshotFrame, SnapshotReassembler, WELCOME_BYTES,
    Welcome,
};
use mmorpg_core::{EntityKind, EntityRef, TICK_HZ, ZoneCommand, ZoneId, ZoneSnapshot};
use std::sync::atomic::{AtomicU64, Ordering};
use std::{collections::BTreeSet, error::Error, fmt, path::Path, time::Duration};
use tokio::sync::Mutex;
use url::Url;
use wtransport::{
    ClientConfig, Connection, Endpoint, VarInt,
    endpoint::endpoint_side::Client,
    error::{ConnectionError, SendDatagramError},
    tls::Certificate,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(5);
const RECONNECT_SETTLE: Duration = Duration::from_millis(250);

#[derive(Debug)]
pub enum SessionError {
    Receive(ConnectionError),
    Send(SendDatagramError),
    SnapshotTimeout,
    InvalidData(ClientError),
    InvalidMovement,
    SequenceExhausted,
}

impl fmt::Display for SessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Receive(error) => error.fmt(formatter),
            Self::Send(error) => error.fmt(formatter),
            Self::SnapshotTimeout => formatter.write_str("server snapshot timed out"),
            Self::InvalidData(error) => write!(formatter, "invalid session data: {error}"),
            Self::InvalidMovement => {
                formatter.write_str("forward and strafe intent must be in [-1, 1]")
            }
            Self::SequenceExhausted => formatter.write_str("command sequence exhausted"),
        }
    }
}

impl Error for SessionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Receive(error) => Some(error),
            Self::Send(error) => Some(error),
            Self::InvalidData(error) => Some(error.as_ref()),
            _ => None,
        }
    }
}

// Parse and build routes through the same contract as the server. Never accept
// a credential-bearing reconnect URL from CLI configuration.
struct SessionRoute {
    url: Url,
    prefix: BrowserRoutePrefix,
    match_id: MatchId,
}

impl SessionRoute {
    fn parse(value: &str) -> Result<Self, ClientError> {
        let url = Url::parse(value).map_err(|_| "invalid session URL")?;
        if url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err("session URL must be HTTPS without credentials, query, or fragment".into());
        }
        let (prefix, id) = url
            .path()
            .rsplit_once("/matches/")
            .ok_or("session URL must name a hosted match")?;
        let prefix = BrowserRoutePrefix::new(prefix)?;
        let match_id = MatchId::new(id)?;
        Ok(Self {
            url,
            prefix,
            match_id,
        })
    }

    fn reconnect_url(&self, token: [u8; 16]) -> Url {
        let mut url = self.url.clone();
        url.set_path(
            &self
                .prefix
                .reconnect_path(&self.match_id, ReconnectToken(token)),
        );
        url
    }
}

pub struct ClientSession {
    // The endpoint and its verified TLS configuration outlive every connection.
    endpoint: Endpoint<Client>,
    connection: Connection,
    reassembler: Mutex<SnapshotReassembler>,
    route: SessionRoute,
    welcome: Welcome,
    sequence: u32,
    /// Facing of the latest movement intent, reused by the stopped resume intent.
    facing: u16,
    /// One more than the newest projection tick received, 0 before the first
    /// (#415); combo inputs report it as their observation.
    observed: AtomicU64,
    resume_available: bool,
    zone_id: ZoneId,
    content_revision: u64,
    durable_character: bool,
    character_service: Option<bool>,
}

impl ClientSession {
    pub async fn connect(
        url: &str,
        certificate: Option<&Path>,
        zone_id: ZoneId,
        content_revision: u64,
    ) -> Result<Self, ClientError> {
        let route = SessionRoute::parse(url)?;
        tokio::time::timeout(CONNECT_TIMEOUT, async {
            let builder = ClientConfig::builder().with_bind_default();
            let config = match certificate {
                Some(path) => {
                    builder.with_server_certificate_hashes([Certificate::load_pemfile(path)
                        .await?
                        .hash()])
                }
                None => builder.with_native_certs(),
            }
            .keep_alive_interval(Some(Duration::from_secs(1)))
            .build();
            let endpoint = Endpoint::client(config)?;
            let connection = endpoint.connect(route.url.as_str()).await?;
            let welcome = read_welcome(&connection).await?;
            Ok(Self {
                endpoint,
                connection,
                reassembler: Mutex::new(SnapshotReassembler::new()),
                route,
                welcome,
                sequence: 0,
                facing: 0,
                observed: AtomicU64::new(0),
                resume_available: true,
                zone_id,
                content_revision,
                durable_character: false,
                character_service: None,
            })
        })
        .await
        .map_err(|_| "connection or welcome timed out")?
    }

    async fn character_control(
        &self,
        payload: &[u8],
    ) -> Result<game_server::ControlResponse, ClientError> {
        tokio::time::timeout(Duration::from_secs(5), async {
            let (mut send, mut receive) = self.connection.open_bi().await?.await?;
            send.write_all(&game_server::encode_control_request(payload)?)
                .await?;
            send.finish().await?;
            let mut header = [0_u8; game_server::CONTROL_HEADER_BYTES];
            receive.read_exact(&mut header).await?;
            let length = usize::from(u16::from_be_bytes([header[6], header[7]]));
            if length > game_server::MAX_CONTROL_PAYLOAD_BYTES {
                return Err::<_, ClientError>("oversized character control response".into());
            }
            let mut frame = header.to_vec();
            frame.resize(header.len() + length, 0);
            receive.read_exact(&mut frame[header.len()..]).await?;
            let mut trailing = [0_u8; 1];
            if receive.read(&mut trailing).await?.is_some() {
                return Err("trailing character control response bytes".into());
            }
            Ok(game_server::decode_control_response(&frame)?)
        })
        .await
        .map_err(|_| "character control request timed out")?
    }

    /// Discover once per session; an unavailable exchange never becomes anonymous entry.
    pub async fn character_service_enabled(&mut self) -> Result<bool, ClientError> {
        if let Some(enabled) = self.character_service {
            return Ok(enabled);
        }
        let capability = self.character_control(&[1, 0]).await?;
        if capability.accepted && capability.payload != [1, 1] {
            return Err("unsupported character service".into());
        }
        self.character_service = Some(capability.accepted);
        Ok(capability.accepted)
    }

    /// A server-side lease claim in explicit development/LAN trust mode.
    /// A rejected capability means an anonymous host; a failed claim never falls back.
    pub async fn claim_character(
        &mut self,
        id: &mmorpg_core::CharacterId,
        class: mmorpg_core::ClassChoice,
    ) -> Result<bool, ClientError> {
        if !self.character_service_enabled().await? {
            return Ok(false);
        }
        let mut request = vec![1, 1, class.appearance() as u8];
        request.extend_from_slice(id.as_str().as_bytes());
        let result = self.character_control(&request).await?;
        if !result.accepted || result.payload != [1, 1] {
            return Err("the zone host refused this character; it may already be in play".into());
        }
        self.durable_character = true;
        Ok(true)
    }

    #[must_use]
    pub const fn has_durable_character(&self) -> bool {
        self.durable_character
    }

    /// Explicit logout; interrupted paths retain the session for reconnect instead.
    pub async fn release_character(&mut self) -> Result<(), ClientError> {
        if self.durable_character {
            let result = self.character_control(&[1, 2]).await?;
            if !result.accepted || result.payload != [1, 2] {
                return Err("the zone host did not confirm character logout".into());
            }
            self.durable_character = false;
        }
        Ok(())
    }

    #[must_use]
    pub const fn player_id(&self) -> u32 {
        self.welcome.player_id
    }

    #[must_use]
    pub const fn connection_epoch(&self) -> u32 {
        self.welcome.connection_epoch
    }

    /// The largest datagram this connection currently negotiates, or `None`
    /// when the peer does not support datagrams. Both peers run the pinned
    /// WebTransport stack with default transport configuration; the host
    /// observes its own current budget when sending each snapshot.
    #[must_use]
    pub fn max_datagram_size(&self) -> Option<usize> {
        self.connection.max_datagram_size()
    }

    /// Interrupt transport while retaining the in-memory resume identity.
    pub fn disconnect(&self) {
        self.connection
            .close(VarInt::from_u32(0), b"client disconnected");
    }

    /// One bounded resume attempt, never a fallback to fresh admission. A failure
    /// ends the session: the server may already have rotated the one-time token.
    /// Cancellation is supported by dropping this future and its owning session.
    pub async fn reconnect(&mut self) -> Result<ZoneSnapshot, ClientError> {
        if !self.resume_available {
            return Err("session resume is unavailable after an incomplete attempt".into());
        }
        self.resume_available = false;
        let grace =
            Duration::from_secs_f64(self.welcome.reconnect_grace_ticks as f64 / f64::from(TICK_HZ));
        let bound = CONNECT_TIMEOUT.min(grace);
        self.disconnect();
        let result = tokio::time::timeout(bound, async {
            // Let the server observe connection closure before it handles resume.
            // There is no protocol-level disconnect acknowledgement in this pin.
            tokio::time::sleep(RECONNECT_SETTLE).await;
            let url = self.route.reconnect_url(self.welcome.reconnect_token);
            // Do not propagate an error containing the credential-bearing URL.
            self.connection = self
                .endpoint
                .connect(url.as_str())
                .await
                .map_err(|_| "could not reconnect to the existing session")?;
            // Fragment and delivered-tick state belongs to the old connection.
            self.reassembler = Mutex::new(SnapshotReassembler::new());
            let welcome = read_welcome(&self.connection).await?;
            if welcome.player_id != self.welcome.player_id
                || welcome.connection_epoch <= self.welcome.connection_epoch
                || welcome.reconnect_token == self.welcome.reconnect_token
            {
                return Err("server did not resume the expected player and epoch".into());
            }
            self.welcome = welcome;
            // Keep the highest *sent* sequence, even if its acknowledgement was
            // lost. Never replay old inputs. Start the resumed connection stopped.
            let facing = self.facing;
            self.send_move(0, 0, facing)?;
            let stopped_sequence = self.sequence;
            let mut resend = tokio::time::interval(Duration::from_millis(50));
            resend.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    snapshot = self.receive_snapshot() => {
                        let snapshot = snapshot?;
                        if snapshot.acknowledged_sequence >= stopped_sequence {
                            return Ok(snapshot);
                        }
                    }
                    _ = resend.tick() => self.send_move(0, 0, facing)?,
                }
            }
        })
        .await
        .map_err(|_| ClientError::from("session reconnect timed out"))
        .and_then(|result| result);
        if result.is_err() {
            self.disconnect();
        } else {
            self.resume_available = true;
        }
        result
    }

    /// Only interruption/timeouts are candidates for resume. Server application
    /// rejection, malformed snapshots and protocol errors terminate the session.
    pub async fn can_reconnect(&self, error: &SessionError) -> bool {
        if !self.resume_available {
            return false;
        }
        match error {
            SessionError::SnapshotTimeout => true,
            SessionError::Receive(error) => transient_connection_error(error),
            SessionError::Send(SendDatagramError::NotConnected) => {
                tokio::time::timeout(Duration::from_secs(1), self.connection.closed())
                    .await
                    .is_ok_and(|error| transient_connection_error(&error))
            }
            _ => false,
        }
    }

    /// Held movement relative to `facing`; positive strafe is the character's right.
    pub fn send_move(&mut self, forward: i8, strafe: i8, facing: u16) -> Result<(), SessionError> {
        if !(-1..=1).contains(&forward) || !(-1..=1).contains(&strafe) {
            return Err(SessionError::InvalidMovement);
        }
        self.send_command(ZoneCommand::Move {
            forward,
            strafe,
            facing,
        })?;
        self.facing = facing;
        Ok(())
    }

    /// One edge-triggered jump; the server ignores it unless grounded.
    pub fn send_jump(&mut self) -> Result<(), SessionError> {
        self.send_command(ZoneCommand::Jump)
    }

    /// Selects a unit, or clears the selection; the zone validates it.
    pub fn send_select_target(&mut self, target: Option<EntityRef>) -> Result<(), SessionError> {
        self.send_command(ZoneCommand::SelectTarget(target))
    }

    /// Starts (`true`) or stops auto-attacking the selected target.
    pub fn send_attack(&mut self, start: bool) -> Result<(), SessionError> {
        self.send_command(if start {
            ZoneCommand::StartAttack
        } else {
            ZoneCommand::StopAttack
        })
    }

    /// Asks a dead character to return to the graveyard.
    pub fn send_release_spirit(&mut self) -> Result<(), SessionError> {
        self.send_command(ZoneCommand::ReleaseSpirit)
    }

    /// Chooses the class and sex once; the zone refuses a repeat with an event.
    pub fn send_choose_class(&mut self, class: u8, sex: u8) -> Result<(), SessionError> {
        self.send_command(ZoneCommand::ChooseClass { class, sex })
    }

    /// Uses a class ability at the current selection; the zone validates it.
    pub fn send_use_ability(&mut self, ability: u8) -> Result<(), SessionError> {
        self.send_command(ZoneCommand::UseAbility {
            ability,
            target: None,
        })
    }

    /// Stops the character's own cast or channel.
    pub fn send_cancel_cast(&mut self) -> Result<(), SessionError> {
        self.send_command(ZoneCommand::CancelCast)
    }

    /// Claims all remaining rewards from the projected corpse death.
    pub fn send_loot(&mut self, claim: mmorpg_core::LootClaim) -> Result<(), SessionError> {
        self.send_command(ZoneCommand::Loot(claim))
    }

    /// Queues a move within the player's own authoritative bag.
    pub fn send_move_item(
        &mut self,
        source: u8,
        destination: u8,
        quantity: u16,
    ) -> Result<(), SessionError> {
        self.send_command(ZoneCommand::MoveItem {
            source,
            destination,
            quantity,
        })
    }

    /// Sends any zone command under the next sequence; the zone validates it.
    pub fn send_command(&mut self, command: ZoneCommand) -> Result<(), SessionError> {
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or(SessionError::SequenceExhausted)?;
        // Combo inputs report the newest projection they were made against, so
        // the zone can make up for their travel time within its bounded grace
        // (#415); it still judges them on arrival.
        let observed = self.observed.load(Ordering::Relaxed).checked_sub(1);
        let payload = match (command, observed) {
            (ZoneCommand::UseAbility { .. } | ZoneCommand::DirectedStrike { .. }, Some(tick)) => {
                mmorpg_protocol::encode_observed_command(command, tick)
            }
            _ => mmorpg_protocol::encode_command(command),
        };
        let frame = game_server::encode_command(sequence, &payload)
            .map_err(|error| SessionError::InvalidData(error.into()))?;
        self.connection
            .send_datagram(frame)
            .map_err(SessionError::Send)?;
        self.sequence = sequence;
        Ok(())
    }

    pub async fn receive_snapshot(&self) -> Result<ZoneSnapshot, SessionError> {
        tokio::time::timeout(SNAPSHOT_TIMEOUT, async {
            loop {
                let datagram = self
                    .connection
                    .receive_datagram()
                    .await
                    .map_err(SessionError::Receive)?;
                let frame = self
                    .reassembler
                    .lock()
                    .await
                    .accept(&datagram)
                    .map_err(|error| SessionError::InvalidData(error.into()))?;
                if let Some(frame) = frame {
                    let snapshot = self
                        .decode_snapshot(frame)
                        .map_err(SessionError::InvalidData)?;
                    if self.durable_character && snapshot.viewer.class.is_none() {
                        continue;
                    }
                    self.observed
                        .fetch_max(snapshot.tick.saturating_add(1), Ordering::Relaxed);
                    return Ok(snapshot);
                }
            }
        })
        .await
        .map_err(|_| SessionError::SnapshotTimeout)?
    }

    fn decode_snapshot(&self, frame: SnapshotFrame) -> Result<ZoneSnapshot, ClientError> {
        let snapshot = mmorpg_protocol::decode_snapshot(&frame.payload)?;
        if snapshot.zone_id != self.zone_id || snapshot.content_revision != self.content_revision {
            return Err("server zone or world content does not match this client".into());
        }
        if frame.tick != snapshot.tick {
            return Err("session and zone snapshot ticks disagree".into());
        }
        if snapshot.viewer_id != self.player_id() {
            return Err("player projection is addressed to another player".into());
        }
        let mut entities = BTreeSet::new();
        if snapshot
            .entities
            .iter()
            .any(|entity| !entities.insert((entity.kind, entity.id)))
        {
            return Err("duplicate entity in snapshot".into());
        }
        if !entities.contains(&(EntityKind::Player, self.player_id())) {
            return Err("player projection does not contain the local player".into());
        }
        Ok(snapshot)
    }
}

async fn read_welcome(connection: &Connection) -> Result<Welcome, ClientError> {
    let mut stream = connection.accept_uni().await?;
    let mut bytes = [0; WELCOME_BYTES];
    stream.read_exact(&mut bytes).await?;
    let mut trailing = [0];
    if stream.read(&mut trailing).await?.is_some() {
        return Err("welcome contains trailing bytes".into());
    }
    let welcome = game_server::decode_welcome(&bytes)?;
    if welcome.tick_hz != TICK_HZ || welcome.player_id == 0 || welcome.connection_epoch == 0 {
        return Err("server welcome is incompatible with this client".into());
    }
    Ok(welcome)
}

fn transient_connection_error(error: &ConnectionError) -> bool {
    matches!(
        error,
        ConnectionError::TimedOut | ConnectionError::LocallyClosed
    )
}

impl Drop for ClientSession {
    fn drop(&mut self) {
        self.disconnect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resume_route_preserves_origin_and_uses_shared_route_contract() {
        let route =
            SessionRoute::parse("https://localhost:4433/custom/game/matches/zone-2").unwrap();
        let token = [0xab; 16];
        let resumed = route.reconnect_url(token);
        assert_eq!(resumed.origin(), route.url.origin());
        let parsed = route.prefix.parse(resumed.path()).unwrap().unwrap();
        assert_eq!(parsed.match_id, route.match_id);
        assert_eq!(
            parsed.admission,
            game_server::BrowserAdmission::Reconnect(ReconnectToken(token))
        );
        for invalid in [
            "http://localhost/game/matches/zone-1",
            "https://user:secret@localhost/game/matches/zone-1",
            "https://localhost/game/matches/zone-1?token=secret",
            "https://localhost/game/matches/zone-1#fragment",
            "https://localhost/game/matches/zone-1/reconnect/secret",
            "https://localhost/game/matches/zone-1/",
        ] {
            assert!(SessionRoute::parse(invalid).is_err());
        }
    }
}
