//! The production character backend (TECH-002): entries in the shared
//! PostgreSQL deployment, in the `mmorpg_characters` schema.
//!
//! Each character is one row holding its encoded [`Entry`](crate::Entry)
//! bytes, so the lease rules and the strict entry format stay in
//! [`CharacterStore`](crate::CharacterStore). An update runs in one
//! transaction under a per-character advisory lock, so updates from several
//! hosts serialize — also for a character that has no row yet — and a failed
//! update leaves the row as it was. Every statement is bounded by the
//! connection's statement and lock timeouts and can be cancelled from another
//! thread through [`PostgresEntries::cancel_token`].
//!
//! The schema is versioned by [`MIGRATIONS`]; [`migrate`] applies them with a
//! privileged role, and [`PostgresEntries::connect`] refuses a database whose
//! schema is not exactly the version this build expects.
//!
//! [`PostgresEntries::transaction`] composes the same character transitions with
//! caller-owned journal/grant SQL in one PostgreSQL transaction (TECH-005).
//! [`CharacterStore::commit_at_revision`] checks the expected committed revision
//! under the same per-character lock as the lease/generation fence. The callback
//! returns only after commit; an error rolls back its character and journal work.
//! Failed SQL/entry updates poison the composition even when accidentally caught.
//! A commit transport failure is uncertain and must be resolved by the journal's
//! stable transaction identity before the host acknowledges a durable outcome.

use std::cell::RefCell;
use std::time::Duration;

use mmorpg_core::CharacterId;
use postgres::{CancelToken, Client, Config, GenericClient, NoTls, Transaction};

use crate::{CharacterStore, EntryBackend, EntryChange, StoreError};

/// The schema migrations, in order; version `n` is `MIGRATIONS[n - 1]`.
pub const MIGRATIONS: &[&str] = &[include_str!("../migrations/0001_entries.sql")];

/// The schema version this build reads and writes.
#[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
pub const SCHEMA_VERSION: i32 = MIGRATIONS.len() as i32;

/// Serializes migrators across hosts (`pg_advisory_xact_lock` key).
const MIGRATION_LOCK: i64 = 0x4d4d_4348_4d49_4752; // "MMCHMIGR"

pub struct PostgresEntries {
    client: RefCell<Client>,
}

impl PostgresEntries {
    /// Connects to `url` (a libpq URL or key/value string). `timeout` bounds
    /// connecting and every later statement and lock wait.
    pub fn connect(url: &str, timeout: Duration) -> Result<Self, StoreError> {
        let mut config: Config = url.parse().map_err(backend)?;
        config.connect_timeout(timeout);
        let mut client = config.connect(NoTls).map_err(backend)?;
        let millis = timeout.as_millis().max(1);
        client
            .batch_execute(&format!(
                "SET statement_timeout = {millis}; SET lock_timeout = {millis}"
            ))
            .map_err(backend)?;
        let version: Option<i32> = client
            .query_one("SELECT max(version) FROM mmorpg_characters.migrations", &[])
            .map_err(backend)?
            .get(0);
        if version != Some(SCHEMA_VERSION) {
            return Err(StoreError::Backend(format!(
                "character schema is at version {}, this build needs {SCHEMA_VERSION}",
                version.unwrap_or(0)
            )));
        }
        Ok(Self {
            client: RefCell::new(client),
        })
    }

