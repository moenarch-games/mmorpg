//! PostgreSQL connections for the native hosts (#1142): the one place that
//! reads a connection string's `sslmode` and builds the matching connector.
//!
//! A connection string is a libpq URL (`postgres://…?sslmode=verify-full`) or
//! key/value string (`host=… sslmode=require`). This crate takes `sslmode` and
//! `sslrootcert` out of it, since the driver parses neither libpq's verifying
//! modes nor root certificates, and hands the rest to [`postgres::Config`]:
//!
//! | `sslmode` | Connection |
//! | --- | --- |
//! | unset, `disable` | plain, for local development |
//! | `allow`, `prefer` | TLS when the server offers it, plain otherwise |
//! | `require`, `verify-ca`, `verify-full` | TLS only |
//!
//! Every TLS connection verifies the server's certificate chain and host name
//! against the system's trusted authorities plus the PEM certificates
//! `sslrootcert` names (`sslrootcert=system` names none), stricter than libpq,
//! which verifies nothing under `require`. The TLS stack is rustls over ring,
//! so no OpenSSL system library is needed.
#![forbid(unsafe_code)]

use std::{
    error::Error,
    fmt,
    sync::{Arc, OnceLock},
};

use postgres::{CancelToken, Client, Config, GenericClient, NoTls, config::SslMode};
use rustls::{
    ClientConfig, RootCertStore,
    pki_types::{CertificateDer, pem::PemObject},
};
use tokio_postgres_rustls::MakeRustlsConnect;

/// What a connection string's `sslmode` asks of TLS.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TlsMode {
    /// Plain connections: `sslmode` unset or `disable`.
    Disabled,
    /// TLS when the server offers it: `allow` or `prefer`.
    Preferred,
    /// TLS or no connection: `require`, `verify-ca` or `verify-full`.
    Required,
}

impl TlsMode {
    fn parse(value: Option<&str>) -> Result<Self, ConnectError> {
        match value {
            None | Some("disable") => Ok(Self::Disabled),
            Some("allow" | "prefer") => Ok(Self::Preferred),
            Some("require" | "verify-ca" | "verify-full") => Ok(Self::Required),
            Some(other) => Err(ConnectError::Config(format!(
                "sslmode `{other}` must be disable, allow, prefer, require, verify-ca or verify-full"
            ))),
        }
    }
}

/// A parsed connection string: the driver configuration and its connector.
pub struct Target {
    config: Config,
    tls: Tls,
    mode: TlsMode,
}

impl Target {
    /// Parses `url`, building the TLS connector when `sslmode` asks for one.
    pub fn parse(url: &str) -> Result<Self, ConnectError> {
        let options = TlsOptions::split(url)?;
        let mode = TlsMode::parse(options.mode.as_deref())?;
        let mut config: Config = options
            .rest
            .parse()
            .map_err(|error| ConnectError::Config(format!("invalid connection string: {error}")))?;
        let tls = match mode {
            TlsMode::Disabled => {
                config.ssl_mode(SslMode::Disable);
                Tls(None)
            }
            TlsMode::Preferred => {
                config.ssl_mode(SslMode::Prefer);
                Tls(Some(connector(options.root_cert.as_deref())?))
            }
            TlsMode::Required => {
                config.ssl_mode(SslMode::Require);
                Tls(Some(connector(options.root_cert.as_deref())?))
            }
        };
        Ok(Self { config, tls, mode })
    }

    /// The driver configuration, for timeouts and other connection options.
    pub fn config_mut(&mut self) -> &mut Config {
        &mut self.config
    }

    #[must_use]
    pub fn tls_mode(&self) -> TlsMode {
        self.mode
    }

    /// Opens one connection, with a [`Canceller`] that reaches it through the same connector.
    pub fn connect(&self) -> Result<Connection, ConnectError> {
        let client = match &self.tls.0 {
            None => self.config.connect(NoTls),
            Some(connector) => self.config.connect(connector.clone()),
        }?;
        let canceller = Canceller {
            token: client.cancel_token(),
            tls: self.tls.clone(),
        };
        Ok(Connection { client, canceller })
    }
}

/// One-off connection with no further options, such as a migration or test setup.
pub fn connect(url: &str) -> Result<Client, ConnectError> {
    Ok(Target::parse(url)?.connect()?.client)
}

