#![forbid(unsafe_code)]
use game_server::{
    BrowserRoutePrefix, DEFAULT_HOST_STATUS_PORT, MatchHostStatusConfig,
    MatchHostWebTransportConfig,
};
use mmorpg_core::{ZoneId, long_ticks_from_ms};
use mmorpg_game_server::{
    DevelopmentCharacterHostConfig, local_world, party::serve_open_zone_host,
    serve_development_character_host,
};
use std::env;
use std::error::Error;
use std::fmt;
use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;
use tokio::sync::mpsc;

const DEFAULT_PORT: u16 = 4433;
const DEFAULT_DRAIN_GRACE_MS: u64 = 500;
/// A disconnected player keeps its unit this long (20 s) before it leaves the zone.
const DEFAULT_RECONNECT_GRACE_TICKS: u64 = long_ticks_from_ms(20_000) as u64;
const DEFAULT_ROUTE_PREFIX: &str = "/game";
const DEFAULT_ZONE_IDS: &str = "1";
#[cfg(feature = "postgres")]
const DEFAULT_TRANSFER_LINK: &str = "127.0.0.1:4500";
#[cfg(feature = "postgres")]
const DEFAULT_GRANT_TTL_MS: u64 = 30_000;
#[cfg(feature = "postgres")]
const DEFAULT_HANDOFF_TIMEOUT_MS: u64 = 2_000;

#[derive(Clone, Debug, Eq, PartialEq)]
struct ConfigError(String);

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for ConfigError {}

fn main() -> Result<(), Box<dyn Error>> {
    let arguments: Vec<_> = env::args_os().skip(1).collect();
    if !arguments.is_empty() {
        if arguments.len() != 2 || arguments[0] != "--local-world" {
            return Err(ConfigError(
                "usage: mmorpg-zone-host [--local-world SAVE_DIRECTORY]".into(),
            )
            .into());
        }
        let directory = PathBuf::from(&arguments[1]);
        // Declare ownership before the runtime: even on failure/panic, runtime teardown
        // joins every writer before the save lock can be released.
        let mut owner = local_world::LocalWorldHost::acquire(&directory)
            .map_err(|error| ConfigError(error.to_string()))?;
        let runtime = tokio::runtime::Runtime::new()?;
        let result = runtime.block_on(serve_local_world(directory, &mut owner));
        drop(runtime);
        drop(owner);
        if let Err(error) = &result
            && local_world::is_port_conflict(error.as_ref())
        {
            // The launcher starts a fresh host on new ports.
            eprintln!("Error: {error}");
            std::process::exit(local_world::PORT_CONFLICT_EXIT_CODE);
        }
        return result;
    }
    tokio::runtime::Runtime::new()?.block_on(serve_configured_host())
}

