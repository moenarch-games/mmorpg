//! Trusted, replayed character admission. Storage I/O stays in the host service.
use game_server::SimulationError;
use mmorpg_core::{CharacterId, CharacterRecord, ZoneSimulation};
use mmorpg_protocol::{decode_character_record, encode_character_record};

use crate::{ZoneGameServerAdapter, map_protocol_error, map_zone_error};

impl ZoneGameServerAdapter {
    /// Stable identity for a successfully bound live session.
    #[must_use]
    pub fn character_id(&self, player: game_server::PlayerId) -> Option<&CharacterId> {
        self.characters.get(&player)
    }

    /// Encodes already leased server data, never a client-supplied record.
    pub fn character_admission_input(record: &CharacterRecord) -> Result<Vec<u8>, SimulationError> {
        let mut payload = vec![1, 1]; // host-input version, admission tag
        payload.extend_from_slice(&encode_character_record(record).map_err(map_protocol_error)?);
        if payload.len() > game_server::MAX_COMMAND_PAYLOAD_BYTES {
            return Err(SimulationError::new(
                "character admission exceeds host input limit",
            ));
        }
        Ok(payload)
    }

    pub(super) fn admit_stored_character(
        &mut self,
        player: game_server::PlayerId,
        payload: &[u8],
    ) -> Result<(), SimulationError> {
        let Some(bytes) = payload.strip_prefix(&[1, 1]) else {
            return Err(SimulationError::new("unsupported character host input"));
        };
        let record = decode_character_record(bytes).map_err(map_protocol_error)?;
        if self.characters.contains_key(&player)
            || self
                .characters
                .values()
                .any(|id| id == &record.character_id)
        {
            return Err(SimulationError::new(
                "character or session is already bound",
            ));
        }
        // Admission is infrequent. Stage it through core's existing exact restore
        // path so malformed/incompatible records cannot erase the current player.
        let mut candidate = ZoneSimulation::from_snapshot(
            self.zone.snapshot().map_err(map_zone_error)?,
            self.zone.content().clone(),
        )
        .map_err(map_zone_error)?;
        if !candidate.remove_player(player) {
            return Err(SimulationError::new("unknown admission session"));
        }
        candidate
            .admit_character(player, &record)
            .map_err(map_zone_error)?;
        self.zone = candidate;
        self.characters.insert(player, record.character_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use game_server::{GameSimulation, MatchRuntime, ReconnectToken, verify_replay};
    use mmorpg_core::{ClassChoice, ZoneId, greyhaven_vale};

    fn record() -> CharacterRecord {
        let mut zone =
            ZoneSimulation::with_content(ZoneId::new(1), greyhaven_vale::content()).unwrap();
        zone.create_character(99, ClassChoice::from_appearance(4).unwrap())
            .unwrap();
        zone.capture_character(99, CharacterId::new("durable-ranger").unwrap())
            .unwrap()
    }

    #[test]
    fn stored_admission_replays_under_a_different_session_identity() {
        let mut runtime = MatchRuntime::new_with_replay_capture(
            ZoneGameServerAdapter::with_content(ZoneId::new(1), greyhaven_vale::content()).unwrap(),
            20,
        );
        let lease = runtime.admit(ReconnectToken([1; 16])).unwrap();
        let record = record();
        let input = ZoneGameServerAdapter::character_admission_input(&record).unwrap();
        runtime.submit_host_input(lease.player_id, &input).unwrap();
        let snapshot = runtime.advance_tick().unwrap();
        let replay = runtime.replay_log().unwrap();
        let verified = verify_replay(
            ZoneGameServerAdapter::with_content(ZoneId::new(1), greyhaven_vale::content()).unwrap(),
            replay,
        )
        .unwrap();
        assert_eq!(verified.final_snapshot, snapshot);
    }

    #[test]
    fn invalid_or_duplicate_admission_preserves_existing_live_state() {
        let mut adapter =
            ZoneGameServerAdapter::with_content(ZoneId::new(1), greyhaven_vale::content()).unwrap();
        adapter.add_player(1).unwrap();
        adapter.add_player(2).unwrap();
        let mut invalid = record();
        invalid.level = 0;
        let before = adapter.snapshot().unwrap();
        let input = ZoneGameServerAdapter::character_admission_input(&invalid).unwrap();
        assert!(adapter.apply_host_input(1, &input).is_err());
        assert_eq!(adapter.snapshot().unwrap(), before);
        let record = record();
        let input = ZoneGameServerAdapter::character_admission_input(&record).unwrap();
        adapter.apply_host_input(1, &input).unwrap();
        assert_eq!(adapter.character_id(1), Some(&record.character_id));
        let before = adapter.snapshot().unwrap();
        assert!(adapter.apply_host_input(2, &input).is_err());
        assert_eq!(adapter.snapshot().unwrap(), before);
    }
}
