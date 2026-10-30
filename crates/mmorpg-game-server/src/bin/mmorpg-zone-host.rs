use game_server::{
    BrowserRoutePrefix, DEFAULT_HOST_STATUS_PORT, MatchHostRecoveryConfig, MatchHostStatusConfig,
    MatchHostWebTransportConfig, RejectMatchControlService, prepare_match_host_for_recovery,
    serve_match_host_with_status_and_control_and_shutdown,
    serve_prepared_match_host_with_status_and_control_and_shutdown,
};
use mmorpg_core::{ZoneId, long_ticks_from_ms};
use mmorpg_game_server::{
    DevelopmentCharacterHostConfig, build_zone_host, build_zone_matches,
    serve_development_character_host,
};
use std::env;
use std::error::Error;
use std::fmt;
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
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

    let (shutdown_sender, shutdown_receiver) = mpsc::channel(4);
    install_shutdown_forwarder(shutdown_sender)?;
    let transport_config = MatchHostWebTransportConfig {
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
        eprintln!(
            "PostgreSQL persistence selected; character admission uses trusted development/LAN identities."
        );
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
    } else if let Some(directory) = recovery_directory {
        let matches = build_zone_matches(zone_ids)?;
        let max_matches = matches.len();
        let prepared = prepare_match_host_for_recovery(
            matches,
            max_matches,
            reconnect_grace_ticks,
            MatchHostRecoveryConfig { directory },
        )
        .await?;
        serve_prepared_match_host_with_status_and_control_and_shutdown(
            prepared,
            RejectMatchControlService,
            transport_config,
            status_config,
            shutdown_receiver,
        )
        .await?;
    } else {
        let host = build_zone_host(zone_ids, reconnect_grace_ticks)?;
        serve_match_host_with_status_and_control_and_shutdown(
            host,
            RejectMatchControlService,
            transport_config,
            status_config,
            shutdown_receiver,
        )
        .await?;
    }
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

#[cfg_attr(not(feature = "postgres"), allow(dead_code))]
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
