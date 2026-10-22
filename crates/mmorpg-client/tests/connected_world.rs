use game_server::{
    BrowserRoutePrefix, MatchHost, MatchHostWebTransportConfig, MatchRuntime,
    serve_match_host_with_shutdown,
};
use mmorpg_client::{
    ClientError,
    network::ClientSession,
    session::{NetworkUpdate, PlayerInput},
};
use mmorpg_core::{
    EntityFlags, EntityKind, EntitySnapshot, MAX_VISIBLE_ENTITIES, PLAYER_HALF_EXTENTS_UNITS,
    SNAPSHOT_SCHEMA_VERSION, TICK_HZ, TargetDetail, ViewerState, ZoneId, ZoneSnapshot,
    greyhaven_vale, greyhaven_vale_definition,
};
use mmorpg_game_server::{ZoneGameServerAdapter, zone_match_id};
use mmorpg_protocol::{
    DATAGRAM_SAFETY_MARGIN_BYTES, ENTITY_RECORD_BYTES, MAX_PLAYER_PROJECTION_BYTES,
    MAX_WIRE_ENTITIES, MEASURED_MIN_DATAGRAM_BYTES, PLAYER_SNAPSHOT_FIXED_BYTES,
    SESSION_SNAPSHOT_HEADER_BYTES,
};
use std::{
    net::UdpSocket,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::watch;

/// Forward-movement headings: yaw 0 faces +Z, a quarter turn faces +X and
/// half a turn faces -Z.
const NORTH: u16 = 0;
const EAST: u16 = 16_384;
const SOUTH: u16 = 32_768;

/// A real WebTransport zone host on an OS-assigned port with disposable TLS.
struct LocalHost {
    temporary: tempfile::TempDir,
    certificate: PathBuf,
    url: String,
    shutdown: tokio::sync::mpsc::Sender<()>,
    task: tokio::task::JoinHandle<Result<(), Box<dyn std::error::Error + Send + Sync>>>,
}

enum HostMode {
    Anonymous(MatchHost<ZoneGameServerAdapter>),
    Persistent(PathBuf),
}

impl LocalHost {
    fn start(host: MatchHost<ZoneGameServerAdapter>, zone_id: ZoneId) -> Self {
        Self::start_mode(HostMode::Anonymous(host), zone_id)
    }

    fn start_mode(mode: HostMode, zone_id: ZoneId) -> Self {
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
        let config = MatchHostWebTransportConfig {
            port,
            certificate_pem: certificate.clone(),
            private_key_pem: key,
            route_prefix: BrowserRoutePrefix::new("/game").unwrap(),
            allowed_origins: None,
            drain_grace: Duration::from_millis(10),
        };
        let task = tokio::spawn(async move {
            match mode {
                HostMode::Anonymous(host) => serve_match_host_with_shutdown(host, config, receiver)
                    .await
                    .map_err(|error| Box::<dyn std::error::Error + Send + Sync>::from(error)),
                HostMode::Persistent(directory) => {
                    mmorpg_game_server::serve_development_character_host(
                        mmorpg_game_server::DevelopmentCharacterHostConfig {
                            directory,
                            recovery_directory: None,
                            zone_ids: vec![zone_id],
                            reconnect_grace_ticks: 20,
                        },
                        config,
                        game_server::MatchHostStatusConfig { port: 0 },
                        receiver,
                    )
                    .await
                    .inspect_err(|error| eprintln!("development host failed: {error}"))
                }
            }
        });
        Self {
            temporary,
            certificate,
            url: format!(
                "https://localhost:{port}/game/matches/zone-{}",
                zone_id.get()
            ),
            shutdown,
            task,
        }
    }

    fn scratch(&self) -> &Path {
        self.temporary.path()
    }

    async fn stop(self) {
        self.shutdown.send(()).await.unwrap();
        tokio::time::timeout(Duration::from_secs(3), self.task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
}

#[tokio::test]
async fn corpse_loot_crosses_real_transport_and_resume_without_double_credit() {
    use mmorpg_core::{
        CreatureBehaviour, CreatureId, CreatureSpawn, EntityRef, ItemId, LootOutcome, LootTable,
        ZoneAreas, ZoneContent, ZoneDefinition,
    };
    let zone_id = ZoneId::new(1);
    let definition = ZoneDefinition::default();
    let feet = definition.spawn_grid().feet(0).unwrap();
    let mut template = greyhaven_vale::content().creature_templates()[0].clone();
    template.behaviour = CreatureBehaviour::Neutral;
    template.min_level = 1;
    template.max_level = 1;
    template.health = 1;
    template.health_per_level = 0;
    let spawn = CreatureSpawn {
        id: CreatureId::new(1),
        template: template.id,
        position: [feet[0] - 180, feet[2]],
        facing: 0,
        wander_radius: 0,
        levels: None,
    };
    let table = LootTable::new(
        [2, 2],
        &[LootOutcome::Item {
            weight: 1,
            item: ItemId::new(1),
            quantity: [2, 2],
        }],
    )
    .unwrap();
    let template_id = spawn.template;
    let content = Arc::new(
        ZoneContent::new(
            definition,
            ZoneAreas::default(),
            vec![template],
            vec![spawn],
            vec![],
            [feet[0], feet[2]],
        )
        .unwrap()
        .with_loot_tables(1, vec![(template_id, table)])
        .unwrap(),
    );
    let revision = content.revision();
    let mut host = MatchHost::new(1).unwrap();
    host.insert(
        zone_match_id(zone_id).unwrap(),
        MatchRuntime::new(
            ZoneGameServerAdapter::with_content(zone_id, content).unwrap(),
            120,
        ),
    )
    .map_err(|failure| failure.into_parts().0)
    .unwrap();
    let local = LocalHost::start(host, zone_id);
    let result = tokio::time::timeout(Duration::from_secs(15), async {
        let mut session =
            ClientSession::connect(&local.url, Some(&local.certificate), zone_id, revision).await?;
        session.send_select_target(Some(EntityRef::Creature(CreatureId::new(1))))?;
        session.send_attack(true)?;
        let mut sheet = None;
        for _ in 0..90 {
            let snapshot = session.receive_snapshot().await?;
            if let Some(view) = snapshot.loot {
                sheet = Some(view);
                break;
            }
        }
        let sheet = sheet.ok_or("the authority did not publish corpse loot")?;
        assert_eq!(sheet.rewards.money, 2);
        assert_eq!(sheet.rewards.item.unwrap().quantity(), 2);
        let mut other =
            ClientSession::connect(&local.url, Some(&local.certificate), zone_id, revision).await?;
        other.send_loot(sheet.claim)?;
        let mut rejected_owner = false;
        for _ in 0..30 {
            let snapshot = other.receive_snapshot().await?;
            assert_eq!(snapshot.viewer.copper, 0);
            assert!(snapshot.loot.is_none());
            rejected_owner |= snapshot.events.iter().any(|event| {
                matches!(
                    event,
                    mmorpg_core::ZoneEvent::Error {
                        code: mmorpg_core::ErrorCode::NotLootOwner,
                        ..
                    }
                )
            });
            if rejected_owner {
                break;
            }
        }
        assert!(rejected_owner);
        session.send_loot(sheet.claim)?;
        let mut credited = false;
        for _ in 0..30 {
            let snapshot = session.receive_snapshot().await?;
            if snapshot.viewer.copper == 2 && snapshot.inventory_revision == 2 {
                assert!(snapshot.loot.is_none());
                credited = true;
                break;
            }
        }
        assert!(credited);
        let identity = session.player_id();
        session.reconnect().await?;
        assert_eq!(session.player_id(), identity);
        session.send_loot(sheet.claim)?;
        let mut saw_refusal = false;
        let mut saw_bag = false;
        for _ in 0..30 {
            let snapshot = session.receive_snapshot().await?;
            assert_eq!(snapshot.viewer.copper, 2);
            assert!(snapshot.loot.is_none());
            if let Some(bag) = snapshot.inventory {
                assert_eq!(bag.slots()[0].unwrap().quantity(), 5);
                saw_bag = true;
            }
            saw_refusal |= snapshot.events.iter().any(|event| {
                matches!(
                    event,
                    mmorpg_core::ZoneEvent::Error {
                        code: mmorpg_core::ErrorCode::EmptyLoot,
                        ..
                    }
                )
            });
            if saw_bag && saw_refusal {
                break;
            }
        }
        assert!(saw_bag && saw_refusal);
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    })
    .await;
    local.stop().await;
    result.unwrap().unwrap();
}

#[tokio::test]
async fn bag_intents_cross_real_transport_and_resume_without_losing_items() {
    let zone_id = ZoneId::new(1);
    let local = LocalHost::start(
        mmorpg_game_server::build_zone_host([zone_id], 120).unwrap(),
        zone_id,
    );
    let result = tokio::time::timeout(Duration::from_secs(10), async {
        let mut session = ClientSession::connect(
            &local.url,
            Some(&local.certificate),
            zone_id,
            greyhaven_vale::REVISION,
        )
        .await?;
        session.send_move_item(0, 15, 2)?;
        let mut changed = None;
        for _ in 0..30 {
            let snapshot = session.receive_snapshot().await?;
            if snapshot.inventory_revision == 2 && snapshot.inventory.is_some() {
                changed = snapshot.inventory;
                break;
            }
        }
        let changed = changed.ok_or("the authority did not publish the moved bag")?;
        assert_eq!(changed.slots()[0].unwrap().quantity(), 1);
        assert_eq!(changed.slots()[15].unwrap().quantity(), 2);
        let player_id = session.player_id();
        session.reconnect().await?;
        assert_eq!(session.player_id(), player_id);
        let mut recovered = false;
        for _ in 0..30 {
            let snapshot = session.receive_snapshot().await?;
            if let Some(inventory) = snapshot.inventory {
                assert_eq!(inventory, changed);
                assert_eq!(snapshot.inventory_revision, 2);
                recovered = true;
                break;
            }
        }
        assert!(
            recovered,
            "resume recovers the durable bag from a periodic sheet"
        );
        session.send_move_item(15, 0, 2)?;
        let mut merged = false;
        for _ in 0..30 {
            let snapshot = session.receive_snapshot().await?;
            if snapshot.inventory_revision == 3
                && let Some(inventory) = snapshot.inventory
            {
                assert_eq!(inventory.slots()[0].unwrap().quantity(), 3);
                assert!(inventory.slots()[15].is_none());
                merged = true;
                break;
            }
        }
        assert!(merged);
        Ok::<(), ClientError>(())
    })
    .await;
    local.stop().await;
    result.unwrap().unwrap();
}

#[tokio::test]
async fn clients_share_authority_resume_identity_and_reject_wrong_content() {
    let zone_id = ZoneId::new(1);
    let host = mmorpg_game_server::build_zone_host([zone_id], 120).unwrap();
    let local = LocalHost::start(host, zone_id);
    let (url, certificate) = (local.url.clone(), local.certificate.clone());
    let result = tokio::time::timeout(Duration::from_secs(20), async {
        let revision = greyhaven_vale_definition().revision();
        let mut first = ClientSession::connect(&url, Some(&certificate), zone_id, revision).await?;
        let second = ClientSession::connect(&url, Some(&certificate), zone_id, revision).await?;
        // Path MTU discovery only grows this above the measured floor.
        let negotiated = second.max_datagram_size().ok_or("datagrams are required")?;
        eprintln!(
            "{{\"event\":\"negotiated_max_datagram_size\",\"bytes\":{negotiated},\"budget_floor\":{MEASURED_MIN_DATAGRAM_BYTES}}}"
        );
        assert!(negotiated >= MEASURED_MIN_DATAGRAM_BYTES);
        assert_ne!(first.player_id(), second.player_id());
        let initial = first.receive_snapshot().await?;
        assert_eq!(initial.viewer_id, first.player_id());
        let start = initial
            .entities
            .iter()
            .find(|player| player.id == first.player_id())
            .unwrap()
            .position;
        first.send_move(1, 0, SOUTH)?;
        let mut moved = false;
        for _ in 0..30 {
            let snapshot = second.receive_snapshot().await?;
            assert_eq!(snapshot.viewer_id, second.player_id());
            if snapshot.entities.iter().any(|player| {
                player.id == first.player_id()
                    && player.position[2] < start[2]
                    && player.facing == SOUTH
            }) {
                moved = true;
                break;
            }
        }
        assert!(
            moved,
            "other clients must see the server-resolved movement and facing"
        );
        first.send_move(0, 0, SOUTH)?;
        let mut stopped = false;
        for _ in 0..30 {
            let snapshot = first.receive_snapshot().await?;
            if snapshot.acknowledged_sequence >= 2 {
                let player = snapshot
                    .entities
                    .iter()
                    .find(|player| player.id == first.player_id())
                    .unwrap();
                if player.velocity[2] == 0 {
                    stopped = true;
                    break;
                }
            }
        }
        assert!(
            stopped,
            "key release must become authoritative and acknowledged"
        );
        // Commands may have reached the server even if their ACK was lost.
        // Preserve the highest sent sequence across connection replacement.
        for _ in 0..32 {
            first.send_move(0, 0, SOUTH)?;
        }
        let player_id = first.player_id();
        let previous_epoch = first.connection_epoch();
        let resumed = first.reconnect().await?;
        assert_eq!(first.player_id(), player_id);
        assert_eq!(first.connection_epoch(), previous_epoch + 1);
        assert!(resumed.acknowledged_sequence >= 35);
        let resumed_player = resumed
            .entities
            .iter()
            .find(|p| p.kind == EntityKind::Player && p.id == player_id)
            .unwrap();
        assert!(
            resumed_player.position[2] < start[2],
            "resume must retain movement state"
        );
        assert_eq!(resumed_player.velocity, [0; 3]);
        assert_eq!(
            resumed_player.facing, SOUTH,
            "the stopped intent keeps facing"
        );
        assert_eq!(
            resumed
                .entities
                .iter()
                .filter(|entity| entity.kind == EntityKind::Player)
                .count(),
            2,
            "resume must not admit another player"
        );
        // A second successful resume needs the newly rotated token.
        let again = first.reconnect().await?;
        assert_eq!(first.connection_epoch(), previous_epoch + 2);
        assert!(again.acknowledged_sequence > resumed.acknowledged_sequence);
        first.send_move(1, 0, NORTH)?;
        let mut resumed_movement = false;
        for _ in 0..30 {
            let snapshot = second.receive_snapshot().await?;
            if snapshot
                .entities
                .iter()
                .any(|p| p.id == player_id && p.velocity[2] > 0)
            {
                resumed_movement = true;
                break;
            }
        }
        assert!(
            resumed_movement,
            "resumed commands must reach the same authoritative player"
        );
        first.send_jump()?;
        let mut jumped = false;
        for _ in 0..30 {
            let snapshot = second.receive_snapshot().await?;
            if snapshot.entities.iter().any(|p| {
                p.id == player_id
                    && p.position[1] > PLAYER_HALF_EXTENTS_UNITS[1]
                    && p.velocity[2] > 0
            }) {
                jumped = true;
                break;
            }
        }
        assert!(
            jumped,
            "a grounded running jump must be visible to other clients"
        );
        let mut incompatible =
            ClientSession::connect(&url, Some(&certificate), zone_id, revision + 1).await?;
        let incompatible_error = incompatible.receive_snapshot().await.unwrap_err();
        assert!(!incompatible.can_reconnect(&incompatible_error).await);
        assert!(incompatible.reconnect().await.is_err());
        assert_eq!(
            incompatible.reconnect().await.unwrap_err().to_string(),
            "session resume is unavailable after an incomplete attempt"
        );
        let unrelated_identity = wtransport::Identity::self_signed(["localhost"]).unwrap();
        let wrong_certificate = local.scratch().join("wrong-cert.pem");
        std::fs::write(
            &wrong_certificate,
            unrelated_identity.certificate_chain().as_slice()[0].to_pem(),
        )
        .unwrap();
        assert!(
            ClientSession::connect(&url, Some(&wrong_certificate), zone_id, revision)
                .await
                .is_err()
        );
        let third = ClientSession::connect(&url, Some(&certificate), zone_id, revision).await?;
        verify_live_input(third).await?;
        verify_automatic_resume(first).await?;
        verify_shutdown_during_resume(second).await?;
        Ok::<(), ClientError>(())
    })
    .await;
    local.stop().await;
    result.unwrap().unwrap();
}

/// The byte budget starts from the smallest datagram the pinned stack
/// negotiates: QUIC's initial MTU before path MTU discovery. Disabling
/// discovery on both peers pins that floor, so an upgrade that shrinks it
/// fails here rather than closing sessions in the field.
/// Walks `session`'s player through `waypoints` (feet XZ, units) by steering toward each in
/// turn, and returns the last snapshot once it stands within a step of the final one.
async fn walk(
    session: &mut ClientSession,
    waypoints: &[[i32; 2]],
) -> Result<ZoneSnapshot, ClientError> {
    let player_id = session.player_id();
    let mut snapshot = session.receive_snapshot().await?;
    let deadline = snapshot.tick + 30 * u64::from(TICK_HZ);
    for (index, goal) in waypoints.iter().enumerate() {
        loop {
            let at = own(&snapshot, player_id).position;
            let [dx, dz] = [goal[0] - at[0], goal[1] - at[2]];
            if f64::from(dx).hypot(f64::from(dz)) <= 15. {
                if index + 1 == waypoints.len() {
                    session.send_move(0, 0, 0)?;
                }
                break;
            }
            if snapshot.tick > deadline {
                return Err(format!("never reached {goal:?}, stuck at {at:?}").into());
            }
            // Yaw 0 faces +Z and a quarter turn faces +X.
            let turns = f64::from(dx).atan2(f64::from(dz)) / std::f64::consts::TAU;
            session.send_move(1, 0, (turns.rem_euclid(1.) * 65_536.) as u16)?;
            snapshot = session.receive_snapshot().await?;
        }
    }
    // Let the stop settle before the caller acts on the position.
    for _ in 0..4 {
        snapshot = session.receive_snapshot().await?;
    }
    Ok(snapshot)
}

/// Snapshots until one from tick `from` on shows the inn door in state `open`, failing past
/// tick `by` (a client that has not read for a while first drains its older queued snapshots).
async fn door_reaches(
    session: &ClientSession,
    open: bool,
    [from, by]: [u64; 2],
) -> Result<ZoneSnapshot, ClientError> {
    loop {
        let snapshot = session.receive_snapshot().await?;
        if snapshot.doors.iter().any(|door| door.open == open) && snapshot.tick >= from {
            return Ok(snapshot);
        }
        if snapshot.tick > by {
            return Err(format!("the inn door never became open={open}").into());
        }
    }
}

/// The inn journey over a real zone host (#158): walk to the inn, open its door, enter, trade
/// with Bram across his bar, leave and close the door, while a second client sees every
/// door state change.
#[tokio::test]
async fn the_inn_journey_crosses_real_transport_and_both_players_see_the_door() {
    use mmorpg_core::{NpcId, ZoneCommand, greyhaven_vale::inn};
    let zone_id = ZoneId::new(1);
    let local = LocalHost::start(
        mmorpg_game_server::build_zone_host([zone_id], 120).unwrap(),
        zone_id,
    );
    let result = tokio::time::timeout(Duration::from_secs(60), async {
        let revision = greyhaven_vale::REVISION;
        let mut walker =
            ClientSession::connect(&local.url, Some(&local.certificate), zone_id, revision).await?;
        let watcher =
            ClientSession::connect(&local.url, Some(&local.certificate), zone_id, revision).await?;
        let start = walker.receive_snapshot().await?;
        assert!(
            start.doors.iter().all(|door| !door.open),
            "closed at zone start"
        );
        let spawn = own(&start, walker.player_id()).position;
        let door = |open| ZoneCommand::UseDoor {
            door: inn::DOOR,
            open,
        };
        // Straight past the watcher one slot east: the walker slides around it.
        let outside = [[1_495, spawn[2]], [1_495, 620]];
        walk(&mut walker, &outside).await?;
        walker.send_command(door(true))?;
        let opened = door_reaches(&walker, true, [0, u64::MAX]).await?.tick;
        door_reaches(&watcher, true, [opened, opened + 40]).await?;
        let bar = walk(&mut walker, &[[1_495, -10], [1_915, -10], [1_915, -178]]).await?;
        assert!(
            own(&bar, walker.player_id()).position[2] < 0,
            "inside the inn"
        );
        walker.send_command(ZoneCommand::SellItem {
            npc: NpcId::new(3),
            bag_slot: 0,
            quantity: 1,
        })?;
        let mut sold = false;
        for _ in 0..40 {
            if walker.receive_snapshot().await?.viewer.copper > 0 {
                sold = true;
                break;
            }
        }
        assert!(sold, "Bram buys across his bar");
        walk(&mut walker, &[[1_915, -10], [1_495, -10], [1_495, 620]]).await?;
        walker.send_command(door(false))?;
        let closed = door_reaches(&walker, false, [0, u64::MAX]).await?.tick;
        door_reaches(&watcher, false, [closed, closed + 40]).await?;
        Ok::<(), ClientError>(())
    })
    .await;
    local.stop().await;
    result.unwrap().unwrap();
}

#[tokio::test]
async fn datagram_budget_floor_matches_the_pinned_transport() {
    use wtransport::{ClientConfig, Endpoint, Identity, ServerConfig, config::QuicTransportConfig};
    fn without_discovery() -> Arc<QuicTransportConfig> {
        let mut transport = QuicTransportConfig::default();
        transport.mtu_discovery_config(None);
        Arc::new(transport)
    }
    assert_eq!(
        SESSION_SNAPSHOT_HEADER_BYTES,
        game_server::SNAPSHOT_HEADER_BYTES
    );
    assert_eq!(
        MAX_PLAYER_PROJECTION_BYTES,
        MEASURED_MIN_DATAGRAM_BYTES - SESSION_SNAPSHOT_HEADER_BYTES - DATAGRAM_SAFETY_MARGIN_BYTES
    );
    let identity = Identity::self_signed(["localhost"]).unwrap();
    let hash = identity.certificate_chain().as_slice()[0].hash();
    let mut server_config = ServerConfig::builder()
        .with_bind_default(0)
        .with_identity(identity)
        .build();
    server_config
        .quic_config_mut()
        .transport_config(without_discovery());
    let mut client_config = ClientConfig::builder()
        .with_bind_default()
        .with_server_certificate_hashes([hash])
        .build();
    client_config
        .quic_config_mut()
        .transport_config(without_discovery());
    let server = Endpoint::server(server_config).unwrap();
    let port = server.local_addr().unwrap().port();
    let floors = tokio::time::timeout(Duration::from_secs(10), async move {
        // The host keeps its endpoint and connection open until both peers measured.
        let accepted = tokio::spawn(async move {
            let request = server.accept().await.await.unwrap();
            let connection = request.accept().await.unwrap();
            let size = connection.max_datagram_size();
            (server, connection, size)
        });
        let client = Endpoint::client(client_config).unwrap();
        let connection = client
            .connect(format!("https://localhost:{port}/"))
            .await
            .unwrap();
        let (_server, _host_connection, host_size) = accepted.await.unwrap();
        (connection.max_datagram_size(), host_size)
    })
    .await
    .unwrap();
    eprintln!(
        "{{\"event\":\"datagram_floor\",\"client\":{:?},\"host\":{:?}}}",
        floors.0, floors.1
    );
    assert_eq!(
        floors,
        (
            Some(MEASURED_MIN_DATAGRAM_BYTES),
            Some(MEASURED_MIN_DATAGRAM_BYTES)
        )
    );
}

/// A crowded projection, packed to the largest size the zone publishes, is
/// delivered through the real host as one datagram, snapshot after snapshot.
#[tokio::test]
async fn a_budget_packed_crowded_projection_reaches_a_client() {
    let zone_id = ZoneId::new(1);
    let content = greyhaven_vale::content();
    let revision = content.revision();
    let mut adapter = ZoneGameServerAdapter::with_content(zone_id, content).unwrap();
    // Resting players without sessions crowd the spawn area beyond the cap.
    for index in 0..MAX_VISIBLE_ENTITIES + 16 {
        let player_id = 100_000 + u32::try_from(index).unwrap();
        adapter.zone_mut().add_player(player_id).unwrap();
    }
    let mut host = MatchHost::new(1).unwrap();
    host.insert(
        zone_match_id(zone_id).unwrap(),
        MatchRuntime::new(adapter, 120),
    )
    .map_err(|failure| failure.into_parts().0)
    .unwrap();
    let local = LocalHost::start(host, zone_id);
    let result = tokio::time::timeout(Duration::from_secs(20), async {
        let session =
            ClientSession::connect(&local.url, Some(&local.certificate), zone_id, revision).await?;
        // Idle players have no events, so the whole budget carries entities.
        let largest = PLAYER_SNAPSHOT_FIXED_BYTES + MAX_WIRE_ENTITIES * ENTITY_RECORD_BYTES;
        assert!(largest <= MAX_PLAYER_PROJECTION_BYTES);
        let mut previous_tick = None;
        for _ in 0..10 {
            let snapshot = session.receive_snapshot().await?;
            // The self sheet: bag, equipment and stat totals, then the quest
            // part (mask, entry count, entries, marker count, markers).
            let sheet_bytes = snapshot.quests.as_ref().map_or(0, |quests| {
                84 + 4 + 1 + 4 * quests.entries.len() + 1 + 3 * quests.markers.len()
            });
            // Every projection also carries the content doors (#158).
            let door_bytes = 2 * snapshot.doors.len();
            let expected_entities = (MAX_PLAYER_PROJECTION_BYTES
                - PLAYER_SNAPSHOT_FIXED_BYTES
                - sheet_bytes
                - door_bytes)
                / ENTITY_RECORD_BYTES;
            assert_eq!(snapshot.entities.len(), expected_entities);
            assert_eq!(snapshot.entities[0].id, session.player_id());
            assert_eq!(snapshot.entities[0].kind, EntityKind::Player);
            assert_eq!(
                mmorpg_protocol::encode_snapshot(&snapshot)?.len(),
                PLAYER_SNAPSHOT_FIXED_BYTES
                    + sheet_bytes
                    + door_bytes
                    + expected_entities * ENTITY_RECORD_BYTES
            );
            assert!(
                previous_tick < Some(snapshot.tick),
                "the session stays open"
            );
            previous_tick = Some(snapshot.tick);
        }
        Ok::<(), ClientError>(())
    })
    .await;
    local.stop().await;
    result.unwrap().unwrap();
}

/// Exercise the native receive loop with a valid v5 projection split into
/// datagrams. The largest budget-packed projection fits the measured
/// production path; a smaller send budget forces the transport branch without
/// changing policy.
#[tokio::test]
async fn fragmented_projection_reaches_the_native_client() {
    use game_server::{
        SnapshotFrame, Welcome, encode_snapshot, encode_snapshot_fragments, encode_welcome,
        snapshot_hash,
    };
    use wtransport::{Endpoint, Identity, ServerConfig};

    let zone_id = ZoneId::new(1);
    let revision = greyhaven_vale_definition().revision();
    let player_id = 42;
    let snapshot = ZoneSnapshot {
        doors: Vec::new(),
        projectiles: Vec::new(),
        debris: Vec::new(),
        pieces: Vec::new(),
        loot: None,
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        zone_id,
        tick: 7,
        content_revision: revision,
        acknowledged_sequence: 0,
        viewer_id: player_id,
        inventory_revision: 1,
        inventory: None,
        equipment: None,
        quests: None,
        viewer: ViewerState {
            experience: 0,
            experience_to_next_level: 100,
            health: 50,
            max_health: 50,
            level: 1,
            ..ViewerState::default()
        },
        target_of_target: None,
        target_detail: TargetDetail::default(),
        cooldowns: Vec::new(),
        auras: Vec::new(),
        events: Vec::new(),
        chat: Vec::new(),
        entities: (0..MAX_WIRE_ENTITIES)
            .map(|index| EntitySnapshot {
                kind: EntityKind::Player,
                id: player_id + u32::try_from(index).unwrap(),
                appearance: 0,
                position: [0; 3],
                velocity: [0; 3],
                facing: 0,
                level: 1,
                health_percent: 100,
                flags: EntityFlags::default(),
            })
            .collect(),
    };
    let payload = mmorpg_protocol::encode_snapshot(&snapshot).unwrap();
    let frame = encode_snapshot(&SnapshotFrame {
        tick: snapshot.tick,
        state_hash: snapshot_hash(snapshot.tick, &payload),
        payload,
    })
    .unwrap();
    let test_budget = game_server::MIN_FRAGMENTED_DATAGRAM_BYTES;
    assert!(frame.len() > test_budget);
    let fragments = encode_snapshot_fragments(&frame, test_budget).unwrap();
    assert!(fragments.len() > 1);
    assert!(
        fragments
            .iter()
            .all(|fragment| fragment.len() <= test_budget)
    );
    let tick = snapshot.tick;

    let identity = Identity::self_signed(["localhost"]).unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let certificate = temporary.path().join("cert.pem");
    std::fs::write(
        &certificate,
        identity.certificate_chain().as_slice()[0].to_pem(),
    )
    .unwrap();
    let server = Endpoint::server(
        ServerConfig::builder()
            .with_bind_default(0)
            .with_identity(identity)
            .build(),
    )
    .unwrap();
    let port = server.local_addr().unwrap().port();
    let (release, released) = tokio::sync::oneshot::channel::<()>();
    let host = tokio::spawn(async move {
        let request = server.accept().await.await.unwrap();
        let connection = request.accept().await.unwrap();
        let opening = connection.open_uni().await.unwrap();
        let mut stream = opening.await.unwrap();
        stream
            .write_all(&encode_welcome(Welcome {
                player_id,
                tick_hz: TICK_HZ,
                max_players: 512,
                current_tick: tick,
                connection_epoch: 1,
                reconnect_token: [1; 16],
                reconnect_grace_ticks: 120,
            }))
            .await
            .unwrap();
        stream.finish().await.unwrap();
        for fragment in fragments {
            connection.send_datagram(&fragment).unwrap();
        }
        let _ = released.await;
    });
    let result = tokio::time::timeout(Duration::from_secs(10), async {
        let session = ClientSession::connect(
            &format!("https://localhost:{port}/game/matches/zone-1"),
            Some(&certificate),
            zone_id,
            revision,
        )
        .await?;
        let delivered = session.receive_snapshot().await?;
        assert_eq!(delivered, snapshot);
        Ok::<(), ClientError>(())
    })
    .await;
    let _ = release.send(());
    host.await.unwrap();
    result.unwrap().unwrap();
}

async fn verify_automatic_resume(session: ClientSession) -> Result<(), ClientError> {
    use mmorpg_client::session::{NetworkUpdate, PlayerInput, run_session};
    use tokio::sync::{oneshot, watch};
    let player_id = session.player_id();
    let epoch = session.connection_epoch();
    session.disconnect();
    let (_input, input) = watch::channel(PlayerInput::default());
    let (updates, mut receiver) = watch::channel(NetworkUpdate::Waiting);
    let (shutdown, stopped) = oneshot::channel();
    // JoinSet aborts its owned task on every early-return/panic path.
    let mut tasks = tokio::task::JoinSet::new();
    tasks.spawn(async move { run_session(session, input, &updates, stopped).await });
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            receiver.changed().await?;
            if let NetworkUpdate::Snapshot {
                connection_epoch,
                snapshot,
                ..
            } = &*receiver.borrow_and_update()
            {
                assert_eq!(*connection_epoch, epoch + 1);
                assert!(snapshot.entities.iter().any(|p| p.id == player_id));
                break;
            }
        }
        Ok::<(), ClientError>(())
    })
    .await??;
    shutdown
        .send(())
        .map_err(|_| "worker exited before shutdown")?;
    tokio::time::timeout(Duration::from_secs(1), tasks.join_next())
        .await?
        .ok_or("missing worker")???;
    Ok(())
}

