//! Connection imports are native staging objects. Credential material never
//! appears in a serializable preview or in an error message.
use crate::types::DbError;
use cc_models::database::{
    DbAuthMechanism, DbConnection, DbEngine, DbOptions, MongoReadPreference,
};
use percent_encoding::percent_decode_str;
use secrecy::{ExposeSecret, SecretString};
use std::collections::BTreeSet;
use url::Url;
use zeroize::Zeroizing;

const MAX_IMPORT_BYTES: usize = 1024 * 1024;
const MAX_ENTRIES: usize = 1000;

pub struct ImportedConnection {
    connection: DbConnection,
    password: Option<SecretString>,
}

impl ImportedConnection {
    pub fn connection(&self) -> &DbConnection {
        &self.connection
    }

    pub fn password(&self) -> Option<&SecretString> {
        self.password.as_ref()
    }

    /// Native Vault code persists the secret and fills its reference only
    /// after explicit user confirmation of the staged import.
    pub fn into_parts(self) -> (DbConnection, Option<SecretString>) {
        (self.connection, self.password)
    }
}

pub struct PgpassImport {
    pub entries: Vec<ImportedConnection>,
    pub skipped_wildcards: usize,
    pub skipped_local_sockets: usize,
}

pub fn parse_uri(input: &SecretString) -> Result<ImportedConnection, DbError> {
    let input = input.expose_secret().trim();
    if input.is_empty() || input.len() > MAX_IMPORT_BYTES || input.chars().any(char::is_control) {
        return Err(DbError::InvalidRequest);
    }
    let (scheme, remaining) = input.split_once("://").ok_or(DbError::InvalidRequest)?;
    let (engine, srv) = match scheme.to_ascii_lowercase().as_str() {
        "postgres" | "postgresql" => (DbEngine::Postgresql, false),
        "mysql" => (DbEngine::Mysql, false),
        "mariadb" => (DbEngine::Mariadb, false),
        "clickhouse" | "clickhouses" => (DbEngine::Clickhouse, false),
        "redis" | "rediss" => (DbEngine::Redis, false),
        "mongodb" => (DbEngine::Mongodb, false),
        "mongodb+srv" => (DbEngine::Mongodb, true),
        _ => return Err(DbError::InvalidRequest),
    };
    let authority_end = remaining.find(['/', '?', '#']).unwrap_or(remaining.len());
    let (authority, suffix) = remaining.split_at(authority_end);
    let (user_info, address) = match authority.rsplit_once('@') {
        Some((user, host)) => (Some(user), host),
        None => (None, authority),
    };
    // Never pass a password-bearing URL into a driver or into URL diagnostics.
    // Multiple seeds need a separately reviewed topology, not silent truncation.
    if address.is_empty() || address.contains([',', '%', '@']) || address.contains('\\') {
        return Err(DbError::InvalidRequest);
    }
    let (username, password) = if let Some(user) = user_info {
        let (name, password) = match user.split_once(':') {
            Some((name, password)) => (name, Some(password)),
            None => (user, None),
        };
        (
            decoded(name)?,
            password.map(decoded).transpose()?.map(SecretString::from),
        )
    } else {
        (String::new(), None)
    };
    let sanitized = Zeroizing::new(format!("db://{address}{suffix}"));
    let parsed = Url::parse(&sanitized).map_err(|_| DbError::InvalidRequest)?;
    if parsed.fragment().is_some() || parsed.password().is_some() || !parsed.username().is_empty() {
        return Err(DbError::InvalidRequest);
    }
    let host = match parsed.host().ok_or(DbError::InvalidRequest)? {
        url::Host::Domain(host) => host.to_owned(),
        url::Host::Ipv4(host) => host.to_string(),
        url::Host::Ipv6(host) => host.to_string(),
    };
    if host.is_empty() || host.len() > 253 || (srv && parsed.port().is_some()) {
        return Err(DbError::InvalidRequest);
    }
    if srv && host.parse::<std::net::IpAddr>().is_ok() {
        return Err(DbError::InvalidRequest);
    }
    let database = decoded(parsed.path().strip_prefix('/').unwrap_or(parsed.path()))?;
    if database.contains('/') || database.len() > 256 {
        return Err(DbError::InvalidRequest);
    }
    let mut connection = DbConnection::new(host.clone(), host, engine);
    connection.username = username;
    connection.port = parsed.port().unwrap_or_else(|| engine.default_port(true));
    if !database.is_empty() {
        connection.database = database;
    }
    if let DbOptions::Mongodb { srv: value, .. } = &mut connection.options {
        *value = srv;
    }
    let mut seen = BTreeSet::new();
    for (key, value) in parsed.query_pairs() {
        let key = key.to_ascii_lowercase();
        if !seen.insert(key.clone()) || value.chars().any(char::is_control) {
            return Err(DbError::InvalidRequest);
        }
        apply_option(&mut connection, &key, &value)?;
    }
    connection.validate().map_err(|_| DbError::InvalidRequest)?;
    Ok(ImportedConnection {
        connection,
        password,
    })
}

fn decoded(value: &str) -> Result<String, DbError> {
    // percent_decode_str accepts a stray percent literally, which is too
    // ambiguous for imported credentials and identifiers.
    let bytes = value.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'%'
            && !bytes
                .get(index + 1..index + 3)
                .is_some_and(|pair| pair.iter().all(u8::is_ascii_hexdigit))
        {
            return Err(DbError::InvalidRequest);
        }
    }
    let decoded = percent_decode_str(value)
        .decode_utf8()
        .map_err(|_| DbError::InvalidRequest)?
        .into_owned();
    if decoded.contains('\0') || decoded.len() > 65536 {
        return Err(DbError::InvalidRequest);
    }
    Ok(decoded)
}

