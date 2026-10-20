//! Acknowledgement barrier for one explicitly configured physical standby.
//!
//! Local COMMIT alone never satisfies this boundary. After COMMIT, capture a
//! fixed primary WAL flush position and wait until this *same cluster's* standby
//! has durably received and replayed through it. Promotion must retain this
//! confirmed standby's history; choosing/fencing a new primary is the operator's
//! coordination responsibility, not a decision made by a zone host.
//!
//! PostgreSQL defines the flush/receive/replay positions here:
//! <https://www.postgresql.org/docs/17/functions-admin.html>.
//! The standby endpoint must be on a separate durable failure domain in production.
//! Global progression queries use the confirmed standby, because local primary
//! visibility can precede the completion of this acknowledgement barrier.
use mmorpg_characters::StoreError;
use postgres::{CancelToken, Client, Config, NoTls};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};

#[derive(Eq, PartialEq)]
struct ClusterIdentity {
    system_identifier: String,
    timeline: i32,
}

pub(super) struct ReplicaBarrier {
    primary: Client,
    standby: Client,
    identity: ClusterIdentity,
    timeout: Duration,
}

impl ReplicaBarrier {
    pub(super) fn connect(
        primary_url: &str,
        standby_url: &str,
        timeout: Duration,
    ) -> Result<Self, StoreError> {
        if timeout.is_zero() {
            return Err(failure("replication confirmation timeout must be positive"));
        }
        let mut primary = connect(primary_url, timeout)?;
        let mut standby = connect(standby_url, timeout)?;
        let primary_identity = identity(&mut primary, false)?;
        let standby_identity = identity(&mut standby, true)?;
        if primary_identity != standby_identity {
            return Err(failure(
                "replication endpoints belong to different PostgreSQL clusters",
            ));
        }
        Ok(Self {
            primary,
            standby,
            identity: primary_identity,
            timeout,
        })
    }

    /// The owner can cancel in-flight SQL as well as setting the cooperative
    /// cancellation flag. A failed/cancelled barrier leaves the commit uncertain;
    /// it is never permission to publish or continue advancing the simulation.
    pub(super) fn cancel_tokens(&self) -> [CancelToken; 2] {
        [self.primary.cancel_token(), self.standby.cancel_token()]
    }

    pub(super) fn confirm(&mut self, cancelled: &AtomicBool) -> Result<(), StoreError> {
        if cancelled.load(Ordering::Acquire) {
            return Err(failure(
                "replication confirmation cancelled; commit outcome unresolved",
            ));
        }
        let deadline = Instant::now()
            .checked_add(self.timeout)
            .ok_or_else(|| failure("replication timeout overflow"))?;
        if identity(&mut self.primary, false)? != self.identity {
            return Err(failure("primary PostgreSQL cluster changed"));
        }
        // All mutations whose success is being confirmed have already returned
        // from local synchronous_commit=on. This fixed LSN therefore includes
        // their commit records; concurrent activity can only make it stricter.
        let required: String = self
            .primary
            .query_one("SELECT pg_current_wal_flush_lsn()::text", &[])
            .map_err(sql)?
            .get(0);
        loop {
            if cancelled.load(Ordering::Acquire) {
                return Err(failure(
                    "replication confirmation cancelled; commit outcome unresolved",
                ));
            }
            if Instant::now() >= deadline {
                return Err(failure(
                    "replication confirmation timed out; commit outcome unresolved",
                ));
            }
            if identity(&mut self.standby, true)? != self.identity {
                return Err(failure("standby PostgreSQL cluster changed"));
            }
            let row = self.standby.query_one(
                "SELECT coalesce(pg_last_wal_receive_lsn()>=$1::text::pg_lsn,false), coalesce(pg_last_wal_replay_lsn()>=$1::text::pg_lsn,false)",
                &[&required],
            ).map_err(sql)?;
            if row.get::<_, bool>(0) && row.get::<_, bool>(1) {
                // Recheck the role after the LSN observation: a promoted standby
                // cannot silently become an independent durability witness.
                if identity(&mut self.standby, true)? != self.identity {
                    return Err(failure("standby changed during replication confirmation"));
                }
                if Instant::now() >= deadline || cancelled.load(Ordering::Acquire) {
                    return Err(failure(
                        "replication confirmation exceeded its bound or was cancelled",
                    ));
                }
                return Ok(());
            }
            thread::sleep(
                Duration::from_millis(10).min(deadline.saturating_duration_since(Instant::now())),
            );
        }
    }
}

