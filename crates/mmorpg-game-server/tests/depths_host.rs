#![cfg(feature = "postgres")]
//! A real handoff host (#404 part 2): a qualified character walks through the Redbrand Hollow
//! threshold on zone 1, is redirected into its private depths slot and walks back out through
//! the depths' exit, all through the fenced transfer and a directory shared over PostgreSQL. An
//! unqualified character standing beside it is refused with a notice and stays put.
//!
//! Needs `MMORPG_CHARACTERS_POSTGRES_URL` (a database the test may migrate); without it the
//! test does nothing.
use std::{
    path::PathBuf,
    process::{Child, Command},
    time::Duration,
};

use game_server::{SnapshotReassembler, WELCOME_BYTES};
use mmorpg_characters::{CharacterStore, LeaseHolder, PostgresEntries};
use mmorpg_core::greyhaven_vale::{cave::GREYFANG, quests};
use mmorpg_core::redbrand_depths::{self, ARRIVAL};
use mmorpg_core::{
    CharacterId, CharacterLocation, CharacterRecord, ClassChoice, ErrorCode,
    PLAYER_HALF_EXTENTS_UNITS, PlayerClass, Sex, WaystoneSet, ZoneCommand, ZoneEvent, ZoneId,
    ZoneSimulation,
};
use mmorpg_protocol::{
    RedirectNotice, decode_redirect_notice, decode_snapshot, is_redirect_notice,
};
use wtransport::{ClientConfig, Connection, Endpoint, Identity};

