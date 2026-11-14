//! Native hosting controls; work runs off the window thread, status is always observed.
use mmorpg_game_server::local_world::{self, RunningWorld};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio::sync::watch;

#[derive(Clone)]
pub enum State {
    Stopped,
    Working(&'static str),
    Ready(RunningWorld, mmorpg_game_server::hosted_access::ShareStatus),
    Failed(String),
}

#[derive(Clone)]
pub struct Hosting {
    state: watch::Sender<State>,
    operation: Arc<Mutex<()>>,
    revision: Arc<AtomicU64>,
    directory: Option<PathBuf>,
    pub available: bool,
}

impl Default for Hosting {
    fn default() -> Self {
        let (state, _) = watch::channel(State::Stopped);
        Self {
            state,
            operation: Arc::default(),
            revision: Arc::default(),
            available: true,
            directory: crate::roster::config_dir().map(|dir| dir.join("worlds/default")),
        }
    }
}

impl Hosting {
    pub fn for_zone(zone: mmorpg_core::ZoneId) -> Self {
        Self {
            available: zone == mmorpg_core::ZoneId::new(1),
            ..Self::default()
        }
    }

    pub fn state(&self) -> State {
        self.state.borrow().clone()
    }

    /// Probe only; opening the game never starts a world.
    pub fn refresh(&self) {
        self.run(false, true);
    }
    pub fn start(&self) {
        self.run(false, false);
    }
    pub fn stop(&self) {
        self.run(true, false);
    }

    pub fn guest_decision(&self, key: String, allow: bool) {
        self.run_action(false, false, Some((key, allow)), false);
    }
    pub fn retry_sharing(&self) {
        self.run_action(false, false, None, true);
    }
    fn run(&self, stop: bool, probe: bool) {
        self.run_action(stop, probe, None, false);
    }
    fn run_action(&self, stop: bool, probe: bool, decision: Option<(String, bool)>, share: bool) {
        if matches!(self.state(), State::Working(_)) {
            return;
        }
        // A user action queues behind a read-only probe instead of silently disappearing.
        if probe && self.operation.try_lock().is_err() {
            return;
        }
        let revision = if probe {
            self.revision.load(Ordering::Acquire)
        } else {
            self.revision.fetch_add(1, Ordering::AcqRel) + 1
        };
        // Polls must not remove and reposition Copy/Approve underneath the pointer.
        // The worker mutex still serializes background probes and user actions.
        if !probe || !matches!(self.state(), State::Ready(..)) {
            self.state.send_replace(State::Working(if stop {
                "Stopping world and saving…"
            } else if probe {
                "Checking local world…"
            } else if decision.is_some() {
                "Updating guest access…"
            } else if share {
                "Retrying world registration…"
            } else {
                "Starting world…"
            }));
        }
        let this = self.clone();
        std::thread::spawn(move || {
            let Ok(_guard) = this.operation.lock() else {
                this.state.send_replace(State::Failed(
                    "Local hosting operation failed. Restart the client to retry.".into(),
                ));
                return;
            };
            let result = (|| {
                let directory = this
                    .directory
                    .as_ref()
                    .ok_or("No save directory. Set MMORPG_CONFIG_DIR to a writable directory.")?;
                if stop {
                    local_world::stop(directory).map_err(|error| error.to_string())?;
                    return Ok(State::Stopped);
                }
                let ready = |world: RunningWorld| -> Result<State, String> {
                    let status = world.share_status().map_err(|e| e.to_string())?;
                    Ok(State::Ready(world, status))
                };
                if let Some((key, allow)) = decision {
                    let world = local_world::running(directory)
                        .map_err(|e| e.to_string())?
                        .ok_or("Local world stopped")?;
                    world
                        .guest_decision(&key, allow)
                        .map_err(|e| e.to_string())?;
                    return ready(world);
                }
                if share {
                    let world = local_world::running(directory)
                        .map_err(|e| e.to_string())?
                        .ok_or("Local world stopped")?;
                    world.retry_sharing().map_err(|e| e.to_string())?;
                    return ready(world);
                }
                if probe {
                    return local_world::running(directory)
                        .map_err(|e| e.to_string())
                        .and_then(|world| world.map_or(Ok(State::Stopped), ready))
                        .map_err(|error| {
                            format!(
                                "Cannot reach the local world: {error}. Check {}.",
                                directory.join("host.log").display()
                            )
                        });
                }
                let executable = std::env::current_exe()
                    .map_err(|error| error.to_string())?
                    .with_file_name(if cfg!(windows) {
                        "mmorpg-zone-host.exe"
                    } else {
                        "mmorpg-zone-host"
                    });
                local_world::start(directory, &executable)
                    .map_err(|e| e.to_string())
                    .and_then(ready)
                    .map_err(|error| error.to_string())
            })();
            // A probe started before a user action must not replace its Working state.
            if this.revision.load(Ordering::Acquire) == revision {
                this.state
                    .send_replace(result.unwrap_or_else(State::Failed));
            }
        });
    }

    /// Called after Host World selected a character. Wait for the real /readyz result.
    pub async fn ready(&self) -> Result<RunningWorld, String> {
        let mut state = self.state.subscribe();
        loop {
            let current = state.borrow_and_update().clone();
            match current {
                State::Ready(world, _) => return Ok(world),
                State::Working(_) => {}
                State::Failed(error) => return Err(error),
                State::Stopped => return Err("Choose Host World to start the saved world.".into()),
            }
            state
                .changed()
                .await
                .map_err(|_| "Local host controls closed.")?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn stop_is_not_lost_while_a_background_probe_owns_the_worker() {
        let directory = tempfile::tempdir().unwrap();
        let hosting = Hosting {
            directory: Some(directory.path().into()),
            ..Hosting::default()
        };
        let mut state = {
            let _probe = hosting.operation.lock().unwrap();
            hosting.stop();
            assert!(matches!(hosting.state(), State::Working(_)));
            hosting.state.subscribe()
        };
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            state.wait_for(|state| matches!(state, State::Stopped)),
        )
        .await
        .unwrap()
        .unwrap();
    }
}
