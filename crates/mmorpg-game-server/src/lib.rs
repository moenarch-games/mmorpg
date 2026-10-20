#![forbid(unsafe_code)]

mod character_admission;
mod dev_characters;
mod dev_host;
#[cfg(feature = "postgres")]
pub mod persistence;
pub use dev_characters::{DevCharacterService, DevCharacterStorage};
pub use dev_host::{DevelopmentCharacterHostConfig, serve_development_character_host};
mod fenced_runtime;
pub use fenced_runtime::{FencedRuntimeError, FencedZoneRuntime};

use game_server::{
    GameSimulation, HostError, MatchHost, MatchId, MatchIdError, MatchRuntime, SimulationError,
    SimulationSnapshot, SnapshotScope,
};
use std::sync::Arc;

use mmorpg_core::{MAX_PLAYERS_PER_ZONE, TICK_HZ, ZoneContent, ZoneId, ZoneSimulation};
use mmorpg_protocol::{decode_command, encode_canonical_snapshot, pack_snapshot};

pub const PINNED_GAME_SERVER_REVISION: &str = "f9da1f462f0b0c7f87bbd17588a979df4745c00b";
pub const PINNED_PHYSICS_ENGINE_REVISION: &str = "1b98f84d409796b2a15b84f3fa4ed7f03a11f8bd";

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ZoneHostBuildError {
    Empty,
    DuplicateZone(ZoneId),
    MatchId(MatchIdError),
    Host(HostError),
    Content(mmorpg_core::ZoneError),
}

impl std::fmt::Display for ZoneHostBuildError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => formatter.write_str("zone host must contain at least one zone"),
            Self::DuplicateZone(zone_id) => {
                write!(
                    formatter,
                    "zone {} is configured more than once",
                    zone_id.get()
                )
            }
            Self::MatchId(error) => error.fmt(formatter),
            Self::Host(error) => error.fmt(formatter),
            Self::Content(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ZoneHostBuildError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::MatchId(error) => Some(error),
            Self::Host(error) => Some(error),
            Self::Content(error) => Some(error),
            Self::Empty | Self::DuplicateZone(_) => None,
        }
    }
}

pub struct ZoneGameServerAdapter {
    zone: ZoneSimulation,
    characters: std::collections::BTreeMap<game_server::PlayerId, mmorpg_core::CharacterId>,
    parked_characters:
        std::collections::BTreeMap<game_server::PlayerId, character_admission::ParkedCharacter>,
    character_ownership: Option<Arc<std::fs::File>>,
}

impl ZoneGameServerAdapter {
    #[must_use]
    pub fn new(zone_id: ZoneId) -> Self {
        Self {
            zone: ZoneSimulation::new(zone_id),
            characters: Default::default(),
            parked_characters: Default::default(),
            character_ownership: None,
        }
    }

    /// A zone of the given content. Recovery replays onto a fresh adapter
    /// built with the same content.
    pub fn with_content(
        zone_id: ZoneId,
        content: Arc<ZoneContent>,
    ) -> Result<Self, mmorpg_core::ZoneError> {
        Ok(Self {
            zone: ZoneSimulation::with_content(zone_id, content)?,
            characters: Default::default(),
            parked_characters: Default::default(),
            character_ownership: None,
        })
    }

    #[must_use]
    pub fn zone(&self) -> &ZoneSimulation {
        &self.zone
    }

    pub fn zone_mut(&mut self) -> &mut ZoneSimulation {
        &mut self.zone
    }

    #[must_use]
    pub fn into_zone(self) -> ZoneSimulation {
        self.zone
    }
}

pub fn zone_match_id(zone_id: ZoneId) -> Result<MatchId, MatchIdError> {
    MatchId::new(format!("zone-{}", zone_id.get()))
}

