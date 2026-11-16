use serde::{Deserialize, Serialize};
use std::fmt;
use url::Url;

use crate::{Error, ErrorCode};

/// Persist references only. Credential material is never part of this DTO.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum AuthRef {
    None,
    BearerRef {
        #[serde(rename = "credentialId")]
        credential_id: String,
    },
    BasicRef {
        #[serde(rename = "credentialId")]
        credential_id: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Route {
    Direct,
    /// Explicit saved inventory SSH host, including its Native planned jump route.
    /// Legacy SshTunnel route IDs are a different namespace and remain unsupported.
    SshHost {
        #[serde(rename = "hostId")]
        host_id: String,
    },
    SshTunnel {
        #[serde(rename = "routeId")]
        route_id: String,
    },
    /// Persistent destination metadata. A separately approved current route is required.
    AgentDestination {
        #[serde(rename = "associationId")]
        association_id: String,
        #[serde(rename = "hostId")]
        host_id: String,
        #[serde(rename = "destinationIp")]
        destination_ip: String,
        #[serde(rename = "destinationPort")]
        destination_port: u16,
    },
    AgentTunnel {
        #[serde(rename = "routeId")]
        route_id: String,
    },
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TlsConfig {
    #[serde(serialize_with = "serialize_ca")]
    pub custom_ca_pem: Option<String>,
}
fn certificate_blocks_only(pem: &str) -> bool {
    let mut rest = pem.trim();
    let mut found = false;
    while !rest.is_empty() {
        let Some(body) = rest.strip_prefix("-----BEGIN CERTIFICATE-----") else {
            return false;
        };
        let Some((body, tail)) = body.split_once("-----END CERTIFICATE-----") else {
            return false;
        };
        if body.trim().is_empty()
            || !body.bytes().all(|b| {
                b.is_ascii_alphanumeric()
                    || b.is_ascii_whitespace()
                    || matches!(b, b'+' | b'/' | b'=')
            })
        {
            return false;
        }
        found = true;
        rest = tail.trim();
    }
    found && pem.len() <= 256 * 1024
}
fn serialize_ca<S: serde::Serializer>(
    pem: &Option<String>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    if pem
        .as_ref()
        .is_some_and(|pem| !certificate_blocks_only(pem))
    {
        return Err(serde::ser::Error::custom(
            "CA must contain only certificate PEM blocks",
        ));
    }
    pem.serialize(serializer)
}
impl fmt::Debug for TlsConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TlsConfig")
            .field("custom_ca", &self.custom_ca_pem.is_some())
            .finish()
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceConfig {
    pub id: String,
    pub name: String,
    #[serde(serialize_with = "serialize_endpoint")]
    pub endpoint: String,
    pub route: Route,
    pub tls: TlsConfig,
    pub auth: AuthRef,
}
impl fmt::Debug for SourceConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SourceConfig")
            .field("endpoint", &"[configured]")
            .field("route", &self.route)
            .finish_non_exhaustive()
    }
}

fn serialize_endpoint<S: serde::Serializer>(
    endpoint: &str,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let url = Url::parse(endpoint).map_err(|_| serde::ser::Error::custom("Invalid endpoint"))?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(serde::ser::Error::custom(
            "Endpoint must not contain credentials, query or fragment",
        ));
    }
    serializer.serialize_str(endpoint)
}

impl SourceConfig {
    pub fn validate(&self) -> Result<Url, Error> {
        let bad = || Error::new(ErrorCode::InvalidConfig, "Invalid source configuration");
        if self.id.is_empty()
            || self.id.len() > 256
            || self.name.len() > 256
            || self.endpoint.len() > 2048
        {
            return Err(bad());
        }
        let mut url = Url::parse(&self.endpoint).map_err(|_| bad())?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(bad());
        }
        // An explicitly saved HTTP+Basic source is an intentional plaintext
        // transport choice. Bearer tokens continue to require HTTPS. Neither
        // choice changes certificate verification for HTTPS sources.
        if matches!(self.auth, AuthRef::BearerRef { .. }) && url.scheme() != "https" {
            return Err(bad());
        }
        if matches!(&self.route, Route::SshHost { host_id } if host_id.is_empty() || host_id.len() > 256)
        {
            return Err(bad());
        }
        if let Route::AgentDestination { association_id, host_id, destination_ip, destination_port } = &self.route {
            fn canonical_id(value: &str) -> bool {
                value.len() == 36 && value != "00000000-0000-0000-0000-000000000000"
                    && value.bytes().enumerate().all(|(i, b)| if [8,13,18,23].contains(&i) {
                        b == b'-'
                    } else { b.is_ascii_digit() || (b'a'..=b'f').contains(&b) })
            }
            let ip = destination_ip.parse::<std::net::IpAddr>().map_err(|_| bad())?;
            if !canonical_id(association_id) || !canonical_id(host_id)
                || !matches!(destination_ip.as_str(), "127.0.0.1" | "::1")
                || ip.to_string() != *destination_ip || *destination_port == 0 { return Err(bad()); }
        }
        match &self.auth {
            AuthRef::BearerRef { credential_id } | AuthRef::BasicRef { credential_id }
                if credential_id.is_empty() || credential_id.len() > 256 =>
            {
                return Err(bad())
            }
            _ => {}
        }
        if self
            .tls
            .custom_ca_pem
            .as_ref()
            .is_some_and(|v| !certificate_blocks_only(v))
        {
            return Err(bad());
        }
        if !url.path().ends_with('/') {
            url.set_path(&format!("{}/", url.path()));
        }
        Ok(url)
    }
}

/// Hard ceilings keep caller-supplied settings from disabling resource limits.
#[derive(Clone, Debug)]
pub struct Budgets {
    pub timeout_seconds: u64,
    pub max_response_bytes: usize,
    pub max_series: usize,
    pub max_points_per_series: usize,
    pub max_total_points: usize,
    pub max_concurrent_requests: usize,
    pub max_query_bytes: usize,
    pub max_metadata_values: usize,
}
impl Default for Budgets {
    fn default() -> Self {
        Self {
            timeout_seconds: 15,
            max_response_bytes: 16 * 1024 * 1024,
            max_series: 100,
            max_points_per_series: 1000,
            max_total_points: 100_000,
            max_concurrent_requests: 4,
            max_query_bytes: 16 * 1024,
            max_metadata_values: 1000,
        }
    }
}
impl Budgets {
    pub fn validate(&self) -> Result<(), Error> {
        let max = Self::default();
        let valid = self.timeout_seconds > 0
            && self.timeout_seconds <= max.timeout_seconds
            && self.max_response_bytes > 0
            && self.max_response_bytes <= max.max_response_bytes
            && self.max_series > 0
            && self.max_series <= max.max_series
            && self.max_points_per_series >= 2
            && self.max_points_per_series <= max.max_points_per_series
            && self.max_total_points > 0
            && self.max_total_points <= max.max_total_points
            && self.max_concurrent_requests > 0
            && self.max_concurrent_requests <= max.max_concurrent_requests
            && self.max_query_bytes > 0
            && self.max_query_bytes <= max.max_query_bytes
            && self.max_metadata_values > 0
            && self.max_metadata_values <= max.max_metadata_values;
        if valid {
            Ok(())
        } else {
            Err(Error::new(
                ErrorCode::InvalidConfig,
                "Invalid request budget",
            ))
        }
    }
}
