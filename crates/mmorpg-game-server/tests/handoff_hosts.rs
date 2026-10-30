#![cfg(feature = "postgres")]
//! Two real zone-host processes hand characters to each other (#145 part 3b):
//! Seam Test West (901) and East (902) share one PostgreSQL database for the
//! characters, the zone grants and the handoff records, and talk over the
//! trusted-LAN transfer link. A walker crosses the seam West → East and a
//! traveller teleports East → West by waystone; each session receives a
//! redirect notice, reconnects to the destination's route and continues
//! there, with the character's lease held by the destination zone.
//!
//! Needs `MMORPG_CHARACTERS_POSTGRES_URL` (a database the test may migrate);
//! without it the test does nothing.
use std::{
    path::PathBuf,
    process::{Child, Command},
    time::Duration,
};

use game_server::{SnapshotReassembler, WELCOME_BYTES};
use mmorpg_characters::{CharacterStore, LeaseHolder, PostgresEntries};
use mmorpg_core::{
    CharacterId, CharacterLocation, CharacterRecord, ClassChoice, PLAYER_HALF_EXTENTS_UNITS,
    PlayerClass, SEAM_TEST_EAST, SEAM_TEST_WEST, Sex, WaystoneSet, ZoneCommand, ZoneId,
    ZoneSimulation, seam_test, waystone,
};
use mmorpg_protocol::{
    RedirectNotice, decode_redirect_notice, decode_snapshot, is_redirect_notice,
};
use wtransport::{ClientConfig, Connection, Endpoint, Identity};

struct Directory(PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("handoff host logs: {}", self.0.display());
            return;
        }
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

const CLASS: ClassChoice = ClassChoice {
    class: PlayerClass::Warden,
    sex: Sex::Female,
};