struct Directory(PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("depths host logs: {}", self.0.display());
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
const GREYHAVEN: ZoneId = ZoneId::new(1);
const SLOT: ZoneId = ZoneId::new(redbrand_depths::ZONE_IDS.start);
/// Inside the threshold strip, three metres from the door's Brute.
const STRIP: [i32; 2] = [7_890, -13_620];

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

fn threshold_record(id: &CharacterId, qualified: bool, offset: i32) -> CharacterRecord {
    let mut simulation =
        ZoneSimulation::with_content(GREYHAVEN, mmorpg_core::content_for_zone(GREYHAVEN)).unwrap();
    simulation.create_character(1, CLASS).unwrap();
    let mut record = simulation.capture_character(1, id.clone()).unwrap();
    record.location = CharacterLocation {
        zone_id: GREYHAVEN,
        position: [
            STRIP[0],
            GREYFANG.floor_y + PLAYER_HALF_EXTENTS_UNITS[1],
            STRIP[1] - offset,
        ],
        facing: 16_384,
    };
    record.waystones = WaystoneSet::EMPTY;
    if qualified {
        record.level = 6;
        record.experience = 0;
        record.health = mmorpg_core::unit::player_max_health(6);
        record.quests.completed = (1..=quests::GARRICK_REDBRAND_QUEST.get())
            .filter_map(|quest| mmorpg_core::QuestId::new(quest).bit())
            .fold(0, |mask, bit| mask | bit);
    }
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

/// Connects to `route` and claims `id`; a claim waiting for the cutover is retried.
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

/// Waits for the session's redirect notice, moving `facing` meanwhile if `moving`.
async fn redirected(connection: &Connection, moving: Option<u16>) -> RedirectNotice {
    let mut reassembler = SnapshotReassembler::new();
    tokio::time::timeout(Duration::from_secs(20), async {
        let mut sequence = 0_u32;
        loop {
            if let Some(facing) = moving {
                sequence += 1;
                let payload = mmorpg_protocol::encode_command(ZoneCommand::Move {
                    forward: 1,
                    strafe: 0,
                    facing,
                });
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

/// Waits for `code` among the session's error events.
async fn refused_with(connection: &Connection, code: ErrorCode) {
    let mut reassembler = SnapshotReassembler::new();
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            if let Frame::Projection(snapshot) = next_frame(connection, &mut reassembler).await
                && snapshot.events.iter().any(
                    |event| matches!(event, ZoneEvent::Error { code: found, .. } if *found == code),
                )
            {
                return;
            }
        }
    })
    .await
    .expect("the refusal notice arrived");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_qualified_character_enters_the_depths_and_an_unqualified_one_is_refused() {
    let Ok(url) = std::env::var("MMORPG_CHARACTERS_POSTGRES_URL") else {
        return;
    };
    let run = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = Directory(
        std::env::temp_dir().join(format!("mmorpg-depths-host-{}-{run}", std::process::id())),
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

    let hero = CharacterId::new(format!("depths-hero-{run}")).unwrap();
    let rook = CharacterId::new(format!("depths-rook-{run}")).unwrap();
    seed(
        &url,
        vec![
            threshold_record(&hero, true, 0),
            threshold_record(&rook, false, 100),
        ],
    )
    .await;

    let (port, link) = (udp_port(), tcp_port());
    let log = directory.0.join("host.log");
    let routes = format!(
        "{}=127.0.0.1:{link}@{},{}=127.0.0.1:{link}@{}",
        GREYHAVEN.get(),
        session_route(port, GREYHAVEN),
        SLOT.get(),
        session_route(port, SLOT),
    );
    let _host = Process(
        Command::new(env!("CARGO_BIN_EXE_mmorpg-zone-host"))
            .env_remove("MMORPG_CHARACTER_DIR")
            .env_remove("MMORPG_RECOVERY_DIR")
            .env_remove("MMORPG_PERSISTENCE_MODE")
            .env("MMORPG_HANDOFF_DATABASE_URL", &url)
            .env("MMORPG_HOST_ID", format!("depths-test-{run}"))
            .env(
                "MMORPG_ZONE_IDS",
                format!("{},{}", GREYHAVEN.get(), SLOT.get()),
            )
            .env("MMORPG_PORT", port.to_string())
            .env("MMORPG_STATUS_PORT", "0")
            .env("MMORPG_DRAIN_GRACE_MS", "0")
            .env("MMORPG_CERT_PEM", &cert)
            .env("MMORPG_KEY_PEM", &key)
            .env("MMORPG_TRANSFER_LINK", format!("127.0.0.1:{link}"))
            .env("MMORPG_ZONE_ROUTES", routes)
            .stdout(std::fs::File::create(&log).unwrap())
            .stderr(std::fs::File::create(log.with_extension("stderr")).unwrap())
            .spawn()
            .unwrap(),
    );
    let endpoint = Endpoint::client(
        ClientConfig::builder()
            .with_bind_default()
            .with_server_certificate_hashes([identity.certificate_chain().as_slice()[0].hash()])
            .build(),
    )
    .unwrap();

    // The unqualified character stands in the strip and is refused.
    let rook_session = join(&endpoint, &session_route(port, GREYHAVEN), &rook).await;
    refused_with(&rook_session, ErrorCode::DepthsLocked).await;

    // The qualified character enters: redirected to its slot, it arrives in the entrance hall.
    let source = join(&endpoint, &session_route(port, GREYHAVEN), &hero).await;
    let notice = redirected(&source, None).await;
    assert_eq!((notice.zone_id, notice.destination), (GREYHAVEN, SLOT));
    assert_eq!(notice.route, session_route(port, SLOT));
    let inside = join(&endpoint, &notice.route, &hero).await;
    let position = arrived(&inside, SLOT).await;
    assert!(
        (position[0] - ARRIVAL[0]).abs() < 300 && position[2].abs() < 300,
        "arrived in the entrance hall: {position:?}"
    );
    drop(source);

    // Walking west through the exit corridor returns the character to the public cave.
    let back = redirected(&inside, Some(49_152)).await;
    assert_eq!((back.zone_id, back.destination), (SLOT, GREYHAVEN));
    assert_eq!(back.route, session_route(port, GREYHAVEN));
    let outside = join(&endpoint, &back.route, &hero).await;
    let returned = arrived(&outside, GREYHAVEN).await;
    assert!(
        returned[0] > 6_000 && returned[0] < 6_800 && returned[2] < -14_500,
        "back at the public cave's mouth: {returned:?}"
    );
}
