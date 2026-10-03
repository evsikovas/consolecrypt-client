// ignore: unused_import
import 'package:intl/intl.dart' as intl;

import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for English (`en`).
class AppLocalizationsEn extends AppLocalizations {
  AppLocalizationsEn([String locale = 'en']) : super(locale);

  @override
  String get commonCancel => 'Cancel';

  @override
  String get commonSave => 'Save';

  @override
  String get commonDelete => 'Delete';

  @override
  String get commonEdit => 'Edit';

  @override
  String get commonRename => 'Rename';

  @override
  String get commonClose => 'Close';

  @override
  String get commonCopy => 'Copy';

  @override
  String get commonBack => 'Back';

  @override
  String get commonNext => 'Next';

  @override
  String get commonContinue => 'Continue';

  @override
  String get commonDone => 'Done';

  @override
  String get commonRetry => 'Retry';

  @override
  String get commonAdd => 'Add';

  @override
  String get commonCreate => 'Create';

  @override
  String get commonOpen => 'Open';

  @override
  String get commonRemove => 'Remove';

  @override
  String get commonChange => 'Change';

  @override
  String get commonShow => 'Show';

  @override
  String get commonHide => 'Hide';

  @override
  String get commonSearch => 'Search';

  @override
  String get commonOk => 'OK';

  @override
  String get commonReview => 'Review';

  @override
  String get commonConnect => 'Connect';

  @override
  String get commonNever => 'Never';

  @override
  String get commonNone => 'None';

  @override
  String get commonUnknown => 'Unknown';

  @override
  String get commonName => 'Name';

  @override
  String get commonDescription => 'Description';

  @override
  String get commonOptional => 'Optional';

  @override
  String get tagsLabel => 'Tags';

  @override
  String get tagsHint => 'Type a tag and press Enter';

  @override
  String get tagsAdd => 'Add tag';

  @override
  String copiedNotice(String what) {
    return '$what copied.';
  }

  @override
  String copiedSecretNotice(String what, int seconds) {
    return '$what copied. The clipboard will be cleared in $seconds s.';
  }

  @override
  String get copyWhatSecret => 'Secret';

  @override
  String get copyWhatText => 'Text';

  @override
  String get copyWhatCommand => 'Command';

  @override
  String get copyWhatPublicKey => 'Public key';

  @override
  String get copyWhatPassword => 'Password';

  @override
  String get copyWhatRecoveryWords => 'Recovery words';

  @override
  String get copyWhatFingerprint => 'Fingerprint';

  @override
  String get timeJustNow => 'just now';