/// An open connection and the way to cancel its running statement.
pub struct Connection {
    pub client: Client,
    pub canceller: Canceller,
}

/// Cancels a connection's running statement from any thread. The cancel
/// request opens its own connection, so it needs the same TLS as the original.
#[derive(Clone)]
pub struct Canceller {
    token: CancelToken,
    tls: Tls,
}

impl Canceller {
    pub fn cancel(&self) -> Result<(), ConnectError> {
        match &self.tls.0 {
            None => self.token.cancel_query(NoTls),
            Some(connector) => self.token.cancel_query(connector.clone()),
        }
        .map_err(ConnectError::from)
    }
}

/// The connector of one target: `None` connects in plain text.
#[derive(Clone)]
struct Tls(Option<MakeRustlsConnect>);

/// How an operator applies the schema migrations, with a login allowed to change schemas.
pub const MIGRATION_STEP: &str =
    "MMORPG_PERSISTENCE_MODE=migrate with MMORPG_MIGRATION_DATABASE_URL";

/// Refuses a database whose `schema` is not at exactly `needed`, the version
/// this build reads and writes. A serving host calls this instead of
/// migrating, so its login needs no rights to change schemas (#1142).
pub fn require_schema(
    client: &mut impl GenericClient,
    schema: &'static str,
    needed: i32,
) -> Result<(), ConnectError> {
    let table = format!("{schema}.migrations");
    let exists: bool = client
        .query_one("SELECT to_regclass($1) IS NOT NULL", &[&table])?
        .get(0);
    let found: Option<i32> = if exists {
        client
            .query_one(&format!("SELECT max(version) FROM {table}"), &[])?
            .get(0)
    } else {
        None
    };
    if found == Some(needed) {
        return Ok(());
    }
    Err(ConnectError::Schema {
        schema,
        found,
        needed,
    })
}

#[derive(Debug)]
pub enum ConnectError {
    /// The connection string or its TLS settings are unusable.
    Config(String),
    /// Connecting, querying or cancelling failed.
    Postgres(postgres::Error),
    /// The database's schema is not the version this build needs.
    Schema {
        schema: &'static str,
        found: Option<i32>,
        needed: i32,
    },
}

impl fmt::Display for ConnectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(message) => f.write_str(message),
            Self::Schema {
                schema,
                found: Some(found),
                needed,
            } if found > needed => write!(
                f,
                "schema {schema} is at version {found}, newer than this build's {needed}; \
                 run a build that matches the database"
            ),
            Self::Schema {
                schema,
                found,
                needed,
            } => write!(
                f,
                "schema {schema} is at version {}, this build needs {needed}; \
                 apply the migrations first ({MIGRATION_STEP}, a login allowed to change schemas)",
                found.unwrap_or(0)
            ),
            Self::Postgres(error) => match error.as_db_error() {
                Some(db) => write!(f, "{error}: {}", db.message()),
                None => match error.source() {
                    Some(cause) => write!(f, "{error}: {cause}"),
                    None => write!(f, "{error}"),
                },
            },
        }
    }
}

impl Error for ConnectError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Config(_) | Self::Schema { .. } => None,
            Self::Postgres(error) => Some(error),
        }
    }
}

impl From<postgres::Error> for ConnectError {
    fn from(error: postgres::Error) -> Self {
        Self::Postgres(error)
    }
}

/// The connection string without its TLS options, and those options.
#[derive(Debug, Default, Eq, PartialEq)]
struct TlsOptions {
    rest: String,
    mode: Option<String>,
    root_cert: Option<String>,
}

impl TlsOptions {
    fn split(url: &str) -> Result<Self, ConnectError> {
        if url.starts_with("postgres://") || url.starts_with("postgresql://") {
            Self::split_url(url)
        } else {
            Self::split_key_values(url)
        }
    }

    fn take(&mut self, key: &str, value: String) -> bool {
        match key {
            "sslmode" => self.mode = Some(value),
            "sslrootcert" => self.root_cert = Some(value),
            _ => return false,
        }
        true
    }