async fn verify_shutdown_during_resume(session: ClientSession) -> Result<(), ClientError> {
    use mmorpg_client::session::{NetworkUpdate, PlayerInput, run_session};
    use tokio::sync::{oneshot, watch};
    session.disconnect();
    let (_input, input) = watch::channel(PlayerInput::default());
    let (updates, mut receiver) = watch::channel(NetworkUpdate::Waiting);
    let (shutdown, stopped) = oneshot::channel();
    let mut tasks = tokio::task::JoinSet::new();
    tasks.spawn(async move { run_session(session, input, &updates, stopped).await });
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            receiver.changed().await?;
            if matches!(*receiver.borrow_and_update(), NetworkUpdate::Reconnecting) {
                break;
            }
        }
        Ok::<(), ClientError>(())
    })
    .await??;
    shutdown
        .send(())
        .map_err(|_| "worker exited before shutdown")?;
    tokio::time::timeout(Duration::from_secs(1), tasks.join_next())
        .await?
        .ok_or("missing worker")???;
    Ok(())
}

/// Drives `run_session` through its input channel, as the window does: a Jump
/// pressed while the connection is down is dropped on resume, each later press
/// sends one Jump, and a facing-only change while running turns the player.
async fn verify_live_input(session: ClientSession) -> Result<(), ClientError> {
    use mmorpg_client::session::run_session;
    use tokio::sync::oneshot;
    let player_id = session.player_id();
    let epoch = session.connection_epoch();
    // A freshly admitted player stands still on flat ground.
    let spawned = session.receive_snapshot().await?;
    assert!(is_at_rest(own(&spawned, player_id)));
    session.disconnect();
    let (input, watched) = watch::channel(PlayerInput::default());
    let (updates, mut receiver) = watch::channel(NetworkUpdate::Waiting);
    let (shutdown, stopped) = oneshot::channel();
    let mut tasks = tokio::task::JoinSet::new();
    tasks.spawn(async move { run_session(session, watched, &updates, stopped).await });
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            receiver.changed().await?;
            if matches!(*receiver.borrow_and_update(), NetworkUpdate::Reconnecting) {
                break;
            }
        }
        // Resume publishes its snapshot before it rereads input, so the press
        // lands in the outage while `Reconnecting` is still the latest update.
        input.send_modify(|input| input.jumps += 1);
        assert!(
            matches!(*receiver.borrow(), NetworkUpdate::Reconnecting),
            "the press must happen before the session resumes"
        );
        let resumed = next_snapshot(&mut receiver).await?;
        assert_eq!(resumed.0, epoch + 1);
        // Any input change reaches the outbox; a replayed press would ride along.
        input.send_modify(|input| input.facing = EAST);
        let turned = wait_for(&mut receiver, player_id, |player| player.facing == EAST).await?;
        assert!(
            is_at_rest(own(&turned, player_id)),
            "the outage press must not be sent with the first change after resume"
        );
        assert_stays_at_rest(&mut receiver, player_id, turned.tick).await?;

        input.send_modify(|input| input.jumps += 1);
        wait_for(&mut receiver, player_id, |player| {
            player.position[1] > PLAYER_HALF_EXTENTS_UNITS[1]
        })
        .await?;
        let landed = wait_for(&mut receiver, player_id, is_at_rest).await?;
        // A repeated Jump would lift the player again as soon as it lands.
        assert_stays_at_rest(&mut receiver, player_id, landed.tick).await?;

        input.send_modify(|input| {
            input.forward = 1;
            input.facing = NORTH;
        });
        wait_for(&mut receiver, player_id, |player| {
            player.facing == NORTH && player.velocity[2] > 0
        })
        .await?;
        input.send_modify(|input| input.facing = EAST);
        wait_for(&mut receiver, player_id, |player| {
            player.facing == EAST && player.velocity[0] > 0 && player.velocity[2] == 0
        })
        .await?;
        Ok::<(), ClientError>(())
    })
    .await??;
    shutdown
        .send(())
        .map_err(|_| "worker exited before shutdown")?;
    tokio::time::timeout(Duration::from_secs(1), tasks.join_next())
        .await?
        .ok_or("missing worker")???;
    Ok(())
}