fn udp_port() -> u16 {
    std::net::UdpSocket::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn tcp_port() -> u16 {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn session_route(port: u16, zone: ZoneId) -> String {
    format!("https://localhost:{port}/game/matches/zone-{}", zone.get())
}

/// A Seam Test warden placed at `position` in `zone`, with `waystones`.
fn record(
    id: &CharacterId,
    zone: ZoneId,
    position: [i32; 2],
    waystones: WaystoneSet,
) -> CharacterRecord {
    let mut simulation =
        ZoneSimulation::with_content(zone, mmorpg_core::content_for_zone(zone)).unwrap();
    simulation.create_character(1, CLASS).unwrap();
    let mut record = simulation.capture_character(1, id.clone()).unwrap();
    record.location = CharacterLocation {
        zone_id: zone,
        position: [position[0], PLAYER_HALF_EXTENTS_UNITS[1], position[1]],
        facing: 0,
    };
    record.waystones = waystones;
    record
}

async fn seed(url: &str, records: Vec<CharacterRecord>) {
    let url = url.to_owned();
    tokio::task::spawn_blocking(move || {
        let mut client = postgres::Client::connect(&url, postgres::NoTls).unwrap();
        mmorpg_control_plane::postgres::migrate(&mut client).unwrap();
        mmorpg_characters::postgres::migrate(&mut client).unwrap();
        let mut store =
            CharacterStore::new(PostgresEntries::connect(&url, Duration::from_secs(5)).unwrap());
        for record in records {
            let lease = store
                .acquire(
                    &record.character_id,
                    LeaseHolder {
                        zone_id: record.location.zone_id,
                        zone_epoch: 0,
                    },
                )
                .unwrap()
                .lease;
            store.release(&lease, Some(&record)).unwrap();
        }
    })
    .await
    .unwrap();
}

async fn holder(url: &str, id: &CharacterId) -> Option<LeaseHolder> {
    let url = url.to_owned();
    let id = id.clone();
    tokio::task::spawn_blocking(move || {
        CharacterStore::new(PostgresEntries::connect(&url, Duration::from_secs(5)).unwrap())
            .committed(&id)
            .unwrap()
            .unwrap()
            .leased_by
    })
    .await
    .unwrap()
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

/// Connects to `route` once its host serves it, and claims `id`; a claim
/// waiting for the cutover is retried.
async fn join(
    endpoint: &Endpoint<wtransport::endpoint::endpoint_side::Client>,
    route: &str,
    id: &CharacterId,
) -> Connection {
    let connection = tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            if let Ok(connection) = endpoint.connect(route).await {
                return connection;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .unwrap();
    let mut stream = connection.accept_uni().await.unwrap();
    let mut welcome = [0; WELCOME_BYTES];
    stream.read_exact(&mut welcome).await.unwrap();
    game_server::decode_welcome(&welcome).unwrap();
    assert!(control(&connection, &[1, 0]).await.accepted);
    let mut claim = vec![1, 1, u8::try_from(CLASS.appearance()).unwrap()];
    claim.extend_from_slice(id.as_str().as_bytes());
    for _ in 0..50 {
        let reply = control(&connection, &claim).await;
        if reply.accepted {
            return connection;
        }
        eprintln!(
            "claim at {route}: {}",
            String::from_utf8_lossy(&reply.payload)
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("claim of {} at {route} never succeeded", id.as_str());
}

enum Frame {
    Projection(Box<mmorpg_core::ZoneSnapshot>),
    Redirect(RedirectNotice),
}

async fn next_frame(connection: &Connection, reassembler: &mut SnapshotReassembler) -> Frame {
    loop {
        let datagram = connection.receive_datagram().await.unwrap();
        if let Some(frame) = reassembler.accept(&datagram).unwrap() {
            return if is_redirect_notice(&frame.payload) {
                Frame::Redirect(decode_redirect_notice(&frame.payload).unwrap())
            } else {
                Frame::Projection(Box::new(decode_snapshot(&frame.payload).unwrap()))
            };
        }
    }
}

/// Waits for the session's redirect notice, sending `command` meanwhile.
async fn redirected(connection: &Connection, command: Option<ZoneCommand>) -> RedirectNotice {
    let mut reassembler = SnapshotReassembler::new();
    tokio::time::timeout(Duration::from_secs(20), async {
        let mut sequence = 0_u32;
        loop {
            if let Some(command) = command {
                sequence += 1;
                let payload = mmorpg_protocol::encode_command(command);
                let _ = connection
                    .send_datagram(game_server::encode_command(sequence, &payload).unwrap());
            }
            if let Frame::Redirect(notice) = next_frame(connection, &mut reassembler).await {
                return notice;
            }
        }
    })
    .await
    .expect("the session was redirected")
}

/// Waits for the session's own unit in `zone` and returns its position.
async fn arrived(connection: &Connection, zone: ZoneId) -> [i32; 3] {
    let mut reassembler = SnapshotReassembler::new();
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            if let Frame::Projection(snapshot) = next_frame(connection, &mut reassembler).await
                && snapshot.zone_id == zone
                && snapshot.viewer.class == Some(CLASS)
            {
                return snapshot.entities[0].position;
            }
        }
    })
    .await
    .expect("the character went live at the destination")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_hosts_hand_characters_over_through_the_seam_and_by_waystone() {
    let Ok(url) = std::env::var("MMORPG_CHARACTERS_POSTGRES_URL") else {
        return;
    };
    let run = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = Directory(
        std::env::temp_dir().join(format!("mmorpg-handoff-hosts-{}-{run}", std::process::id())),
    );
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

    let (west, east) = (seam_test::WEST_ZONE, seam_test::EAST_ZONE);
    let walker = CharacterId::new(format!("seam-walker-{run}")).unwrap();
    let traveller = CharacterId::new(format!("seam-traveller-{run}")).unwrap();
    let both = WaystoneSet::EMPTY.with(SEAM_TEST_WEST).with(SEAM_TEST_EAST);
    let east_stone = waystone(SEAM_TEST_EAST).unwrap().arrival;
    seed(
        &url,
        vec![
            // Standing in West's crossing strip: it departs on its first tick.
            record(&walker, west, [5_200, 0], WaystoneSet::EMPTY),
            record(&traveller, east, east_stone, both),
        ],
    )
    .await;

    let (west_port, east_port) = (udp_port(), udp_port());
    let (west_link, east_link) = (tcp_port(), tcp_port());
    let host = |name: &str,
                zone: ZoneId,
                port: u16,
                link: u16,
                peer: ZoneId,
                peer_port: u16,
                peer_link: u16| {
        let log = directory.0.join(format!("{name}.log"));
        Process(
            Command::new(env!("CARGO_BIN_EXE_mmorpg-zone-host"))
                .env_remove("MMORPG_CHARACTER_DIR")
                .env_remove("MMORPG_RECOVERY_DIR")
                .env_remove("MMORPG_PERSISTENCE_MODE")
                .env("MMORPG_HANDOFF_DATABASE_URL", &url)
                .env("MMORPG_HOST_ID", format!("seam-test-{name}"))
                .env("MMORPG_ZONE_IDS", zone.get().to_string())
                .env("MMORPG_PORT", port.to_string())
                .env("MMORPG_STATUS_PORT", "0")
                .env("MMORPG_DRAIN_GRACE_MS", "0")
                .env("MMORPG_CERT_PEM", &cert)
                .env("MMORPG_KEY_PEM", &key)
                .env("MMORPG_TRANSFER_LINK", format!("127.0.0.1:{link}"))
                .env(
                    "MMORPG_ZONE_ROUTES",
                    format!(
                        "{}=127.0.0.1:{peer_link}@{}",
                        peer.get(),
                        session_route(peer_port, peer)
                    ),
                )
                .stdout(std::fs::File::create(&log).unwrap())
                .stderr(std::fs::File::create(log.with_extension("stderr")).unwrap())
                .spawn()
                .unwrap(),
        )
    };
    let _west_host = host(
        "west", west, west_port, west_link, east, east_port, east_link,
    );
    let _east_host = host(
        "east", east, east_port, east_link, west, west_port, west_link,
    );
    let endpoint = Endpoint::client(
        ClientConfig::builder()
            .with_bind_default()
            .with_server_certificate_hashes([identity.certificate_chain().as_slice()[0].hash()])
            .build(),
    )
    .unwrap();

    // Walking through the seam: West → East.
    let source = join(&endpoint, &session_route(west_port, west), &walker).await;
    let notice = redirected(&source, None).await;
    assert_eq!((notice.zone_id, notice.destination), (west, east));
    assert_eq!(notice.route, session_route(east_port, east));
    let destination = join(&endpoint, &notice.route, &walker).await;
    let position = arrived(&destination, east).await;
    assert!(
        (position[0] - (5_200 - seam_test::SHIFT)).abs() < 300,
        "the walker continues where it crossed: {position:?}"
    );
    assert_eq!(
        holder(&url, &walker).await.map(|held| held.zone_id),
        Some(east)
    );
    drop(source);

    // Teleporting by waystone: East → West.
    let source = join(&endpoint, &session_route(east_port, east), &traveller).await;
    let notice = redirected(
        &source,
        Some(ZoneCommand::UseWaystone {
            waystone: SEAM_TEST_WEST,
        }),
    )
    .await;
    assert_eq!((notice.zone_id, notice.destination), (east, west));
    let destination = join(&endpoint, &notice.route, &traveller).await;
    let position = arrived(&destination, west).await;
    let arrival = waystone(SEAM_TEST_WEST).unwrap().arrival;
    assert!(
        (position[0] - arrival[0]).abs() < 100 && (position[2] - arrival[1]).abs() < 100,
        "the traveller arrives at West's stone: {position:?}"
    );
    assert_eq!(
        holder(&url, &traveller).await.map(|held| held.zone_id),
        Some(west)
    );
}
