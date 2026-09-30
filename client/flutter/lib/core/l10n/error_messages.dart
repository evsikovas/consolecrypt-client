/// The one place where errors become user-facing text.
///
/// Services throw [AppException] (`code` + optional `reason`/`args`); its
/// English `message` is a diagnostic only and never the primary UI text.
///
/// Server `cc_protocol::ErrorCode` → [AppErrorCode] (mapped by app-core):
///
/// | ErrorCode | AppErrorCode |
/// |---|---|
/// | BadRequest | validation |
/// | Unauthorized, RefreshTokenReused | sessionExpired |
/// | InvalidCredentials | invalidCredentials |
/// | Forbidden | forbidden |
/// | DeviceRevoked | deviceRevoked |
/// | DeviceNotTrusted | deviceNotTrusted |
/// | EmailNotVerified | emailNotVerified |
/// | NotFound | notFound |
/// | Conflict | conflict |
/// | AlreadyExists | emailTaken (`field = email`) / conflict (+ reason alreadyExists) |
/// | Gone | requestExpired |
/// | PayloadTooLarge | payloadTooLarge |
/// | InvalidProof | invalidProof |
/// | UpgradeRequired | incompatibleServer |
/// | RateLimited | rateLimited (`args.retry_after_seconds`) |
/// | Internal | internal |
/// | Unavailable | serverUnavailable |
/// | (transport failure) | serverUnreachable / offline |
library;

import 'package:consolecrypt/core/models/validation.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';

/// Localized, user-presentable text for any error surfaced by a service call.
String errorMessage(AppLocalizations l, Object error) => switch (error) {
  AppException() => _reasonMessage(l, error) ?? errorCodeMessage(l, error.code, args: error.args),
  ValidationError() => validationMessage(l, error.field, error.reason),
  _ => l.errorUnknown,
};

/// Code-level text (used when no specific reason is known).
String errorCodeMessage(AppLocalizations l, AppErrorCode code, {Map<String, String> args = const {}}) => switch (code) {
  AppErrorCode.invalidCredentials => l.errorInvalidCredentials,
  AppErrorCode.serverUnreachable => l.errorServerUnreachable,
  AppErrorCode.incompatibleServer => l.errorIncompatibleServer,
  AppErrorCode.registrationClosed => l.errorRegistrationClosed,
  AppErrorCode.emailTaken => l.errorEmailTaken,
  AppErrorCode.wrongPassphrase => l.errorWrongPassphrase,
  AppErrorCode.invalidRecoveryKey => l.errorInvalidRecoveryKey,
  AppErrorCode.deviceNotTrusted => l.errorDeviceNotTrusted,
  AppErrorCode.secureStore => l.errorSecureStore,
  AppErrorCode.osAuthFailed => l.errorOsAuthFailed,
  AppErrorCode.verificationMismatch => l.errorVerificationMismatch,
  AppErrorCode.requestExpired => l.errorRequestExpired,
  AppErrorCode.validation => l.errorValidation,
  AppErrorCode.notFound => l.errorNotFound,
  AppErrorCode.conflict => l.errorConflict,
  AppErrorCode.offline => l.errorOffline,
  AppErrorCode.cancelled => l.errorCancelled,
  AppErrorCode.hostKeyRejected => l.errorHostKeyRejected,
  AppErrorCode.hostKeyChanged => l.errorHostKeyChanged,
  AppErrorCode.authFailed => l.errorAuthFailed,
  AppErrorCode.unsupported => l.errorUnsupported,
  AppErrorCode.internal => l.errorInternal,
  AppErrorCode.sessionExpired => l.errorSessionExpired,
  AppErrorCode.deviceRevoked => l.errorDeviceRevoked,
  AppErrorCode.emailNotVerified => l.errorEmailNotVerified,
  AppErrorCode.forbidden => l.errorForbidden,
  AppErrorCode.rateLimited => switch (int.tryParse(args['retry_after_seconds'] ?? '')) {
    final seconds? when seconds > 0 => l.errorRateLimitedRetry(seconds),
    _ => l.errorRateLimited,
  },
  AppErrorCode.payloadTooLarge => l.errorPayloadTooLarge,
  AppErrorCode.invalidProof => l.errorInvalidProof,
  AppErrorCode.serverUnavailable => l.errorServerUnavailable,
  AppErrorCode.sharingReconciliationRequired => l.sharingReconcileHelp,
};