pub fn build_zone_host(
    zone_ids: impl IntoIterator<Item = ZoneId>,
    reconnect_grace_ticks: u64,
) -> Result<MatchHost<ZoneGameServerAdapter>, ZoneHostBuildError> {
    let matches = build_zone_matches(zone_ids)?;
    let mut host = MatchHost::new(matches.len()).map_err(ZoneHostBuildError::Host)?;
    for (match_id, simulation) in matches {
        let runtime = MatchRuntime::new(simulation, reconnect_grace_ticks);
        host.insert(match_id, runtime).map_err(|failure| {
            let (error, _, _) = failure.into_parts();
            ZoneHostBuildError::Host(error)
        })?;
    }
    Ok(host)
}

pub fn build_zone_matches(
    zone_ids: impl IntoIterator<Item = ZoneId>,
) -> Result<Vec<(MatchId, ZoneGameServerAdapter)>, ZoneHostBuildError> {
    let zone_ids = zone_ids.into_iter().collect::<Vec<_>>();
    if zone_ids.is_empty() {
        return Err(ZoneHostBuildError::Empty);
    }

    let mut ordered = zone_ids.clone();
    ordered.sort_unstable();
    if let Some(duplicate) = ordered.windows(2).find_map(|pair| {
        let [left, right] = pair else {
            return None;
        };
        (left == right).then_some(*left)
    }) {
        return Err(ZoneHostBuildError::DuplicateZone(duplicate));
    }

    zone_ids
        .into_iter()
        .map(|zone_id| {
            let match_id = zone_match_id(zone_id).map_err(ZoneHostBuildError::MatchId)?;
            let simulation = ZoneGameServerAdapter::with_content(
                zone_id,
                mmorpg_core::greyhaven_vale::content(),
            )
            .map_err(ZoneHostBuildError::Content)?;
            Ok((match_id, simulation))
        })
        .collect()
}

impl GameSimulation for ZoneGameServerAdapter {
    fn tick_hz(&self) -> u16 {
        TICK_HZ
    }

    fn max_players(&self) -> usize {
        MAX_PLAYERS_PER_ZONE
    }

    fn current_tick(&self) -> u64 {
        self.zone.current_tick()
    }

    fn add_player(&mut self, player_id: game_server::PlayerId) -> Result<(), SimulationError> {
        self.zone.add_player(player_id).map_err(map_zone_error)
    }

    fn remove_player(&mut self, player_id: game_server::PlayerId) -> bool {
        self.characters.remove(&player_id);
        self.parked_characters.remove(&player_id).is_some() || self.zone.remove_player(player_id)
    }

    fn apply_command(
        &mut self,
        player_id: game_server::PlayerId,
        sequence: u32,
        payload: &[u8],
    ) -> Result<(), SimulationError> {
        if self.character_ownership.is_some() && !self.characters.contains_key(&player_id) {
            return Err(SimulationError::new(
                "claim a durable character before gameplay",
            ));
        }
        let command = decode_command(payload).map_err(map_protocol_error)?;
        if self.parked_characters.contains_key(&player_id) {
            // Ignore in-flight gameplay after logout without interrupting its reply.
            return Ok(());
        }
        self.zone
            .apply_command(player_id, sequence, command)
            .map_err(map_zone_error)
    }

    fn apply_host_input(
        &mut self,
        player_id: game_server::PlayerId,
        payload: &[u8],
    ) -> Result<(), SimulationError> {
        self.admit_stored_character(player_id, payload)
    }

    fn advance_tick(&mut self) -> Result<(), SimulationError> {
        self.zone.advance_tick().map_err(map_zone_error)
    }

    fn restore_checkpoint(
        &mut self,
        checkpoint: &SimulationSnapshot,
    ) -> Result<(), SimulationError> {
        self.restore_character_checkpoint(checkpoint)
    }

    fn snapshot_scope(&self) -> SnapshotScope {
        SnapshotScope::PlayerScoped
    }

    fn snapshot(&self) -> Result<SimulationSnapshot, SimulationError> {
        let snapshot = self.zone.snapshot().map_err(map_zone_error)?;
        let canonical = encode_canonical_snapshot(&snapshot).map_err(map_protocol_error)?;
        let payload = self.character_snapshot(canonical)?;
        Ok(SimulationSnapshot::new(snapshot.tick, payload))
    }

