//! Monitoring facade accepts source IDs/revisions, never arbitrary fetch URLs
//! or raw authorization material during reads. Config writes are separate CAS.
use crate::api::error::BridgeError;
use crate::state::{from_json, to_json, with_core};
use serde::{Deserialize, Deserializer, de};
use std::{collections::BTreeMap, fmt};

const MAX_PANEL_REQUEST_BYTES: usize = 64 * 1024;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreatePanelRequest {
    expected_catalog_revision: u64,
    definition: cc_app_core::monitoring_sources::MonitoringPanelDefinition,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdatePanelRequest {
    id: String,
    expected_catalog_revision: u64,
    expected_panel_revision: u64,
    definition: cc_app_core::monitoring_sources::MonitoringPanelDefinition,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DeletePanelRequest {
    id: String,
    expected_catalog_revision: u64,
    expected_panel_revision: u64,
}
fn panel_request<T: de::DeserializeOwned>(raw: &str) -> Result<T, BridgeError> {
    if raw.len() > MAX_PANEL_REQUEST_BYTES {
        return Err(BridgeError::invalid(
            "monitoring_panels",
            "panel_request_budget",
        ));
    }
    from_json("monitoring_panels", raw)
}

pub async fn monitoring_panel_create(request_json: String) -> Result<String, BridgeError> {
    let request: CreatePanelRequest = panel_request(&request_json)?;
    with_core(move |c| async move {
        to_json(
            &c.monitoring_panel_create(request.expected_catalog_revision, request.definition)
                .await?,
        )
    })
    .await
}
pub async fn monitoring_panel_update(request_json: String) -> Result<String, BridgeError> {
    let request: UpdatePanelRequest = panel_request(&request_json)?;
    with_core(move |c| async move {
        to_json(
            &c.monitoring_panel_update(
                request.id,
                request.expected_catalog_revision,
                request.expected_panel_revision,
                request.definition,
            )
            .await?,
        )
    })
    .await
}
pub async fn monitoring_panel_delete(request_json: String) -> Result<String, BridgeError> {
    let request: DeletePanelRequest = panel_request(&request_json)?;
    with_core(move |c| async move {
        to_json(
            &c.monitoring_panel_delete(
                request.id,
                request.expected_catalog_revision,
                request.expected_panel_revision,
            )
            .await?,
        )
    })
    .await
}

const MAX_BINDING_REQUEST_BYTES: usize = 64 * 1024;

// The JSON seam has no authority, endpoint, secret or caller-assigned binding
// revision/identity on create. Native performs the authoritative catalog checks.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateBindingRequest {
    #[serde(default)]
    exporter_origin: Option<cc_app_core::monitoring_sources::MonitoringExporterBindingOrigin>,
    expected_catalog_revision: u64,
    name: String,
    host_id: String,
    source_id: String,
    expected_source_revision: u64,
    #[serde(deserialize_with = "binding_selector")]
    selector: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateBindingRequest {
    id: String,
    expected_catalog_revision: u64,
    expected_binding_revision: u64,
    name: String,
    host_id: String,
    source_id: String,
    expected_source_revision: u64,
    #[serde(deserialize_with = "binding_selector")]
    selector: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DeleteBindingRequest {
    id: String,
    expected_catalog_revision: u64,
    expected_binding_revision: u64,
}

fn binding_request<T: de::DeserializeOwned>(raw: &str) -> Result<T, BridgeError> {
    if raw.len() > MAX_BINDING_REQUEST_BYTES {
        return Err(BridgeError::invalid(
            "monitoring_binding",
            "request exceeds budget",
        ));
    }
    from_json("monitoring_binding", raw)
}

fn binding_selector<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<String, String>, D::Error> {
    struct ExactSelector;
    impl<'de> de::Visitor<'de> for ExactSelector {
        type Value = BTreeMap<String, String>;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a bounded unique exact selector")
        }
        fn visit_map<A: de::MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut selector = BTreeMap::new();
            let mut bytes = 0usize;
            while let Some((name, value)) = map.next_entry::<String, String>()? {
                // Wire limits mirror the HostBinding v1 contract. Native's
                // validated type additionally checks names and PromQL rendering.
                bytes = bytes.saturating_add(name.len()).saturating_add(value.len());
                if name.is_empty()
                    || name.len() > 128
                    || value.is_empty()
                    || value.len() > 4096
                    || selector.len() >= 32
                    || bytes > 16 * 1024
                    || selector.insert(name, value).is_some()
                {
                    return Err(de::Error::custom("invalid bounded unique selector"));
                }
            }
            if selector.is_empty() {
                return Err(de::Error::custom("empty exact selector"));
            }
            Ok(selector)
        }
    }
    deserializer.deserialize_map(ExactSelector)
}

pub async fn monitoring_binding_create(request_json: String) -> Result<String, BridgeError> {
    let request: CreateBindingRequest = binding_request(&request_json)?;
    with_core(move |c| async move {
        to_json(
            &c.monitoring_binding_create_with_exporter_origin(
                request.expected_catalog_revision,
                request.name,
                request.host_id,
                request.source_id,
                request.expected_source_revision,
                request.selector,
                request.exporter_origin,
            )
            .await?,
        )
    })
    .await
}

pub async fn monitoring_binding_update(request_json: String) -> Result<String, BridgeError> {
    let request: UpdateBindingRequest = binding_request(&request_json)?;
    with_core(move |c| async move {
        to_json(
            &c.monitoring_binding_update(
                request.id,
                request.expected_catalog_revision,
                request.expected_binding_revision,
                request.name,
                request.host_id,
                request.source_id,
                request.expected_source_revision,
                request.selector,
            )
            .await?,
        )
    })
    .await
}

pub async fn monitoring_binding_delete(request_json: String) -> Result<String, BridgeError> {
    let request: DeleteBindingRequest = binding_request(&request_json)?;
    with_core(move |c| async move {
        to_json(
            &c.monitoring_binding_delete(
                request.id,
                request.expected_catalog_revision,
                request.expected_binding_revision,
            )
            .await?,
        )
    })
    .await
}

pub async fn monitoring_sources() -> Result<String, BridgeError> {
    with_core(|c| async move { to_json(&c.monitoring_sources().await?) }).await
}

pub async fn monitoring_source_create(
    expected_catalog_revision: u64,
    configuration_json: String,
) -> Result<String, BridgeError> {
    if configuration_json.len() > 300 * 1024 {
        return Err(BridgeError::invalid(
            "monitoring",
            "configuration exceeds budget",
        ));
    }
    let configuration = from_json("monitoring", &configuration_json)?;
    with_core(move |c| async move {
        to_json(
            &c.monitoring_source_create(expected_catalog_revision, configuration)
                .await?,
        )
    })
    .await
}

pub async fn monitoring_source_update(
    id: String,
    expected_catalog_revision: u64,
    expected_source_revision: u64,
    configuration_json: String,
) -> Result<String, BridgeError> {
    if configuration_json.len() > 300 * 1024 {
        return Err(BridgeError::invalid(
            "monitoring",
            "configuration exceeds budget",
        ));
    }
    let configuration = from_json("monitoring", &configuration_json)?;
    with_core(move |c| async move {
        to_json(
            &c.monitoring_source_update(
                id,
                expected_catalog_revision,
                expected_source_revision,
                configuration,
            )
            .await?,
        )
    })
    .await
}

pub async fn monitoring_source_delete(
    id: String,
    expected_catalog_revision: u64,
    expected_source_revision: u64,
) -> Result<String, BridgeError> {
    with_core(move |c| async move {
        to_json(
            &c.monitoring_source_delete(id, expected_catalog_revision, expected_source_revision)
                .await?,
        )
    })
    .await
}

/// JSON actions include parameter-free alerts/alertRules. Native resolves the
/// saved source/revision and enforces the same unlock/auth/cancel fences.
pub async fn monitoring_read(request_json: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.monitoring_read(request_json).await?) }).await
}