fn apply_option(connection: &mut DbConnection, key: &str, value: &str) -> Result<(), DbError> {
    // Import never weakens verification or forwards opaque driver options.
    match key {
        "sslmode"
            if matches!(
                connection.engine,
                DbEngine::Postgresql | DbEngine::Mysql | DbEngine::Mariadb
            ) && matches!(
                value.to_ascii_lowercase().as_str(),
                "verify-full" | "verify_identity"
            ) =>
        {
            return Ok(())
        }
        "tls" | "ssl" if matches!(value, "true" | "1") => return Ok(()),
        "connect_timeout" | "connecttimeoutms" => {
            let value = value.parse::<u32>().map_err(|_| DbError::InvalidRequest)?;
            let seconds = if key.ends_with("ms") {
                if value % 1000 != 0 {
                    return Err(DbError::InvalidRequest);
                }
                value / 1000
            } else {
                value
            };
            if !(1..=60).contains(&seconds) {
                return Err(DbError::InvalidRequest);
            }
            connection.limits.connect_timeout_seconds = seconds;
            return Ok(());
        }
        _ => {}
    }
    match &mut connection.options {
        DbOptions::Postgresql {
            application_name, ..
        } if key == "application_name" => {
            if value.is_empty() || value.len() > 128 {
                return Err(DbError::InvalidRequest);
            }
            *application_name = value.into();
        }
        DbOptions::Mysql { charset } | DbOptions::Mariadb { charset } if key == "charset" => {
            if !matches!(value, "utf8mb4" | "utf8" | "ascii" | "latin1") {
                return Err(DbError::PolicyDenied);
            }
            *charset = value.into();
        }
        DbOptions::Redis { resp3 } if key == "protocol" => {
            *resp3 = match value {
                "2" => false,
                "3" => true,
                _ => return Err(DbError::InvalidRequest),
            };
        }
        DbOptions::Mongodb {
            auth_source,
            replica_set,
            read_preference,
            ..
        } => match key {
            "authsource" => *auth_source = Some(bounded_option(value)?),
            "replicaset" => *replica_set = Some(bounded_option(value)?),
            "authmechanism" => {
                connection.auth.mechanism = match value {
                    "SCRAM-SHA-256" => DbAuthMechanism::ScramSha256,
                    "SCRAM-SHA-1" => DbAuthMechanism::ScramSha1,
                    _ => return Err(DbError::PolicyDenied),
                }
            }
            "readpreference" => {
                *read_preference = match value {
                    "primary" => MongoReadPreference::Primary,
                    "primaryPreferred" => MongoReadPreference::PrimaryPreferred,
                    "secondary" => MongoReadPreference::Secondary,
                    "secondaryPreferred" => MongoReadPreference::SecondaryPreferred,
                    "nearest" => MongoReadPreference::Nearest,
                    _ => return Err(DbError::InvalidRequest),
                }
            }
            _ => return Err(DbError::PolicyDenied),
        },
        _ => return Err(DbError::PolicyDenied),
    }
    Ok(())
}

fn bounded_option(value: &str) -> Result<String, DbError> {
    if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        Err(DbError::InvalidRequest)
    } else {
        Ok(value.to_owned())
    }
}

pub fn parse_pgpass(input: &SecretString) -> Result<PgpassImport, DbError> {
    let input = input.expose_secret();
    if input.len() > MAX_IMPORT_BYTES || input.contains('\0') {
        return Err(DbError::InvalidRequest);
    }
    let mut result = PgpassImport {
        entries: Vec::new(),
        skipped_wildcards: 0,
        skipped_local_sockets: 0,
    };
    for line in input.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if result.entries.len() + result.skipped_wildcards + result.skipped_local_sockets
            >= MAX_ENTRIES
        {
            return Err(DbError::ResultLimit);
        }
        let fields = pgpass_fields(line)?;
        if fields[..4].iter().any(|field| field.as_str() == "*") {
            result.skipped_wildcards += 1;
            continue;
        }
        if fields[0].starts_with('/') {
            result.skipped_local_sockets += 1;
            continue;
        }
        let mut connection =
            DbConnection::new(fields[0].as_str(), fields[0].as_str(), DbEngine::Postgresql);
        connection.port = fields[1].parse().map_err(|_| DbError::InvalidRequest)?;
        connection.database = fields[2].to_string();
        connection.username = fields[3].to_string();
        connection.validate().map_err(|_| DbError::InvalidRequest)?;
        result.entries.push(ImportedConnection {
            connection,
            password: Some(SecretString::from(fields[4].to_string())),
        });
        if result.entries.len() + result.skipped_wildcards + result.skipped_local_sockets
            > MAX_ENTRIES
        {
            return Err(DbError::ResultLimit);
        }
    }
    Ok(result)
}

fn pgpass_fields(line: &str) -> Result<Vec<Zeroizing<String>>, DbError> {
    let mut fields = vec![Zeroizing::new(String::new())];
    let mut escaped = false;
    for character in line.chars() {
        if escaped {
            if !matches!(character, ':' | '\\') {
                return Err(DbError::InvalidRequest);
            }
            fields
                .last_mut()
                .ok_or(DbError::InvalidRequest)?
                .push(character);
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == ':' {
            fields.push(Zeroizing::new(String::new()));
            if fields.len() > 5 {
                return Err(DbError::InvalidRequest);
            }
        } else {
            fields
                .last_mut()
                .ok_or(DbError::InvalidRequest)?
                .push(character);
        }
    }
    if escaped || fields.len() != 5 || fields[..4].iter().any(|field| field.is_empty()) {
        Err(DbError::InvalidRequest)
    } else {
        Ok(fields)
    }
}