async fn serve_configured_host() -> Result<(), Box<dyn Error>> {
    let zone_ids = parse_zone_ids(&read_string("MMORPG_ZONE_IDS", DEFAULT_ZONE_IDS)?)?;
    let port = read_number("MMORPG_PORT", DEFAULT_PORT)?;
    let status_port = read_number("MMORPG_STATUS_PORT", DEFAULT_HOST_STATUS_PORT)?;
    let reconnect_grace_ticks = read_number(
        "MMORPG_RECONNECT_GRACE_TICKS",
        DEFAULT_RECONNECT_GRACE_TICKS,
    )?;
    let drain_grace_ms = read_number("MMORPG_DRAIN_GRACE_MS", DEFAULT_DRAIN_GRACE_MS)?;
    let certificate_pem = PathBuf::from(read_string("MMORPG_CERT_PEM", "cert.pem")?);
    let private_key_pem = PathBuf::from(read_string("MMORPG_KEY_PEM", "key.pem")?);
    let recovery_directory = read_optional_path("MMORPG_RECOVERY_DIR")?;
    let character_directory = read_optional_path("MMORPG_CHARACTER_DIR")?;
    let route_prefix =
        BrowserRoutePrefix::new(read_string("MMORPG_ROUTE_PREFIX", DEFAULT_ROUTE_PREFIX)?)?;
    let bind_ip = parse_bind_ip(read_optional_string("MMORPG_BIND_IP")?.as_deref())?;
    if env::var_os("MMORPG_PERSISTENCE_MODE").is_some() {
        require_claim_acknowledgement(
            bind_ip,
            read_string("MMORPG_ALLOW_UNAUTHENTICATED_CLAIMS", "")? == "1",
        )?;
    }
    eprintln!(
        "Zone host serving on {bind_ip} (WebTransport UDP {port}, status TCP {status_port})."
    );
    if !bind_ip.is_loopback() {
        eprintln!(
            "Reachable beyond this machine: character claims are unauthenticated until account authentication exists."
        );
    }

    let (shutdown_sender, shutdown_receiver) = mpsc::channel(4);
    install_shutdown_forwarder(shutdown_sender)?;
    let transport_config = MatchHostWebTransportConfig {
        bind_ip: Some(bind_ip),
        port,
        certificate_pem,
        private_key_pem,
        route_prefix,
        allowed_origins: None,
        drain_grace: Duration::from_millis(drain_grace_ms),
    };
    let status_config = MatchHostStatusConfig { port: status_port };

    #[cfg(feature = "postgres")]
    if env::var_os("MMORPG_PERSISTENCE_MODE").is_some() {
        if character_directory.is_some() || recovery_directory.is_some() {
            return Err(ConfigError("PostgreSQL persistence cannot be combined with file character/recovery directories".into()).into());
        }
        eprintln!("PostgreSQL persistence selected; character claims are unauthenticated.");
        mmorpg_game_server::persistence::serve_postgres_character_host(
            zone_ids,
            reconnect_grace_ticks,
            transport_config,
            status_config,
            shutdown_receiver,
        )
        .await
        .map_err(|cause| ConfigError(cause.to_string()))?;
        return Ok(());
    }
    #[cfg(not(feature = "postgres"))]
    if env::var_os("MMORPG_PERSISTENCE_MODE").is_some() {
        return Err(
            ConfigError("MMORPG_PERSISTENCE_MODE requires the postgres feature".into()).into(),
        );
    }
    #[cfg(feature = "postgres")]
    if let Some(database_url) = read_optional_string("MMORPG_HANDOFF_DATABASE_URL")? {
        if character_directory.is_some() || recovery_directory.is_some() {
            return Err(ConfigError(
                "handoff hosts keep characters in PostgreSQL, not file directories".into(),
            )
            .into());
        }
        eprintln!("Trusted-LAN handoff host: unauthenticated character claims and transfer link.");
        let config = mmorpg_game_server::handoff_host::HandoffHostConfig {
            database_url,
            host_id: mmorpg_control_plane::HostId::new(read_string("MMORPG_HOST_ID", "")?)
                .map_err(|_| ConfigError("MMORPG_HOST_ID must name this host".into()))?,
            zone_ids,
            link_address: read_string("MMORPG_TRANSFER_LINK", DEFAULT_TRANSFER_LINK)?,
            routes: parse_zone_routes(&read_string("MMORPG_ZONE_ROUTES", "")?)?,
            reconnect_grace_ticks,
            grant_ttl: Duration::from_millis(read_number(
                "MMORPG_GRANT_TTL_MS",
                DEFAULT_GRANT_TTL_MS,
            )?),
            timeout: Duration::from_millis(read_number(
                "MMORPG_HANDOFF_TIMEOUT_MS",
                DEFAULT_HANDOFF_TIMEOUT_MS,
            )?),
            depths_grace: Duration::from_millis(read_number(
                "MMORPG_DEPTHS_GRACE_MS",
                mmorpg_control_plane::instance::DEFAULT_RUN_GRACE_MS,
            )?),
            confirmed_stopped: read_string("MMORPG_CONFIRM_SOURCE_STOPPED", "")? == "1",
        };
        mmorpg_game_server::handoff_host::serve_handoff_character_host(
            config,
            transport_config,
            status_config,
            shutdown_receiver,
        )
        .await
        .map_err(|cause| ConfigError(cause.to_string()))?;
        return Ok(());
    }
    #[cfg(not(feature = "postgres"))]
    if env::var_os("MMORPG_HANDOFF_DATABASE_URL").is_some() {
        return Err(ConfigError(
            "MMORPG_HANDOFF_DATABASE_URL requires the postgres feature".into(),
        )
        .into());
    }
    if let Some(directory) = character_directory {
        eprintln!("Development/LAN character claims enabled (unauthenticated).");
        serve_development_character_host(
            DevelopmentCharacterHostConfig {
                directory,
                recovery_directory,
                zone_ids,
                reconnect_grace_ticks,
            },
            transport_config,
            status_config,
            shutdown_receiver,
        )
        .await
        .map_err(|error| ConfigError(error.to_string()))?;
    } else {
        serve_open_zone_host(
            zone_ids,
            reconnect_grace_ticks,
            recovery_directory,
            transport_config,
            status_config,
            shutdown_receiver,
        )
        .await
        .map_err(|error| ConfigError(error.to_string()))?;
    }
    Ok(())
}

