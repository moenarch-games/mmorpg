//! Opt-in real-process stage-5 check. Uses the native session adapter without a GPU;
//! it never substitutes for missing-asset or two-network acceptance.
use mmorpg_client::{ClientError, network::ClientSession};
use mmorpg_core::{CharacterId, ClassChoice, EntityKind, ZoneId, ZoneSnapshot};
use mmorpg_game_server::{
    invitation::JoinInvite,
    local_world::{self, RunningWorld},
};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

fn report(value: serde_json::Value) {
    println!("{value}");
}
fn directory() -> Result<PathBuf, ClientError> {
    Ok(mmorpg_client::roster::config_dir()
        .ok_or("Set MMORPG_CONFIG_DIR for this check")?
        .join("worlds/default"))
}
fn executable() -> Result<PathBuf, ClientError> {
    Ok(std::env::current_exe()?
        .parent()
        .and_then(|p| p.parent())
        .ok_or("Missing debug binary directory")?
        .join("mmorpg-zone-host"))
}
async fn invite(world: &RunningWorld) -> Result<String, ClientError> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let status = world.share_status()?;
        if let Some(invite) = status.invitation {
            return Ok(invite);
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "World registration failed: {}",
                status.problem.unwrap_or_default()
            )
            .into());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
fn own(snapshot: &ZoneSnapshot) -> Result<[i32; 3], ClientError> {
    snapshot
        .entities
        .iter()
        .find(|e| e.kind == EntityKind::Player && e.id == snapshot.viewer_id)
        .map(|e| e.position)
        .ok_or_else(|| "Missing own authoritative unit".into())
}
fn class() -> ClassChoice {
    ClassChoice {
        class: mmorpg_core::PlayerClass::Warden,
        sex: mmorpg_core::Sex::Male,
    }
}

