//! The local single-player zone host behind the wasm-bindgen surface.
//!
//! It owns one `ZoneSimulation` built from the same content as
//! `mmorpg-zone-host` and plays the session role that `game-server`'s
//! `MatchRuntime` plays for network hosts: it allocates player IDs, drops
//! stale command sequences, and publishes only encoded player-scoped
//! projections. Gameplay rules stay in `mmorpg-core`; encoding stays in
//! `mmorpg-protocol`.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use mmorpg_core::{
    PlayerId, ZoneContent, ZoneDefinition, ZoneError, ZoneId, ZoneSimulation, greyhaven_vale,
};
use mmorpg_protocol::{ProtocolError, decode_command, pack_snapshot};

/// The zone the local host simulates. It matches `mmorpg-zone-host`'s default
/// `MMORPG_ZONE_IDS=1`, so local and network projections carry the same zone ID.
pub const LOCAL_ZONE_ID: ZoneId = ZoneId::new(1);

/// The content every host loads for [`LOCAL_ZONE_ID`]; `mmorpg-game-server`'s
/// `build_zone_matches` installs the same content (checked by tests).
#[must_use]
pub fn hosted_content() -> Arc<ZoneContent> {
    greyhaven_vale::content()
}

/// The collision definition of [`hosted_content`], which scenery derives from.
#[must_use]
pub fn hosted_definition() -> ZoneDefinition {
    hosted_content().definition().clone()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LocalZoneError {
    /// The player never joined or has left.
    UnknownPlayer(PlayerId),
    /// Sequence 0 is never valid.
    InvalidSequence,
    /// The command bytes are malformed.
    Command(ProtocolError),
    /// The zone refused the operation, e.g. at capacity or on tick overflow.
    Zone(ZoneError),
    /// Player IDs are never reused, and all have been issued.
    PlayerIdsExhausted,
}

impl fmt::Display for LocalZoneError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownPlayer(player_id) => {
                write!(formatter, "player {player_id} is not in the local zone")
            }
            Self::InvalidSequence => formatter.write_str("command sequence 0 is invalid"),
            Self::Command(error) => write!(formatter, "malformed command: {error}"),
            Self::Zone(error) => write!(formatter, "zone error: {error}"),
            Self::PlayerIdsExhausted => formatter.write_str("local player IDs are exhausted"),
        }
    }
}

impl std::error::Error for LocalZoneError {}

/// What happened to a well-formed command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubmitOutcome {
    Applied,
    /// A sequence at or below the last applied one changes nothing, like
    /// `game_server::CommandOutcome::IgnoredStale`.
    IgnoredStale,
}

pub struct LocalZoneHost {
    zone: ZoneSimulation,
    next_player_id: PlayerId,
    last_sequences: BTreeMap<PlayerId, u32>,
}

impl LocalZoneHost {
    pub fn new() -> Result<Self, LocalZoneError> {
        let zone = ZoneSimulation::with_content(LOCAL_ZONE_ID, hosted_content())
            .map_err(LocalZoneError::Zone)?;
        Ok(Self {
            zone,
            next_player_id: 1,
            last_sequences: BTreeMap::new(),
        })
    }

    #[must_use]
    pub const fn zone_id(&self) -> ZoneId {
        self.zone.zone_id()
    }

    #[must_use]
    pub fn content_revision(&self) -> u64 {
        self.zone.content().revision()
    }

    #[must_use]
    pub const fn current_tick(&self) -> u64 {
        self.zone.current_tick()
    }

    /// Spawns a new player. IDs start at 1 and are never reused, so a
    /// projection addressed to a player who left can never alias a newcomer.
    pub fn join(&mut self) -> Result<PlayerId, LocalZoneError> {
        let player_id = self.next_player_id;
        let next = player_id
            .checked_add(1)
            .ok_or(LocalZoneError::PlayerIdsExhausted)?;
        self.zone
            .add_player(player_id)
            .map_err(LocalZoneError::Zone)?;
        self.next_player_id = next;
        self.last_sequences.insert(player_id, 0);
        Ok(player_id)
    }

