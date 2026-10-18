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

use std::cell::RefCell;
use std::time::Duration;

use mmorpg_core::CharacterId;
use postgres::{CancelToken, Client, Config, NoTls};

use crate::{EntryBackend, EntryChange, StoreError};

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
        let row = self
            .client
            .borrow_mut()
            .query_opt(
                "SELECT entry FROM mmorpg_characters.entries WHERE character_id = $1",
                &[&id.to_string()],
            )
            .map_err(backend)?;
        Ok(row.map(|row| row.get(0)))
    }

    fn update(&mut self, id: &CharacterId, change: &mut EntryChange<'_>) -> Result<(), StoreError> {
        let id = id.to_string();
        let mut client = self.client.borrow_mut();
        let mut transaction = client.transaction().map_err(backend)?;
        transaction
            .execute(
                "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
                &[&id],
            )
            .map_err(backend)?;
        let current: Option<Vec<u8>> = transaction
            .query_opt(
                "SELECT entry FROM mmorpg_characters.entries WHERE character_id = $1",
                &[&id],
            )
            .map_err(backend)?
            .map(|row| row.get(0));
        // A failing change drops the transaction, which rolls it back.
        let next = change(current.as_deref())?;
        transaction
            .execute(
                "INSERT INTO mmorpg_characters.entries (character_id, entry) VALUES ($1, $2)
                 ON CONFLICT (character_id) DO UPDATE SET entry = EXCLUDED.entry",
                &[&id, &next],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)
    }
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