    fn split_url(url: &str) -> Result<Self, ConnectError> {
        let mut options = Self::default();
        let Some((base, query)) = url.split_once('?') else {
            options.rest = url.to_owned();
            return Ok(options);
        };
        let mut kept = Vec::new();
        for pair in query.split('&').filter(|pair| !pair.is_empty()) {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            if !options.take(&percent_decode(key)?, percent_decode(value)?) {
                kept.push(pair);
            }
        }
        options.rest = if kept.is_empty() {
            base.to_owned()
        } else {
            format!("{base}?{}", kept.join("&"))
        };
        Ok(options)
    }

    /// libpq's `key = value` form; a value may be single-quoted, and a
    /// backslash escapes the next character in either form.
    fn split_key_values(text: &str) -> Result<Self, ConnectError> {
        let mut options = Self::default();
        let mut kept = Vec::new();
        let mut chars = text.char_indices().peekable();
        let skip_space = |chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>| {
            while chars.next_if(|(_, c)| c.is_whitespace()).is_some() {}
        };
        loop {
            skip_space(&mut chars);
            let Some(&(start, _)) = chars.peek() else {
                break;
            };
            let mut key = String::new();
            while let Some((_, c)) = chars.next_if(|(_, c)| *c != '=' && !c.is_whitespace()) {
                key.push(c);
            }
            skip_space(&mut chars);
            if chars.next_if(|(_, c)| *c == '=').is_none() {
                return Err(ConnectError::Config(format!(
                    "invalid connection string: `{key}` has no value"
                )));
            }
            skip_space(&mut chars);
            let mut value = String::new();
            if chars.next_if(|(_, c)| *c == '\'').is_some() {
                loop {
                    match chars.next() {
                        Some((_, '\'')) => break,
                        Some((_, '\\')) => value.extend(chars.next().map(|(_, c)| c)),
                        Some((_, c)) => value.push(c),
                        None => {
                            return Err(ConnectError::Config(format!(
                                "invalid connection string: `{key}` has an unterminated quote"
                            )));
                        }
                    }
                }
            } else {
                while let Some((_, c)) = chars.next_if(|(_, c)| !c.is_whitespace()) {
                    if c == '\\' {
                        value.extend(chars.next().map(|(_, c)| c));
                    } else {
                        value.push(c);
                    }
                }
            }
            let end = chars.peek().map_or(text.len(), |&(index, _)| index);
            if !options.take(&key, value) {
                kept.push(&text[start..end]);
            }
        }
        options.rest = kept.join(" ");
        Ok(options)
    }
}

fn percent_decode(text: &str) -> Result<String, ConnectError> {
    let invalid = || ConnectError::Config(format!("invalid percent-encoding in `{text}`"));
    let mut bytes = Vec::with_capacity(text.len());
    let mut rest = text.as_bytes();
    while let Some((&byte, tail)) = rest.split_first() {
        if byte == b'%' {
            let hex = tail.get(..2).ok_or_else(invalid)?;
            let hex = std::str::from_utf8(hex).map_err(|_| invalid())?;
            bytes.push(u8::from_str_radix(hex, 16).map_err(|_| invalid())?);
            rest = &tail[2..];
        } else {
            bytes.push(byte);
            rest = tail;
        }
    }
    String::from_utf8(bytes).map_err(|_| invalid())
}

/// The system's trusted authorities, read once per process.
fn system_roots() -> &'static [CertificateDer<'static>] {
    static ROOTS: OnceLock<Vec<CertificateDer<'static>>> = OnceLock::new();
    ROOTS.get_or_init(|| rustls_native_certs::load_native_certs().certs)
}