    /// Spawns a new player and chooses its class (0 Warden, 1 Ranger,
    /// 2 Arcanist) and sex (0 female, 1 male) as its first command, under
    /// sequence 1, so the client's own commands start at sequence 2. The
    /// zone refuses an invalid choice with an event in the next tick.
    pub fn join_as(&mut self, class: u8, sex: u8) -> Result<PlayerId, LocalZoneError> {
        let player_id = self.join()?;
        let choice =
            mmorpg_protocol::encode_command(mmorpg_core::ZoneCommand::ChooseClass { class, sex });
        if let Err(error) = self.submit(player_id, 1, &choice) {
            self.leave(player_id);
            return Err(error);
        }
        Ok(player_id)
    }

    /// Removes the player's unit from the zone immediately.
    pub fn leave(&mut self, player_id: PlayerId) -> bool {
        self.last_sequences.remove(&player_id);
        self.zone.remove_player(player_id)
    }

    /// Checks follow `MatchRuntime::submit_command`: sequence 0 and unknown
    /// players are errors, stale sequences are ignored before decoding, and
    /// malformed bytes are errors that change nothing.
    pub fn submit(
        &mut self,
        player_id: PlayerId,
        sequence: u32,
        payload: &[u8],
    ) -> Result<SubmitOutcome, LocalZoneError> {
        if sequence == 0 {
            return Err(LocalZoneError::InvalidSequence);
        }
        let last = self
            .last_sequences
            .get_mut(&player_id)
            .ok_or(LocalZoneError::UnknownPlayer(player_id))?;
        if sequence <= *last {
            return Ok(SubmitOutcome::IgnoredStale);
        }
        let command = decode_command(payload).map_err(LocalZoneError::Command)?;
        self.zone
            .apply_command(player_id, sequence, command)
            .map_err(LocalZoneError::Zone)?;
        *last = sequence;
        Ok(SubmitOutcome::Applied)
    }

    /// Advances one fixed 30 Hz tick and returns the new tick.
    pub fn tick(&mut self) -> Result<u64, LocalZoneError> {
        self.zone.advance_tick().map_err(LocalZoneError::Zone)?;
        Ok(self.zone.current_tick())
    }

