//! Trusted, replayed character admission. Storage I/O stays in the host service.
use game_server::SimulationError;
use mmorpg_core::{CharacterId, CharacterRecord, ZoneSimulation};
use mmorpg_protocol::{
    decode_canonical_snapshot, decode_character_record, encode_character_record,
};

use crate::{ZoneGameServerAdapter, map_protocol_error, map_zone_error};

pub struct StoredCharacter {
    pub player: game_server::PlayerId,
    pub parked: bool,
    pub record: CharacterRecord,
}

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
        if payload == [1, 2] {
            let id = self
                .characters
                .get(&player)
                .ok_or_else(|| SimulationError::new("session has no character"))?
                .clone();
            let record = self
                .zone
                .capture_character(player, id)
                .map_err(map_zone_error)?;
            self.zone.remove_player(player);
            self.parked_characters.insert(player, record);
            return Ok(());
        }
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
    /// Holds the development directory lock for the whole simulation lifetime.
    pub fn require_character_claims(&mut self, ownership: std::sync::Arc<std::fs::File>) {
        self.character_ownership = Some(ownership);
    }

    pub(super) fn character_snapshot(
        &self,
        canonical: Vec<u8>,
    ) -> Result<Vec<u8>, SimulationError> {
        if self.characters.is_empty() {
            return Ok(canonical);
        }
        let mut bytes = b"MMOC\x01".to_vec();
        bytes.extend_from_slice(&(canonical.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&canonical);
        bytes.extend_from_slice(&(self.characters.len() as u32).to_be_bytes());
        for (player, id) in &self.characters {
            let record = match self.parked_characters.get(player) {
                Some(record) => record.clone(),
                None => self
                    .zone
                    .capture_character(*player, id.clone())
                    .map_err(map_zone_error)?,
            };
            let record = encode_character_record(&record).map_err(map_protocol_error)?;
            bytes.extend_from_slice(&player.to_be_bytes());
            bytes.push(u8::from(self.parked_characters.contains_key(player)));
            bytes.extend_from_slice(&(record.len() as u32).to_be_bytes());
            bytes.extend_from_slice(&record);
        }
        Ok(bytes)
    }

    /// Private tick-boundary records for off-tick persistence, never a player projection.
    pub fn stored_characters(payload: &[u8]) -> Result<Vec<StoredCharacter>, SimulationError> {
        if !payload.starts_with(b"MMOC") {
            decode_canonical_snapshot(payload).map_err(map_protocol_error)?;
            return Ok(Vec::new());
        }
        let mut bytes = payload
            .strip_prefix(b"MMOC\x01")
            .ok_or_else(|| SimulationError::new("unsupported character checkpoint"))?;
        fn take<'a>(bytes: &mut &'a [u8], length: usize) -> Result<&'a [u8], SimulationError> {
            let (value, rest) = bytes
                .split_at_checked(length)
                .ok_or_else(|| SimulationError::new("truncated character checkpoint"))?;
            *bytes = rest;
            Ok(value)
        }
        fn number(bytes: &mut &[u8]) -> Result<u32, SimulationError> {
            Ok(u32::from_be_bytes(take(bytes, 4)?.try_into().map_err(
                |_| SimulationError::new("invalid checkpoint field"),
            )?))
        }
        let size = number(&mut bytes)? as usize;
        decode_canonical_snapshot(take(&mut bytes, size)?).map_err(map_protocol_error)?;
        let count = number(&mut bytes)? as usize;
        if count > mmorpg_core::MAX_PLAYERS_PER_ZONE {
            return Err(SimulationError::new("too many stored characters"));
        }
        let mut records = Vec::with_capacity(count);
        for _ in 0..count {
            let player = number(&mut bytes)?;
            let parked = take(&mut bytes, 1)?[0];
            if parked > 1 {
                return Err(SimulationError::new("invalid parked character flag"));
            }
            let size = number(&mut bytes)? as usize;
            let record =
                decode_character_record(take(&mut bytes, size)?).map_err(map_protocol_error)?;
            records.push(StoredCharacter {
                player,
                parked: parked == 1,
                record,
            });
        }
        if !bytes.is_empty() {
            return Err(SimulationError::new("trailing character checkpoint bytes"));
        }
        Ok(records)
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
    #[test]
    fn logout_parks_the_exact_record_for_off_tick_commit_and_replay() {
        let mut runtime = MatchRuntime::new_with_replay_capture(
            ZoneGameServerAdapter::with_content(ZoneId::new(1), greyhaven_vale::content()).unwrap(),
            20,
        );
        let lease = runtime.admit(ReconnectToken([2; 16])).unwrap();
        let record = record();
        let input = ZoneGameServerAdapter::character_admission_input(&record).unwrap();
        runtime.submit_host_input(lease.player_id, &input).unwrap();
        runtime.submit_host_input(lease.player_id, &[1, 2]).unwrap();
        let snapshot = runtime.advance_tick().unwrap();
        let stored = ZoneGameServerAdapter::stored_characters(&snapshot.payload).unwrap();
        assert_eq!(stored.len(), 1);
        assert!(stored[0].parked);
        assert_eq!(stored[0].record, record);
        let verified = verify_replay(
            ZoneGameServerAdapter::with_content(ZoneId::new(1), greyhaven_vale::content()).unwrap(),
            runtime.replay_log().unwrap(),
        )
        .unwrap();
        assert_eq!(verified.final_snapshot, snapshot);
    }
}