fn own(snapshot: &ZoneSnapshot, player_id: u32) -> &EntitySnapshot {
    snapshot
        .entities
        .iter()
        .find(|player| player.id == player_id)
        .expect("a player always sees itself")
}

/// Feet on the ground with no vertical motion.
fn is_at_rest(player: &EntitySnapshot) -> bool {
    player.position[1] == PLAYER_HALF_EXTENTS_UNITS[1] && player.velocity[1] == 0
}

async fn next_snapshot(
    receiver: &mut watch::Receiver<NetworkUpdate>,
) -> Result<(u32, Arc<ZoneSnapshot>), ClientError> {
    loop {
        receiver.changed().await?;
        match &*receiver.borrow_and_update() {
            NetworkUpdate::Snapshot {
                connection_epoch,
                snapshot,
                ..
            } => return Ok((*connection_epoch, Arc::clone(snapshot))),
            NetworkUpdate::Failed(error) => return Err(error.clone().into()),
            NetworkUpdate::Waiting | NetworkUpdate::Reconnecting => {}
        }
    }
}

/// The first published snapshot, within two seconds of server ticks, in which
/// the player satisfies `condition`. A full jump lands within about 33 ticks.
async fn wait_for(
    receiver: &mut watch::Receiver<NetworkUpdate>,
    player_id: u32,
    condition: impl Fn(&EntitySnapshot) -> bool,
) -> Result<Arc<ZoneSnapshot>, ClientError> {
    let mut snapshot = next_snapshot(receiver).await?.1;
    let deadline = snapshot.tick + 2 * u64::from(TICK_HZ);
    while !condition(own(&snapshot, player_id)) {
        if snapshot.tick > deadline {
            return Err(format!("player never reached the expected state: {snapshot:?}").into());
        }
        snapshot = next_snapshot(receiver).await?.1;
    }
    Ok(snapshot)
}

