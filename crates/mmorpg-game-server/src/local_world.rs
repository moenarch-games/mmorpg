//! Game-owned local world lifecycle. The host holds the save lock until graceful shutdown
//! has flushed persistence. Clients own no process handle that could end the world on exit.
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream, UdpSocket},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use tokio::sync::mpsc;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
const TIMEOUT: Duration = Duration::from_secs(30);
/// A zone host that cannot bind the ports it picked exits with this code, and the launcher
/// starts it again on fresh ones: another process may take a port between the pick and the
/// host's bind (#1147).
pub const PORT_CONFLICT_EXIT_CODE: i32 = 75;
const PORT_CONFLICT_ATTEMPTS: u32 = 3;

/// Whether serving failed because the status listener or the WebTransport endpoint could not
/// bind its port. Both bind before recovery is consumed, so a retry starts from the same save.
pub fn is_port_conflict(error: &(dyn std::error::Error + 'static)) -> bool {
    use game_server::{HostStatusServerError, MatchHostTransportError};
    matches!(
        error.downcast_ref::<HostStatusServerError>(),
        Some(
            HostStatusServerError::Bind(_)
                | HostStatusServerError::Transport(MatchHostTransportError::Endpoint(_))
        )
    )
}

#[derive(Clone, Serialize, Deserialize)]
pub struct RunningWorld {
    pub port: u16,
    pub status_port: u16,
    control: SocketAddr,
    token: String,
    pub certificate: PathBuf,
    /// The host process holding the save lock, so an unresponsive holder can be named
    /// (#1050). Older hosts did not record it.
    #[serde(default)]
    pid: Option<u32>,
}

impl RunningWorld {
    pub fn sign_owner_claim(&self, body: &[u8], player: u32, epoch: u32) -> Vec<u8> {
        let mut message = player.to_be_bytes().to_vec();
        message.extend_from_slice(&epoch.to_be_bytes());
        message.extend_from_slice(body);
        let signature = ring::hmac::sign(
            &ring::hmac::Key::new(ring::hmac::HMAC_SHA256, self.token.as_bytes()),
            &message,
        );
        let mut payload = vec![1, 3];
        payload.extend_from_slice(signature.as_ref());
        payload.extend_from_slice(body);
        payload
    }

    fn control_socket(&self, action: &str) -> Result<TcpStream> {
        let mut socket = TcpStream::connect_timeout(&self.control, Duration::from_millis(300))?;
        socket.set_read_timeout(Some(Duration::from_secs(1)))?;
        socket.set_write_timeout(Some(Duration::from_secs(1)))?;
        writeln!(socket, "{} {action}", self.token)?;
        let mut response = [0; 2];
        socket.read_exact(&mut response)?;
        if &response != b"OK" {
            return Err("Local hosting action failed. Check local world status and retry.".into());
        }
        Ok(socket)
    }

    pub fn share_status(&self) -> Result<crate::hosted_access::ShareStatus> {
        let mut socket = self.control_socket("status")?;
        let mut size = [0; 4];
        socket.read_exact(&mut size)?;
        let length = u32::from_be_bytes(size) as usize;
        if length > 1_048_576 {
            return Err("Invalid local hosting status".into());
        }
        let mut bytes = vec![0; length];
        socket.read_exact(&mut bytes)?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub fn guest_decision(&self, key: &str, allow: bool) -> Result<()> {
        if key.len() != 64 || !key.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("Invalid guest key".into());
        }
        self.request(&format!(
            "{} {key}",
            if allow { "approve" } else { "remove" }
        ))
    }
    pub fn retry_sharing(&self) -> Result<()> {
        self.request("share")
    }
    #[must_use]
    pub fn url(&self) -> String {
        format!("https://localhost:{}/game/matches/zone-1", self.port)
    }

    fn request(&self, action: &str) -> Result<()> {
        self.control_socket(action).map(|_| ())
    }

    fn ready(&self) -> Result<()> {
        self.request("status")?;
        let address = SocketAddr::from(([127, 0, 0, 1], self.status_port));
        let mut socket = TcpStream::connect_timeout(&address, Duration::from_millis(300))?;
        socket.set_read_timeout(Some(Duration::from_secs(1)))?;
        socket.set_write_timeout(Some(Duration::from_secs(1)))?;
        socket.write_all(b"GET /readyz HTTP/1.0\r\nHost: localhost\r\n\r\n")?;
        let mut response = [0; 12];
        socket.read_exact(&mut response)?;
        if &response != b"HTTP/1.1 200" && &response != b"HTTP/1.0 200" {
            return Err("local world is not ready".into());
        }
        Ok(())
    }
}

fn lock_file(directory: &Path) -> Result<File> {
    fs::create_dir_all(directory)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
    }
    Ok(OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(directory.join("world.lock"))?)
}

