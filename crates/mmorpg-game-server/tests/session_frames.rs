//! The shared Rust/browser session-frame fixture: game-server welcome,
//! command, snapshot and snapshot-fragment frames rendered by the pinned
//! game-server encoders. The browser's WebTransport source decodes and encodes
//! the same bytes (`web/tests/session-frames.test.ts`), so a game-server pin
//! bump that changes a frame fails here and there together.
use game_server::{
    BROWSER_PROTOCOL_CONTRACT, BrowserRoutePrefix, MatchId, ReconnectToken,
    SNAPSHOT_REASSEMBLY_MAX_BUFFERED_BYTES, SNAPSHOT_REASSEMBLY_MAX_IDLE_DATAGRAMS,
    SNAPSHOT_REASSEMBLY_MAX_PENDING, SnapshotFrame, SnapshotReassembler, Welcome, decode_command,
    decode_snapshot, decode_snapshot_fragment, decode_welcome, encode_command, encode_snapshot,
    encode_snapshot_fragments, encode_welcome, snapshot_hash,
};
use mmorpg_core::ZoneId;
use mmorpg_game_server::zone_match_id;

const FIXTURE: &str = include_str!("../../../fixtures/protocol/session-frames-v3.hex");
const TOKEN: [u8; 16] = [
    0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
];
/// A datagram budget small enough to split the fragmented fixture snapshot into three.
const FRAGMENT_BUDGET: usize = game_server::SNAPSHOT_FRAGMENT_HEADER_BYTES + 24;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|offset| u8::from_str_radix(&text[offset..offset + 2], 16).unwrap())
        .collect()
}

/// Opaque snapshot payload bytes; game-server never interprets them.
fn payload(length: usize, seed: u8) -> Vec<u8> {
    (0..length)
        .map(|index| (index as u8).wrapping_mul(31).wrapping_add(seed))
        .collect()
}

fn snapshot(tick: u64, payload: Vec<u8>) -> SnapshotFrame {
    SnapshotFrame {
        tick,
        state_hash: snapshot_hash(tick, &payload),
        payload,
    }
}

fn welcomes() -> [Welcome; 2] {
    [
        Welcome {
            player_id: 7,
            tick_hz: 30,
            max_players: 16,
            current_tick: 99,
            connection_epoch: 3,
            reconnect_token: TOKEN,
            reconnect_grace_ticks: 600,
        },
        Welcome {
            player_id: u32::MAX,
            tick_hz: u16::MAX,
            max_players: u16::MAX,
            current_tick: u64::MAX,
            connection_epoch: u32::MAX,
            reconnect_token: [0xff; 16],
            reconnect_grace_ticks: u64::MAX,
        },
    ]
}

fn commands() -> [(u32, Vec<u8>); 3] {
    [
        (1, vec![2, 1, 0, 0, 0, 0]),
        (0x0102_0304, vec![2, 2]),
        (u32::MAX, Vec::new()),
    ]
}

fn snapshots() -> [SnapshotFrame; 3] {
    [
        snapshot(0, Vec::new()),
        snapshot(42, payload(12, 7)),
        snapshot(0x0102_0304_0506_0708, payload(40, 3)),
    ]
}