fn connector(root_cert: Option<&str>) -> Result<MakeRustlsConnect, ConnectError> {
    let mut roots = RootCertStore::empty();
    roots.add_parsable_certificates(system_roots().iter().cloned());
    if let Some(path) = root_cert.filter(|path| *path != "system") {
        let unreadable =
            |error: &dyn fmt::Display| ConnectError::Config(format!("sslrootcert {path}: {error}"));
        let mut added = 0_usize;
        for certificate in CertificateDer::pem_file_iter(path).map_err(|e| unreadable(&e))? {
            roots
                .add(certificate.map_err(|e| unreadable(&e))?)
                .map_err(|e| unreadable(&e))?;
            added += 1;
        }
        if added == 0 {
            return Err(unreadable(&"holds no PEM certificate"));
        }
    }
    if roots.is_empty() {
        return Err(ConnectError::Config(
            "TLS needs a trusted certificate authority: the system store has none, \
             so name one with sslrootcert"
                .into(),
        ));
    }
    let config =
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .map_err(|error| ConnectError::Config(format!("TLS configuration: {error}")))?
            .with_root_certificates(roots)
            .with_no_client_auth();
    Ok(MakeRustlsConnect::new(config))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CA: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/ca.pem");

    fn split(url: &str) -> TlsOptions {
        TlsOptions::split(url).unwrap()
    }

    #[test]
    fn a_url_loses_only_its_tls_options() {
        assert_eq!(
            split(
                "postgres://user:pw@db:5432/app?sslmode=verify-full&connect_timeout=3&sslrootcert=%2Fetc%2Fca%20dir%2Fca.pem"
            ),
            TlsOptions {
                rest: "postgres://user:pw@db:5432/app?connect_timeout=3".into(),
                mode: Some("verify-full".into()),
                root_cert: Some("/etc/ca dir/ca.pem".into()),
            }
        );
        assert_eq!(
            split("postgresql://db/app?sslmode=require").rest,
            "postgresql://db/app"
        );
        assert_eq!(
            split("postgres://db/app"),
            TlsOptions {
                rest: "postgres://db/app".into(),
                ..TlsOptions::default()
            }
        );
    }

    #[test]
    fn a_key_value_string_loses_only_its_tls_options() {
        assert_eq!(
            split(
                "host=db  sslmode = require user='app user' sslrootcert='/etc/it\\'s/ca.pem' dbname=app"
            ),
            TlsOptions {
                rest: "host=db user='app user' dbname=app".into(),
                mode: Some("require".into()),
                root_cert: Some("/etc/it's/ca.pem".into()),
            }
        );
        assert!(TlsOptions::split("host=db sslmode").is_err());
        assert!(TlsOptions::split("host='db").is_err());
    }

    #[test]
    fn sslmode_selects_the_connector() {
        for (url, mode, ssl) in [
            ("postgres://db/app", TlsMode::Disabled, SslMode::Disable),
            (
                "host=db sslmode=disable",
                TlsMode::Disabled,
                SslMode::Disable,
            ),
            (
                "postgres://db/app?sslmode=prefer",
                TlsMode::Preferred,
                SslMode::Prefer,
            ),
            (
                "postgres://db/app?sslmode=allow",
                TlsMode::Preferred,
                SslMode::Prefer,
            ),
            (
                "postgres://db/app?sslmode=require",
                TlsMode::Required,
                SslMode::Require,
            ),
            (
                "postgres://db/app?sslmode=verify-ca",
                TlsMode::Required,
                SslMode::Require,
            ),
            (
                "postgres://db/app?sslmode=verify-full",
                TlsMode::Required,
                SslMode::Require,
            ),
        ] {
            let url = if mode == TlsMode::Disabled {
                url.to_owned()
            } else {
                // A named authority, so the test does not depend on the system store.
                format!("{url}&sslrootcert={CA}").replace(' ', "%20")
            };
            let target = Target::parse(&url).unwrap();
            assert_eq!(target.tls_mode(), mode, "{url}");
            assert_eq!(target.config.get_ssl_mode(), ssl, "{url}");
            assert_eq!(target.tls.0.is_some(), mode != TlsMode::Disabled, "{url}");
        }
    }

    #[test]
    fn unusable_tls_settings_are_refused() {
        let error = Target::parse("postgres://db/app?sslmode=verify")
            .err()
            .unwrap();
        assert!(error.to_string().contains("sslmode `verify`"), "{error}");
        let error = Target::parse(&format!(
            "host=db sslmode=verify-full sslrootcert='{CA}.missing'"
        ))
        .err()
        .unwrap();
        assert!(error.to_string().starts_with("sslrootcert "), "{error}");
        let error = Target::parse(&format!(
            "host=db sslmode=require sslrootcert='{}'",
            concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")
        ))
        .err()
        .unwrap();
        assert!(error.to_string().contains("no PEM certificate"), "{error}");
    }
}