fn read_running(directory: &Path) -> Result<RunningWorld> {
    Ok(serde_json::from_slice(&fs::read(
        directory.join("running.json"),
    )?)?)
}

/// Returns a ready managed host, or none if the save is unlocked. A held but unhealthy
/// lock is an error: starting another writer is never a recovery strategy.
pub fn running(directory: &Path) -> Result<Option<RunningWorld>> {
    let file = lock_file(directory)?;
    if file.try_lock().is_ok() {
        return Ok(None);
    }
    let world = read_running(directory)?;
    world.ready().map_err(|error| unresponsive(&world, error))?;
    Ok(Some(world))
}

/// A held lock whose holder does not answer is a hung or foreign host, not a disk or
/// network fault: a timed-out read would otherwise surface as a bare EAGAIN (#1050).
fn unresponsive(
    world: &RunningWorld,
    error: Box<dyn std::error::Error + Send + Sync>,
) -> Box<dyn std::error::Error + Send + Sync> {
    if error.downcast_ref::<std::io::Error>().is_none() {
        return error;
    }
    let holder = world.pid.map_or("its zone host".to_owned(), |pid| {
        format!("zone host process {pid}")
    });
    format!(
        "This world is held by {holder}, which is not responding ({error}). Stop that \
         process (it may be an older build that is still running), then choose Host World \
         again; saved data is kept"
    )
    .into()
}

/// Starts the sibling zone-host binary, or reuses the holder of this save's lock.
/// Concurrent launchers may spawn contenders, but only the lock winner can read saves
/// or create a simulation. Failed starts retain logs and never reset a save.
pub fn start(directory: &Path, executable: &Path) -> Result<RunningWorld> {
    let launcher = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open({
            lock_file(directory)?;
            directory.join("launcher.lock")
        })?;
    launcher.lock()?;
    let file = lock_file(directory)?;
    let mut host = None;
    if file.try_lock().is_ok() {
        // Release before spawning: the child must acquire and retain the lock itself.
        drop(file);
        host = Some(spawn_host(directory, executable)?);
    }
    let mut attempts = 1;
    let deadline = Instant::now() + TIMEOUT;
    let outcome = loop {
        let last = match running(directory) {
            Ok(Some(world)) => break Ok(world),
            Ok(None) => None,
            Err(error) => Some(error),
        };
        // A host that lost a port before binding it has exited and released the lock.
        if let Some(child) = &mut host
            && attempts < PORT_CONFLICT_ATTEMPTS
            && child
                .try_wait()
                .ok()
                .flatten()
                .is_some_and(|status| status.code() == Some(PORT_CONFLICT_EXIT_CODE))
        {
            attempts += 1;
            *child = spawn_host(directory, executable)?;
            continue;
        }
        if Instant::now() >= deadline {
            break Err(match last {
                // The lock is held by a host that never answered: name it (#1050).
                Some(error) if error.to_string().contains("not responding") => error,
                _ => format!("World did not become ready. See {}. Retry Host World after correcting the error; saved data was retained.", directory.join("host.log").display()).into(),
            });
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    if let Some(mut child) = host {
        // Reap the process without tying its lifetime to the launching client.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
    outcome
}

fn spawn_host(directory: &Path, executable: &Path) -> Result<std::process::Child> {
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(directory.join("host.log"))?;
    let mut command = Command::new(executable);
    // Managed hosting must not inherit development endpoints, stores or routing.
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("MMORPG_")
            && !matches!(
                name.to_str(),
                Some(
                    "MMORPG_SHARED_ART_DIR"
                        | "MMORPG_TEST_SETUP_ENDPOINT"
                        | "MMORPG_WEBRTC_BIND_IP"
                        | "MMORPG_TEST_OFFLINE_HOST"
                )
            )
        {
            command.env_remove(name);
        }
    }
    command
        .arg("--local-world")
        .arg(directory)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x00000008 | 0x00000200);
    }
    Ok(command.spawn().map_err(|error| {
        format!(
            "Cannot start {}: {error}. Build/install mmorpg-zone-host beside mmorpg-client.",
            executable.display()
        )
    })?)
}