    fn snapshot_for(
        &self,
        player_id: game_server::PlayerId,
    ) -> Result<SimulationSnapshot, SimulationError> {
        if let Some(parked) = self.parked_characters.get(&player_id) {
            return Ok(parked.projection.clone());
        }
        let snapshot = self
            .zone
            .snapshot_for_player(player_id)
            .map_err(map_zone_error)?;
        // Budget-driven packing: entities beyond one datagram are omitted by
        // relevance priority, never truncated by transport.
        let payload = pack_snapshot(&snapshot)
            .map_err(map_protocol_error)?
            .payload;
        Ok(SimulationSnapshot::new(snapshot.tick, payload))
    }
}

fn map_zone_error(error: mmorpg_core::ZoneError) -> SimulationError {
    SimulationError::new(error.to_string())
}

fn map_protocol_error(error: mmorpg_protocol::ProtocolError) -> SimulationError {
    SimulationError::new(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use game_server::{MatchRuntime, RECONNECT_TOKEN_BYTES, ReconnectToken};
    use mmorpg_core::ZoneCommand;
    use mmorpg_protocol::{decode_canonical_snapshot, decode_snapshot, encode_command};

    /// Forward-movement headings: yaw 0 faces +Z, a quarter turn faces +X.
    const EAST: u16 = mmorpg_core::trig::YAW_QUARTER_TURN;
    const SOUTH: u16 = 2 * mmorpg_core::trig::YAW_QUARTER_TURN;

    fn run(facing: u16) -> ZoneCommand {
        ZoneCommand::Move {
            forward: 1,
            strafe: 0,
            facing,
        }
    }

    #[test]
    fn zones_map_to_stable_route_safe_match_ids() {
        assert_eq!(zone_match_id(ZoneId::new(42)).unwrap().as_str(), "zone-42");
    }

    #[test]
    fn adapter_keeps_canonical_and_player_scoped_snapshots_separate() {
        let mut adapter = ZoneGameServerAdapter::new(ZoneId::new(7));
        // One default spawn row: player 24 stands beyond player 1's 45 m radius.
        for player_id in 1..=24 {
            adapter.add_player(player_id).unwrap();
        }
        adapter.zone_mut().apply_command(1, 9, run(EAST)).unwrap();

        let canonical = decode_canonical_snapshot(&adapter.snapshot().unwrap().payload).unwrap();
        let projected = decode_snapshot(&adapter.snapshot_for(1).unwrap().payload).unwrap();

        assert_eq!(adapter.snapshot_scope(), SnapshotScope::PlayerScoped);
        assert_eq!(canonical.players.len(), 24);
        assert_eq!(canonical.players[0].forward, 1);
        assert_eq!(canonical.players[0].facing, EAST);
        assert_eq!(canonical.players[0].last_sequence, 9);
        assert_eq!(projected.viewer_id, 1);
        assert_eq!(projected.acknowledged_sequence, 9);
        assert_eq!(projected.entities.len(), 23);
        assert_eq!(projected.entities[0].id, 1);
        assert_eq!(projected.entities[0].facing, EAST);
        assert!(projected.entities.iter().all(|entity| entity.id != 24));
    }

    #[test]
    fn shared_game_server_runtime_drives_the_same_zone_authority() {
        let adapter = ZoneGameServerAdapter::new(ZoneId::new(9));
        let mut runtime = MatchRuntime::new(adapter, 120);
        let lease = runtime
            .admit(ReconnectToken([7; RECONNECT_TOKEN_BYTES]))
            .unwrap();
        let payload = encode_command(run(EAST));

        runtime
            .submit_command(lease.player_id, lease.connection_epoch, 1, &payload)
            .unwrap();
        for _ in 0..5 {
            runtime.advance_tick().unwrap();
        }
        runtime
            .submit_command(
                lease.player_id,
                lease.connection_epoch,
                2,
                &encode_command(ZoneCommand::Jump),
            )
            .unwrap();
        assert!(
            runtime
                .submit_command(lease.player_id, lease.connection_epoch, 3, &[1, 1, 1, 0])
                .is_err(),
            "legacy command wire version is rejected"
        );

        let snapshot =
            decode_snapshot(&runtime.snapshot_for(lease.player_id).unwrap().payload).unwrap();
        assert_eq!(snapshot.zone_id, ZoneId::new(9));
        assert_eq!(snapshot.tick, 5);
        assert_eq!(snapshot.viewer_id, lease.player_id);
        assert_eq!(snapshot.entities.len(), 1);
        assert!(snapshot.entities[0].position[0] > 0);
    }

    #[test]
    fn game_server_recovery_restores_zone_and_reconnect_state() {
        let zone_id = ZoneId::new(9);
        let previous_token = ReconnectToken([7; RECONNECT_TOKEN_BYTES]);
        let replacement_token = ReconnectToken([8; RECONNECT_TOKEN_BYTES]);
        let mut runtime =
            MatchRuntime::new_with_replay_capture(ZoneGameServerAdapter::new(zone_id), 120);
        let lease = runtime.admit(previous_token).unwrap();
        let payload = encode_command(run(EAST));

        runtime
            .submit_command(lease.player_id, lease.connection_epoch, 9, &payload)
            .unwrap();
        for _ in 0..5 {
            runtime.advance_tick().unwrap();
        }
        assert!(runtime.disconnect(lease.player_id, lease.connection_epoch));
        runtime.freeze_for_recovery();

        let image = runtime.recovery_image().unwrap();
        let mut restored =
            MatchRuntime::restore_from_recovery(ZoneGameServerAdapter::new(zone_id), image)
                .unwrap();
        let snapshot = decode_canonical_snapshot(&restored.snapshot().unwrap().payload).unwrap();
        let reconnected = restored
            .reconnect(previous_token, replacement_token)
            .unwrap();

        assert_eq!(snapshot.zone_id, zone_id);
        assert_eq!(snapshot.tick, 5);
        assert_eq!(snapshot.players[0].last_sequence, 9);
        assert!(snapshot.players[0].position[0] > 0.0);
        assert_eq!(reconnected.player_id, lease.player_id);
        assert_eq!(reconnected.connection_epoch, lease.connection_epoch + 1);
    }

    #[test]
    fn recovery_replays_combat_intents_onto_the_hosted_content() {
        use mmorpg_core::{EntityRef, NpcId};
        let zone_id = ZoneId::new(1);
        let hosted = || {
            ZoneGameServerAdapter::with_content(zone_id, mmorpg_core::greyhaven_vale::content())
                .unwrap()
        };
        let token = ReconnectToken([3; RECONNECT_TOKEN_BYTES]);
        let mut runtime = MatchRuntime::new_with_replay_capture(hosted(), 120);
        let lease = runtime.admit(token).unwrap();
        // The marshal stands beside the plaza; attacking an NPC is refused
        // with an event, not a session error.
        let marshal = EntityRef::Npc(NpcId::new(1));
        for (sequence, command) in [
            ZoneCommand::SelectTarget(Some(marshal)),
            ZoneCommand::StartAttack,
            run(SOUTH),
        ]
        .into_iter()
        .enumerate()
        {
            runtime
                .submit_command(
                    lease.player_id,
                    lease.connection_epoch,
                    u32::try_from(sequence).unwrap() + 1,
                    &encode_command(command),
                )
                .unwrap();
        }
        runtime.advance_tick().unwrap();
        let projection =
            decode_snapshot(&runtime.snapshot_for(lease.player_id).unwrap().payload).unwrap();
        assert_eq!(projection.viewer.target, Some(marshal));
        assert_eq!(
            projection.events,
            [mmorpg_core::ZoneEvent::Error {
                code: mmorpg_core::ErrorCode::NotAttackable,
                target: Some(marshal)
            }]
        );
        for _ in 0..90 {
            runtime.advance_tick().unwrap();
        }
        runtime.freeze_for_recovery();
        let expected = runtime.snapshot().unwrap().payload;
        let image = runtime.recovery_image().unwrap();
        let restored = MatchRuntime::restore_from_recovery(hosted(), image).unwrap();
        assert_eq!(restored.snapshot().unwrap().payload, expected);
        let canonical = decode_canonical_snapshot(&expected).unwrap();
        assert_eq!(
            canonical.creatures.len(),
            94,
            "the vale's creatures are hosted"
        );
        assert_eq!(
            canonical.content_fingerprint,
            mmorpg_core::greyhaven_vale::content().fingerprint()
        );
    }

    #[test]
    fn host_rejects_empty_and_duplicate_zone_sets() {
        assert_eq!(
            build_zone_host([], 120).err(),
            Some(ZoneHostBuildError::Empty)
        );
        assert_eq!(
            build_zone_host([ZoneId::new(4), ZoneId::new(4)], 120).err(),
            Some(ZoneHostBuildError::DuplicateZone(ZoneId::new(4)))
        );
    }

    #[test]
    fn one_host_advances_zones_independently() {
        let first_zone = ZoneId::new(10);
        let second_zone = ZoneId::new(11);
        let first_match = zone_match_id(first_zone).unwrap();
        let second_match = zone_match_id(second_zone).unwrap();
        let mut host = build_zone_host([first_zone, second_zone], 120).unwrap();

        let first_lease = host
            .with_runtime_mut(&first_match, |runtime| {
                runtime.admit(ReconnectToken([1; RECONNECT_TOKEN_BYTES]))
            })
            .unwrap()
            .unwrap();
        let second_lease = host
            .with_runtime_mut(&second_match, |runtime| {
                runtime.admit(ReconnectToken([2; RECONNECT_TOKEN_BYTES]))
            })
            .unwrap()
            .unwrap();

        let first_command = encode_command(run(EAST));
        let second_command = encode_command(run(SOUTH));
        host.with_runtime_mut(&first_match, |runtime| {
            runtime.submit_command(
                first_lease.player_id,
                first_lease.connection_epoch,
                1,
                &first_command,
            )
        })
        .unwrap()
        .unwrap();
        host.with_runtime_mut(&second_match, |runtime| {
            runtime.submit_command(
                second_lease.player_id,
                second_lease.connection_epoch,
                1,
                &second_command,
            )
        })
        .unwrap()
        .unwrap();

        for _ in 0..3 {
            host.with_runtime_mut(&first_match, MatchRuntime::advance_tick)
                .unwrap()
                .unwrap();
        }
        host.with_runtime_mut(&second_match, MatchRuntime::advance_tick)
            .unwrap()
            .unwrap();

        let first_snapshot = host
            .with_runtime_mut(&first_match, |runtime| {
                runtime.snapshot_for(first_lease.player_id)
            })
            .unwrap()
            .unwrap();
        let second_snapshot = host
            .with_runtime_mut(&second_match, |runtime| {
                runtime.snapshot_for(second_lease.player_id)
            })
            .unwrap()
            .unwrap();
        let first_snapshot = decode_snapshot(&first_snapshot.payload).unwrap();
        let second_snapshot = decode_snapshot(&second_snapshot.payload).unwrap();

        // Each first player starts on spawn slot 0 of the hosted vale.
        let [spawn_x, spawn_z] = mmorpg_core::greyhaven_vale::SPAWN_GRID.origin;
        assert_eq!(first_snapshot.zone_id, first_zone);
        assert_eq!(first_snapshot.tick, 3);
        assert_eq!(
            first_snapshot.entities[0].position,
            [spawn_x + 3 * 21, 90, spawn_z]
        );
        assert_eq!(second_snapshot.zone_id, second_zone);
        assert_eq!(second_snapshot.tick, 1);
        assert_eq!(
            second_snapshot.entities[0].position,
            [spawn_x, 90, spawn_z - 21]
        );
    }
}