pub async fn monitoring_cancel(operation_id: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.monitoring_cancel(operation_id).await?) }).await
}

#[cfg(test)]
mod binding_input_tests {
    use super::*;
    use serde_json::json;

    fn create() -> serde_json::Value {
        json!({
            "expectedCatalogRevision": 4,
            "name": "Node exporter",
            "hostId": "11111111-1111-4111-8111-111111111111",
            "sourceId": "22222222-2222-4222-8222-222222222222",
            "expectedSourceRevision": 2,
            "selector": {"job": "node", "instance": "synthetic:9100"}
        })
    }

    #[test]
    fn binding_crud_frames_preserve_explicit_metadata_and_reject_extra_authority() {
        let request: CreateBindingRequest = binding_request(&create().to_string()).unwrap();
        assert_eq!(request.expected_catalog_revision, 4);
        assert_eq!(request.expected_source_revision, 2);
        assert_eq!(request.selector["instance"], "synthetic:9100");
        let mut update = create();
        update["id"] = json!("33333333-3333-4333-8333-333333333333");
        update["expectedBindingRevision"] = json!(3);
        let request: UpdateBindingRequest = binding_request(&update.to_string()).unwrap();
        assert_eq!(request.expected_binding_revision, 3);
        let delete =
            json!({"id": request.id, "expectedCatalogRevision": 4, "expectedBindingRevision": 3});
        let request: DeleteBindingRequest = binding_request(&delete.to_string()).unwrap();
        assert_eq!(request.expected_binding_revision, 3);
        for key in [
            "id",
            "revision",
            "version",
            "endpoint",
            "authorization",
            "resolution",
        ] {
            let mut input = create();
            input[key] = json!("synthetic-secret");
            let error = binding_request::<CreateBindingRequest>(&input.to_string())
                .err()
                .unwrap();
            assert!(!error.message.contains("synthetic-secret"));
        }
    }