/// Requests graceful shutdown and waits for the *save lock*, including the final flush.
/// Never kills a PID read from a stale file and never forces a second shutdown signal.
pub fn stop(directory: &Path) -> Result<()> {
    let file = lock_file(directory)?;
    if file.try_lock().is_ok() {
        return Ok(());
    }
    read_running(directory)?.request("stop")?;
    let deadline = Instant::now() + TIMEOUT;
    while file.try_lock().is_err() {
        if Instant::now() >= deadline {
            return Err(format!(
                "World is still stopping. Check {} before retrying; do not delete world.lock.",
                directory.join("host.log").display()
            )
            .into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}

/// Retained by the zone-host main function through persistence shutdown.
pub struct LocalWorldHost {
    _lock: File,
    directory: PathBuf,
    pub world: RunningWorld,
    pub private_key: PathBuf,
    listener: Option<TcpListener>,
    pub managed: crate::hosted_access::ManagedWorld,
}

impl LocalWorldHost {
    /// Acquires exclusive ownership before touching certificate or persistence data.
    pub fn acquire(directory: &Path) -> Result<Self> {
        let directory = std::path::absolute(directory)?;
        let lock = lock_file(&directory)?;
        lock.try_lock()
            .map_err(|_| "This world is already running; use Host World to reconnect.")?;
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        // The host binds these itself; losing one first exits with PORT_CONFLICT_EXIT_CODE.
        let port = UdpSocket::bind(("127.0.0.1", 0))?.local_addr()?.port();
        let status_port = TcpListener::bind(("127.0.0.1", 0))?.local_addr()?.port();
        let identity = wtransport::Identity::self_signed(["localhost", "127.0.0.1"])?;
        let certificate = directory.join("certificate.pem");
        let private_key = directory.join("key.pem");
        fs::write(
            &certificate,
            identity.certificate_chain().as_slice()[0].to_pem(),
        )?;
        fs::write(&private_key, identity.private_key().to_secret_pem())?;
        let mut random = [0; 32];
        getrandom::fill(&mut random)?;
        let token = random.iter().map(|byte| format!("{byte:02x}")).collect();
        let world = RunningWorld {
            port,
            status_port,
            control: listener.local_addr()?,
            token,
            certificate,
            pid: Some(std::process::id()),
        };
        let managed = crate::hosted_access::ManagedWorld::open(&directory, world.token.as_bytes())?;
        fs::write(directory.join("running.json"), serde_json::to_vec(&world)?)?;
        Ok(Self {
            _lock: lock,
            directory,
            world,
            private_key,
            listener: Some(listener),
            managed,
        })
    }

    /// Local control is loopback only and requires the private per-run capability.
    pub fn serve_control(&mut self, shutdown: mpsc::Sender<()>) -> Result<()> {
        let listener = self
            .listener
            .take()
            .ok_or("local control already started")?;
        listener.set_nonblocking(true)?;
        let listener = tokio::net::TcpListener::from_std(listener)?;
        let token = self.world.token.clone();
        let managed = self.managed.clone();
        tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut bytes = [0; 256];
                let request = tokio::time::timeout(Duration::from_secs(1), async {
                    let mut length = 0;
                    while length < bytes.len() {
                        let read = socket.read(&mut bytes[length..]).await?;
                        if read == 0 {
                            break;
                        }
                        length += read;
                        if bytes[..length].contains(&b'\n') {
                            break;
                        }
                    }
                    Ok::<_, std::io::Error>(
                        String::from_utf8_lossy(&bytes[..length]).trim().to_owned(),
                    )
                })
                .await;
                if let Ok(Ok(request)) = request {
                    if request == format!("{token} status") {
                        if let Ok(bytes) = serde_json::to_vec(&managed.status()) {
                            let mut response = b"OK".to_vec();
                            response.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
                            response.extend_from_slice(&bytes);
                            let _ = socket.write_all(&response).await;
                        }
                    } else if request == format!("{token} share") {
                        managed.request_share();
                        let _ = socket.write_all(b"OK").await;
                    } else if let Some(action) = request.strip_prefix(&format!("{token} "))
                        && let Some((kind, key)) = action.split_once(' ')
                        && matches!(kind, "approve" | "remove")
                    {
                        let access = managed.access.clone();
                        let key = key.to_owned();
                        let allow = kind == "approve";
                        let success =
                            tokio::task::spawn_blocking(move || access.decide(&key, allow))
                                .await
                                .is_ok_and(|r| r.is_ok());
                        let _ = socket.write_all(if success { b"OK" } else { b"NO" }).await;
                    } else if request == format!("{token} stop") {
                        managed.clear(None);
                        let _ = socket.write_all(b"OK").await;
                        let _ = shutdown.send(()).await;
                        break;
                    }
                }
            }
        });
        Ok(())
    }
}

