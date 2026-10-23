import 'package:consolecrypt/core/models/validation.dart';

/// Error codes crossing the core → UI boundary. app-core maps its error
/// enums to these (FRB translates a Rust `enum AppError { code, message }`).
/// Server `cc_protocol::ErrorCode`s surface through the codes documented in
/// `lib/core/l10n/error_messages.dart`. Messages must never contain secrets.
enum AppErrorCode {
  invalidCredentials,
  serverUnreachable,
  incompatibleServer,
  registrationClosed,
  emailTaken,
  wrongPassphrase,

  /// A step-up needs the vault passphrase (the OS check is not offered or
  /// its presence token was refused).
  passphraseRequired,
  invalidRecoveryKey,
  deviceNotTrusted,
  osAuthFailed,

  /// The device unlock key became invalid (biometrics changed): unlock with
  /// the passphrase and switch device unlock on again.
  osAuthInvalidated,
  secureStore,
  verificationMismatch,
  requestExpired,
  validation,
  notFound,
  conflict,
  offline,
  cancelled,
  hostKeyRejected,
  hostKeyChanged,
  authFailed,
  unsupported,
  internal,

  /// Server `Unauthorized` / `RefreshTokenReused`: sign in again.
  sessionExpired,

  /// Server `DeviceRevoked`.
  deviceRevoked,

  /// Server `EmailNotVerified`.
  emailNotVerified,

  /// Server `Forbidden`.
  forbidden,

  /// Server `RateLimited` (`args['retry_after_seconds']` when known;
  /// protocol 1.11: `args['limit']` names the server limit).
  rateLimited,

  /// Server `PayloadTooLarge`.
  payloadTooLarge,

  /// Server `InvalidProof` (bad signature / vault access key).
  invalidProof,

  /// Server `Unavailable` (503) — reachable but temporarily not serving.
  serverUnavailable,

  /// Server `DeviceInQuarantine` (protocol 1.7): this device is new and may
  /// not act on trusted devices yet (`args['until_ms']` when known).
  deviceInQuarantine,

  /// Server `EnvelopeRateLimited` (protocol 1.7): too many vault unlock
  /// attempts from new devices (`args['retry_after_seconds']`).
  envelopeRateLimited,
  sharingReconciliationRequired,

  /// Enterprise sign-in (protocol 1.8): the account was disabled by the
  /// administrator / in the IdP or directory; every session was revoked.
  accountDisabled,

  /// The server only accepts the organisation's sign-in (no account
  /// password, registration or password reset).
  enterpriseAuthRequired,

  /// The organisation's identity is linked to another account here.
  identityConflict,

  /// The e-mail domain is not allowed on this server.
  domainNotAllowed,

  /// The identity lacks the required group / role.
  groupRequired,

  /// The organisation requires multi-factor sign-in.
  mfaRequired,

  /// The organisation's IdP / directory is unreachable (retry later).
  idpUnavailable,

  /// The server sent a vault key that differs from the key this device
  /// trusts (audit H2). Nothing was opened with it; sync is stopped.
  /// `args`: `vault_id`, `reason`, `pinned`, `received`.
  vaultKeyMismatch,

  /// A sharing recipient (or the owner to accept) has no usable key pin
  /// (audit H3). `args['reason']` = `not_chosen` | `keys_changed` |
  /// `unverified`, `args['device_id']`.
  recipientUnverified,

  /// The safety number / QR code did not match the colleague's keys.
  peerVerificationFailed,
}

/// Optional refinement of an [AppErrorCode] so the UI can show a precise,
/// localized message (`lib/core/l10n/error_messages.dart`). The Rust core
/// sends the [wireName]; unknown names are ignored (code-level text is used).
enum AppErrorReason {
  /// A model field failed validation: `args` = `{field, rule}` (see
  /// [AppException.fromValidation]).
  invalidField('invalid_field'),
  nameRequired('name_required'),
  profileNameRequired('profile_name_required'),
  passwordRequired('password_required'),
  keyPassphraseRequired('key_passphrase_required'),
  agentPathRequired('agent_path_required'),
  invalidEmail('invalid_email'),
  accountPasswordTooShort('account_password_too_short'),
  weakPassphrase('weak_passphrase'),
  invalidBaseUrl('invalid_base_url'),
  chatModelRequired('chat_model_required'),
  templateRequired('template_required'),

