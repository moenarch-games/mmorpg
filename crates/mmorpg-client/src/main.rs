#![forbid(unsafe_code)]

mod desktop;

use mmorpg_client::{
    ClientError,
    camera::OrbitCamera,
    graphics::render_offscreen,
    hud::CombatHud,
    network::ClientSession,
    presentation::Presentation,
    session::{NetworkUpdate, PlayerInput, run_session},
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
    class: PlayerClass,
    sex: Sex,
}

impl Options {
    fn parse(args: impl IntoIterator<Item = String>) -> Result<Option<Self>, ClientError> {
        let mut options = Self {
            url: "https://localhost:4433/game/matches/zone-1".into(),
            certificate: None,
            zone_id: ZoneId::new(1),
            smoke: false,
            frames: None,
            class: PlayerClass::Warden,
            sex: Sex::Male,
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
                    let name = args
                        .next()
                        .ok_or("--class needs warden, ranger or arcanist")?;
                    options.class = PlayerClass::ALL
                        .into_iter()
                        .find(|class| class.name() == name)
                        .ok_or("--class must be warden, ranger or arcanist")?;
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
        Ok(Some(options))
    }
}

fn main() -> Result<(), ClientError> {
    let Some(options) = Options::parse(std::env::args().skip(1))? else {
        println!(
            "mmorpg-client [--url https://host:4433/game/matches/zone-1] [--zone 1] [--certificate cert.pem] [--class warden|ranger|arcanist] [--sex female|male] [--smoke] [--frames N]\nW/S move, A/D or Q/E strafe, Space jumps; Tab targets the nearest creature, F attacks, 1-4 use the class abilities, R releases a dead spirit; drag a mouse button to orbit, wheel zooms. Escape cancels a cast, otherwise closes. --smoke connects, chooses the class and verifies an offscreen GPU frame.\nWithout --certificate, system certificate trust is used. The class defaults to a male Warden."
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
    let class_choice = (options.class.code(), options.sex.code());
    if options.smoke {
        return runtime.block_on(async {
            // Choose the class until the zone confirms it (a datagram may be
            // lost), then render the resumed authoritative world.
            let mut chosen = None;
            for _ in 0..90 {
                session.send_choose_class(class_choice.0, class_choice.1)?;
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
            let colors = render_offscreen(&world, &presentation.scene(now, view), &hud, view).await?;
            println!("{{\"event\":\"client_smoke_passed\",\"player_id\":{player_id},\"class\":\"{}\",\"tick\":{tick},\"connection_epoch\":{connection_epoch},\"rendered_colors\":{colors}}}", class.name());
            Ok(())
        });
    }
    // The session sends the class choice until a projection confirms it.
    let (input_sender, input_receiver) = watch::channel(PlayerInput {
        class_choice: Some(class_choice),
        ..PlayerInput::default()
    });
    let (update_sender, update_receiver) = watch::channel(NetworkUpdate::Waiting);
    let (shutdown_sender, shutdown_receiver) = oneshot::channel();
    let mut task = runtime.spawn(async move {
        if let Err(error) =
            run_session(session, input_receiver, &update_sender, shutdown_receiver).await
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
        update_receiver,
        options.frames,
        options.class,
    );
    // A closed worker already owns its connection cleanup.
    let _ = shutdown_sender.send(());
    runtime.block_on(async {
        match tokio::time::timeout(Duration::from_secs(3), &mut task).await {
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
            vec!["--class", "rogue"],
            vec!["--class"],
            vec!["--sex", "other"],
        ] {
            assert!(Options::parse(args.into_iter().map(str::to_owned)).is_err());
        }
    }
}
