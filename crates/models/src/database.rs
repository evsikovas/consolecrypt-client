//! Database inventory contains connection metadata and Secret references only.
//! Session permissions, query results and provider consents are device-local.

use crate::{ObjectId, Timestamp, ValidationError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DbEngine {
    Postgresql,
    Mysql,
    Mariadb,
    Clickhouse,
    Redis,
    Mongodb,
}

impl DbEngine {
    pub const ALL: [Self; 6] = [
        Self::Postgresql,
        Self::Mysql,
        Self::Mariadb,
        Self::Clickhouse,
        Self::Redis,
        Self::Mongodb,
    ];

    pub const fn default_port(self, tls: bool) -> u16 {
        match self {
            Self::Postgresql => 5432,
            Self::Mysql | Self::Mariadb => 3306,
            Self::Clickhouse if tls => 8443,
            Self::Clickhouse => 8123,
            Self::Redis => 6379,
            Self::Mongodb => 27017,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DbTlsMode {
    Disable,
    Prefer,
    Require,
    #[default]
    Verify,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DbTls {
    #[serde(default)]
    pub mode: DbTlsMode,
    #[serde(default)]
    pub certificate_authority_pem: Option<String>,
    #[serde(default)]
    pub server_name: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DbRoute {
    #[default]
    Direct,
    Ssh {
        host_id: ObjectId,
    },
    Jump {
        jump_profile_id: ObjectId,
    },
    Proxy {
        proxy_id: ObjectId,
    },
}

impl DbRoute {
    pub fn reference(&self) -> Option<ObjectId> {
        match self {
            Self::Direct => None,
            Self::Ssh { host_id } => Some(*host_id),
            Self::Jump { jump_profile_id } => Some(*jump_profile_id),
            Self::Proxy { proxy_id } => Some(*proxy_id),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DbAuthMechanism {
    #[default]
    Default,
    ScramSha256,
    ScramSha1,
    MongoX509,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DbClientCertificate {
    pub certificate_pem: String,
    pub key_secret_id: ObjectId,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DbAuth {
    #[serde(default)]
    pub password_secret_id: Option<ObjectId>,
    #[serde(default)]
    pub client_certificate: Option<DbClientCertificate>,
    #[serde(default)]
    pub mechanism: DbAuthMechanism,
}

impl DbAuth {
    pub fn secret_ids(&self) -> Vec<ObjectId> {
        self.password_secret_id
            .iter()
            .copied()
            .chain(
                self.client_certificate
                    .iter()
                    .map(|cert| cert.key_secret_id),
            )
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DbLimits {
    pub max_rows: u32,
    pub statement_timeout_seconds: u32,
    pub connect_timeout_seconds: u32,
    pub result_memory_bytes: u32,
}

impl Default for DbLimits {
    fn default() -> Self {
        Self {
            max_rows: 1000,
            statement_timeout_seconds: 60,
            connect_timeout_seconds: 10,
            result_memory_bytes: 64 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MongoReadPreference {
    Primary,
    PrimaryPreferred,
    Secondary,
    #[default]
    SecondaryPreferred,
    Nearest,
}

/// Driver settings are typed instead of passing arbitrary URL parameters to a
/// driver. In particular, no option can enable file access or disable TLS checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DbOptions {
    Postgresql {
        #[serde(default = "application_name")]
        application_name: String,
        #[serde(default)]
        search_path: Vec<String>,
    },
    Mysql {
        #[serde(default = "charset")]
        charset: String,
    },
    Mariadb {
        #[serde(default = "charset")]
        charset: String,
    },
    Clickhouse {
        #[serde(default)]
        settings: BTreeMap<String, u64>,
    },
    Redis {
        #[serde(default)]
        resp3: bool,
    },
    Mongodb {
        #[serde(default)]
        auth_source: Option<String>,
        #[serde(default)]
        replica_set: Option<String>,
        #[serde(default)]
        srv: bool,
        #[serde(default)]
        read_preference: MongoReadPreference,
    },
}

impl DbOptions {
    pub fn for_engine(engine: DbEngine) -> Self {
        match engine {
            DbEngine::Postgresql => Self::Postgresql {
                application_name: application_name(),
                search_path: Vec::new(),
            },
            DbEngine::Mysql => Self::Mysql { charset: charset() },
            DbEngine::Mariadb => Self::Mariadb { charset: charset() },
            DbEngine::Clickhouse => Self::Clickhouse {
                settings: BTreeMap::new(),
            },
            DbEngine::Redis => Self::Redis { resp3: false },
            DbEngine::Mongodb => Self::Mongodb {
                auth_source: None,
                replica_set: None,
                srv: false,
                read_preference: MongoReadPreference::default(),
            },
        }
    }

    pub const fn engine(&self) -> DbEngine {
        match self {
            Self::Postgresql { .. } => DbEngine::Postgresql,
            Self::Mysql { .. } => DbEngine::Mysql,
            Self::Mariadb { .. } => DbEngine::Mariadb,
            Self::Clickhouse { .. } => DbEngine::Clickhouse,
            Self::Redis { .. } => DbEngine::Redis,
            Self::Mongodb { .. } => DbEngine::Mongodb,
        }
    }
}

fn application_name() -> String {
    "ConsoleCrypt".into()
}
fn charset() -> String {
    "utf8mb4".into()
}
fn production_default() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DbConnection {
    pub id: ObjectId,
    pub name: String,
    pub engine: DbEngine,
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub database: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub auth: DbAuth,
    #[serde(default)]
    pub tls: DbTls,
    #[serde(default)]
    pub route: DbRoute,
    #[serde(default = "production_default")]
    pub production: bool,
    #[serde(default)]
    pub limits: DbLimits,
    pub options: DbOptions,
    #[serde(default)]
    pub group_id: Option<ObjectId>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub color: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl DbConnection {
    pub fn new(name: impl Into<String>, host: impl Into<String>, engine: DbEngine) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: ObjectId::new(),
            name: name.into(),
            engine,
            host: host.into(),
            port: engine.default_port(true),
            database: if engine == DbEngine::Redis {
                "0".into()
            } else {
                String::new()
            },
            username: String::new(),
            auth: DbAuth::default(),
            tls: DbTls::default(),
            route: DbRoute::default(),
            production: true,
            limits: DbLimits::default(),
            options: DbOptions::for_engine(engine),
            group_id: None,
            tags: Vec::new(),
            color: None,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.id == ObjectId::NIL
            || self.route.reference() == Some(ObjectId::NIL)
            || self.group_id == Some(ObjectId::NIL)
            || self.auth.secret_ids().contains(&ObjectId::NIL)
        {
            return Err(invalid("reference", "invalid object reference"));
        }
        text(&self.name, 256, false, "name")?;
        host(&self.host, "host")?;
        if self.port == 0 {
            return Err(invalid("port", "must be positive"));
        }
        text(&self.database, 1024, true, "database")?;
        text(&self.username, 1024, true, "username")?;
        if self.engine == DbEngine::Redis && self.database.parse::<u16>().is_err() {
            return Err(invalid(
                "database",
                "Redis requires a numeric database index",
            ));
        }
        if let Some(name) = &self.tls.server_name {
            host(name, "tls_server_name")?;
        }
        if let Some(ca) = &self.tls.certificate_authority_pem {
            crate::kubernetes::validate_certificates(ca)?;
        }
        if let Some(cert) = &self.auth.client_certificate {
            crate::kubernetes::validate_certificates(&cert.certificate_pem)?;
        }
        if self.auth.mechanism != DbAuthMechanism::Default && self.engine != DbEngine::Mongodb {
            return Err(invalid("auth", "mechanism requires MongoDB"));
        }
        if self.auth.mechanism == DbAuthMechanism::MongoX509
            && (self.auth.client_certificate.is_none()
                || self.auth.password_secret_id.is_some()
                || self.tls.mode == DbTlsMode::Disable)
        {
            return Err(invalid(
                "auth",
                "X.509 requires a TLS client certificate without a password",
            ));
        }
        let limits = &self.limits;
        if limits.max_rows == 0
            || limits.max_rows > 1_000_000
            || limits.statement_timeout_seconds == 0
            || limits.statement_timeout_seconds > if self.production { 300 } else { 86_400 }
            || !(1..=120).contains(&limits.connect_timeout_seconds)
            || !(1024 * 1024..=256 * 1024 * 1024).contains(&limits.result_memory_bytes)
        {
            return Err(invalid("limits", "limits are outside supported bounds"));
        }
        if self.options.engine() != self.engine {
            return Err(invalid("options", "driver options do not match the engine"));
        }
        match &self.options {
            DbOptions::Postgresql {
                application_name,
                search_path,
            } => {
                text(application_name, 256, false, "application_name")?;
                if search_path.len() > 16 {
                    return Err(invalid("search_path", "too many schemas"));
                }
                for schema in search_path {
                    text(schema, 256, false, "search_path")?;
                }
            }
            DbOptions::Mysql { charset } | DbOptions::Mariadb { charset } => {
                if !matches!(charset.as_str(), "utf8mb4" | "utf8" | "latin1" | "ascii") {
                    return Err(invalid("charset", "unsupported character set"));
                }
            }
            DbOptions::Clickhouse { settings } => {
                for (key, value) in settings {
                    let maximum = match key.as_str() {
                        "max_threads" => 256,
                        "max_memory_usage" => 64 * 1024 * 1024 * 1024,
                        "max_bytes_to_read" => 1024 * 1024 * 1024 * 1024,
                        "max_rows_to_read" => 1_000_000_000,
                        _ => return Err(invalid("options", "unsupported ClickHouse setting")),
                    };
                    if *value == 0 || *value > maximum {
                        return Err(invalid(
                            "options",
                            "ClickHouse setting is outside supported bounds",
                        ));
                    }
                }
            }
            DbOptions::Redis { .. } => {}
            DbOptions::Mongodb {
                auth_source,
                replica_set,
                srv,
                ..
            } => {
                for value in [auth_source, replica_set].into_iter().flatten() {
                    text(value, 256, false, "mongo_options")?;
                }
                if *srv
                    && (self.port != 27017
                        || self.host.parse::<std::net::IpAddr>().is_ok()
                        || self.tls.mode != DbTlsMode::Verify)
                {
                    return Err(invalid(
                        "srv",
                        "SRV requires a hostname, default port and verified TLS",
                    ));
                }
            }
        }
        if self.tags.len() > 64 {
            return Err(invalid("tags", "too many tags"));
        }
        for tag in &self.tags {
            text(tag, 128, false, "tag")?;
        }
        if let Some(color) = &self.color {
            if color.len() != 7
                || !color.starts_with('#')
                || !color.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit)
            {
                return Err(invalid("color", "expected #RRGGBB"));
            }
        }
        Ok(())
    }
}

fn invalid(field: &'static str, reason: &'static str) -> ValidationError {
    ValidationError::new(field, reason)
}

fn text(
    value: &str,
    maximum: usize,
    empty: bool,
    field: &'static str,
) -> Result<(), ValidationError> {
    if (!empty && value.trim().is_empty())
        || value.len() > maximum
        || value.chars().any(char::is_control)
    {
        return Err(invalid(field, "invalid text"));
    }
    Ok(())
}

fn host(value: &str, field: &'static str) -> Result<(), ValidationError> {
    if value.is_empty()
        || value.len() > 253
        || value.chars().any(char::is_whitespace)
        || value.contains(['/', '\\', '@', '?', '#', '%', '[', ']'])
        || (value.parse::<std::net::IpAddr>().is_err() && url::Host::parse(value).is_err())
    {
        return Err(invalid(
            field,
            "expected a hostname or unbracketed IP address",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_engine_defaults_and_references_roundtrip_without_secrets() {
        for engine in DbEngine::ALL {
            let mut value = DbConnection::new("fixture", "db.example.test", engine);
            value.auth.password_secret_id = Some(ObjectId::new());
            value.validate().unwrap();
            let encoded = serde_json::to_string(&value).unwrap();
            let restored: DbConnection = serde_json::from_str(&encoded).unwrap();
            assert_eq!(restored, value);
            assert!(restored.production);
            assert_eq!(restored.tls.mode, DbTlsMode::Verify);
            assert_eq!(restored.auth.secret_ids().len(), 1);
        }
    }

    #[test]
    fn untyped_driver_security_switches_cannot_enter_inventory() {
        let value = DbConnection::new("fixture", "db.example.test", DbEngine::Mysql);
        let mut json = serde_json::to_value(value).unwrap();
        for key in [
            "local_infile",
            "multiStatements",
            "tlsAllowInvalidCertificates",
            "password",
        ] {
            json["options"][key] = serde_json::json!(true);
            assert!(serde_json::from_value::<DbConnection>(json.clone()).is_err());
            json["options"].as_object_mut().unwrap().remove(key);
        }
    }

    #[test]
    fn metadata_rejects_urls_nil_secrets_unbounded_limits_and_engine_mismatch() {
        let original = DbConnection::new("fixture", "db.example.test", DbEngine::Postgresql);
        for address in [
            "postgres://user:password@db",
            "db/secret",
            "db\nhost",
            "db:password@host",
        ] {
            let mut value = original.clone();
            value.host = address.into();
            assert!(value.validate().is_err());
        }
        let mut value = original.clone();
        value.auth.password_secret_id = Some(ObjectId::NIL);
        assert!(value.validate().is_err());
        value = original.clone();
        value.limits.statement_timeout_seconds = 301;
        assert!(value.validate().is_err());
        value.production = false;
        assert!(value.validate().is_ok());
        value.options = DbOptions::Redis { resp3: true };
        assert!(value.validate().is_err());
    }

    #[test]
    fn raw_ipv6_addresses_are_distinct_from_host_port_and_uri_inputs() {
        for address in ["127.0.0.1", "::1", "2001:db8::23", "db.example.test"] {
            DbConnection::new("fixture", address, DbEngine::Postgresql)
                .validate()
                .unwrap();
        }
        for address in ["[::1]", "host:5432", "::1/path", "user@::1"] {
            assert!(DbConnection::new("fixture", address, DbEngine::Postgresql)
                .validate()
                .is_err());
        }
    }
}
