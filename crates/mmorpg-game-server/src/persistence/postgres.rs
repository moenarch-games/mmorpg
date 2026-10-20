//! Synchronous PostgreSQL composition; callers run this on owned blocking workers.
//! Epoch installation belongs to a separately privileged coordinator, after an
//! actual grant/confirmed retirement. This module never chooses a zone owner.
use super::{CharacterWrite, GrantId, replication::ReplicaBarrier};
use game_server::{
    ReconnectToken, RecoverableSession, RecoveryImage, ReplayLog, SessionRecoverySnapshot,
    SnapshotCommitBatch,
};
use mmorpg_characters::{
    CharacterStore, PostgresEntries, StoreError, postgres::PostgresTransactionEntries,
};
use mmorpg_control_plane::ZoneLease;
use mmorpg_core::CharacterId;
use postgres::{Client, Row, Transaction};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

const VERSION: i32 = 1;
const MIGRATION: &str = include_str!("../../migrations/0001_journal.sql");

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StreamFence {
    pub lease: ZoneLease,
    pub world_id: String,
    pub timeline: u64,
}
#[derive(Clone, Copy)]
pub(crate) struct JournalLimits {
    pub max_batch_bytes: u64,
    pub max_retained_bytes: u64,
    pub max_retained_batches: u64,
    pub max_replay_ticks: u64,
}
impl JournalLimits {
    fn validate(self) -> Result<Self, StoreError> {
        if [
            self.max_batch_bytes,
            self.max_retained_bytes,
            self.max_retained_batches,
            self.max_replay_ticks,
        ]
        .contains(&0)
            || self.max_batch_bytes > self.max_retained_bytes
        {
            return Err(failure("invalid journal limits"));
        }
        for value in [
            self.max_batch_bytes,
            self.max_retained_bytes,
            self.max_retained_batches,
            self.max_replay_ticks,
        ] {
            integer(value)?;
        }
        Ok(self)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CheckpointReference {
    pub version: u8,
    pub zone_id: u32,
    pub world_id: String,
    pub timeline: u64,
    pub position: u64,
    pub tick: u64,
    pub object_key: String,
    pub sha256: [u8; 32],
    pub byte_length: u64,
    pub content_fingerprint: u64,
    pub canonical_version: u16,
}
impl CheckpointReference {
    pub(super) fn validate(&self, fence: &StreamFence) -> Result<(), StoreError> {
        if self.version != 1
            || self.zone_id != fence.lease.zone_id.get()
            || self.world_id != fence.world_id
            || self.timeline != fence.timeline
            || self.byte_length == 0
            || self.object_key.is_empty()
            || self.object_key.len() > 512
            || self
                .object_key
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err(corrupt("invalid checkpoint reference/context"));
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionEnvelope {
    version: u8,
    reconnect_grace_ticks: u64,
    next_player_id: u32,
    sessions: Vec<SessionEntry>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionEntry {
    player_id: u32,
    token: String,
    epoch: u32,
    grace: u64,
}
impl SessionEnvelope {
    fn capture(image: &RecoveryImage) -> Self {
        Self {
            version: 1,
            reconnect_grace_ticks: image.reconnect_grace_ticks,
            next_player_id: image.sessions.next_player_id,
            sessions: image
                .sessions
                .sessions
                .iter()
                .map(|session| SessionEntry {
                    player_id: session.player_id,
                    token: session.reconnect_token.encode_hex(),
                    epoch: session.connection_epoch,
                    grace: session.remaining_grace_ticks,
                })
                .collect(),
        }
    }
    fn restore(self) -> Result<(u64, SessionRecoverySnapshot), StoreError> {
        if self.version != 1 || self.sessions.len() > u16::MAX as usize {
            return Err(corrupt("unsupported journal session metadata"));
        }
        let sessions = self
            .sessions
            .into_iter()
            .map(|session| {
                Ok(RecoverableSession {
                    player_id: session.player_id,
                    reconnect_token: ReconnectToken::decode_hex(&session.token)
                        .ok_or_else(|| corrupt("invalid journal reconnect token"))?,
                    connection_epoch: session.epoch,
                    remaining_grace_ticks: session.grace,
                })
            })
            .collect::<Result<Vec<_>, StoreError>>()?;
        Ok((
            self.reconnect_grace_ticks,
            SessionRecoverySnapshot {
                next_player_id: self.next_player_id,
                sessions,
            },
        ))
    }
}

pub(crate) fn migrate(client: &mut Client) -> Result<(), StoreError> {
    mmorpg_characters::postgres::migrate(client)?;
    let mut transaction = client.transaction().map_err(sql)?;
    transaction
        .execute(
            "SELECT pg_advisory_xact_lock($1)",
            &[&0x4d4d_5a4a_4d49_4752_i64],
        )
        .map_err(sql)?;
    transaction.batch_execute("CREATE SCHEMA IF NOT EXISTS mmorpg_persistence; CREATE TABLE IF NOT EXISTS mmorpg_persistence.migrations (version integer PRIMARY KEY)").map_err(sql)?;
    let version: Option<i32> = transaction
        .query_one(
            "SELECT max(version) FROM mmorpg_persistence.migrations",
            &[],
        )
        .map_err(sql)?
        .get(0);
    match version {
        None => {
            transaction.batch_execute(MIGRATION).map_err(sql)?;
            transaction
                .execute(
                    "INSERT INTO mmorpg_persistence.migrations VALUES ($1)",
                    &[&VERSION],
                )
                .map_err(sql)?;
        }
        Some(VERSION) => {}
        Some(_) => return Err(corrupt("unsupported persistence schema")),
    }
    transaction.commit().map_err(sql)
}

/// Coordinator-only mirror of an already granted lease. An installed writer
/// cannot be replaced until the coordinator has confirmed and recorded retirement.
pub(crate) fn install_writer(
    client: &mut Client,
    lease: ZoneLease,
    expected_epoch: u64,
) -> Result<StreamFence, StoreError> {
    if lease.epoch == 0 || lease.epoch <= expected_epoch {
        return Err(failure("writer epoch must advance"));
    }
    let mut transaction = client.transaction().map_err(sql)?;
    let zone = i64::from(lease.zone_id.get());
    transaction
        .execute(
            "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
            &[&format!("mmorpg-zone-stream:{zone}")],
        )
        .map_err(sql)?;
    let old = transaction.query_opt("SELECT writer_epoch, retired FROM mmorpg_persistence.streams WHERE zone_id=$1 FOR UPDATE", &[&zone]).map_err(sql)?;
    match old {
        None if expected_epoch == 0 => {
            transaction.execute("INSERT INTO mmorpg_persistence.streams (zone_id,writer_host,writer_epoch) VALUES ($1,$2,$3)", &[&zone, &lease.host_id.as_str(), &integer(lease.epoch)?]).map_err(sql)?;
        }
        Some(row) if unsigned(row.get(0))? == expected_epoch && row.get::<_, bool>(1) => {
            transaction.execute("UPDATE mmorpg_persistence.streams SET writer_host=$2,writer_epoch=$3,retired=false WHERE zone_id=$1", &[&zone, &lease.host_id.as_str(), &integer(lease.epoch)?]).map_err(sql)?;
        }
        _ => {
            return Err(failure(
                "previous zone writer is current or was not confirmed retired",
            ));
        }
    }
    let row = transaction
        .query_one(
            "SELECT world_id::text,timeline FROM mmorpg_persistence.streams WHERE zone_id=$1",
            &[&zone],
        )
        .map_err(sql)?;
    let fence = StreamFence {
        lease,
        world_id: row.get(0),
        timeline: unsigned(row.get(1))?,
    };
    transaction.commit().map_err(sql)?;
    Ok(fence)
}
pub(crate) fn record_retirement(
    client: &mut Client,
    fence: &StreamFence,
) -> Result<(), StoreError> {
    let changed = client.execute("UPDATE mmorpg_persistence.streams SET retired=true WHERE zone_id=$1 AND writer_host=$2 AND writer_epoch=$3 AND timeline=$4 AND world_id=$5::text::uuid AND NOT retired", &[
        &i64::from(fence.lease.zone_id.get()), &fence.lease.host_id.as_str(), &integer(fence.lease.epoch)?, &integer(fence.timeline)?, &fence.world_id,
    ]).map_err(sql)?;
    if changed != 1 {
        return Err(failure("stale retirement fence"));
    }
    Ok(())
}

pub(crate) struct PostgresJournal {
    entries: PostgresEntries,
    fence: StreamFence,
    limits: JournalLimits,
    replication: Option<ReplicaBarrier>,
    cancelled: Arc<AtomicBool>,
}
impl PostgresJournal {
    pub(crate) fn connect_development(
        url: &str,
        fence: StreamFence,
        timeout: Duration,
        limits: JournalLimits,
    ) -> Result<Self, StoreError> {
        let mut entries = PostgresEntries::connect(url, timeout)?;
        entries.transaction(|store| {
            store.backend().with_sql(|transaction| {
                let version: Option<i32> = transaction
                    .query_one(
                        "SELECT max(version) FROM mmorpg_persistence.migrations",
                        &[],
                    )
                    .map_err(sql)?
                    .get(0);
                if version != Some(VERSION) {
                    return Err(corrupt("unsupported persistence schema"));
                }
                check_fence(transaction, &fence, false)?;
                Ok(())
            })
        })?;
        Ok(Self {
            entries,
            fence,
            limits: limits.validate()?,
            replication: None,
            cancelled: Arc::new(AtomicBool::new(false)),
        })
    }
    /// Production durability requires an explicitly named physical standby.
    /// A successful call confirms every preceding mutation on that standby;
    /// there is no fallback to development/local durability.
    pub(crate) fn connect_replicated(
        primary_url: &str,
        standby_url: &str,
        fence: StreamFence,
        timeout: Duration,
        limits: JournalLimits,
    ) -> Result<Self, StoreError> {
        let mut journal = Self::connect_development(primary_url, fence, timeout, limits)?;
        journal.entries.transaction(|store| {
            store.backend().with_sql(|transaction| {
                transaction
                    .batch_execute("SET synchronous_commit=on")
                    .map_err(sql)
            })
        })?;
        let mut replication = ReplicaBarrier::connect(primary_url, standby_url, timeout)?;
        replication.confirm(&AtomicBool::new(false))?;
        journal.replication = Some(replication);
        Ok(journal)
    }

    pub(super) fn set_cancellation(&mut self, cancelled: Arc<AtomicBool>) {
        self.cancelled = cancelled;
    }
    pub(super) fn cancel_tokens(&self) -> Vec<::postgres::CancelToken> {
        let mut tokens = vec![self.entries.cancel_token()];
        if let Some(replication) = &self.replication {
            tokens.extend(replication.cancel_tokens());
        }
        tokens
    }
    fn check_cancelled(&self) -> Result<(), StoreError> {
        if self.cancelled.load(Ordering::Acquire) {
            Err(failure("persistence worker cancelled; outcome unresolved"))
        } else {
            Ok(())
        }
    }
    fn confirm_commit(&mut self) -> Result<(), StoreError> {
        self.check_cancelled()?;
        if let Some(replication) = &mut self.replication {
            replication.confirm(&self.cancelled)?;
        }
        Ok(())
    }

    pub(crate) fn characters<R>(
        &mut self,
        operation: impl FnOnce(
            &mut CharacterStore<PostgresTransactionEntries<'_>>,
        ) -> Result<R, StoreError>,
    ) -> Result<R, StoreError> {
        self.check_cancelled()?;
        let result = self.entries.transaction(|store| {
            store
                .backend()
                .with_sql(|transaction| check_fence(transaction, &self.fence, true).map(|_| ()))?;
            operation(store)
        })?;
        self.confirm_commit()?;
        Ok(result)
    }
    pub(crate) fn commit_tick(
        &mut self,
        expected_position: u64,
        batch: &SnapshotCommitBatch,
        writes: &[CharacterWrite],
    ) -> Result<u64, StoreError> {
        self.check_cancelled()?;
        let mut previous = None;
        for write in writes {
            if previous.is_some_and(|id| id >= &write.lease.character_id)
                || write.lease.character_id != write.record.character_id
                || write.lease.holder.zone_id != self.fence.lease.zone_id
                || write.lease.holder.zone_epoch != self.fence.lease.epoch
            {
                return Err(failure(
                    "invalid character write identity, order or zone fence",
                ));
            }
            previous = Some(&write.lease.character_id);
        }
        let replay = batch
            .journal
            .encode()
            .map_err(|error| corrupt(error.to_string()))?;
        let sessions =
            serde_json::to_vec(&SessionEnvelope::capture(&batch.checkpoint)).map_err(sql)?;
        let position = expected_position
            .checked_add(1)
            .ok_or_else(|| failure("journal positions exhausted"))?;
        let payload_digest = digest_payload(
            &self.fence,
            position,
            batch.snapshot.tick,
            &replay,
            &sessions,
        );
        let commit_digest = digest_commit(payload_digest, writes)?;
        let bytes = (replay.len() as u64)
            .checked_add(sessions.len() as u64)
            .and_then(|count| count.checked_add(64))
            .ok_or_else(|| failure("journal length overflow"))?;
        if bytes > self.limits.max_batch_bytes {
            return Err(failure("journal batch byte limit reached"));
        }
        let limits = self.limits;
        self.entries.transaction(|store| {
            let row = store.backend().with_sql(|transaction| check_fence(transaction, &self.fence, true))?;
            let last_position = unsigned(row.get("last_position"))?;
            if last_position == position {
                let same = store.backend().with_sql(|transaction| Ok(transaction.query_opt("SELECT commit_digest FROM mmorpg_persistence.journal WHERE zone_id=$1 AND timeline=$2 AND position=$3", &[
                    &i64::from(self.fence.lease.zone_id.get()), &integer(self.fence.timeline)?, &integer(position)?,
                ]).map_err(sql)?.is_some_and(|row| row.get::<_, Vec<u8>>(0) == commit_digest)))?;
                if !same { return Err(failure("conflicting or pruned journal retry")); }
                return Ok(position);
            }
            if last_position != expected_position { return Err(failure("journal position changed")); }
            let last_tick = unsigned(row.get("last_tick"))?;
            if batch.snapshot.tick != last_tick.checked_add(1).ok_or_else(|| failure("zone tick exhausted"))? { return Err(failure("journal tick is not the next authoritative tick")); }
            let checkpoint_tick = row.get::<_, Option<i64>>("checkpoint_tick").ok_or_else(|| failure("zone has no committed checkpoint"))?;
            let retained = unsigned(row.get("retained_bytes"))?.checked_add(bytes).ok_or_else(|| failure("journal length overflow"))?;
            let count = unsigned(row.get("retained_batches"))?.checked_add(1).ok_or_else(|| failure("journal count overflow"))?;
            if retained > limits.max_retained_bytes || count > limits.max_retained_batches
                || batch.snapshot.tick.checked_sub(unsigned(checkpoint_tick)?).is_none_or(|ticks| ticks > limits.max_replay_ticks) { return Err(failure("retained journal/replay limit reached")); }
            let mut previous: Option<&CharacterId> = None;
            for write in writes {
                if previous.is_some_and(|id| id >= &write.lease.character_id) || write.lease.holder.zone_id != self.fence.lease.zone_id || write.lease.holder.zone_epoch != self.fence.lease.epoch {
                    return Err(failure("invalid character write order or zone fence"));
                }
                previous = Some(&write.lease.character_id);
                let revision = store.commit_at_revision(&write.lease, write.expected_revision, &write.record)?;
                if write.release { store.release(&write.lease, None)?; }
                for grant in &write.grants {
                    store.backend().with_sql(|transaction| {
                        let key = grant.reward_key();
                        let encoded = key.map(mmorpg_protocol::encode_reward_receipt).transpose().map_err(sql)?;
                        let source = key.and_then(mmorpg_core::RewardKey::creature_life);
                        let creature = source.map(|(id, _)| i64::from(id.get()));
                        let life = source.map(|(_, life)| integer(life)).transpose()?;
                        let world_owned = key.is_some_and(mmorpg_core::RewardKey::world_owned);
                        let inserted = transaction.execute("INSERT INTO mmorpg_persistence.grants (grant_id,character_id,zone_id,world_id,timeline,journal_position,record_revision,reward_key,source_creature,source_life,world_owned) VALUES ($1,$2,$3,$4::text::uuid,$5,$6,$7,$8,$9,$10,$11) ON CONFLICT DO NOTHING", &[
                            &grant.as_str(), &write.lease.character_id.as_str(), &i64::from(self.fence.lease.zone_id.get()), &self.fence.world_id,
                            &integer(self.fence.timeline)?, &integer(position)?, &integer(revision)?, &encoded, &creature, &life, &world_owned,
                        ]).map_err(sql)?;
                        if inserted != 1 { return Err(failure("grant already committed; admission/recovery must load its durable deduplication state")); }
                        Ok(())
                    })?;
                }
            }
            store.backend().with_sql(|transaction| {
                transaction.execute("INSERT INTO mmorpg_persistence.journal (zone_id,timeline,position,tick,writer_epoch,replay,sessions,digest,commit_digest,byte_length) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)", &[
                    &i64::from(self.fence.lease.zone_id.get()), &integer(self.fence.timeline)?, &integer(position)?, &integer(batch.snapshot.tick)?, &integer(self.fence.lease.epoch)?,
                    &replay, &sessions, &payload_digest.as_slice(), &commit_digest.as_slice(), &integer(bytes)?,
                ]).map_err(sql)?;
                transaction.execute("UPDATE mmorpg_persistence.streams SET last_position=$2,last_tick=$3,retained_bytes=$4,retained_batches=$5 WHERE zone_id=$1", &[
                    &i64::from(self.fence.lease.zone_id.get()), &integer(position)?, &integer(batch.snapshot.tick)?, &integer(retained)?, &integer(count)?,
                ]).map_err(sql)?;
                Ok(())
            })?;
            Ok(position)
        })?;
        self.confirm_commit()?;
        Ok(position)
    }

    /// The object upload/readback verifier is the only production caller. This
    /// SQL layer publishes metadata, never treats a successful PUT as a checkpoint.
    pub(crate) fn publish_checkpoint(
        &mut self,
        verified: &super::checkpoints::VerifiedCheckpoint,
    ) -> Result<(), StoreError> {
        self.check_cancelled()?;
        let reference = verified.reference();
        reference.validate(&self.fence)?;
        let encoded = serde_json::to_vec(reference).map_err(sql)?;
        self.entries.transaction(|store| store.backend().with_sql(|transaction| {
            let stream = check_fence(transaction, &self.fence, true)?;
            let position = unsigned(stream.get("last_position"))?;
            let previous: Option<i64> = stream.get("checkpoint_position");
            if previous == Some(integer(reference.position)?) {
                if stream.get::<_, Option<Vec<u8>>>("checkpoint_ref").as_deref() == Some(encoded.as_slice()) {
                    return Ok(());
                }
                return Err(failure("checkpoint position already has a different immutable reference"));
            }
            if reference.position > position || previous.is_some_and(|old| reference.position < old as u64) { return Err(failure("checkpoint reference moved outside committed history")); }
            if reference.position > 0 {
                let row = transaction.query_opt("SELECT tick FROM mmorpg_persistence.journal WHERE zone_id=$1 AND timeline=$2 AND position=$3", &[
                    &i64::from(self.fence.lease.zone_id.get()), &integer(self.fence.timeline)?, &integer(reference.position)?,
                ]).map_err(sql)?.ok_or_else(|| failure("checkpoint journal anchor is unavailable"))?;
                if unsigned(row.get(0))? != reference.tick { return Err(failure("checkpoint tick differs from journal anchor")); }
            } else if position != 0 { return Err(failure("initial checkpoint cannot replace an active journal")); }
            transaction.execute("UPDATE mmorpg_persistence.streams SET checkpoint_position=$2,checkpoint_tick=$3,checkpoint_ref=$4,last_tick=CASE WHEN last_position=0 THEN $3 ELSE last_tick END WHERE zone_id=$1", &[
                &i64::from(self.fence.lease.zone_id.get()), &integer(reference.position)?, &integer(reference.tick)?, &encoded,
            ]).map_err(sql)?;
            Ok(())
        }))?;
        self.confirm_commit()
    }
    pub(crate) fn prune(&mut self) -> Result<(), StoreError> {
        self.check_cancelled()?;
        self.entries.transaction(|store| store.backend().with_sql(|transaction| {
            let row = check_fence(transaction, &self.fence, true)?;
            let through: Option<i64> = row.get("checkpoint_position");
            let through = through.ok_or_else(|| failure("cannot prune without a committed checkpoint"))?;
            let removed = transaction.query_one("WITH removed AS (DELETE FROM mmorpg_persistence.journal WHERE zone_id=$1 AND timeline=$2 AND position <= $3 RETURNING byte_length) SELECT count(*)::bigint,coalesce(sum(byte_length),0)::bigint FROM removed", &[
                &i64::from(self.fence.lease.zone_id.get()), &integer(self.fence.timeline)?, &through,
            ]).map_err(sql)?;
            let count: i64 = removed.get(0); let bytes: i64 = removed.get(1);
            transaction.execute("UPDATE mmorpg_persistence.streams SET retained_batches=retained_batches-$2,retained_bytes=retained_bytes-$3 WHERE zone_id=$1", &[&i64::from(self.fence.lease.zone_id.get()), &count, &bytes]).map_err(sql)?;
            Ok(())
        }))?;
        self.confirm_commit()
    }
    /// Capture a stable reference before object I/O. Recovery verifies that it
    /// is still current when reading the suffix, so concurrent pruning cannot
    /// turn a missing range into a successful restore.
    pub(crate) fn checkpoint_reference(&mut self) -> Result<CheckpointReference, StoreError> {
        self.entries.transaction(|store| {
            store.backend().with_sql(|transaction| {
                let row = check_fence(transaction, &self.fence, false)?;
                let bytes: Option<Vec<u8>> = row.get("checkpoint_ref");
                let reference: CheckpointReference = serde_json::from_slice(
                    &bytes.ok_or_else(|| failure("zone has no committed checkpoint"))?,
                )
                .map_err(sql)?;
                reference.validate(&self.fence)?;
                if row.get::<_, Option<i64>>("checkpoint_position")
                    != Some(integer(reference.position)?)
                    || row.get::<_, Option<i64>>("checkpoint_tick")
                        != Some(integer(reference.tick)?)
                {
                    return Err(corrupt("checkpoint reference differs from stream cursor"));
                }
                Ok(reference)
            })
        })
    }

    /// The caller first verifies and decompresses the immutable object. This
    /// method only appends committed, contiguous, digest-checked journal rows;
    /// game-server remains the sole interpreter of commands and session state.
    pub(crate) fn recover_suffix(
        &mut self,
        reference: &CheckpointReference,
        mut image: RecoveryImage,
    ) -> Result<(u64, RecoveryImage), StoreError> {
        reference.validate(&self.fence)?;
        if image.current_tick != reference.tick {
            return Err(corrupt("checkpoint object tick differs from reference"));
        }
        self.entries.transaction(|store| store.backend().with_sql(|transaction| {
            let stream = check_fence(transaction, &self.fence, false)?;
            let encoded = serde_json::to_vec(reference).map_err(sql)?;
            if stream.get::<_, Option<Vec<u8>>>("checkpoint_ref").as_deref() != Some(encoded.as_slice()) {
                return Err(failure("checkpoint changed while fetching object; retry recovery"));
            }
            let last = unsigned(stream.get("last_position"))?;
            let last_tick = unsigned(stream.get("last_tick"))?;
            let count = last.checked_sub(reference.position).ok_or_else(|| corrupt("checkpoint ahead of journal"))?;
            let ticks = last_tick.checked_sub(reference.tick).ok_or_else(|| corrupt("checkpoint tick ahead of journal"))?;
            if count > self.limits.max_retained_batches || ticks > self.limits.max_replay_ticks || count != ticks {
                return Err(corrupt("journal suffix exceeds recovery limits or has inconsistent ticks"));
            }
            // Inspect lengths before materializing bytea values. Holding the
            // stream share lock excludes normal journal commit/prune writers.
            let bounds = transaction.query_one(
                "SELECT count(*)::bigint,coalesce(sum(octet_length(replay)::bigint+octet_length(sessions)::bigint+64),0)::bigint,coalesce(max(octet_length(replay)::bigint+octet_length(sessions)::bigint+64),0)::bigint FROM mmorpg_persistence.journal WHERE zone_id=$1 AND timeline=$2 AND position>$3",
                &[&i64::from(self.fence.lease.zone_id.get()), &integer(self.fence.timeline)?, &integer(reference.position)?],
            ).map_err(sql)?;
            if unsigned(bounds.get(0))? != count
                || unsigned(bounds.get(1))? > self.limits.max_retained_bytes
                || unsigned(bounds.get(2))? > self.limits.max_batch_bytes {
                return Err(corrupt("missing or oversized journal suffix"));
            }
            let rows = transaction.query(
                "SELECT position,tick,writer_epoch,replay,sessions,digest,byte_length FROM mmorpg_persistence.journal WHERE zone_id=$1 AND timeline=$2 AND position>$3 ORDER BY position",
                &[&i64::from(self.fence.lease.zone_id.get()), &integer(self.fence.timeline)?, &integer(reference.position)?],
            ).map_err(sql)?;
            let mut position = reference.position;
            for row in rows {
                position = position.checked_add(1).ok_or_else(|| corrupt("journal position overflow"))?;
                let tick = image.current_tick.checked_add(1).ok_or_else(|| corrupt("journal tick overflow"))?;
                let replay: Vec<u8> = row.get("replay");
                let sessions: Vec<u8> = row.get("sessions");
                let mut writer = self.fence.clone();
                writer.lease.epoch = unsigned(row.get("writer_epoch"))?;
                if unsigned(row.get("position"))? != position || unsigned(row.get("tick"))? != tick
                    || writer.lease.epoch == 0 || writer.lease.epoch > self.fence.lease.epoch
                    || unsigned(row.get("byte_length"))? != replay.len() as u64 + sessions.len() as u64 + 64
                    || row.get::<_, Vec<u8>>("digest") != digest_payload(&writer, position, tick, &replay, &sessions) {
                    return Err(corrupt("journal gap, context or digest mismatch"));
                }
                let suffix = ReplayLog::decode(&replay).map_err(sql)?;
                image.replay.append_journal(&suffix).map_err(sql)?;
                let metadata: SessionEnvelope = serde_json::from_slice(&sessions).map_err(sql)?;
                let (grace, sessions) = metadata.restore()?;
                image.current_tick = tick;
                image.reconnect_grace_ticks = grace;
                image.sessions = sessions;
            }
            if position != last || image.current_tick != last_tick {
                return Err(corrupt("journal suffix does not reach committed cursor"));
            }
            // Validate shared structural/session invariants before returning.
            image.encode().map_err(sql)?;
            Ok((position, image))
        }))
    }

    /// Load exact receipts relevant to the current/future lives, bounded before
    /// allocation. Quest completion receipts are character-global across worlds.
    pub(crate) fn reward_receipts(
        &mut self,
        character: &CharacterId,
        lives: &std::collections::BTreeMap<mmorpg_core::CreatureId, u64>,
        maximum: u16,
    ) -> Result<Vec<mmorpg_core::RewardKey>, StoreError> {
        if maximum == 0 {
            return Err(failure("reward receipt limit must be positive"));
        }
        let creatures: Vec<i64> = lives.keys().map(|id| i64::from(id.get())).collect();
        let generations: Vec<i64> = lives
            .values()
            .copied()
            .map(integer)
            .collect::<Result<_, _>>()?;
        self.entries.transaction(|store| store.backend().with_sql(|transaction| {
            check_fence(transaction, &self.fence, false)?;
            let rows = transaction.query(
                "SELECT grant_id,character_id,world_id::text,zone_id,source_creature,source_life,world_owned,CASE WHEN octet_length(reward_key)::bigint<=$7 THEN reward_key ELSE NULL END AS key FROM mmorpg_persistence.grants WHERE reward_key IS NOT NULL AND ((world_id=$1::text::uuid AND zone_id=$2 AND (world_owned OR character_id=$3) AND EXISTS (SELECT 1 FROM unnest($4::bigint[],$5::bigint[]) AS lives(creature,life) WHERE source_creature=lives.creature AND source_life>=lives.life)) OR (character_id=$3 AND source_creature IS NULL)) ORDER BY grant_id LIMIT $6",
                &[&self.fence.world_id, &i64::from(self.fence.lease.zone_id.get()), &character.as_str(), &creatures, &generations, &(i64::from(maximum)+1), &integer(mmorpg_protocol::MAX_REWARD_RECEIPT_BYTES as u64)?],
            ).map_err(sql)?;
            if rows.len() > usize::from(maximum) { return Err(failure("reward receipt admission limit reached")); }
            let mut keys = std::collections::BTreeSet::new();
            for row in rows {
                let bytes: Option<Vec<u8>> = row.get("key");
                let key = mmorpg_protocol::decode_reward_receipt(&bytes.ok_or_else(|| corrupt("oversized reward receipt metadata"))?).map_err(sql)?;
                let owner = CharacterId::new(row.get::<_, String>(1)).map_err(sql)?;
                let world: String = row.get(2);
                let zone: u32 = unsigned(row.get(3))?.try_into().map_err(sql)?;
                let expected = GrantId::for_reward(&world, mmorpg_core::ZoneId::new(zone), &owner, key).map_err(sql)?;
                let source = key.creature_life();
                if row.get::<_, String>(0) != expected.as_str()
                    || row.get::<_, bool>("world_owned") != key.world_owned()
                    || row.get::<_, Option<i64>>("source_creature") != source.map(|(id, _)| i64::from(id.get()))
                    || row.get::<_, Option<i64>>("source_life") != source.map(|(_, life)| integer(life)).transpose()? {
                    return Err(corrupt("reward receipt metadata differs from stable grant identity"));
                }
                keys.insert(key);
            }
            Ok(keys.into_iter().collect())
        }))
    }

    pub(crate) fn grant_ids(
        &mut self,
        character: Option<&CharacterId>,
    ) -> Result<Vec<GrantId>, StoreError> {
        self.entries.transaction(|store| store.backend().with_sql(|transaction| {
            check_fence(transaction, &self.fence, false)?;
            let key = character.map(CharacterId::as_str);
            transaction.query("SELECT grant_id FROM mmorpg_persistence.grants WHERE world_id=$1::text::uuid AND ($2::text IS NULL OR character_id=$2) ORDER BY grant_id", &[&self.fence.world_id, &key]).map_err(sql)?.into_iter().map(|row| GrantId::new(row.get::<_, String>(0)).map_err(sql)).collect()
        }))
    }
}

fn check_fence(
    transaction: &mut Transaction<'_>,
    fence: &StreamFence,
    write: bool,
) -> Result<Row, StoreError> {
    let query = if write {
        "SELECT *,world_id::text AS world_key FROM mmorpg_persistence.streams WHERE zone_id=$1 FOR UPDATE"
    } else {
        "SELECT *,world_id::text AS world_key FROM mmorpg_persistence.streams WHERE zone_id=$1 FOR SHARE"
    };
    let row = transaction
        .query_opt(query, &[&i64::from(fence.lease.zone_id.get())])
        .map_err(sql)?
        .ok_or_else(|| failure("unknown zone journal"))?;
    if unsigned(row.get("writer_epoch"))? != fence.lease.epoch
        || row.get::<_, String>("writer_host") != fence.lease.host_id.as_str()
        || unsigned(row.get("timeline"))? != fence.timeline
        || row.get::<_, String>("world_key") != fence.world_id
        || row.get::<_, bool>("retired")
    {
        return Err(failure("stale zone journal fence"));
    }
    Ok(row)
}
fn digest_payload(
    fence: &StreamFence,
    position: u64,
    tick: u64,
    replay: &[u8],
    sessions: &[u8],
) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"mmorpg/journal/v1\0");
    digest.update(fence.lease.zone_id.get().to_be_bytes());
    for value in [fence.timeline, fence.lease.epoch, position, tick] {
        digest.update(value.to_be_bytes());
    }
    for bytes in [fence.world_id.as_bytes(), replay, sessions] {
        digest.update((bytes.len() as u64).to_be_bytes());
        digest.update(bytes);
    }
    digest.finalize().into()
}
fn digest_commit(payload: [u8; 32], writes: &[CharacterWrite]) -> Result<[u8; 32], StoreError> {
    let mut digest = Sha256::new();
    digest.update(b"mmorpg/character-journal-commit/v1\0");
    digest.update(payload);
    digest.update((writes.len() as u64).to_be_bytes());
    for write in writes {
        digest.update([u8::from(write.release)]);
        for value in [
            write.lease.generation,
            write.expected_revision,
            write.lease.holder.zone_epoch,
        ] {
            digest.update(value.to_be_bytes());
        }
        digest.update(write.lease.holder.zone_id.get().to_be_bytes());
        digest.update((write.lease.character_id.as_str().len() as u64).to_be_bytes());
        digest.update(write.lease.character_id.as_str().as_bytes());
        let record = mmorpg_protocol::encode_character_record(&write.record).map_err(sql)?;
        digest.update((record.len() as u64).to_be_bytes());
        digest.update(record);
        digest.update((write.grants.len() as u64).to_be_bytes());
        for grant in &write.grants {
            let key = grant
                .reward_key()
                .map(mmorpg_protocol::encode_reward_receipt)
                .transpose()
                .map_err(sql)?
                .unwrap_or_default();
            digest.update((key.len() as u64).to_be_bytes());
            digest.update(key);
            digest.update((grant.as_str().len() as u64).to_be_bytes());
            digest.update(grant.as_str().as_bytes());
        }
    }
    Ok(digest.finalize().into())
}
fn integer(value: u64) -> Result<i64, StoreError> {
    value.try_into().map_err(sql)
}
fn unsigned(value: i64) -> Result<u64, StoreError> {
    value
        .try_into()
        .map_err(|_| corrupt("negative persisted counter"))
}
fn failure(message: impl Into<String>) -> StoreError {
    StoreError::Backend(message.into())
}
fn corrupt(message: impl Into<String>) -> StoreError {
    StoreError::Corrupt(message.into())
}
fn sql(error: impl std::fmt::Display) -> StoreError {
    failure(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ZoneGameServerAdapter;
    use game_server::{MatchId, MatchRuntime};
    use mmorpg_characters::LeaseHolder;
    use mmorpg_control_plane::HostId;
    use mmorpg_core::{ClassChoice, PlayerClass, Sex, ZoneId, ZoneSimulation};

    #[test]
    fn postgres_journal_recovers_committed_suffix_and_fences_retired_writer() {
        let Ok(url) = std::env::var("MMORPG_CHARACTERS_POSTGRES_URL") else {
            return;
        };
        let mut coordinator = Client::connect(&url, postgres::NoTls).unwrap();
        migrate(&mut coordinator).unwrap();
        migrate(&mut coordinator).unwrap();
        coordinator
            .execute("SELECT pg_advisory_lock($1)", &[&0x4d4d_5a4a_5445_5354_i64])
            .unwrap();
        // Serialize stream allocation across invocations of this database test.
        let zone_number: i64 = coordinator
            .query_one(
                "SELECT coalesce(max(zone_id),1000)+1 FROM mmorpg_persistence.streams",
                &[],
            )
            .unwrap()
            .get(0);
        let zone = ZoneId::new(zone_number.try_into().unwrap());
        let adapter = || ZoneGameServerAdapter::new(zone);
        let mut baseline = MatchRuntime::new_with_replay_capture(adapter(), 30);
        let image = baseline.checkpoint_replay().unwrap();
        let lease = ZoneLease {
            zone_id: zone,
            host_id: HostId::new("journal-test").unwrap(),
            epoch: 1,
            expires_at_tick: 1000,
        };
        let fence = install_writer(&mut coordinator, lease.clone(), 0).unwrap();
        assert!(
            install_writer(
                &mut coordinator,
                ZoneLease {
                    epoch: 2,
                    ..lease.clone()
                },
                1
            )
            .is_err()
        );
        let limits = JournalLimits {
            max_batch_bytes: 1024 * 1024,
            max_retained_bytes: 2 * 1024 * 1024,
            max_retained_batches: 2,
            max_replay_ticks: 2,
        };
        let mut journal = if let Ok(standby) = std::env::var("MMORPG_POSTGRES_STANDBY_URL") {
            PostgresJournal::connect_replicated(
                &url,
                &standby,
                fence.clone(),
                Duration::from_secs(5),
                limits,
            )
            .unwrap()
        } else {
            PostgresJournal::connect_development(
                &url,
                fence.clone(),
                Duration::from_secs(5),
                limits,
            )
            .unwrap()
        };
        let reference = CheckpointReference {
            version: 1,
            zone_id: zone.get(),
            world_id: fence.world_id.clone(),
            timeline: fence.timeline,
            position: 0,
            tick: 0,
            object_key: "test/checkpoint.zst".into(),
            sha256: [1; 32],
            byte_length: 1,
            content_fingerprint: 0,
            canonical_version: 1,
        };
        journal
            .publish_checkpoint(
                &super::super::checkpoints::VerifiedCheckpoint::for_sql_test(reference.clone()),
            )
            .unwrap();
        assert_eq!(journal.checkpoint_reference().unwrap(), reference);
        let mut live = MatchRuntime::new_with_replay_capture(adapter(), 30);
        let character = CharacterId::new(format!("journal-test-{zone_number}")).unwrap();
        let acquired = journal
            .characters(|store| {
                store.acquire(
                    &character,
                    LeaseHolder {
                        zone_id: zone,
                        zone_epoch: 1,
                    },
                )
            })
            .unwrap();
        let mut simulation =
            ZoneSimulation::with_content(zone, mmorpg_core::greyhaven_vale::content()).unwrap();
        simulation
            .create_character(
                1,
                ClassChoice {
                    class: PlayerClass::Warden,
                    sex: Sex::Male,
                },
            )
            .unwrap();
        let mut record = simulation.capture_character(1, character.clone()).unwrap();
        record.copper = 123;
        let writes = [CharacterWrite {
            lease: acquired.lease,
            expected_revision: 0,
            record: record.clone(),
            release: false,
            grants: [
                GrantId::new(format!("test/{zone_number}/reward")).unwrap(),
                GrantId::for_reward(
                    &fence.world_id,
                    zone,
                    &character,
                    mmorpg_core::RewardKey::LootMoney {
                        creature: mmorpg_core::CreatureId::new(7),
                        life: 2,
                    },
                )
                .unwrap(),
                GrantId::for_reward(
                    &fence.world_id,
                    zone,
                    &character,
                    mmorpg_core::RewardKey::KillExperience {
                        creature: mmorpg_core::CreatureId::new(7),
                        life: 2,
                    },
                )
                .unwrap(),
                GrantId::for_reward(
                    &fence.world_id,
                    zone,
                    &character,
                    mmorpg_core::RewardKey::QuestCompletion {
                        quest: mmorpg_core::QuestId::new(1),
                    },
                )
                .unwrap(),
            ]
            .into(),
        }];
        let snapshot = live.advance_tick().unwrap();
        let batch = SnapshotCommitBatch {
            match_id: MatchId::new(format!("zone-{}", zone.get())).unwrap(),
            snapshot: snapshot.clone(),
            journal: live.replay_log().unwrap().clone(),
            checkpoint: live.checkpoint_replay().unwrap(),
        };
        assert_eq!(journal.commit_tick(0, &batch, &writes).unwrap(), 1);
        assert_eq!(journal.commit_tick(0, &batch, &writes).unwrap(), 1);
        assert_eq!(
            journal.grant_ids(Some(&character)).unwrap(),
            writes[0].grants.iter().cloned().collect::<Vec<_>>()
        );
        let lives = [(mmorpg_core::CreatureId::new(7), 2)].into();
        assert_eq!(
            journal
                .reward_receipts(&character, &lives, 4)
                .unwrap()
                .len(),
            3
        );
        assert!(journal.reward_receipts(&character, &lives, 2).is_err());
        let later = [(mmorpg_core::CreatureId::new(7), 3)].into();
        assert_eq!(
            journal.reward_receipts(&character, &later, 4).unwrap(),
            vec![mmorpg_core::RewardKey::QuestCompletion {
                quest: mmorpg_core::QuestId::new(1)
            }]
        );
        let other = CharacterId::new("other-receipt-owner").unwrap();
        assert_eq!(
            journal.reward_receipts(&other, &lives, 4).unwrap(),
            vec![mmorpg_core::RewardKey::LootMoney {
                creature: mmorpg_core::CreatureId::new(7),
                life: 2
            }]
        );
        let committed = journal
            .characters(|store| store.committed(&character))
            .unwrap()
            .unwrap();
        assert_eq!(committed.record, record);
        assert_eq!(committed.revision, 1);
        if let Ok(standby) = std::env::var("MMORPG_POSTGRES_STANDBY_URL") {
            let profiles = CharacterStore::new(
                PostgresEntries::connect(&standby, Duration::from_secs(5)).unwrap(),
            );
            assert_eq!(
                profiles.committed(&character).unwrap().unwrap().record,
                record
            );
        }
        let (position, recovered) = journal.recover_suffix(&reference, image.clone()).unwrap();
        assert_eq!(position, 1);
        let restored = MatchRuntime::restore_from_recovery(adapter(), recovered).unwrap();
        assert_eq!(restored.snapshot().unwrap(), snapshot);
        coordinator.execute("UPDATE mmorpg_persistence.journal SET digest=decode(repeat('00',32),'hex') WHERE zone_id=$1", &[&zone_number]).unwrap();
        assert!(journal.recover_suffix(&reference, image).is_err());
        let latest = CheckpointReference {
            position: 1,
            tick: 1,
            object_key: "test/checkpoint-1.zst".into(),
            ..reference
        };
        journal
            .publish_checkpoint(
                &super::super::checkpoints::VerifiedCheckpoint::for_sql_test(latest.clone()),
            )
            .unwrap();
        journal.prune().unwrap();
        journal
            .publish_checkpoint(
                &super::super::checkpoints::VerifiedCheckpoint::for_sql_test(latest.clone()),
            )
            .unwrap();
        assert_eq!(
            journal.grant_ids(Some(&character)).unwrap().len(),
            writes[0].grants.len()
        );
        let retained: i64 = coordinator
            .query_one(
                "SELECT retained_batches FROM mmorpg_persistence.streams WHERE zone_id=$1",
                &[&zone_number],
            )
            .unwrap()
            .get(0);
        assert_eq!(retained, 0);
        record_retirement(&mut coordinator, &fence).unwrap();
        assert!(journal.commit_tick(0, &batch, &[]).is_err());
        let successor =
            install_writer(&mut coordinator, ZoneLease { epoch: 2, ..lease }, 1).unwrap();
        assert_eq!(successor.world_id, fence.world_id);
        assert!(journal.checkpoint_reference().is_err());
    }
}
