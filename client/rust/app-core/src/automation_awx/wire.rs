//! Closed, bounded public wire. The returned operation is an opaque Native
//! registration; JSON cannot encode its authority, body state or completion.
use super::{
    catalog::AwxScope,
    runtime::{AwxNativeOperation, ReadAction},
    *,
};
use crate::{AppCore, AppError, AppResult};

const MAX_WIRE: usize = 64 * 1024;
fn invalid() -> AppError {
    AppError::invalid("automation_awx", "awx_invalid_input")
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    version: u8,
    profile_id: String,
    notification_scope_id: String,
    action: Action,
}
#[derive(Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum Action {
    Templates {
        connection_id: String,
        connection_revision: u64,
        page: u64,
        search: String,
    },
    Prepare {
        connection_id: String,
        connection_revision: u64,
        template_id: u64,
        parameters: AwxLaunchParameters,
    },
    Job {
        connection_id: String,
        connection_revision: u64,
        job_id: u64,
    },
    Events {
        connection_id: String,
        connection_revision: u64,
        job_id: u64,
        after: u64,
    },
    Launch {
        review_ticket: String,
    },
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Discard {
    version: u8,
    profile_id: String,
    notification_scope_id: String,
    review_ticket: String,
}
fn decode<T: serde::de::DeserializeOwned>(raw: &str) -> AppResult<T> {
    if raw.len() > MAX_WIRE {
        return Err(invalid());
    }
    serde_json::from_value(super::strict_json::parse(raw.as_bytes()).map_err(|_| invalid())?)
        .map_err(|_| invalid())
}

impl AppCore {
    pub async fn automation_awx_scope_json(&self) -> AppResult<String> {
        serde_json::to_string(&self.automation_awx_scope().await?).map_err(|_| invalid())
    }
    pub async fn automation_awx_catalog_request_json(&self, request: String) -> AppResult<String> {
        self.automation_awx_catalog_json(&request).await
    }
    pub async fn automation_awx_register_request_json(
        &self,
        request: String,
    ) -> AppResult<AwxNativeOperation> {
        let request: Request = decode(&request)?;
        if request.version != 1 {
            return Err(invalid());
        }
        let scope = AwxScope {
            version: request.version,
            profile_id: request.profile_id,
            notification_scope_id: request.notification_scope_id,
        };
        let (connection, revision, action) = match request.action {
            Action::Templates {
                connection_id,
                connection_revision,
                page,
                search,
            } => (
                connection_id,
                connection_revision,
                ReadAction::Templates { page, search },
            ),
            Action::Prepare {
                connection_id,
                connection_revision,
                template_id,
                parameters,
            } => (
                connection_id,
                connection_revision,
                ReadAction::Prepare {
                    template: template_id,
                    parameters,
                },
            ),
            Action::Job {
                connection_id,
                connection_revision,
                job_id,
            } => (
                connection_id,
                connection_revision,
                ReadAction::Job { job: job_id },
            ),
            Action::Events {
                connection_id,
                connection_revision,
                job_id,
                after,
            } => (
                connection_id,
                connection_revision,
                ReadAction::Events { job: job_id, after },
            ),
            Action::Launch { review_ticket } => {
                return self
                    .automation_awx_register_launch(&scope, &review_ticket)
                    .await;
            }
        };
        self.automation_awx_register_read(&scope, &connection, revision, action)
            .await
    }
    pub async fn automation_awx_discard_review_json(&self, request: String) -> AppResult<()> {
        let request: Discard = decode(&request)?;
        if request.version != 1 {
            return Err(invalid());
        }
        self.automation_awx_discard_review(
            &AwxScope {
                version: request.version,
                profile_id: request.profile_id,
                notification_scope_id: request.notification_scope_id,
            },
            &request.review_ticket,
        )
        .await
    }
    pub async fn automation_awx_start_registered(
        &self,
        registration: &AwxNativeOperation,
    ) -> AppResult<String> {
        serde_json::to_string(&self.automation_awx_start(registration).await?)
            .map_err(|_| invalid())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn closed_wire_rejects_unknown_duplicate_future_authority_and_oversize_before_admission() {
        let base = serde_json::json!({"version":1,"profileId":"p","notificationScopeId":"s",
            "action":{"kind":"prepare","connectionId":"c","connectionRevision":1,"templateId":7,"parameters":{}}});
        assert!(decode::<Request>(&base.to_string()).is_ok());
        let mut changed = base.clone();
        changed["action"]["approved"] = Value::Bool(true);
        assert!(decode::<Request>(&changed.to_string()).is_err());
        let mut changed = base.clone();
        changed["action"]["parameters"]["credentials"] = serde_json::json!([1]);
        assert!(decode::<Request>(&changed.to_string()).is_err());
        let mut changed = base.clone();
        changed["arbitraryEndpoint"] = serde_json::json!("https://untrusted.invalid");
        assert!(decode::<Request>(&changed.to_string()).is_err());
        let duplicate =
            base.to_string()
                .replacen("\"version\":1", "\"version\":1,\"version\":1", 1);
        assert!(decode::<Request>(&duplicate).is_err());
        assert!(decode::<Request>(&" ".repeat(MAX_WIRE + 1)).is_err());
    }

    #[test]
    fn launch_wire_carries_only_ticket_and_never_replacement_parameters_or_connection() {
        let valid = r#"{"version":1,"profileId":"p","notificationScopeId":"s","action":{"kind":"launch","reviewTicket":"ticket"}}"#;
        assert!(decode::<Request>(valid).is_ok());
        for field in [
            "parameters",
            "connectionId",
            "connectionRevision",
            "templateId",
            "approve",
        ] {
            let mut raw: Value = serde_json::from_str(valid).unwrap();
            raw["action"][field] = Value::Null;
            assert!(decode::<Request>(&raw.to_string()).is_err(), "{field}");
        }
    }
}
