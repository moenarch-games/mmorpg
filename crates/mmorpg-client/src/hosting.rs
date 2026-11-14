//! Native hosting controls; work runs off the window thread, status is always observed.
use mmorpg_game_server::local_world::{self, RunningWorld};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
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
    operation: Arc<AtomicBool>,
    directory: Option<PathBuf>,
    pub available: bool,
}

impl Default for Hosting {
    fn default() -> Self {
        let (state, _) = watch::channel(State::Stopped);
        Self {
            state,
            operation: Arc::default(),
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
        if self
            .operation
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        // Polls must not remove and reposition Copy/Approve underneath the pointer.
        // The operation reservation still serializes background probes and user actions.
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
        // Reserve before leaving the window thread, including probes that retain Ready.
        std::thread::spawn(move || {
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
            this.state
                .send_replace(result.unwrap_or_else(State::Failed));
            this.operation.store(false, Ordering::Release);
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
