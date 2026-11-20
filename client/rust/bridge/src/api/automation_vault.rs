//! Fixed-purpose Ansible Vault file operation. Passwords use the typed byte
//! channel and are immediately moved into a zeroizing Native owner.
use crate::{api::error::BridgeError, state::{to_json, with_core}};
use zeroize::Zeroizing;

pub async fn automation_vault_transform(request_json: String, password: Option<Vec<u8>>) -> Result<String, BridgeError> {
    let password = password.map(Zeroizing::new);
    with_core(move |core| async move {
        let password = password.map(|value| value.to_vec());
        to_json(&core.automation_vault_transform(request_json,password).await?)
    }).await
}