fn connect(url: &str, timeout: Duration) -> Result<Client, StoreError> {
    let mut config: Config = url.parse().map_err(sql)?;
    config.connect_timeout(timeout);
    let mut client = config.connect(NoTls).map_err(sql)?;
    let millis = timeout.as_millis().max(1);
    client
        .batch_execute(&format!(
            "SET statement_timeout={millis}; SET lock_timeout={millis}"
        ))
        .map_err(sql)?;
    Ok(client)
}

fn identity(client: &mut Client, standby: bool) -> Result<ClusterIdentity, StoreError> {
    let row = client.query_one(
        "SELECT pg_is_in_recovery(),current_setting('fsync'),current_setting('full_page_writes'),system_identifier::text,timeline_id FROM pg_control_system() CROSS JOIN pg_control_checkpoint()", &[]
    ).map_err(sql)?;
    if row.get::<_, bool>(0) != standby
        || row.get::<_, String>(1) != "on"
        || row.get::<_, String>(2) != "on"
    {
        return Err(failure(
            "replication endpoint has the wrong role or unsafe WAL durability settings",
        ));
    }
    Ok(ClusterIdentity {
        system_identifier: row.get(3),
        timeline: row.get(4),
    })
}
fn failure(message: impl Into<String>) -> StoreError {
    StoreError::Backend(message.into())
}
fn sql(error: impl std::fmt::Display) -> StoreError {
    failure(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replication_requires_an_independent_standby() {
        let Ok(url) = std::env::var("MMORPG_CHARACTERS_POSTGRES_URL") else {
            return;
        };
        assert!(ReplicaBarrier::connect(&url, &url, Duration::from_secs(1)).is_err());
    }

    #[test]
    fn physical_standby_confirms_a_commit() {
        let (Ok(primary), Ok(standby)) = (
            std::env::var("MMORPG_CHARACTERS_POSTGRES_URL"),
            std::env::var("MMORPG_POSTGRES_STANDBY_URL"),
        ) else {
            return;
        };
        let mut barrier =
            ReplicaBarrier::connect(&primary, &standby, Duration::from_secs(5)).unwrap();
        barrier.primary.batch_execute("CREATE TABLE IF NOT EXISTS replication_barrier_test (id bigint PRIMARY KEY); INSERT INTO replication_barrier_test VALUES (1) ON CONFLICT DO NOTHING").unwrap();
        barrier.confirm(&AtomicBool::new(false)).unwrap();
        assert_eq!(
            barrier
                .standby
                .query_one("SELECT count(*)::bigint FROM replication_barrier_test", &[])
                .unwrap()
                .get::<_, i64>(0),
            1
        );
        assert!(barrier.confirm(&AtomicBool::new(true)).is_err());
        // An independent connection resumes replay on every exit, including a
        // failed assertion, so this test never leaves the shared test standby paused.
        struct PausedReplay(Client);
        impl Drop for PausedReplay {
            fn drop(&mut self) {
                let _ = self.0.batch_execute("SELECT pg_wal_replay_resume()");
            }
        }
        let mut paused = PausedReplay(connect(&standby, Duration::from_secs(5)).unwrap());
        paused
            .0
            .batch_execute("SELECT pg_wal_replay_pause()")
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while paused
            .0
            .query_one("SELECT pg_get_wal_replay_pause_state()", &[])
            .unwrap()
            .get::<_, String>(0)
            != "paused"
        {
            assert!(Instant::now() < deadline, "standby did not pause replay");
            thread::sleep(Duration::from_millis(10));
        }
        barrier
            .primary
            .batch_execute("UPDATE replication_barrier_test SET id=id WHERE id=1")
            .unwrap();
        barrier.timeout = Duration::from_millis(100);
        assert!(barrier.confirm(&AtomicBool::new(false)).is_err());
        drop(paused);
        barrier.timeout = Duration::from_secs(5);
        barrier.confirm(&AtomicBool::new(false)).unwrap();
    }
}