  @override
  String timeMinutesAgo(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '$count min ago');
    return '$_temp0';
  }

  @override
  String timeHoursAgo(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '$count h ago');
    return '$_temp0';
  }

  @override
  String timeDaysAgo(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '$count d ago');
    return '$_temp0';
  }

  @override
  String get timeExpired => 'expired';

  @override
  String get timeInLessThanMinute => 'in <1 min';

  @override
  String timeInMinutes(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: 'in $count min');
    return '$_temp0';
  }

  @override
  String timeInHours(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: 'in $count h');
    return '$_temp0';
  }

  @override
  String timeInDays(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: 'in $count d');
    return '$_temp0';
  }

  @override
  String get unitBytes => 'B';

  @override
  String get unitKibibytes => 'KiB';

  @override
  String get unitMebibytes => 'MiB';

  @override
  String get unitGibibytes => 'GiB';

  @override
  String get unitTebibytes => 'TiB';

  @override
  String formatSize(String value, String unit) {
    return '$value $unit';
  }

  @override
  String get languageLabel => 'Language';

  @override
  String get languageSystem => 'System';

  @override
  String languageSystemResolved(String language) {
    return 'System ($language)';
  }

  @override
  String get languageSwitcherTooltip => 'Language / Язык';

  @override
  String get languageSettingsHelp =>
      'Applies immediately. \"System\" follows the OS language and falls back to English.';

  @override
  String get riskReadOnly => 'Read-only';

  @override
  String get riskModifying => 'Modifying';

  @override
  String get riskDestructive => 'Destructive';

  @override
  String get riskUnknown => 'Unknown';

  @override
  String riskSemantics(String level) {
    return 'Risk: $level';
  }

  @override
  String verificationCodeSemantics(String groups) {
    return 'Verification code $groups';
  }

  @override
  String get strengthVeryWeak => 'Very weak';

  @override
  String get strengthWeak => 'Weak';

  @override
  String get strengthFair => 'Fair';

  @override
  String get strengthStrong => 'Strong';

  @override
  String get strengthVeryStrong => 'Very strong';

  @override
  String strengthLabelWithHint(String label, String hint) {
    return '$label — $hint';
  }

  @override
  String strengthHintEmpty(int min) {
    return 'Use at least 4 random words or $min+ characters';
  }

  @override
  String get strengthHintSequence => 'Avoid keyboard or alphabet sequences';

  @override
  String get strengthHintCommon => 'Avoid common passwords and well-known phrases';

  @override
  String strengthHintTooShort(int min) {
    return 'Use at least $min characters';
  }

  @override
  String get strengthHintGood => 'Good. Store it somewhere safe — nobody can reset it for you.';

  @override
  String get strengthHintAddMore => 'Add more random words or characters';

  @override
  String get backupFrequencyDaily => 'Daily';

  @override
  String get backupFrequencyWeekly => 'Weekly';

  @override
  String get aiProviderKindOpenaiCompatible => 'OpenAI-compatible';

  @override
  String get privacyProfileStrict => 'Strict';

  @override
  String get privacyProfileStandard => 'Standard';

  @override
  String get privacyProfileLocal => 'Local';

  @override
  String get privacyProfileStrictDescription => 'Redact secrets and also IPs, hostnames, usernames, DB names';

  @override
  String get privacyProfileStandardDescription => 'Redact secrets, keep host metadata';

  @override
  String get privacyProfileLocalDescription => 'For local models: more context; secrets still redacted';

  @override
  String get credentialKindPassword => 'Password';

  @override
  String get credentialKindSshKey => 'SSH key';

  @override
  String get credentialKindSshCertificate => 'SSH certificate';

  @override
  String get credentialKindOsSshAgent => 'OS SSH agent';

  @override
  String get credentialKindFido2 => 'FIDO2 security key';

  @override
  String get credentialKindExternalAgent => 'External agent';

  @override
  String get hostKeyPolicyAsk => 'Ask';

  @override
  String get hostKeyPolicyStrict => 'Strict';

  @override
  String get hostKeyPolicyAcceptNew => 'Accept new';

  @override
  String get hostKeyPolicyAskDescription => 'Confirm unknown host keys; changed keys always fail';

  @override
  String get hostKeyPolicyStrictDescription => 'Only accept keys already in known hosts';

  @override
  String get hostKeyPolicyAcceptNewDescription => 'Record unknown keys automatically; changed keys always fail';

  @override
  String get sshBackendNative => 'Built-in (russh)';

  @override
  String get sshBackendOpenSsh => 'System OpenSSH (compatibility)';

  @override
  String get knownHostSourceTofu => 'Accepted on first connect';

  @override
  String get knownHostSourceManual => 'Added manually';

  @override
  String get knownHostSourceImported => 'Imported from known_hosts';

  @override
  String get knownHostSourceCertAuthority => 'Certificate authority';

  @override
  String get historyModeLocalOnly => 'Local only';

  @override
  String get historyModeEncryptedSync => 'Encrypted sync';

  @override
  String get historyModeDisabled => 'Disabled';

  @override
  String get historyModeLocalOnlyDescription => 'History stays on this device (default)';

  @override
  String get historyModeEncryptedSyncDescription => 'History is synced end-to-end encrypted';

  @override
  String get historyModeDisabledDescription => 'No command history is recorded';

  @override
  String get profileKindLocal => 'Local only';

  @override
  String get profileKindSynced => 'Synced';

  @override
  String profileSyncedSubtitle(String account, String server) {
    return '$account · $server';
  }

  @override
  String get profileUnknownAccount => 'account';

  @override
  String get profileUnknownServer => 'server';

  @override
  String get enableSyncStepAuthenticating => 'Signing in and registering this device';

  @override
  String get enableSyncStepCreatingRemoteVault => 'Creating the vault on the server (same vault ID)';

  @override
  String get enableSyncStepReconnecting => 'Vault already exists on the server — merging';

  @override
  String get enableSyncStepUploading => 'Uploading encrypted objects';

  @override
  String get enableSyncStepFinishing => 'Finishing';

  @override
  String get enableSyncStepDone => 'Sync enabled';

  @override
  String get enableSyncStepFailed => 'Failed';

  @override
  String get tunnelKindLocal => 'Local';

  @override
  String get tunnelKindRemote => 'Remote';

  @override
  String get tunnelKindDynamic => 'Dynamic (SOCKS5)';

  @override
  String get tunnelKindLocalDescription => 'Forward a local port to a host reachable from the server';

  @override
  String get tunnelKindRemoteDescription => 'Expose a local service on a port of the server';

  @override
  String get tunnelKindDynamicDescription => 'Local SOCKS5 proxy through the server';

  @override
  String get snippetSourceUser => 'User';

  @override
  String get snippetSourceAi => 'AI';

  @override
  String get snippetSourceImported => 'Imported';

  @override
  String get snippetSourceHistory => 'History';

  @override
  String get platformSystemAuthentication => 'system authentication';

  @override
  String get platformDeviceNameMac => 'My Mac';

  @override
  String get platformDeviceNamePc => 'My PC';

  @override
  String get platformDeviceNameOther => 'This device';

  @override
  String get commandPalette => 'Command Palette';

  @override
  String get commandNewTerminalTab => 'New Terminal Tab';

  @override
  String get commandCloseTerminalTab => 'Close Terminal Tab';

  @override
  String get commandNewHost => 'New Host';

  @override
  String get commandLockVault => 'Lock Vault';

  @override
  String get commandOpenSettings => 'Settings';

  @override
  String get commandNewTabPickerTitle => 'New terminal tab';

  @override
  String get menuFile => 'File';

  @override
  String get menuView => 'View';

  @override
  String get menuWindow => 'Window';

  @override
  String get navHosts => 'Hosts';

  @override
  String get navGroups => 'Groups';

  @override
  String get navCredentials => 'Credentials';

  @override
  String get navTerminal => 'Terminal';

  @override
  String get navSftp => 'SFTP';

  @override
  String get navTunnels => 'Tunnels';

  @override
  String get navSnippets => 'Snippets';

  @override
  String get navAiChat => 'AI Chat';

  @override
  String get navDevices => 'Devices';

  @override
  String get navSync => 'Sync';

  @override
  String get navBackups => 'Backups';

  @override
  String get navSettings => 'Settings';

  @override
  String shellLockVaultTooltip(String shortcut) {
    return 'Lock vault ($shortcut)';
  }

  @override
  String get shellSearchPlaceholder => 'Search hosts, snippets or ask AI…';

  @override
  String get shellOfflineTooltip =>
      'The sync server is unreachable. Hosts, keys and SSH keep working; changes are queued.';

  @override
  String shellNewTabTooltip(String shortcut) {
    return 'New terminal tab ($shortcut)';
  }

  @override
  String shellApprovalBannerTitle(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count device approval requests',
      one: 'Device approval requested',
    );
    return '$_temp0';
  }

  @override
  String shellApprovalBannerMessage(String device, String platform, String ago) {
    return '\"$device\" ($platform) asked to join your vault $ago. Approve only after comparing verification codes.';
  }

  @override
  String get syncStateLocalOnly => 'Local only';

  @override
  String get syncStateSynced => 'Synced';

  @override
  String get syncStateSyncedShort => 'Sync';

  @override
  String syncStateSyncedAgo(String ago) {
    return 'Synced $ago';
  }

  @override
  String get syncStateSyncing => 'Syncing…';

  @override
  String get syncStateOffline => 'Offline';

  @override
  String syncStateOfflinePending(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: 'Offline · $count pending');
    return '$_temp0';
  }

  @override
  String get syncStateError => 'Sync error';

  @override
  String get syncStatePaused => 'Sync paused';

  @override
  String get syncIndicatorLocalTooltip => 'This profile is not synced. Enable sync in Settings.';

  @override
  String syncIndicatorPendingTooltip(String state, int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$state — $count local changes waiting',
      one: '$state — 1 local change waiting',
    );
    return '$_temp0';
  }

  @override
  String get errorUnknown => 'Something went wrong. Please try again.';

  @override
  String get errorInvalidCredentials => 'Wrong e-mail or password.';

  @override
  String get errorServerUnreachable => 'Server unreachable. Check the URL and your connection.';

  @override
  String get errorIncompatibleServer =>
      'This server version is not compatible with this app. Update the app or the server.';

  @override
  String get errorRegistrationClosed => 'Registration is closed on this server.';

  @override
  String get errorEmailTaken => 'An account with this e-mail already exists.';

  @override
  String get errorWrongPassphrase => 'Wrong passphrase.';

  @override
  String get errorInvalidRecoveryKey => 'This Recovery Key does not open the vault.';

  @override
  String get errorDeviceNotTrusted => 'This device is not trusted for the vault yet.';

  @override
  String get errorOsAuthFailed => 'System authentication failed or was cancelled.';

  @override
  String get errorVerificationMismatch => 'Verification codes do not match — approval refused.';

  @override
  String get errorRequestExpired => 'The request has expired.';

  @override
  String get errorValidation => 'Check the entered values.';

  @override
  String get errorNotFound => 'Not found. It may have been deleted.';

  @override
  String get errorConflict => 'This was changed elsewhere. Reload and try again.';

  @override
  String get errorOffline => 'You are offline. The change will be retried when the connection is back.';

  @override
  String get errorCancelled => 'Cancelled.';

  @override
  String get errorHostKeyRejected => 'The host key was rejected.';

  @override
  String get errorHostKeyChanged => 'The host key has changed. The connection was blocked.';

  @override
  String get errorAuthFailed => 'SSH authentication failed.';

  @override
  String get errorUnsupported => 'This action is not supported here.';

  @override
  String get errorInternal => 'Internal error. Please try again.';

  @override
  String get errorSessionExpired => 'Your session has expired. Sign in again.';

  @override
  String get errorDeviceRevoked => 'This device has been revoked. Sign in again to register it.';

  @override
  String get errorEmailNotVerified => 'Verify your e-mail address first.';

  @override
  String get errorForbidden => 'You do not have access to this.';

  @override
  String get errorRateLimited => 'Too many attempts. Try again later.';

  @override
  String errorRateLimitedRetry(int seconds) {
    String _temp0 = intl.Intl.pluralLogic(
      seconds,
      locale: localeName,
      other: 'Too many attempts. Try again in $seconds s.',
    );
    return '$_temp0';
  }

  @override
  String get errorPayloadTooLarge => 'The data is too large for the server.';

  @override
  String get errorInvalidProof => 'Cryptographic check failed. The request was rejected.';

  @override
  String get errorServerUnavailable => 'The server is temporarily unavailable. Try again later.';

  @override
  String get errorReasonNameRequired => 'Enter a name.';

  @override
  String get errorReasonProfileNameRequired => 'Enter a profile name.';

  @override
  String get errorReasonPasswordRequired => 'Enter the password.';

  @override
  String get errorReasonKeyPassphraseRequired => 'Enter the key passphrase.';

  @override
  String get errorReasonAgentPathRequired => 'Enter the agent socket path or pipe name.';

  @override
  String get errorReasonInvalidEmail => 'Enter a valid e-mail address.';

  @override
  String errorReasonAccountPasswordTooShort(int min) {
    String _temp0 = intl.Intl.pluralLogic(
      min,
      locale: localeName,
      other: 'Account password must be at least $min characters.',
    );
    return '$_temp0';
  }

  @override
  String get errorReasonWeakPassphrase => 'Choose a stronger passphrase.';

  @override
  String get errorReasonInvalidBaseUrl => 'Enter a valid base URL.';

  @override
  String get errorReasonChatModelRequired => 'Enter the chat model name.';

  @override
  String get errorReasonTemplateRequired => 'Enter the command template.';

  @override
  String errorReasonMissingVariables(String names) {
    return 'Fill in: $names';
  }

  @override
  String get errorReasonInvalidKey => 'This is not a valid private key.';

  @override
  String get errorReasonNotACertificate => 'Not an OpenSSH certificate line.';

  @override
  String get errorReasonNotAgentCredential => 'This is not an agent credential.';

  @override
  String get errorReasonKeyHasNoPassphrase => 'This key has no passphrase.';

  @override
  String get errorReasonUnsupportedKeyAlgorithm => 'Generate Ed25519 or RSA 3072/4096 keys.';

  @override
  String get errorReasonGroupCycle => 'A group cannot be inside itself.';

  @override
  String get errorReasonJumpChainEmpty => 'A jump profile needs at least one hop.';

  @override
  String get errorReasonHostNotFound => 'Host not found.';

  @override
  String get errorReasonCredentialNotFound => 'The credential no longer exists.';

  @override
  String get errorReasonProfileNotFound => 'Profile not found.';

  @override
  String get errorReasonDeviceNotFound => 'Device not found.';

  @override
  String get errorReasonTunnelNotFound => 'Tunnel not found.';

  @override
  String get errorReasonProviderNotFound => 'AI provider not found.';

  @override
  String get errorReasonRequestNotFound => 'The request no longer exists.';

  @override
  String get errorReasonSecretNotStored => 'No secret is stored for this credential.';

  @override
  String get errorReasonSessionClosed => 'The session is closed.';

  @override
  String errorReasonDirectoryNotFound(String path) {
    return 'No such directory: $path';
  }

  @override
  String get errorReasonDirectoryNotEmpty => 'The directory is not empty.';

  @override
  String errorReasonAlreadyExists(String name) {
    return '\"$name\" already exists.';
  }

  @override
  String get errorReasonSelectFile => 'Select a file.';

  @override
  String get errorReasonVaultLocked => 'The vault is locked.';

  @override
  String get errorReasonNoActiveProfile => 'No active profile.';

  @override
  String get errorReasonNoVault => 'This profile has no vault yet.';

  @override
  String get errorReasonVaultExists => 'A vault already exists.';

  @override
  String get errorReasonNotABackup => 'Not a ConsoleCrypt backup (or the file is missing).';

  @override
  String errorReasonBackupExtension(String extension) {
    return 'Backup files must end with .$extension';
  }

  @override
  String get errorReasonBackupFolderRequired => 'Choose a backup folder first.';

  @override
  String get errorReasonKeepAtLeastOne => 'Keep at least one backup.';

  @override
  String get errorReasonSyncedProfileRequired => 'This needs a synced profile.';

  @override
  String get errorReasonLocalProfileNoAccount => 'Local profiles have no account.';

  @override
  String get errorReasonNoDeviceRegistered => 'No device is registered.';

  @override
  String get errorReasonTrustedDeviceRequired => 'Only an unlocked trusted device can do this.';

  @override
  String get errorReasonCurrentPassphraseWrong => 'The current passphrase is wrong.';

  @override
  String get errorReasonCurrentPasswordWrong => 'The current password is wrong.';

  @override
  String get errorReasonAccountMismatch => 'This profile belongs to a different account.';

  @override
  String get validationFieldName => 'Name';

  @override
  String get validationFieldAddress => 'Address';

  @override
  String get validationFieldPort => 'Port';

  @override
  String get validationFieldBindHost => 'Bind address';

  @override
  String get validationFieldBindPort => 'Bind port';

  @override
  String get validationFieldTargetHost => 'Target host';

  @override
  String get validationFieldTargetPort => 'Target port';

  @override
  String validationRequired(String field) {
    return '$field: required';
  }

  @override
  String validationNoWhitespace(String field) {
    return '$field: must not contain spaces';
  }

  @override
  String validationPortRange(String field) {
    return '$field: must be 1–65535';
  }

  @override
  String validationInvalid(String field) {
    return '$field: invalid value';
  }

  @override
  String get validationJumpChainSelf => 'A host cannot jump through itself.';

  @override
  String get serverUrlEmpty => 'Enter the URL of your ConsoleCrypt server';

  @override
  String get serverUrlNotAUrl => 'Enter a full URL, e.g. https://sync.example.org';

  @override
  String get serverUrlInsecure => 'Use https:// (plain http is only allowed for localhost)';

  @override
  String get loginTitle => 'Connect to your server';

  @override
  String get loginTitleReauth => 'Sign in again';

  @override
  String get loginSubtitle => 'Use the URL of your self-hosted ConsoleCrypt server.';

  @override
  String get loginSubtitleReauth => 'Your session for this profile ended. Local data stays on this device.';

  @override
  String get loginServerUrlLabel => 'Server URL';

  @override
  String get loginCheckServer => 'Check';

  @override
  String loginServerInfoRegistrationOpen(String serverVersion, String protocolVersion) {
    return 'ConsoleCrypt server $serverVersion · protocol $protocolVersion · registration open';
  }

  @override
  String loginServerInfoInviteOnly(String serverVersion, String protocolVersion) {
    return 'ConsoleCrypt server $serverVersion · protocol $protocolVersion · invite only';
  }

  @override
  String get loginModeSignIn => 'Sign in';

  @override
  String get loginModeRegister => 'Create account';

  @override
  String get loginEmailLabel => 'E-mail';

  @override
  String get loginEmailRequired => 'Enter your e-mail address';

  @override
  String get loginPasswordLabel => 'Account password';

  @override
  String get loginPasswordHelperRegister => 'At least 10 characters. Different from your vault passphrase.';

  @override
  String get loginPasswordRequired => 'Enter your password';

  @override
  String get loginPasswordTooShort => 'At least 10 characters';

  @override
  String get loginPasswordRepeatLabel => 'Repeat password';

  @override
  String get loginPasswordsDoNotMatch => 'Passwords do not match';

  @override
  String get loginDeviceNameLabel => 'Name of this device';

  @override
  String get loginDeviceNameHelper => 'Shown in your device list, e.g. \"Work MacBook\".';

  @override
  String get loginDeviceNameRequired => 'Enter a device name';

  @override
  String get loginSubmitSignIn => 'Sign in';

  @override
  String get loginSubmitRegister => 'Create account';

  @override
  String get loginForgotPassword => 'Forgot account password?';

  @override
  String get loginForgotNeedsServerAndEmail => 'Enter the server URL and your e-mail first';

  @override
  String get loginResetEmailSent =>
      'If the account exists, a reset e-mail is on its way. A reset restores account access only — your vault still needs your passphrase or Recovery Key.';

  @override
  String get loginPassphraseNeverLeaves =>
      'Your vault passphrase never leaves this device; the server only stores encrypted data.';

  @override
  String get profileSwitcherTooltip => 'Switch profile';

  @override
  String get profileSwitcherAddProfileMenu => 'Add profile…';

  @override
  String get profileSwitcherManageProfiles => 'Manage profiles';

  @override
  String profileSwitcherSwitchTo(String name) {
    return 'Switch to $name';
  }

  @override
  String get profileSwitcherAddProfile => 'Add profile';

  @override
  String get backupsTitle => 'Backups';

  @override
  String get backupsSubtitle =>
      'Encrypted .ccbackup files contain only ciphertext and envelopes — restorable with your passphrase or Recovery Key on any machine.';

  @override
  String get backupsRestoreAction => 'Restore…';

  @override
  String get backupsExportAction => 'Export backup…';

  @override
  String backupsSavedSnack(String fileName, String size) {
    return 'Backup saved: $fileName ($size)';
  }

  @override
  String get backupsNoCloudCopyTitle => 'This profile has no cloud copy';

  @override
  String get backupsNoCloudCopyMessage => 'Backups are the only way to get your data back if this device is lost.';

  @override
  String get backupsSyncedMessage =>
      'This profile is synced, so your server holds an encrypted copy. Offline backups still protect against server loss or accidental deletion.';

  @override
  String get backupsRecentTitle => 'Recent backups';

  @override
  String get backupsRecentEmpty => 'No backups made from this profile yet.';

  @override
  String backupsObjectCount(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '$count objects', one: '$count object');
    return '$_temp0';
  }

  @override
  String get backupsAutomatic => 'automatic';

  @override
  String get backupsAutoTitle => 'Automatic backups';

  @override
  String get backupsAutoSubtitle =>
      'Writes an encrypted backup to a folder you choose (e.g. an external drive or a synced folder).';

  @override
  String get backupsFolderLabel => 'Folder';

  @override
  String get backupsFolderNotChosen => 'Not chosen';

  @override
  String get backupsChooseFolder => 'Choose…';

  @override
  String get backupsFrequencyLabel => 'Frequency';

  @override
  String get backupsKeepLabel => 'Keep';

  @override
  String backupsKeepLast(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'last $count backups',
      one: 'last $count backup',
    );
    return '$_temp0';
  }

  @override
  String get backupsLastRunLabel => 'Last run';

  @override
  String get backupsNextRunLabel => 'Next run';

  @override
  String get backupsLastRunFailed => 'The last automatic backup failed';

  @override
  String backupsWrittenTo(String folder) {
    return 'Backup written to $folder';
  }

  @override
  String get backupsBackUpNow => 'Back up now';

  @override
  String get restoreTitle => 'Restore from backup';

  @override
  String get restoreSubtitle =>
      'Restores an encrypted .ccbackup file into a new local profile on this device. Existing profiles are not touched.';

  @override
  String get restoreFileLabel => 'Backup file';

  @override
  String get restoreBrowse => 'Browse…';

  @override
  String get restoreVaultIdLabel => 'Vault ID';

  @override
  String get restoreCreatedLabel => 'Created';

  @override
  String get restoreObjectsLabel => 'Objects';

  @override
  String restoreObjectsValue(String count, String size) {
    return '$count ($size)';
  }

  @override
  String get restoreWrittenByLabel => 'Written by';

  @override
  String restoreWrittenByValue(String appVersion, int formatVersion) {
    return 'ConsoleCrypt $appVersion · format v$formatVersion';
  }

  @override
  String get restoreUnlockPassphrase => 'Vault passphrase';

  @override
  String get restoreUnlockRecoveryKey => 'Recovery Key';

  @override
  String get restorePassphraseLabel => 'Vault passphrase of the backup';

  @override
  String get restoreRecoveryInputLabel => '24 words or QR text';

  @override
  String get restoreNewPassphraseLabel => 'New vault passphrase';

  @override
  String get restoreProfileNameLabel => 'Profile name';

  @override
  String get restoreProfileNameHint => 'e.g. Personal (restored)';

  @override
  String get restoreOpenBackup => 'Open backup';

  @override
  String get restoreIntoNewProfile => 'Restore into new profile';

  @override
  String get welcomeDefaultProfileName => 'Personal';

  @override
  String get welcomeTitle => 'Welcome to ConsoleCrypt';

  @override
  String get welcomeTitleAddProfile => 'Add a profile';

  @override
  String get welcomeSubtitle =>
      'An end-to-end encrypted SSH client. Hosts, keys, passwords and snippets are encrypted on this device with a key only you control.';

  @override
  String get welcomeLocalTitle => 'Use locally (no account)';

  @override
  String get welcomeLocalBody =>
      'Everything stays encrypted on this device. No server, no account, no network traffic for sync. You can enable sync later.';

  @override
  String get welcomeProfileNameLabel => 'Profile name';

  @override
  String get welcomeLocalAction => 'Use locally';

  @override
  String get welcomeServerTitle => 'Connect to a server';

  @override
  String get welcomeServerBody =>
      'Sync end-to-end encrypted between your machines through your own self-hosted ConsoleCrypt server. The server never sees your data or keys.';

  @override
  String get welcomeServerAction => 'Sign in or create account';

  @override
  String get welcomeRestoreFromBackup => 'Restore from a backup file (.ccbackup)';

  @override
  String get welcomeProfilesHint =>
      'Several profiles can coexist (e.g. a local personal vault and a synced work vault); switch between them from the sidebar.';

  @override
  String get credentialsTitle => 'Credentials';

  @override
  String get credentialsSubtitle =>
      'Passwords and private keys are stored as separate encrypted secrets and never shown unless you reveal them.';

  @override
  String get credentialsNew => 'New credential';

  @override
  String get credentialsNewPassword => 'Password';

  @override
  String get credentialsNewGenerateKey => 'Generate SSH key…';

  @override
  String get credentialsNewImportKey => 'Import OpenSSH key…';

  @override
  String get credentialsNewCertificate => 'SSH certificate…';

  @override
  String get credentialsNewAgent => 'SSH agent…';

  @override
  String get credentialsEmptyTitle => 'No credentials yet';

  @override
  String get credentialsEmptyMessage => 'Generate an Ed25519 key, import an existing OpenSSH key, or store a password.';

  @override
  String credentialsListUser(String username) {
    return 'user $username';
  }

  @override
  String get credentialsListPassphraseRemembered => 'passphrase remembered';

  @override
  String get credentialsListPassphraseProtected => 'passphrase-protected';

  @override
  String credentialsListUsedBy(int count) {
    return 'used by $count';
  }

  @override
  String credentialsDeleteTitle(String name) {
    return 'Delete $name?';
  }

  @override
  String get credentialsDeleteMessage =>
      'The credential and its secret are deleted from the vault. Hosts that use it will need another credential.';

  @override
  String get credentialsDetailsType => 'Type';

  @override
  String get credentialsDetailsUsername => 'Username';

  @override
  String get credentialsDetailsAlgorithm => 'Algorithm';

  @override
  String get credentialsDetailsFingerprint => 'Fingerprint';

  @override
  String get credentialsDetailsPassphraseRemembered => 'Remembered (stored as a separate encrypted secret)';

  @override
  String get credentialsDetailsPassphraseAsked => 'Asked when connecting — the key keeps its own protection';

  @override
  String get credentialsDetailsAgentSocket => 'Agent socket';

  @override
  String get credentialsDetailsCreated => 'Created';

  @override
  String get credentialsDetailsPublicKey => 'Public key';

  @override
  String get credentialsCopyPublicKey => 'Copy public key';

  @override
  String get credentialsDetailsCertificate => 'Certificate';

  @override
  String get credentialsReveal => 'Reveal';

  @override
  String get credentialsRevealHint =>
      'Revealed values hide again after 20 s; copied values are cleared from the clipboard automatically.';

  @override
  String get credentialDialogNewPasswordTitle => 'New password';

  @override
  String get credentialDialogUsernameOptional => 'Username (optional)';

  @override
  String get credentialDialogPasswordLabel => 'Password';

  @override
  String get credentialDialogKeyPassphrase => 'Key passphrase';

  @override
  String get credentialDialogRememberPassphrase => 'Remember SSH key passphrase';

  @override
  String get credentialDialogAgentTitle => 'SSH agent';

  @override
  String get credentialDialogAgentDefaultName => 'SSH agent';

  @override
  String get credentialDialogAgentPathLabel => 'Socket path or pipe name';

  @override
  String credentialDialogAgentPathHint(String unixPath, String windowsPath) {
    return '$unixPath  or  $windowsPath';
  }

  @override
  String get credentialDialogAgentNotice => 'Keys stay in the agent; ConsoleCrypt never sees them.';

  @override
  String get credentialGenerateTitle => 'Generate SSH key';

  @override
  String get credentialGenerateAction => 'Generate';

  @override
  String credentialGenerateRecommended(String algorithm) {
    return '$algorithm (recommended)';
  }

  @override
  String get credentialGenerateRsaNotice =>
      'RSA is for compatibility with older servers; generation takes a few seconds.';

  @override
  String get credentialGenerateCommentLabel => 'Comment (optional)';

  @override
  String get credentialGeneratePassphraseLabel => 'Key passphrase (optional)';

  @override
  String get credentialGeneratePassphraseHelper => 'Protects the key itself, in addition to vault encryption.';

  @override
  String get credentialGenerateRepeatPassphrase => 'Repeat key passphrase';

  @override
  String get credentialGeneratePassphraseMismatch => 'Passphrases do not match';

  @override
  String get credentialGenerateRememberSubtitle =>
      'Stored as a separate encrypted secret in the vault, so you are not asked when connecting.';

  @override
  String get credentialGenerateDoneTitle => 'Key generated';

  @override
  String get credentialGenerateDoneMessage => 'Add this public key to ~/.ssh/authorized_keys on your servers:';

  @override
  String get credentialImportTitle => 'Import OpenSSH key';

  @override
  String get credentialImportTitleWithCertificate => 'Import key + certificate';

  @override
  String get credentialImportAction => 'Import';

  @override
  String get credentialImportPrivateKey => 'Private key';

  @override
  String get credentialImportUnknownAlgorithm => 'Key';

  @override
  String get credentialImportEncryptedNotice => 'Passphrase-protected: it stays encrypted in your vault.';

  @override
  String get credentialImportInvalidKey => 'Invalid key';

  @override
  String get credentialImportRememberSubtitle =>
      'Off: you are asked for it on each connection. On: stored as a separate encrypted secret.';

  @override
  String get credentialImportCertificateLabel => 'OpenSSH certificate';

  @override
  String approveDeviceTitle(String name) {
    return 'Approve \"$name\"?';
  }

  @override
  String approveDeviceStepShowsCode(String name) {
    return '1. On \"$name\", ConsoleCrypt shows a verification code.';
  }

  @override
  String get approveDeviceStepCompare => '2. Compare it with the code computed here, group by group:';

  @override
  String approveDeviceCodesMatchCheckbox(String name) {
    return 'All 6 groups match the code shown on \"$name\"';
  }

  @override
  String get approveDeviceWarning =>
      'Approving gives this device the key to your vault. Never approve a device you are not setting up yourself right now.';

  @override
  String get approveDeviceCodesDontMatch => 'Codes don\'t match';

  @override
  String get approveDeviceApprove => 'Approve';

  @override
  String approveDeviceApproved(String name) {
    return '$name can now unlock your vault';
  }

  @override
  String get approveDeviceRejectedTitle => 'Request rejected';

  @override
  String get approveDeviceRejectedMessage =>
      'The codes did not match, so the device was NOT approved. Someone — possibly a compromised server — may have tried to add their own device to your account.\n\nIf you did not start this, change your account password and review your devices.';

  @override
  String get devicesTitle => 'Devices';

  @override
  String devicesRevokeTitle(String name) {
    return 'Revoke $name?';
  }

  @override
  String get devicesRevokeMessage =>
      'The device is signed out immediately, loses access to all your vaults and can no longer sync. This cannot be undone — to use it again it must be approved as a new device.\n\nRevocation protects the future, not the past: data the device already decrypted cannot be erased remotely.';

  @override
  String get devicesRevokeConfirm => 'Revoke device';

  @override
  String devicesRevoked(String name) {
    return '$name revoked';
  }

  @override
  String get devicesRenameTitle => 'Rename device';

  @override
  String get devicesLocalTitle => 'Devices apply to synced profiles';

  @override
  String get devicesLocalMessage =>
      'This profile is local-only: it exists on this device and nowhere else. Enable sync to use the vault on several machines with device approval.';

  @override
  String get devicesEnableSync => 'Enable sync…';

  @override
  String devicesSubtitle(String email) {
    return 'Devices signed in to $email. Only trusted devices can decrypt your vault.';
  }

  @override
  String get devicesSubtitleNoEmail =>
      'Devices signed in to your account. Only trusted devices can decrypt your vault.';

  @override
  String get devicesRefresh => 'Refresh';

  @override
  String devicesPendingTitle(String name) {
    return '\"$name\" wants to access your vault';
  }

  @override
  String devicesPendingMeta(String platform, String ago, String remaining) {
    return '$platform · requested $ago · expires $remaining';
  }

  @override
  String devicesPendingMetaExpired(String platform, String ago) {
    return '$platform · requested $ago · expires expired';
  }

  @override
  String get devicesPendingHint =>
      'Approve only if you are setting up this device right now and the verification codes match.';

  @override
  String get devicesRequestRejected => 'Request rejected';

  @override
  String get devicesReject => 'Reject';

  @override
  String get devicesReviewApprove => 'Review & approve';

  @override
  String get devicesStatusRevoked => 'Revoked';

  @override
  String get devicesStatusTrusted => 'Trusted';

  @override
  String get devicesStatusAwaitingApproval => 'Awaiting approval';

  @override
  String get devicesStatusNotTrusted => 'Not trusted for this vault';

  @override
  String get devicesThisDevice => 'This device';

  @override
  String devicesLastSeen(String ago) {
    return 'last seen $ago';
  }

  @override
  String devicesAdded(String date) {
    return 'added $date';
  }

  @override
  String devicesRevokedOn(String date) {
    return 'revoked $date';
  }

  @override
  String get devicesRevokeMenu => 'Revoke…';

  @override
  String get syncScreenTitle => 'Sync';

  @override
  String get syncScreenLocalSubtitle => 'This profile has no server and no account. Nothing leaves this device.';

  @override
  String get syncScreenLocalBody =>
      'Enable sync to use this vault on other machines. The vault is uploaded end-to-end encrypted to your self-hosted server; its ID, passphrase and Recovery Kit stay the same.';

  @override
  String get syncScreenEnableSync => 'Enable sync…';

  @override
  String get syncScreenBackups => 'Backups';

  @override
  String syncScreenServer(String url) {
    return 'Server: $url';
  }

  @override
  String get syncScreenSyncNow => 'Sync now';

  @override
  String get syncScreenState => 'State';

  @override
  String get syncScreenStateIdle => 'idle';

  @override
  String get syncScreenStateSyncing => 'syncing';

  @override
  String get syncScreenStateOffline => 'offline';

  @override
  String get syncScreenStateError => 'error';

  @override
  String get syncScreenStatePaused => 'paused';

  @override
  String get syncScreenLastSync => 'Last successful sync';

  @override
  String get syncScreenPendingChanges => 'Pending changes';

  @override
  String get syncScreenServerSequence => 'Server sequence';

  @override
  String get syncScreenNextRetry => 'Next retry';

  @override
  String get syncScreenOfflineMessage =>
      'The server is unreachable. Your hosts, keys, SSH, SFTP and tunnels keep working; changes are stored locally and pushed when the server is back.';

  @override
  String get syncScreenProblems => 'Problems';

  @override
  String get syncScreenIssueRetryable => 'Sync failed — will retry automatically';

  @override
  String get syncScreenIssue => 'Sync failed';

  @override
  String get syncScreenDisconnectTitle => 'Disconnect';

  @override
  String get syncScreenDisconnectBody => 'Stop syncing this profile and keep its data locally.';

  @override
  String get syncScreenDisconnect => 'Disconnect…';

  @override
  String get syncScreenDeveloper => 'Developer (mock backend)';

  @override
  String get syncScreenSimulateOutage => 'Simulate server outage';

  @override
  String get syncDisconnectTitle => 'Disconnect sync?';

  @override
  String get syncDisconnectMessage =>
      'Sync stops and this profile becomes local-only. All data stays on this device. The encrypted copy on the server is left untouched.';

  @override
  String get syncDisconnectRevoke => 'Also revoke this device on the server';

  @override
  String get syncDisconnectConfirm => 'Disconnect (keep data locally)';

  @override
  String get enableSyncDialogTitle => 'Enable sync';

  @override
  String get enableSyncDialogStepServer => 'Step 1 of 3 · Your server';

  @override
  String get enableSyncDialogStepAccount => 'Step 2 of 3 · Account';

  @override
  String get enableSyncDialogStepUpload => 'Step 3 of 3 · Upload';

  @override
  String get enableSyncDialogIntro =>
      'Your vault is uploaded end-to-end encrypted: the server stores only ciphertext and never sees your passphrase or keys. The vault keeps its ID and your Recovery Kit stays valid.';

  @override
  String get enableSyncDialogServerUrl => 'Server URL';

  @override
  String enableSyncDialogServerInfo(String serverVersion, String protocolVersion) {
    return 'ConsoleCrypt server $serverVersion · protocol $protocolVersion';
  }

  @override
  String get enableSyncDialogCreateAccount => 'Create account';

  @override
  String get enableSyncDialogSignIn => 'Sign in';

  @override
  String get enableSyncDialogEmail => 'E-mail';

  @override
  String get enableSyncDialogPassword => 'Account password';

  @override
  String get enableSyncDialogPasswordHelper => 'At least 10 characters. Not your vault passphrase.';

  @override
  String get enableSyncDialogDeviceName => 'Name of this device';

  @override
  String get enableSyncDialogCredentialsRequired => 'Enter your e-mail and password';

  @override
  String enableSyncDialogUploadedObjects(int uploaded, int total) {
    String _temp0 = intl.Intl.pluralLogic(total, locale: localeName, other: '$uploaded of $total encrypted objects');
    return '$_temp0';
  }

  @override
  String get enableSyncDialogDone =>
      'This profile is now synced. Add other devices by signing in there and approving them here with the verification code.';

  @override
  String get enableSyncDialogFailed => 'Enabling sync failed.';

  @override
  String get enableSyncDialogFailedTitle => 'Enabling sync failed';

  @override
  String get enableSyncDialogStart => 'Enable sync';

  @override
  String get enableSyncDialogTryAgain => 'Try again';

  @override
  String get glassSetting => 'Glass';

  @override
  String get glassModeClear => 'Clear';

  @override
  String get glassModeStandard => 'Default';

  @override
  String get glassModeTinted => 'Tinted';

  @override
  String get glassModeSolid => 'Solid';

  @override
  String get glassCapsLockOn => 'Caps Lock is on';

  @override
  String get glassDismiss => 'Dismiss';

  @override
  String get glassCloseTab => 'Close tab';

  @override
  String get glassNewTab => 'New tab';

  @override
  String get glassTabConnected => 'Connected';

  @override
  String get glassTabReconnecting => 'Reconnecting…';

  @override
  String get glassTabDisconnected => 'Disconnected';

  @override
  String get glassVerificationCode => 'Verification code';

  @override
  String glassCodeGroup(int number, String digits) {
    return 'Group $number: $digits';
  }

  @override
  String glassClearsIn(int seconds) {
    return 'Clears in $seconds s';
  }

  @override
  String get glassMoreActions => 'More';

  @override
  String get hostsTitle => 'Hosts';

  @override
  String hostsSubtitle(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count hosts in this vault',
      one: '$count host in this vault',
    );
    return '$_temp0';
  }

  @override
  String get hostsNewHost => 'New host';

  @override
  String get hostsSearchHint => 'Search by name, address, tag or group';

  @override
  String get hostsEmptyTitle => 'No hosts yet';

  @override
  String get hostsNoMatches => 'No matching hosts';

  @override
  String get hostsEmptyMessage => 'Add your first SSH host to connect.';

  @override
  String get hostsAddHost => 'Add host';

  @override
  String get hostsViaJumpHostsTooltip => 'Connects through jump hosts';

  @override
  String get hostsOpenSshTooltip => 'Uses the system OpenSSH backend';

  @override
  String get hostsAssignGroup => 'Add to group…';

  @override
  String get hostsGroupHelp =>
      'Choose one group for this host. Connection settings left unset on the host are inherited from the group.';

  @override
  String get hostsGroupSearchHint => 'Find a group…';

  @override
  String get hostsGroupNoMatches => 'No matching groups';

  @override
  String get hostsMoreTooltip => 'More';

  @override
  String get hostsOpenSftp => 'Open SFTP';

  @override
  String hostsDeleteTitle(String name) {
    return 'Delete $name?';
  }

  @override
  String get hostsDeleteMessage =>
      'The host is removed from this vault (and from your other devices after sync). Jump chains that use it are updated.';

  @override
  String get hostsDeleted => 'Host deleted';

  @override
  String get hostsNewCredentialEntry => '+ New credential…';

  @override
  String get hostEditorLoadingTitle => 'Host';

  @override
  String get hostEditorNewTitle => 'New host';

  @override
  String hostEditorEditTitle(String name) {
    return 'Edit $name';
  }

  @override
  String get hostEditorEditTitleFallback => 'Edit host';

  @override
  String get hostEditorConnectionSection => 'Connection';

  @override
  String get hostEditorNameRequired => 'Enter a name';

  @override
  String get hostEditorAddressLabel => 'Address';

  @override
  String get hostEditorAddressHint => 'hostname or IP';

  @override
  String get hostEditorAddressRequired => 'Enter an address';

  @override
  String get hostEditorAddressNoSpaces => 'No spaces allowed';

  @override
  String get hostEditorPortLabel => 'Port';

  @override
  String get hostEditorPortHint => 'inherit';

  @override
  String get hostEditorUsernameLabel => 'Username';

  @override
  String get hostEditorUsernameHint => 'inherit from group';

  @override
  String get hostEditorGroupLabel => 'Group';

  @override
  String get hostEditorNoGroup => 'No group';

  @override
  String get hostEditorJumpSection => 'Jump hosts';

  @override
  String get hostEditorJumpSubtitle =>
      'Multi-hop: each hop opens a direct-tcpip channel to the next. No limit on the number of hops.';

  @override
  String get hostEditorJumpInherit => 'Inherit / direct';

  @override
  String get hostEditorJumpProfile => 'Jump profile';

  @override
  String get hostEditorJumpCustom => 'Custom chain';

  @override
  String get hostEditorJumpInheritHelp => 'Uses the jump profile of the group (if any), otherwise connects directly.';

  @override
  String hostEditorJumpProfileItem(int count, String name) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '$name ($count hops)');
    return '$_temp0';
  }

  @override
  String get hostEditorThisHost => 'this host';

  @override
  String get hostEditorSecuritySection => 'Security';

  @override
  String get hostEditorHostKeyPolicy => 'Host key policy';

  @override
  String get hostEditorSshBackend => 'SSH backend';

  @override
  String get hostEditorKeepalive => 'Keepalive interval (s)';

  @override
  String get hostEditorKeepaliveHint => 'app default';

  @override
  String get hostEditorAgentForwarding => 'Agent forwarding';

  @override
  String get hostEditorAgentForwardingHelp =>
      'Lets this server use your keys for onward connections. Enable only for hosts you trust.';

  @override
  String get hostEditorOrganisationSection => 'Organisation';

  @override
  String get hostEditorNotes => 'Notes';

  @override
  String get hostEditorEffectiveTitle => 'Effective settings';

  @override
  String get hostEditorEffectiveSubtitle => 'Resolved from this host and its group chain — what a connection will use.';

  @override
  String get hostEditorEffectiveGroup => 'Group';

  @override
  String get hostEditorEffectiveUsername => 'Username';

  @override
  String get hostEditorEffectivePort => 'Port';

  @override
  String get hostEditorEffectiveCredential => 'Credential';

  @override
  String get hostEditorEffectiveRoute => 'Route';

  @override
  String get hostEditorRouteDirect => 'Direct';

  @override
  String get hostEditorSourceHost => 'set on this host';

  @override
  String hostEditorSourceFrom(String name) {
    return 'from $name';
  }

  @override
  String hostEditorSourceCredential(String name) {
    return 'from credential $name';
  }

  @override
  String hostEditorSourceGroup(String name) {
    return 'inherited from group $name';
  }

  @override
  String hostEditorSourceJumpProfile(String name) {
    return 'jump profile $name';
  }

  @override
  String get hostEditorSourceDefault => 'default';

  @override
  String get hostEditorSourceUnset => 'not set';

  @override
  String get hostEditorCredentialPrompt => 'Password — asked when connecting';

  @override
  String get hostEditorProblemJumpHostDeleted => 'A jump host in the chain was deleted';

  @override
  String get hostEditorProblemSelfJump => 'The host cannot jump through itself';

  @override
  String get hostEditorProblemNoCredential => 'No credential: you will be asked for a password when connecting';

  @override
  String get hostEditorProblemCredentialMissing => 'The selected credential no longer exists';

  @override
  String get hostEditorProblemNoUsername => 'No username: set one on the host or on a group';

  @override
  String get hostAuthModePassword => 'Password';

  @override
  String get hostAuthModeSshKey => 'SSH key';

  @override
  String get hostAuthModeAgent => 'Agent';

  @override
  String get hostAuthModeInherit => 'Inherit from group';

  @override
  String get hostAuthTitle => 'Authentication';

  @override
  String get hostAuthUseExisting => 'Use existing credential…';

  @override
  String get hostAuthLinkedDeleted => 'Linked credential was deleted';

  @override
  String hostAuthLinkedShared(String name, String kind) {
    return '$name · shared $kind';
  }

  @override
  String get hostAuthUnlink => 'Enter a password instead';

  @override
  String get hostAuthChangeLinked => 'Change…';

  @override
  String get hostAuthPasswordSaved => 'Password saved ';

  @override
  String get hostAuthPasswordLabel => 'Password';

  @override
  String get hostAuthNewPasswordLabel => 'New password';

  @override
  String get hostAuthPasswordHelper => 'Leave empty to be asked when connecting.';

  @override
  String get hostAuthSavePassword => 'Save password in vault';

  @override
  String get hostAuthSavePasswordOn => 'Stored end-to-end encrypted as a separate secret; never shown again.';

  @override
  String get hostAuthSavePasswordOff => 'Nothing is stored — the terminal asks for the password each time you connect.';

  @override
  String get hostAuthKeepSaved => 'Keep the saved password';

  @override
  String get hostAuthKeyLabel => 'Key';

  @override
  String get hostAuthGenerateKey => 'Generate Ed25519…';

  @override
  String get hostAuthImportKey => 'Import key…';

  @override
  String get hostAuthPassphraseRemembered => 'Key passphrase remembered';

  @override
  String get hostAuthForget => 'Forget';

  @override
  String get hostAuthPassphraseForgotten => 'Passphrase forgotten';

  @override
  String get hostAuthKeyPassphraseLabel => 'Key passphrase';

  @override
  String get hostAuthKeyPassphraseHelper =>
      'The key stays passphrase-protected. Without \"Remember\" you are asked when connecting.';

  @override
  String get hostAuthRememberPassphrase => 'Remember passphrase';

  @override
  String get hostAuthRememberPassphraseHelper =>
      'Stored as a separate encrypted secret for this key (shared by all its hosts).';

  @override
  String get hostAuthAgentPathLabel => 'Socket path or pipe name';

  @override
  String hostAuthAgentPathHint(String unixPath, String windowsPath) {
    return '$unixPath  or  $windowsPath';
  }

  @override
  String get hostAuthAgentNote => 'Keys stay in the agent; ConsoleCrypt never sees them.';

  @override
  String get hostAuthInheritNoGroup =>
      'This host is not in a group, so there is nothing to inherit: the terminal will ask for a password. Pick a group or another mode.';

  @override
  String hostAuthInheritFromGroup(String group) {
    return 'Username and credential come from group \"$group\" and its parents — see Effective settings.';
  }

  @override
  String hostAuthPreviewPasswordShared(String name) {
    return 'Password · $name';
  }

  @override
  String get hostAuthPreviewPasswordDeleted => 'Password · deleted credential';

  @override
  String get hostAuthPreviewPasswordSaved => 'Password · saved in vault';

  @override
  String get hostAuthPreviewPasswordPrompt => 'Password · asked when connecting';

  @override
  String hostAuthPreviewSshKey(String name) {
    return 'SSH key · $name';
  }

  @override
  String hostAuthPreviewAgentAt(String path) {
    return 'Agent at $path';
  }

  @override
  String get hostAuthErrorKeyRequired => 'Choose, generate or import an SSH key';

  @override
  String get hostAuthErrorAgentPathRequired => 'Enter the agent socket path or pipe name';

  @override
  String get hostAuthPickerTitle => 'Use existing credential';

  @override
  String get hostAuthPickerEmpty => 'No credentials yet';

  @override
  String hostAuthPickerUser(String username) {
    return 'user $username';
  }

  @override
  String hostAuthPickerUsedBy(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: 'used by $count host(s)');
    return '$_temp0';
  }

  @override
  String get hostAuthPickerNew => 'New credential…';

  @override
  String get hostPickerDefaultTitle => 'Connect to host';

  @override
  String get hostPickerSearchHint => 'Search hosts';

  @override
  String get hostPickerEmpty => 'No hosts';

  @override
  String get jumpChainDeletedHost => '(deleted host)';

  @override
  String get jumpChainThisDevice => 'This device';

  @override
  String get jumpChainTarget => 'target';

  @override
  String get jumpChainEmpty => 'No hops — add the first jump host below.';

  @override
  String get jumpChainMoveUp => 'Move up';

  @override
  String get jumpChainRemoveHop => 'Remove hop';

  @override
  String get jumpChainAddTooltip => 'Add jump host';

  @override
  String get jumpChainAddHop => 'Add hop';

  @override
  String get groupsTitle => 'Groups';

  @override
  String get groupsSubtitle => 'Hosts inherit username, port, credential and jump profile from their group chain.';

  @override
  String get groupsNewGroup => 'New group';

  @override
  String get groupsEditGroup => 'Edit group';

  @override
  String get groupsEmpty => 'No groups yet';

  @override
  String groupsHostCount(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '$count hosts');
    return '$_temp0';
  }

  @override
  String get groupsHasDefaults => 'has defaults';

  @override
  String groupsDeleteTitle(String name) {
    return 'Delete group $name?';
  }

  @override
  String get groupsDeleteMessage =>
      'Subgroups and hosts move to the parent group. Hosts that inherited settings from this group will inherit from the parent instead.';

  @override
  String get groupsAddSubgroup => 'Add subgroup';

  @override
  String get groupsSourceOwn => 'set on this group';

  @override
  String groupsSourceInherited(String name) {
    return 'inherited from $name';
  }

  @override
  String get groupsSourceUnset => 'not set';

  @override
  String get groupsUsername => 'Username';

  @override
  String get groupsPort => 'Port';

  @override
  String get groupsCredential => 'Credential';

  @override
  String get groupsJumpProfile => 'Jump profile';

  @override
  String get groupsHostsSection => 'Hosts in this group';

  @override
  String get groupsNoHosts => 'No hosts directly in this group.';

  @override
  String get groupsParentGroup => 'Parent group';

  @override
  String get groupsNoParent => 'None (top level)';

  @override
  String get groupsDefaultsHeading => 'Defaults inherited by hosts (leave empty to inherit from the parent)';

  @override
  String get groupsInherit => 'Inherit';

  @override
  String get groupsJumpInherit => 'Inherit / direct';

  @override
  String get groupsJumpProfilesSection => 'Jump profiles';

  @override
  String get groupsNewJumpProfile => 'New jump profile';

  @override
  String get groupsEditJumpProfile => 'Edit jump profile';

  @override
  String get groupsJumpProfilesEmpty => 'Reusable ordered chains of jump hosts.';

  @override
  String get settingsTitle => 'Settings';

  @override
  String get settingsRenameProfileTitle => 'Rename profile';

  @override
  String settingsRemoveProfileTitle(String name) {
    return 'Remove profile $name from this device?';
  }

  @override
  String get settingsRemoveProfileLocalMessage =>
      'The local database and keys of this profile are deleted from this device. Without a backup the data is gone for good.';

  @override
  String get settingsRemoveProfileSyncedMessage =>
      'Local data of this profile is deleted from this device. The encrypted vault on your server is not touched.';

  @override
  String get settingsRemoveProfileConfirm => 'Remove profile';

  @override
  String get settingsProfilesTitle => 'Profiles';

  @override
  String get settingsProfilesSubtitle =>
      'Each profile is a separate encrypted vault (e.g. a local personal vault and a synced work vault).';

  @override
  String get settingsAddProfile => 'Add profile';

  @override
  String settingsProfileActive(String name) {
    return '$name  (active)';
  }

  @override
  String get settingsProfileSwitch => 'Switch';

  @override
  String get settingsSyncTitle => 'Sync';

  @override
  String get settingsSyncLocalSubtitle => 'Local only: no server, no account.';

  @override
  String get settingsSyncLocalBody =>
      'Upload this vault end-to-end encrypted to your self-hosted server to use it on other devices.';

  @override
  String get settingsEnableSync => 'Enable sync…';

  @override
  String get settingsAccountSyncTitle => 'Account & sync';

  @override
  String get settingsServerLabel => 'Server';

  @override
  String get settingsAccountLabel => 'Account';

  @override
  String get settingsThisDeviceLabel => 'This device';

  @override
  String get settingsChangeAccountPassword => 'Change account password';

  @override
  String get settingsDisconnect => 'Disconnect (keep data locally)…';

  @override
  String get settingsSignOut => 'Sign out';

  @override
  String get settingsDifferentServerHint =>
      'To use a different server, add another profile; each profile has its own server and vault.';

  @override
  String get settingsVaultTitle => 'Vault';

  @override
  String get settingsVaultNameLabel => 'Name';

  @override
  String get settingsVaultIdLabel => 'Vault ID';

  @override
  String get settingsAutoLockLabel => 'Auto-lock';

  @override
  String settingsAutoLockAfterMinutes(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'after $count minutes',
      one: 'after 1 minute',
    );
    return '$_temp0';
  }

  @override
  String settingsAutoLockAfterHours(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: 'after $count hours', one: 'after 1 hour');
    return '$_temp0';
  }

  @override
  String get settingsAutoLockNever => 'never';

  @override
  String get settingsChangePassphrase => 'Change passphrase…';

  @override
  String get settingsNewRecoveryKit => 'New Recovery Kit…';

  @override
  String get settingsBackups => 'Backups…';

  @override
  String get settingsLockNow => 'Lock now';

  @override
  String get settingsAiProvidersTitle => 'AI providers';

  @override
  String get settingsAiProvidersSubtitle =>
      'The AI can read snippets and host metadata you allow — never passwords, keys or the vault key.';

  @override
  String settingsAiProviderDefault(String name) {
    return '$name  · default';
  }

  @override
  String get settingsAiProviderApiKeyStored => 'API key stored';

  @override
  String get settingsDefaultPrivacyLabel => 'Default privacy';

  @override
  String get settingsKeepAiConversations => 'Keep AI conversations in the vault';

  @override
  String get settingsKeepAiConversationsHelp =>
      'Stored encrypted (and synced for synced profiles). Off: conversations are forgotten.';

  @override
  String get settingsTerminalTitle => 'Terminal';

  @override
  String get settingsCommandHistoryLabel => 'Command history';

  @override
  String settingsHistoryPendingSync(String description) {
    return '$description (takes effect once sync is enabled)';
  }

  @override
  String get settingsFontSizeLabel => 'Font size';

  @override
  String get settingsScrollbackLabel => 'Scrollback';

  @override
  String settingsScrollbackLines(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '$count lines', one: '$count line');
    return '$_temp0';
  }

  @override
  String get settingsTerminalBuffersNote => 'Terminal buffers stay on this device and are never synced.';

  @override
  String get settingsAppearanceSecurityTitle => 'Appearance & security';

  @override
  String get settingsThemeLabel => 'Theme';

  @override
  String get settingsThemeSystem => 'System';

  @override
  String get settingsThemeLight => 'Light';

  @override
  String get settingsThemeDark => 'Dark';

  @override
  String get settingsClearClipboardLabel => 'Clear clipboard';

  @override
  String settingsClearClipboardAfter(int seconds) {
    return '$seconds s after copying a secret';
  }

  @override
  String get settingsKnownHostsTitle => 'Known hosts';

  @override
  String get settingsKnownHostsSubtitle =>
      'Host keys you trusted. A changed key always blocks the connection until you remove the old one here.';

  @override
  String get settingsKnownHostsEmpty => 'No known hosts yet.';

  @override
  String settingsRemoveKnownHostTitle(String host) {
    return 'Remove $host?';
  }

  @override
  String get settingsRemoveKnownHostMessage => 'The next connection will ask you to verify the host key again.';

  @override
  String get settingsDialogPassphraseChanged => 'Vault passphrase changed';

  @override
  String get settingsDialogChangePassphraseTitle => 'Change vault passphrase';

  @override
  String get settingsDialogCurrentPassphrase => 'Current passphrase';

  @override
  String get settingsDialogNewPassphrase => 'New passphrase';

  @override
  String get settingsDialogRepeatPassphrase => 'Repeat new passphrase';

  @override
  String get settingsDialogChangePassphraseConfirm => 'Change passphrase';

  @override
  String get settingsDialogNewKitTitle => 'New Recovery Kit';

  @override
  String get settingsDialogNewKitWarning =>
      'A new Recovery Key replaces the current one: your old Recovery Kit stops working. Save the new kit before closing this window.';

  @override
  String get settingsDialogShowKit => 'Show Recovery Kit';

  @override
  String get settingsDialogKitSaved => 'I saved it';

  @override
  String get settingsDialogGenerateKit => 'Generate new kit';

  @override
  String get settingsDialogAccountPasswordChanged => 'Account password changed';

  @override
  String get settingsDialogAccountPasswordTitle => 'Change account password';

  @override
  String get settingsDialogCurrentPassword => 'Current password';

  @override
  String get settingsDialogNewPassword => 'New password';

  @override
  String settingsDialogPasswordMinLength(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'At least $count characters',
      one: 'At least 1 character',
    );
    return '$_temp0';
  }

  @override
  String get settingsDialogVaultPassphraseUnchanged => 'This does not change your vault passphrase.';

  @override
  String get aiProviderDialogAddTitle => 'Add AI provider';

  @override
  String aiProviderDialogEditTitle(String name) {
    return 'Edit $name';
  }

  @override
  String get aiProviderDialogProviderLabel => 'Provider';

  @override
  String get aiProviderDialogNameLabel => 'Name';

  @override
  String get aiProviderDialogBaseUrlLabel => 'Base URL';

  @override
  String get aiProviderDialogInsecureHttp =>
      'Plain http to a remote host: the API key and prompts would travel unencrypted. Use https.';

  @override
  String get aiProviderDialogChatModelLabel => 'Chat model';

  @override
  String get aiProviderDialogEmbeddingModelLabel => 'Embedding model (optional)';

  @override
  String get aiProviderDialogApiKeyLabel => 'API key';

  @override
  String get aiProviderDialogApiKeyStoredHint => 'A key is stored — type to replace it';

  @override
  String get aiProviderDialogApiKeyOptionalHint => 'Optional for local providers';

  @override
  String get aiProviderDialogApiKeyHelp =>
      'Stored as an encrypted secret in the vault. The AI subsystem cannot read it; it is only placed into the HTTP header of requests.';

  @override
  String get aiProviderDialogRemoveApiKey => 'Remove the stored API key';

  @override
  String get aiProviderDialogPrivacyProfileLabel => 'Privacy profile';

  @override
  String aiProviderDialogLocalProfileWarning(String profile) {
    return 'The $profile profile sends more context. Use it only with models running on machines you control.';
  }

  @override
  String get aiProviderDialogTimeoutLabel => 'Timeout (s)';

  @override
  String get aiProviderDialogStreaming => 'Streaming';

  @override
  String get aiProviderDialogToolCalling => 'Tool calling';

  @override
  String get aiProviderDialogDefault => 'Default';

  @override
  String get aiProviderDialogHealthOk => 'Connection test passed';

  @override
  String get aiProviderDialogHealthFailed => 'Connection test failed';

  @override
  String aiProviderDialogHealthModels(String models) {
    return 'Models: $models';
  }

  @override
  String get aiProviderDialogSaveAndTest => 'Save & test';

  @override
  String get runFlowVariablesPreview => 'Preview';

  @override
  String get runFlowConfirmTitle => 'Run command?';

  @override
  String runFlowLocalRules(String reasons) {
    return 'Local rules: $reasons.';
  }

  @override
  String runFlowDeclaredVsLocal(String declared, String effective) {
    return 'Declared as $declared; local rules rate it $effective.';
  }

  @override
  String runFlowAckDestructive(String host) {
    return 'I understand this command can delete or destroy data on $host';
  }

  @override
  String runFlowAckModifying(String host) {
    return 'I understand this command changes the state of $host';
  }

  @override
  String get runFlowAckUnknown => 'I reviewed this command; its effect is not known to local rules';

  @override
  String get runFlowRun => 'Run';

  @override
  String get runFlowPickHostTitle => 'Run on which host?';

  @override
  String get runFlowOpenTerminalFirst => 'Open a terminal tab first.';

  @override
  String get snippetEditorTitleNew => 'New snippet';

  @override
  String get snippetEditorTitleEdit => 'Edit snippet';

  @override
  String get snippetEditorAiDraftBanner => 'Drafted by AI. Review the command and its risk before saving.';

  @override
  String get snippetEditorTemplateLabel => 'Command template';

  @override
  String get snippetEditorShellLabel => 'Shell / dialect (optional)';

  @override
  String get snippetEditorVariables => 'Variables';

  @override
  String get snippetEditorVariableDefault => 'Default';

  @override
  String get snippetEditorRisk => 'Risk';

  @override
  String get snippetEditorEffective => 'Effective: ';

  @override
  String snippetEditorLocalRules(String detail) {
    return 'Local rules: $detail';
  }

  @override
  String get snippetPickerTitle => 'Run snippet';

  @override
  String get snippetPickerSearchHint => 'Search snippets';

  @override
  String get snippetsTypeLabel => 'Type';

  @override
  String snippetsSubtitle(String syntax) {
    return 'Reusable commands with $syntax. Running always shows the command, host and risk first.';
  }

  @override
  String get snippetsNewButton => 'New snippet';

  @override
  String get snippetsSearchHint => 'Search text or intent, e.g. \"disk space\"';

  @override
  String get snippetsAllTypes => 'All types';

  @override
  String get snippetsEmptyTitle => 'No snippets yet';

  @override
  String get snippetsEmptyMessage => 'Save commands you use often — or let AI draft them.';

  @override
  String get snippetsNothingFound => 'Nothing found';

  @override
  String get snippetsDraftedByAi => 'Drafted by AI';

  @override
  String get snippetsSemanticMatch => 'Semantic match';

  @override
  String snippetsUsedCount(int count) {
    return 'used $count×';
  }

  @override
  String get snippetsInsertIntoTerminal => 'Insert into terminal';

  @override
  String get snippetsRun => 'Run';

  @override
  String snippetsDeleteTitle(String name) {
    return 'Delete $name?';
  }

  @override
  String get snippetsDeleteMessage => 'The snippet is removed from this vault.';

  @override
  String get aiChatSubtitle => 'Answers never run by themselves: Insert or Run… always asks you first.';

  @override
  String get aiChatProviderLabel => 'Provider';

  @override
  String get aiChatNewConversation => 'New conversation';

  @override
  String aiChatRemoteProviderNotice(String profile, String description) {
    return 'Remote provider · privacy profile $profile: $description. Passwords and keys are never sent.';
  }

  @override
  String get aiChatLocalModelNotice => 'Local model · nothing leaves this machine.';

  @override
  String get aiChatNoProviderBanner => 'No AI provider configured.';

  @override
  String get aiChatEmptyTitle => 'Ask anything about your servers';

  @override
  String get aiChatEmptyMessage => 'e.g. \"find the biggest files in /var/log\" or \"restart nginx safely\"';

  @override
  String get aiChatInputHint => 'Message (Enter to send, Shift+Enter for a new line)';

  @override
  String get aiChatStop => 'Stop';

  @override
  String get aiChatSend => 'Send';

  @override
  String get aiChatIncludeSelection => 'Include selected terminal text';

  @override
  String get aiChatSelectToInclude => 'Select text in a terminal to include it';

  @override
  String get aiChatErrorNoProvider => 'No AI provider configured. Add one in Settings → AI providers.';

  @override
  String get aiChatErrorRequestFailed => 'The AI request failed.';

  @override
  String get aiChatStoppedSuffix => '…(stopped)';

  @override
  String paletteGenerateCommand(String query) {
    return 'Generate command: \"$query\"';
  }

  @override
  String get paletteGenerateSubtitle => 'Ask the AI (never runs automatically)';

  @override
  String get paletteNewHost => 'New host';

  @override
  String get paletteOpenAiChat => 'Open AI chat';

  @override
  String get paletteLockVault => 'Lock vault';

  @override
  String get paletteSearchHint => 'Find a host by name or IP, a snippet, an action, or describe a command…';

  @override
  String get paletteAskSelectionHint => 'Ask about the selected terminal output…';

  @override
  String get paletteAiSuggestion => 'AI suggestion';

  @override
  String get paletteThinking => 'Thinking…';

  @override
  String get paletteErrorNoProvider => 'No AI provider configured. Add Ollama, LM Studio or DeepSeek in Settings.';

  @override
  String paletteRiskMismatch(String aiRisk, String localRisk) {
    return 'The AI rated this $aiRisk; local rules say $localRisk.';
  }

  @override
  String get codeBlocksInsert => 'Insert';

  @override
  String get codeBlocksRun => 'Run…';

  @override
  String get codeBlocksSaveAsSnippet => 'Save as snippet';

  @override
  String codeBlocksSnippetSaved(String name) {
    return 'Snippet \"$name\" saved';
  }

  @override
  String codeBlocksProcessedLocally(String profile) {
    return 'Processed by a local model · $profile profile';
  }

  @override
  String codeBlocksSanitized(int count, String profile) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'Sanitized ($profile) before leaving this device · $count items redacted',
      one: 'Sanitized ($profile) before leaving this device · $count item redacted',
    );
    return '$_temp0';
  }

  @override
  String get terminalEmptyTitle => 'No open sessions';

  @override
  String terminalEmptyMessage(String shortcut) {
    return 'Connect to a host to open a terminal tab ($shortcut).';
  }

  @override
  String get terminalConnectToHost => 'Connect to host';

  @override
  String get terminalCloseTab => 'Close tab';

  @override
  String terminalNewTabTooltip(String shortcut) {
    return 'New tab ($shortcut)';
  }

  @override
  String get terminalSplitViewLater => 'Split view arrives in a later release';

  @override
  String get terminalSnippets => 'Snippets';

  @override
  String get terminalAskAi => 'Ask AI';

  @override
  String terminalHostKeyChangedTitle(String host) {
    return 'HOST KEY CHANGED for $host';
  }

  @override
  String terminalHostKeyChangedMessage(String keyType, String fingerprint) {
    return 'Host key verification failed: the server key CHANGED. This can mean a man-in-the-middle attack. Connection refused.\nPresented: $keyType $fingerprint. If the server was really reinstalled, remove the old key in Settings → Known hosts and reconnect.';
  }

  @override
  String terminalUnknownHostTitle(String host) {
    return 'Unknown host $host';
  }

  @override
  String get terminalUnknownHostMessage =>
      'The authenticity of this host cannot be established. Verify the fingerprint with the server administrator before trusting it.';

  @override
  String get terminalHostKeyReject => 'Reject';

  @override
  String get terminalHostKeyAcceptOnce => 'Accept once';

  @override
  String get terminalHostKeyAcceptAndSave => 'Accept and save';

  @override
  String get terminalConnecting => 'Connecting';

  @override
  String terminalConnectingRoute(String route) {
    return 'Connecting $route';
  }

  @override
  String get terminalReconnecting => 'Reconnecting';

  @override
  String terminalReconnectingRoute(String route) {
    return 'Reconnecting $route';
  }

  @override
  String get terminalSessionEnded => 'Session ended';

  @override
  String get terminalDisconnected => 'Disconnected';

  @override
  String get terminalConnectionClosed => 'The connection was closed.';

  @override
  String get terminalReconnect => 'Reconnect';

  @override
  String terminalPasswordPromptTitle(String user, String host) {
    return 'Password for $user@$host';
  }

  @override
  String get terminalPasswordRetry => 'Permission denied, please try again.';

  @override
  String get terminalPasswordLabel => 'Password';

  @override
  String get terminalPasswordHelper =>
      'Used for this connection only — not saved. Save it in the host settings to skip this step.';

  @override
  String get sftpOpenPickerTitle => 'Open SFTP';

  @override
  String get sftpSubtitle => 'Browse and transfer files over SSH.';

  @override
  String get sftpDisconnect => 'Disconnect';

  @override
  String get sftpNotConnectedTitle => 'Not connected';

  @override
  String get sftpNotConnectedMessage => 'Pick a host to browse, transfer and edit its files.';

  @override
  String get sftpLocalPaneTitle => 'This device';

  @override
  String get sftpUploadTooltip => 'Upload selected file';

  @override
  String get sftpDownloadTooltip => 'Download selected file';

  @override
  String get sftpNewFolder => 'New folder';

  @override
  String sftpDeleteTitle(String name) {
    return 'Delete $name?';
  }

  @override
  String get sftpDeleteFolderMessage => 'The folder and everything inside it are deleted on the server.';

  @override
  String get sftpDeleteFileMessage => 'The file is deleted on the server.';

  @override
  String get sftpRefresh => 'Refresh';

  @override
  String get sftpUp => 'Up';

  @override
  String sftpTransferDone(String size) {
    return 'Done · $size';
  }

  @override
  String sftpTransferFailed(String detail) {
    return 'Failed: $detail';
  }

  @override
  String get sftpTransferCancelled => 'Cancelled';

  @override
  String sftpTransferProgress(String done, String total) {
    return '$done of $total';
  }

  @override
  String sftpTransferSpeed(String size) {
    return '$size/s';
  }

  @override
  String get sftpToolbarQuickLook => 'Quick Look';

  @override
  String get sftpToolbarActions => 'Actions';

  @override
  String get sftpToolbarTransfers => 'Transfers';

  @override
  String get sftpToolbarEditing => 'Editing';

  @override
  String get sftpToolbarShowLocal => 'Show local files side by side';

  @override
  String get sftpToolbarHideLocal => 'Hide local files';

  @override
  String get sftpToolbarSwitchHost => 'Connect to another host';

  @override
  String get sftpSearchHint => 'Search this folder';

  @override
  String get sftpSearchClear => 'Clear search';

  @override
  String sftpConnecting(String host) {
    return 'Connecting to $host…';
  }

  @override
  String get sftpPathHint => 'Type a path, e.g. /var/www or ~/logs';

  @override
  String sftpPathEditTooltip(String shortcut) {
    return 'Click or press $shortcut to type a path';
  }

  @override
  String get sftpCopyPath => 'Copy path';

  @override
  String get sftpCopyWhatPath => 'Path';

  @override
  String get sftpCopyWhatName => 'Name';

  @override
  String get sftpColumnName => 'Name';

  @override
  String get sftpColumnSize => 'Size';

  @override
  String get sftpColumnKind => 'Kind';

  @override
  String get sftpColumnModified => 'Date Modified';

  @override
  String get sftpColumnPermissions => 'Permissions';

  @override
  String get sftpColumnOwner => 'Owner';

  @override
  String get sftpColumnGroup => 'Group';

  @override
  String get sftpFoldersFirst => 'Folders first';

  @override
  String get sftpShowHidden => 'Show hidden files';

  @override
  String get sftpResetColumns => 'Reset columns';

  @override
  String get sftpExpand => 'Expand';

  @override
  String get sftpCollapse => 'Collapse';

  @override
  String get sftpEmptyFolder => 'This folder is empty';

  @override
  String sftpNoMatches(String query) {
    return 'Nothing in this folder matches “$query”';
  }

  @override
  String sftpSymlinkTooltip(String target) {
    return 'Symbolic link → $target';
  }

  @override
  String get sftpDanglingLink => 'Broken link';

  @override
  String get sftpKindFolder => 'Folder';

  @override
  String get sftpKindText => 'Plain text';

  @override
  String get sftpKindDocument => 'Document';

  @override
  String sftpKindDocumentFormat(String format) {
    return '$format document';
  }

  @override
  String get sftpKindExecutable => 'Executable';

  @override
  String get sftpKindShellScript => 'Shell script';

  @override
  String sftpKindImage(String format) {
    return '$format image';
  }

  @override
  String sftpKindArchive(String format) {
    return '$format archive';
  }

  @override
  String get sftpKindLog => 'Log file';

  @override
  String get sftpKindConfig => 'Configuration';

  @override
  String get sftpKindPdf => 'PDF document';

  @override
  String get sftpKindAudio => 'Audio';

  @override
  String get sftpKindVideo => 'Video';

  @override
  String get sftpKindFont => 'Font';

  @override
  String get sftpKindKey => 'Key or certificate';

  @override
  String get sftpKindDatabase => 'Database';

  @override
  String get sftpKindSymlink => 'Symbolic link';

  @override
  String sftpKindSymlinkTo(String kind) {
    return 'Link → $kind';
  }

  @override
  String get sftpKindSpecial => 'Special file';

  @override
  String sftpStatusFolders(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '$count folders', one: '$count folder');
    return '$_temp0';
  }

  @override
  String sftpStatusFiles(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '$count files', one: '$count file');
    return '$_temp0';
  }

  @override
  String sftpStatusSummary(String folders, String files, String size) {
    return '$folders, $files, $size';
  }

  @override
  String sftpStatusSelection(String selected, String total, String size) {
    return '$selected of $total selected, $size';
  }

  @override
  String sftpStatusEditing(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'Editing $count files',
      one: 'Editing $count file',
    );
    return '$_temp0';
  }

  @override
  String get sftpStatusSecure => 'Encrypted SFTP connection over SSH';

  @override
  String get sftpApplications => 'Applications';

  @override
  String get sftpChooseApplication => 'Choose Application';

  @override
  String errorEditorLaunchFailed(String detail) {
    return 'Could not open the editor. Choose another application using Open With. System message: $detail';
  }

  @override
  String errorApplicationPickerFailed(String detail) {
    return 'Could not show the application picker. System message: $detail';
  }

  @override
  String get sftpActionOpenWith => 'Open With…';

  @override
  String get sftpActionDownload => 'Download…';

  @override
  String get sftpActionUploadHere => 'Upload here…';

  @override
  String get sftpActionNewFile => 'New file';

  @override
  String get sftpActionDuplicate => 'Duplicate';

  @override
  String get sftpActionGetInfo => 'Get Info';

  @override
  String get sftpActionCopyName => 'Copy name';

  @override
  String get sftpUntitledFolder => 'untitled folder';

  @override
  String get sftpUntitledFile => 'untitled.txt';

  @override
  String get sftpDuplicateSuffix => 'copy';

  @override
  String get sftpNameInvalid => 'A name can’t be empty or contain “/”.';

  @override
  String sftpDeleteManyTitle(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'Delete $count items?',
      one: 'Delete $count item?',
    );
    return '$_temp0';
  }

  @override
  String get sftpDeleteManyMessage =>
      'The selected items are deleted on the server, folders with everything inside them. This can’t be undone.';

  @override
  String sftpUploadStarted(int count, String folder) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'Uploading $count items to $folder',
      one: 'Uploading $count item to $folder',
    );
    return '$_temp0';
  }

  @override
  String sftpDownloadStarted(int count, String folder) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'Downloading $count items to $folder',
      one: 'Downloading $count item to $folder',
    );
    return '$_temp0';
  }

  @override
  String sftpMoved(int count, String folder) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'Moved $count items to $folder',
      one: 'Moved $count item to $folder',
    );
    return '$_temp0';
  }

  @override
  String sftpDragItems(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '$count items', one: '$count item');
    return '$_temp0';
  }

  @override
  String get sftpDisconnectEditsTitle => 'Stop editing and disconnect?';

  @override
  String sftpDisconnectEditsMessage(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count files from this host are being edited.',
      one: '$count file from this host is being edited.',
    );
    return '$_temp0 Pending changes are uploaded first; if that fails, the working copies are kept for recovery.';
  }

  @override
  String sftpInfoTitle(String name) {
    return '“$name” info';
  }

  @override
  String get sftpInfoWhere => 'Where';

  @override
  String sftpInfoSizeBytes(String size, String bytes) {
    return '$size ($bytes bytes)';
  }

  @override
  String get sftpInfoLinkTarget => 'Points to';

  @override
  String get sftpInfoSymlinkNote => 'Permissions of a symbolic link apply to the item it points to.';

  @override
  String get sftpInfoOthers => 'Others';

  @override
  String get sftpInfoRead => 'Read';

  @override
  String get sftpInfoWrite => 'Write';

  @override
  String get sftpInfoExecute => 'Execute';

  @override
  String get sftpInfoOctal => 'Octal';

  @override
  String get sftpInfoOctalInvalid => 'Enter 3 or 4 digits from 0 to 7.';

  @override
  String get sftpInfoApply => 'Apply';

  @override
  String get sftpInfoPermissionsSaved => 'Permissions changed.';

  @override
  String get sftpQuickLookFolder => 'Folders have no preview.';

  @override
  String get sftpQuickLookNoPreview => 'No preview for this kind of file.';

  @override
  String sftpQuickLookTooLarge(String size) {
    return 'This image is too large to preview ($size).';
  }

  @override
  String get sftpQuickLookImageError => 'The image couldn’t be displayed.';

  @override
  String sftpQuickLookTruncated(String shown, String total) {
    return 'Showing the first $shown of $total.';
  }

  @override
  String get sftpQuickLookMemoryNote => 'Loaded into memory only — nothing is saved on this device.';

  @override
  String get sftpQuickLookOpenInEditor => 'Open in editor';

  @override
  String get sftpActivityHide => 'Hide panel';

  @override
  String get sftpTransfersClear => 'Clear finished';

  @override
  String get sftpTransfersEmpty => 'No transfers yet. Drop files onto the list or use Upload here…';

  @override
  String get sftpTransferDirectionUpload => 'Upload';

  @override
  String get sftpTransferDirectionDownload => 'Download';

  @override
  String get sftpTransferQueued => 'Waiting…';

  @override
  String sftpTransferTo(String path) {
    return 'to $path';
  }

  @override
  String get sftpEditingEmpty =>
      'No files are being edited. Double-click a file to open it in its default app — every save is uploaded automatically.';

  @override
  String get sftpEditStatusOpening => 'Opening…';

  @override
  String get sftpEditStatusSynced => 'Synced';

  @override
  String get sftpEditStatusModified => 'Modified';

  @override
  String sftpEditStatusUploading(int percent) {
    return 'Uploading $percent%';
  }

  @override
  String get sftpEditStatusUploadingUnknown => 'Uploading…';

  @override
  String get sftpEditStatusConflict => 'Conflict';

  @override
  String get sftpEditStatusError => 'Upload failed';

  @override
  String get sftpEditStatusClosed => 'Closed';

  @override
  String sftpEditLastSynced(String time) {
    return 'uploaded $time';
  }

  @override
  String sftpEditOpenedWith(String app) {
    return 'in $app';
  }

  @override
  String get sftpEditRevealFinder => 'Show in Finder';

  @override
  String get sftpEditRevealExplorer => 'Show in Explorer';

  @override
  String get sftpEditRevealOther => 'Show in folder';

  @override
  String get sftpEditReopen => 'Reopen in editor';

  @override
  String get sftpEditSyncNow => 'Sync now';

  @override
  String get sftpEditStop => 'Stop editing';

  @override
  String get sftpEditResolve => 'Resolve…';

  @override
  String sftpEditOpened(String name) {
    return 'Opened “$name”. Every save is uploaded automatically.';
  }

  @override
  String sftpEditStopped(String name) {
    return 'Stopped editing “$name”.';
  }

  @override
  String sftpEditStopFailed(String name, String detail) {
    return '“$name” was not uploaded ($detail). Editing continues.';
  }

  @override
  String sftpEditTooLarge(String name, String size, String limit) {
    return '“$name” is too large to edit ($size, limit $limit).';
  }

  @override
  String get sftpEditNotAFile => 'Only regular files can be opened for editing.';

  @override
  String get sftpEditHintTitle => 'Editing in another app';

  @override
  String sftpEditHintBody(String name) {
    return '“$name” opens in its default app, outside ConsoleCrypt. While you edit, a private working copy is kept on this device (readable only by your user) and every save is uploaded to the server automatically.';
  }

  @override
  String get sftpEditHintControl =>
      'ConsoleCrypt can’t control what that app does with the file — for example its own autosave, backups or cloud sync. The working copy is deleted when you stop editing.';

  @override
  String get sftpEditHintDontShow => 'Don’t show again';

  @override
  String sftpConflictTitle(String name) {
    return '“$name” changed on the server';
  }

  @override
  String get sftpConflictMessage =>
      'Someone changed this file on the server after you opened it. Your latest save has not been uploaded, so nothing was overwritten. Choose what to do:';

  @override
  String get sftpConflictDeletedMessage =>
      'The file was deleted or moved on the server after you opened it. Your latest save has not been uploaded.';

  @override
  String sftpConflictRemoteMeta(String size, String time) {
    return 'Server version: $size, modified $time';
  }

  @override
  String get sftpConflictOverwriteTitle => 'Overwrite the server file';

  @override
  String get sftpConflictOverwriteBody => 'Upload your version. The other changes on the server are lost.';

  @override
  String get sftpConflictRecreateTitle => 'Upload my version again';

  @override
  String get sftpConflictRecreateBody => 'Create the file on the server again from your local copy.';

  @override
  String get sftpConflictKeepTitle => 'Keep both';

  @override
  String sftpConflictKeepBody(String name) {
    return 'Save the server version next to your copy as “$name.remote-…” and open it to compare. Your next save uploads your version.';
  }

  @override
  String get sftpConflictDiscardTitle => 'Discard my changes';

  @override
  String get sftpConflictDiscardBody => 'Replace your local copy with the server version.';

  @override
  String get sftpConflictLater => 'Decide later';

  @override
  String get sftpLeftoversTitle => 'Recover unsaved edits?';

  @override
  String get sftpLeftoversMessage =>
      'ConsoleCrypt was closed while these remote files were being edited. Their working copies are still on this device.';

  @override
  String get sftpLeftoverModified => 'Changed locally, not on the server yet';

  @override
  String get sftpLeftoverUnchanged => 'No local changes';

  @override
  String get sftpLeftoverUnknownHost => 'Unknown host — can only be deleted';

  @override
  String get sftpLeftoverUnreadable => 'Session data is damaged — can only be deleted';

  @override
  String get sftpLeftoversRecover => 'Recover selected';

  @override
  String get sftpLeftoversRecoverHint =>
      'Selected files are uploaded after a conflict check and stay open for editing. The other working copies are deleted.';

  @override
  String get sftpLeftoversDiscardAll => 'Delete all';

  @override
  String get sftpLeftoversLater => 'Later';

  @override
  String sftpLeftoversRecovered(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'Recovered $count files',
      one: 'Recovered $count file',
    );
    return '$_temp0';
  }

  @override
  String sftpLeftoversFailed(String name, String error) {
    return 'Couldn’t recover “$name”: $error';
  }

  @override
  String get sftpDebugMenu => 'Simulate (demo backend)';

  @override
  String get sftpDebugSave => 'Simulate a save in the editor';

  @override
  String get sftpDebugRemoteChange => 'Simulate a change on the server';

  @override
  String get sftpDebugFailUpload => 'Make the next upload fail';

  @override
  String get sftpDebugFailTransfer => 'Make the next transfer fail (demo)';

  @override
  String get tunnelsTitle => 'Tunnels';

  @override
  String get tunnelsSubtitle =>
      'Port forwarding over SSH. Tunnels run on this device; their definitions sync with your vault.';

  @override
  String get tunnelsNew => 'New tunnel';

  @override
  String get tunnelsEmptyTitle => 'No tunnels yet';

  @override
  String get tunnelsEmptyMessage => 'e.g. 127.0.0.1:15432 → db.internal:5432 through a bastion.';

  @override
  String get tunnelsDeletedHost => '(deleted host)';

  @override
  String tunnelsStatusRunning(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: 'Running · $count conn.');
    return '$_temp0';
  }

  @override
  String tunnelsStatusRunningSince(int count, String ago) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: 'Running since $ago · $count conn.');
    return '$_temp0';
  }

  @override
  String get tunnelsStatusStarting => 'Starting…';

  @override
  String get tunnelsStatusFailed => 'Failed';

  @override
  String tunnelsStatusFailedDetail(String detail) {
    return 'Failed: $detail';
  }

  @override
  String get tunnelsStatusStopped => 'Stopped';

  @override
  String tunnelsPublicBindTooltip(String address) {
    return 'Bound to $address: reachable from the network';
  }

  @override
  String tunnelsSummaryRemote(String bind, String target) {
    return 'remote $bind → $target';
  }

  @override
  String tunnelsTileSubtitle(String summary, String host) {
    return '$summary  ·  via $host';
  }

  @override
  String tunnelsDeleteTitle(String name) {
    return 'Delete tunnel $name?';
  }

  @override
  String get tunnelsDeleteMessage => 'The tunnel is stopped and removed from the vault.';

  @override
  String get tunnelsErrorChooseHost => 'Choose the host that carries the tunnel';

  @override
  String get tunnelsEdit => 'Edit tunnel';

  @override
  String get tunnelsHostLabel => 'Through host (SSH connection incl. its jump chain)';

  @override
  String get tunnelsBindAddressRemote => 'Bind address on the server';

  @override
  String get tunnelsBindAddressLocal => 'Bind address (this device)';

  @override
  String get tunnelsPortLabel => 'Port';

  @override
  String get tunnelsTargetRemote => 'Target on this device';

  @override
  String get tunnelsTargetLocal => 'Target host (as seen from the server)';

  @override
  String get tunnelsPublicBindTitle => 'Not bound to loopback';

  @override
  String tunnelsPublicBindWarningRemote(String address) {
    return '\"$address\" exposes the forwarded port to the server\'s network (requires GatewayPorts). Anyone who can reach it can use the tunnel.';
  }

  @override
  String tunnelsPublicBindWarningLocal(String address) {
    return '\"$address\" exposes the tunnel to your local network. Anyone who can reach this device can use it. Use 127.0.0.1 unless you really mean it.';
  }

  @override
  String get tunnelsPublicBindAck => 'I understand this tunnel is reachable from the network';

  @override
  String get tunnelsAutoStart => 'Start automatically after unlock';

  @override
  String get approvalWaitTitle => 'Approve this device';

  @override
  String approvalWaitSubtitle(String device) {
    return 'On a device where ConsoleCrypt is unlocked, open Devices and review the request from \"$device\". The code shown there must match this one exactly.';
  }

  @override
  String get approvalWaitSubtitleUnnamed =>
      'On a device where ConsoleCrypt is unlocked, open Devices and review the request from \"this device\". The code shown there must match this one exactly.';

  @override
  String get approvalWaitCodeLabel => 'Verification code of this device';

  @override
  String get approvalWaitWaiting => 'Waiting for approval… The request expires in 24 hours.';

  @override
  String get approvalWaitMismatchWarning =>
      'If the codes differ, reject the request on the other device. A mismatch can mean someone (even a compromised server) is trying to add their own device.';

  @override
  String get approvalWaitSimulateApproval => 'Simulate approval (mock)';

  @override
  String get createVaultDefaultName => 'Personal';

  @override
  String get createVaultTitle => 'Create your vault passphrase';

  @override
  String get createVaultSubtitleLocal => 'It encrypts everything in this profile on this device.';

  @override
  String get createVaultSubtitleSynced =>
      'It encrypts your vault before anything is synced. Your server never sees it, and it is different from your account password.';

  @override
  String get createVaultNameLabel => 'Vault name';

  @override
  String get createVaultPassphraseLabel => 'Vault passphrase';

  @override
  String get createVaultRepeatPassphraseLabel => 'Repeat passphrase';

  @override
  String get createVaultPassphraseMismatch => 'Passphrases do not match';

  @override
  String get createVaultNoResetNotice =>
      'Nobody can reset this passphrase for you — not even your server administrator. Next you will get a Recovery Kit, the only other way into the vault.';

  @override
  String get createVaultSubmit => 'Create vault';

  @override
  String get recoveryKitTitle => 'Save your Recovery Kit';

  @override
  String get recoveryKitSubtitle =>
      'If you forget your passphrase and lose access to your trusted devices, this kit is the only way back into your vault. Keep it offline: print it or write it down.';

  @override
  String get recoveryKitLostFromMemory =>
      'The Recovery Kit is no longer in memory (the app was restarted). Generate a new one — the previous kit stops working.';

  @override
  String get recoveryKitGenerateNew => 'Generate a new Recovery Kit';

  @override
  String get recoveryKitPrintUnavailable => 'Printing arrives with M5. For now, write the words down.';

  @override
  String get recoveryKitPrint => 'Print / Save as PDF';

  @override
  String get recoveryKitCopyWords => 'Copy words';

  @override
  String get recoveryKitSavedConfirmation => 'I have stored my Recovery Kit somewhere safe';

  @override
  String get recoveryKitRevealFirst => 'Reveal the kit first';

  @override
  String get recoveryKitKeyNotStored =>
      'The Recovery Key is never stored by ConsoleCrypt or your server — only an envelope it can open.';

  @override
  String get recoveryKitVaultId => 'Vault ID';

  @override
  String get recoveryKitServer => 'Server';

  @override
  String get recoveryKitNoServerCopy => 'Local profile — no server copy';

  @override
  String get recoveryKitCreated => 'Created';

  @override
  String get recoveryKitPrivacyHint => 'Make sure nobody can see your screen.';

  @override
  String get recoveryKitShow => 'Show Recovery Kit';

  @override
  String get verifyKitTitle => 'Confirm your Recovery Kit';

  @override
  String get verifyKitNoKit => 'No Recovery Kit in memory. Go back to generate one.';

  @override
  String get verifyKitBackToRecoveryKit => 'Back to Recovery Kit';

  @override
  String get verifyKitSubtitle =>
      'Enter the requested words from your kit to prove it is saved. This step is required.';

  @override
  String verifyKitWordLabel(int index) {
    return 'Word #$index';
  }

  @override
  String get verifyKitMismatch => 'Those words don\'t match your Recovery Kit. Check your kit and try again.';

  @override
  String get verifyKitBackToKit => 'Back to kit';

  @override
  String get verifyKitSubmit => 'Verify';

  @override
  String get onboardingLocalNoticeTitle => 'No cloud copy';

  @override
  String get onboardingLocalNoticeSubtitle =>
      'This profile lives only on this device. Keep your Recovery Kit and make backups.';

  @override
  String get onboardingLocalNoticeKitTitle => 'Keep your Recovery Kit safe';

  @override
  String get onboardingLocalNoticeKitBody => 'It opens the vault if you forget your passphrase.';

  @override
  String get onboardingLocalNoticeBackupsTitle => 'Make encrypted backups';

  @override
  String get onboardingLocalNoticeBackupsBody =>
      'Export a .ccbackup file, or let ConsoleCrypt back up to a folder automatically.';

  @override
  String get onboardingLocalNoticeLossTitle => 'Lost device without a backup = lost data';

  @override
  String get onboardingLocalNoticeLossBody =>
      'There is no server copy to restore from. You can enable sync later in Settings.';

  @override
  String get onboardingLocalNoticeSetUpBackups => 'Set up automatic backups';

  @override
  String get onboardingLocalNoticeContinue => 'I understand — continue';

  @override
  String unlockTitle(String name) {
    return 'Unlock $name';
  }

  @override
  String get unlockTitleGeneric => 'Unlock vault';

  @override
  String get unlockPassphraseLabel => 'Vault passphrase';

  @override
  String get unlockSubmit => 'Unlock';

  @override
  String get unlockOsAuthReason => 'Unlock your ConsoleCrypt vault';

  @override
  String unlockWithOsAuth(String method) {
    return 'Unlock with $method';
  }

  @override
  String get unlockUntrustedTitle => 'This device is not trusted yet';

  @override
  String get unlockUntrustedMessage =>
      'Unlock with your passphrase, or approve this device from one you already use. Approval requires comparing a verification code on both screens.';

  @override
  String get unlockApproveFromOtherDevice => 'Approve from another device';

  @override
  String get unlockForgotPassphrase => 'Forgot passphrase?';

  @override
  String get unlockSignOut => 'Sign out';

  @override
  String get recoveryTitle => 'Recover vault access';

  @override
  String get recoverySubtitle => 'Forgot your vault passphrase? Open the vault another way and set a new passphrase.';

  @override
  String get recoveryMethodRecoveryKey => 'Recovery Key';

  @override
  String get recoveryMethodTrustedDevice => 'This trusted device';

  @override
  String get recoveryWordsInstructions => 'Type the 24 words from your Recovery Kit, or paste the text of its QR code.';

  @override
  String get recoveryInputHint => 'word1 word2 … word24  or  consolecrypt-recovery:v1:…';

  @override
  String get recoveryQrRecognised => 'QR text recognised';

  @override
  String get recoveryQrIncomplete => 'QR text is incomplete';

  @override
  String recoveryWordCount(int count, int total) {
    return '$count / $total words';
  }

  @override
  String recoveryTrustedDeviceInfo(String method) {
    return 'Uses this device\'s key, protected by $method, to open the vault. Then set a new passphrase; other devices keep working.';
  }

  @override
  String get recoveryDeviceNotTrusted => 'This device is not trusted for the vault. Use your Recovery Key instead.';

  @override
  String get recoveryNewPassphraseLabel => 'New vault passphrase';

  @override
  String get recoveryRepeatNewPassphraseLabel => 'Repeat new passphrase';

  @override
  String get recoverySubmitTrustedDevice => 'Authenticate and set passphrase';

  @override
  String get recoverySubmitRecoveryKey => 'Recover and set passphrase';

  @override
  String get recoveryLostEverythingTitle => 'Lost the passphrase, the Recovery Key and every trusted device?';

  @override
  String get recoveryLostEverythingLocal =>
      'This local profile can only come back from a .ccbackup file opened with its passphrase or Recovery Key (Welcome → Restore from a backup). Without one, the data cannot be recovered — by design, nobody holds a master key.';

  @override
  String get recoveryLostEverythingSynced =>
      'The old vault cannot be decrypted by anyone, including your server — by design. You can recover your account by e-mail and start a new, empty vault.';

  @override
  String get shellToggleSidebar => 'Show or hide the sidebar';

  @override
  String get shellNewConnection => 'New connection';

  @override
  String shellNewConnectionTooltip(String shortcut) {
    return 'New connection ($shortcut)';
  }

  @override
  String get shellSyncNever => 'Never';

  @override
  String get shellSyncDetails => 'Sync details';

  @override
  String aboutTitle(String appName) {
    return 'About $appName';
  }

  @override
  String get aboutTagline => 'End-to-end encrypted SSH client';

  @override
  String aboutVersion(String version) {
    return 'Version $version';
  }

  @override
  String get aboutAuthor => 'Author';

  @override
  String get aboutLicenses => 'Licenses';

  @override
  String get aboutLicenseClient => 'Desktop client';

  @override
  String get aboutLicenseServer => 'Sync server';

  @override
  String get aboutOpenSource => 'ConsoleCrypt is open-source software. Your connections are secure.';

  @override
  String get settingsAboutTitle => 'About';

  @override
  String get settingsAppearanceTitle => 'Appearance';

  @override
  String get settingsSecurityTitle => 'Security';

  @override
  String get settingsGlassHelp =>
      'How see-through the sidebar, toolbar, menus and dialogs are. Security prompts are always opaque.';

  @override
  String get settingsGlassSolidReduceTransparency =>
      'Shown solid because Reduce Transparency is on in the system settings.';

  @override
  String get settingsGlassSolidRemote => 'Shown solid in a remote desktop session.';

  @override
  String get settingsGlassSolidBattery => 'Shown solid to save battery (battery saver / Low Power Mode).';

  @override
  String get settingsGlassSolidPerformance => 'Glass reduced for performance.';

  @override
  String get settingsSidebarLabel => 'Sidebar';

  @override
  String get settingsSidebarFloating => 'Floating';

  @override
  String get settingsSidebarEdgeToEdge => 'Edge to edge';

  @override
  String get secretFieldCyrillicHint => 'Typed text contains Cyrillic letters — check the keyboard layout.';

  @override
  String get secretFieldNonLatinHint => 'Typed text contains letters outside A–Z — check the keyboard layout.';

  @override
  String get passphraseLayoutNotice =>
      'This passphrase contains letters outside A–Z. To unlock the vault you will have to type it with the same keyboard layout.';

  @override
  String get unlockDeleteProfile => 'Delete this profile…';

  @override
  String get deleteProfileLocalWarning =>
      'This vault exists only on this device. Once deleted, it can come back only from a backup file (.ccbackup) opened with its passphrase or Recovery Kit. Without one, every host, key and password in it is lost for good.';

  @override
  String get deleteProfileAcknowledge => 'I understand that this cannot be undone';

  @override
  String runFlowConfirmTitleDestructive(String host) {
    return 'Run a destructive command on $host?';
  }

  @override
  String get paletteGroupActions => 'Actions';

  @override
  String get paletteGroupAi => 'AI';

  @override
  String get settingsUiFontScale => 'Interface text size';

  @override
  String get settingsUiScaleHelp =>
      '100% is the new base size, equal to the previous 90%. Choose from 80% to 140%. Terminal text size is configured separately.';

  @override
  String get settingsAccentColor => 'Accent colour';

  @override
  String get settingsBackgroundTint => 'Background tint';

  @override
  String get settingsColorsHelp =>
      'Choose a swatch or enter any HEX colour. Backgrounds adapt to the light or dark theme; text stays readable.';

  @override
  String get settingsColorApply => 'Apply';

  @override
  String get settingsColorHex => 'HEX colour';

  @override
  String get settingsColorInvalid => 'Enter six hexadecimal digits, for example #4C8DFF.';

  @override
  String get settingsResetUi => 'Reset text size and colours';

  @override
  String get settingsTerminalColorScheme => 'Terminal colour theme';

  @override
  String get settingsTerminalThemeHelp =>
      'Applies to all terminal tabs immediately, independently of interface colours.';

  @override
  String get settingsTerminalThemeSystem => 'Follow interface';

  @override
  String get settingsTerminalThemeDark => 'Graphite';

  @override
  String get settingsTerminalThemeLight => 'Paper';

  @override
  String get settingsTerminalThemeMidnight => 'Midnight';

  @override
  String get settingsTerminalThemeOcean => 'Ocean';

  @override
  String get settingsTerminalThemeForest => 'Forest';

  @override
  String get settingsTerminalThemeAmber => 'Amber';

  @override
  String get settingsTerminalPreview => 'Terminal preview';

  @override
  String get settingsAppearanceLocalHint => 'Saved on this device. No restart required.';

  @override
  String get errorSecureStore =>
      'System secure storage is unavailable. On macOS, the “login” keychain asks for your Mac login password, not your vault passphrase. If your Mac password changed, the keychain may still use the old one. Open Keychain Access and unlock “login”, then retry opening the profile. Do not delete or reset the keychain: it holds this device’s encryption keys.';

  @override
  String get welcomeResumeTitle => 'Open a profile';

  @override
  String get welcomeResumeHelp => 'Your profiles are on this device. Choose one to continue.';

  @override
  String get welcomeKeychainHelp =>
      'Opening a profile may ask for access to the system keychain. On macOS, enter the password for the “login” keychain (usually your Mac login password). If rejected, unlock “login” in Keychain Access; after a Mac password change it may require the previous password.';

  @override
  String get reopenLastProfile => 'Open the last profile at startup';

  @override
  String get reopenLastProfileHelp => 'May show the system keychain prompt before the app opens.';

  @override
  String get settingsTerminalThemeCustom => 'Custom';

  @override
  String get colorSpectrum => 'Colour spectrum';

  @override
  String get colorHue => 'Hue';

  @override
  String get colorSaturation => 'Saturation';

  @override
  String get colorBrightness => 'Brightness';

  @override
  String get commonThemeColor => 'Colour';

  @override
  String get terminalThemeEdit => 'Customize / import…';

  @override
  String get terminalThemeEditorHelp =>
      'Choose a colour to edit. Changes apply to all terminal tabs when you press Apply.';

  @override
  String get terminalThemeImport => 'Import theme';

  @override
  String get terminalThemeExport => 'Export JSON';

  @override
  String get terminalThemeFormats =>
      'Import: iTerm .itermcolors (XML) or ConsoleCrypt .json, up to 256 KB. Export: ConsoleCrypt .json.';

  @override
  String get terminalThemeImportError =>
      'Could not import this theme. Choose a valid iTerm XML preset or ConsoleCrypt JSON file, up to 256 KB. Your saved theme has not changed.';

  @override
  String get terminalThemeExportError => 'Could not save the theme. Choose another location and try again.';

  @override
  String get terminalColorBackground => 'Background';

  @override
  String get terminalColorForeground => 'Text';

  @override
  String get terminalColorCursor => 'Cursor';

  @override
  String get terminalColorSelection => 'Selection';

  @override
  String terminalColorAnsi(int index) {
    return 'ANSI $index';
  }

  @override
  String terminalColorAnsiBright(int index) {
    return 'Bright $index';
  }

  @override
  String get terminalThemeFileType => 'Terminal colour schemes';

  @override
  String get terminalThemeJsonType => 'ConsoleCrypt theme';

  @override
  String get terminalSampleSelection => ' selected text ';

  @override
  String get settingsGlassRetryEffects => 'Retry glass effects';

  @override
  String get deviceUnlockTitle => 'Fingerprint unlock';

  @override
  String get deviceUnlockHelp => 'Confirm access with your fingerprint. macOS may also offer your account password.';

  @override
  String get deviceUnlockLocalOnly =>
      'Only for this vault on this device. Your passphrase and Recovery Key remain available.';

  @override
  String get deviceUnlockEnableReason => 'Allow unlocking your ConsoleCrypt vault on this Mac';

  @override
  String get deviceUnlockNeedsTrust => 'First unlock the vault with its passphrase on this device.';

  @override
  String get deviceUnlockNotEnrolled =>
      'No fingerprint is enrolled. Add one in macOS System Settings → Touch ID & Password, then check availability again.';

  @override
  String get deviceUnlockUnsupported =>
      'Biometric unlock is currently unavailable on this device. If your Mac supports Touch ID, configure it in System Settings → Touch ID & Password.';

  @override
  String get deviceUnlockMacPassword =>
      'macOS currently offers its account password. Fingerprint unlock needs available Touch ID with an enrolled fingerprint: System Settings → Touch ID & Password.';

  @override
  String get deviceUnlockRecoveryUnavailable =>
      'Unlock with this device is disabled or currently unavailable. Use your Recovery Key. You can enable it after signing in: Settings → Vault.';

  @override
  String get deviceUnlockRefresh => 'Check availability again';

  @override
  String deviceUnlockWithMethod(String method) {
    return 'Unlock with $method';
  }

  @override
  String get deviceUnlockMacPasswordName => 'macOS password';

  @override
  String get workspaceToolsClose => 'Close right panel';

  @override
  String get workspaceToolsResize => 'Right panel width';

  @override
  String get snippetPackage => 'Package';

  @override
  String get snippetPackageHint => 'Choose an existing name or enter a new one. Leave empty for no package.';

  @override
  String get snippetAllPackages => 'All snippets';

  @override
  String get snippetNoPackage => 'Without a package';

  @override
  String get snippetStarterCatalog => 'Starter packages';

  @override
  String get snippetStarterIntro =>
      'Add useful commands to your vault. You can edit or delete them afterwards; changes sync with your vault.';

  @override
  String snippetStarterOrigin(String package) {
    return 'From the $package starter package.';
  }

  @override
  String snippetAddCount(int count) {
    return 'Add $count commands';
  }

  @override
  String get snippetPackageAdded => 'Added';

  @override
  String get snippetPackageRename => 'Rename package';

  @override
  String get snippetPackageDelete => 'Delete package';

  @override
  String snippetPackageDeleteMessage(int count) {
    return 'Delete all $count snippets in this package? This deletion also syncs to your other devices.';
  }

  @override
  String get snippetMovePackage => 'Move to package…';

  @override
  String get snippetPaste => 'Paste';

  @override
  String get snippetRunMany => 'Run in multiple terminals…';

  @override
  String get snippetChooseTerminals => 'Choose terminals';

  @override
  String get snippetConnectedOnly =>
      'Only connected terminals are listed. The command and all targets are reviewed before running.';

  @override
  String get snippetNoConnected => 'Connect a terminal first.';

  @override
  String get snippetTargetsChanged => 'The profile or selected terminals changed. Choose your targets again.';

  @override
  String get snippetUnsafePaste =>
      'This terminal does not support safe multiline paste. Use Run to review the script before execution.';

  @override
  String get snippetSyncHint => 'Saved in the vault · synced when sync is enabled';

  @override
  String get snippetPackLinux => 'Linux diagnostics';

  @override
  String get snippetPackLinuxDescription => 'System load, disks, memory, ports and service logs. For Linux hosts.';

  @override
  String get snippetPackDocker => 'Docker';

  @override
  String get snippetPackDockerDescription =>
      'Containers, Compose, resource usage and logs. Requires Docker on the host.';

  @override
  String get snippetPackKubernetes => 'Kubernetes';

  @override
  String get snippetPackKubernetesDescription =>
      'Nodes, pods, services, logs and events. Requires a configured kubectl context.';

  @override
  String get snippetCatalogUptime => 'System load and uptime';

  @override
  String get snippetCatalogDisk => 'Disk space';

  @override
  String get snippetCatalogMemory => 'Memory usage';

  @override
  String get snippetCatalogPorts => 'Listening ports';

  @override
  String get snippetCatalogJournal => 'Service logs';

  @override
  String get snippetCatalogContainers => 'All containers';

  @override
  String get snippetCatalogCompose => 'Compose project status';

  @override
  String get snippetCatalogStats => 'Container resource usage';

  @override
  String get snippetCatalogDockerLogs => 'Container logs';

  @override
  String get snippetCatalogDockerDisk => 'Docker disk usage';

  @override
  String get snippetCatalogNodes => 'Cluster nodes';

  @override
  String get snippetCatalogPods => 'Pods in namespace';

  @override
  String get snippetCatalogServices => 'Services in namespace';

  @override
  String get snippetCatalogPodLogs => 'Pod logs';

  @override
  String get snippetCatalogEvents => 'Recent cluster events';

  @override
  String get glassScrollTabsLeft => 'Scroll tabs left';

  @override
  String get glassScrollTabsRight => 'Scroll tabs right';

  @override
  String inventoryOverview(int hosts, int groups) {
    return '$hosts hosts · $groups groups';
  }

  @override
  String get inventoryAllHosts => 'All hosts';

  @override
  String get inventoryUngrouped => 'Ungrouped';

  @override
  String get inventoryFolders => 'GROUPS';

  @override
  String get inventoryLocation => 'Location';

  @override
  String get inventoryExpand => 'Expand group';

  @override
  String get inventoryCollapse => 'Collapse group';

  @override
  String get inventorySearch => 'Search hosts, groups, addresses or tags';

  @override
  String get inventoryCards => 'Cards';

  @override
  String get inventoryList => 'List';

  @override
  String get inventoryGroupSettings => 'Group settings';

  @override
  String get inventoryEffectiveDefaults => 'Saved settings and inheritance';

  @override
  String get inventoryIncludesSubgroups => 'Hosts in this group and its subgroups';

  @override
  String get inventoryEmptyHelp => 'Add hosts or groups to organize your servers.';

  @override
  String get inventorySearchHelp => 'Try another search or clear the filters.';

  @override
  String get inventoryClearFilters => 'Clear filters';

  @override
  String get inventoryConnected => 'Active SSH connection';

  @override
  String get settingsWorkspacePanel => 'Right panel';

  @override
  String get settingsWorkspaceFloating => 'Bubbles';

  @override
  String get settingsWorkspaceExpanded => 'Unified panel';

  @override
  String get settingsWorkspaceFloatingHelp =>
      'Separate floating panels beside the tool icons. This preference is saved on this device.';

  @override
  String get settingsWorkspaceExpandedHelp =>
      'The right rail expands into one panel with Snippets and AI chat tabs. It shares the workspace in wide windows and overlays it in narrow windows. Saved on this device.';

  @override
  String get workspaceToolsCollapse => 'Collapse right panel';

  @override
  String get mobileMore => 'More';

  @override
  String get mobileKeyboard => 'Keyboard';

  @override
  String get mobileTools => 'Tools';

  @override
  String get mobileMessageHint => 'Message…';

  @override
  String get mobileDeviceAuth => 'Fingerprint or PIN';

  @override
  String get mobileDeviceAuthHelp => 'Confirm your identity using the fingerprint sensor or your phone’s screen lock.';

  @override
  String get mobileBackupsHelp =>
      'Manual encrypted backups can be saved to Files. Automatic scheduled backups are not yet available on Android.';

  @override
  String get sftpDefaultEditor => 'Default editor';

  @override
  String get sftpSystemEditor => 'System default';

  @override
  String get sftpDefaultEditorHelp =>
      'Open and Open in editor use this application. Open With lets you choose another application for one file. Saved only on this device.';

  @override
  String get terminalContextMenu => 'Terminal actions';

  @override
  String get terminalPaste => 'Paste';

  @override
  String get terminalSaveSnippet => 'Save as snippet…';

  @override
  String get terminalSelectAll => 'Select entire buffer';

  @override
  String get terminalClearSelection => 'Clear selection';

  @override
  String get terminalPasteConfirmTitle => 'Paste multiple lines or control characters?';

  @override
  String get terminalPasteConfirmMessage =>
      'This text may execute commands or interrupt a running process. Review it before pasting.';

  @override
  String aiTerminalAttachment(String host) {
    return 'Selected text · $host';
  }

  @override
  String get aiRemoveAttachment => 'Remove selected text';

  @override
  String get screenCaptureTitle => 'Screenshots and screen recording';

  @override
  String get screenCaptureAllow => 'Allow screenshots';

  @override
  String get screenCaptureHelp =>
      'Also allows screen recording and app previews in Recent apps. Turn off to hide the app content from capture.';

  @override
  String get screenCaptureLocalOnly =>
      'Applies immediately to this entire app on this device, including the lock screen. Saved after restart; never synced.';

  @override
  String get screenCaptureMacosHelp =>
      'Screenshots are allowed. Current macOS versions do not provide a reliable way for this app to block screenshots or screen recording.';

  @override
  String get screenCaptureError => 'Could not read or apply the screenshot setting. Refresh its status and try again.';

  @override
  String get updatesTitle => 'Updates';

  @override
  String get updateAutomatic => 'Check for updates at startup';

  @override
  String get updatePrivacyHelp =>
      'Only the app version and platform are used. Vault data, passwords and account tokens are never sent.';

  @override
  String get updateCheck => 'Check for updates';

  @override
  String updateCurrentVersion(String version) {
    return 'Installed version: $version';
  }

  @override
  String get updateChecking => 'Checking for updates…';

  @override
  String get updateCurrent => 'You have the latest version.';

  @override
  String updateAvailable(String version) {
    return 'Version $version is available';
  }

  @override
  String get updateDownloadInstall => 'Download and install';

  @override
  String updateDownloading(int percent) {
    return 'Downloading: $percent%';
  }

  @override
  String get updateInstalling => 'Verifying and starting the installer…';

  @override
  String get updateInstallHelp =>
      'The installer will update ConsoleCrypt and may close the application and its SSH sessions. Your profiles and vaults will be preserved. Android asks you to confirm installation.';

  @override
  String get updateMacInstallHelp =>
      'After the download, choose where to save the verified DMG in the macOS save dialog. The installer will open. Quit ConsoleCrypt, drag the app into Applications and confirm replacement. Your profiles and vaults are preserved.';

  @override
  String get updateAndroidPermission =>
      'Allow ConsoleCrypt to install updates in the Android settings that opened, then press Download and install again.';

  @override
  String get updateInstallerOpened => 'The system installer is open. Complete installation there.';

  @override
  String get updateFailed => 'Could not complete the update.';

  @override
  String get updateFailedHelp =>
      'Check your connection and try again. Files that fail signature or checksum verification are never installed.';

  @override
  String get updateAppleHelp =>
      'On iOS, updates are installed through the App Store, TestFlight or Xcode, depending on how the app was installed.';

  @override
  String get updateLinuxHelp =>
      'On Linux, download the latest .deb or .rpm package for your distribution from the official website and install it with your package manager. Use the same method to update the app.';

  @override
  String get updateLinuxDownloadPage => 'Official download page';

  @override
  String get sharingTitle => 'Sharing';

  @override
  String get sharingSubtitle => 'Selected data for verified devices on this server.';

  @override
  String get sharingUnavailable => 'Sharing is unavailable on this server.';

  @override
  String get sharingLocal => 'Connect a profile to a server to share.';

  @override
  String get sharingReceived => 'Available to me';

  @override
  String get sharingOwned => 'Shared by me';

  @override
  String get sharingPublish => 'Share…';

  @override
  String get sharingPublishConfirm => 'Share';

  @override
  String get sharingEmpty => 'No shared items yet.';

  @override
  String get sharingReceivedHelp => 'Items your colleagues shared with you. Verify the owner before accepting access.';

  @override
  String get sharingOwnedHelp => 'Manage the items you shared, recipient permissions, and access revocation.';

  @override
  String get sharingReceivedEmpty => 'Nothing has been shared with you yet';

  @override
  String get sharingReceivedEmptyHelp =>
      'Ask a colleague on this server to share an item, then refresh this page. Compare the owner’s device code before accepting it.';

  @override
  String get sharingOwnedEmpty => 'You haven’t shared anything yet';

  @override
  String get sharingOwnedEmptyHelp =>
      'Choose Share from an item’s menu. Select the recipients and verify their device codes. Only the items you choose are shared.';

  @override
  String get sharingDevicesTitle => 'Access on another device';

  @override
  String get sharingDevicesHelp =>
      'Add a device to existing shared access. Device verification and the owner’s permissions still apply.';

  @override
  String get sharingDevicePrepare => 'Add this device';

  @override
  String get sharingDeviceEndorse => 'Confirm another device';

  @override
  String get sharingDeviceContinue => 'Continue adding a device';

  @override
  String get sharingDeviceMore => 'More actions';

  @override
  String get sharingDeviceMoreHelp =>
      'Continue with an already confirmed device package, or verify signed history to restore local request state. These actions do not grant access by themselves.';

  @override
  String get sharingInvitation => 'Invitation — verify the owner';

  @override
  String get sharingAccept => 'Verify and accept';

  @override
  String get sharingRefresh => 'Refresh shared data';

  @override
  String get sharingIdentity => 'My device verification code';

  @override
  String get sharingVerifyHelp =>
      'Compare this code with your colleague using an independent channel. Login and email do not verify device keys.';

  @override
  String get sharingCode => 'Verification code';

  @override
  String get sharingConfirmed => 'The code matches the code on my colleague’s device';

  @override
  String get sharingEmail => 'Colleague’s email';

  @override
  String get sharingFind => 'Find devices';

  @override
  String get sharingSelectDevice => 'Select and verify recipient devices';

  @override
  String get sharingReader => 'Read';

  @override
  String get sharingEditor => 'Edit';

  @override
  String get sharingReaderHelp => 'Can read and keep copies; cannot change the shared item.';

  @override
  String get sharingEditorHelp => 'Can read and edit content; the owner manages access.';

  @override
  String get sharingPreview => 'What will be shared';

  @override
  String get sharingExclude =>
      'Passwords, SSH keys, personal references, terminal history and AI keys are excluded. Check commands and free text before sharing.';

  @override
  String get sharingIncludeNotes => 'Include host notes';

  @override
  String get sharingSecretWarning =>
      'The recipient can read and keep this secret. Revoking access cannot remove copies already read. SSH access also requires changing credentials on the target server.';

  @override
  String get sharingSecretConfirmed => 'I understand and allow this secret to be shared with the selected devices';

  @override
  String get sharingHost => 'Host';

  @override
  String get sharingSnippet => 'Snippet';

  @override
  String get sharingGroup => 'Shared collection';

  @override
  String get sharingSecret => 'Secret';

  @override
  String get sharingDevice => 'Device';

  @override
  String get sharingOwner => 'Owner';

  @override
  String get sharingMembers => 'Members';

  @override
  String get sharingVerified => 'Verified';

  @override
  String get sharingBlocked => 'Review required';

  @override
  String get sharingDeleted => 'Deleted';

  @override
  String get sharingRevision => 'Revision';

  @override
  String get sharingQueue => 'Send queue';

  @override
  String get sharingFlush => 'Send pending changes';

  @override
  String get sharingPending => 'Pending';

  @override
  String get sharingConflictHelp =>
      'Access or content changed. Refresh the item and review changes before sending again.';

  @override
  String get sharingRevoke => 'Revoke access';

  @override
  String get sharingRevokeHelp => 'Future updates will use a new key. Copies already read remain with the recipient.';

  @override
  String get sharingStop => 'Delete shared item';

  @override
  String get sharingStopHelp =>
      'The shared item will be deleted for participants. Personal copies and data already read remain.';

  @override
  String get sharingCopyPersonal => 'Save a personal copy';

  @override
  String get sharingCopyHelp =>
      'The copy is saved in your personal vault. Credentials are configured separately and are not shared with colleagues.';

  @override
  String get sharingSaved => 'Done';

  @override
  String get sharingEdit => 'Edit shared item';

  @override
  String get sharingChangeSaved => 'The change was saved or queued for sending.';

  @override
  String get sharingName => 'Name';

  @override
  String get sharingAddress => 'Address';

  @override
  String get sharingPort => 'Port';

  @override
  String get sharingUsername => 'Username';

  @override
  String get sharingNotes => 'Notes';

  @override
  String get sharingDescription => 'Description';

  @override
  String get sharingCommand => 'Command';

  @override
  String get sharingNoDevices => 'No devices found. Check the address and your colleague’s email verification.';

  @override
  String get sharingNoAutoRun => 'Received commands never run automatically.';

  @override
  String get sharingOwnerDeviceOnly => 'Manage access on the owner device that created this item.';

  @override
  String get sharingWorking => 'Preparing secure sharing…';

  @override
  String get sharingCopyCode => 'Copy verification code';

  @override
  String get sharingAddRecipient => 'Add recipient';

  @override
  String get sharingRefreshCopy => 'Refresh personal copy';

  @override
  String get sharingManageAccess => 'Manage access';

  @override
  String get sharingReviewQueue => 'Review queue';

  @override
  String get sharingQueueHelp =>
      'Changes are paused when access or content changes. Review the current item; discard an obsolete request before creating a new change.';

  @override
  String get sharingDiscard => 'Discard queued request';

  @override
  String get sharingDiscardHelp =>
      'Remove this request from this device? Changes already accepted by the server remain. Discarding a request does not revoke published access.';

  @override
  String get sharingReconcile => 'Verify history';

  @override
  String get sharingReconcileHelp =>
      'The local copy is behind the protected checkpoint. Compare the owner’s code. The app will verify the complete signed history and reject missing or conflicting revisions.';

  @override
  String get sharingEndpointChanged =>
      'The shared host address or port changed. Review and confirm the new endpoint before connecting with your credentials.';

  @override
  String get sharingTunnelVerify => 'Verify access to the shared host before starting the tunnel manually.';

  @override
  String get sharingPromptPassword => 'Ask for password when connecting';

  @override
  String get sharingRefreshHost => 'Refresh shared host';

  @override
  String get sharingRefreshHostHelp =>
      'Review the address and port in Sharing. Apply the updated endpoint to your personal copy? Your other connection settings are preserved.';

  @override
  String get sharingDetachHost => 'Detach personal copy';

  @override
  String get sharingDetachHostHelp =>
      'This copy will stop checking changes and access to the shared host. Colleagues keep their own access and copies.';

  @override
  String get sharingCollectionHelp =>
      'Choose independently shared items. A group contains references only and grants no access to their contents.';

  @override
  String get sharingCollectionEmpty => 'Share the required items separately first.';

  @override
  String get sharingCollectionUnavailable => 'Item unavailable: separate access is required.';

  @override
  String get sharingCollectionChildren => 'Group items';

  @override
  String get sharingSecretSelection => 'Secret to share';

  @override
  String get sharingSecretPrimary => 'Password or private key';

  @override
  String get sharingSecretPassphrase => 'Key passphrase (separate item)';

  @override
  String get enrollmentTitle => 'New personal devices';

  @override
  String get enrollmentHelp =>
      'A permission applies to one shared item. A colleague’s trusted device confirms their new device; the owner stays online to grant access. Personal vault data is never transferred.';

  @override
  String get enrollmentCreate => 'Allow new devices';

  @override
  String get enrollmentAnchor => 'Trusted confirming device';

  @override
  String get enrollmentExpiry => 'Permission lifetime';

  @override
  String get enrollmentQuota => 'Number of new devices';

  @override
  String get enrollmentOneDay => '1 day';

  @override
  String get enrollmentSevenDays => '7 days';

  @override
  String get enrollmentThirtyDays => '30 days';

  @override
  String get enrollmentAutomatic => 'Automatically accept confirmed devices';

  @override
  String get enrollmentAutomaticHelp =>
      'Only after code comparison on the trusted device, while the owner is online. The new device cannot exceed the selected permissions.';

  @override
  String get enrollmentDisable => 'Disable permission';

  @override
  String get enrollmentDisableHelp =>
      'This permission will stop admitting new devices. Remove previously admitted devices separately in Manage access. Disabling remains recorded on this device even if the server is unreachable.';

  @override
  String get enrollmentExport => 'Package for the new device';

  @override
  String get enrollmentRequests => 'Device requests';

  @override
  String get enrollmentPrepare => 'I’m on the new device';

  @override
  String get enrollmentEndorse => 'Confirm another device';

  @override
  String get enrollmentSubmit => 'Continue adding a device';

  @override
  String get enrollmentPackage => 'Device package';

  @override
  String get enrollmentPackageHelp =>
      'Transfer this public package between your devices. It contains keys and signed confirmations, without passwords or shared item content. Code comparison is still required.';

  @override
  String get enrollmentPreview => 'Review package';

  @override
  String get enrollmentCodeHelp =>
      'Compare the full code on the new and trusted device in person or through an independent channel. Confirm only an exact match. The code identifies this specific request.';

  @override
  String get enrollmentCopyPackage => 'Copy package';

  @override
  String get enrollmentSavePackage => 'Save package to file';

  @override
  String get enrollmentOpenPackage => 'Open package from file';

  @override
  String get enrollmentPending => 'My device requests';

  @override
  String get enrollmentWait =>
      'Request sent. Keep this device and the owner’s device online. Continue when the owner sends a verification challenge.';

  @override
  String get enrollmentChallenge => 'Verify device keys';

  @override
  String get enrollmentRespond => 'Continue verification';

  @override
  String get enrollmentAccept => 'Grant access';

  @override
  String get enrollmentManualConfirmed => 'I approve adding this device with the displayed permissions';

  @override
  String get enrollmentActive => 'Active';

  @override
  String get enrollmentDisabled => 'Disabled';

  @override
  String get enrollmentRequested => 'Awaiting verification';

  @override
  String get enrollmentChallenged => 'Awaiting device response';

  @override
  String get enrollmentResponded => 'Device keys verified';

  @override
  String get enrollmentAccepted => 'Access granted';

  @override
  String get enrollmentExpired => 'Expired';

  @override
  String get enrollmentBlocked => 'Current access and history must be reviewed';

  @override
  String get enrollmentCreateConfirmed => 'I authorize adding this user’s own devices within the selected limits';

  @override
  String glassOpenTerminalTabs(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count open terminals',
      one: '1 open terminal',
    );
    return '$_temp0';
  }

  @override
  String get updateMacInstallerOpened =>
      'The DMG has been saved and opened. Quit ConsoleCrypt, replace the app in Applications, then launch it again.';

  @override
  String get updateMacSaveTitle => 'Save ConsoleCrypt update';

  @override
  String get updateMacSavePrompt => 'Save and open';

  @override
  String get updateMacDestinationExists =>
      'This file already exists. Retry installation and choose another name or folder for the new DMG.';

  @override
  String get rdpTitle => 'Remote desktop';

  @override
  String get rdpNewConnection => 'New RDP connection';

  @override
  String get rdpEmptyTitle => 'A desktop in a tab';

  @override
  String get rdpEmptyHelp => 'Enter a Windows server and account. Verify the server certificate before signing in.';

  @override
  String get rdpLocked => 'Unlock your vault to open RDP.';

  @override
  String get rdpConnecting => 'Connecting…';

  @override
  String get rdpConnected => 'Connected';

  @override
  String get rdpDisconnected => 'Connection closed. Create a connection to sign in again.';

  @override
  String get rdpConnectionFailed => 'Connection failed. Check the address, account and certificate.';

  @override
  String get rdpSecureAttention => 'Ctrl + Alt + Delete';

  @override
  String get rdpResize => 'Screen 1920 × 1080';

  @override
  String get rdpExpand => 'Full screen';

  @override
  String get rdpCollapse => 'Exit full screen';

  @override
  String get rdpAddress => 'Server address';

  @override
  String get rdpPort => 'Port';

  @override
  String get rdpUsername => 'Username';

  @override
  String get rdpDomain => 'Domain (optional)';

  @override
  String get rdpPassword => 'Windows password';

  @override
  String get rdpCredentialsHelp =>
      'The password is used for this sign-in only and is not saved. It is not sent before you confirm the certificate.';

  @override
  String get rdpInvalidForm => 'Enter an address, a port from 1 to 65535, username and password.';

  @override
  String get rdpCertificateTitle => 'Verify the RDP certificate';

  @override
  String get rdpCertificateHelp =>
      'Compare the entire fingerprint with your administrator over a trusted channel. Confirmation applies only to this connection. Cancel if it does not match.';

  @override
  String get rdpFingerprint => 'Certificate SHA-256';

  @override
  String get rdpCertificateConfirm => 'I compared the entire certificate fingerprint';

  @override
  String get rdpCertificateChanged =>
      'The server certificate changed. Sign-in was blocked; create a new connection and verify the fingerprint.';

  @override
  String get rdpAuthenticationFailed => 'Windows rejected sign-in. Check the username, domain and password.';

  @override
  String get rdpResizeUnavailable => 'The server does not support this screen size. Continue at the original size.';

  @override
  String get rdpWorkspaceHelp =>
      'Remote desktop in a tab. Clipboard and a selected folder are available only with your permission.';

  @override
  String get rdpPermissions => 'Session permissions';

  @override
  String get rdpAllowClipboard => 'Allow text clipboard';

  @override
  String get rdpClipboardHelp =>
      'Ctrl+V (⌘V on Mac) sends and pastes text. Send and Receive exchange text separately. Access is off by default.';

  @override
  String get rdpFolderHelp => 'Give the remote computer access only to a selected folder. Read-only by default.';

  @override
  String get rdpSelectFolder => 'Select folder…';

  @override
  String get rdpStopFolder => 'Disconnect folder';

  @override
  String get rdpNoFolder => 'No folder shared';

  @override
  String get rdpAllowFolderWrite => 'Allow writes to this folder';

  @override
  String get rdpFolderWriteHelp =>
      'The remote computer will be able to create, change and delete files in this folder.';

  @override
  String get rdpFolderUnsupported => 'Folder sharing is unavailable on this device.';

  @override
  String get rdpPermissionsFailed => 'Could not apply permissions. Try again.';

  @override
  String get rdpClipboardReceived => 'Text received. The clipboard will expire according to your security settings.';

  @override
  String get rdpClipboardSent => 'Text sent to the remote computer.';

  @override
  String get rdpClipboardFailed => 'Could not exchange clipboard text.';

  @override
  String get rdpSendClipboard => 'Send clipboard text';

  @override
  String get rdpReceiveClipboard => 'Receive clipboard text';

  @override
  String get rdpSavedPasswordHelp => 'The password will be taken from this host’s encrypted credential.';

  @override
  String get hostProtocolLabel => 'Connection type';

  @override
  String get hostRdpConnectionHelp =>
      'RDP connects directly. The port and credential are set for this host; SSH group settings do not apply.';

  @override
  String get hostRdpPasswordOnly => 'Select a password credential for RDP.';

  @override
  String get hostRdpUsernameRequired => 'Enter the Windows username';

  @override
  String get rdpFolderPending => 'Windows is connecting the folder…';

  @override
  String get rdpFolderReady => 'Folder connected. Open it in Windows File Explorer.';

  @override
  String get rdpFolderDenied =>
      'Windows rejected the folder. Ask your administrator to allow drive redirection; the remote desktop remains connected.';

  @override
  String get rdpFolderUnavailable =>
      'Windows did not connect the folder. Check that drive redirection is allowed on the server, then reconnect.';

  @override
  String get rdpFolderChooseFailed => 'Could not open the selected folder. Check access to it and select it again.';

  @override
  String get rdpFolderChooseLimit =>
      'Could not select another folder. Cancel unused selections or close unneeded sessions and try again.';

  @override
  String get rdpFolderOpenHelp =>
      'Once connected, open This PC in Windows File Explorer or enter this path in the address bar:';

  @override
  String get rdpFolderWindowsPath => '\\\\tsclient\\ConsoleCrypt';

  @override
  String get rdpFolderLimits =>
      'Up to 256 MiB per file and 512 MiB of writes per session. Links and special files are unavailable. Changing the folder interrupts files currently open in it.';

  @override
  String get rdpClipboardUnavailable =>
      'The text clipboard is not ready or is disabled by the server. Wait and try again; if the error persists, check redirection settings with your administrator.';

  @override
  String get rdpClipboardLimit => 'Only text up to 64 KiB can be transferred.';

  @override
  String get rdpClipboardBusy => 'The previous clipboard request is still being processed. Wait and try again.';

  @override
  String get rdpClipboardEmpty => 'No text is available in the clipboard.';

  @override
  String get rdpPinBar => 'Pin connection bar';

  @override
  String get rdpUnpinBar => 'Auto-hide connection bar';

  @override
  String get rdpConnections => 'Switch connection';

  @override
  String get rdpMinimize => 'Minimize window';

  @override
  String get rdpDisconnect => 'Disconnect this session';

  @override
  String get rdpShowBar => 'Show connection bar (Ctrl+Alt+Home)';

  @override
  String get rdpFullscreenFailed => 'Could not change full-screen mode. Try the window controls.';
}
