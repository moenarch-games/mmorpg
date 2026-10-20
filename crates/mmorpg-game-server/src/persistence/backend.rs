//! Global backend composition (TECH-006): independent directory commands and
//! character queries, sharing PostgreSQL but never calling a zone simulation.
//! All I/O runs on an application-owned blocking worker. This is an internal
//! service port; authentication, public routing and primary promotion are outside
//! this module. Only the configured confirmed standby may supply profile facts.
use super::{
    postgres::{self, StreamFence},
    replication::ReplicaBarrier,
};
use ::postgres::{Client, Config, NoTls};
use mmorpg_characters::{Committed, PostgresEntries, StoreError};
use mmorpg_control_plane::{
    HostId, LocalZonePermit,
    postgres::{self as directory, UnconfirmedZoneGrant},
};
use mmorpg_core::{CharacterId, ZoneId};
use std::{
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

#[derive(Clone)]
pub(crate) struct BackendConfig {
    pub coordinator_primary_url: String,
    pub coordinator_standby_url: String,
    pub character_standby_url: String,
    pub timeout: Duration,
}

/// Constructed only after the directory grant and journal fence are committed
/// together and the configured standby has durably received and replayed them.
pub(crate) struct ConfirmedZoneGrant {
    grant: UnconfirmedZoneGrant,
    fence: StreamFence,
    permit: Arc<LocalZonePermit>,
}
impl ConfirmedZoneGrant {
    pub(crate) fn fence(&self) -> &StreamFence {
        &self.fence
    }
    pub(crate) fn permit(&self) -> Arc<LocalZonePermit> {
        Arc::clone(&self.permit)
    }
}

pub(crate) struct GlobalBackend {
    coordinator: Client,
    replication: ReplicaBarrier,
    profiles: PostgresEntries,
    cancelled: Arc<AtomicBool>,
    fenced: bool,
}
impl GlobalBackend {
    pub(crate) fn connect(config: BackendConfig) -> Result<Self, StoreError> {
        if config.timeout.is_zero() {
            return Err(error("global backend timeout must be positive"));
        }
        let mut connection: Config = config.coordinator_primary_url.parse().map_err(error)?;
        connection.connect_timeout(config.timeout);
        let mut coordinator = connection.connect(NoTls).map_err(error)?;
        let millis = config.timeout.as_millis().max(1);
        coordinator
            .batch_execute(&format!(
                "SET statement_timeout={millis}; SET lock_timeout={millis}"
            ))
            .map_err(error)?;
        // Connection never performs schema changes. Operators explicitly run
        // the directory/journal migrations with the independently privileged role.
        let versions = coordinator.query_one("SELECT (SELECT max(version) FROM mmorpg_coordination.migrations),(SELECT max(version) FROM mmorpg_persistence.migrations)", &[]).map_err(error)?;
        if versions.get::<_, Option<i32>>(0) != Some(directory::SCHEMA_VERSION)
            || versions.get::<_, Option<i32>>(1) != Some(postgres::SCHEMA_VERSION)
        {
            return Err(error("unsupported global backend schema"));
        }
        let mut replication = ReplicaBarrier::connect(
            &config.coordinator_primary_url,
            &config.coordinator_standby_url,
            config.timeout,
        )?;
        replication.confirm(&AtomicBool::new(false))?;
        let profiles = PostgresEntries::connect(&config.character_standby_url, config.timeout)?;
        Ok(Self {
            coordinator,
            replication,
            profiles,
            cancelled: Arc::new(AtomicBool::new(false)),
            fenced: false,
        })
    }
    pub(super) fn set_cancellation(&mut self, flag: Arc<AtomicBool>) {
        self.cancelled = flag;
    }
    pub(super) fn cancel_tokens(&self) -> Vec<::postgres::CancelToken> {
        let mut tokens = vec![
            self.coordinator.cancel_token(),
            self.profiles.cancel_token(),
        ];
        tokens.extend(self.replication.cancel_tokens());
        tokens
    }
    pub(crate) fn assignment(
        &mut self,
        zone: ZoneId,
    ) -> Result<Option<directory::StoredAssignment>, StoreError> {
        directory::read_assignment(&mut self.coordinator, zone).map_err(error)
    }
    pub(crate) fn retired_fence(
        &mut self,
        zone: ZoneId,
        expected_epoch: Option<u64>,
    ) -> Result<StreamFence, StoreError> {
        let assignment = self
            .assignment(zone)?
            .ok_or_else(|| error("zone has no directory assignment"))?;
        if !assignment.retired
            || expected_epoch.is_some_and(|epoch| epoch != assignment.lease.epoch)
        {
            return Err(error("expected zone writer is not confirmed retired"));
        }
        let row = self.coordinator.query_one("SELECT writer_host,writer_epoch,world_id::text,timeline,retired FROM mmorpg_persistence.streams WHERE zone_id=$1", &[&i64::from(zone.get())]).map_err(error)?;
        if !row.get::<_, bool>(4)
            || row.get::<_, String>(0) != assignment.lease.host_id.as_str()
            || row.get::<_, i64>(1) != i64::try_from(assignment.lease.epoch).map_err(error)?
        {
            return Err(error("retired directory and journal assignments disagree"));
        }
        Ok(StreamFence {
            lease: assignment.lease,
            world_id: row.get(2),
            timeline: u64::try_from(row.get::<_, i64>(3)).map_err(error)?,
        })
    }
    /// Explicit operator path after the source process is stopped. It cannot
    /// resume a persisted assignment or infer retirement from lease expiry.
    pub(crate) fn confirm_stopped_assignment(
        &mut self,
        zone: ZoneId,
        expected_epoch: u64,
    ) -> Result<(), StoreError> {
        let assignment = self
            .assignment(zone)?
            .ok_or_else(|| error("unknown zone assignment"))?;
        if assignment.lease.epoch != expected_epoch {
            return Err(error("zone epoch changed"));
        }
        let row = self
            .coordinator
            .query_one(
                "SELECT world_id::text,timeline FROM mmorpg_persistence.streams WHERE zone_id=$1",
                &[&i64::from(zone.get())],
            )
            .map_err(error)?;
        let fence = StreamFence {
            lease: assignment.lease,
            world_id: row.get(0),
            timeline: u64::try_from(row.get::<_, i64>(1)).map_err(error)?,
        };
        let mut transaction = self.coordinator.transaction().map_err(error)?;
        directory::retire_in(&mut transaction, &fence.lease).map_err(error)?;
        postgres::record_retirement_in(&mut transaction, &fence)?;
        if let Err(cause) = transaction.commit() {
            self.fenced = true;
            return Err(error(cause));
        }
        self.confirm()
    }
    pub(crate) fn reset_retired(
        &mut self,
        previous: &StreamFence,
        checkpoint: &super::checkpoints::VerifiedWorldReset,
    ) -> Result<StreamFence, StoreError> {
        self.ready()?;
        self.retired_fence(previous.lease.zone_id, Some(previous.lease.epoch))?;
        let next = postgres::reset_retired_world(&mut self.coordinator, previous, checkpoint)?;
        self.confirm()?;
        Ok(next)
    }
    fn ready(&self) -> Result<(), StoreError> {
        if self.fenced {
            Err(error(
                "global backend authority is fenced after an uncertain commit",
            ))
        } else {
            Ok(())
        }
    }
    fn confirm(&mut self) -> Result<(), StoreError> {
        if let Err(cause) = self.replication.confirm(&self.cancelled) {
            self.fenced = true;
            return Err(cause);
        }
        Ok(())
    }
    pub(crate) fn grant(
        &mut self,
        zone: ZoneId,
        host: HostId,
        expected_epoch: u64,
        ttl: Duration,
    ) -> Result<ConfirmedZoneGrant, StoreError> {
        self.ready()?;
        let requested_at = Instant::now();
        let mut transaction = self.coordinator.transaction().map_err(error)?;
        let grant = directory::acquire_in(
            &mut transaction,
            zone,
            host,
            expected_epoch,
            ttl,
            requested_at,
        )
        .map_err(error)?;
        let fence =
            postgres::install_writer_in(&mut transaction, grant.lease().clone(), expected_epoch)?;
        if let Err(cause) = transaction.commit() {
            self.fenced = true;
            return Err(error(cause));
        }
        self.confirm()?;
        let permit = match LocalZonePermit::from_confirmed_lease(
            grant.lease().clone(),
            grant.requested_at(),
            grant.valid_for(),
        ) {
            Ok(permit) => Arc::new(permit),
            Err(cause) => {
                self.fenced = true;
                return Err(error(cause));
            }
        };
        Ok(ConfirmedZoneGrant {
            grant,
            fence,
            permit,
        })
    }
    pub(crate) fn renew(
        &mut self,
        active: &mut ConfirmedZoneGrant,
        ttl: Duration,
    ) -> Result<(), StoreError> {
        let result = self.renew_inner(active, ttl);
        if result.is_err() {
            let _ = active.permit.revoke();
        }
        result
    }
    fn renew_inner(
        &mut self,
        active: &mut ConfirmedZoneGrant,
        ttl: Duration,
    ) -> Result<(), StoreError> {
        self.ready()?;
        active
            .permit
            .ensure_current(active.fence.lease.zone_id)
            .map_err(error)?;
        let requested_at = Instant::now();
        let mut transaction = self.coordinator.transaction().map_err(error)?;
        let renewed = directory::renew_in(&mut transaction, &active.grant, ttl, requested_at)
            .map_err(error)?;
        if let Err(cause) = transaction.commit() {
            self.fenced = true;
            return Err(error(cause));
        }
        self.confirm()?;
        active
            .permit
            .renew_confirmed(renewed.lease(), renewed.requested_at(), renewed.valid_for())
            .map_err(error)?;
        active.fence.lease = renewed.lease().clone();
        active.grant = renewed;
        Ok(())
    }
    /// Call after stopping/joining the source host and its storage worker. The
    /// shared local permit is revoked before any ownership metadata changes.
    pub(crate) fn retire_after_stop(
        &mut self,
        active: &ConfirmedZoneGrant,
    ) -> Result<(), StoreError> {
        active.permit.revoke().map_err(error)?;
        let mut transaction = self.coordinator.transaction().map_err(error)?;
        directory::retire_in(&mut transaction, active.grant.lease()).map_err(error)?;
        postgres::record_retirement_in(&mut transaction, &active.fence)?;
        if let Err(cause) = transaction.commit() {
            self.fenced = true;
            return Err(error(cause));
        }
        self.confirm()
    }
    /// CQRS read: no zone lookup, RPC or simulation access. A fixed read-only
    /// standby snapshot excludes later unconfirmed primary writes even if this
    /// physical standby is promoted while the query is running.
    pub(crate) fn committed_character(
        &mut self,
        id: &CharacterId,
    ) -> Result<Option<Committed>, StoreError> {
        let replication = &self.replication;
        self.profiles.transaction(|store| {
            store.backend().with_sql(|transaction| {
                transaction
                    .batch_execute("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
                    .map_err(error)?;
                replication.ensure_standby_snapshot(transaction)
            })?;
            store.committed(id)
        })
    }
}
fn error(cause: impl std::fmt::Display) -> StoreError {
    StoreError::Backend(cause.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::postgres::{JournalLimits, PostgresJournal};
    use mmorpg_characters::LeaseHolder;
    use mmorpg_core::{ClassChoice, PlayerClass, Sex, ZoneSimulation, greyhaven_vale};

    fn role(url: &str, role: &str) -> String {
        let separator = if url.contains('?') { '&' } else { '?' };
        format!("{url}{separator}options=-c%20role%3D{role}")
    }
    #[test]
    fn global_backend_confirms_atomic_grants_and_reads_profiles_without_a_zone_host() {
        let (Ok(primary), Ok(standby)) = (
            std::env::var("MMORPG_CHARACTERS_POSTGRES_URL"),
            std::env::var("MMORPG_POSTGRES_STANDBY_URL"),
        ) else {
            return;
        };
        let mut admin = Client::connect(&primary, NoTls).unwrap();
        postgres::migrate(&mut admin).unwrap();
        admin
            .execute("SELECT pg_advisory_lock($1)", &[&0x4d4d_5a4a_5445_5354_i64])
            .unwrap();
        let number: i64 = admin.query_one("SELECT greatest(coalesce((SELECT max(zone_id) FROM mmorpg_persistence.streams),1000),coalesce((SELECT max(zone_id) FROM mmorpg_coordination.zones),1000))+1", &[]).unwrap().get(0);
        let zone = ZoneId::new(number.try_into().unwrap());
        let config = BackendConfig {
            coordinator_primary_url: role(&primary, "mmorpg_zone_coordinator"),
            coordinator_standby_url: role(&standby, "mmorpg_zone_coordinator"),
            character_standby_url: role(&standby, "mmorpg_character_profiles"),
            timeout: Duration::from_secs(5),
        };
        let mut backend = GlobalBackend::connect(config).unwrap();
        // An existing conflicting journal fence must roll back acquisition of
        // the independently owned directory grant in the same transaction.
        let collision = ZoneId::new(zone.get() + 1);
        admin.execute("INSERT INTO mmorpg_persistence.streams (zone_id,writer_host,writer_epoch) VALUES ($1,'unretired-reference-writer',77)", &[&i64::from(collision.get())]).unwrap();
        assert!(
            backend
                .grant(
                    collision,
                    HostId::new("new-process").unwrap(),
                    0,
                    Duration::from_secs(30)
                )
                .is_err()
        );
        let count: i64 = admin
            .query_one(
                "SELECT count(*)::bigint FROM mmorpg_coordination.zones WHERE zone_id=$1",
                &[&i64::from(collision.get())],
            )
            .unwrap()
            .get(0);
        assert_eq!(count, 0);
        let mut grant = backend
            .grant(
                zone,
                HostId::new("backend-test").unwrap(),
                0,
                Duration::from_secs(30),
            )
            .unwrap();
        grant.permit().ensure_current(zone).unwrap();
        assert!(
            backend
                .grant(
                    zone,
                    HostId::new("duplicate-process").unwrap(),
                    1,
                    Duration::from_secs(30)
                )
                .is_err()
        );
        backend.renew(&mut grant, Duration::from_secs(40)).unwrap();
        let limits = JournalLimits {
            max_batch_bytes: 1024 * 1024,
            max_retained_bytes: 2 * 1024 * 1024,
            max_retained_batches: 100,
            max_replay_ticks: 100,
        };
        let mut journal = PostgresJournal::connect_production(
            &role(&primary, "mmorpg_zone_journal"),
            &role(&standby, "mmorpg_zone_journal"),
            grant.fence().clone(),
            Duration::from_secs(5),
            limits,
        )
        .unwrap();
        let character = CharacterId::new(format!("backend-profile-{}", zone.get())).unwrap();
        let mut zone_state = ZoneSimulation::with_content(zone, greyhaven_vale::content()).unwrap();
        zone_state
            .create_character(
                1,
                ClassChoice {
                    class: PlayerClass::Warden,
                    sex: Sex::Male,
                },
            )
            .unwrap();
        let mut record = zone_state.capture_character(1, character.clone()).unwrap();
        record.copper = 77;
        journal
            .characters(|store| {
                let acquired = store.acquire(
                    &character,
                    LeaseHolder {
                        zone_id: zone,
                        zone_epoch: grant.fence().lease.epoch,
                    },
                )?;
                store.commit(&acquired.lease, &record)
            })
            .unwrap();
        let profile = backend.committed_character(&character).unwrap().unwrap();
        assert_eq!(profile.record, record);
        assert_eq!(profile.revision, 1);
        let mut query_only = admin.transaction().unwrap();
        query_only
            .batch_execute("SET LOCAL ROLE mmorpg_character_profiles")
            .unwrap();
        assert!(
            query_only
                .execute(
                    "UPDATE mmorpg_characters.entries SET entry=entry WHERE character_id=$1",
                    &[&character.as_str()]
                )
                .is_err()
        );
        query_only.rollback().unwrap();
        backend.retire_after_stop(&grant).unwrap();
        assert!(grant.permit().ensure_current(zone).is_err());
        assert!(journal.characters(|_| Ok(())).is_err());
        // A fresh process gets a greater epoch and keeps the persistence stream.
        let successor = backend
            .grant(
                zone,
                HostId::new("successor-process").unwrap(),
                1,
                Duration::from_secs(30),
            )
            .unwrap();
        assert_eq!(successor.fence().lease.epoch, 2);
        assert_eq!(successor.fence().world_id, grant.fence().world_id);
        assert_eq!(
            backend
                .committed_character(&character)
                .unwrap()
                .unwrap()
                .record
                .copper,
            77
        );
        backend.retire_after_stop(&successor).unwrap();
    }
}