/// Every snapshot for half a second of server ticks after `since` shows the
/// player standing on the ground.
async fn assert_stays_at_rest(
    receiver: &mut watch::Receiver<NetworkUpdate>,
    player_id: u32,
    since: u64,
) -> Result<(), ClientError> {
    loop {
        let (_, snapshot) = next_snapshot(receiver).await?;
        let player = own(&snapshot, player_id);
        assert!(
            is_at_rest(player),
            "no Jump may be sent here, but the player left the ground: {player:?}"
        );
        if snapshot.tick >= since + u64::from(TICK_HZ / 2) {
            return Ok(());
        }
    }
}

#[tokio::test]
async fn durable_character_resumes_after_logout_and_host_restart() -> Result<(), ClientError> {
    use mmorpg_core::{CharacterId, ClassChoice};
    let directory = tempfile::tempdir()?;
    let zone = ZoneId::new(1);
    let revision = greyhaven_vale::content().revision();
    let character = CharacterId::new("persistent-native-ranger")?;
    let class = ClassChoice::from_appearance(3).unwrap();
    let first = LocalHost::start_mode(HostMode::Persistent(directory.path().to_owned()), zone);
    let mut client =
        ClientSession::connect(&first.url, Some(&first.certificate), zone, revision).await?;
    assert!(
        client
            .claim_character(&character, class)
            .await
            .map_err(|error| format!("claim durable character: {error}"))?
    );
    let initial = client
        .receive_snapshot()
        .await
        .map_err(|error| format!("receive durable snapshot: {error}"))?;
    client.send_move(1, 0, EAST)?;
    let mut moved = initial.clone();
    for _ in 0..6 {
        moved = client
            .receive_snapshot()
            .await
            .map_err(|error| format!("receive durable snapshot: {error}"))?;
    }
    client.send_move(0, 0, EAST)?;
    while moved.acknowledged_sequence < 2 {
        moved = client
            .receive_snapshot()
            .await
            .map_err(|error| format!("receive durable snapshot: {error}"))?;
    }
    let position = moved
        .entities
        .iter()
        .find(|unit| unit.kind == EntityKind::Player && unit.id == moved.viewer_id)
        .unwrap()
        .position;
    assert_ne!(
        position,
        initial
            .entities
            .iter()
            .find(|unit| unit.kind == EntityKind::Player && unit.id == initial.viewer_id)
            .unwrap()
            .position
    );
    client
        .release_character()
        .await
        .map_err(|error| format!("release durable character: {error}"))?;
    drop(client);
    first.stop().await;

    let second = LocalHost::start_mode(HostMode::Persistent(directory.path().to_owned()), zone);
    let mut client =
        ClientSession::connect(&second.url, Some(&second.certificate), zone, revision).await?;
    assert!(
        client
            .claim_character(&character, class)
            .await
            .map_err(|error| format!("claim durable character: {error}"))?
    );
    let restored = client
        .receive_snapshot()
        .await
        .map_err(|error| format!("receive durable snapshot: {error}"))?;
    assert_eq!(restored.viewer.class, Some(class));
    let actual = restored
        .entities
        .iter()
        .find(|unit| unit.kind == EntityKind::Player && unit.id == restored.viewer_id)
        .unwrap()
        .position;
    assert_eq!([actual[0], actual[2]], [position[0], position[2]]);
    client
        .release_character()
        .await
        .map_err(|error| format!("release durable character: {error}"))?;
    drop(client);
    second.stop().await;
    Ok(())
}

