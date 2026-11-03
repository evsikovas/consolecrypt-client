use cc_app_core::AppError;
use cc_platform_core::OsAuthError;

#[test]
fn hello_error_keeps_stage_and_numeric_code_for_ui() {
    let error: AppError = OsAuthError::WindowsHello {
        operation: "create",
        diagnostic: "HRESULT 0x80090027".into(),
    }
    .into();
    assert_eq!(error.code(), "os_auth_failed");
    assert_eq!(
        error.args()["hello_diagnostic"],
        "create: HRESULT 0x80090027"
    );
    assert_eq!(
        AppError::from(OsAuthError::Invalidated),
        AppError::OsAuthInvalidated
    );
}
