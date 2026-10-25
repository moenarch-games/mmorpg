#![forbid(unsafe_code)]

mod desktop;

use mmorpg_client::{
    ClientError,
    camera::OrbitCamera,
    graphics::render_offscreen,
    hud::CombatHud,
    network::ClientSession,
    presentation::Presentation,
    session::{CharacterSelection, NetworkUpdate, PlayerInput, run_session_with_characters},
    world::WorldScene,
};
use mmorpg_core::{PlayerClass, Sex, ZoneId, greyhaven_vale};
use mmorpg_scenery::greyhaven_vale_scenery;
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{oneshot, watch};

struct Options {
    url: String,
    certificate: Option<PathBuf>,
    zone_id: ZoneId,
    smoke: bool,
    frames: Option<u32>,
    /// Without `--class`, the window offers character selection.
    class: Option<PlayerClass>,
    sex: Sex,
    character_id: Option<mmorpg_core::CharacterId>,
}

impl Options {
    fn parse(args: impl IntoIterator<Item = String>) -> Result<Option<Self>, ClientError> {
        let mut options = Self {
            url: "https://localhost:4433/game/matches/zone-1".into(),
            certificate: None,
            zone_id: ZoneId::new(1),
            smoke: false,
            frames: None,
            class: None,
            sex: Sex::Male,
            character_id: None,
        };
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--help" | "-h" => return Ok(None),
                "--url" => options.url = args.next().ok_or("--url needs a value")?,
                "--certificate" => {
                    options.certificate =
                        Some(args.next().ok_or("--certificate needs a PEM path")?.into())
                }
                "--zone" => {
                    options.zone_id = ZoneId::new(args.next().ok_or("--zone needs an ID")?.parse()?)
                }
                "--smoke" => options.smoke = true,
                "--class" => {
                    let name = args.next().ok_or(
                        "--class needs warden, ranger, arcanist, necromancer, rogue, cleric or druid",
                    )?;
                    options.class = Some(
                        PlayerClass::ALL
                            .into_iter()
                            .find(|class| class.name() == name)
                            .ok_or(
                                "--class must be warden, ranger, arcanist, necromancer, rogue, cleric or druid",
                            )?,
                    );
                }
                "--character-id" => {
                    options.character_id = Some(mmorpg_core::CharacterId::new(
                        args.next().ok_or("--character-id needs an ID")?,
                    )?);
                }
                "--sex" => {
                    options.sex = match args.next().as_deref() {
                        Some("female") => Sex::Female,
                        Some("male") => Sex::Male,
                        _ => return Err("--sex must be female or male".into()),
                    };
                }
                "--frames" => {
                    let count = args.next().ok_or("--frames needs a count")?.parse()?;
                    if count == 0 {
                        return Err("--frames must be positive".into());
                    }
                    options.frames = Some(count);
                }
                _ => return Err(format!("unknown option {arg}").into()),
            }
        }
        if !options.url.starts_with("https://") {
            return Err("WebTransport requires an https URL".into());
        }
        if options.character_id.is_some() && options.class.is_none() {
            return Err("--character-id requires --class".into());
        }
        Ok(Some(options))
    }
}