impl Drop for LocalWorldHost {
    fn drop(&mut self) {
        // Remove only transient discovery, while still holding the ownership lock.
        let _ = fs::remove_file(self.directory.join("running.json"));
        let _ = fs::remove_file(&self.private_key);
        let _ = fs::remove_file(&self.world.certificate);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_held_lock_whose_host_never_answers_is_named_not_reported_as_eagain() {
        let directory = tempfile::tempdir().unwrap();
        // A hung host: it holds the save lock and its control socket accepts but never replies.
        let lock = lock_file(directory.path()).unwrap();
        lock.try_lock().unwrap();
        let silent = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let world = RunningWorld {
            port: 1,
            status_port: 1,
            control: silent.local_addr().unwrap(),
            token: "t".into(),
            certificate: directory.path().join("certificate.pem"),
            pid: Some(4242),
        };
        fs::write(
            directory.path().join("running.json"),
            serde_json::to_vec(&world).unwrap(),
        )
        .unwrap();
        let error = running(directory.path()).err().unwrap().to_string();
        assert!(error.contains("zone host process 4242"), "{error}");
        assert!(error.contains("not responding"), "{error}");
        // Discovery files written by older hosts, without a PID, still load.
        let mut old = serde_json::to_value(&world).unwrap();
        old.as_object_mut().unwrap().remove("pid");
        fs::write(directory.path().join("running.json"), old.to_string()).unwrap();
        let error = running(directory.path()).err().unwrap().to_string();
        assert!(error.contains("held by its zone host"), "{error}");
    }

    #[test]
    fn only_a_lost_port_restarts_the_host() {
        use game_server::{HostStatusServerError, MatchHostTransportError};
        let boxed = |error: HostStatusServerError| -> Box<dyn std::error::Error + Send + Sync> {
            Box::new(error)
        };
        let taken = boxed(HostStatusServerError::Bind("address in use".into()));
        assert!(is_port_conflict(taken.as_ref()));
        let endpoint = boxed(HostStatusServerError::Transport(
            MatchHostTransportError::Endpoint("address in use".into()),
        ));
        assert!(is_port_conflict(endpoint.as_ref()));
        let identity = boxed(HostStatusServerError::Transport(
            MatchHostTransportError::Identity("bad key".into()),
        ));
        assert!(!is_port_conflict(identity.as_ref()));
        let other: Box<dyn std::error::Error + Send + Sync> = "corrupt save".into();
        assert!(!is_port_conflict(other.as_ref()));
    }
}