/// Local hosting config is deliberately isolated from developer environment overrides.
async fn serve_local_world(
    directory: PathBuf,
    owner: &mut local_world::LocalWorldHost,
) -> Result<(), Box<dyn Error>> {
    let (shutdown_sender, shutdown_receiver) = mpsc::channel(4);
    install_shutdown_forwarder(shutdown_sender.clone())?;
    owner
        .serve_control(shutdown_sender)
        .map_err(|error| ConfigError(error.to_string()))?;
    mmorpg_game_server::serve_managed_character_host(
        DevelopmentCharacterHostConfig {
            directory: directory.join("characters"),
            recovery_directory: Some(directory.join("recovery")),
            zone_ids: vec![ZoneId::new(1)],
            reconnect_grace_ticks: DEFAULT_RECONNECT_GRACE_TICKS,
        },
        MatchHostWebTransportConfig {
            bind_ip: Some(Ipv4Addr::LOCALHOST.into()),
            port: owner.world.port,
            certificate_pem: owner.world.certificate.clone(),
            private_key_pem: owner.private_key.clone(),
            route_prefix: BrowserRoutePrefix::new(DEFAULT_ROUTE_PREFIX)?,
            allowed_origins: None,
            drain_grace: Duration::from_millis(DEFAULT_DRAIN_GRACE_MS),
        },
        MatchHostStatusConfig {
            port: owner.world.status_port,
        },
        shutdown_receiver,
        owner.managed.clone(),
    )
    .await
    .map_err(|error| -> Box<dyn Error> {
        // Kept typed so main can exit for a restart on fresh ports.
        if local_world::is_port_conflict(error.as_ref()) {
            error
        } else {
            Box::new(ConfigError(error.to_string()))
        }
    })?;
    // `owner` retains the lock until the server has flushed its final saves.
    Ok(())
}

fn parse_zone_ids(value: &str) -> Result<Vec<ZoneId>, ConfigError> {
    if value.is_empty() {
        return Err(ConfigError("MMORPG_ZONE_IDS must not be empty".to_owned()));
    }

    value
        .split(',')
        .map(|raw| {
            raw.parse::<u32>().map(ZoneId::new).map_err(|error| {
                ConfigError(format!(
                    "MMORPG_ZONE_IDS contains invalid zone id {raw:?}: {error}"
                ))
            })
        })
        .collect()
}

/// `zone=link@session,…`: each peer zone's transfer link (`host:port`) and
/// the session route its clients connect to.
#[cfg_attr(not(feature = "postgres"), allow(dead_code))]
fn parse_zone_routes(
    value: &str,
) -> Result<
    std::collections::BTreeMap<ZoneId, mmorpg_game_server::handoff_host::ZoneRoute>,
    ConfigError,
> {
    value
        .split(',')
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            let invalid = || {
                ConfigError(format!(
                    "MMORPG_ZONE_ROUTES entry {entry:?} is not zone=host:port@route"
                ))
            };
            let (zone, route) = entry.split_once('=').ok_or_else(invalid)?;
            let (link, session) = route.split_once('@').ok_or_else(invalid)?;
            let zone = zone.parse::<u32>().map_err(|_| invalid())?;
            if link.is_empty() || session.is_empty() {
                return Err(invalid());
            }
            Ok((
                ZoneId::new(zone),
                mmorpg_game_server::handoff_host::ZoneRoute {
                    link: link.to_owned(),
                    session: session.to_owned(),
                },
            ))
        })
        .collect()
}

/// The address the host serves on: loopback unless `MMORPG_BIND_IP` names one (#1141).
fn parse_bind_ip(value: Option<&str>) -> Result<IpAddr, ConfigError> {
    match value {
        None => Ok(Ipv4Addr::LOCALHOST.into()),
        Some(value) => value.parse().map_err(|error| {
            ConfigError(format!(
                "MMORPG_BIND_IP {value:?} is not an IP address: {error}"
            ))
        }),
    }
}

/// PostgreSQL persistence creates durable characters from unauthenticated claims, so it
/// serves beyond loopback only with `MMORPG_ALLOW_UNAUTHENTICATED_CLAIMS=1` (#1141).
fn require_claim_acknowledgement(bind_ip: IpAddr, acknowledged: bool) -> Result<(), ConfigError> {
    if bind_ip.is_loopback() || acknowledged {
        Ok(())
    } else {
        Err(ConfigError(format!(
            "PostgreSQL persistence on {bind_ip} would accept unauthenticated character claims from other machines; set MMORPG_ALLOW_UNAUTHENTICATED_CLAIMS=1 to accept that, or bind loopback"
        )))
    }
}

