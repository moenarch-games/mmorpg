#![cfg(feature = "postgres")]
//! Real process + WebTransport check of the opt-in PostgreSQL/RustFS entrypoint.
use game_server::{BrowserRoutePrefix, MatchId, SnapshotReassembler, WELCOME_BYTES};
use mmorpg_characters::{CharacterStore, PostgresEntries};
use mmorpg_core::{CharacterId, ClassChoice, PlayerClass, Sex};
use std::{
    collections::BTreeMap,
    fs::File,
    path::{Path, PathBuf},
    process::{Child, Command},
    time::Duration,
};
use wtransport::{ClientConfig, Connection, Endpoint, Identity};

struct Directory(PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("persistence host failure logs: {}", self.0.display());
            return;
        }
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
struct Process(Child);
impl Process {
    fn kill_and_wait(&mut self) {
        self.0.kill().unwrap();
        self.0.wait().unwrap();
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}
fn role(url: &str, name: &str) -> String {
    format!(
        "{url}{}options=-c%20role%3D{name}",
        if url.contains('?') { '&' } else { '?' }
    )
}
fn command(env: &BTreeMap<String, String>, mode: &str) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mmorpg-zone-host"));
    command
        .envs(env)
        .env_remove("MMORPG_CHARACTER_DIR")
        .env_remove("MMORPG_RECOVERY_DIR")
        .env("MMORPG_PERSISTENCE_MODE", mode);
    command
}
async fn operator(env: &BTreeMap<String, String>, mode: &str) {
    let env = env.clone();
    let mode = mode.to_owned();
    let output = tokio::task::spawn_blocking(move || command(&env, &mode).output().unwrap())
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
async fn control(connection: &Connection, payload: &[u8]) -> game_server::ControlResponse {
    tokio::time::timeout(Duration::from_secs(10), async {
        let (mut send, mut receive) = connection.open_bi().await.unwrap().await.unwrap();
        send.write_all(&game_server::encode_control_request(payload).unwrap())
            .await
            .unwrap();
        send.finish().await.unwrap();
        let mut header = [0; game_server::CONTROL_HEADER_BYTES];
        receive.read_exact(&mut header).await.unwrap();
        let length = usize::from(u16::from_be_bytes([header[6], header[7]]));
        assert!(length <= game_server::MAX_CONTROL_PAYLOAD_BYTES);
        let mut frame = header.to_vec();
        frame.resize(header.len() + length, 0);
        receive
            .read_exact(&mut frame[header.len()..])
            .await
            .unwrap();
        game_server::decode_control_response(&frame).unwrap()
    })
    .await
    .unwrap()
}
async fn welcome(connection: &Connection) -> game_server::Welcome {
    let mut stream = connection.accept_uni().await.unwrap();
    let mut bytes = [0; WELCOME_BYTES];
    stream.read_exact(&mut bytes).await.unwrap();
    game_server::decode_welcome(&bytes).unwrap()
}
async fn profile(url: &str, id: &CharacterId) -> mmorpg_characters::Committed {
    let url = url.to_owned();
    let id = id.clone();
    tokio::task::spawn_blocking(move || {
        let store =
            CharacterStore::new(PostgresEntries::connect(&url, Duration::from_secs(5)).unwrap());
        store.committed(&id).unwrap().unwrap()
    })
    .await
    .unwrap()
}
async fn stream_context(url: &str, zone: u32) -> (String, i64) {
    let url = url.to_owned();
    tokio::task::spawn_blocking(move || {
        let mut client = postgres::Client::connect(&url, postgres::NoTls).unwrap();
        let row = client
            .query_one(
                "SELECT world_id::text,timeline FROM mmorpg_persistence.streams WHERE zone_id=$1",
                &[&i64::from(zone)],
            )
            .unwrap();
        (row.get(0), row.get(1))
    })
    .await
    .unwrap()
}

fn start(env: &BTreeMap<String, String>, mode: &str, log: &Path) -> Process {
    Process(
        command(env, mode)
            .stdout(File::create(log).unwrap())
            .stderr(File::create(log.with_extension("stderr")).unwrap())
            .spawn()
            .unwrap(),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn postgres_host_acknowledges_claim_survives_process_loss_and_releases_on_logout() {
    let (Ok(primary), Ok(standby), Ok(s3), Ok(bucket)) = (
        std::env::var("MMORPG_CHARACTERS_POSTGRES_URL"),
        std::env::var("MMORPG_POSTGRES_STANDBY_URL"),
        std::env::var("MMORPG_CHECKPOINT_S3_ENDPOINT"),
        std::env::var("MMORPG_CHECKPOINT_S3_BUCKET"),
    ) else {
        return;
    };
    let directory = Directory(std::env::temp_dir().join(format!(
            "mmorpg-persistence-host-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
    std::fs::create_dir_all(&directory.0).unwrap();
    let identity = Identity::self_signed(["localhost", "127.0.0.1"]).unwrap();
    let cert = directory.0.join("cert.pem");
    let key = directory.0.join("key.pem");
    identity
        .certificate_chain()
        .store_pemfile(&cert)
        .await
        .unwrap();
    identity
        .private_key()
        .store_secret_pemfile(&key)
        .await
        .unwrap();
    let reservation = std::net::UdpSocket::bind(("127.0.0.1", 0)).unwrap();
    let port = reservation.local_addr().unwrap().port();
    drop(reservation);
    let mut env = BTreeMap::from([
        ("MMORPG_MIGRATION_DATABASE_URL".into(), primary.clone()),
        (
            "MMORPG_COORDINATOR_DATABASE_URL".into(),
            role(&primary, "mmorpg_zone_coordinator"),
        ),
        (
            "MMORPG_COORDINATOR_STANDBY_URL".into(),
            role(&standby, "mmorpg_zone_coordinator"),
        ),
        (
            "MMORPG_CHARACTER_STANDBY_URL".into(),
            role(&standby, "mmorpg_character_profiles"),
        ),
        (
            "MMORPG_ZONE_DATABASE_URL".into(),
            role(&primary, "mmorpg_zone_journal"),
        ),
        (
            "MMORPG_ZONE_STANDBY_URL".into(),
            role(&standby, "mmorpg_zone_journal"),
        ),
        ("MMORPG_CHECKPOINT_S3_ENDPOINT".into(), s3),
        ("MMORPG_CHECKPOINT_S3_BUCKET".into(), bucket),
        ("MMORPG_CHECKPOINT_ALLOW_HTTP".into(), "1".into()),
        ("MMORPG_HOST_ID".into(), "process-loss-test".into()),
        ("MMORPG_PORT".into(), port.to_string()),
        ("MMORPG_STATUS_PORT".into(), "0".into()),
        ("MMORPG_DRAIN_GRACE_MS".into(), "0".into()),
        ("MMORPG_CERT_PEM".into(), cert.to_str().unwrap().into()),
        ("MMORPG_KEY_PEM".into(), key.to_str().unwrap().into()),
    ]);
    operator(&env, "migrate").await;
    let setup = primary.clone();
    let zone:u32=tokio::task::spawn_blocking(move||{let mut client=postgres::Client::connect(&setup,postgres::NoTls).unwrap();let value:i64=client.query_one("SELECT greatest(coalesce((SELECT max(zone_id) FROM mmorpg_coordination.zones),1000),coalesce((SELECT max(zone_id) FROM mmorpg_persistence.streams),1000))+1",&[]).unwrap().get(0);value.try_into().unwrap()}).await.unwrap();
    env.insert("MMORPG_ZONE_IDS".into(), zone.to_string());
    let endpoint = Endpoint::client(
        ClientConfig::builder()
            .with_bind_default()
            .with_server_certificate_hashes([identity.certificate_chain().as_slice()[0].hash()])
            .build(),
    )
    .unwrap();
    let id = MatchId::new(format!("zone-{zone}")).unwrap();
    let route = BrowserRoutePrefix::new("/game").unwrap();
    let url = format!("https://localhost:{port}{}", route.match_path(&id));
    let mut host = start(&env, "bootstrap", &directory.0.join("first.log"));
    let connection = tokio::time::timeout(Duration::from_secs(10), endpoint.connect(&url))
        .await
        .unwrap()
        .unwrap();
    let first = welcome(&connection).await;
    assert!(control(&connection, &[1, 0]).await.accepted);
    let character = CharacterId::new(format!("process-loss-{zone}")).unwrap();
    let class = ClassChoice {
        class: PlayerClass::Warden,
        sex: Sex::Male,
    };
    let mut claim = vec![1, 1, class.appearance() as u8];
    claim.extend_from_slice(character.as_str().as_bytes());
    let accepted = control(&connection, &claim).await;
    assert!(
        accepted.accepted,
        "{}",
        String::from_utf8_lossy(&accepted.payload)
    );
    let before = profile(&role(&standby, "mmorpg_character_profiles"), &character).await;
    assert_eq!(before.record.class, class);
    assert!(before.revision >= 1);
    // No graceful checkpoint/shutdown: the acknowledgement must already survive.
    host.kill_and_wait();
    drop(connection);
    env.insert("MMORPG_CONFIRM_SOURCE_STOPPED".into(), "1".into());
    env.insert("MMORPG_EXPECTED_ZONE_EPOCH".into(), "1".into());
    operator(&env, "retire").await;
    let mut host = start(&env, "recover", &directory.0.join("second.log"));
    let reconnect = format!(
        "https://localhost:{port}{}",
        route.reconnect_path(&id, game_server::ReconnectToken(first.reconnect_token))
    );
    let connection = tokio::time::timeout(Duration::from_secs(10), endpoint.connect(&reconnect))
        .await
        .unwrap()
        .unwrap();
    let second = welcome(&connection).await;
    assert_eq!(second.player_id, first.player_id);
    assert!(second.connection_epoch > first.connection_epoch);
    assert!(control(&connection, &claim).await.accepted);
    let after = profile(&role(&standby, "mmorpg_character_profiles"), &character).await;
    assert_eq!(
        (
            &after.record.inventory,
            &after.record.equipment,
            after.record.level,
            after.record.experience,
            after.record.copper
        ),
        (
            &before.record.inventory,
            &before.record.equipment,
            before.record.level,
            before.record.experience,
            before.record.copper
        )
    );
    let mut reassembler = SnapshotReassembler::new();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let datagram = connection.receive_datagram().await.unwrap();
            if let Some(frame) = reassembler.accept(&datagram).unwrap() {
                let snapshot = mmorpg_protocol::decode_snapshot(&frame.payload).unwrap();
                if snapshot.viewer.class == Some(class) {
                    break;
                }
            }
        }
    })
    .await
    .unwrap();
    let logout = control(&connection, &[1, 2]).await;
    assert!(logout.accepted);
    let offline = profile(&role(&standby, "mmorpg_character_profiles"), &character).await;
    assert!(offline.leased_by.is_none());
    host.kill_and_wait();
    env.insert("MMORPG_EXPECTED_ZONE_EPOCH".into(), "2".into());
    operator(&env, "retire").await;
    drop(connection);
    let original_context = stream_context(&primary, zone).await;
    let mut reset = start(&env, "reset", &directory.0.join("reset.log"));
    let connection = tokio::time::timeout(Duration::from_secs(10), endpoint.connect(&url))
        .await
        .unwrap()
        .unwrap();
    welcome(&connection).await;
    let reset_context = stream_context(&primary, zone).await;
    assert_eq!(reset_context.0, original_context.0);
    assert_eq!(reset_context.1, original_context.1 + 1);
    assert!(control(&connection, &claim).await.accepted);
    let retained = profile(&role(&standby, "mmorpg_character_profiles"), &character).await;
    assert_eq!(
        (
            &retained.record.inventory,
            &retained.record.equipment,
            retained.record.level,
            retained.record.experience,
            retained.record.copper
        ),
        (
            &before.record.inventory,
            &before.record.equipment,
            before.record.level,
            before.record.experience,
            before.record.copper
        )
    );
    assert!(control(&connection, &[1, 2]).await.accepted);
    reset.kill_and_wait();
    env.insert("MMORPG_EXPECTED_ZONE_EPOCH".into(), "3".into());
    operator(&env, "retire").await;
}