fn main() -> Result<(), ClientError> {
    let Some(options) = Options::parse(std::env::args().skip(1))? else {
        println!(
            "mmorpg-client [--url https://host:4433/game/matches/zone-1] [--zone 1] [--certificate cert.pem] [--class warden|ranger|arcanist] [--sex female|male] [--character-id ID] [--smoke] [--frames N]\nW/S move, A/D strafe, Space jumps (hold to swim up), X swims down; Tab targets the nearest creature, F attacks, 1-8 use the class abilities, Y/U/I/O order a Necromancer's skeletons to attack the target, defend here, follow or retreat, R releases a dead spirit; drag a mouse button to orbit, wheel zooms. E talks to a selected NPC or uses a door in reach, Enter chats, B bags, C character, L quests. Escape cancels a cast, closes panels, otherwise quits. --smoke connects, chooses the class and verifies an offscreen GPU frame.\nWithout --certificate, system certificate trust is used. Without --class the window offers character selection (smoke uses a male Warden)."
        );
        return Ok(());
    };
    let runtime = Arc::new(tokio::runtime::Runtime::new()?);
    // Scenery derives from the shared core content revision the host runs;
    // the content names and sizes its units.
    let scenery = greyhaven_vale_scenery();
    let content = greyhaven_vale::content();
    let world = WorldScene::new(&scenery);
    let mut session = runtime.block_on(ClientSession::connect(
        &options.url,
        options.certificate.as_deref(),
        options.zone_id,
        scenery.content_revision,
    ))?;
    let player_id = session.player_id();
    let class_choice = (
        options.class.unwrap_or(PlayerClass::Warden).code(),
        options.sex.code(),
    );
    let selected = (options.class.is_some() || options.smoke).then(|| CharacterSelection {
        local_id: format!("cli-{}-{}", class_choice.0, class_choice.1),
        class: mmorpg_core::ClassChoice {
            class: options.class.unwrap_or(PlayerClass::Warden),
            sex: options.sex,
        },
        global_id: options.character_id,
    });
    if options.smoke {
        return runtime.block_on(async {
            // Choose the class until the zone confirms it (a datagram may be
            // lost), then render the resumed authoritative world.
            let selected = selected.ok_or("missing smoke character")?;
            let durable = if session.character_service_enabled().await? {
                let id = match selected.global_id {
                    Some(id) => id,
                    None => mmorpg_client::online_identity::for_character(
                        &mmorpg_client::roster::config_dir().ok_or("no character configuration directory")?,
                        &selected.local_id,
                    )?,
                };
                session.claim_character(&id, selected.class).await?
            } else { false };
            let mut chosen = None;
            for _ in 0..90 {
                if !durable { session.send_choose_class(class_choice.0, class_choice.1)?; }
                chosen = session.receive_snapshot().await?.viewer.class;
                if chosen.is_some() {
                    break;
                }
            }
            let class = chosen.ok_or("the zone did not take the class choice")?.class;
            let snapshot = session.reconnect().await?;
            let connection_epoch = session.connection_epoch();
            let tick = snapshot.tick;
            let hud = CombatHud::from_projection(&snapshot).rects();
            let mut presentation = Presentation::new(player_id, scenery, content, Instant::now())?;
            presentation.push(snapshot, Instant::now())?;
            let now = Instant::now();
            let view = OrbitCamera::default().view(presentation.camera_target(now));
            let colors = render_offscreen(&world, &presentation.unit_draws(now, view), &hud, view).await?;
            session.release_character().await?;
            println!("{{\"event\":\"client_smoke_passed\",\"player_id\":{player_id},\"class\":\"{}\",\"tick\":{tick},\"connection_epoch\":{connection_epoch},\"rendered_colors\":{colors}}}", class.name());
            Ok(())
        });
    }
    // The session sends the class choice until a projection confirms it.
    let (input_sender, input_receiver) = watch::channel(PlayerInput {
        class_choice: options.class.map(|_| class_choice),
        ..PlayerInput::default()
    });
    let (character_sender, character_receiver) = watch::channel(selected);
    let (update_sender, update_receiver) = watch::channel(NetworkUpdate::Waiting);
    let (shutdown_sender, shutdown_receiver) = oneshot::channel();
    let mut task = runtime.spawn(async move {
        if let Err(error) = run_session_with_characters(
            session,
            input_receiver,
            character_receiver,
            &update_sender,
            shutdown_receiver,
        )
        .await
        {
            // The owning window observes this final state; no detached errors.
            update_sender.send_replace(NetworkUpdate::Failed(error.to_string()));
        }
    });
    let result = desktop::run(
        Arc::clone(&runtime),
        player_id,
        scenery,
        content,
        world,
        input_sender,
        character_sender,
        update_receiver,
        options.frames,
        options.class.map(|class| (class, options.sex)),
    );
    // A closed worker already owns its connection cleanup.
    let _ = shutdown_sender.send(());
    runtime.block_on(async {
        match tokio::time::timeout(Duration::from_secs(6), &mut task).await {
            Ok(result) => result?,
            Err(_) => {
                task.abort();
                if let Err(error) = task.await
                    && !error.is_cancelled()
                {
                    return Err::<(), ClientError>(error.into());
                }
            }
        }
        Ok::<(), ClientError>(())
    })?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn options_fail_closed_on_unknown_or_invalid_configuration() {
        for args in [
            vec!["--unknown"],
            vec!["--url", "http://localhost"],
            vec!["--certificate"],
            vec!["--zone", "invalid"],
            vec!["--frames", "0"],
            vec!["--class", "bard"],
            vec!["--class"],
            vec!["--sex", "other"],
        ] {
            assert!(Options::parse(args.into_iter().map(str::to_owned)).is_err());
        }
    }
}
