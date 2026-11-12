//! Native hosting controls; work runs off the window thread, status is always observed.
use mmorpg_game_server::local_world::{self, RunningWorld};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tokio::sync::watch;

#[derive(Clone)]
pub enum State {
    Stopped,
    Working(&'static str),
    Ready(RunningWorld),
    Failed(String),
}

#[derive(Clone)]
pub struct Hosting {
    state: watch::Sender<State>,
    operation: Arc<Mutex<()>>,
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

    fn run(&self, stop: bool, probe: bool) {
        if matches!(self.state(), State::Working(_)) {
            return;
        }
        let Ok(guard) = self.operation.try_lock() else {
            return;
        };
        self.state.send_replace(State::Working(if stop {
            "Stopping world and saving…"
        } else if probe {
            "Checking local world…"
        } else {
            "Starting world…"
        }));
        let this = self.clone();
        // Reserve the operation before leaving the window thread. The next operation
        // observes Working, while the worker serializes the actual filesystem work.
        let operation = Arc::clone(&self.operation);
        std::thread::spawn(move || {
            let Ok(_guard) = operation.lock() else {
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
                if probe {
                    return local_world::running(directory)
                        .map(|world| world.map_or(State::Stopped, State::Ready))
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
                    .map(State::Ready)
                    .map_err(|error| error.to_string())
            })();
            this.state
                .send_replace(result.unwrap_or_else(State::Failed));
        });
        drop(guard);
    }

    /// Called after Host World selected a character. Wait for the real /readyz result.
    pub async fn ready(&self) -> Result<RunningWorld, String> {
        let mut state = self.state.subscribe();
        loop {
            let current = state.borrow_and_update().clone();
            match current {
                State::Ready(world) => return Ok(world),
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
