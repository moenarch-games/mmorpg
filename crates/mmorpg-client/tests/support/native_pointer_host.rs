//! Disposable real TLS/WebTransport zone host, matching connected_world.rs.
use game_server::{
    BrowserRoutePrefix, MatchHost, MatchHostWebTransportConfig, MatchRuntime,
    serve_match_host_with_shutdown,
};
use mmorpg_core::ZoneId;
use mmorpg_game_server::zone_match_id;
use std::{net::UdpSocket, path::PathBuf, time::Duration};
/// A real WebTransport zone host on an OS-assigned port with disposable TLS.
pub(super) struct LocalHost {
    _temporary: tempfile::TempDir,
    pub(super) certificate: PathBuf,
    pub(super) url: String,
    _shutdown: tokio::sync::mpsc::Sender<()>,
    task: tokio::task::JoinHandle<Result<(), game_server::MatchHostTransportError>>,
}

impl LocalHost {
    pub(super) fn start<S: game_server::GameSimulation>(simulation: S, zone_id: ZoneId) -> Self {
        let mut host = MatchHost::new(1).unwrap();
        host.insert(
            zone_match_id(zone_id).unwrap(),
            MatchRuntime::new(simulation, 120),
        )
        .map_err(|failure| failure.into_parts().0)
        .unwrap();
        let temporary = tempfile::tempdir().unwrap();
        let certificate = temporary.path().join("cert.pem");
        let key = temporary.path().join("key.pem");
        let identity = wtransport::Identity::self_signed(["localhost", "127.0.0.1"]).unwrap();
        std::fs::write(
            &certificate,
            identity.certificate_chain().as_slice()[0].to_pem(),
        )
        .unwrap();
        let mut key_options = std::fs::OpenOptions::new();
        key_options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            key_options.mode(0o600);
        }
        use std::io::Write;
        key_options
            .open(&key)
            .unwrap()
            .write_all(identity.private_key().to_secret_pem().as_bytes())
            .unwrap();
        // The shared host accepts a port rather than a pre-bound endpoint. Select an
        // OS-assigned disposable port, releasing it immediately before host startup.
        let reservation = UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = reservation.local_addr().unwrap().port();
        let (shutdown, receiver) = tokio::sync::mpsc::channel(1);
        drop(reservation);
        let task = tokio::spawn(serve_match_host_with_shutdown(
            host,
            MatchHostWebTransportConfig {
                port,
                certificate_pem: certificate.clone(),
                private_key_pem: key,
                route_prefix: BrowserRoutePrefix::new("/game").unwrap(),
                allowed_origins: None,
                drain_grace: Duration::from_millis(10),
            },
            receiver,
        ));
        Self {
            _temporary: temporary,
            certificate,
            url: format!(
                "https://localhost:{port}/game/matches/zone-{}",
                zone_id.get()
            ),
            _shutdown: shutdown,
            task,
        }
    }
}
impl Drop for LocalHost {
    fn drop(&mut self) {
        self.task.abort();
    }
}
