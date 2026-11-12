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

#[derive(Clone, Serialize, Deserialize)]
pub struct RunningWorld {
    pub port: u16,
    pub status_port: u16,
    control: SocketAddr,
    token: String,
    pub certificate: PathBuf,
}

impl RunningWorld {
    #[must_use]
    pub fn url(&self) -> String {
        format!("https://localhost:{}/game/matches/zone-1", self.port)
    }

    fn request(&self, action: &str) -> Result<()> {
        let mut socket = TcpStream::connect_timeout(&self.control, Duration::from_millis(300))?;
        socket.set_read_timeout(Some(Duration::from_secs(1)))?;
        socket.set_write_timeout(Some(Duration::from_secs(1)))?;
        writeln!(socket, "{} {action}", self.token)?;
        let mut response = [0; 2];
        socket.read_exact(&mut response)?;
        if &response != b"OK" {
            return Err("local host did not recognize this world; do not delete its lock".into());
        }
        Ok(())
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
    world.ready()?;
    Ok(Some(world))
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
    if file.try_lock().is_ok() {
        // Release before spawning: the child must acquire and retain the lock itself.
        drop(file);
        let log = OpenOptions::new()
            .create(true)
            .append(true)
            .open(directory.join("host.log"))?;
        let mut command = Command::new(executable);
        // Managed hosting must not inherit development endpoints, stores or routing.
        for (name, _) in std::env::vars_os() {
            if name.to_string_lossy().starts_with("MMORPG_") {
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
        let mut child = command.spawn().map_err(|error| {
            format!(
                "Cannot start {}: {error}. Build/install mmorpg-zone-host beside mmorpg-client.",
                executable.display()
            )
        })?;
        // Reap the process without tying its lifetime to the launching client.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
    let deadline = Instant::now() + TIMEOUT;
    loop {
        if let Ok(Some(world)) = running(directory) {
            return Ok(world);
        }
        if Instant::now() >= deadline {
            return Err(format!("World did not become ready. See {}. Retry Host World after correcting the error; saved data was retained.", directory.join("host.log").display()).into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
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
}

impl LocalWorldHost {
    /// Acquires exclusive ownership before touching certificate or persistence data.
    pub fn acquire(directory: &Path) -> Result<Self> {
        let directory = std::path::absolute(directory)?;
        let lock = lock_file(&directory)?;
        lock.try_lock()
            .map_err(|_| "This world is already running; use Host World to reconnect.")?;
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
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
        };
        fs::write(directory.join("running.json"), serde_json::to_vec(&world)?)?;
        Ok(Self {
            _lock: lock,
            directory,
            world,
            private_key,
            listener: Some(listener),
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
        tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut bytes = [0; 128];
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
                        let _ = socket.write_all(b"OK").await;
                    } else if request == format!("{token} stop") {
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