fn read_optional_string(name: &str) -> Result<Option<String>, ConfigError> {
    match env::var(name) {
        Ok(value) if value.is_empty() => Err(ConfigError(format!("{name} must not be empty"))),
        Ok(value) => Ok(Some(value)),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(error) => Err(ConfigError(format!("{name} is invalid: {error}"))),
    }
}

fn read_string(name: &str, default: &str) -> Result<String, ConfigError> {
    match env::var(name) {
        Ok(value) => Ok(value),
        Err(env::VarError::NotPresent) => Ok(default.to_owned()),
        Err(error) => Err(ConfigError(format!("{name} is invalid: {error}"))),
    }
}

fn read_optional_path(name: &str) -> Result<Option<PathBuf>, ConfigError> {
    match env::var(name) {
        Ok(value) if value.is_empty() => Err(ConfigError(format!("{name} must not be empty"))),
        Ok(value) => Ok(Some(PathBuf::from(value))),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(error) => Err(ConfigError(format!("{name} is invalid: {error}"))),
    }
}

fn read_number<T>(name: &str, default: T) -> Result<T, ConfigError>
where
    T: FromStr,
    T::Err: fmt::Display,
{
    match env::var(name) {
        Ok(value) => value
            .parse::<T>()
            .map_err(|error| ConfigError(format!("{name} is invalid: {error}"))),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(ConfigError(format!("{name} is invalid: {error}"))),
    }
}

#[cfg(unix)]
fn install_shutdown_forwarder(sender: mpsc::Sender<()>) -> Result<(), Box<dyn Error>> {
    use tokio::signal::unix::{SignalKind, signal};

    let mut terminate = signal(SignalKind::terminate())?;
    tokio::spawn(async move {
        loop {
            let signal_received = tokio::select! {
                result = tokio::signal::ctrl_c() => result.is_ok(),
                received = terminate.recv() => received.is_some(),
            };
            if !signal_received || sender.send(()).await.is_err() {
                break;
            }
        }
    });
    Ok(())
}

#[cfg(not(unix))]
fn install_shutdown_forwarder(sender: mpsc::Sender<()>) -> Result<(), Box<dyn Error>> {
    tokio::spawn(async move {
        loop {
            if tokio::signal::ctrl_c().await.is_err() || sender.send(()).await.is_err() {
                break;
            }
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ordered_zone_ids() {
        assert_eq!(
            parse_zone_ids("7,11,42").unwrap(),
            vec![ZoneId::new(7), ZoneId::new(11), ZoneId::new(42)]
        );
    }

    #[test]
    fn rejects_empty_and_invalid_zone_ids() {
        assert_eq!(
            parse_zone_ids("").unwrap_err(),
            ConfigError("MMORPG_ZONE_IDS must not be empty".to_owned())
        );
        assert!(parse_zone_ids("7,nope").is_err());
        assert!(parse_zone_ids("7,").is_err());
    }

    #[test]
    fn binds_loopback_unless_an_address_is_configured() {
        assert_eq!(
            parse_bind_ip(None).unwrap(),
            IpAddr::from(Ipv4Addr::LOCALHOST)
        );
        assert_eq!(
            parse_bind_ip(Some("192.168.1.20")).unwrap(),
            IpAddr::from(Ipv4Addr::new(192, 168, 1, 20))
        );
        assert!(parse_bind_ip(Some("::1")).unwrap().is_loopback());
        assert!(parse_bind_ip(Some("lan")).is_err());
    }

    #[test]
    fn persistence_beyond_loopback_needs_the_acknowledgement() {
        let loopback = IpAddr::from(Ipv4Addr::LOCALHOST);
        let lan = IpAddr::from(Ipv4Addr::new(192, 168, 1, 20));
        let everywhere = IpAddr::from(Ipv4Addr::UNSPECIFIED);
        assert!(require_claim_acknowledgement(loopback, false).is_ok());
        assert!(require_claim_acknowledgement(lan, false).is_err());
        assert!(require_claim_acknowledgement(everywhere, false).is_err());
        assert!(require_claim_acknowledgement(lan, true).is_ok());
    }

    #[test]
    fn parses_peer_zone_routes() {
        let routes =
            parse_zone_routes("902=10.0.0.2:4500@https://10.0.0.2:4433/game/matches/zone-902")
                .unwrap();
        let route = &routes[&ZoneId::new(902)];
        assert_eq!(route.link, "10.0.0.2:4500");
        assert_eq!(route.session, "https://10.0.0.2:4433/game/matches/zone-902");
        assert!(parse_zone_routes("").unwrap().is_empty());
        assert!(parse_zone_routes("902=10.0.0.2:4500").is_err());
        assert!(parse_zone_routes("x=a@b").is_err());
    }
}