String? _reasonMessage(AppLocalizations l, AppException e) {
  final args = e.args;
  return switch (e.reason) {
    null => null,
    AppErrorReason.invalidField => validationMessage(l, args['field'] ?? '', args['rule'] ?? ''),
    AppErrorReason.nameRequired => l.errorReasonNameRequired,
    AppErrorReason.profileNameRequired => l.errorReasonProfileNameRequired,
    AppErrorReason.passwordRequired => l.errorReasonPasswordRequired,
    AppErrorReason.keyPassphraseRequired => l.errorReasonKeyPassphraseRequired,
    AppErrorReason.agentPathRequired => l.errorReasonAgentPathRequired,
    AppErrorReason.invalidEmail => l.errorReasonInvalidEmail,
    AppErrorReason.accountPasswordTooShort => l.errorReasonAccountPasswordTooShort(10),
    AppErrorReason.weakPassphrase => l.errorReasonWeakPassphrase,
    AppErrorReason.invalidBaseUrl => l.errorReasonInvalidBaseUrl,
    AppErrorReason.chatModelRequired => l.errorReasonChatModelRequired,
    AppErrorReason.templateRequired => l.errorReasonTemplateRequired,
    AppErrorReason.missingVariables => l.errorReasonMissingVariables(args['names'] ?? ''),
    AppErrorReason.invalidKey => l.errorReasonInvalidKey,
    AppErrorReason.notACertificate => l.errorReasonNotACertificate,
    AppErrorReason.notAgentCredential => l.errorReasonNotAgentCredential,
    AppErrorReason.keyHasNoPassphrase => l.errorReasonKeyHasNoPassphrase,
    AppErrorReason.unsupportedKeyAlgorithm => l.errorReasonUnsupportedKeyAlgorithm,
    AppErrorReason.groupCycle => l.errorReasonGroupCycle,
    AppErrorReason.jumpChainEmpty => l.errorReasonJumpChainEmpty,
    AppErrorReason.hostNotFound => l.errorReasonHostNotFound,
    AppErrorReason.credentialNotFound => l.errorReasonCredentialNotFound,
    AppErrorReason.profileNotFound => l.errorReasonProfileNotFound,
    AppErrorReason.deviceNotFound => l.errorReasonDeviceNotFound,
    AppErrorReason.tunnelNotFound => l.errorReasonTunnelNotFound,
    AppErrorReason.providerNotFound => l.errorReasonProviderNotFound,
    AppErrorReason.requestNotFound => l.errorReasonRequestNotFound,
    AppErrorReason.secretNotStored => l.errorReasonSecretNotStored,
    AppErrorReason.sessionClosed => l.errorReasonSessionClosed,
    AppErrorReason.editorLaunchFailed => l.errorEditorLaunchFailed(args['detail'] ?? ''),
    AppErrorReason.applicationPickerFailed => l.errorApplicationPickerFailed(args['detail'] ?? ''),
    AppErrorReason.directoryNotFound => l.errorReasonDirectoryNotFound(args['path'] ?? ''),
    AppErrorReason.directoryNotEmpty => l.errorReasonDirectoryNotEmpty,
    AppErrorReason.alreadyExists => l.errorReasonAlreadyExists(args['name'] ?? ''),
    AppErrorReason.selectFile => l.errorReasonSelectFile,
    AppErrorReason.vaultLocked => l.errorReasonVaultLocked,
    AppErrorReason.noActiveProfile => l.errorReasonNoActiveProfile,
    AppErrorReason.noVault => l.errorReasonNoVault,
    AppErrorReason.vaultExists => l.errorReasonVaultExists,
    AppErrorReason.notABackup => l.errorReasonNotABackup,
    AppErrorReason.backupExtension => l.errorReasonBackupExtension(args['extension'] ?? 'ccbackup'),
    AppErrorReason.backupFolderRequired => l.errorReasonBackupFolderRequired,
    AppErrorReason.keepAtLeastOne => l.errorReasonKeepAtLeastOne,
    AppErrorReason.syncedProfileRequired => l.errorReasonSyncedProfileRequired,
    AppErrorReason.localProfileNoAccount => l.errorReasonLocalProfileNoAccount,
    AppErrorReason.noDeviceRegistered => l.errorReasonNoDeviceRegistered,
    AppErrorReason.trustedDeviceRequired => l.errorReasonTrustedDeviceRequired,
    AppErrorReason.currentPassphraseWrong => l.errorReasonCurrentPassphraseWrong,
    AppErrorReason.currentPasswordWrong => l.errorReasonCurrentPasswordWrong,
    AppErrorReason.accountMismatch => l.errorReasonAccountMismatch,
  };
}

/// Localized text for a model [ValidationError] (`field` = snake_case
/// cc-models field name, `rule` = its English reason).
String validationMessage(AppLocalizations l, String field, String rule) {
  if (field == 'shared_host' && (rule == 'endpoint_changed_review' || rule == 'exact_endpoint_confirmation_required')) {
    return l.sharingEndpointChanged;
  }
  if (field == 'shared_host' && rule == 'shared_route_requires_verified_plan') return l.sharingTunnelVerify;
  if (field == 'auto_start' && rule == 'shared_host_requires_verification') return l.sharingTunnelVerify;
  if (field == 'jump_chain') return l.validationJumpChainSelf;
  final name = switch (field) {
    'name' => l.validationFieldName,
    'address' => l.validationFieldAddress,
    'port' => l.validationFieldPort,
    'bind_host' => l.validationFieldBindHost,
    'bind_port' => l.validationFieldBindPort,
    'target_host' => l.validationFieldTargetHost,
    'target_port' => l.validationFieldTargetPort,
    _ => field,
  };
  return switch (rule) {
    'must not be empty' || 'required' => l.validationRequired(name), // l10n-ignore: cc-models rule id
    'must not contain whitespace' => l.validationNoWhitespace(name), // l10n-ignore: cc-models rule id
    'must be 1..=65535' => l.validationPortRange(name), // l10n-ignore: cc-models rule id
    _ => l.validationInvalid(name),
  };
}
