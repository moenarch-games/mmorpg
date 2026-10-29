//! PostgreSQL directory storage (TECH-001). Active owner, expiry and epoch floor
//! are persisted together. Expiry rejects renewals/writes; it NEVER automatically
//! grants a replacement. Acquisition requires confirmed source retirement, so a
//! wall-clock jump cannot authorize two processes with live local permits.
//!
//! All returned grants are explicitly UNCONFIRMED. The application must confirm
//! replicated durability before constructing/renewing a LocalZonePermit. This
//! module owns no replication topology, placement policy, transport or gameplay.
//!
//! [`PostgresHandoffs`] keeps the fenced cross-zone handoff records (#145) in
//! the same schema, checked against these zone grants in one transaction.
use crate::{
    ControlPlaneError, EntityId, HandoffAuthority, HandoffPhase, HandoffRecord, HandoffTicket,
    HostId, TransferId, ZoneLease, speaks_for,
};
use mmorpg_core::ZoneId;
use postgres::{CancelToken, Client, Config, GenericClient, NoTls, Row, Transaction};
use std::{
    error::Error,
    fmt,
    time::{Duration, Instant},
};

/// The schema migrations, in order; version `n` is `MIGRATIONS[n - 1]`.
const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/0001_directory.sql"),
    include_str!("../migrations/0002_handoffs.sql"),
];
#[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
pub const SCHEMA_VERSION: i32 = MIGRATIONS.len() as i32;

#[derive(Debug)]
pub enum DirectoryStoreError {
    Conflict,
    Expired,
    InvalidDuration,
    Backend(String),
}
impl fmt::Display for DirectoryStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Conflict => {
                f.write_str("zone grant changed or its previous writer is not retired")
            }
            Self::Expired => f.write_str("zone grant expired"),
            Self::InvalidDuration => {
                f.write_str("zone grant duration must fit positive milliseconds")
            }
            Self::Backend(message) => write!(f, "zone directory: {message}"),
        }
    }
}
impl Error for DirectoryStoreError {}

