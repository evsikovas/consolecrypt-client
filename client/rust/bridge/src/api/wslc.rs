//! Windows WSLc workspace through ConsoleCrypt's single runtime and vault lease.
use crate::api::error::BridgeError;

#[flutter_rust_bridge::frb(sync)]
pub fn wslc_preview(spec_json: String, copy: bool) -> Result<String, BridgeError> {
    #[cfg(windows)]
    {
        let spec = crate::state::from_json("wslc_run_spec", &spec_json)?;
        let result = if copy {
            cc_wslc_core::runspec::copy_command(&spec)
        } else {
            cc_wslc_core::runspec::preview(&spec)
        };
        result.map_err(|_| BridgeError::invalid("wslc", "invalid_run_spec"))
    }
    #[cfg(not(windows))]
    {
        let _ = (spec_json, copy);
        Err(BridgeError::invalid("wslc", "windows_only"))
    }
}

pub async fn wslc_open(profile_id: String) -> Result<String, BridgeError> {
    #[cfg(windows)]
    {
        crate::state::with_core(move |core| crate::wslc_runtime::open(core, profile_id)).await
    }
    #[cfg(not(windows))]
    {
        let _ = profile_id;
        Err(BridgeError::invalid("wslc", "windows_only"))
    }
}

pub async fn wslc_request(
    session_id: String,
    operation: String,
    args_json: String,
    passphrase: Option<Vec<u8>>,
    presence_token: Option<String>,
) -> Result<String, BridgeError> {
    #[cfg(windows)]
    {
        crate::state::with_core(move |core| {
            crate::wslc_runtime::request(
                core,
                session_id,
                operation,
                args_json,
                passphrase,
                presence_token,
            )
        })
        .await
    }
    #[cfg(not(windows))]
    {
        use zeroize::Zeroize;
        let mut passphrase = passphrase;
        passphrase.zeroize();
        let _ = (session_id, operation, args_json, presence_token);
        Err(BridgeError::invalid("wslc", "windows_only"))
    }
}

pub async fn wslc_registry_login(
    session_id: String,
    server: Option<String>,
    username: String,
    password: Vec<u8>,
) -> Result<String, BridgeError> {
    #[cfg(windows)]
    {
        crate::state::with_core(move |core| {
            crate::wslc_runtime::login(core, session_id, server, username, password)
        })
        .await
    }
    #[cfg(not(windows))]
    {
        use zeroize::Zeroize;
        let mut password = password;
        password.zeroize();
        let _ = (session_id, server, username);
        Err(BridgeError::invalid("wslc", "windows_only"))
    }
}

pub async fn wslc_close(session_id: String) -> Result<(), BridgeError> {
    #[cfg(windows)]
    {
        crate::state::run(async move {
            crate::wslc_runtime::close(&session_id).await;
            Ok(())
        })
        .await
    }
    #[cfg(not(windows))]
    {
        let _ = session_id;
        Ok(())
    }
}