    #[test]
    fn duplicate_fields_and_selector_keys_are_rejected_before_normalization() {
        let raw = create().to_string();
        for input in [
            raw.replacen("\"job\":\"node\"", "\"job\":\"node\",\"job\":\"other\"", 1),
            raw.replacen(
                "\"job\":\"node\"",
                "\"job\":\"node\",\"\\u006aob\":\"other\"",
                1,
            ),
            raw.replacen("\"name\":", "\"name\":\"other\",\"name\":", 1),
        ] {
            assert!(binding_request::<CreateBindingRequest>(&input).is_err());
        }
    }

    #[test]
    fn binding_frames_and_selectors_have_independent_budgets() {
        let raw = create().to_string();
        let padded = format!("{raw}{}", " ".repeat(MAX_BINDING_REQUEST_BYTES - raw.len()));
        assert!(binding_request::<CreateBindingRequest>(&padded).is_ok());
        assert!(binding_request::<CreateBindingRequest>(&format!("{padded} ")).is_err());
        let mut input = create();
        for selector in [
            json!({}),
            json!({"job": "x".repeat(4097)}),
            json!({("x".repeat(129)): "node"}),
            json!({"job": 4}),
            serde_json::Value::Object(
                (0..33)
                    .map(|n| (format!("label{n}"), json!("value")))
                    .collect(),
            ),
        ] {
            input["selector"] = selector;
            assert!(binding_request::<CreateBindingRequest>(&input.to_string()).is_err());
        }
        input["selector"] = json!({"a": "x".repeat(4096), "b": "x".repeat(4096), "c": "x".repeat(4096), "d": "x".repeat(4096)});
        assert!(binding_request::<CreateBindingRequest>(&input.to_string()).is_err());
    }
    #[test]
    fn saved_panel_frames_are_bounded_strict_and_never_accept_caller_identity_or_auth() {
        let create = json!({"expectedCatalogRevision":3,"definition":{"title":"Owned panel","sourceId":"11111111-1111-4111-8111-111111111111","sourceRevision":2,"query":"up","unitHint":"unknown"}});
        assert!(panel_request::<CreatePanelRequest>(&create.to_string()).is_ok());
        for key in [
            "id",
            "version",
            "revision",
            "authorization",
            "endpoint",
            "measurements",
        ] {
            let mut invalid = create.clone();
            invalid[key] = json!("never-echo-synthetic-value");
            let error = panel_request::<CreatePanelRequest>(&invalid.to_string())
                .err()
                .unwrap();
            assert!(!error.message.contains("never-echo-synthetic-value"));
        }
        let raw = create.to_string();
        for invalid in [
            raw.replacen("\"query\":", "\"query\":\"first\",\"query\":", 1),
            raw.replacen(
                "\"expectedCatalogRevision\":",
                "\"expectedCatalogRevision\":1,\"expectedCatalogRevision\":",
                1,
            ),
            format!("{raw}{}", " ".repeat(MAX_PANEL_REQUEST_BYTES)),
        ] {
            assert!(panel_request::<CreatePanelRequest>(&invalid).is_err());
        }
        let mut update = create;
        update["id"] = json!("22222222-2222-4222-8222-222222222222");
        update["expectedPanelRevision"] = json!(1);
        let request: UpdatePanelRequest = panel_request(&update.to_string()).unwrap();
        assert_eq!(request.expected_panel_revision, 1);
        let delete = json!({"id":request.id,"expectedCatalogRevision":3,"expectedPanelRevision":1});
        assert!(panel_request::<DeletePanelRequest>(&delete.to_string()).is_ok());
    }
}

/// Metadata-only capture/recheck. No metrics, routing, host material or
/// credentials are returned. Core enforces strict bounded JSON and original
/// profile/unlock scope; this value does not authorize a connection.
pub async fn monitoring_host_context(request_json: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { Ok(c.monitoring_host_context_json(&request_json).await?) })
        .await
}

/// Prepare only the explicit selected observation, using Native sanitization.
pub async fn monitoring_ai_prepare(request_json: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { Ok(c.monitoring_ai_prepare_json(&request_json).await?) }).await
}

/// Consume the original one-use preview; this API has no execution capability.
pub async fn monitoring_ai_send(request_json: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { Ok(c.monitoring_ai_send_json(&request_json).await?) }).await
}

pub async fn monitoring_ai_discard(request_json: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.monitoring_ai_discard_json(&request_json).await?) }).await
}

/// Explicit fixed diagnostic only; no writer/admission grant.
pub async fn monitoring_settings_sync_check(request_json: String) -> Result<String, BridgeError> {
    with_core(move |core| async move {
        Ok(core
            .monitoring_settings_sync_check_json(&request_json)
            .await?)
    })
    .await
}