fn render() -> String {
    let contract = BROWSER_PROTOCOL_CONTRACT;
    let mut fixture = String::from(
        "# game-server session frames (protocol version 3) from the pinned encoders.\n\
         # Rendered and verified by crates/mmorpg-game-server/tests/session_frames.rs;\n\
         # decoded and encoded by web/tests/session-frames.test.ts. Fields are key=value.\n",
    );
    fixture.push_str(&format!(
        "contract route_version={} protocol_version={} reconnect_token_bytes={} \
         max_command_payload_bytes={} max_snapshot_payload_bytes={} max_snapshot_fragments={} \
         reassembly_max_pending={SNAPSHOT_REASSEMBLY_MAX_PENDING} \
         reassembly_max_buffered_bytes={SNAPSHOT_REASSEMBLY_MAX_BUFFERED_BYTES} \
         reassembly_max_idle_datagrams={SNAPSHOT_REASSEMBLY_MAX_IDLE_DATAGRAMS}\n",
        contract.route_version,
        contract.game_protocol_version,
        contract.reconnect_token_bytes,
        contract.max_command_payload_bytes,
        contract.max_snapshot_payload_bytes,
        contract.max_snapshot_fragments,
    ));
    let prefix = BrowserRoutePrefix::new("/game").unwrap();
    let match_id = zone_match_id(ZoneId::new(1)).unwrap();
    fixture.push_str(&format!(
        "route {} prefix={} match_id={} token={}\n",
        prefix.reconnect_path(&match_id, ReconnectToken(TOKEN)),
        prefix.as_str(),
        match_id.as_str(),
        hex(&TOKEN),
    ));
    for welcome in welcomes() {
        fixture.push_str(&format!(
            "welcome {} player_id={} tick_hz={} max_players={} current_tick={} \
             connection_epoch={} reconnect_token={} reconnect_grace_ticks={}\n",
            hex(&encode_welcome(welcome)),
            welcome.player_id,
            welcome.tick_hz,
            welcome.max_players,
            welcome.current_tick,
            welcome.connection_epoch,
            hex(&welcome.reconnect_token),
            welcome.reconnect_grace_ticks,
        ));
    }
    for (sequence, payload) in commands() {
        fixture.push_str(&format!(
            "command {} sequence={sequence} payload={}\n",
            hex(&encode_command(sequence, &payload).unwrap()),
            hex(&payload),
        ));
    }
    for frame in snapshots() {
        let encoded = encode_snapshot(&frame).unwrap();
        fixture.push_str(&format!(
            "snapshot {} tick={} state_hash={:016x} payload={}\n",
            hex(&encoded),
            frame.tick,
            frame.state_hash,
            hex(&frame.payload),
        ));
    }
    let fragmented = snapshots()[2].clone();
    let fragments =
        encode_snapshot_fragments(&encode_snapshot(&fragmented).unwrap(), FRAGMENT_BUDGET).unwrap();
    for datagram in &fragments {
        let fragment = decode_snapshot_fragment(datagram).unwrap();
        fixture.push_str(&format!(
            "fragment {} tick={} index={} count={}\n",
            hex(datagram),
            fragment.tick,
            fragment.index,
            fragment.count,
        ));
    }
    fixture
}

/// Lines of one kind as `(hex, fields)`.
fn lines<'a>(kind: &'a str) -> impl Iterator<Item = (Vec<u8>, &'static str)> + 'a {
    FIXTURE.lines().filter_map(move |line| {
        let rest = line.strip_prefix(kind)?.strip_prefix(' ')?;
        let (encoded, fields) = rest.split_once(' ').unwrap_or((rest, ""));
        Some((unhex(encoded), fields))
    })
}

#[test]
fn session_frames_match_the_shared_golden_fixture() {
    assert_eq!(FIXTURE, render());
}

#[test]
fn fixture_frames_decode_with_the_pinned_decoders() {
    let welcomes = welcomes();
    let decoded: Vec<_> = lines("welcome")
        .map(|(bytes, _)| decode_welcome(&bytes).unwrap())
        .collect();
    assert_eq!(decoded, welcomes);

    let commands = commands();
    let decoded: Vec<_> = lines("command")
        .map(|(bytes, _)| {
            let frame = decode_command(&bytes).unwrap();
            (frame.sequence, frame.payload)
        })
        .collect();
    assert_eq!(decoded, commands);

    let snapshots = snapshots();
    let decoded: Vec<_> = lines("snapshot")
        .map(|(bytes, _)| decode_snapshot(&bytes).unwrap())
        .collect();
    assert_eq!(decoded, snapshots);

    let mut reassembler = SnapshotReassembler::new();
    let mut delivered = None;
    for (datagram, _) in lines("fragment") {
        assert!(datagram.len() <= FRAGMENT_BUDGET);
        delivered = reassembler.accept(&datagram).unwrap().or(delivered);
    }
    assert_eq!(delivered.as_ref(), snapshots.last());
    assert_eq!(reassembler.stats().reassembled_snapshots, 1);
    assert_eq!(lines("fragment").count(), 3);
}

#[test]
fn fixture_route_is_the_hosted_reconnect_route() {
    let (_, fields) = FIXTURE
        .lines()
        .find_map(|line| line.strip_prefix("route ")?.split_once(' '))
        .unwrap();
    let path = FIXTURE
        .lines()
        .find_map(|line| line.strip_prefix("route ")?.split(' ').next())
        .unwrap();
    assert!(fields.contains("match_id=zone-1"));
    let parsed = BrowserRoutePrefix::new("/game")
        .unwrap()
        .parse(path)
        .unwrap()
        .unwrap();
    assert_eq!(parsed.match_id, MatchId::new("zone-1").unwrap());
    assert_eq!(
        parsed.admission,
        game_server::BrowserAdmission::Reconnect(ReconnectToken(TOKEN))
    );
}