#[tokio::test]
async fn durable_character_selection_worker_claims_before_gameplay_and_logs_out()
-> Result<(), ClientError> {
    use mmorpg_client::session::{CharacterSelection, run_session_with_characters};
    use mmorpg_core::{CharacterId, ClassChoice};
    let directory = tempfile::tempdir()?;
    let zone = ZoneId::new(1);
    let local = LocalHost::start_mode(HostMode::Persistent(directory.path().to_owned()), zone);
    let session = ClientSession::connect(
        &local.url,
        Some(&local.certificate),
        zone,
        greyhaven_vale::content().revision(),
    )
    .await?;
    let (input_sender, input) = watch::channel(PlayerInput {
        forward: 1,
        class_choice: Some((1, 0)),
        ..PlayerInput::default()
    });
    let (_selection, characters) = watch::channel(Some(CharacterSelection {
        local_id: "local-1".into(),
        class: ClassChoice::from_appearance(3).unwrap(),
        global_id: Some(CharacterId::new("worker-ranger")?),
    }));
    let (updates, mut received) = watch::channel(NetworkUpdate::Waiting);
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        run_session_with_characters(session, input, characters, &updates, stopped).await
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            received.changed().await.unwrap();
            match &*received.borrow_and_update() {
                NetworkUpdate::Snapshot {
                    snapshot, refusal, ..
                } if snapshot.acknowledged_sequence > 0 => {
                    assert_eq!(snapshot.viewer.class, ClassChoice::from_appearance(3));
                    assert!(refusal.is_none());
                    break;
                }
                NetworkUpdate::Failed(error) => panic!("{error}"),
                _ => {}
            }
        }
    })
    .await?;
    stop.send(()).unwrap();
    task.await??;
    drop(input_sender);
    local.stop().await;
    Ok(())
}