    /// Composes character transitions and caller-owned journal/grant SQL in one
    /// database transaction. Every entry operation uses the existing store rules.
    /// Failed entry updates and SQL operations abort the composition even if caught.
    /// Run on a blocking worker; configured statement/lock bounds still apply.
    pub fn transaction<R>(
        &mut self,
        apply: impl FnOnce(&mut CharacterStore<PostgresTransactionEntries<'_>>) -> Result<R, StoreError>,
    ) -> Result<R, StoreError> {
        let mut client = self.client.borrow_mut();
        let transaction = client.transaction().map_err(backend)?;
        let mut store = CharacterStore::new(PostgresTransactionEntries {
            transaction: RefCell::new(transaction),
            failure: RefCell::new(None),
        });
        let result = apply(&mut store)?;
        if let Some(error) = store.backend.failure.borrow_mut().take() {
            return Err(error);
        }
        // PostgreSQL COMMIT on an aborted transaction can report a successful
        // ROLLBACK. Detect SQL errors swallowed inside a caller's SQL callback.
        store.backend.with_sql(|transaction| {
            transaction.simple_query("SELECT 1").map_err(backend)?;
            Ok(())
        })?;
        store
            .backend
            .transaction
            .into_inner()
            .commit()
            .map_err(backend)?;
        Ok(result)
    }

    /// Cancels the statement this backend is running, from any thread.
    #[must_use]
    pub fn cancel_token(&self) -> CancelToken {
        self.client.borrow().cancel_token()
    }
}

/// Applies every migration `client`'s database lacks, in one transaction
/// that concurrent migrators wait for. Fails on a database newer than this
/// build.
pub fn migrate(client: &mut Client) -> Result<(), StoreError> {
    let mut transaction = client.transaction().map_err(backend)?;
    transaction
        .execute("SELECT pg_advisory_xact_lock($1)", &[&MIGRATION_LOCK])
        .map_err(backend)?;
    transaction
        .batch_execute(
            "CREATE SCHEMA IF NOT EXISTS mmorpg_characters;
             CREATE TABLE IF NOT EXISTS mmorpg_characters.migrations (
               version integer PRIMARY KEY,
               applied_at timestamptz NOT NULL DEFAULT now()
             )",
        )
        .map_err(backend)?;
    let newest: Option<i32> = transaction
        .query_one("SELECT max(version) FROM mmorpg_characters.migrations", &[])
        .map_err(backend)?
        .get(0);
    let newest = newest.unwrap_or(0);
    if newest > SCHEMA_VERSION {
        return Err(StoreError::Backend(format!(
            "character schema is at version {newest}, newer than this build's {SCHEMA_VERSION}"
        )));
    }
    for (version, migration) in (1..)
        .zip(MIGRATIONS)
        .skip(usize::try_from(newest).map_err(backend)?)
    {
        transaction.batch_execute(migration).map_err(backend)?;
        transaction
            .execute(
                "INSERT INTO mmorpg_characters.migrations (version) VALUES ($1)",
                &[&version],
            )
            .map_err(backend)?;
    }
    transaction.commit().map_err(backend)
}

impl EntryBackend for PostgresEntries {
    fn read(&self, id: &CharacterId) -> Result<Option<Vec<u8>>, StoreError> {
        read_entry(&mut *self.client.borrow_mut(), id)
    }

    fn update(&mut self, id: &CharacterId, change: &mut EntryChange<'_>) -> Result<(), StoreError> {
        let mut client = self.client.borrow_mut();
        let mut transaction = client.transaction().map_err(backend)?;
        update_entry(&mut transaction, id, change)?;
        transaction.commit().map_err(backend)
    }
}

/// Entry backend scoped to a single caller-composed PostgreSQL transaction.
/// It cannot outlive or independently commit the enclosing composition.
pub struct PostgresTransactionEntries<'a> {
    transaction: RefCell<Transaction<'a>>,
    failure: RefCell<Option<StoreError>>,
}

impl<'a> PostgresTransactionEntries<'a> {
    /// Executes caller-owned journal/grant operations on the same transaction.
    /// Character entry transitions must go through `CharacterStore`.
    pub fn with_sql<R>(
        &self,
        apply: impl FnOnce(&mut Transaction<'a>) -> Result<R, StoreError>,
    ) -> Result<R, StoreError> {
        let result = apply(&mut self.transaction.borrow_mut());
        if let Err(error) = &result {
            self.failure
                .borrow_mut()
                .get_or_insert_with(|| error.clone());
        }
        result
    }
}

impl EntryBackend for PostgresTransactionEntries<'_> {
    fn read(&self, id: &CharacterId) -> Result<Option<Vec<u8>>, StoreError> {
        self.with_sql(|transaction| read_entry(transaction, id))
    }
    fn update(&mut self, id: &CharacterId, change: &mut EntryChange<'_>) -> Result<(), StoreError> {
        self.with_sql(|transaction| update_entry(transaction, id, change))
    }
}

fn read_entry(
    client: &mut impl GenericClient,
    id: &CharacterId,
) -> Result<Option<Vec<u8>>, StoreError> {
    Ok(client
        .query_opt(
            "SELECT entry FROM mmorpg_characters.entries WHERE character_id = $1",
            &[&id.to_string()],
        )
        .map_err(backend)?
        .map(|row| row.get(0)))
}

fn update_entry(
    transaction: &mut impl GenericClient,
    id: &CharacterId,
    change: &mut EntryChange<'_>,
) -> Result<(), StoreError> {
    let key = id.to_string();
    transaction
        .execute(
            "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
            &[&key],
        )
        .map_err(backend)?;
    let current = read_entry(transaction, id)?;
    let next = change(current.as_deref())?;
    transaction
        .execute(
            "INSERT INTO mmorpg_characters.entries (character_id, entry) VALUES ($1, $2)
         ON CONFLICT (character_id) DO UPDATE SET entry = EXCLUDED.entry",
            &[&key, &next],
        )
        .map_err(backend)?;
    Ok(())
}

fn backend(error: impl Into<Failure>) -> StoreError {
    StoreError::Backend(error.into().0)
}

/// A backend failure's message, with the server's reason for database errors.
struct Failure(String);

impl From<postgres::Error> for Failure {
    fn from(error: postgres::Error) -> Self {
        Self(match error.as_db_error() {
            Some(db) => format!("{error}: {}", db.message()),
            None => error.to_string(),
        })
    }
}

impl From<std::num::TryFromIntError> for Failure {
    fn from(error: std::num::TryFromIntError) -> Self {
        Self(error.to_string())
    }
}