  /// `args['names']` = comma-separated variable names.
  missingVariables('missing_variables'),
  invalidKey('invalid_key'),
  notACertificate('not_a_certificate'),
  notAgentCredential('not_agent_credential'),
  keyHasNoPassphrase('key_has_no_passphrase'),
  unsupportedKeyAlgorithm('unsupported_key_algorithm'),
  groupCycle('group_cycle'),
  jumpChainEmpty('jump_chain_empty'),
  hostNotFound('host_not_found'),
  credentialNotFound('credential_not_found'),
  profileNotFound('profile_not_found'),
  deviceNotFound('device_not_found'),
  tunnelNotFound('tunnel_not_found'),
  providerNotFound('provider_not_found'),
  requestNotFound('request_not_found'),
  secretNotStored('secret_not_stored'),
  sessionClosed('session_closed'),

  /// Teams: this member may only read the team (viewer).
  teamReadOnly('team_read_only'),

  /// Teams: the server showed a membership log this client cannot trust.
  teamUntrusted('team_untrusted'),
  editorLaunchFailed('editor_launch_failed'),

  /// A file that can run code opens only as text, and no text editor was
  /// found (security audit H4): offer "Choose an Editor…".
  noTextEditor('no_text_editor'),
  applicationPickerFailed('application_picker_failed'),

  /// `args['path']`.
  directoryNotFound('directory_not_found'),
  directoryNotEmpty('directory_not_empty'),

  /// `args['name']`.
  alreadyExists('already_exists'),
  selectFile('select_file'),
  vaultLocked('vault_locked'),
  noActiveProfile('no_active_profile'),
  noVault('no_vault'),
  vaultExists('vault_exists'),
  notABackup('not_a_backup'),
  backupExtension('backup_extension'),
  backupFolderRequired('backup_folder_required'),
  keepAtLeastOne('keep_at_least_one'),
  syncedProfileRequired('synced_profile_required'),
  localProfileNoAccount('local_profile_no_account'),
  noDeviceRegistered('no_device_registered'),
  trustedDeviceRequired('trusted_device_required'),
  currentPassphraseWrong('current_passphrase_wrong'),
  currentPasswordWrong('current_password_wrong'),

  /// Signing in to a synced profile with a different account.
  accountMismatch('account_mismatch'),

  /// The browser came back with a missing or foreign `state` / code.
  enterpriseCallbackInvalid('enterprise_callback_invalid'),

  /// The browser sign-in took too long (10 minutes).
  enterpriseSignInTimeout('enterprise_sign_in_timeout'),

  /// The system browser / redirect listener could not be used.
  enterpriseBrowserUnavailable('enterprise_browser_unavailable'),

  /// Protocol 1.11 quota (`413`): the vault would hold more than
  /// `args['limit']` objects.
  objectLimit('object_limit'),

  /// Protocol 1.11 quota (`413`): the vault's server storage
  /// (`args['limit']` bytes) is full.
  vaultQuota('vault_quota'),

  /// Protocol 1.11 quota at sign-in (`403`): the account already has
  /// `args['limit']` devices.
  deviceLimit('device_limit'),

  /// Protocol 1.11 quota at sign-in (`403`): too many new devices in 24 h;
  /// `args['until_ms']` is when the next one is accepted.
  deviceDailyLimit('device_daily_limit');

  const AppErrorReason(this.wireName);

  /// snake_case name sent by app-core.
  final String wireName;

  static AppErrorReason? fromWire(String? name) => values.where((r) => r.wireName == name).firstOrNull;
}

/// The single exception type service implementations throw.
final class AppException implements Exception {
  const AppException(this.code, this.message, {this.reason, this.args = const {}});

  /// A model-level [ValidationError] (`Host.validate`, `Tunnel.validate`).
  AppException.fromValidation(ValidationError error)
    : this(
        AppErrorCode.validation,
        error.toString(),
        reason: AppErrorReason.invalidField,
        args: {'field': error.field, 'rule': error.reason},
      );

  final AppErrorCode code;

  /// English, secret-free diagnostic text. The UI never shows it as the
  /// primary message: it renders [code]/[reason] via `errorMessage()`.
  final String message;

  final AppErrorReason? reason;

  /// Non-secret parameters for [reason] (names, paths, counts).
  final Map<String, String> args;

  @override
  String toString() => 'AppException(${code.name}${reason == null ? '' : '/${reason!.wireName}'}): $message';
}