#[tokio::main]
async fn main() -> Result<(), ClientError> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let mode = args
        .first()
        .map(String::as_str)
        .ok_or("Probe mode missing")?;
    // Observing/stopping an already managed local world does not create public lobbies.
    if std::env::var_os("MMORPG_TEST_SETUP_ENDPOINT").is_none()
        && !matches!(mode, "status" | "stop" | "approve" | "remove")
    {
        return Err("Starting probe sessions requires a loopback test setup service".into());
    }
    match mode {
        "corrupt-host" => {
            use game_server::webrtc_transport::{Lobby, WebRtcHost, WebRtcHostConfig};
            use std::sync::Arc;
            let access = mmorpg_game_server::guest_access::GuestAccess::open(&directory()?)?;
            let catalogue = mmorpg_game_server::shared_assets::Catalogue::open(
                &mmorpg_game_server::shared_assets::installed_directory()?,
                mmorpg_core::content_for_zone(ZoneId::new(1)).revision(),
            )?;
            let lobby = Lobby::create(mmorpg_game_server::invitation::setup_config()?, 4).await?;
            let lobby_id = lobby.lobby_id().to_owned();
            let matched = game_server::MatchId::new("zone-1")?;
            let runtime = game_server::MatchRuntime::new(
                mmorpg_game_server::ZoneGameServerAdapter::with_content(
                    ZoneId::new(1),
                    mmorpg_core::content_for_zone(ZoneId::new(1)),
                )?,
                u64::from(mmorpg_core::TICK_HZ) * 20,
            );
            let host = WebRtcHost::new(
                lobby,
                matched.clone(),
                Arc::new(tokio::sync::Mutex::new(runtime)),
                Arc::new(NoCharacters),
                WebRtcHostConfig {
                    bind_ip: "127.0.0.1".into(),
                    relay_only: false,
                },
            )?
            .with_content_service(Arc::new(CorruptArt(catalogue)));
            report(
                serde_json::json!({"event":"corrupt_host_ready", "invitation":JoinInvite { lobby_id:lobby_id.clone(), match_id:matched.clone() }.url()}),
            );
            let validator = access.clone();
            host.run_with_authorization(
                move |participant, proof| {
                    let access = access.clone();
                    let lobby = lobby_id.clone();
                    let matched = matched.as_str().to_owned();
                    async move { access.authorize(lobby, matched, participant, proof).await }
                },
                move |participant| validator.character(participant).is_some(),
                std::future::pending(),
            )
            .await?;
        }
        "owner" => {
            let world = local_world::start(&directory()?, &executable()?)?;
            let invitation = invite(&world).await?;
            let mut session = ClientSession::connect(
                &world.url(),
                Some(&world.certificate),
                ZoneId::new(1),
                mmorpg_core::content_for_zone(ZoneId::new(1)).revision(),
            )
            .await?;
            session.set_local_owner(world.clone());
            session
                .claim_character(&CharacterId::new("owner-probe")?, class())
                .await?;
            let first = session.receive_snapshot().await?;
            report(
                serde_json::json!({"event":"hosting_ready", "invitation":invitation, "tick":first.tick}),
            );
            let deadline = Instant::now() + Duration::from_secs(90);
            let mut first_guest = None;
            loop {
                if Instant::now() >= deadline {
                    return Err("Owner did not observe authoritative guest movement".into());
                }
                for guest in world.share_status()?.guests {
                    if guest.pending {
                        world.guest_decision(&guest.key, true)?;
                    }
                }
                let snapshot = session.receive_snapshot().await?;
                if let Some(guest) = snapshot
                    .entities
                    .iter()
                    .find(|e| e.kind == EntityKind::Player && e.id != snapshot.viewer_id)
                {
                    if let Some(position) = first_guest {
                        if position != guest.position {
                            report(
                                serde_json::json!({"event":"owner_passed", "owner":snapshot.viewer_id, "guest":guest.id, "tick":snapshot.tick}),
                            );
                            break;
                        }
                    } else {
                        first_guest = Some(guest.position);
                    }
                }
            }
            session.release_character().await?;
        }
        "guest" | "resume" => {
            let invitation = JoinInvite::parse(args.get(1).ok_or("Guest invitation missing")?)?;
            let mut session =
                ClientSession::connect_invite(&invitation, "guest-probe", |message| {
                    eprintln!("{message}")
                })
                .await?;
            // A hostile supplied ID must not impersonate the owner: scoped control
            // binds the admitted key to its independently server-assigned character.
            session
                .claim_character(&CharacterId::new("owner-probe")?, class())
                .await?;
            session.party_hello("Guest Probe").await?;
            let initial = session.receive_snapshot().await?;
            let first = own(&initial)?;
            session.send_move(1, 0, 0).await?;
            let mut moved = first;
            for _ in 0..45 {
                moved = own(&session.receive_snapshot().await?)?;
            }
            session.send_move(0, 0, 0).await?;
            if first == moved {
                return Err("Authoritative guest movement did not advance".into());
            }
            let player = session.player_id();
            let epoch = session.connection_epoch();
            let resumed = session.reconnect().await?;
            if session.player_id() != player || session.connection_epoch() <= epoch {
                return Err("Guest reconnect lost fencing identity".into());
            }
            let mut without_owner = false;
            let mut last = resumed;
            for _ in 0..90 {
                last = session.receive_snapshot().await?;
                without_owner |= !last
                    .entities
                    .iter()
                    .any(|e| e.kind == EntityKind::Player && e.id != last.viewer_id);
            }
            if !without_owner {
                return Err("Guest did not continue after the owner client exited".into());
            }
            let metrics = session
                .connectivity_metrics()
                .await
                .ok_or("Missing actual WebRTC path")?;
            report(
                serde_json::json!({"event":"guest_ready", "player":player, "epoch":session.connection_epoch(), "initial":first, "position":own(&last)?, "tick":last.tick, "relayed":metrics.relayed, "continuedWithoutOwner":without_owner}),
            );
            if mode == "guest" {
                let closed = tokio::time::timeout(Duration::from_secs(15), async {
                    loop {
                        if session.receive_snapshot().await.is_err() {
                            break;
                        }
                    }
                })
                .await;
                if closed.is_err() {
                    return Err("Revoked guest remained connected".into());
                }
                if session.reconnect().await.is_ok() {
                    return Err("Revoked guest resumed".into());
                }
                report(serde_json::json!({"event":"revocation_passed"}));
            } else {
                session.release_character().await?;
            }
        }
        "start" => {
            let world = local_world::start(&directory()?, &executable()?)?;
            report(
                serde_json::json!({"event":"hosting_ready", "invitation":invite(&world).await?}),
            );
        }
        "status" => {
            let world = local_world::running(&directory()?)?.ok_or("World stopped")?;
            report(serde_json::to_value(world.share_status()?)?);
        }
        "approve" | "remove" => {
            let world = local_world::running(&directory()?)?.ok_or("World stopped")?;
            let status = world.share_status()?;
            if status.guests.len() != 1 {
                return Err("Probe expects exactly one controlled guest".into());
            }
            for guest in status.guests {
                world.guest_decision(&guest.key, mode == "approve")?;
            }
        }
        "stop" => local_world::stop(&directory()?)?,
        "stale" => {
            let invitation = JoinInvite::parse(args.get(1).ok_or("Stale invitation missing")?)?;
            if ClientSession::connect_invite(&invitation, "guest-probe", |_| {})
                .await
                .is_ok()
            {
                return Err("Stopped world admitted a stale invitation".into());
            }
            report(serde_json::json!({"event":"stale_invitation_rejected"}));
        }
        _ => return Err("Unknown probe mode".into()),
    }
    Ok(())
}

// Deliberately hostile content provider lives only in this opt-in test executable.
// The production host always verifies and serves its immutable approved bytes.
struct CorruptArt(mmorpg_game_server::shared_assets::Catalogue);
impl game_server::webrtc_transport::WebRtcContentService for CorruptArt {
    fn handle<'a>(
        &'a self,
        _: &'a str,
        _: &'a game_server::MatchId,
        _: game_server::ControlContext,
        request: &'a [u8],
    ) -> game_server::webrtc_transport::WebRtcContentFuture<'a> {
        Box::pin(async move {
            let mut bytes = self
                .0
                .response(request)
                .map_err(game_server::ControlServiceError::new)?;
            if request.first() == Some(&1)
                && let Some(first) = bytes.first_mut()
            {
                *first ^= 1;
            }
            Ok(bytes)
        })
    }
}
struct NoCharacters;
impl game_server::ControlService for NoCharacters {
    fn handle(
        &self,
        _: game_server::ControlContext,
        _: &[u8],
    ) -> Result<Vec<u8>, game_server::ControlServiceError> {
        Err(game_server::ControlServiceError::new(
            "isolated fault-test world has no character storage",
        ))
    }
}