/// A process-local renewal capability returned only by acquisition. Looking up an
/// assignment never returns its renewal token and cannot resume an old process.
/// No serialization/Clone implementation: the backend keeps this handle while
/// the grant is active. Success still requires application-owned replication proof.
pub struct UnconfirmedZoneGrant {
    lease: ZoneLease,
    token: String,
    requested_at: Instant,
    valid_for: Duration,
}
impl UnconfirmedZoneGrant {
    #[must_use]
    pub fn lease(&self) -> &ZoneLease {
        &self.lease
    }
    #[must_use]
    pub fn requested_at(&self) -> Instant {
        self.requested_at
    }
    #[must_use]
    pub fn valid_for(&self) -> Duration {
        self.valid_for
    }
    pub fn ensure_locally_live(&self) -> Result<(), DirectoryStoreError> {
        if self
            .requested_at
            .checked_add(self.valid_for)
            .is_none_or(|deadline| Instant::now() >= deadline)
        {
            return Err(DirectoryStoreError::Expired);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredAssignment {
    /// Expiry is in database UNIX milliseconds, NOT simulation/reference ticks.
    pub lease: ZoneLease,
    pub retired: bool,
}

pub struct PostgresZoneDirectory {
    client: Client,
}
impl PostgresZoneDirectory {
    pub fn connect(url: &str, timeout: Duration) -> Result<Self, DirectoryStoreError> {
        if timeout.is_zero() {
            return Err(DirectoryStoreError::InvalidDuration);
        }
        let mut config: Config = url.parse().map_err(backend)?;
        config.connect_timeout(timeout);
        let mut client = config.connect(NoTls).map_err(backend)?;
        let millis = timeout.as_millis().max(1);
        client
            .batch_execute(&format!(
                "SET statement_timeout={millis}; SET lock_timeout={millis}"
            ))
            .map_err(backend)?;
        let version: Option<i32> = client
            .query_one(
                "SELECT max(version) FROM mmorpg_coordination.migrations",
                &[],
            )
            .map_err(backend)?
            .get(0);
        if version != Some(SCHEMA_VERSION) {
            return Err(backend("unsupported directory schema"));
        }
        Ok(Self { client })
    }
    #[must_use]
    pub fn cancel_token(&self) -> CancelToken {
        self.client.cancel_token()
    }
    pub fn acquire(
        &mut self,
        zone: ZoneId,
        host: HostId,
        expected_epoch: u64,
        ttl: Duration,
    ) -> Result<UnconfirmedZoneGrant, DirectoryStoreError> {
        let requested_at = Instant::now();
        let mut transaction = self.client.transaction().map_err(backend)?;
        let grant = acquire_in(
            &mut transaction,
            zone,
            host,
            expected_epoch,
            ttl,
            requested_at,
        )?;
        transaction.commit().map_err(backend)?;
        Ok(grant)
    }
    pub fn renew(
        &mut self,
        previous: &UnconfirmedZoneGrant,
        ttl: Duration,
    ) -> Result<UnconfirmedZoneGrant, DirectoryStoreError> {
        let requested_at = Instant::now();
        let mut transaction = self.client.transaction().map_err(backend)?;
        let grant = renew_in(&mut transaction, previous, ttl, requested_at)?;
        transaction.commit().map_err(backend)?;
        Ok(grant)
    }
    /// Caller must stop/fence the source before recording retirement. Expiration
    /// alone is deliberately insufficient evidence for reassignment here.
    pub fn retire(&mut self, expected: &ZoneLease) -> Result<(), DirectoryStoreError> {
        let mut transaction = self.client.transaction().map_err(backend)?;
        retire_in(&mut transaction, expected)?;
        transaction.commit().map_err(backend)
    }
    /// Read-only facts for coordination, never a usable ownership capability.
    pub fn assignment(
        &mut self,
        zone: ZoneId,
    ) -> Result<Option<StoredAssignment>, DirectoryStoreError> {
        read_assignment(&mut self.client, zone)
    }
}

pub fn read_assignment(
    client: &mut impl GenericClient,
    zone: ZoneId,
) -> Result<Option<StoredAssignment>, DirectoryStoreError> {
    client.query_opt("SELECT zone_id,host_id,epoch,floor(extract(epoch FROM expires_at)*1000)::bigint AS expiry,retired FROM mmorpg_coordination.zones WHERE zone_id=$1", &[&i64::from(zone.get())])
        .map_err(backend)?.map(|row| Ok(StoredAssignment { lease: read_lease(&row)?, retired: row.get("retired") })).transpose()
}

pub fn migrate(client: &mut Client) -> Result<(), DirectoryStoreError> {
    let mut transaction = client.transaction().map_err(backend)?;
    transaction
        .execute(
            "SELECT pg_advisory_xact_lock($1)",
            &[&0x4d4d_5a44_4d49_4752_i64],
        )
        .map_err(backend)?;
    transaction.batch_execute("CREATE SCHEMA IF NOT EXISTS mmorpg_coordination; CREATE TABLE IF NOT EXISTS mmorpg_coordination.migrations (version integer PRIMARY KEY,applied_at timestamptz NOT NULL DEFAULT now())").map_err(backend)?;
    let version: Option<i32> = transaction
        .query_one(
            "SELECT max(version) FROM mmorpg_coordination.migrations",
            &[],
        )
        .map_err(backend)?
        .get(0);
    let applied = version.unwrap_or(0);
    if !(0..=SCHEMA_VERSION).contains(&applied) {
        return Err(backend("unsupported directory schema"));
    }
    let applied = usize::try_from(applied).map_err(backend)?;
    for (version, migration) in (1_i32..).zip(MIGRATIONS).skip(applied) {
        transaction.batch_execute(migration).map_err(backend)?;
        transaction
            .execute(
                "INSERT INTO mmorpg_coordination.migrations (version) VALUES ($1)",
                &[&version],
            )
            .map_err(backend)?;
    }
    transaction.commit().map_err(backend)
}

/// Transaction composition lets the application install its independently owned
/// journal fence in the SAME commit as the directory grant. Replication proof
/// belongs after that outer commit, never inside this transaction callback.
pub fn acquire_in(
    transaction: &mut Transaction<'_>,
    zone: ZoneId,
    host: HostId,
    expected_epoch: u64,
    ttl: Duration,
    requested_at: Instant,
) -> Result<UnconfirmedZoneGrant, DirectoryStoreError> {
    let millis = duration(ttl)?;
    let next = expected_epoch
        .checked_add(1)
        .ok_or(DirectoryStoreError::Conflict)?;
    let next = integer(next)?;
    transaction
        .batch_execute("SET LOCAL synchronous_commit=on")
        .map_err(backend)?;
    transaction
        .execute(
            "SELECT pg_advisory_xact_lock(hashtextextended($1,0))",
            &[&format!("mmorpg-directory:{}", zone.get())],
        )
        .map_err(backend)?;
    let previous = transaction
        .query_opt(
            "SELECT epoch,retired FROM mmorpg_coordination.zones WHERE zone_id=$1 FOR UPDATE",
            &[&i64::from(zone.get())],
        )
        .map_err(backend)?;
    match previous {
        None if expected_epoch == 0 => {
            transaction.execute("INSERT INTO mmorpg_coordination.zones (zone_id,epoch,host_id,expires_at) VALUES ($1,$2,$3,clock_timestamp()+$4::bigint*interval '1 millisecond')", &[&i64::from(zone.get()), &next, &host.as_str(), &millis]).map_err(backend)?;
        }
        Some(row)
            if row.get::<_, i64>("epoch") == integer(expected_epoch)?
                && row.get::<_, bool>("retired") =>
        {
            transaction.execute("UPDATE mmorpg_coordination.zones SET epoch=$2,host_id=$3,expires_at=clock_timestamp()+$4::bigint*interval '1 millisecond',retired=false,renewal_token=gen_random_uuid() WHERE zone_id=$1", &[&i64::from(zone.get()), &next, &host.as_str(), &millis]).map_err(backend)?;
        }
        _ => return Err(DirectoryStoreError::Conflict),
    }
    read_grant(
        transaction,
        zone,
        requested_at,
        Duration::from_millis(millis.try_into().map_err(backend)?),
    )
}

pub fn renew_in(
    transaction: &mut Transaction<'_>,
    previous: &UnconfirmedZoneGrant,
    ttl: Duration,
    requested_at: Instant,
) -> Result<UnconfirmedZoneGrant, DirectoryStoreError> {
    previous.ensure_locally_live()?;
    let millis = duration(ttl)?;
    transaction
        .batch_execute("SET LOCAL synchronous_commit=on")
        .map_err(backend)?;
    // Lock before evaluating expiry: a lock wait must not turn an old valid
    // predicate into permission to revive an already expired grant.
    transaction
        .query_opt(
            "SELECT zone_id FROM mmorpg_coordination.zones WHERE zone_id=$1 FOR UPDATE",
            &[&i64::from(previous.lease.zone_id.get())],
        )
        .map_err(backend)?;
    previous.ensure_locally_live()?;
    let changed = transaction.execute("UPDATE mmorpg_coordination.zones SET expires_at=greatest(expires_at,clock_timestamp()+$5::bigint*interval '1 millisecond') WHERE zone_id=$1 AND epoch=$2 AND host_id=$3 AND renewal_token=$4::text::uuid AND NOT retired AND expires_at>clock_timestamp()", &[
        &i64::from(previous.lease.zone_id.get()), &integer(previous.lease.epoch)?, &previous.lease.host_id.as_str(), &previous.token, &millis,
    ]).map_err(backend)?;
    if changed != 1 {
        return Err(DirectoryStoreError::Expired);
    }
    read_grant(
        transaction,
        previous.lease.zone_id,
        requested_at,
        Duration::from_millis(millis.try_into().map_err(backend)?),
    )
}

pub fn retire_in(
    transaction: &mut Transaction<'_>,
    expected: &ZoneLease,
) -> Result<(), DirectoryStoreError> {
    transaction
        .batch_execute("SET LOCAL synchronous_commit=on")
        .map_err(backend)?;
    let changed = transaction.execute("UPDATE mmorpg_coordination.zones SET retired=true WHERE zone_id=$1 AND epoch=$2 AND host_id=$3", &[
        &i64::from(expected.zone_id.get()), &integer(expected.epoch)?, &expected.host_id.as_str(),
    ]).map_err(backend)?;
    if changed != 1 {
        return Err(DirectoryStoreError::Conflict);
    }
    Ok(())
}

/// Durable sink fence. Hold this shared row lock through the transaction's
/// gameplay/character/journal writes so a coordinator cannot replace the owner
/// between checking it and committing those effects. This function grants nothing.
pub fn ensure_writer_in(
    transaction: &mut Transaction<'_>,
    lease: &ZoneLease,
) -> Result<(), DirectoryStoreError> {
    let status: i16 = transaction
        .query_one(
            "SELECT mmorpg_coordination.lock_current_writer($1,$2,$3)",
            &[
                &i64::from(lease.zone_id.get()),
                &integer(lease.epoch)?,
                &lease.host_id.as_str(),
            ],
        )
        .map_err(backend)?
        .get(0);
    match status {
        0 => Err(DirectoryStoreError::Conflict),
        1 => Err(DirectoryStoreError::Expired),
        2 => Ok(()),
        _ => Err(backend("invalid directory fence result")),
    }
}

fn read_grant(
    transaction: &mut Transaction<'_>,
    zone: ZoneId,
    requested_at: Instant,
    ttl: Duration,
) -> Result<UnconfirmedZoneGrant, DirectoryStoreError> {
    let row = transaction.query_one("SELECT zone_id,epoch,host_id,floor(extract(epoch FROM expires_at)*1000)::bigint AS expiry,renewal_token::text AS token FROM mmorpg_coordination.zones WHERE zone_id=$1", &[&i64::from(zone.get())]).map_err(backend)?;
    let grant = UnconfirmedZoneGrant {
        lease: read_lease(&row)?,
        token: row.get("token"),
        requested_at,
        valid_for: ttl,
    };
    grant.ensure_locally_live()?;
    Ok(grant)
}
fn read_lease(row: &Row) -> Result<ZoneLease, DirectoryStoreError> {
    Ok(ZoneLease {
        zone_id: ZoneId::new(row.get::<_, i64>("zone_id").try_into().map_err(backend)?),
        host_id: HostId::new(row.get::<_, String>("host_id")).map_err(backend)?,
        epoch: row.get::<_, i64>("epoch").try_into().map_err(backend)?,
        expires_at_tick: row.get::<_, i64>("expiry").try_into().map_err(backend)?,
    })
}
fn duration(ttl: Duration) -> Result<i64, DirectoryStoreError> {
    let millis = ttl
        .as_millis()
        .try_into()
        .map_err(|_| DirectoryStoreError::InvalidDuration)?;
    if millis == 0 {
        return Err(DirectoryStoreError::InvalidDuration);
    }
    Ok(millis)
}
fn integer(value: u64) -> Result<i64, DirectoryStoreError> {
    value.try_into().map_err(backend)
}
fn backend(cause: impl fmt::Display) -> DirectoryStoreError {
    DirectoryStoreError::Backend(cause.to_string())
}

/// The durable [`HandoffAuthority`] (#145): handoff records in
/// `mmorpg_coordination.handoffs`, each step one transaction that first
/// share-locks the zone grants it names (`lock_current_writer`), so no grant
/// changes between the check and the record write. The rules are those of
/// [`crate::HandoffRegistry`].
pub struct PostgresHandoffs {
    client: Client,
}

impl PostgresHandoffs {
    /// Connects to `url`; `timeout` bounds connecting and every statement.
    pub fn connect(url: &str, timeout: Duration) -> Result<Self, DirectoryStoreError> {
        Ok(Self {
            client: PostgresZoneDirectory::connect(url, timeout)?.client,
        })
    }

    #[must_use]
    pub fn cancel_token(&self) -> CancelToken {
        self.client.cancel_token()
    }

    fn step<R>(
        &mut self,
        body: impl FnOnce(&mut Transaction<'_>) -> Result<R, ControlPlaneError>,
    ) -> Result<R, ControlPlaneError> {
        let mut transaction = self.client.transaction().map_err(unavailable)?;
        transaction
            .batch_execute("SET LOCAL synchronous_commit=on")
            .map_err(unavailable)?;
        let result = body(&mut transaction)?;
        transaction.commit().map_err(unavailable)?;
        Ok(result)
    }
}

const PREPARED: i16 = 0;
const ACCEPTED: i16 = 1;
const COMMITTED: i16 = 2;
const ABORTED: i16 = 3;

const fn phase_code(phase: HandoffPhase) -> i16 {
    match phase {
        HandoffPhase::Prepared => PREPARED,
        HandoffPhase::Accepted => ACCEPTED,
        HandoffPhase::Committed => COMMITTED,
        HandoffPhase::Aborted => ABORTED,
    }
}

fn phase_of(code: i16) -> Result<HandoffPhase, ControlPlaneError> {
    Ok(match code {
        PREPARED => HandoffPhase::Prepared,
        ACCEPTED => HandoffPhase::Accepted,
        COMMITTED => HandoffPhase::Committed,
        ABORTED => HandoffPhase::Aborted,
        _ => return Err(unavailable("invalid handoff phase")),
    })
}

/// Unsigned IDs are stored bit for bit in `bigint` columns.
const fn signed(value: u64) -> i64 {
    i64::from_be_bytes(value.to_be_bytes())
}

const fn unsigned(value: i64) -> u64 {
    u64::from_be_bytes(value.to_be_bytes())
}

fn unavailable(cause: impl fmt::Display) -> ControlPlaneError {
    ControlPlaneError::Backend(cause.to_string())
}

fn sql_integer(value: u64) -> Result<i64, ControlPlaneError> {
    value.try_into().map_err(unavailable)
}

/// Share-locks `lease`'s zone row and fails unless it is the current grant.
fn ensure_current_in(
    client: &mut impl GenericClient,
    lease: &ZoneLease,
) -> Result<(), ControlPlaneError> {
    let zone = i64::from(lease.zone_id.get());
    let status: i16 = client
        .query_one(
            "SELECT mmorpg_coordination.lock_current_writer($1,$2,$3)",
            &[&zone, &sql_integer(lease.epoch)?, &lease.host_id.as_str()],
        )
        .map_err(unavailable)?
        .get(0);
    match status {
        2 => Ok(()),
        1 => Err(ControlPlaneError::LeaseExpired {
            zone_id: lease.zone_id,
            epoch: lease.epoch,
            expires_at_tick: lease.expires_at_tick,
            observed_at_tick: lease.expires_at_tick,
        }),
        0 => {
            let row = client
                .query_opt(
                    "SELECT epoch,host_id,retired FROM mmorpg_coordination.zones WHERE zone_id=$1",
                    &[&zone],
                )
                .map_err(unavailable)?;
            let Some(row) = row.filter(|row| !row.get::<_, bool>("retired")) else {
                return Err(ControlPlaneError::ZoneNotAssigned(lease.zone_id));
            };
            let actual_epoch = u64::try_from(row.get::<_, i64>("epoch")).map_err(unavailable)?;
            if actual_epoch != lease.epoch {
                return Err(ControlPlaneError::StaleLease {
                    zone_id: lease.zone_id,
                    expected_epoch: lease.epoch,
                    actual_epoch,
                });
            }
            Err(ControlPlaneError::LeaseOwnerMismatch(lease.zone_id))
        }
        _ => Err(unavailable("invalid directory fence result")),
    }
}

fn lease_of(row: &Row, prefix: &str) -> Result<ZoneLease, ControlPlaneError> {
    let zone: i64 = row.get(format!("{prefix}_zone").as_str());
    let host: String = row.get(format!("{prefix}_host").as_str());
    let epoch: i64 = row.get(format!("{prefix}_epoch").as_str());
    Ok(ZoneLease {
        zone_id: ZoneId::new(zone.try_into().map_err(unavailable)?),
        host_id: HostId::new(host).map_err(unavailable)?,
        epoch: epoch.try_into().map_err(unavailable)?,
        // Records name grants by identity; the directory holds the deadline.
        expires_at_tick: 0,
    })
}

/// The record of `transfer_id`, row-locked for this transaction.
fn record_in(
    client: &mut impl GenericClient,
    transfer_id: TransferId,
) -> Result<Option<HandoffRecord>, ControlPlaneError> {
    let row = client
        .query_opt(
            "SELECT entity_id,source_zone,source_host,source_epoch,destination_zone,destination_host,destination_epoch,phase FROM mmorpg_coordination.handoffs WHERE transfer_id=$1 FOR UPDATE",
            &[&signed(transfer_id.get())],
        )
        .map_err(unavailable)?;
    row.map(|row| {
        Ok(HandoffRecord {
            ticket: HandoffTicket {
                transfer_id,
                entity_id: EntityId::new(unsigned(row.get("entity_id"))),
                source: lease_of(&row, "source")?,
                destination: lease_of(&row, "destination")?,
            },
            phase: phase_of(row.get("phase"))?,
        })
    })
    .transpose()
}

fn set_phase(
    client: &mut impl GenericClient,
    transfer_id: TransferId,
    phase: HandoffPhase,
) -> Result<(), ControlPlaneError> {
    client
        .execute(
            "UPDATE mmorpg_coordination.handoffs SET phase=$2 WHERE transfer_id=$1",
            &[&signed(transfer_id.get()), &phase_code(phase)],
        )
        .map_err(unavailable)?;
    Ok(())
}

impl HandoffAuthority for PostgresHandoffs {
    fn ensure_current(&mut self, lease: &ZoneLease) -> Result<(), ControlPlaneError> {
        self.step(|transaction| ensure_current_in(transaction, lease))
    }

    fn current_lease(&mut self, zone_id: ZoneId) -> Result<Option<ZoneLease>, ControlPlaneError> {
        Ok(read_assignment(&mut self.client, zone_id)
            .map_err(unavailable)?
            .filter(|assignment| !assignment.retired)
            .map(|assignment| assignment.lease))
    }

    fn record(
        &mut self,
        transfer_id: TransferId,
    ) -> Result<Option<HandoffRecord>, ControlPlaneError> {
        self.step(|transaction| record_in(transaction, transfer_id))
    }

    fn prepare(&mut self, ticket: HandoffTicket) -> Result<HandoffPhase, ControlPlaneError> {
        if ticket.source.zone_id == ticket.destination.zone_id {
            return Err(ControlPlaneError::SameZoneTransfer(ticket.source.zone_id));
        }
        self.step(|transaction| {
            ensure_current_in(transaction, &ticket.source)?;
            ensure_current_in(transaction, &ticket.destination)?;
            // Serialize transfers of one entity, also before it has a row.
            transaction
                .execute(
                    "SELECT pg_advisory_xact_lock(hashtextextended($1,0))",
                    &[&format!("mmorpg-handoff:{}", ticket.entity_id.get())],
                )
                .map_err(unavailable)?;
            if let Some(existing) = record_in(transaction, ticket.transfer_id)? {
                if existing.ticket.entity_id != ticket.entity_id
                    || !existing.ticket.source.same_authority(&ticket.source)
                    || !existing
                        .ticket
                        .destination
                        .same_authority(&ticket.destination)
                {
                    return Err(ControlPlaneError::TransferIdCollision(ticket.transfer_id));
                }
                return Ok(existing.phase);
            }
            let busy = transaction
                .query_opt(
                    "SELECT 1 FROM mmorpg_coordination.handoffs WHERE entity_id=$1 AND phase IN (0,1)",
                    &[&signed(ticket.entity_id.get())],
                )
                .map_err(unavailable)?;
            if busy.is_some() {
                return Err(ControlPlaneError::EntityTransferInProgress(
                    ticket.entity_id,
                ));
            }
            let (source, destination) = (&ticket.source, &ticket.destination);
            transaction
                .execute(
                    "INSERT INTO mmorpg_coordination.handoffs (transfer_id,entity_id,source_zone,source_host,source_epoch,destination_zone,destination_host,destination_epoch,phase) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
                    &[
                        &signed(ticket.transfer_id.get()),
                        &signed(ticket.entity_id.get()),
                        &i64::from(source.zone_id.get()),
                        &source.host_id.as_str(),
                        &sql_integer(source.epoch)?,
                        &i64::from(destination.zone_id.get()),
                        &destination.host_id.as_str(),
                        &sql_integer(destination.epoch)?,
                        &PREPARED,
                    ],
                )
                .map_err(unavailable)?;
            Ok(HandoffPhase::Prepared)
        })
    }

    fn accept(
        &mut self,
        transfer_id: TransferId,
        destination: &ZoneLease,
    ) -> Result<HandoffPhase, ControlPlaneError> {
        self.step(|transaction| {
            ensure_current_in(transaction, destination)?;
            let record = record_in(transaction, transfer_id)?
                .ok_or(ControlPlaneError::UnknownTransfer(transfer_id))?;
            if !record.ticket.destination.same_authority(destination) {
                return Err(ControlPlaneError::TransferLeaseMismatch(transfer_id));
            }
            match record.phase {
                HandoffPhase::Aborted => Err(ControlPlaneError::TransferAborted(transfer_id)),
                HandoffPhase::Prepared => {
                    ensure_current_in(transaction, &record.ticket.source)?;
                    set_phase(transaction, transfer_id, HandoffPhase::Accepted)?;
                    Ok(HandoffPhase::Accepted)
                }
                phase @ (HandoffPhase::Accepted | HandoffPhase::Committed) => {
                    ensure_current_in(transaction, &record.ticket.source)?;
                    Ok(phase)
                }
            }
        })
    }

    fn commit(
        &mut self,
        transfer_id: TransferId,
        source: &ZoneLease,
    ) -> Result<HandoffPhase, ControlPlaneError> {
        self.step(|transaction| {
            ensure_current_in(transaction, source)?;
            let record = record_in(transaction, transfer_id)?
                .ok_or(ControlPlaneError::UnknownTransfer(transfer_id))?;
            if !speaks_for(&record.ticket.source, source) {
                return Err(ControlPlaneError::TransferLeaseMismatch(transfer_id));
            }
            match record.phase {
                HandoffPhase::Prepared => Err(ControlPlaneError::TransferNotAccepted(transfer_id)),
                HandoffPhase::Aborted => Err(ControlPlaneError::TransferAborted(transfer_id)),
                HandoffPhase::Accepted => {
                    set_phase(transaction, transfer_id, HandoffPhase::Committed)?;
                    Ok(HandoffPhase::Committed)
                }
                HandoffPhase::Committed => Ok(HandoffPhase::Committed),
            }
        })
    }

    fn abort(
        &mut self,
        transfer_id: TransferId,
        source: &ZoneLease,
    ) -> Result<HandoffPhase, ControlPlaneError> {
        self.step(|transaction| {
            ensure_current_in(transaction, source)?;
            let record = record_in(transaction, transfer_id)?
                .ok_or(ControlPlaneError::UnknownTransfer(transfer_id))?;
            if !speaks_for(&record.ticket.source, source) {
                return Err(ControlPlaneError::TransferLeaseMismatch(transfer_id));
            }
            match record.phase {
                HandoffPhase::Prepared => {
                    set_phase(transaction, transfer_id, HandoffPhase::Aborted)?;
                    Ok(HandoffPhase::Aborted)
                }
                HandoffPhase::Aborted => Ok(HandoffPhase::Aborted),
                HandoffPhase::Accepted | HandoffPhase::Committed => {
                    Err(ControlPlaneError::TransferAlreadyAccepted(transfer_id))
                }
            }
        })
    }
}