    /// The encoded player-scoped projection addressed to `player_id`, the same
    /// budget-packed bytes a network host sends. Canonical state never leaves
    /// this type.
    pub fn projection(&self, player_id: PlayerId) -> Result<Vec<u8>, LocalZoneError> {
        if !self.last_sequences.contains_key(&player_id) {
            return Err(LocalZoneError::UnknownPlayer(player_id));
        }
        let snapshot = self
            .zone
            .snapshot_for_player(player_id)
            .map_err(LocalZoneError::Zone)?;
        pack_snapshot(&snapshot)
            .map(|packed| packed.payload)
            .map_err(LocalZoneError::Command)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use game_server::{
        CommandOutcome, MatchRuntime, RECONNECT_TOKEN_BYTES, ReconnectToken, RuntimeError,
    };
    use mmorpg_core::{EntityKind, ZoneCommand, trig::YAW_QUARTER_TURN};
    use mmorpg_game_server::build_zone_matches;
    use mmorpg_protocol::{decode_snapshot, encode_command};

    const EAST: u16 = YAW_QUARTER_TURN;

    fn run(facing: u16) -> Vec<u8> {
        encode_command(ZoneCommand::Move {
            forward: 1,
            strafe: 0,
            facing,
        })
    }

    fn own_position(host: &LocalZoneHost, player_id: PlayerId) -> [i32; 3] {
        let snapshot = decode_snapshot(&host.projection(player_id).unwrap()).unwrap();
        assert_eq!(snapshot.viewer_id, player_id);
        snapshot
            .entities
            .iter()
            .find(|entity| entity.kind == EntityKind::Player && entity.id == player_id)
            .unwrap()
            .position
    }

    #[test]
    fn joining_with_a_class_spends_sequence_one_on_the_choice() {
        let mut host = LocalZoneHost::new().unwrap();
        let player = host.join_as(2, 0).unwrap();
        assert_eq!(
            host.submit(player, 1, &run(EAST)).unwrap(),
            SubmitOutcome::IgnoredStale
        );
        host.tick().unwrap();
        let snapshot = decode_snapshot(&host.projection(player).unwrap()).unwrap();
        assert_eq!(snapshot.acknowledged_sequence, 1);
        assert_eq!(
            snapshot.viewer.class.map(|choice| choice.class),
            Some(mmorpg_core::PlayerClass::Arcanist)
        );
        assert_eq!(snapshot.entities[0].appearance, 5);
        // An invalid choice is a gameplay refusal, not a join failure.
        let other = host.join_as(9, 9).unwrap();
        host.tick().unwrap();
        let snapshot = decode_snapshot(&host.projection(other).unwrap()).unwrap();
        assert_eq!(snapshot.viewer.class, None);
        assert_eq!(snapshot.events.len(), 1);
    }

    #[test]
    fn hosts_the_same_zone_and_content_as_the_network_zone_host() {
        let matches = build_zone_matches([LOCAL_ZONE_ID]).unwrap();
        let (_, network) = &matches[0];
        let host = LocalZoneHost::new().unwrap();
        assert_eq!(host.zone_id(), network.zone().zone_id());
        assert_eq!(&hosted_definition(), network.zone().definition());
        assert_eq!(hosted_content().as_ref(), network.zone().content().as_ref());
        assert_eq!(host.content_revision(), network.zone().content().revision());
    }

    #[test]
    fn join_move_and_jump_drive_the_shared_simulation() {
        let mut host = LocalZoneHost::new().unwrap();
        let player = host.join().unwrap();
        assert_eq!(player, 1);
        let spawn = own_position(&host, player);
        assert_eq!(
            host.submit(player, 1, &run(EAST)),
            Ok(SubmitOutcome::Applied)
        );
        for _ in 0..10 {
            host.tick().unwrap();
        }
        let moved = own_position(&host, player);
        assert!(moved[0] > spawn[0], "facing east runs toward +X");
        assert_eq!(moved[2], spawn[2]);

        assert_eq!(
            host.submit(player, 2, &encode_command(ZoneCommand::Jump)),
            Ok(SubmitOutcome::Applied)
        );
        host.tick().unwrap();
        assert!(
            own_position(&host, player)[1] > moved[1],
            "a grounded jump rises"
        );
        assert_eq!(host.current_tick(), 11);
    }

    #[test]
    fn session_rules_match_the_network_runtime() {
        let mut local = LocalZoneHost::new().unwrap();
        let (_, adapter) = build_zone_matches([LOCAL_ZONE_ID])
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        let mut network = MatchRuntime::new(adapter, 120);
        let lease = network
            .admit(ReconnectToken([1; RECONNECT_TOKEN_BYTES]))
            .unwrap();
        let player = local.join().unwrap();
        assert_eq!(player, lease.player_id, "both hosts issue player 1 first");

        let malformed = [2, 1, 2, 0, 0, 0];
        let cases: [(u32, Vec<u8>); 6] = [
            (0, run(EAST)),
            (3, run(EAST)),
            (3, run(0)),
            (2, run(0)),
            (4, malformed.to_vec()),
            (5, encode_command(ZoneCommand::Jump)),
        ];
        for (sequence, payload) in cases {
            let network_outcome =
                network.submit_command(lease.player_id, lease.connection_epoch, sequence, &payload);
            let local_outcome = local.submit(player, sequence, &payload);
            match (network_outcome, local_outcome) {
                (Ok(CommandOutcome::Applied), Ok(SubmitOutcome::Applied))
                | (Ok(CommandOutcome::IgnoredStale), Ok(SubmitOutcome::IgnoredStale))
                | (Err(RuntimeError::InvalidSequence), Err(LocalZoneError::InvalidSequence))
                | (Err(RuntimeError::Simulation(_)), Err(LocalZoneError::Command(_))) => {}
                (network, local) => {
                    panic!("sequence {sequence}: network {network:?} but local {local:?}")
                }
            }
        }
        for _ in 0..12 {
            network.advance_tick().unwrap();
            local.tick().unwrap();
            assert_eq!(
                network.snapshot_for(lease.player_id).unwrap().payload,
                local.projection(player).unwrap(),
                "identical projection bytes at tick {}",
                local.current_tick()
            );
        }
    }

    /// Targeting and attack intents, including refused ones, produce the
    /// same projection bytes (events included) on both hosts.
    #[test]
    fn combat_intents_project_identically_on_both_hosts() {
        use mmorpg_core::{EntityRef, NpcId};
        let mut local = LocalZoneHost::new().unwrap();
        let (_, adapter) = build_zone_matches([LOCAL_ZONE_ID])
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        let mut network = MatchRuntime::new(adapter, 120);
        let lease = network
            .admit(ReconnectToken([4; RECONNECT_TOKEN_BYTES]))
            .unwrap();
        let player = local.join().unwrap();
        let commands = [
            ZoneCommand::StartAttack,
            ZoneCommand::SelectTarget(Some(EntityRef::Npc(NpcId::new(1)))),
            ZoneCommand::StartAttack,
            ZoneCommand::ReleaseSpirit,
            ZoneCommand::SelectTarget(None),
        ];
        for (sequence, command) in (1..).zip(commands) {
            let payload = encode_command(command);
            assert_eq!(
                network.submit_command(lease.player_id, lease.connection_epoch, sequence, &payload),
                Ok(CommandOutcome::Applied)
            );
            assert_eq!(
                local.submit(player, sequence, &payload),
                Ok(SubmitOutcome::Applied)
            );
        }
        for _ in 0..30 {
            network.advance_tick().unwrap();
            local.tick().unwrap();
            assert_eq!(
                network.snapshot_for(lease.player_id).unwrap().payload,
                local.projection(player).unwrap()
            );
        }
        let first = decode_snapshot(&local.projection(player).unwrap()).unwrap();
        assert_eq!(first.viewer.target, None);
        assert!(first.events.is_empty(), "events last one tick");
    }

    /// A step of `scenarios/bots/browser-local-session.toml`.
    enum SessionStep {
        Join,
        Command(ZoneCommand),
        Leave,
    }

    /// Replays the browser demo's scripted session against both hosts. While
    /// the player is joined, every projection must be byte-identical. Leaving
    /// differs by design: the local host removes the unit at once, the network
    /// host only after reconnect grace, so nothing is compared while away.
    #[test]
    fn browser_local_session_projections_match_the_network_runtime() {
        const GRACE_TICKS: u64 = 3;
        const TICKS: u64 = 54;
        let intent = |forward, strafe| ZoneCommand::Move {
            forward,
            strafe,
            facing: EAST,
        };
        let steps = [
            (0, SessionStep::Join),
            (0, SessionStep::Command(intent(1, 0))),
            (10, SessionStep::Command(ZoneCommand::Jump)),
            (20, SessionStep::Command(intent(0, 1))),
            (30, SessionStep::Command(intent(0, 0))),
            (44, SessionStep::Leave),
            (50, SessionStep::Join),
        ];
        let mut local = LocalZoneHost::new().unwrap();
        let (_, adapter) = build_zone_matches([LOCAL_ZONE_ID])
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        let mut network = MatchRuntime::new(adapter, GRACE_TICKS);
        let mut tokens = 0_u8;
        let mut session = None;
        let mut sequence = 0_u32;
        let mut compared = Vec::new();
        let mut steps = steps.into_iter().peekable();
        for tick in 0..TICKS {
            while let Some((_, step)) = steps.next_if(|(at, _)| *at == tick) {
                match step {
                    SessionStep::Join => {
                        tokens += 1;
                        let lease = network
                            .admit(ReconnectToken([tokens; RECONNECT_TOKEN_BYTES]))
                            .unwrap();
                        assert_eq!(local.join().unwrap(), lease.player_id);
                        session = Some(lease);
                        sequence = 0;
                    }
                    SessionStep::Command(command) => {
                        let lease = session.unwrap();
                        sequence += 1;
                        let payload = encode_command(command);
                        assert_eq!(
                            network.submit_command(
                                lease.player_id,
                                lease.connection_epoch,
                                sequence,
                                &payload
                            ),
                            Ok(CommandOutcome::Applied)
                        );
                        assert_eq!(
                            local.submit(lease.player_id, sequence, &payload),
                            Ok(SubmitOutcome::Applied)
                        );
                    }
                    SessionStep::Leave => {
                        let lease = session.take().unwrap();
                        assert!(network.disconnect(lease.player_id, lease.connection_epoch));
                        assert!(local.leave(lease.player_id));
                    }
                }
            }
            network.advance_tick().unwrap();
            assert_eq!(local.tick().unwrap(), tick + 1);
            if let Some(lease) = session {
                assert_eq!(
                    network.snapshot_for(lease.player_id).unwrap().payload,
                    local.projection(lease.player_id).unwrap(),
                    "identical projection bytes for player {} at tick {}",
                    lease.player_id,
                    tick + 1
                );
                compared.push((lease.player_id, tick + 1));
            }
        }
        assert!(steps.next().is_none(), "every scripted step ran");
        let expected: Vec<_> = (1..=44)
            .map(|tick| (1, tick))
            .chain((51..=TICKS).map(|tick| (2, tick)))
            .collect();
        assert_eq!(
            compared, expected,
            "compared the first session and the re-entered one"
        );
    }

    #[test]
    fn unknown_players_and_left_players_fail_closed() {
        let mut host = LocalZoneHost::new().unwrap();
        assert_eq!(
            host.submit(7, 1, &run(0)),
            Err(LocalZoneError::UnknownPlayer(7))
        );
        assert_eq!(host.projection(7), Err(LocalZoneError::UnknownPlayer(7)));
        let first = host.join().unwrap();
        assert!(host.leave(first));
        assert!(!host.leave(first), "leaving twice changes nothing");
        assert_eq!(
            host.projection(first),
            Err(LocalZoneError::UnknownPlayer(first))
        );
        assert_eq!(
            host.submit(first, 1, &run(0)),
            Err(LocalZoneError::UnknownPlayer(first))
        );
        let second = host.join().unwrap();
        assert_eq!(second, first + 1, "IDs are never reused");
        let snapshot = decode_snapshot(&host.projection(second).unwrap()).unwrap();
        assert_eq!(
            snapshot
                .entities
                .iter()
                .filter(|entity| entity.kind == EntityKind::Player)
                .map(|entity| entity.id)
                .collect::<Vec<_>>(),
            [second],
            "the departed unit is gone"
        );
    }

    #[test]
    fn malformed_commands_change_nothing() {
        let mut host = LocalZoneHost::new().unwrap();
        let player = host.join().unwrap();
        let before = host.projection(player).unwrap();
        for payload in [
            &[][..],
            &[1, 1, 1, 0],
            &[2, 9],
            &[2, 2, 0],
            &[2, 1, 0, 0, 0],
        ] {
            assert!(matches!(
                host.submit(player, 1, payload),
                Err(LocalZoneError::Command(_))
            ));
        }
        assert_eq!(host.projection(player).unwrap(), before);
        assert_eq!(host.submit(player, 1, &run(0)), Ok(SubmitOutcome::Applied));
    }

    #[test]
    fn projections_are_player_scoped_not_canonical() {
        let mut host = LocalZoneHost::new().unwrap();
        let player = host.join().unwrap();
        let bytes = host.projection(player).unwrap();
        assert!(decode_snapshot(&bytes).is_ok());
        assert!(
            mmorpg_protocol::decode_canonical_snapshot(&bytes).is_err(),
            "canonical recovery state never reaches the browser"
        );
    }
}
