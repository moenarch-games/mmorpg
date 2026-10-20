//! One owned global-backend worker: grant renewal remains independent of zone
//! ticks and checkpoint uploads. Only a stopped/joined host may request retirement.
use super::{
    backend::{BackendConfig, ConfirmedZoneGrant, GlobalBackend},
    postgres::StreamFence,
};
use mmorpg_control_plane::{HostId, LocalZonePermit};
use mmorpg_core::ZoneId;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};
use tokio::sync::{oneshot, watch};

pub(crate) struct ZoneGrant {
    pub fence: StreamFence,
    pub permit: Arc<LocalZonePermit>,
}
enum Stop {
    Retire,
    Abandon,
}
enum Cancel {
    Query,
    Stop,
}
pub(crate) struct BackendWorker {
    stop: mpsc::SyncSender<Stop>,
    cancellation: mpsc::SyncSender<Cancel>,
    cancelled: Arc<AtomicBool>,
    permits: Arc<Mutex<Vec<Arc<LocalZonePermit>>>>,
    thread: Option<thread::JoinHandle<Result<(), String>>>,
    pub faults: watch::Receiver<Option<String>>,
}
impl BackendWorker {
    pub(crate) async fn start(
        config: BackendConfig,
        zones: Vec<ZoneId>,
        host: HostId,
        ttl: Duration,
        renew_every: Duration,
    ) -> Result<(Self, Vec<ZoneGrant>), String> {
        if renew_every.is_zero() || renew_every >= ttl {
            return Err("renewal interval must be positive and shorter than grant lifetime".into());
        }
        let (stop, requests) = mpsc::sync_channel(1);
        let (cancellation, cancellations) = mpsc::sync_channel(1);
        let (faults_tx, faults) = watch::channel(None);
        let (ready, response) = oneshot::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let permits = Arc::new(Mutex::new(Vec::new()));
        let worker_flag = Arc::clone(&cancelled);
        let worker_permits = Arc::clone(&permits);
        let cancellation_end = cancellation.clone();
        let thread = thread::Builder::new()
            .name("mmorpg-global-backend".into())
            .spawn(move || {
                let mut backend = match GlobalBackend::connect(config) {
                    Ok(backend) => backend,
                    Err(cause) => {
                        let cause = cause.to_string();
                        let _ = ready.send(Err(cause.clone()));
                        return Err(cause);
                    }
                };
                backend.set_cancellation(Arc::clone(&worker_flag));
                let tokens = backend.cancel_tokens();
                let canceller = thread::Builder::new()
                    .name("mmorpg-global-cancel".into())
                    .spawn(move || {
                        while let Ok(command) = cancellations.recv() {
                            match command {
                                Cancel::Query => {
                                    for token in &tokens {
                                        let _ = token.cancel_query(::postgres::NoTls);
                                    }
                                }
                                Cancel::Stop => break,
                            }
                        }
                    })
                    .map_err(|cause| cause.to_string())?;
                let mut active = Vec::<ConfirmedZoneGrant>::new();
                let initialized = (|| -> Result<Vec<ZoneGrant>, String> {
                    let mut result = Vec::new();
                    for zone in zones {
                        if worker_flag.load(Ordering::Acquire) {
                            return Err("global backend startup cancelled".into());
                        }
                        let previous = backend
                            .assignment(zone)
                            .map_err(|cause| cause.to_string())?
                            .map_or(0, |stored| stored.lease.epoch);
                        let grant = backend
                            .grant(zone, host.clone(), previous, ttl)
                            .map_err(|cause| cause.to_string())?;
                        let permit = grant.permit();
                        worker_permits
                            .lock()
                            .map_err(|cause| cause.to_string())?
                            .push(Arc::clone(&permit));
                        result.push(ZoneGrant {
                            fence: grant.fence().clone(),
                            permit,
                        });
                        active.push(grant);
                    }
                    Ok(result)
                })();
                let mut failed = false;
                let outcome = match initialized {
                    Err(cause) => {
                        for grant in &active {
                            let _ = backend.retire_after_stop(grant);
                        }
                        let _ = ready.send(Err(cause.clone()));
                        Err(cause)
                    }
                    Ok(grants) => {
                        if ready.send(Ok(grants)).is_err() {
                            for grant in &active {
                                let _ = backend.retire_after_stop(grant);
                            }
                            Err("global backend startup abandoned".into())
                        } else {
                            loop {
                                match requests.recv_timeout(renew_every) {
                                    Ok(Stop::Retire) => {
                                        let mut result = Ok(());
                                        for grant in &active {
                                            if let Err(cause) = backend.retire_after_stop(grant) {
                                                result = Err(cause.to_string());
                                            }
                                        }
                                        break result;
                                    }
                                    Ok(Stop::Abandon)
                                    | Err(mpsc::RecvTimeoutError::Disconnected) => break Ok(()),
                                    Err(mpsc::RecvTimeoutError::Timeout) => {
                                        if worker_flag.load(Ordering::Acquire) {
                                            break Ok(());
                                        }
                                        if failed {
                                            continue;
                                        }
                                        for grant in &mut active {
                                            if let Err(cause) = backend.renew(grant, ttl) {
                                                failed = true;
                                                for permit in worker_permits
                                                    .lock()
                                                    .map_err(|cause| cause.to_string())?
                                                    .iter()
                                                {
                                                    let _ = permit.revoke();
                                                }
                                                let _ = faults_tx.send(Some(cause.to_string()));
                                                break;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                };
                for permit in worker_permits
                    .lock()
                    .map_err(|cause| cause.to_string())?
                    .iter()
                {
                    let _ = permit.revoke();
                }
                let _ = cancellation_end.send(Cancel::Stop);
                let _ = canceller.join();
                outcome
            })
            .map_err(|cause| cause.to_string())?;
        let driver = Self {
            stop,
            cancellation,
            cancelled,
            permits,
            thread: Some(thread),
            faults,
        };
        let grants = response.await.map_err(|cause| cause.to_string())??;
        Ok((driver, grants))
    }
    pub(crate) async fn shutdown(mut self) -> Result<(), String> {
        let _ = self.stop.send(Stop::Retire);
        if let Some(worker) = self.thread.take() {
            tokio::task::spawn_blocking(move || {
                worker
                    .join()
                    .map_err(|_| "global backend worker panicked".to_string())?
            })
            .await
            .map_err(|cause| cause.to_string())??;
        }
        Ok(())
    }
}
impl Drop for BackendWorker {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        if let Ok(permits) = self.permits.lock() {
            for permit in permits.iter() {
                let _ = permit.revoke();
            }
        }
        let _ = self.cancellation.try_send(Cancel::Query);
        let _ = self.stop.try_send(Stop::Abandon);
    }
}
