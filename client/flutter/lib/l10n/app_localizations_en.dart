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
  String get commandToggleSidebar => 'Toggle Sidebar';

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
  String get notesShared => 'Shared notes';

  @override
  String get notesSharedRefresh => 'Refresh shared notes';

  @override
  String get notesSharedNew => 'New shared note';

  @override
  String get notesSharedFailed => 'Shared notes could not be loaded. Check the connection to the sync server.';

  @override
  String get notesSharedWaiting =>
      'The owner has not opened shared notes for this object yet. They appear once the owner opens the notes panel.';

  @override
  String notesSharedPending(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count notes become readable after the owner opens the notes panel',
      one: '1 note becomes readable after the owner opens the notes panel',
    );
    return '$_temp0';
  }

  @override
  String get notesSharedEmpty => 'No shared notes yet.';

  @override
  String get notesSharedHint => 'Everyone you share this object with sees the note. Only you can change it.';

  @override
  String get notesSharedReadOnly => 'Only the author can change this note.';

  @override
  String notesSharedDeleteMessage(String title) {
    return '“$title” will be deleted for everyone with access to the object.';
  }

  @override
  String get notesAuthorYou => 'You';

  @override
  String get notesAuthorOwner => 'Owner';

  @override
  String get notesAuthorMember => 'Member';

  @override
  String get navNotes => 'Notes';

  @override
  String get notesThisObject => 'Object';

  @override
  String get notesAll => 'All';

  @override
  String get notesTasks => 'Tasks';

  @override
  String get notesSearchHint => 'Search notes';

  @override
  String get notesNew => 'New note';

  @override
  String get notesEmptyTitle => 'No notes yet';

  @override
  String get notesEmptyMessage => 'Keep tasks, reminders and what to remember about servers, databases and containers.';

  @override
  String notesEmptyObject(String name) {
    return 'Nothing about $name yet';
  }

  @override
  String get notesNothingFound => 'Nothing found';

  @override
  String notesCompleted(int count) {
    return 'Completed ($count)';
  }

  @override
  String get notesTitle => 'Title';

  @override
  String get notesBody => 'Text (Markdown)';

  @override
  String get notesBodyEmpty => 'No text';

  @override
  String get notesEdit => 'Edit';

  @override
  String get notesPreview => 'Preview';

  @override
  String get notesTags => 'Tags, comma-separated';

  @override
  String get notesTask => 'Task';

  @override
  String get notesDue => 'Due';

  @override
  String get notesDone => 'Done';

  @override
  String get notesReminder => 'Reminder';

  @override
  String get notesAttachedTo => 'Attached to';

  @override
  String get notesNoTarget => 'Not attached';

  @override
  String get notesTargetMissing => 'Object not found';

  @override
  String get notesWhenNone => 'None';

  @override
  String get notesWhenInHour => 'In an hour';

  @override
  String get notesWhenThisEvening => 'Today at 18:00';

  @override
  String get notesWhenTomorrow => 'Tomorrow at 09:00';

  @override
  String get notesWhenNextWeek => 'In a week';

  @override
  String get notesWhenCustom => 'Date and time…';

  @override
  String get notesWhenCustomHint => 'YYYY-MM-DD HH:MM';

  @override
  String get notesInvalidTime => 'Enter the date as YYYY-MM-DD HH:MM';

  @override
  String notesOverdueAt(String when) {
    return 'Overdue · $when';
  }

  @override
  String get notesPin => 'Pin';

  @override
  String get notesUnpin => 'Unpin';

  @override
  String get notesSnooze => 'Remind in an hour';

  @override
  String notesAttachHere(String name) {
    return 'Attach to $name';
  }

  @override
  String get notesDetach => 'Detach';

  @override
  String get notesDelete => 'Delete note';

  @override
  String notesDeleteMessage(String title) {
    return '“$title” will be deleted on all your devices.';
  }

  @override
  String get notesMarkDone => 'Mark done';

  @override
  String get notesMarkOpen => 'Mark not done';

  @override
  String get notesBack => 'Back to notes';

  @override
  String get notesSaveFailed => 'Not saved';

  @override
  String get notesSystemReminder => 'A reminder from ConsoleCrypt';

  @override
  String notesReminderToast(String title) {
    return 'Reminder: $title';
  }

  @override
  String get notesOpen => 'Open';

  @override
  String get notesHostNotesTitle => 'Notes from the host card';

  @override
  String get notesHostNotesMove => 'Move to notes';

  @override
  String notesFromHostTitle(String name) {
    return 'Notes: $name';
  }

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
  String get errorOsAuthFailed => 'System authentication failed or was cancelled. Unlock with the vault passphrase.';

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
  String get credentialImportChooseFile => 'Choose file…';

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
  String get glassTabColumnResize => 'Tab column width';

  @override
  String get glassTabColumnCollapse => 'Collapse tabs to icons';

  @override
  String get glassTabColumnExpand => 'Expand tabs';

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
  String get terminalSplit => 'Split';

  @override
  String get terminalSplitRight => 'Split right';

  @override
  String get terminalSplitDown => 'Split down';

  @override
  String get terminalNextPane => 'Next pane';

  @override
  String get terminalClosePane => 'Close pane';

  @override
  String terminalSplitFull(int count) {
    return 'A tab holds up to $count panes';
  }

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
  String terminalKeyPassphrasePromptTitle(String key, String host) {
    return 'Passphrase for key “$key” on $host';
  }

  @override
  String get terminalKeyPassphraseRetry => 'Wrong key passphrase, please try again.';

  @override
  String get terminalKeyPassphraseLabel => 'Key passphrase';

  @override
  String get terminalKeyPassphraseHelper =>
      'Used for this connection only — not saved. Save it with the key credential to skip this step.';

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
  String get recoveryKitCopyWords => 'Copy words';

  @override
  String get recoveryKitPrint => 'Print / Save as PDF';

  @override
  String get recoveryKitPrintHint =>
      'A saved PDF is as secret as the printed sheet — keep it out of cloud-synced folders.';

  @override
  String get recoveryKitPrintFailed => 'Couldn\'t print the Recovery Kit. Write the words down instead.';

  @override
  String get recoveryKitSheetTitle => 'ConsoleCrypt Recovery Kit';

  @override
  String get recoveryKitSheetWarning =>
      'Anyone who has this sheet can open your vault. Keep it offline, somewhere safe: it is the only way back in if you forget your passphrase and lose your trusted devices.';

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
  String get unlockOsAuthReason => 'unlock your ConsoleCrypt vault';

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
  String get recoveryScanQr => 'Scan the QR';

  @override
  String get recoveryQrFromImage => 'QR from a picture';

  @override
  String get recoveryQrNotFound => 'No Recovery Kit QR found. Try a sharper, closer picture or type the words.';

  @override
  String get recoveryScanQrHelp => 'Point the camera at the QR code of your Recovery Kit. Frames stay on this device.';

  @override
  String get recoveryScanQrUnavailable =>
      'The camera is unavailable. Allow camera access in Settings or use a picture of the QR.';

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
    return '$method releases this device\'s unlock key to open the vault. Then set a new passphrase; other devices keep working.';
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
  String get shellCollapseSidebar => 'Collapse sidebar to icons';

  @override
  String get shellExpandSidebar => 'Expand sidebar';

  @override
  String get shellCollapseSidebarShort => 'Collapse';

  @override
  String shellActionWithShortcut(String action, String shortcut) {
    return '$action ($shortcut)';
  }

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
  String get deviceUnlockHelp =>
      'The system releases the vault key only after your fingerprint, face or device password.';

  @override
  String get deviceUnlockLocalOnly =>
      'Only for this vault on this device. Your passphrase and Recovery Key remain available.';

  @override
  String get deviceUnlockEnableReason => 'allow unlocking your ConsoleCrypt vault on this device';

  @override
  String get deviceUnlockNeedsTrust => 'First unlock the vault with its passphrase on this device.';

  @override
  String get deviceUnlockNotEnrolled =>
      'No biometrics are enrolled. Set up fingerprint or face recognition in your device’s system settings, then check availability again.';

  @override
  String get deviceUnlockUnsupported =>
      'Biometric unlock is currently unavailable on this device. Check biometric support and setup in the system settings.';

  @override
  String get deviceUnlockMacPassword =>
      'macOS currently offers its account password. Touch ID needs an enrolled fingerprint: System Settings → Touch ID & Password.';

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
  String get errorOsAuthInvalidated =>
      'Fingerprints or the device sign-in changed. Unlock with the vault passphrase and turn device unlock on again in Settings → Vault.';

  @override
  String deviceUnlockProtectedBySystem(String method) {
    return 'Protected by the system: the key is released only after $method.';
  }

  @override
  String get deviceUnlockConvenienceWarning =>
      'Unlocking without a passphrase is available to any program running under your user account. This is a convenience, not protection.';

  @override
  String get deviceUnlockConvenienceConfirmTitle => 'Unlock without a passphrase?';

  @override
  String get deviceUnlockConvenienceConfirmAction => 'Turn on anyway';

  @override
  String get deviceUnlockKeyringName => 'system keyring';

  @override
  String deviceUnlockReofferMigratedTitle(String method) {
    return 'Turn $method back on?';
  }

  @override
  String get deviceUnlockReofferMigratedMessage =>
      'Unlocking with this device is now protected by the system. It was switched off during the update; turn it on again with one confirmation.';

  @override
  String get deviceUnlockReofferInvalidatedTitle => 'Fingerprints changed';

  @override
  String deviceUnlockReofferInvalidatedMessage(String method) {
    return 'The $method key is no longer valid because the device sign-in changed. Turn unlocking with this device on again?';
  }

  @override
  String get deviceUnlockReofferEnable => 'Turn on';

  @override
  String get deviceUnlockReofferLater => 'Not now';

  @override
  String get recoveryDeviceResetReason => 'reset the ConsoleCrypt vault passphrase';

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
  String get sharingIdentity => 'My device key fingerprint';

  @override
  String get sharingVerifyHelp =>
      'This is your device\'s key fingerprint. To check keys with a colleague, use “Verify” next to their device: compare the safety number by voice or scan the QR code in person. Login and e-mail do not verify device keys.';

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
  String get sharingRdpHost => 'RDP host';

  @override
  String get sharingRdpHelp =>
      'The card carries the address, port, user, domain and tags. Notes and the saved password are shared only when ticked.';

  @override
  String get sharingRdpIncludePassword => 'Include the saved password';

  @override
  String get sharingRdpPasswordWarning =>
      'Recipients get the password itself and can keep it. Share it only with verified people you trust.';

  @override
  String get sharingRdpCopyPassword => 'Save the shared password with my copy';

  @override
  String get sharingRdpNoPassword => 'The password is not shared: it is asked when connecting.';

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
  String get sharingCopyCode => 'Copy fingerprint';

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
      'The local copy is behind the protected checkpoint. The owner\'s keys must be the ones pinned on this device; the app will verify the complete signed history and reject missing or conflicting revisions.';

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
      'Both devices show the same code for this request. Type the code from the other device\'s screen to confirm that you compared it.';

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

  @override
  String get rdpMobileTouchpad => 'Touchpad';

  @override
  String get rdpMobileDirectTouch => 'Direct touch';

  @override
  String get rdpMobileShowKeyboard => 'Show keyboard';

  @override
  String get rdpMobileHideKeyboard => 'Hide keyboard';

  @override
  String get rdpMobileLeftClick => 'Left click at cursor';

  @override
  String get rdpMobileRightClick => 'Right click at cursor';

  @override
  String get rdpMobileHelpTitle => 'Phone controls';

  @override
  String get rdpMobileHelp =>
      'The desktop is a touchpad. Slide one finger to move the cursor; lift and reposition your finger to continue.\n\nTap to left-click, tap twice to double-click. Hold the second tap and move to drag. You can also hold one finger for half a second, then drag.\n\nTwo fingers: tap to right-click, slide together to scroll, pinch to zoom from 1× to 4×. Double-tap with two fingers to fit the desktop. When zoomed in, the picture follows the cursor.\n\nTap with three fingers to show or hide the keyboard. It overlays the desktop without shrinking it. In Keys, tap Ctrl, Alt, Win or Shift once for the next key, twice to lock, and again to release.\n\nThe round button opens the menu. Drag it to either edge. Switch to direct touch, change resolution, switch sessions or minimize here. An attached mouse keeps its normal controls.';

  @override
  String get localAiTitle => 'Built-in local AI';

  @override
  String get localAiDescription =>
      'Choose a model for this computer. Chat and command suggestions work offline after installation.';

  @override
  String get localAiModel => 'Model';

  @override
  String localAiDownloadSize(String size) {
    return 'Model download: $size';
  }

  @override
  String get localAiNotInstalled => 'Not installed';

  @override
  String get localAiInstalling => 'Downloading and verifying files…';

  @override
  String get localAiStopped => 'Installed · starts when you send a request';

  @override
  String get localAiStarting => 'Starting the model…';

  @override
  String get localAiReady => 'Ready · running on this computer';

  @override
  String get localAiStopping => 'Stopping…';

  @override
  String get localAiError => 'Local AI needs attention';

  @override
  String get localAiUnsupported => 'Available on supported Windows and macOS computers.';

  @override
  String get localAiDefault => 'Use by default after installation';

  @override
  String get localAiDeviceOnly => 'Only on this computer. Other devices and saved provider settings stay independent.';

  @override
  String get localAiInstall => 'Install local AI';

  @override
  String get localAiStart => 'Start';

  @override
  String get localAiStop => 'Stop';

  @override
  String get localAiRemove => 'Remove selected model';

  @override
  String get localAiResourceHelp =>
      'Uses memory and CPU while running; stops after 15 minutes without requests. Small models may make mistakes. Review commands before running them.';

  @override
  String get localAiOperationFailed => 'The local AI operation could not be completed. Check its status and try again.';

  @override
  String get localAiInstallFailed =>
      'Installation failed. Check the internet connection and free disk space, then try again.';

  @override
  String get localAiFilesChanged => 'The installed files could not be verified. Remove the model and install it again.';

  @override
  String get localAiStartFailed => 'The model could not start or stopped unexpectedly. Free some memory and try again.';

  @override
  String get localAiStorageUnavailable =>
      'Cannot access local AI files. Check app data folder permissions and free disk space.';

  @override
  String localAiRamRecommendation(int ram) {
    return 'Recommended total RAM: $ram GB. Speed depends on your hardware.';
  }

  @override
  String get localAiModelsPreserved =>
      'Switching models keeps other downloads. Installed models and your choice survive app updates.';

  @override
  String get localAiTierCustom => 'Custom';

  @override
  String get localAiImport => 'Add your own model (GGUF)…';

  @override
  String get localAiImportHelp =>
      'The file is copied into the ConsoleCrypt models folder and checked by SHA-256; you can delete the original. Any GGUF the built-in llama.cpp reads will do; memory guidance is estimated from the file size.';

  @override
  String get localAiCustomModelNotice => 'Your own model: check its origin and license yourself.';

  @override
  String get localAiInvalidGguf => 'This is not a GGUF model file.';

  @override
  String get localAiImportFailed => 'Could not add the model. Check free disk space.';

  @override
  String get localAiCustomMissing => 'The custom model file is missing. Remove it and add it again.';

  @override
  String get localAiModelInstalled => 'Installed';

  @override
  String get localAiRuntimeMissing => 'The inference engine is missing. Repair it; downloaded models will be kept.';

  @override
  String get localAiBundledRuntimeMissing =>
      'The app’s inference engine is unavailable. Reinstall the application; downloaded models will be kept.';

  @override
  String get localAiRepairRuntime => 'Repair engine';

  @override
  String get localAiInstallSelectedHelp =>
      'Install the selected model to use local AI. Your local default stays enabled; requests will not switch to a cloud provider.';

  @override
  String get localAiTierLight => 'Light';

  @override
  String get localAiTierMedium => 'Medium';

  @override
  String get localAiTierStrong => 'Heavy';

  @override
  String localAiModelRequirements(String size, int ram) {
    return '$size download · $ram GB RAM recommended';
  }

  @override
  String get localAiRecommended => 'Recommended for this computer’s total RAM';

  @override
  String localAiDetectedMemory(String size) {
    return 'This computer: $size total RAM. The star is a suggestion; your selection stays unchanged.';
  }

  @override
  String get localAiMetalHelp =>
      'Apple Silicon uses Metal acceleration. Larger models need more memory and can respond more slowly.';

  @override
  String get localAiCpuHelp =>
      'Windows and Intel Mac use the CPU. RAM guidance does not guarantee fast responses; larger models can be slow.';

  @override
  String localAiModelAttribution(String publisher, String license) {
    return 'Model / GGUF: $publisher · License: $license';
  }

  @override
  String get localAiModelNotices => 'Model sources and licenses';

  @override
  String get terminalLocalSearch => 'Search shells or SSH hosts';

  @override
  String get terminalLocalSection => 'Local shells';

  @override
  String get terminalLocalSshSection => 'SSH hosts';

  @override
  String get terminalLocalComputer => 'This computer';

  @override
  String get terminalLocalUnavailable => 'No local shells are available.';

  @override
  String get terminalLocalSandboxUnavailable => 'Local shells are unavailable in this sandboxed build.';

  @override
  String get terminalLocalRefresh => 'Refresh local shells';

  @override
  String get terminalLocalEmptyMessage => 'Open a local shell or connect to an SSH host in a new tab.';

  @override
  String get terminalLocalOpen => 'Open terminal';

  @override
  String get terminalLocalRestart => 'Restart shell';

  @override
  String get terminalLocalInsertOnly => 'Insert the command, review it, then press Enter in the local terminal.';

  @override
  String get rdpMobileMenu => 'Session controls';

  @override
  String get rdpMobileKeys => 'Keys';

  @override
  String get rdpMobileFit => 'Fit desktop';

  @override
  String get rdpMobileFitResolution => 'Match screen resolution';

  @override
  String get rdpMobileOrientation => 'Orientation';

  @override
  String get rdpMobileAutomatic => 'Automatic';

  @override
  String get rdpMobileLandscape => 'Landscape';

  @override
  String get rdpMobilePortrait => 'Portrait';

  @override
  String get rdpMobileMinimize => 'Minimize session';

  @override
  String get rdpMobileBackAgain => 'Press Back again to minimize the session';

  @override
  String get rdpMobileDisconnectConfirm => 'Disconnect from the remote desktop?';

  @override
  String get rdpMobileClipboard => 'Clipboard';

  @override
  String get rdpMobileIntroMove =>
      'Slide your finger on the desktop to move the cursor. Lift and reposition your finger to continue.';

  @override
  String get rdpMobileIntroZoom => 'Two fingers: tap to right-click, slide to scroll, pinch to zoom.';

  @override
  String get rdpMobileIntroMenu =>
      'Tap with three fingers for the keyboard. The floating round button opens the menu; drag it to either edge.';

  @override
  String get rdpMobileDontShow => 'Do not show again';

  @override
  String get rdpMobileSettingsTitle => 'RDP · Phone controls';

  @override
  String get rdpMobileSettingsLocal =>
      'These preferences stay on this device. The last input mode is remembered separately for each host.';

  @override
  String get rdpMobileDefaultMode => 'Default input mode';

  @override
  String get rdpMobileAutoDefault => 'Touchpad on phones, direct touch on tablets';

  @override
  String get rdpMobileSpeed => 'Pointer speed';

  @override
  String get rdpMobileAcceleration => 'Pointer acceleration';

  @override
  String get rdpMobileCursorSize => 'Cursor size';

  @override
  String get rdpMobileAutoResize => 'Match resolution to the screen';

  @override
  String get rdpMobileClarity => 'Clarity';

  @override
  String get rdpMobileWindowsScale => 'Windows scale';

  @override
  String get rdpMobileAutoHide => 'Auto-hide controls';

  @override
  String get paletteNewLocalTerminal => 'New local terminal';

  @override
  String get hostImportTitle => 'Import system hosts';

  @override
  String get hostImportHelp =>
      'Select addresses from known_hosts. This file contains no usernames or passwords; set them after import. Trusted keys are not changed; verify the server key on first connection.';

  @override
  String get hostImportReadFailed =>
      'Could not read a known_hosts file. Check access permissions and the file size (up to 4 MiB).';

  @override
  String get hostImportSaveFailed =>
      'Could not save all hosts. Saved hosts are marked in the list; retry to add the remaining hosts.';

  @override
  String hostImportSkipped(int count) {
    return 'Skipped entries: $count. Hashed addresses, patterns, CA records, revoked and malformed keys do not create hosts.';
  }

  @override
  String hostImportSaved(int count) {
    return 'Hosts added: $count.';
  }

  @override
  String get hostImportSelectAll => 'Select all new hosts';

  @override
  String get hostImportEmpty =>
      'No addresses found to import. The file may contain only hashed names, which cannot be recovered.';

  @override
  String get hostImportExists => 'Already in this profile';

  @override
  String get commonImport => 'Import';

  @override
  String get kubeTitle => 'Kubernetes';

  @override
  String get kubeSubtitle => 'Cluster connections in your encrypted vault.';

  @override
  String get kubeImport => 'Import kubeconfig';

  @override
  String get kubeImportHelp =>
      'Select a kubeconfig or paste YAML. Choose the contexts to save. Import does not connect to a cluster or run authentication commands.';

  @override
  String get kubeChooseFile => 'Choose file';

  @override
  String get kubeYaml => 'Kubeconfig YAML';

  @override
  String get kubePreview => 'Review contexts';

  @override
  String get kubeSaveContexts => 'Save selected contexts';

  @override
  String get kubeChooseAgain => 'Choose another configuration';

  @override
  String get kubeEmpty => 'No Kubernetes connections yet. Import a kubeconfig to add contexts.';

  @override
  String get kubeNamespace => 'Namespace';

  @override
  String get kubeAuthentication => 'Authentication';

  @override
  String get kubeProduction => 'Production: protect against changes';

  @override
  String get kubeApproveFiles => 'Read these referenced files for the selected contexts';

  @override
  String get kubeExecHelp =>
      'This context uses an external authentication command. Importing it does not grant permission to run that command.';

  @override
  String get kubeImportInvalid =>
      'This context cannot be imported. Check its cluster, user and authentication settings in the source configuration.';

  @override
  String get kubeImportFailed =>
      'Could not import the configuration. Check the selected contexts and files, then review it again.';

  @override
  String get kubeFileFailed => 'Select a regular UTF-8 file no larger than 4 MiB.';

  @override
  String get kubeExpired => 'This preview has expired. Review the configuration again.';

  @override
  String get kubeTlsRequired =>
      'Production requires TLS certificate verification. Correct the kubeconfig or mark this as a non-production context.';

  @override
  String get kubeRoute => 'Connection route';

  @override
  String get kubeRouteDirect => 'Direct connection';

  @override
  String get kubeRouteRequired =>
      'This kubeconfig specifies a proxy. Select a saved SSH route that can reach the API server.';

  @override
  String get kubeSaved => 'Selected contexts saved.';

  @override
  String get kubeDeleteHelp =>
      'Remove this saved context from your vault? This does not change anything in the Kubernetes cluster.';

  @override
  String get kubeLoadFailed => 'Could not load Kubernetes connections.';

  @override
  String get kubeRefresh => 'Refresh connections';

  @override
  String get kubeContextLimit => 'Select up to 64 contexts per import.';

  @override
  String get kubeAuthToken => 'Token';

  @override
  String get kubeAuthCertificate => 'Client certificate';

  @override
  String get kubeAuthExec => 'External command';

  @override
  String get kubeAuthNone => 'Not configured';

  @override
  String get kubeConnect => 'Connect';

  @override
  String get kubeDisconnect => 'Disconnect';

  @override
  String get kubeConnecting => 'Connecting to cluster…';

  @override
  String get kubeConnectionFailed => 'Could not connect. Check the network, authentication and connection route.';

  @override
  String get kubeRetry => 'Try again';

  @override
  String get kubeBackToContexts => 'Saved connections';

  @override
  String get kubeReadOnly => 'Production · read only';

  @override
  String get kubeReadOnlyHelp => 'This context is protected against changes by the native Kubernetes client.';

  @override
  String get kubeResources => 'Resources';

  @override
  String get kubeAllNamespaces => 'All namespaces';

  @override
  String get kubeResourceKind => 'Resource type';

  @override
  String get kubeLabelSelector => 'Label selector';

  @override
  String get kubeApplyFilter => 'Apply filter';

  @override
  String get kubeNoResources => 'No resources match these filters.';

  @override
  String get kubeNoResourceKinds =>
      'No readable resource types were discovered. Check this account’s Kubernetes permissions.';

  @override
  String get kubeRequestFailed => 'Could not load cluster data. Check the connection and Kubernetes permissions.';

  @override
  String get kubeNamespacesFailed =>
      'The namespace list is unavailable. You can still view resources in the selected namespace.';

  @override
  String get kubeLoadMore => 'Load more';

  @override
  String get kubeRefreshResources => 'Refresh resources';

  @override
  String get kubeLive => 'Live updates';

  @override
  String get kubeSnapshot => 'Snapshot';

  @override
  String get kubeWatchFailed => 'Live updates stopped. Refresh to reconnect.';

  @override
  String get kubeResourceLimit => 'The in-memory limit is 10,000 resources. Narrow the namespace or label filter.';

  @override
  String get kubeOverview => 'Overview';

  @override
  String get kubeObjectYaml => 'YAML';

  @override
  String get kubeLogs => 'Logs';

  @override
  String get kubeContainer => 'Container';

  @override
  String get kubePreviousLogs => 'Previous container';

  @override
  String get kubeLogFilter => 'Filter log lines';

  @override
  String get kubeNoLogs => 'No log lines to display.';

  @override
  String get kubeLogTailHelp => 'The last 500 lines are loaded into memory. Refresh to retrieve a new tail.';

  @override
  String get kubeSecretRedacted => 'Secret values are hidden. This view contains metadata and key names only.';

  @override
  String get kubeStatus => 'Status';

  @override
  String get kubeCreatedAt => 'Created';

  @override
  String get kubeLabels => 'Labels';

  @override
  String get kubeConditions => 'Conditions';

  @override
  String get kubeOwners => 'Owners';

  @override
  String get kubeName => 'Name';

  @override
  String get kubeNotReported => 'Not reported';

  @override
  String get kubeSessionEnded => 'The connection has ended. Reconnect to load cluster data.';

  @override
  String kubeLoadedCount(int count) {
    return '$count resources loaded';
  }

  @override
  String get kubeShowDetails => 'Open resource details';

  @override
  String kubeClusterVersion(String version) {
    return 'Kubernetes $version';
  }

  @override
  String get kubeSortName => 'Sort by name';

  @override
  String get kubeSortNamespace => 'Sort by namespace';

  @override
  String get kubeSortStatus => 'Sort by status';

  @override
  String get kubeSortAge => 'Newest first';

  @override
  String get kubeRefreshDetails => 'Refresh details';

  @override
  String get kubeAllowChanges => 'Allow changes for 10 minutes';

  @override
  String get kubeAllowChangesHelp =>
      'This opens a temporary write window for this production context. Changes will be recorded in the local audit log. Enter the vault passphrase to continue.';

  @override
  String get kubeCloseWriteWindow => 'Return to read only';

  @override
  String kubeWriteWindowRemaining(String time) {
    return 'PROD · changes allowed · $time remaining';
  }

  @override
  String get kubeWriteWindowFailed => 'Could not change the write window. Reconnect and try again.';

  @override
  String get kubeEditYaml => 'Edit YAML';

  @override
  String get kubeApplyYaml => 'Apply changes';

  @override
  String get kubeApplyYamlHelp =>
      'Apply the reviewed changes to this resource? Enter the vault passphrase to confirm. The server will reject the update if its resource version has changed.';

  @override
  String get kubeApplyFailed =>
      'The change was not accepted. Refresh the object and check permissions, validation errors or ownership conflicts before trying again.';

  @override
  String get kubeReviewChanges => 'Review changes';

  @override
  String get kubeDryRun => 'Validate on server';

  @override
  String get kubeDryRunRequired =>
      'Validate the YAML on the server first. Review the resulting changes before applying them.';

  @override
  String get kubeDryRunHelp =>
      'Server dry run: removed lines are marked − and added lines +. Nothing has been saved to the cluster yet.';

  @override
  String get kubeForceApply => 'Take ownership of conflicting fields';

  @override
  String get kubeForceApplyHelp =>
      'Only enable this when you intend to replace fields managed by another controller. Validate the changes again after enabling.';

  @override
  String get kubeNoYamlChanges => 'The server returned no YAML changes.';

  @override
  String get kubeDeleteResource => 'Delete Kubernetes resource';

  @override
  String get kubeDeleteResourceHelp =>
      'This deletes the resource from the cluster and may stop running workloads. Enter the vault passphrase to confirm.';

  @override
  String get kubeFollowLogs => 'Follow new log lines';

  @override
  String get kubePauseLogs => 'Pause';

  @override
  String get kubeResumeLogs => 'Resume';

  @override
  String get kubeLogBufferHelp => 'Live log buffer: up to 20,000 lines and 2 MiB, kept only in memory.';

  @override
  String get kubeLogOptions => 'Log options';

  @override
  String get kubeGitOps => 'GitOps';

  @override
  String get kubeProblems => 'Problems';

  @override
  String get kubeHelm => 'Helm';

  @override
  String get kubeInsightsPartial =>
      'Some resources could not be inspected because of permissions, discovery failures or size limits. Results are incomplete.';

  @override
  String get kubeGitOpsAll => 'All';

  @override
  String get kubeGitOpsReady => 'Ready';

  @override
  String get kubeGitOpsAttention => 'Needs attention';

  @override
  String get kubeGitOpsSuspended => 'Suspended';

  @override
  String get kubeDependencies => 'Dependencies';

  @override
  String get kubeNoVisibleProblems => 'No problems were found among the resources that could be inspected.';

  @override
  String get kubeNoProblems => 'No known problems were found in the inspected resources.';

  @override
  String get kubeNoGitOpsResources => 'No Flux or Argo CD resources match this filter.';

  @override
  String get kubeGitOpsActions => 'Controller actions';

  @override
  String get kubeDependencyCycle => 'Dependency cycle detected';

  @override
  String get kubeDependencyMissing => 'Dependency was not found';

  @override
  String get kubeDependencyReady => 'Depends on a ready resource';

  @override
  String get kubeDependencyWaiting => 'Waiting for dependency';

  @override
  String get kubeGitOpsSuspend => 'Pause sync';

  @override
  String get kubeGitOpsResume => 'Resume sync';

  @override
  String get kubeGitOpsReset => 'Reset failure counter';

  @override
  String get kubeGitOpsForce => 'Force Helm upgrade';

  @override
  String get kubeGitOpsReconcile => 'Sync now';

  @override
  String get kubeArgoSync => 'Sync application';

  @override
  String get kubeArgoRefresh => 'Refresh application';

  @override
  String get kubeArgoHardRefresh => 'Refresh application and manifests';

  @override
  String get kubeGitOpsChangeHelp =>
      'This sends a change to the controller and can affect running applications. Review the action and enter the vault passphrase to continue.';

  @override
  String get kubeGitOpsReason => 'Reason (stored in the resource annotation)';

  @override
  String get kubeIssueCrashLoop => 'Container keeps crashing';

  @override
  String get kubeIssueImagePull => 'Container image cannot be downloaded';

  @override
  String get kubeIssueContainerConfig => 'Container configuration is invalid';

  @override
  String get kubeIssueOom => 'Container exceeded its memory limit';

  @override
  String get kubeIssueRestarts => 'Container has restarted repeatedly';

  @override
  String get kubeIssueScheduling => 'Pod cannot be scheduled';

  @override
  String get kubeIssuePodNotReady => 'Pod is not ready';

  @override
  String get kubeIssueNodeNotReady => 'Node is not ready';

  @override
  String get kubeIssueNodePressure => 'Node is under resource pressure';

  @override
  String get kubeIssueCondition => 'Resource requires attention';

  @override
  String get kubeIssueRolloutDeadline => 'Workload rollout exceeded its deadline';

  @override
  String get kubeIssueReplicaFailure => 'Controller could not create replicas';

  @override
  String get kubeIssueReplicasUnavailable => 'Some workload replicas are unavailable';

  @override
  String get kubeIssueVolumePending => 'Volume claim is waiting for storage';

  @override
  String get kubeIssueVolumeLost => 'Volume claim lost its volume';

  @override
  String get kubeIssueJobFailed => 'Job failed';

  @override
  String get kubeIssueArgoDegraded => 'Argo CD application is degraded';

  @override
  String get kubeIssueArgoOutOfSync => 'Argo CD application is out of sync';

  @override
  String get kubeIssueFluxPending => 'Flux has not observed the latest configuration';

  @override
  String get kubeIssueFluxNotReady => 'Flux resource is not ready';

  @override
  String get kubeIssueFluxStalled => 'Flux sync has stalled with an error';

  @override
  String get kubeIssueFluxSuspended => 'Flux sync is paused';

  @override
  String get kubeHelmAllRevisions => 'Show all revisions';

  @override
  String get kubeHelmEmpty => 'No Helm releases match this filter.';

  @override
  String kubeHelmRevision(int revision) {
    return 'Revision $revision';
  }

  @override
  String get kubeHelmHistory => 'Release history';

  @override
  String get kubeHelmSecretsMasked => 'Sensitive values are masked. Values and manifests remain in memory only.';

  @override
  String get kubeHelmValues => 'User values';

  @override
  String get kubeHelmChartValues => 'Chart defaults';

  @override
  String get kubeHelmManifest => 'Manifest';

  @override
  String get kubeHelmCompare => 'Compare with revision';

  @override
  String get kubeHelmChooseRevision => 'Choose revision';

  @override
  String get kubeHelmDiffHelp =>
      'Removed lines are marked − and added lines +. This comparison does not change the release.';

  @override
  String get kubeExecReviewTitle => 'Allow authentication command?';

  @override
  String get kubeExecReviewHelp =>
      'This command runs on this computer. Review its executable, arguments and environment. Approval applies only to this unlocked profile; a changed command requires approval again.';

  @override
  String get kubeExecArguments => 'Arguments';

  @override
  String get kubeExecEnvironment => 'Environment';

  @override
  String get kubeExecAllow => 'Allow this command';

  @override
  String get kubeAuthFailed => 'Authentication could not be completed. Check the connection settings and try again.';

  @override
  String get kubeAuthExpired => 'This request has expired. Start authentication again.';

  @override
  String get kubeOidcTitle => 'Sign in to the cluster';

  @override
  String get kubeOidcHelp =>
      'Sign in with your identity provider in the system browser. Your credentials stay between this device and the provider.';

  @override
  String get kubeOidcOpen => 'Open browser';

  @override
  String get kubeOidcWaiting => 'Waiting for browser sign-in…';

  @override
  String get kubeAuthAction => 'Authenticate';

  @override
  String get kubeExecMobile =>
      'External authentication commands are unavailable on mobile. Use a token, client certificate or browser sign-in (OIDC).';

  @override
  String get kubePodTerminal => 'Pod terminal';

  @override
  String get kubeExecProductionBlocked => 'Terminal access is disabled for this production context.';

  @override
  String get kubeAttach => 'Attach to process';

  @override
  String get kubeAttachHelp =>
      'Connect to the container’s existing process. Keyboard input goes directly to that process.';

  @override
  String get kubeShell => 'Shell in container';

  @override
  String get kubeOpenShell => 'Open shell';

  @override
  String get kubeTerminalFailed =>
      'Could not open the terminal. Check container status, permissions and the selected shell.';

  @override
  String get kubeHidePanel => 'Hide terminal panel';

  @override
  String get kubeShowPanel => 'Open terminal panel';

  @override
  String get kubeTerminalEmpty => 'Open a terminal from a Pod’s details.';

  @override
  String get kubeTerminalConnected => 'Connected';

  @override
  String get kubeTerminalDisconnected => 'Disconnected';

  @override
  String get kubePortForward => 'Forward a Pod port';

  @override
  String get kubeForwardHelp =>
      'The port is available only on this device at 127.0.0.1. The forward stops when you disconnect the cluster or lock the profile.';

  @override
  String get kubeForwardProductionBlocked => 'Port forwarding is disabled for this production context.';

  @override
  String get kubeRemotePort => 'Container TCP port';

  @override
  String get kubeLocalPort => 'Local port';

  @override
  String get kubeLocalPortHelp => 'Use 0 to choose a free port automatically.';

  @override
  String get kubeStartForward => 'Start forwarding';

  @override
  String get kubeForwardCreated => 'Forwarding is running. Manage it in Tunnels or the cluster panel.';

  @override
  String get kubeForwardFailed =>
      'Could not forward this port. Check Pod status, permissions and local port availability.';

  @override
  String get kubePortForwards => 'Kubernetes port forwards';

  @override
  String get kubeNoForwards => 'No active Kubernetes port forwards.';

  @override
  String get kubeForwardRunning => 'Running';

  @override
  String get kubeForwardStopped => 'Stopped';

  @override
  String get kubeStopForward => 'Stop forwarding';

  @override
  String kubeForwardTraffic(int count, String sent, String received) {
    return '$count connections · sent $sent · received $received';
  }

  @override
  String get kubePermissions => 'Access permissions';

  @override
  String get kubePermissionsHelp =>
      'Rules returned by Kubernetes for the current identity in this namespace. Production safeguards remain in effect.';

  @override
  String get kubePermissionsIncomplete =>
      'The server returned an incomplete permission report. Missing rules do not prove that an action is forbidden.';

  @override
  String get kubeScale => 'Scale workload';

  @override
  String get kubeRestart => 'Restart rollout';

  @override
  String get kubeCordon => 'Cordon node';

  @override
  String get kubeUncordon => 'Uncordon node';

  @override
  String get kubeDrain => 'Drain node';

  @override
  String get kubeDebugContainer => 'Add debug container';

  @override
  String get kubeMutationConfirmHelp =>
      'Review the operation and enter the vault passphrase to confirm. Kubernetes permissions and production safeguards are checked again before execution.';

  @override
  String get kubeOperationFailed =>
      'The operation could not complete. Check permissions and resource state, then refresh before trying again.';

  @override
  String get kubeReplicas => 'Replica count (0–1,000,000)';

  @override
  String get kubeContainerImage => 'Container image';

  @override
  String get kubeDebugHelp =>
      'The image is started inside this Pod with /bin/sh. An ephemeral container cannot be removed from the Pod afterward. Use an image you trust.';

  @override
  String get kubeDrainHelp =>
      'The node is already cordoned. Review every Pod before eviction. DaemonSets and mirror Pods are skipped. Disruption budgets remain enforced; unmanaged Pods and local emptyDir data block this operation.';

  @override
  String kubeDrainProgress(int done, int total) {
    return 'Evicted: $done of $total';
  }

  @override
  String get kubeDrainEmpty => 'There are no Pods eligible for eviction.';

  @override
  String get kubeDrainUnmanaged => 'Blocked: Pod has no controller.';

  @override
  String get kubeDrainLocalData => 'Blocked: Pod uses local emptyDir data.';

  @override
  String get kubeDrainBlocked => 'Blocked: review this Pod before draining.';

  @override
  String get kubeMetrics => 'Resource usage';

  @override
  String get kubeMeasuredAt => 'Measured at';

  @override
  String get kubeMetricsUnavailable => 'Metrics are unavailable. Check metrics-server and access permissions.';

  @override
  String get kubeWorkloadPods => 'Workload Pods';

  @override
  String get kubeEvents => 'Kubernetes events';

  @override
  String get kubeFluxCliTitle => 'Flux CLI';

  @override
  String get kubeFluxCliHelp => 'Optional desktop tools. Cluster access uses a temporary local proxy.';

  @override
  String get kubeFluxCliPath => 'Path to Flux';

  @override
  String get kubeFluxCliFind => 'Find';

  @override
  String get kubeFluxCliProbe => 'Check version';

  @override
  String get kubeFluxCliVersion => 'Release version';

  @override
  String get kubeFluxCliMethod => 'Installation method';

  @override
  String get kubeFluxCliInstall => 'Install';

  @override
  String get kubeFluxCliUpdate => 'Update';

  @override
  String get kubeFluxCliRemove => 'Uninstall';

  @override
  String get kubeFluxCliOfficial => 'Official signed release';

  @override
  String get kubeFluxCliConfirm => 'Confirm CLI operation';

  @override
  String get kubeFluxCliVerify =>
      'Official downloads require SHA-256 and Cosign verification. Package commands open in a visible terminal.';

  @override
  String get kubeFluxCliFailed => 'The CLI operation failed or verification was unavailable.';

  @override
  String get kubeFluxCliOutput => 'CLI output';

  @override
  String get kubeFluxCliRun => 'Run';

  @override
  String get kubeFluxCliTerminal => 'Open Flux terminal';

  @override
  String get kubeFluxCliLocalPath => 'Local repository directory for diff';

  @override
  String get kubeFluxCliResource => 'Resource name';

  @override
  String get kubeFluxCliKind => 'Resource kind';

  @override
  String get kubeNewConnection => 'New connection';

  @override
  String get kubeConnectionSettings => 'Connection settings';

  @override
  String get kubeServerUrl => 'Kubernetes API URL (HTTPS)';

  @override
  String get kubeCertificateAuthority => 'CA certificate (PEM, optional)';

  @override
  String get kubeClientCertificate => 'Client certificate (PEM)';

  @override
  String get kubePrivateKey => 'Client private key (PEM)';

  @override
  String get kubeManualReviewHelp =>
      'Continue to review and import this context. Production protection is enabled by default.';

  @override
  String get kubeProductionContext => 'Production context';

  @override
  String get kubeAllowExecInProd => 'Allow Pod terminals in production';

  @override
  String get kubeAllowForwardInProd => 'Allow port forwarding in production';

  @override
  String get kubeRecordEvents => 'Keep event history';

  @override
  String get kubeRecordEventsHelp => 'Encrypted on this device for 7 days. Event history is not synchronized.';

  @override
  String get kubeScanFolder => 'Scan a kubeconfig folder';

  @override
  String get kubeScanEmpty => 'No kubeconfig files were found in this folder.';

  @override
  String kubeQueuedFiles(int count) {
    return '$count files queued for review';
  }

  @override
  String get kubeNextFile => 'Review next file';

  @override
  String get kubeExport => 'Export kubeconfig';

  @override
  String get kubeExportHelp => 'A kubeconfig file can contain credentials. Save it only to a trusted location.';

  @override
  String get kubeIncludeCredentials => 'Include credentials';

  @override
  String get kubeExportSaved => 'Kubeconfig saved.';

  @override
  String get kubeAiAssistant => 'AI assistant';

  @override
  String get kubeAiHelp =>
      'Read-only cluster tools. Secret values are excluded; Kubernetes permissions and your chosen context limit access. Answers never run commands.';

  @override
  String get kubeAiProvider => 'AI provider';

  @override
  String get kubeAiDefaultProvider => 'Default provider on this device';

  @override
  String get kubeAiConfigMaps => 'Allow reading ConfigMap values for this question';

  @override
  String get kubeAiUnresolved =>
      'Some checks could not be completed; review the tool errors before drawing conclusions.';

  @override
  String get kubeAiReadTool => 'Read-only tool';

  @override
  String get kubeAiTruncated => 'Result was truncated.';

  @override
  String get kubeAiToolFailed => 'This check could not complete. Data may be missing or access may be restricted.';

  @override
  String get kubeAiFailed =>
      'The assistant could not complete the request. Check the selected AI provider and cluster connection.';

  @override
  String get kubeAiQuestion => 'Ask about this cluster';

  @override
  String get kubeAiAsk => 'Ask assistant';

  @override
  String get kubeAiExternalTitle => 'Send cluster context to this provider?';

  @override
  String get kubeAiExternalHelp =>
      'This question may send sanitized resource metadata, status and logs to the external provider shown above. Secret values remain excluded. Approval applies only to this question.';

  @override
  String get kubeAiNoTools =>
      'This provider cannot call tools; the assistant will use only the available read-only context.';

  @override
  String get kubeAiExplain => 'Explain with AI';

  @override
  String kubeAiExplainQuestion(String problem, String resource) {
    return 'Explain the problem $problem for $resource. Check related status and events without changing resources.';
  }

  @override
  String get sharingKubeContext => 'Kubernetes context';

  @override
  String get sharingKubeCluster => 'Cluster';

  @override
  String get sharingKubeExclude =>
      'Only the connection settings are shared by default. Authentication, local routes and approvals remain on this device.';

  @override
  String get sharingKubeIncludeCredentials => 'Include authentication credentials';

  @override
  String get sharingKubeCredentialWarning =>
      'Recipients can use these credentials to access the cluster. Share them only with verified devices you trust. Revoking the share does not revoke credentials already copied by a recipient.';

  @override
  String get sharingKubeCredentialsIncluded => 'Authentication is included; values stay hidden.';

  @override
  String get sharingKubeCredentialsExcluded => 'Authentication is not included.';

  @override
  String get sharingKubeCopyHelp =>
      'Save a personal copy of this connection. Choose your own route. Authentication is imported only with your explicit selection; executable plugins still require approval on this device.';

  @override
  String get sharingKubeImportCredentials => 'Import the included authentication credentials';

  @override
  String get sharingKubeRouteRequired =>
      'This connection requires a route. Select a saved SSH host or jump profile on this device.';

  @override
  String get sharingKubeUnsupported => 'This server does not support sharing Kubernetes contexts.';

  @override
  String get sharingKubeSnapshotChanged => 'Review this connection again if its settings or credentials have changed.';

  @override
  String get kubeHistory => 'Event history';

  @override
  String get kubeHistoryHelp => 'Local encrypted history for the last 7 days. Enable recording in connection settings.';

  @override
  String get kubeHistoryRecording => 'Recording cluster events';

  @override
  String get kubeHistoryDisabled => 'History recording is disabled';

  @override
  String get kubeHistoryConnecting => 'Reconnecting the event recorder…';

  @override
  String get kubeHistoryUnavailable => 'Event recording is unavailable; saved history remains readable.';

  @override
  String get kubeHistoryWarnings => 'Warnings only';

  @override
  String get kubeHistoryClear => 'Clear local event history';

  @override
  String get kubeHistoryClearHelp =>
      'Delete saved event history for this context on this device. Cluster resources are unaffected.';

  @override
  String get kubeHelmComputedValues => 'Computed values';

  @override
  String get kubeRevealSecret => 'Reveal Secret values';

  @override
  String get kubeRevealSecretHelp =>
      'Values are visible only in this protected dialog for 30 seconds. Copies are cleared from the clipboard within 30 seconds. Values are never sent to AI or stored in resource history.';

  @override
  String get kubeSecretExpired => 'Values hidden. Reveal again if needed.';

  @override
  String get kubeCreateResource => 'Create resource';

  @override
  String get kubeManifestTemplate => 'Insert metadata template';

  @override
  String get kubeSchema => 'Kubernetes resource schema';

  @override
  String get kubeHelmRollback => 'Roll back Helm release';

  @override
  String get kubeHelmRollbackHelp =>
      'Review the exact Helm binary, checksum and command. The operation changes cluster resources; Helm hooks are disabled. Enter the vault passphrase to confirm.';

  @override
  String get kubeHelmRollbackFailed =>
      'Rollback could not complete. Check the Helm path, permissions and current release revision. Review the command again before retrying.';

  @override
  String get kubeHelmBinary => 'Absolute path to Helm (optional)';

  @override
  String get kubeAiHistory => 'AI conversation history';

  @override
  String get kubeAiHistoryLocal => 'Saved on this device';

  @override
  String get kubeAiHistorySynced => 'Encrypted synchronization enabled';

  @override
  String get kubeAiHistoryDeleteHelp =>
      'Delete this saved conversation. If synchronized, its deletion will also synchronize.';

  @override
  String get kubeRolloutHistory => 'Rollout history';

  @override
  String get kubeDiffBefore => 'Before';

  @override
  String get kubeDiffAfter => 'After';

  @override
  String get kubeCompare => 'Compare';

  @override
  String get kubeDiffTruncated => 'The diff is truncated. Review the full resource before applying changes.';

  @override
  String get kubeNoChanges => 'No changes';

  @override
  String get kubeDiffReview => 'Review changed fields. Secret values remain hidden.';

  @override
  String get kubeFluxCliAdviceMissing => 'Flux CLI was not found. Install it?';

  @override
  String get kubeFluxCliAdviceUpdate => 'The CLI is older than Flux in this cluster. Update it?';

  @override
  String get kubeFluxCliNeverSuggest => 'Do not suggest again';

  @override
  String get kubeFluxCliLater => 'Not now';

  @override
  String get kubeFluxCliPathHelp => 'Add Flux to PATH';

  @override
  String get kubeFluxCliPathHint =>
      'Open a new terminal window after updating PATH. On macOS/Linux, add the shown line to the indicated profile file.';

  @override
  String get kubeFluxCliPathAdded => 'The directory was added to the user PATH.';

  @override
  String get kubeFluxCliController => 'Flux version in the cluster';

  @override
  String get kubeGitOpsFlow => 'Resource flow';

  @override
  String get kubeGraphList => 'List';

  @override
  String get kubeGraphReset => 'Reset zoom and position';

  @override
  String get kubeIssueCertificateExpiring => 'Certificate expires soon';

  @override
  String get kubeIssueCertificateExpired => 'Certificate has expired';

  @override
  String get kubeIssueEvicted => 'Pod was evicted';

  @override
  String get kubeIssuePendingResources => 'Insufficient cluster resources';

  @override
  String get kubeIssuePendingTaints => 'Node taints prevent scheduling';

  @override
  String get kubeIssuePendingVolume => 'Pod is waiting for a volume';

  @override
  String get kubeIssueSuspendedLong => 'Sync paused for over 24 hours';

  @override
  String get kubeIssueSourceStale => 'Source artifact is out of date';

  @override
  String get kubeIssueHelmDrift => 'Helm release has drifted';

  @override
  String get kubeIssueDependencyNotReady => 'Dependency is not ready';

  @override
  String get kubeIssueDependencyMissing => 'Dependency is missing or unavailable';

  @override
  String get kubeExitCode => 'Exit code';

  @override
  String get kubeCommandOpen => 'Open Kubernetes context';

  @override
  String get kubeCommandAsk => 'Ask AI about Kubernetes';

  @override
  String get kubeCommandContext => 'Cluster context';

  @override
  String get kubeRelatedObjects => 'Related resources';

  @override
  String get kubeRelationOwner => 'Owner';

  @override
  String get kubeRelationDependent => 'Dependent resource';

  @override
  String get kubeRelationVolume => 'Volume connection';

  @override
  String get kubeRelationConfiguration => 'Configuration reference';

  @override
  String get kubeMetricsTrend => 'CPU trend during this session';

  @override
  String get kubeLogSince => 'Time range';

  @override
  String get kubeLogAllTime => 'All time';

  @override
  String get kubeLogLastMinute => 'Last minute';

  @override
  String get kubeLogLastHour => 'Last hour';

  @override
  String get kubeLogLastDay => 'Last day';

  @override
  String get kubeLogRegex => 'Regular expression';

  @override
  String get kubeLogRegexInvalid => 'Invalid or excessively complex pattern.';

  @override
  String get kubeLogLevel => 'Level';

  @override
  String get kubeLogAllPods => 'All pods';

  @override
  String get kubeLogAllLevels => 'All levels';

  @override
  String get kubeLogWrap => 'Wrap lines';

  @override
  String get kubeLogExport => 'Export / share logs';

  @override
  String get kubeLogExportHelp =>
      'Logs may contain secrets. Only lines visible under the current filter will be saved.';

  @override
  String get kubeLogMask => 'Mask recognized secrets before exporting';

  @override
  String get kubeLogTargetsTruncated => 'Showing the first 8 pods. Open a specific pod to view others.';

  @override
  String get kubeLogSaved => 'Logs saved.';

  @override
  String get kubeTags => 'Tags, separated by commas';

  @override
  String get kubeTagsHelp => 'Cluster tags and color are shared by its saved contexts.';

  @override
  String get kubeColor => 'Cluster color';

  @override
  String get kubeNoColor => 'No color';

  @override
  String get kubeFavorite => 'Toggle favorite';

  @override
  String get kubeFavorites => 'Favorites';

  @override
  String get kubeAllKinds => 'All kinds';

  @override
  String get kubeGroupWorkloads => 'Workloads';

  @override
  String get kubeGroupNetwork => 'Network';

  @override
  String get kubeGroupConfig => 'Configuration';

  @override
  String get kubeGroupStorage => 'Storage';

  @override
  String get kubeGroupAccess => 'Access';

  @override
  String get kubeGroupCluster => 'Cluster / CRDs';

  @override
  String get kubeLogDefaultContainer => 'Default container';

  @override
  String get kubeForwardChoosePod => 'Choose a Pod for forwarding';

  @override
  String get kubeForwardChoosePodHelp =>
      'Select the target Pod. The connection starts only after confirming its ports.';

  @override
  String get kubeMemoryTrend => 'Memory trend during this session';

  @override
  String get kubeRequests => 'Requests';

  @override
  String get kubeLimits => 'Limits';

  @override
  String get kubeCapacity => 'Capacity';

  @override
  String get kubeAllocatable => 'Allocatable';

  @override
  String get kubeResourceBudgetsUnavailable => 'Resource requests and limits are unavailable.';

  @override
  String get kubeBulkActions => 'Selected resources';

  @override
  String get kubeBulkHelp =>
      'Review the exact list and enter the vault passphrase. Operations run in order; cancellation stops after the current operation. Up to 100 resources.';

  @override
  String get kubeBulkProductionBlocked => 'Bulk changes are disabled in production contexts.';

  @override
  String get kubeBulkFailed =>
      'The operation stopped. Completed resources are marked; review the remaining resources before retrying.';

  @override
  String get kubeContextTerminal => 'Local terminal for this cluster';

  @override
  String get kubeContextTerminalHelp =>
      'The terminal uses a temporary local API proxy. Cluster credentials stay in ConsoleCrypt; production restrictions remain active.';

  @override
  String get kubeOutput => 'Output';

  @override
  String get kubePinLogs => 'Open logs in the bottom panel';

  @override
  String get kubeResizePanel => 'Resize bottom panel';

  @override
  String get kubeAiProposalReview => 'Review proposal';

  @override
  String get kubeAiProposalContext =>
      'The command will be inserted without execution into a new local terminal for this context.';

  @override
  String get kubeAiHistoryContext => 'History messages in this request';

  @override
  String get kubeAiNewConversation => 'New conversation';

  @override
  String get kubeTableColumns => 'Columns';

  @override
  String get kubeAge => 'Age';

  @override
  String get kubeGitOpsSource => 'Source';

  @override
  String get kubeGitOpsArtifact => 'Artifact';

  @override
  String get kubeGitOpsInterval => 'Reconciliation interval';

  @override
  String get kubeGitOpsRevision => 'Revision';

  @override
  String get kubeGitOpsSignature => 'Signature verification';

  @override
  String get kubeGitOpsInventory => 'Managed resources';

  @override
  String get kubeGitOpsValuesFrom => 'Referenced values';

  @override
  String get kubeGitOpsControllerLogs => 'Controller logs';

  @override
  String get kubeGitOpsPath => 'Repository path';

  @override
  String get kubeGitOpsBranch => 'Branch';

  @override
  String get kubeGitOpsTag => 'Tag / version constraint';

  @override
  String get kubeGitOpsChart => 'Chart';

  @override
  String get kubeGitOpsDrift => 'Drift detection';

  @override
  String get kubeGitOpsHistoryEmpty => 'No reconciliation history is available.';

  @override
  String get kubeGitOpsControllerUnavailable =>
      'Controller logs are unavailable. Check resource discovery and permissions.';

  @override
  String get kubeGitOpsSignatureUnknown => 'Not reported by the controller';

  @override
  String get kubeGitOpsSourceReconcileHelp =>
      'Request a new reconciliation for this source. Enter the vault passphrase to continue.';

  @override
  String get kubeGitOpsMetadataOnly => 'Only object references are shown here. Secret values are hidden.';

  @override
  String get kubeResourceConfiguration => 'Workload and node configuration';

  @override
  String get kubePort => 'Port';

  @override
  String get kubePorts => 'Ports';

  @override
  String get kubeNode => 'Node';

  @override
  String get kubeServiceAccount => 'Service account';

  @override
  String get kubeNodeSelector => 'Node selector';

  @override
  String get kubeReady => 'Ready';

  @override
  String get kubeRestarts => 'Restarts';

  @override
  String get kubeIssueExitCode => 'Exit code';

  @override
  String get kubeVolumeMounts => 'Volume mounts';

  @override
  String get kubeVolumes => 'Volumes';

  @override
  String get kubeUnschedulable => 'Scheduling disabled';

  @override
  String get kubeTaints => 'Taints';

  @override
  String get kubeNodeInfo => 'Node software';

  @override
  String get kubeGraphProblems => 'Paths to problems';

  @override
  String get kubeGraphSearch => 'Find a resource in the graph';

  @override
  String get kubeGraphFold => 'Hide inventory leaves';

  @override
  String get kubeReplicaStatus => 'Ready / desired replicas';

  @override
  String get kubeTableSort => 'Sort resources';

  @override
  String get kubeForwardRestore => 'Restore after reconnecting';

  @override
  String get kubeForwardRestoreHelp =>
      'Only while this context stays open and this profile is unlocked. Leaving the context clears this choice.';

  @override
  String get kubeForwardRestoreFailed =>
      'Some port forwards could not be restored. Check the pod and local port, then start them again.';

  @override
  String get kubeGitOpsReconcileWithSource => 'Sync with source';

  @override
  String get kubeGitOpsReconcileWithSourceHelp =>
      'Queue a sync of the source chain, then of this resource. Controller readiness is shown by the live status. Confirm the exact resource name.';

  @override
  String get kubeForwardNetwork => 'Allow access from the local network';

  @override
  String get kubeForwardBindAddress => 'Local IP address to listen on';

  @override
  String get kubeForwardNetworkWarning =>
      'I understand that other devices may access this port without Kubernetes authentication. Allow this network exposure.';

  @override
  String get kubeHelmComputedPartial =>
      'Values were merged from the saved release; subchart defaults and global imports may be missing.';

  @override
  String get kubeManifestSnippets => 'Insert saved manifest';

  @override
  String get kubeManifestSnippetsHelp =>
      'Choose a kubectl snippet containing YAML or JSON. Review identifiers and variables in the editor before server validation.';

  @override
  String get kubeNoManifestSnippets => 'No manifest snippets yet. Save YAML or JSON as a kubectl snippet in Snippets.';

  @override
  String get kubeGitOpsSources => 'Sources';

  @override
  String get kubeGitOpsNotifications => 'Notifications';

  @override
  String get kubeGitOpsImages => 'Image automation';

  @override
  String get kubeGitOpsRecent => 'Recent controller status changes';

  @override
  String get kubeDockRight => 'Dock panel to the right';

  @override
  String get kubeDockBottom => 'Dock panel at the bottom';

  @override
  String get kubeMaskedEditHelp =>
      'This manifest contains masked values. YAML changes are disabled to prevent replacing credentials with placeholders. Use the individual resource actions or edit the original trusted manifest.';

  @override
  String get kubeAutoShell => 'Detect shell automatically';

  @override
  String get kubeGitOpsLocalAlerts => 'Alerts and reminders on this device';

  @override
  String get kubeGitOpsNotify => 'Notify about Flux failures';

  @override
  String get kubeGitOpsNotifyHelp =>
      'For this context while the app is open. New Not ready or Stalled transitions appear as a toast and a badge.';

  @override
  String kubeGitOpsFailureNotice(String resource, int count) {
    return 'Flux needs attention: $resource ($count)';
  }

  @override
  String kubeGitOpsReminderDue(String resource, int count) {
    return 'Cluster reminder: $resource ($count)';
  }

  @override
  String kubeGitOpsUnread(int count) {
    return 'Unread alerts: $count';
  }

  @override
  String get kubeGitOpsCancelReminder => 'Cancel reminder';

  @override
  String get kubeGitOpsRemind => 'Local reminder';

  @override
  String get kubeGitOpsNoReminder => 'Do not remind';

  @override
  String kubeGitOpsRemindHours(int hours) {
    return 'Remind in $hours h';
  }

  @override
  String get kubePreferencesFailed => 'Device preferences could not be saved or loaded. Try again.';

  @override
  String get kubeContextWslHelp =>
      'WSL requires Bash and either WSL 1 or WSL 2 mirrored networking. Host loopback access is checked before launch; NAT networking is unsupported.';

  @override
  String get kubeContextWslUnavailable =>
      'WSL cannot connect to the local proxy. Use WSL 1, WSL 2 mirrored networking or PowerShell.';

  @override
  String get kubeKubectlMissing => 'kubectl was not found. It is only needed for the local cluster terminal.';

  @override
  String get kubeKubectlInstall => 'Install kubectl';

  @override
  String get kubeKubectlPackageHelp =>
      'The package manager configuration determines the package source, version and installation directory. Keep kubectl within one minor version of the cluster. The command runs in a visible terminal; no kubeconfig is passed to it.';

  @override
  String get kubeKubectlDocs => 'Installation instructions';

  @override
  String get kubeKubectlCheck => 'Check kubectl';

  @override
  String get kubeSchemaFields => 'Fields from the cluster schema';

  @override
  String get kubeSchemaHelp =>
      'Browse fields and their descriptions from the cluster OpenAPI schema. Inserting a field only changes this editor; validate the manifest before applying.';

  @override
  String get kubeSchemaUnavailable =>
      'The cluster schema is unavailable. Server validation is still required before applying.';

  @override
  String get kubeSchemaRequired => 'Required';

  @override
  String get kubeSchemaInsert => 'Insert field at cursor';

  @override
  String get kubeSchemaNoFields => 'No matching schema fields.';

  @override
  String get kubeSchemaParent => 'Parent field';

  @override
  String get kubeSchemaRoot => 'Root fields';

  @override
  String get kubeGitOpsAppliedRevision => 'Applied revision';

  @override
  String get kubeGitOpsLatestRevision => 'Latest source revision';

  @override
  String get kubeGraphExpand => 'Expand objects';

  @override
  String get kubeClusters => 'Clusters';

  @override
  String get kubeExecInspect => 'Inspect command';

  @override
  String get kubeExecImportHelp =>
      'Review the command before importing. Import never runs it; execution requires a separate confirmation when connecting.';

  @override
  String get kubeExecBinaryUnavailable =>
      'The executable is unavailable on this device. Its path and SHA-256 will be checked before execution.';

  @override
  String get kubeGitOpsTimeline => 'Reconciliation timeline';

  @override
  String get kubeHelmCompareLive => 'Compare with cluster';

  @override
  String get kubeHelmCompareHelp =>
      'Read-only comparison of saved manifest fields with live objects. Server defaults and controller reconciliation rules are not evaluated.';

  @override
  String get kubeHelmCompareMasked => 'Masked fields are excluded from comparison.';

  @override
  String get kubeHelmCompareEqual => 'Visible fields match';

  @override
  String get kubeHelmCompareChanged => 'Differences found';

  @override
  String get kubeHelmCompareMissing => 'Object is missing';

  @override
  String get kubeHelmTests => 'Helm tests';

  @override
  String get kubeGitOpsStalled => 'Stalled';

  @override
  String get kubeGitOpsStale => 'Out of date';

  @override
  String get kubeStatusUnknown => 'Not checked yet';

  @override
  String get kubeStatusChecking => 'Checking…';

  @override
  String get kubeStatusAvailable => 'Last check: reachable';

  @override
  String get kubeStatusForbidden => 'Last check: access denied';

  @override
  String get kubeStatusFailed => 'Last check: failed';

  @override
  String get kubeNavCluster => 'Cluster';

  @override
  String get kubeNavCustom => 'Custom resources';

  @override
  String get kubeNavCrdFilter => 'Filter CRDs…';

  @override
  String get kubeNavOverview => 'Overview';

  @override
  String get kubeNavHelmReleases => 'Helm releases';

  @override
  String get kubeLabelSelectorHint => 'Labels: app=api';

  @override
  String get kubeLiveShort => 'Live';

  @override
  String get kubeSnapshotShort => 'Snapshot';

  @override
  String kubeNoResourcesIn(String namespace) {
    return 'No resources in $namespace';
  }

  @override
  String get kubePodIp => 'Pod IP';

  @override
  String get kubeHostIp => 'Host IP';

  @override
  String get kubeQos => 'QoS class';

  @override
  String get kubeAnnotations => 'Annotations';

  @override
  String get kubeContainers => 'Containers';

  @override
  String get kubeControlledBy => 'Controlled by';

  @override
  String get kubeDetailsSection => 'Details';

  @override
  String get kubeUsage => 'Usage';

  @override
  String get kubeOperations => 'Operations';

  @override
  String get kubeSpecSection => 'Specification';

  @override
  String get kubeCloseInspector => 'Close details';

  @override
  String get kubeImages => 'Images';

  @override
  String get kubeShellShort => 'Shell';

  @override
  String get kubeForwardShort => 'Port';

  @override
  String get kubeAiShort => 'Ask AI';

  @override
  String get kubeEditShort => 'Edit';

  @override
  String get kubeHistoryShort => 'Rollouts';

  @override
  String get kubeRevealShort => 'Values';

  @override
  String kubeAiClusterChat(String name) {
    return 'Cluster $name';
  }

  @override
  String get kubeAiGeneralChat => 'General chat';

  @override
  String get kubeToggleNavigator => 'Show or hide resources';

  @override
  String get kubeRevision => 'Revision';

  @override
  String get kubeAppVersion => 'App version';

  @override
  String get kubeUpdated => 'Updated';

  @override
  String get kubeScanHome => 'Find in ~/.kube';

  @override
  String get kubeScanOtherFolder => 'Other folder…';

  @override
  String get kubeImportPasteHint => 'Or paste kubeconfig YAML here';

  @override
  String get kubeEditorConnection => 'Connection';

  @override
  String get kubeEditorOrganization => 'Organization';

  @override
  String get kubeEditorSafety => 'Safety';

  @override
  String get kubeReplicasReady => 'Replicas';

  @override
  String get kubeGitOpsAlertSettings => 'Notifications and reminders';

  @override
  String get kubeFluxCliOpen => 'Flux CLI';

  @override
  String get kubeOverviewNodes => 'Nodes';

  @override
  String get kubeOverviewPods => 'Pods running';

  @override
  String get kubeOverviewNamespaces => 'Namespaces';

  @override
  String get kubeOverviewCpu => 'CPU';

  @override
  String get kubeOverviewMemory => 'Memory';

  @override
  String get kubeOverviewPodPhases => 'Pods by phase';

  @override
  String get kubeNavClusterOverview => 'Cluster overview';

  @override
  String get settingsTerminalFontLabel => 'Font';

  @override
  String get settingsTerminalFontSystem => 'System default';

  @override
  String get settingsTerminalFontCustom => 'Other font…';

  @override
  String get settingsTerminalFontCustomTitle => 'Terminal font';

  @override
  String get settingsTerminalFontCustomHint => 'Installed font name, e.g. JetBrains Mono';

  @override
  String get terminalTabRename => 'Rename…';

  @override
  String get terminalTabRenameTitle => 'Tab name';

  @override
  String get terminalTabResetName => 'Restore original name';

  @override
  String get terminalTabDuplicate => 'Duplicate';

  @override
  String get terminalTabClose => 'Close';

  @override
  String get terminalTabCloseOthers => 'Close other tabs';

  @override
  String get terminalTabCloseRight => 'Close tabs to the right';

  @override
  String get kubeClusterProperties => 'Properties…';

  @override
  String get kubeMarkAsTest => 'Mark as test';

  @override
  String get kubeMarkAsProduction => 'Mark as production';

  @override
  String get kubeMarkAsTestConfirm =>
      'Production safeguards (extra confirmations, read-only by default) will be turned off for this cluster.';

  @override
  String get kubeClusterColorMenu => 'Label colour…';

  @override
  String get settingsTerminalTabSubtitle => 'Tab subtitle';

  @override
  String get settingsTerminalTabSubtitleName => 'Name from the list';

  @override
  String get settingsTerminalTabSubtitleAddress => 'Address or computer name';

  @override
  String get settingsTerminalTabSubtitleOff => 'Hidden';

  @override
  String get settingsTerminalTabPlacement => 'Tab placement';

  @override
  String get settingsTerminalTabPlacementTop => 'Top';

  @override
  String get settingsTerminalTabPlacementLeft => 'Left';

  @override
  String get settingsTerminalTabPlacementHelp =>
      'Left: a column of tabs with names and addresses beside the terminal. Drag its edge to change the width or fold it to icons. The app sidebar folds next to it until you expand it. Saved on this device.';

  @override
  String get kubeApplyConflict =>
      'Another controller owns some of these fields, or the object changed. Turn on \"Take ownership of conflicting fields\" or reload the object.';

  @override
  String get kubeApplyForbidden => 'The cluster does not allow you to change this object.';

  @override
  String get kubeApplyInvalid =>
      'The server rejected the YAML: check the syntax and fields that cannot be changed (most of a Pod\'s spec is immutable).';

  @override
  String kubeApplyOwnedBy(String managers) {
    return 'These fields are managed by $managers. Turn on \"Take ownership of conflicting fields\" to override them; the controller may change them back.';
  }

  @override
  String get kubeApplyRejectedFields => 'The server rejected these fields:';

  @override
  String kubeApplyPodImmutable(String owner) {
    return 'A running Pod only allows changing the image, activeDeadlineSeconds, added tolerations, labels and annotations. Edit its owner ($owner) instead.';
  }

  @override
  String get shellAiNone => 'AI is not set up — open settings';

  @override
  String shellAiDevice(String model) {
    return 'AI: built-in model on this device · $model';
  }

  @override
  String shellAiLocal(String name, String model) {
    return 'AI: local server $name · $model';
  }

  @override
  String shellAiApi(String name, String model) {
    return 'AI: via API $name · $model (requests leave this computer)';
  }

  @override
  String get sshConfigImportButton => 'From ~/.ssh/config';

  @override
  String get sshConfigImportTitle => 'Import from ~/.ssh/config';

  @override
  String get sshConfigImportHelp =>
      'Host entries become hosts with their address, port, user, jump hosts and key. Wildcard blocks (Host *) are applied as defaults.';

  @override
  String sshConfigNotFound(String path) {
    return '$path was not found.';
  }

  @override
  String get sshConfigImportKeys => 'Import keys from IdentityFile';

  @override
  String get sshConfigImportKeysHelp => 'A key is stored in the vault as it is; an encrypted key keeps its passphrase.';

  @override
  String sshConfigVia(String hosts) {
    return 'via $hosts';
  }

  @override
  String get sshConfigProxyCommand => 'ProxyCommand is not supported; the host is imported without it';

  @override
  String sshConfigKeyFailed(String path) {
    return 'Key $path was not imported';
  }

  @override
  String get settingsShortcutsTitle => 'Keyboard shortcuts';

  @override
  String get settingsShortcutsSubtitle =>
      'The primary modifier is ⌘ on macOS and Ctrl on Windows/Linux. App shortcuts are handled before the terminal.';

  @override
  String get settingsShortcutsReset => 'Restore defaults';

  @override
  String get settingsShortcutChange => 'Change';

  @override
  String settingsShortcutResetOne(String shortcut) {
    return 'Restore $shortcut';
  }

  @override
  String get settingsShortcutOff => 'Off';

  @override
  String settingsShortcutCaptureTitle(String command) {
    return 'Shortcut for “$command”';
  }

  @override
  String get settingsShortcutCaptureHint => 'Press a combination with ⌘/Ctrl or Alt. Esc cancels.';

  @override
  String settingsShortcutConflict(String command) {
    return 'Already used by “$command”; that command will lose its shortcut.';
  }

  @override
  String get settingsShortcutReserved =>
      'The system or the terminal uses this combination; it will stop reaching the terminal.';

  @override
  String get settingsShortcutAssign => 'Assign';

  @override
  String get settingsShortcutDisable => 'Turn off';

  @override
  String get settingsShortcutReservedBadge => 'Overlaps the system or terminal';

  @override
  String get kubeMetricsInstall => 'Install metrics-server';

  @override
  String kubeMetricsInstallHelp(String version) {
    return 'Basic metrics-server $version in kube-system: ServiceAccount, RBAC, Service, Deployment and the v1beta1.metrics.k8s.io APIService. The cluster pulls the image from registry.k8s.io.';
  }

  @override
  String get kubeMetricsInsecureTls => 'Kubelets use self-signed certificates (--kubelet-insecure-tls)';

  @override
  String get kubeMetricsInsecureTlsHelp =>
      'Needed for local clusters (OrbStack, kind, Docker Desktop, minikube). Turns off kubelet certificate checks.';

  @override
  String kubeMetricsInstalled(int created, int existing) {
    return 'metrics-server installed ($created created, $existing already present). Metrics appear in about a minute.';
  }

  @override
  String kubeMetricsInstallDenied(String kind, String name) {
    return '$kind $name: the cluster refused. Check your permissions; production clusters need an open write window.';
  }

  @override
  String kubeMetricsInstallFailed(String kind, String name) {
    return '$kind $name was not created.';
  }

  @override
  String kubeMetricsUnknownKind(String kind, String apiVersion) {
    return 'The cluster does not offer $kind ($apiVersion).';
  }

  @override
  String get navPortForwards => 'Port forwards';

  @override
  String kubeForwardsOpen(int count) {
    return 'Port forwards: $count';
  }

  @override
  String get kubeForwardCopyAddress => 'Copy local address';

  @override
  String get kubeForwardsEmptyHelp => 'Start one from a Pod or Service: Port forward.';

  @override
  String get dbTitle => 'Databases';

  @override
  String get dbSubtitle => 'Encrypted connections; queries connect directly from this device.';

  @override
  String get dbUnavailable => 'Database support is unavailable in this build.';

  @override
  String get dbNewConnection => 'New database connection';

  @override
  String get dbImport => 'Import connection';

  @override
  String get dbConnectionString => 'Connection string';

  @override
  String get dbImportHelp => 'The password stays in the encrypted vault. Review the destination before saving.';

  @override
  String get dbImportPgpass => 'Choose .pgpass file';

  @override
  String get dbImportExpired => 'The import review expired. Import the source again.';

  @override
  String get dbEmpty => 'No database connections yet.';

  @override
  String get dbFailed => 'The database operation failed. Check the connection settings and permissions.';

  @override
  String get dbInvalid => 'Check the highlighted fields.';

  @override
  String get dbRequired => 'Required';

  @override
  String get dbName => 'Connection name';

  @override
  String get dbEngine => 'Database engine';

  @override
  String get dbHost => 'Host';

  @override
  String get dbPort => 'Port';

  @override
  String get dbDatabase => 'Database / Redis index';

  @override
  String get dbUsername => 'Username';

  @override
  String get dbPassword => 'Password';

  @override
  String get dbSavePassword => 'Save password in encrypted vault';

  @override
  String get dbSavedPassword => 'An existing saved password is retained unless replaced or removed.';

  @override
  String get dbClearPassword => 'Remove saved password';

  @override
  String get dbProduction => 'Production — read only, writes only in a temporary window';

  @override
  String get dbProductionHelp =>
      'Production connections open read-only. “Unlock production” in the workspace allows writes for 5–30 minutes after confirming with the vault passphrase (or biometrics); every change is still reviewed. Prefer a database account with read-only privileges for everyday work.';

  @override
  String get dbProd => 'PROD';

  @override
  String get dbReadOnly => 'READ ONLY';

  @override
  String get dbWriteAllowed => 'Writes allowed for this session';

  @override
  String get dbAllowWrite => 'Allow writes for this session';

  @override
  String get dbAllowWriteHelp =>
      'Only this non-production session will allow writes. Dangerous queries still require confirmation.';

  @override
  String get dbRevokeWrite => 'Return to read-only mode';

  @override
  String get dbPrivileges => 'Check privileges';

  @override
  String get dbPrivilegesWarning => 'This account may have write privileges. Use a read-only role for production.';

  @override
  String get dbServerReadOnly => 'Server read-only protection is active.';

  @override
  String get dbRoleReadOnly => 'The account\'s read-only role was verified.';

  @override
  String get dbGuardOnly => 'Native query protection is active; server read-only privileges are not verified.';

  @override
  String get dbTls => 'TLS protection';

  @override
  String get dbTlsVerify => 'Verify certificate and hostname';

  @override
  String get dbTlsRequire => 'Require encryption';

  @override
  String get dbTlsPrefer => 'Prefer encryption';

  @override
  String get dbTlsDisable => 'Disable TLS';

  @override
  String get dbTlsWarning =>
      'This mode does not verify the database server identity. Prefer certificate and hostname verification.';

  @override
  String get dbTlsCa => 'Certificate authority (PEM)';

  @override
  String get dbTlsServerName => 'TLS server name override';

  @override
  String get dbClientCertificate => 'Client certificate (PEM)';

  @override
  String get dbPrivateKey => 'Client private key (PEM)';

  @override
  String get dbAuthMechanism => 'Authentication mechanism';

  @override
  String get dbAuthDefault => 'Default';

  @override
  String get dbRoute => 'Connection route';

  @override
  String get dbDirect => 'Direct';

  @override
  String get dbSsh => 'SSH host';

  @override
  String get dbJump => 'SSH jump profile';

  @override
  String get dbProxy => 'Proxy';

  @override
  String get dbRouteHelp => 'TLS verification uses the original database hostname through SSH routes too.';

  @override
  String get dbGroup => 'Group';

  @override
  String get dbUngrouped => 'No group';

  @override
  String get dbTags => 'Tags, separated by commas';

  @override
  String get dbColor => 'Color';

  @override
  String get dbLimits => 'Limits';

  @override
  String get dbMaxRows => 'Initial result rows';

  @override
  String get dbStatementTimeout => 'Query timeout, seconds';

  @override
  String get dbConnectTimeout => 'Connection timeout, seconds';

  @override
  String get dbMemoryLimit => 'Result memory, MiB';

  @override
  String get dbOptions => 'Driver settings';

  @override
  String get dbApplicationName => 'Application name';

  @override
  String get dbSearchPath => 'Search path, separated by commas';

  @override
  String get dbCharset => 'Character set';

  @override
  String get dbMongoAuthSource => 'Authentication database';

  @override
  String get dbMongoReplicaSet => 'Replica set';

  @override
  String get dbMongoSrv => 'Discover using DNS SRV';

  @override
  String get dbMongoReadPreference => 'Read preference';

  @override
  String get dbMongoReadHelp => 'Read preference selects a replica; it does not prohibit writes.';

  @override
  String get dbTestConnection => 'Test connection';

  @override
  String get dbTestSuccess => 'Connection succeeded.';

  @override
  String get dbDisconnect => 'Disconnect';

  @override
  String get dbRefresh => 'Refresh';

  @override
  String get dbDeleteHelp => 'Delete this saved connection? Active sessions will close.';

  @override
  String get dbLocked => 'Unlock the vault to access database connections.';

  @override
  String get dbTree => 'Structure';

  @override
  String get dbDdl => 'Show DDL';

  @override
  String get dbFirstRows => 'Preview first 100 rows';

  @override
  String get dbInsertName => 'Insert identifier';

  @override
  String get dbInspect => 'Object details';

  @override
  String get dbSampleSchema => 'Infer schema from sample';

  @override
  String get dbSampleHelp =>
      'This reads documents from the selected collection to infer field types. No data is sent to AI.';

  @override
  String get dbLoadMore => 'Load more';

  @override
  String get dbEditor => 'Query editor';

  @override
  String get dbNewQuery => 'New query';

  @override
  String get dbRun => 'Run';

  @override
  String get dbRunCurrent => 'Current statement';

  @override
  String get dbRunSelection => 'Selection';

  @override
  String get dbRunScript => 'Whole script';

  @override
  String get dbCancelQuery => 'Cancel query';

  @override
  String get dbFormat => 'Format query';

  @override
  String get dbComment => 'Toggle line comments';

  @override
  String get dbFindReplace => 'Find and replace';

  @override
  String get dbReplace => 'Replace';

  @override
  String get dbReplaceAll => 'Replace all';

  @override
  String get dbVariables => 'Query variables';

  @override
  String get dbVariablesHelp => 'Values are bound as native parameters, never concatenated into the query.';

  @override
  String get dbValueType => 'Value type';

  @override
  String get dbValue => 'Value';

  @override
  String get dbReviewQuery => 'Review query';

  @override
  String get dbQueryBlocked =>
      'This query is blocked by connection policy. Use a non-production connection with appropriate privileges.';

  @override
  String get dbQueryDangerous =>
      'This query can change or delete data. Review the query and enter the vault passphrase.';

  @override
  String get dbReviewExpired => 'The review expired. Run the query again to obtain a fresh review.';

  @override
  String get dbResults => 'Results';

  @override
  String get dbGrid => 'Grid';

  @override
  String get dbRecords => 'Records';

  @override
  String get dbNoResults => 'Run a query to see results.';

  @override
  String get dbNull => 'NULL';

  @override
  String get dbTruncated => 'Result limit reached. Export to stream the full result.';

  @override
  String get dbCancelled => 'Query cancelled.';

  @override
  String get dbRunning => 'Query running…';

  @override
  String get dbMessages => 'Server messages';

  @override
  String get dbShowValue => 'Show full value';

  @override
  String get dbPinColumn => 'Pin column';

  @override
  String get dbUnpinColumn => 'Unpin column';

  @override
  String get dbColumns => 'Columns';

  @override
  String get dbCopyCell => 'Copy cell';

  @override
  String get dbCopyRow => 'Copy row';

  @override
  String get dbCopySelection => 'Copy selection';

  @override
  String get dbCopyResult => 'Copy loaded result';

  @override
  String get dbSensitiveClipboard => 'The clipboard is cleared automatically according to your security settings.';

  @override
  String get dbExport => 'Export result';

  @override
  String get dbExportFormat => 'Format';

  @override
  String get dbExportHeader => 'Include column names';

  @override
  String get dbExportBom => 'UTF-8 byte order mark (BOM)';

  @override
  String get dbExportDelimiter => 'Delimiter';

  @override
  String get dbExportQuote => 'Quote character';

  @override
  String get dbExportHelp => 'Export streams the full result to the selected file. Results may contain sensitive data.';

  @override
  String get dbExportDone => 'Export completed.';

  @override
  String get dbHistory => 'Query history';

  @override
  String get dbFavorites => 'Favorites';

  @override
  String get dbAllQueries => 'All queries';

  @override
  String get dbSuccessful => 'Successful';

  @override
  String get dbWithErrors => 'With errors';

  @override
  String get dbHistoryLocal => 'Local encrypted history';

  @override
  String get dbHistorySync => 'Encrypted synchronization';

  @override
  String get dbHistoryDisabled => 'History disabled';

  @override
  String get dbClearHistory => 'Clear query history';

  @override
  String get dbClearHistoryHelp => 'Delete the saved query history for this connection?';

  @override
  String get dbSaveSnippet => 'Save as snippet';

  @override
  String get dbSnippetName => 'Snippet name';

  @override
  String get dbExplain => 'EXPLAIN';

  @override
  String get dbExplainAnalyze => 'Execute with ANALYZE';

  @override
  String get dbExplainAnalyzeHelp =>
      'ANALYZE executes the query to collect measurements. Available only for non-production connections.';

  @override
  String get dbTrace => 'Trace query';

  @override
  String get dbTraceHelp =>
      'The query runs once with tracing on: every node records its steps and delays in system_traces. Available only for non-production connections.';

  @override
  String get dbEditCell => 'Edit cell';

  @override
  String get dbApplyEdits => 'Review pending edits';

  @override
  String get dbDiscardEdits => 'Discard changes';

  @override
  String get dbCellEditsHelp =>
      'Review the parameterized UPDATE statements. Changes are applied in one transaction; failure rolls it back.';

  @override
  String get dbAi => 'AI assistant';

  @override
  String get dbAiQuestion => 'Describe what you need';

  @override
  String get dbAiProvider => 'AI provider';

  @override
  String get dbAiCompose => 'Compose query';

  @override
  String get dbAiExplain => 'Explain query';

  @override
  String get dbAiFix => 'Fix with AI';

  @override
  String get dbAiOptimize => 'Optimize with AI';

  @override
  String get dbAiTranslate => 'Translate dialect';

  @override
  String get dbAiSnippet => 'Create parameterized snippet';

  @override
  String get dbAiExamples => 'Example queries';

  @override
  String get dbAiResult => 'Explain selected rows';

  @override
  String get dbAiDraft => 'AI draft — review before accepting';

  @override
  String get dbAiAccept => 'Accept draft';

  @override
  String get dbAiReject => 'Reject';

  @override
  String get dbAiRefine => 'Refine';

  @override
  String get dbAiConsent => 'Review AI context';

  @override
  String get dbAiSchema => 'Allow schema metadata';

  @override
  String get dbAiRows => 'Allow explicitly selected result rows';

  @override
  String get dbAiSample => 'Allow sample_rows (up to 20 rows)';

  @override
  String get dbAiReadonly => 'Allow run_readonly (up to 50 rows)';

  @override
  String get dbAiConsentHelp =>
      'Schema and row data require separate consent. Tool calls are read only, shown below, and cannot apply changes.';

  @override
  String get dbAiExternal =>
      'This provider is external. Strict privacy replaces identifiers before sending and restores them locally.';

  @override
  String get dbAiLocal => 'This provider runs locally.';

  @override
  String get dbAiContextLimit => 'Maximum schema objects in context';

  @override
  String get dbAiTools => 'Context sent to the model';

  @override
  String get dbAiNoExecute => 'Accepting a draft never executes it. Use Run after reviewing.';

  @override
  String get dbAiProdDdl => 'Production DDL suggestions are recommendations only and cannot be run.';

  @override
  String get dbAiRevoke => 'Revoke AI consent';

  @override
  String get dbBackgroundIos => 'On iOS, a running query is cancelled after 30 seconds in the background.';

  @override
  String get dbCloseRunning => 'Close this query and cancel its active request?';

  @override
  String get dbOperationFailed => 'The database operation failed. Check the connection and permissions, then retry.';

  @override
  String get dbTreeSearch => 'Search loaded objects';

  @override
  String get dbInvalidValue => 'Check the value and its type.';

  @override
  String get dbComplete => 'Complete query';

  @override
  String get dbBinaryPreview => 'Hex preview (first 4096 bytes)';

  @override
  String get dbAiNewConversation => 'New conversation';

  @override
  String get dbAiHistoryMessages => 'History messages in request';

  @override
  String get dbVerifiedReadOnly => 'Server-side read-only protection is verified.';

  @override
  String get dbRoleCanWrite =>
      'This database role has write privileges. Use a dedicated read-only role for production.';

  @override
  String get dbResultStale => 'Changes applied. Run the query again to refresh this result.';

  @override
  String get dbAllowWritesHelp =>
      'Allow writes for this non-production session? Each changing query still requires review. This permission ends when the connection closes.';

  @override
  String get dbCheckPrivileges => 'Check privileges';

  @override
  String get dbAllowWrites => 'Allow writes';

  @override
  String get dbUnverifiedReadOnly =>
      'Server-side read-only protection is unverified. Use a dedicated read-only database role.';

  @override
  String dbElapsed(int ms) {
    return '$ms ms';
  }

  @override
  String get dbQueryBlockedTitle => 'Query blocked';

  @override
  String get dbScanInsteadOfKeys =>
      'KEYS scans every key at once and can stall the server. Use SCAN to read keys page by page.';

  @override
  String get dbOpenSuggestion => 'Insert into the editor';

  @override
  String get dbPlanByTime => 'Bars show each step\'s own share of the execution time.';

  @override
  String get dbPlanByCost => 'Bars show each step\'s own share of the estimated cost.';

  @override
  String dbPlanTime(String ms) {
    return '$ms ms';
  }

  @override
  String dbPlanRows(int count) {
    return '$count rows';
  }

  @override
  String dbPlanCost(String cost) {
    return 'cost $cost';
  }

  @override
  String get dbPlanTree => 'Plan tree';

  @override
  String get dbPlanJson => 'JSON';

  @override
  String get dbErrorTls =>
      'TLS failed: the server does not offer TLS or its certificate did not pass the check. For a port-forward or a local database choose TLS \"prefer\" or \"disable\"; for a remote server check the certificate and server name.';

  @override
  String get dbErrorAuth => 'Sign-in refused: check the user name, password and database name.';

  @override
  String get dbErrorConnection =>
      'Cannot reach the server: check the address, the port and that the port-forward or tunnel is running.';

  @override
  String get dbErrorTimeout => 'The server did not answer in time. Check the address and the connection timeout.';

  @override
  String get dbErrorRoute => 'The selected route is unavailable: check the SSH host or jump profile.';

  @override
  String get dbTlsLocalHint => 'Local address: TLS set to \"prefer\" (a port-forward usually has no TLS).';

  @override
  String get dbTransportVerified => 'TLS (verified)';

  @override
  String get dbTransportUnverified => 'TLS (certificate not verified)';

  @override
  String get dbTransportTunnel => 'No TLS — protected by a tunnel';

  @override
  String get dbTransportPlaintext => 'No TLS';

  @override
  String get dbTlsPreferHelp => 'TLS if the server supports it; without TLS only through a tunnel or on this computer.';

  @override
  String get dbTlsRequireHelp => 'Encryption without certificate verification.';

  @override
  String get dbTlsAllowUnbound => 'Allow sign-in without TLS binding (less secure)';

  @override
  String get dbTlsAllowUnboundHelp =>
      'Only for servers with md5 or password authentication or without SCRAM-SHA-256-PLUS. Without a verified certificate, an interceptor could then obtain the password.';

  @override
  String get dbErrorTlsUnavailable =>
      'The server does not support TLS. Connecting without encryption would send the password in clear text. Choose \"Disable TLS\" explicitly if that is acceptable.';

  @override
  String get dbErrorChannelBinding =>
      'The server did not bind the password to this TLS session (SCRAM-SHA-256-PLUS), and the certificate is not verified: an interceptor could obtain the password. Choose \"Verify certificate and hostname\", use SCRAM on the server, or allow sign-in without TLS binding in TLS protection.';

  @override
  String get dbErrorCleartext =>
      'The server asked for the password in clear text over an unencrypted network connection. Enable TLS or connect through a tunnel.';

  @override
  String get colorPickerCustom => 'Custom colour';

  @override
  String get kubeConnectDatabase => 'Connect to database';

  @override
  String kubeConnectDatabaseHelp(String engine) {
    return 'Forward the $engine port to localhost and open it in Databases';
  }

  @override
  String get dockerTitle => 'Docker';

  @override
  String get dockerSubtitle => 'Containers on this computer or on a saved SSH host.';

  @override
  String get dockerUnavailable => 'Docker is unavailable';

  @override
  String get dockerUnavailableHelp => 'This build of the app cannot reach Docker engines.';

  @override
  String get dockerSearchHosts => 'Search hosts';

  @override
  String get dockerRefresh => 'Refresh';

  @override
  String get dockerHostsFailed => 'Could not load the host list.';

  @override
  String get dockerNoHosts => 'No hosts';

  @override
  String get dockerNoHostsHelp => 'Docker was not found on this computer and there are no saved SSH hosts yet.';

  @override
  String get dockerLocalHost => 'This computer';

  @override
  String get dockerLocalHostHelp => 'Local Docker engine';

  @override
  String get dockerConnect => 'Connect';

  @override
  String get dockerConnecting => 'Connecting…';

  @override
  String get dockerReconnect => 'Reconnect';

  @override
  String get dockerPermissionDeniedTitle => 'No access to the Docker socket';

  @override
  String dockerPermissionDenied(String host, String group) {
    return 'The SSH user on $host has no rights to the Docker socket. Add the user to the “$group” group on the host and reconnect:';
  }

  @override
  String get dockerPermissionDeniedNote =>
      'Run it on the host yourself; the app never runs it. The new group applies to new SSH sessions.';

  @override
  String get dockerCopyCommand => 'Copy command';

  @override
  String get dockerNotRunningTitle => 'Docker is not running';

  @override
  String dockerNotRunning(String host) {
    return 'Docker is not running or not installed on $host. Start the Docker service there and reconnect.';
  }

  @override
  String get dockerConnectionFailedTitle => 'Connection failed';

  @override
  String dockerConnectionFailed(String host) {
    return 'Could not connect to $host.';
  }

  @override
  String get dockerRequestFailed => 'Docker request failed';

  @override
  String get dockerRetry => 'Retry';

  @override
  String get dockerDisconnect => 'Disconnect and choose another host';

  @override
  String dockerEngineVersion(String version) {
    return 'Engine $version';
  }

  @override
  String dockerRunningOfTotal(int running, int total) {
    return '$running of $total running';
  }

  @override
  String get dockerForwards => 'Port forwards';

  @override
  String get dockerFilterContainers => 'Filter by name or image';

  @override
  String get dockerNoContainers => 'No containers';

  @override
  String get dockerNoContainersHelp => 'Containers started on this engine appear here.';

  @override
  String get dockerNoMatches => 'No matching containers';

  @override
  String get dockerContainerGone => 'The container no longer exists';

  @override
  String get dockerStandalone => 'Containers';

  @override
  String get dockerCompose => 'compose';

  @override
  String dockerGroupRunning(int running, int total) {
    return '$running/$total running';
  }

  @override
  String get dockerStateCreated => 'Created';

  @override
  String get dockerStateRunning => 'Running';

  @override
  String get dockerStatePaused => 'Paused';

  @override
  String get dockerStateRestarting => 'Restarting';

  @override
  String get dockerStateRemoving => 'Removing';

  @override
  String get dockerStateExited => 'Exited';

  @override
  String get dockerStateDead => 'Dead';

  @override
  String get dockerStateUnknown => 'Unknown';

  @override
  String get dockerHealthHealthy => 'Healthy';

  @override
  String get dockerHealthUnhealthy => 'Unhealthy';

  @override
  String get dockerHealthStarting => 'Starting';

  @override
  String get dockerActionStart => 'Start';

  @override
  String get dockerActionStop => 'Stop';

  @override
  String get dockerActionRestart => 'Restart';

  @override
  String get dockerActionPause => 'Pause';

  @override
  String get dockerActionUnpause => 'Resume';

  @override
  String get dockerActionKill => 'Kill';

  @override
  String get dockerActionRemove => 'Remove';

  @override
  String dockerKillTitle(String name) {
    return 'Kill $name?';
  }

  @override
  String get dockerKillHelp => 'The container\'s processes are stopped at once, without a chance to shut down cleanly.';

  @override
  String dockerRemoveTitle(String name) {
    return 'Remove $name?';
  }

  @override
  String get dockerRemoveHelp => 'The container and its writable layer are deleted. Named volumes are kept.';

  @override
  String get dockerLogs => 'Logs';

  @override
  String get dockerTerminal => 'Terminal';

  @override
  String get dockerForwardPort => 'Forward port';

  @override
  String get dockerDetails => 'Details';

  @override
  String get dockerCopyId => 'Copy ID';

  @override
  String get dockerContainerId => 'Container ID';

  @override
  String get dockerOverview => 'Overview';

  @override
  String get dockerInspectJson => 'JSON';

  @override
  String get dockerCopyJson => 'Copy JSON';

  @override
  String get dockerImage => 'Image';

  @override
  String get dockerStatus => 'Status';

  @override
  String get dockerHealth => 'Health';

  @override
  String get dockerCreated => 'Created';

  @override
  String get dockerStarted => 'Started';

  @override
  String get dockerCommand => 'Command';

  @override
  String get dockerRestartCount => 'Restarts';

  @override
  String get dockerComposeProject => 'Compose project';

  @override
  String get dockerComposeService => 'Compose service';

  @override
  String get dockerPorts => 'Ports';

  @override
  String get dockerNetworks => 'Networks';

  @override
  String get dockerMounts => 'Mounts';

  @override
  String get dockerLabels => 'Labels';

  @override
  String get dockerEnvironment => 'Environment';

  @override
  String get dockerLogsPause => 'Pause';

  @override
  String get dockerLogsResume => 'Resume';

  @override
  String get dockerLogsFollow => 'Follow';

  @override
  String get dockerLogsTimestamps => 'Timestamps';

  @override
  String get dockerLogsCopy => 'Copy shown lines';

  @override
  String get dockerLogsClear => 'Clear';

  @override
  String get dockerLogsFind => 'Find in logs';

  @override
  String dockerLogsMatches(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count matches',
      one: '$count match',
      zero: 'No matches',
    );
    return '$_temp0';
  }

  @override
  String get dockerLogsPaused => 'Paused: new lines are kept until you resume';

  @override
  String dockerLogsTrimmed(int limit) {
    return 'Only the last $limit lines are kept';
  }

  @override
  String get dockerLogsFailed => 'The log stream stopped with an error';

  @override
  String get dockerLogsWaiting => 'Waiting for log lines…';

  @override
  String get dockerNoLogs => 'No log lines';

  @override
  String get dockerForwardHelpSsh =>
      'A local port is forwarded to the container port through the SSH connection to the host.';

  @override
  String get dockerForwardHelpLocal => 'A local port is forwarded to the container port on this computer.';

  @override
  String get dockerForwardContainerPort => 'Container port';

  @override
  String get dockerForwardLocalPort => 'Local port';

  @override
  String get dockerForwardLocalHelp => '0 picks a free port';

  @override
  String get dockerForwardStart => 'Start forward';

  @override
  String get dockerForwardCreated => 'The forward is running. It is listed in the port forwards panel.';

  @override
  String get dockerForwardCopyAddress => 'Copy address';

  @override
  String get dockerForwardStop => 'Stop forward';

  @override
  String get dockerForwardRunning => 'Running';

  @override
  String get dockerForwardFailed => 'Failed';

  @override
  String get dockerForwardStopped => 'Stopped';

  @override
  String dockerForwardTraffic(int connections, String sent, String received) {
    return '$connections conn. · ↑ $sent · ↓ $received';
  }

  @override
  String get dockerNoForwards => 'No port forwards';

  @override
  String get dockerNoForwardsHelp => 'Use “Forward port” on a running container.';

  @override
  String get dockerTerminalTitle => 'Container terminal';

  @override
  String get dockerShell => 'Shell';

  @override
  String get dockerAutoShell => 'Detect automatically (bash, ash or sh)';

  @override
  String get dockerOpenShell => 'Open terminal';

  @override
  String get dockerTerminalFailed => 'Could not open a terminal in the container.';

  @override
  String get dockerSocketForwardingDisabled =>
      'The SSH server does not allow forwarding the Docker socket and has no Docker CLI. Set \"AllowStreamLocalForwarding yes\" in sshd_config or install the docker CLI on the host.';

  @override
  String get dockerStateFilter => 'Container state';

  @override
  String dockerFilterAll(int count) {
    return 'All $count';
  }

  @override
  String dockerFilterRunning(int count) {
    return 'Running $count';
  }

  @override
  String dockerFilterStopped(int count) {
    return 'Stopped $count';
  }

  @override
  String get dockerRevealSecrets => 'Show secrets';

  @override
  String get dockerHideSecrets => 'Hide secrets';

  @override
  String dockerRevealHelp(String name) {
    return 'Enter the vault passphrase to show the passwords and tokens in the environment of $name. Copied values clear from the clipboard automatically.';
  }

  @override
  String get dockerVaultPassphrase => 'Vault passphrase';

  @override
  String get dockerWrongPassphrase => 'Wrong vault passphrase.';

  @override
  String get dockerMountType => 'Type';

  @override
  String get dockerMountSource => 'Source';

  @override
  String get dockerMountDestination => 'Destination';

  @override
  String get dockerMountMode => 'Mode';

  @override
  String get dockerMountReadOnly => 'ro';

  @override
  String get dockerMountReadWrite => 'rw';

  @override
  String get dockerView => 'Show';

  @override
  String get dockerVolumes => 'Volumes';

  @override
  String get dockerVolumesOther => 'Other volumes';

  @override
  String dockerVolumeCount(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '$count volumes', one: '1 volume');
    return '$_temp0';
  }

  @override
  String get dockerVolumeUnused => 'not used';

  @override
  String get dockerNoVolumes => 'No volumes on this engine.';

  @override
  String get workspaceForwardsEmptyHelp =>
      'Forward a port from a Kubernetes pod or a Docker container; running forwards show here.';

  @override
  String shellAiDeviceIdle(String model) {
    return 'AI: built-in model not running (starts on request) · $model';
  }

  @override
  String shellAiDeviceStarting(String model) {
    return 'AI: built-in model starting · $model';
  }

  @override
  String shellAiDeviceError(String model) {
    return 'AI: built-in model failed to start · $model';
  }

  @override
  String get dbEditorHint => 'Write a query… ⌘↵ runs it';

  @override
  String dbRowCount(String count) {
    return 'Rows: $count';
  }

  @override
  String dbRowsAffected(int count) {
    return 'Affected: $count';
  }

  @override
  String get dbStatusReady => 'Ready';

  @override
  String dbRunStatementHelp(String shortcut) {
    return 'Run the selection, or the statement under the cursor ($shortcut)';
  }

  @override
  String dbRunScriptHelp(String shortcut) {
    return 'Run the whole script statement by statement ($shortcut)';
  }

  @override
  String get dbFolderTables => 'Tables';

  @override
  String get dbFolderViews => 'Views';

  @override
  String get dbFolderFunctions => 'Functions';

  @override
  String get dbFolderSequences => 'Sequences';

  @override
  String get dbFolderDictionaries => 'Dictionaries';

  @override
  String get dbFolderColumns => 'Columns';

  @override
  String get dbFolderIndexes => 'Indexes';

  @override
  String get dbFolderConstraints => 'Constraints';

  @override
  String get dbFolderTriggers => 'Triggers';

  @override
  String get dbFolderProjections => 'Projections';

  @override
  String get dbCollapseAll => 'Collapse all';

  @override
  String dockerConnectDatabaseHelp(String engine) {
    return 'Open the $engine database of this container in Databases';
  }

  @override
  String dbRouteKubernetes(String name) {
    return 'Kubernetes: $name';
  }

  @override
  String get dbContainerPort => 'Container port';

  @override
  String get dbKubeTarget => 'Forward to';

  @override
  String get dbKubeTargetName => 'Object name';

  @override
  String get dbKubeRouteHelp =>
      'The port-forward is restored on every connect, or a running one is reused. Services and Deployments use a ready pod, so the connection survives pod restarts.';

  @override
  String get dbErrorKubeUnreachable =>
      'Cannot reach the Kubernetes cluster to restore the port-forward: check the context, the network and your sign-in.';

  @override
  String get dbErrorKubeTarget =>
      'The pod for the port-forward was not found: check the namespace and the object name in the connection route.';

  @override
  String get dbErrorKubeDenied => 'The cluster denied access to the pod or its port-forward.';

  @override
  String get dbErrorKubeForward => 'Could not start the port-forward: stop unused forwards and try again.';

  @override
  String get dbRouteKubeHint => 'Kubernetes port-forward, restored on connect';

  @override
  String dbRouteTunnel(String name) {
    return 'Tunnel: $name';
  }

  @override
  String get dbRouteDocker => 'Docker container';

  @override
  String get dbDockerEngine => 'Docker engine';

  @override
  String get dbDockerContainer => 'Container name';

  @override
  String get dbDockerRouteHelp =>
      'The port-forward to the container is restored on every connect, or a running one is reused. The container is found by name, so a re-created container still works.';

  @override
  String get dbTunnelRouteHelp =>
      'The saved tunnel is started on connect when it is not running. The address and port are the tunnel target.';

  @override
  String dbUseTunnel(String name) {
    return 'Use the saved tunnel “$name”';
  }

  @override
  String get dbErrorTunnel => 'Could not start the saved tunnel: check its SSH host in Tunnels.';

  @override
  String get dbErrorTunnelKind => 'Only a local port-forward tunnel can carry a database connection.';

  @override
  String get dbErrorDockerUnreachable =>
      'Cannot reach the Docker engine to restore the port-forward: check that it is running and the SSH host.';

  @override
  String get dbErrorDockerContainer =>
      'The container was not found or is not running: check its name in the connection route.';

  @override
  String get dbErrorDockerForward => 'Could not start the Docker port-forward: stop unused forwards and try again.';

  @override
  String get dockerOpenInSftp => 'Open in SFTP';

  @override
  String get dockerVolumeInVm =>
      'This volume is stored inside the Docker virtual machine, so its folder can\'t be opened from this computer.';

  @override
  String get dbRouteTunnelHint => 'Saved SSH tunnel, started on connect';

  @override
  String get dbRouteDockerHint => 'Docker port forward, restored on connect';

  @override
  String get dockerComposeActions => 'Project actions';

  @override
  String get dockerComposeStartAll => 'Start all';

  @override
  String get dockerComposeStopAll => 'Stop all';

  @override
  String get dockerComposeRestartAll => 'Restart all';

  @override
  String get dockerComposeViewYaml => 'View YAML';

  @override
  String get dockerComposeEditYaml => 'Edit YAML';

  @override
  String dockerComposeStopAllTitle(String project) {
    return 'Stop all containers of $project?';
  }

  @override
  String dockerComposeRestartAllTitle(String project) {
    return 'Restart all containers of $project?';
  }

  @override
  String dockerComposeBulkHelp(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count containers are affected.',
      one: '1 container is affected.',
    );
    return '$_temp0';
  }

  @override
  String dockerComposeBulkFailed(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'The action failed for $count containers.',
      one: 'The action failed for 1 container.',
    );
    return '$_temp0';
  }

  @override
  String dockerComposeFilesTitle(String project) {
    return 'Compose files: $project';
  }

  @override
  String dockerComposeWorkingDir(String path) {
    return 'Project folder: $path';
  }

  @override
  String dockerComposeEnvFiles(String files) {
    return 'Environment files: $files';
  }

  @override
  String get dockerComposeFiles => 'Files';

  @override
  String dockerComposeUnlockHelp(String project) {
    return 'Compose files can contain passwords and decide what runs on the host. Enter the vault passphrase to edit the files of $project.';
  }

  @override
  String get dockerComposeUnlockExpired =>
      'The editing permission has expired. Enter the vault passphrase again to save.';

  @override
  String get dockerComposeUnlock => 'Allow editing';

  @override
  String get dockerComposeSaveApply => 'Save and apply';

  @override
  String get dockerComposeSaveOnly => 'Save without applying';

  @override
  String get dockerComposeValidating => 'Checking the configuration on the host…';

  @override
  String get dockerComposeInvalid => 'The configuration has errors. Nothing was saved.';

  @override
  String dockerComposeConflict(String path) {
    return '$path changed on the host after it was opened. Nothing was saved: copy your changes, then open the file again.';
  }

  @override
  String dockerComposeWriteDenied(String path) {
    return 'No permission to write $path or its folder. The file was not changed.';
  }

  @override
  String dockerComposeWriteFailed(String path) {
    return 'Could not write $path. The file was not changed.';
  }

  @override
  String get dockerComposeMissing =>
      'The host has no compose command line tool, so the change cannot be checked. Nothing was saved.';

  @override
  String dockerComposeRunFailed(String detail) {
    return 'The check could not run ($detail). Nothing was saved.';
  }

  @override
  String dockerComposeSavedBackups(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'Saved. The previous versions are kept as:',
      one: 'Saved. The previous version is kept as:',
    );
    return '$_temp0';
  }

  @override
  String get dockerComposeSavedNotApplied => 'The running containers still use the previous configuration.';

  @override
  String get dockerComposeApplying => 'Applying the changes…';

  @override
  String get dockerComposeApplied => 'Changes applied.';

  @override
  String dockerComposeApplyFailed(int code) {
    return 'Applying failed (exit code $code).';
  }

  @override
  String get dockerComposeApplyError => 'Applying failed.';

  @override
  String get dockerComposeOutput => 'Output';

  @override
  String get dockerComposeDiscardTitle => 'Discard the changes?';

  @override
  String get dockerComposeDiscardHelp => 'The edited files have not been saved.';

  @override
  String get dockerComposeDiscard => 'Discard';

  @override
  String get dockerComposeFileNotFound => 'This file no longer exists on the host.';

  @override
  String get dockerComposeFileDenied => 'No permission to read this file: it may belong to another user, such as root.';

  @override
  String get dockerComposeFileTooLarge => 'The file is larger than 1 MB and is not shown.';

  @override
  String get dockerComposeFileNotText => 'The file is not UTF-8 text.';

  @override
  String get dockerComposeFileReadFailed => 'The file could not be read.';

  @override
  String get dockerComposeProjectGone => 'The project has no containers any more.';

  @override
  String get dockerComposeFilesUnknown => 'The project\'s containers do not record which compose files created them.';

  @override
  String get sharingDbConnection => 'Database connection';

  @override
  String get sharingDbUnsupported => 'This server does not support sharing database connections.';

  @override
  String get sharingDbSnapshotChanged => 'Review this connection again if its settings or credentials have changed.';

  @override
  String get sharingDbExclude =>
      'Only the connection settings are shared by default. The password, client key, group, query history and AI settings remain on this device.';

  @override
  String get sharingDbIncludeCredentials => 'Include the saved password and client key';

  @override
  String get sharingDbCredentialWarning =>
      'Recipients can use these credentials to access the database. Share them only with verified devices you trust. Revoking the share does not revoke credentials already copied by a recipient: change the password on the database server.';

  @override
  String get sharingDbCredentialsIncluded => 'The password or client key is included; values stay hidden.';

  @override
  String get sharingDbCredentialsExcluded => 'The password and client key are not included.';

  @override
  String get sharingDbRouteRelay =>
      'This connection goes through an SSH host, jump profile, proxy or tunnel that is not shared. Recipients select their own route.';

  @override
  String get sharingDbRouteKubernetes =>
      'This connection is forwarded from a Kubernetes workload. Recipients select their own Kubernetes context; the namespace and workload are kept.';

  @override
  String get sharingDbRouteDocker =>
      'This connection is forwarded from a Docker container. Recipients select their own Docker engine; the container name is kept.';

  @override
  String get sharingDbCopyHelp =>
      'Save a personal copy of this connection to Databases. Choose your own route. The password and client key are imported only with your explicit selection.';

  @override
  String get sharingDbRouteRequired =>
      'This connection is not reachable directly. Select a compatible route saved on this device.';

  @override
  String get sharingDbNoRoute => 'No compatible route is saved on this device. Add one first.';

  @override
  String get sharingDbImportCredentials => 'Import the included password and client key';

  @override
  String get sharingDbRouteKind => 'Route';

  @override
  String get sharingDbRouteKindRelay => 'SSH, jump profile, proxy or tunnel';

  @override
  String get sharingDbWorkload => 'Workload';

  @override
  String get sharingDbContainer => 'Container';

  @override
  String get dbSharedTitle => 'Shared connections';

  @override
  String get dbSharedBadge => 'Shared';

  @override
  String get dbSharedOwned => 'Shared by you';

  @override
  String get dbSharedOpen => 'Details';

  @override
  String dbSharedCount(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count shared connections',
      one: '$count shared connection',
    );
    return '$_temp0';
  }

  @override
  String shellNewDeviceTitle(String device, String platform) {
    return 'New device signed in: $device, $platform';
  }

  @override
  String shellNewDeviceMessage(String ago) {
    return 'Signed in $ago. If this wasn\'t you, revoke it and change your account password. For its first days it cannot revoke your trusted devices.';
  }

  @override
  String get shellNewDeviceItsMe => 'It\'s me';

  @override
  String get shellNewDeviceRevoke => 'Revoke…';

  @override
  String shellNewDeviceRevoked(String device) {
    return '\"$device\" was revoked. Change your account password: whoever signed in knows it.';
  }

  @override
  String get devicesStatusUnconfirmed => 'Not confirmed';

  @override
  String devicesQuarantineBadge(String date) {
    return 'New, restricted until $date';
  }

  @override
  String get errorDeviceInQuarantine =>
      'This device signed in recently and cannot revoke or sign out trusted devices yet. Do it on a trusted device.';

  @override
  String errorDeviceInQuarantineUntil(String date) {
    return 'This device signed in recently and cannot revoke or sign out trusted devices until $date. Do it on a trusted device.';
  }

  @override
  String get errorEnvelopeRateLimited => 'Too many attempts to open the vault from new devices. Try again later.';

  @override
  String errorEnvelopeRateLimitedUntil(String time) {
    return 'Too many attempts to open the vault from new devices. Try again after $time.';
  }

  @override
  String get settingsSignOutEverywhere => 'Sign out on all devices…';

  @override
  String get settingsSignOutEverywhereTitle => 'Sign out on all devices?';

  @override
  String get settingsSignOutEverywhereMessage =>
      'Every session of this account ends, including this one. Devices sign in again with the account password; their local data stays.';

  @override
  String get settingsSignOutEverywhereConfirm => 'Sign out everywhere';

  @override
  String get settingsSignOutEverywhereDone => 'Signed out on all devices.';

  @override
  String settingsSignOutEverywhereKept(int count, String date) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other:
          'This device is new, so your $count trusted devices stayed signed in (until $date). Sessions of new devices ended.',
      one: 'This device is new, so your trusted device stayed signed in (until $date). Sessions of new devices ended.',
    );
    return '$_temp0';
  }

  @override
  String get settingsDialogAccountPasswordChangedNewDevice =>
      'Account password changed. This device is new, so your trusted devices stay signed in.';

  @override
  String get dockerShowFull => 'Show in full';

  @override
  String get dockerSecretsMasked => 'Passwords, tokens and keys are hidden.';

  @override
  String dockerJsonRevealHelp(String name) {
    return 'Enter the vault passphrase to show the full inspect JSON of $name, passwords and tokens included. Copying it then clears the clipboard automatically.';
  }

  @override
  String dockerComposeRevealHelp(String project) {
    return 'Compose files can contain passwords. Enter the vault passphrase to show the files of $project in full.';
  }

  @override
  String dockerComposeTampered(String path) {
    return 'A temporary file for $path was replaced or changed on the host by someone else during the save. Nothing was written to $path.';
  }

  @override
  String dockerComposeTamperedRestored(String path, String backup) {
    return '$path was replaced on the host by someone else during the save. The original was restored from the backup $backup.';
  }

  @override
  String dockerComposeTamperedNotRestored(String path, String backup) {
    return '$path was replaced on the host by someone else during the save, and the original could not be put back. Restore it from the backup $backup.';
  }

  @override
  String get dockerComposeUnsafeDirTitle => 'Save in a shared directory?';

  @override
  String dockerComposeUnsafeDirHelp(String dirs) {
    return 'The project directory can be written by other users: $dirs. Saving may be unsafe: they could swap the temporary file or the backup while it is written.';
  }

  @override
  String get dockerComposeSaveAnyway => 'Save anyway';

  @override
  String get dockerTerminalHint =>
      'The terminal gives full access to the container, including its environment variables.';

  @override
  String get updateWindowsInstallHelp =>
      'ConsoleCrypt closes, the installer updates it and starts it again. SSH sessions are closed; profiles and vaults are kept.';

  @override
  String get updatePortableHelp =>
      'This copy of ConsoleCrypt runs from an unpacked ZIP archive and cannot update itself. Open the download page and get the installer or the new ZIP archive. Profiles and vaults are kept.';

  @override
  String get updateOpenDownloadPage => 'Open download page';

  @override
  String get updateErrorNetwork =>
      'The update could not be downloaded: the server is unreachable or the connection was interrupted. Check the connection and retry.';

  @override
  String get updateErrorIntegrity =>
      'The downloaded file does not match the signed size or checksum and was not installed. Retry the download.';

  @override
  String get updateErrorFeed =>
      'The update information could not be verified (signature, format or validity period). Nothing was installed.';

  @override
  String get updateErrorRollback =>
      'The server returned older update information than this device has already seen. It may have been replaced; nothing was installed.';

  @override
  String updateErrorLaunch(String code) {
    return 'The installer could not be started (Windows error $code). Antivirus software may have blocked it. Download the installer from the download page.';
  }

  @override
  String get updateErrorStorage =>
      'The update could not be saved to the temporary folder. Free up disk space and retry.';

  @override
  String updateErrorBrowser(String url) {
    return 'The browser could not be opened. Download page: $url';
  }

  @override
  String get kubeGitOpsReconcileTooltip =>
      'Flux immediately compares the selected resources with Git and applies the changes (flux reconcile)';

  @override
  String get kubeGitOpsReconcileWithSourceTooltip =>
      'Flux first fetches the latest source (Git repository or chart), then syncs the selected resource (flux reconcile --with-source)';

  @override
  String get kubeGitOpsSuspendTooltip =>
      'Flux stops applying changes from Git to the selected resources until sync is resumed (flux suspend)';

  @override
  String get kubeGitOpsResumeTooltip =>
      'Flux applies changes from Git to the selected resources again and syncs them right away (flux resume)';

  @override
  String get kubeGitOpsResetTooltip =>
      'Clears the failed install and upgrade count of the HelmRelease so Flux tries again (flux reconcile helmrelease --reset)';

  @override
  String get kubeGitOpsForceTooltip =>
      'Flux performs a one-off Helm install or upgrade even if nothing changed (flux reconcile helmrelease --force)';

  @override
  String get copyWhatDdl => 'DDL';

  @override
  String get copyWhatValue => 'value';

  @override
  String get dbOpenInEditor => 'Open in editor';

  @override
  String get dbUnlockProduction => 'Unlock production';

  @override
  String get dbUnlockProductionTitle => 'Unlock production for changes';

  @override
  String dbUnlockProductionWarning(String name) {
    return 'Changes will go straight to the production database “$name”. Every changing statement is still shown for review and needs the vault passphrase. Writes lock again when the time runs out, when you lock them, disconnect or lock the vault.';
  }

  @override
  String get dbUnlockFor => 'Allow writes for';

  @override
  String dbMinutes(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '$count minutes', one: '$count minute');
    return '$_temp0';
  }

  @override
  String dbProdWriteUntil(String time) {
    return 'Production writes until $time';
  }

  @override
  String dbProdWriteLeft(String time) {
    return '$time left';
  }

  @override
  String get dbLockProduction => 'Lock';

  @override
  String get dbProdWriteLocked => 'Production writes are locked again.';

  @override
  String get dbProdRoleReadOnly =>
      'This connection\'s database role cannot change data, so the server refuses writes even while production is unlocked.';

  @override
  String get dbHistoryProdWrite => 'Production change';

  @override
  String get dbSetNull => 'Set to NULL';

  @override
  String get dbCellEditHint => 'Enter saves, Esc cancels, Tab moves to the next cell';

  @override
  String get dbPendingEdit => 'Changed, not applied yet';

  @override
  String dbSelectedRows(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'Selected: $count rows',
      one: 'Selected: $count row',
    );
    return '$_temp0';
  }

  @override
  String get vaultPassphraseConfirmLabel => 'Vault passphrase';

  @override
  String get vaultPassphraseConfirmWrong => 'Wrong vault passphrase.';

  @override
  String dbPreviewTabTitle(String name) {
    return '$name — data';
  }

  @override
  String dbDetailsSection(String id) {
    String _temp0 = intl.Intl.selectLogic(id, {
      'general': 'General',
      'size': 'Size',
      'structure': 'Structure',
      'partitioning': 'Partitioning',
      'maintenance': 'Maintenance',
      'replication': 'Replication',
      'storage': 'Storage',
      'other': '$id',
    });
    return '$_temp0';
  }

  @override
  String dbDetailsField(String id) {
    String _temp0 = intl.Intl.selectLogic(id, {
      'kind': 'Object type',
      'persistence': 'Storage',
      'schema': 'Schema',
      'owner': 'Owner',
      'tablespace': 'Tablespace',
      'comment': 'Comment',
      'rows': 'Rows',
      'live_rows': 'Live rows',
      'dead_rows': 'Dead rows',
      'total_size': 'Total size',
      'table_size': 'Table data',
      'index_size': 'Indexes',
      'toast_size': 'TOAST',
      'columns': 'Columns',
      'indexes': 'Indexes',
      'constraints': 'Constraints',
      'triggers': 'Triggers',
      'definition_length': 'Definition length, characters',
      'partition_key': 'Partition key',
      'partitions': 'Partitions',
      'partition_of': 'Partition of',
      'last_vacuum': 'Last VACUUM',
      'last_autovacuum': 'Last autovacuum',
      'last_analyze': 'Last ANALYZE',
      'last_autoanalyze': 'Last autoanalyze',
      'engine': 'Engine',
      'row_format': 'Row format',
      'collation': 'Collation',
      'definer': 'Definer',
      'updatable': 'Updatable',
      'security': 'Security',
      'average_row': 'Average row',
      'free_space': 'Free space',
      'auto_increment': 'Next AUTO_INCREMENT',
      'created': 'Created',
      'updated': 'Updated',
      'uncompressed_size': 'Uncompressed',
      'parts': 'Active parts',
      'sorting_key': 'Sorting key',
      'primary_key': 'Primary key',
      'sampling_key': 'Sampling key',
      'ttl': 'TTL',
      'metadata_modified': 'Metadata changed',
      'type': 'Type',
      'encoding': 'Encoding',
      'database': 'Database',
      'memory': 'Memory',
      'keys': 'Keys',
      'length_bytes': 'Length, bytes',
      'length_fields': 'Fields',
      'length_elements': 'Elements',
      'length_members': 'Members',
      'length_entries': 'Entries',
      'page_size': 'Page size',
      'page_count': 'Pages',
      'freelist_count': 'Free pages',
      'foreign_keys': 'Foreign keys',
      'strict': 'STRICT',
      'without_rowid': 'WITHOUT ROWID',
      'autoincrement': 'AUTOINCREMENT',
      'version': 'Version',
      'table': 'Table',
      'keyspace': 'Keyspace',
      'base_table': 'Base table',
      'id': 'ID',
      'cql_partition_key': 'Partition key',
      'clustering_key': 'Clustering key',
      'compaction': 'Compaction',
      'compression': 'Compression',
      'gc_grace_seconds': 'GC grace, seconds',
      'default_ttl': 'Default TTL, seconds',
      'caching': 'Caching',
      'bloom_filter': 'Bloom filter false positives',
      'speculative_retry': 'Speculative retry',
      'cql_partitions': 'Partitions',
      'mean_partition_size': 'Mean partition size',
      'strategy': 'Replication strategy',
      'replication': 'Replicas',
      'durable_writes': 'Durable writes',
      'tables': 'Tables',
      'views': 'Materialized views',
      'types': 'User types',
      'functions': 'Functions',
      'other': '$id',
    });
    return '$_temp0';
  }

  @override
  String dbDetailsKeyword(String id) {
    String _temp0 = intl.Intl.selectLogic(id, {
      'table': 'Table',
      'partitioned_table': 'Partitioned table',
      'view': 'View',
      'materialized_view': 'Materialized view',
      'foreign_table': 'Foreign table',
      'unlogged': 'Unlogged',
      'temporary': 'Temporary',
      'index': 'Index',
      'yes': 'Yes',
      'no': 'No',
      'keyspace': 'Keyspace',
      'other': '$id',
    });
    return '$_temp0';
  }

  @override
  String get dbDetailsNoExpiry => 'No expiry';

  @override
  String get dbDetailsJson => 'JSON';

  @override
  String get dbDetailsEmpty => 'Nothing to show for this object.';

  @override
  String get copyWhatObjectDetails => 'Object details';

  @override
  String get dbWrapLines => 'Wrap lines';

  @override
  String settingsTerminalFontMissing(String family, String fallback) {
    return 'Font “$family” is not installed — using $fallback.';
  }

  @override
  String get settingsCaptureProtectionLabel => 'Hide window from screen capture';

  @override
  String get settingsCaptureProtectionOff => 'Never';

  @override
  String get settingsCaptureProtectionSecrets => 'While secrets are shown';

  @override
  String get settingsCaptureProtectionAlways => 'Always';

  @override
  String get settingsCaptureProtectionHelp =>
      'The window is left out of screenshots, recordings and screen sharing (Zoom, Teams). With “Always”, colleagues will not see the ConsoleCrypt window while you share your screen.';

  @override
  String get sftpActiveContentTitle => 'This file can run code';

  @override
  String sftpActiveContentBody(String name) {
    return '“$name” can run code. It will open as text in an editor.';
  }

  @override
  String get sftpActiveContentNoRun =>
      'ConsoleCrypt never runs downloaded files. To run it, use a terminal on the server.';

  @override
  String get sftpActiveContentOpenAsText => 'Open as Text';

  @override
  String get sftpNoTextEditorTitle => 'No text editor found';

  @override
  String sftpNoTextEditorBody(String name) {
    return '“$name” can run code, so it opens only as text, but no text editor was found. Choose an editor to open it with.';
  }

  @override
  String get sftpChooseEditor => 'Choose an Editor…';

  @override
  String get errorNoTextEditor => 'No text editor found to open this file safely. Choose an editor using Open With.';

  @override
  String get dbExportEscapeFormulas => 'Escape formulas (for Excel)';

  @override
  String settingsClearClipboardSeconds(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '$count seconds', one: '$count second');
    return '$_temp0 after copying a secret';
  }

  @override
  String settingsClearClipboardMinutes(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '$count minutes', one: '$count minute');
    return '$_temp0 after copying a secret';
  }

  @override
  String settingsClearClipboardHours(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '$count hours', one: '$count hour');
    return '$_temp0 after copying a secret';
  }

  @override
  String settingsClearClipboardDays(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '$count days', one: '$count day');
    return '$_temp0 after copying a secret';
  }

  @override
  String sharingUnreadableEntries(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count shared entries could not be read and are hidden.',
      one: '$count shared entry could not be read and is hidden.',
    );
    return '$_temp0';
  }

  @override
  String get snippetPackPostgresql => 'SQL · PostgreSQL';

  @override
  String get snippetPackPostgresqlDescription =>
      'Activity, locks, sizes, indexes, vacuum, replication and settings. For psql or the Databases workspace.';

  @override
  String get snippetPackMysql => 'SQL · MySQL/MariaDB';

  @override
  String get snippetPackMysqlDescription =>
      'Processlist, InnoDB locks, sizes, indexes, replication and statement digests. For the mysql client or the Databases workspace.';

  @override
  String get snippetPackClickhouse => 'SQL · ClickHouse';

  @override
  String get snippetPackClickhouseDescription =>
      'Running and slow queries, parts, merges, mutations, replication and disks. For clickhouse-client or the Databases workspace.';

  @override
  String get snippetPackRedis => 'Redis';

  @override
  String get snippetPackRedisDescription =>
      'Memory, clients, slow log, keyspace and replication. For redis-cli or the Databases workspace.';

  @override
  String get snippetPackMongodb => 'MongoDB';

  @override
  String get snippetPackMongodbDescription =>
      'Operations, server and collection stats, indexes, replica set and profiler. For mongosh; some also run in the Databases workspace.';

  @override
  String get snippetDbPgActivity => 'Active queries';

  @override
  String get snippetDbPgActivityNote => 'Sessions that are not idle, longest first.';

  @override
  String get snippetDbPgLongRunning => 'Long-running queries';

  @override
  String get snippetDbPgLongRunningNote => 'Queries running longer than the given number of seconds.';

  @override
  String get snippetDbPgBlocking => 'Lock tree';

  @override
  String get snippetDbPgBlockingNote =>
      'Blocking sessions with the sessions waiting on them, indented by depth (PG 9.6+).';

  @override
  String get snippetDbPgTableSizes => 'Largest tables';

  @override
  String get snippetDbPgTableSizesNote => 'Total size with heap, indexes and TOAST; row counts are planner estimates.';

  @override
  String get snippetDbPgDatabaseSizes => 'Database sizes';

  @override
  String get snippetDbPgDatabaseSizesNote => 'Size of every database that accepts connections.';

  @override
  String get snippetDbPgIndexUsage => 'Index usage';

  @override
  String get snippetDbPgIndexUsageNote =>
      'Least-used indexes first; idx_scan = 0 means unused since the last statistics reset. Check replicas too, and keep unique indexes.';

  @override
  String get snippetDbPgDeadTuples => 'Dead tuples and autovacuum';

  @override
  String get snippetDbPgDeadTuplesNote => 'Bloat estimate from dead tuples with the last vacuum and analyze times.';

  @override
  String get snippetDbPgCacheHit => 'Cache hit ratio';

  @override
  String get snippetDbPgCacheHitNote =>
      'Share of blocks read from shared buffers per database; below 99% on OLTP is worth a look.';

  @override
  String get snippetDbPgReplication => 'Replication status';

  @override
  String get snippetDbPgReplicationNote =>
      'Connected replicas with lag in bytes and time. Run on the primary (PG 10+).';

  @override
  String get snippetDbPgSlots => 'Replication slots';

  @override
  String get snippetDbPgSlotsNote => 'Slots and the WAL they retain; an inactive slot keeps WAL forever (PG 13+).';

  @override
  String get snippetDbPgConnections => 'Connections by user and application';

  @override
  String get snippetDbPgConnectionsNote => 'Client connections grouped by database, user, application and state.';

  @override
  String get snippetDbPgSettingsChanged => 'Non-default settings';

  @override
  String get snippetDbPgSettingsChangedNote => 'Settings whose source is not the built-in default.';

  @override
  String get snippetDbPgColumns => 'Table columns';

  @override
  String get snippetDbPgColumnsNote => 'Columns with types, defaults and comments.';

  @override
  String get snippetDbPgForeignKeys => 'Foreign keys of a table';

  @override
  String get snippetDbPgForeignKeysNote => 'Foreign keys of the table and those that reference it.';

  @override
  String get snippetDbPgSequences => 'Sequences near their limit';

  @override
  String get snippetDbPgSequencesNote =>
      'Sequences that used more than the given percent of their range; also check the column type (PG 10+).';

  @override
  String get snippetDbPgFkWithoutIndex => 'Foreign keys without an index';

  @override
  String get snippetDbPgFkWithoutIndexNote =>
      'Foreign keys whose columns do not lead any index; deletes and updates on the parent scan the child table.';

  @override
  String get snippetDbPgStatements => 'Top queries (pg_stat_statements)';

  @override
  String get snippetDbPgStatementsNote =>
      'Needs the pg_stat_statements extension (shared_preload_libraries and CREATE EXTENSION); otherwise it fails with \"relation does not exist\". Column names are for PG 13+.';

  @override
  String get snippetDbPgExplainAnalyze => 'EXPLAIN ANALYZE';

  @override
  String get snippetDbPgExplainAnalyzeNote =>
      'Runs the query to measure the real plan; put data-changing statements inside BEGIN … ROLLBACK.';

  @override
  String get snippetDbPgCancel => 'Cancel a query';

  @override
  String get snippetDbPgCancelNote => 'Cancels the current query of the backend; the session stays connected.';

  @override
  String get snippetDbPgTerminate => 'Terminate a session';

  @override
  String get snippetDbPgTerminateNote => 'Disconnects the backend and rolls back its open transaction.';

  @override
  String get snippetDbMysqlProcesslist => 'Active queries';

  @override
  String get snippetDbMysqlProcesslistNote => 'Connections that are not sleeping, longest first.';

  @override
  String get snippetDbMysqlLongRunning => 'Long-running queries';

  @override
  String get snippetDbMysqlLongRunningNote => 'Statements running longer than the given number of seconds.';

  @override
  String get snippetDbMysqlLockWaits => 'InnoDB lock waits';

  @override
  String get snippetDbMysqlLockWaitsNote =>
      'Who waits for whom, with the blocking statement. Needs the sys schema (MySQL 5.7+, MariaDB 10.6+).';

  @override
  String get snippetDbMysqlTransactions => 'Open transactions';

  @override
  String get snippetDbMysqlTransactionsNote => 'InnoDB transactions, oldest first, with locked and modified rows.';

  @override
  String get snippetDbMysqlTableSizes => 'Largest tables';

  @override
  String get snippetDbMysqlTableSizesNote =>
      'Data, index and free space in MB; information_schema statistics are approximate and cached.';

  @override
  String get snippetDbMysqlDatabaseSizes => 'Database sizes';

  @override
  String get snippetDbMysqlDatabaseSizesNote => 'Total data and index size per schema in MB.';

  @override
  String get snippetDbMysqlIndexUsage => 'Index usage';

  @override
  String get snippetDbMysqlIndexUsageNote => 'Reads and writes per index since server start (performance_schema).';

  @override
  String get snippetDbMysqlUnusedIndexes => 'Unused indexes';

  @override
  String get snippetDbMysqlUnusedIndexesNote =>
      'Indexes without reads since server start. Needs the sys schema; check replicas before dropping.';

  @override
  String get snippetDbMysqlAutoIncrement => 'AUTO_INCREMENT near the limit';

  @override
  String get snippetDbMysqlAutoIncrementNote => 'Share of the column type range already used. Needs the sys schema.';

  @override
  String get snippetDbMysqlIndexes => 'Indexes of a table';

  @override
  String get snippetDbMysqlIndexesNote => 'Index columns in order with cardinality.';

  @override
  String get snippetDbMysqlColumns => 'Table columns';

  @override
  String get snippetDbMysqlColumnsNote => 'Columns with types, keys, defaults and comments.';

  @override
  String get snippetDbMysqlForeignKeys => 'Foreign keys of a table';

  @override
  String get snippetDbMysqlForeignKeysNote => 'Foreign keys of the table and those that reference it.';

  @override
  String get snippetDbMysqlReplicaStatus => 'Replica status';

  @override
  String get snippetDbMysqlReplicaStatusNote =>
      'MySQL 8.0.22+ and MariaDB 10.5.1+; on older servers use SHOW SLAVE STATUS.';

  @override
  String get snippetDbMysqlVariables => 'Server variables';

  @override
  String get snippetDbMysqlVariablesNote => 'Global variables matching a LIKE pattern.';

  @override
  String get snippetDbMysqlStatus => 'Key status counters';

  @override
  String get snippetDbMysqlStatusNote => 'Uptime, connections, slow queries, row lock waits and buffer pool reads.';

  @override
  String get snippetDbMysqlConnections => 'Connections by user and host';

  @override
  String get snippetDbMysqlConnectionsNote => 'Connections grouped by user, client host, schema and command.';

  @override
  String get snippetDbMysqlDigest => 'Top queries (statement digests)';

  @override
  String get snippetDbMysqlDigestNote =>
      'Normalized statements by total time from performance_schema (enabled by default in MySQL; off by default in MariaDB).';

  @override
  String get snippetDbMysqlInnodbStatus => 'InnoDB status';

  @override
  String get snippetDbMysqlInnodbStatusNote =>
      'Engine report with the latest deadlock, semaphores and buffer pool; needs the PROCESS privilege.';

  @override
  String get snippetDbMysqlExplainAnalyze => 'EXPLAIN ANALYZE';

  @override
  String get snippetDbMysqlExplainAnalyzeNote =>
      'Runs the query to measure the real plan (MySQL 8.0.18+; in MariaDB use ANALYZE SELECT …).';

  @override
  String get snippetDbMysqlKillQuery => 'Cancel a query';

  @override
  String get snippetDbMysqlKillQueryNote => 'Stops the current statement of the connection; the connection stays open.';

  @override
  String get snippetDbClickhouseProcesses => 'Running queries';

  @override
  String get snippetDbClickhouseProcessesNote => 'Initial queries with elapsed time, memory and rows read.';

  @override
  String get snippetDbClickhouseLongRunning => 'Long-running queries';

  @override
  String get snippetDbClickhouseLongRunningNote => 'Queries running longer than the given number of seconds.';

  @override
  String get snippetDbClickhouseSlowQueries => 'Slow queries';

  @override
  String get snippetDbClickhouseSlowQueriesNote => 'Slowest finished queries from system.query_log for the last hours.';

  @override
  String get snippetDbClickhouseFailedQueries => 'Failed queries';

  @override
  String get snippetDbClickhouseFailedQueriesNote => 'Recent queries that ended with an exception.';

  @override
  String get snippetDbClickhouseQueryPatterns => 'Heaviest query patterns';

  @override
  String get snippetDbClickhouseQueryPatternsNote => 'Finished queries grouped by normalized text, by total duration.';

  @override
  String get snippetDbClickhouseTableSizes => 'Largest tables';

  @override
  String get snippetDbClickhouseTableSizesNote =>
      'Compressed and uncompressed size of active parts with the compression ratio.';

  @override
  String get snippetDbClickhouseDatabaseSizes => 'Database sizes';

  @override
  String get snippetDbClickhouseDatabaseSizesNote => 'Disk usage of active parts per database.';

  @override
  String get snippetDbClickhousePartitions => 'Partitions of a table';

  @override
  String get snippetDbClickhousePartitionsNote => 'Parts, rows and size per partition.';

  @override
  String get snippetDbClickhousePartsCount => 'Partitions with many parts';

  @override
  String get snippetDbClickhousePartsCountNote =>
      'Too many active parts slow down inserts and selects (“Too many parts”).';

  @override
  String get snippetDbClickhouseColumnSizes => 'Column sizes of a table';

  @override
  String get snippetDbClickhouseColumnSizesNote => 'Compressed size, ratio and codec per column.';

  @override
  String get snippetDbClickhouseMerges => 'Running merges';

  @override
  String get snippetDbClickhouseMergesNote => 'Merges and mutations in progress.';

  @override
  String get snippetDbClickhouseMutations => 'Unfinished mutations';

  @override
  String get snippetDbClickhouseMutationsNote => 'ALTER … UPDATE/DELETE still running, with the latest failure reason.';

  @override
  String get snippetDbClickhouseReplicationQueue => 'Replication queue';

  @override
  String get snippetDbClickhouseReplicationQueueNote =>
      'Oldest replication tasks with retries and the last error (Replicated* tables).';

  @override
  String get snippetDbClickhouseReplicas => 'Replica status';

  @override
  String get snippetDbClickhouseReplicasNote => 'Delay, queue and read-only state of Replicated* tables.';

  @override
  String get snippetDbClickhouseDisks => 'Disks';

  @override
  String get snippetDbClickhouseDisksNote => 'Free and total space of the configured disks.';

  @override
  String get snippetDbClickhouseErrors => 'Server errors';

  @override
  String get snippetDbClickhouseErrorsNote => 'Error counters since start with the latest message.';

  @override
  String get snippetDbClickhouseSettingsChanged => 'Changed settings';

  @override
  String get snippetDbClickhouseSettingsChangedNote => 'Session settings that differ from the defaults.';

  @override
  String get snippetDbClickhouseMetrics => 'Current metrics';

  @override
  String get snippetDbClickhouseMetricsNote => 'Non-zero gauges: queries, merges, connections, memory.';

  @override
  String get snippetDbClickhouseExplain => 'EXPLAIN with indexes';

  @override
  String get snippetDbClickhouseExplainNote =>
      'Plan with the primary key and skip indexes used; the query is not executed.';

  @override
  String get snippetDbClickhouseKillQuery => 'Cancel a query';

  @override
  String get snippetDbClickhouseKillQueryNote => 'Stops the query with the given query_id.';

  @override
  String get snippetDbRedisInfo => 'Server info';

  @override
  String get snippetDbRedisInfoNote =>
      'One INFO section: server, clients, memory, persistence, stats, replication, cpu, keyspace.';

  @override
  String get snippetDbRedisMemory => 'Memory';

  @override
  String get snippetDbRedisMemoryNote => 'Used and peak memory, fragmentation and the maxmemory policy.';

  @override
  String get snippetDbRedisMemoryDoctor => 'Memory doctor';

  @override
  String get snippetDbRedisMemoryDoctorNote => 'Redis advice on memory problems.';

  @override
  String get snippetDbRedisSlowlog => 'Slow log';

  @override
  String get snippetDbRedisSlowlogNote => 'Latest slow commands with duration in microseconds.';

  @override
  String get snippetDbRedisClients => 'Client list';

  @override
  String get snippetDbRedisClientsNote => 'Connected clients with address, age, idle time and last command.';

  @override
  String get snippetDbRedisDbsize => 'Number of keys';

  @override
  String get snippetDbRedisDbsizeNote => 'Keys in the selected database.';

  @override
  String get snippetDbRedisKeyspace => 'Keyspace';

  @override
  String get snippetDbRedisKeyspaceNote => 'Keys and expiring keys per database.';

  @override
  String get snippetDbRedisScan => 'Find keys by pattern';

  @override
  String get snippetDbRedisScanNote =>
      'Incremental SCAN instead of KEYS; repeat with the returned cursor until it is 0.';

  @override
  String get snippetDbRedisType => 'Key type';

  @override
  String get snippetDbRedisTypeNote => 'Data type stored at the key.';

  @override
  String get snippetDbRedisTtl => 'Key TTL';

  @override
  String get snippetDbRedisTtlNote => 'Seconds to expiry; -1 means no expiry, -2 means the key does not exist.';

  @override
  String get snippetDbRedisMemoryUsage => 'Key memory usage';

  @override
  String get snippetDbRedisMemoryUsageNote => 'Bytes used by the key and its value.';

  @override
  String get snippetDbRedisLatency => 'Latency doctor';

  @override
  String get snippetDbRedisLatencyNote => 'Latency report; needs latency-monitor-threshold > 0.';

  @override
  String get snippetDbRedisConfig => 'Configuration value';

  @override
  String get snippetDbRedisConfigNote => 'Configuration parameters matching a glob pattern.';

  @override
  String get snippetDbRedisReplication => 'Replication';

  @override
  String get snippetDbRedisReplicationNote => 'Role, connected replicas and replication offsets.';

  @override
  String get snippetDbRedisCommandstats => 'Command statistics';

  @override
  String get snippetDbRedisCommandstatsNote => 'Calls and time per command since the last reset.';

  @override
  String get snippetDbMongodbCurrentOps => 'Active operations';

  @override
  String get snippetDbMongodbCurrentOpsNote => 'Operations running now (mongosh).';

  @override
  String get snippetDbMongodbLongOps => 'Long-running operations';

  @override
  String get snippetDbMongodbLongOpsNote => 'Operations running longer than the given number of seconds (mongosh).';

  @override
  String get snippetDbMongodbConnections => 'Connections';

  @override
  String get snippetDbMongodbConnectionsNote => 'Current, available and total created connections (mongosh).';

  @override
  String get snippetDbMongodbOpcounters => 'Operation counters';

  @override
  String get snippetDbMongodbOpcountersNote => 'Inserts, queries, updates, deletes and commands since start (mongosh).';

  @override
  String get snippetDbMongodbMemory => 'Memory';

  @override
  String get snippetDbMongodbMemoryNote => 'Resident and virtual memory in MB (mongosh).';

  @override
  String get snippetDbMongodbDbStats => 'Database stats';

  @override
  String get snippetDbMongodbDbStatsNote => 'Collections, objects, data and index size in MB.';

  @override
  String get snippetDbMongodbCollectionStats => 'Collection stats';

  @override
  String get snippetDbMongodbCollectionStatsNote => 'Documents, storage and per-index size in MB.';

  @override
  String get snippetDbMongodbIndexes => 'Indexes of a collection';

  @override
  String get snippetDbMongodbIndexesNote => 'Index keys and options.';

  @override
  String get snippetDbMongodbIndexStats => 'Index usage';

  @override
  String get snippetDbMongodbIndexStatsNote => 'Operations per index since the last restart (mongosh).';

  @override
  String get snippetDbMongodbExplain => 'Query plan with statistics';

  @override
  String get snippetDbMongodbExplainNote =>
      'Runs the find and reports the plan, keys and documents examined (mongosh).';

  @override
  String get snippetDbMongodbRsStatus => 'Replica set status';

  @override
  String get snippetDbMongodbRsStatusNote => 'Members, their state and health (mongosh).';

  @override
  String get snippetDbMongodbRsLag => 'Replication lag';

  @override
  String get snippetDbMongodbRsLagNote => 'How far each secondary is behind the primary (mongosh, MongoDB 4.4+).';

  @override
  String get snippetDbMongodbProfilerStatus => 'Profiler level';

  @override
  String get snippetDbMongodbProfilerStatusNote =>
      'Profiler level and slowms threshold of the current database (mongosh).';

  @override
  String get snippetDbMongodbSlowOps => 'Slow operations (profiler)';

  @override
  String get snippetDbMongodbSlowOpsNote =>
      'Latest profiled operations slower than the threshold; enable with db.setProfilingLevel(1).';

  @override
  String get snippetDbMongodbKillOp => 'Kill an operation';

  @override
  String get snippetDbMongodbKillOpNote => 'Stops the operation with the given opid from currentOp (mongosh).';

  @override
  String snippetStarterDbOrigin(String note, String client, String package) {
    return '$note For $client or the Databases workspace. From the $package starter package.';
  }

  @override
  String snippetStarterDbTerminalOrigin(String note, String client, String package) {
    return '$note Runs in $client in a terminal; the Databases workspace does not allow it. From the $package starter package.';
  }

  @override
  String get snippetClientSessionRequired =>
      'Start the database client (psql, mysql, clickhouse-client, redis-cli, mongosh) in a connected terminal tab first, then run the snippet there.';

  @override
  String get dbSnippetPickerTitle => 'Insert a snippet';

  @override
  String dbSnippetPickerSearch(String engine) {
    return 'Search $engine snippets';
  }

  @override
  String dbSnippetPickerEmpty(String engine) {
    return 'No $engine snippets yet. Add them in Snippets → Starter packages or save a query from the history.';
  }

  @override
  String get sharingLeave => 'Leave share';

  @override
  String get sharingLeaveHelp =>
      'You stop receiving this item on all your devices and it disappears from this list. Personal copies you saved stay. The owner sees that you left.';

  @override
  String get sharingLeft => 'You left the share';

  @override
  String get sharingStopRestoreHelp =>
      'Recipients lose access immediately. For 30 days you can restore the item from Recently deleted.';

  @override
  String sharingMembersLeft(int count) {
    return 'Left the share: $count';
  }

  @override
  String get sharingRecentlyDeleted => 'Recently deleted';

  @override
  String get sharingRecentlyDeletedHelp =>
      'Shared items you deleted can be restored for 30 days. A restored item is shared again with the same people except those who left; they accept it again.';

  @override
  String get sharingRecentlyDeletedEmpty => 'No recently deleted items';

  @override
  String get sharingRestore => 'Restore';

  @override
  String get sharingRestored => 'Item restored and shared again';

  @override
  String sharingRestoreUntil(String date, int count) {
    return 'Restorable until $date · recipients: $count';
  }

  @override
  String loginSsoButton(String name) {
    return 'Sign in with $name';
  }

  @override
  String get loginSsoButtonDefault => 'Sign in with SSO';

  @override
  String get loginSsoEmailLabel => 'E-mail (optional)';

  @override
  String get loginSsoEmailHelper => 'Pre-filled on your organisation\'s sign-in page.';

  @override
  String get loginSsoWaiting => 'Finish signing in in the browser…';

  @override
  String get loginSsoWaitingHelp => 'The browser shows your organisation\'s sign-in page. Return here when it says so.';

  @override
  String get loginLdapLoginLabel => 'Login (email or DOMAIN\\user)';

  @override
  String get loginLdapPasswordLabel => 'Password';

  @override
  String get loginLdapCaption => 'Your organisation\'s account';

  @override
  String get loginLdapLoginRequired => 'Enter your login';

  @override
  String get corporateAuthNoticeTitle => 'Corporate sign-in';

  @override
  String corporateAuthNotice(String name) {
    return 'Sign-in through the corporate authorization server $name. Your organisation\'s password does not open the vault — the vault passphrase does.';
  }

  @override
  String get authProviderOidc => 'OIDC';

  @override
  String get authProviderLdap => 'Active Directory';

  @override
  String get authProviderSsoFallback => 'SSO';

  @override
  String get serverNeedsNewerApp =>
      'This server uses a sign-in method this version does not support. Update ConsoleCrypt.';

  @override
  String get signInMethodEmail => 'e-mail and password';

  @override
  String signInMethodOidc(String name) {
    return '$name (OIDC)';
  }

  @override
  String signInMethodLdap(String name) {
    return '$name (Active Directory)';
  }

  @override
  String signInLastTime(String method, String login) {
    return 'Last time you signed in with $method as $login';
  }

  @override
  String signInLastTimeNoLogin(String method) {
    return 'Last time you signed in with $method';
  }

  @override
  String get signInForget => 'Forget';

  @override
  String authModeChangedToEnterprise(String name) {
    return 'The server switched to sign-in through the corporate authorization server $name. The previous account password is no longer used.';
  }

  @override
  String get authModeChangedToStandard =>
      'The server switched from corporate sign-in to e-mail and account password. Use the password your administrator set for your account.';

  @override
  String get recentServersLabel => 'Recent servers';

  @override
  String get authBadgeEmail => 'Email';

  @override
  String authBadgeSso(String name) {
    return 'SSO: $name';
  }

  @override
  String get authBadgeAd => 'AD';

  @override
  String get settingsAuthLabel => 'Authorization';

  @override
  String settingsAuthCorporate(String name, String kind) {
    return 'Corporate server — $name ($kind)';
  }

  @override
  String get settingsAuthStandard => 'E-mail and account password';

  @override
  String get settingsAuthCorporateHelp =>
      'Your organisation\'s authorization server checks who signs in. It never gets access to the vault: the vault passphrase stays on your devices.';

  @override
  String settingsAuthSignedIn(String method, String date) {
    return 'This device signed in with $method on $date';
  }

  @override
  String settingsAuthSignedInAs(String method, String login, String date) {
    return 'This device signed in with $method as $login on $date';
  }

  @override
  String devicesAuthCorporate(String name) {
    return 'Accounts on this server sign in through the corporate authorization server $name.';
  }

  @override
  String get accountDisabledTitle => 'Access disabled by the administrator';

  @override
  String accountDisabledBody(String server) {
    return 'Your organisation disabled this account on $server. Synchronisation stopped and the profile is locked. The data on this device stays encrypted.';
  }

  @override
  String get accountDisabledSignIn => 'Sign in again';

  @override
  String get accountDisabledDelete => 'Delete profile from this device';

  @override
  String accountDisabledDeleteTitle(String name) {
    return 'Delete profile “$name”?';
  }

  @override
  String get accountDisabledDeleteMessage =>
      'The encrypted local copy is removed from this device. Nothing changes on the server.';

  @override
  String get errorAccountDisabled => 'Access was disabled by the administrator.';

  @override
  String get errorEnterpriseAuthRequired =>
      'This server only accepts your organisation\'s sign-in. Check the server again, or update the app.';

  @override
  String get errorIdentityConflict =>
      'Your organisation\'s account is already linked to another account on this server. Contact the administrator.';

  @override
  String get errorDomainNotAllowed => 'Your e-mail domain is not allowed on this server.';

  @override
  String get errorGroupRequired =>
      'Your account is not in the group allowed to use this server. Contact the administrator.';

  @override
  String get errorMfaRequired => 'Your organisation requires sign-in with a second factor. Sign in again using it.';

  @override
  String get errorIdpUnavailable => 'Your organisation\'s sign-in service is unavailable. Try again later.';

  @override
  String get errorReasonEnterpriseCallbackInvalid =>
      'The browser returned an unexpected answer. Start signing in again.';

  @override
  String get errorReasonEnterpriseSignInTimeout => 'Signing in took too long. Start again.';

  @override
  String get errorReasonEnterpriseBrowserUnavailable => 'Could not open the browser for signing in.';

  @override
  String get teamsTitle => 'Teams';

  @override
  String get teamsSubtitle => 'Resources owned by a team. Every member sees them; roles decide who may change them.';

  @override
  String get teamsEmpty => 'You are not in any team yet.';

  @override
  String get teamsCreate => 'Create team';

  @override
  String get teamsRefresh => 'Refresh';

  @override
  String get teamsUnnamed => 'Unnamed team';

  @override
  String get teamsPendingConfirmation => 'Waiting for an admin to compare the code';

  @override
  String get teamsUnavailable => 'Not available right now';

  @override
  String teamsRowMeta(String role, int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '$count members', one: '1 member');
    return '$role · $_temp0';
  }

  @override
  String get teamsWarningBadge => 'Paused';

  @override
  String get teamsRotationBadge => 'Rotate key';

  @override
  String get teamsOfflineBadge => 'Offline';

  @override
  String get teamsReadOnly => 'Read only';

  @override
  String teamsChipSemantics(String name) {
    return 'Team $name';
  }

  @override
  String teamsInvitationFrom(String email, String role) {
    return '$email invites you to a team as $role.';
  }

  @override
  String teamsInvitationNote(String note) {
    return 'Note: $note';
  }

  @override
  String get teamsAccept => 'Accept';

  @override
  String get teamsDecline => 'Decline';

  @override
  String get teamsCreateTitle => 'New team';

  @override
  String get teamsCreateBody =>
      'The name, colour and everything the team stores are end-to-end encrypted. The server only sees who is a member.';

  @override
  String get teamsNameLabel => 'Team name';

  @override
  String get teamsDescriptionLabel => 'Description (optional)';

  @override
  String get teamsColorLabel => 'Colour';

  @override
  String get teamsColorNone => 'Team colour';

  @override
  String get teamsColorCustom => 'Custom…';

  @override
  String teamsCreated(String name) {
    return 'Team \"$name\" created';
  }

  @override
  String get teamsJoinCodeTitle => 'Compare the code';

  @override
  String teamsJoinCodeBody(String email) {
    return 'Read this code to $email (in person, by phone or in a messenger). They see the same code on their screen and confirm it.';
  }

  @override
  String get teamsJoinCodeWait => 'The team opens on your devices as soon as the admin confirms.';

  @override
  String teamsJoinRequest(String email, String role) {
    return '$email accepted the invitation ($role). Compare the code before adding them.';
  }

  @override
  String get teamsCompareCode => 'Compare code';

  @override
  String teamsCompareTitle(String email) {
    return 'Add $email?';
  }

  @override
  String teamsCompareRole(String role) {
    return 'Role: $role';
  }

  @override
  String teamsCompareBody(String email) {
    return 'Ask $email to read the code on their screen. Add them only if every group matches.';
  }

  @override
  String teamsCodesMatchCheckbox(String email) {
    return 'The code on $email\'s screen is the same';
  }

  @override
  String get teamsCompareWarning =>
      'A different code means someone may be intercepting the invitation. Do not add the member.';

  @override
  String get teamsCodesDontMatch => 'Codes don\'t match';

  @override
  String get teamsAddMember => 'Add to team';

  @override
  String teamsMemberAdded(String email) {
    return '$email added to the team';
  }

  @override
  String get teamsCodeRejectedTitle => 'Join rejected';

  @override
  String get teamsCodeRejectedBody =>
      'Nobody was added. Invite the person again and compare the code over a channel you trust.';

  @override
  String get teamsTabMembers => 'Members';

  @override
  String get teamsTabSettings => 'Settings';

  @override
  String get teamsTabHistory => 'History';

  @override
  String get teamsWarningInvalidLog =>
      'The server showed a membership change nobody signed. The team is paused (read only) and no keys are shared until this is resolved.';

  @override
  String get teamsWarningRollback =>
      'The server showed an older or different team membership than before. The team is paused (read only).';

  @override
  String get teamsRotationRequired =>
      'A member left the team. Rotate the key so they cannot read anything added from now on.';

  @override
  String get teamsRotateKey => 'Rotate key';

  @override
  String get teamsKeyRotated => 'Team key rotated';

  @override
  String teamsMemberYou(String email) {
    return '$email (you)';
  }

  @override
  String get teamsPendingInvitations => 'Invitations sent';

  @override
  String get teamsCancelInvitation => 'Cancel';

  @override
  String get teamsInvite => 'Invite member';

  @override
  String get teamsTransferOwner => 'Make owner';

  @override
  String teamsTransferOwnerConfirm(String email) {
    return '$email becomes the owner of the team; you stay an admin.';
  }

  @override
  String get teamsRemoveMember => 'Remove from team';

  @override
  String teamsRemoveMemberConfirm(String email) {
    return '$email loses access to the team. The team key is rotated so nothing added later is readable for them.';
  }

  @override
  String teamsInviteTitle(String name) {
    return 'Invite to $name';
  }

  @override
  String get teamsInviteBody =>
      'The person needs a ConsoleCrypt account on this server. After they accept, you compare a code with them before they get access.';

  @override
  String get teamsInviteEmail => 'Email';

  @override
  String get teamsRoleLabel => 'Role';

  @override
  String get teamsInviteNote => 'Note for the invitation (optional)';

  @override
  String get teamsInviteNoteWarning => 'The note is not encrypted: the server can read it.';

  @override
  String get teamsInviteSend => 'Send invitation';

  @override
  String teamsInvited(String email) {
    return 'Invitation sent to $email';
  }

  @override
  String get teamsSettingsReadOnly => 'Only owners and admins change team settings.';

  @override
  String get teamsPoliciesTitle => 'Policies';

  @override
  String get teamsPolicyProduction => 'Mark new Kubernetes contexts and databases as production';

  @override
  String get teamsPolicyBlockAi => 'Never send these resources to AI assistants';

  @override
  String get teamsPolicyConfirmRisky => 'Ask before running risky commands';

  @override
  String get teamsPolicyClipboard => 'Clear copied secrets after';

  @override
  String get teamsPolicyClipboardDefault => 'App default';

  @override
  String teamsPolicySeconds(int seconds) {
    return '$seconds s';
  }

  @override
  String get teamsSettingsSaved => 'Team settings saved';

  @override
  String get teamsMyColorTitle => 'My colour for this team';

  @override
  String get teamsMyColorBody => 'Changes the colour only on your devices.';

  @override
  String get teamsLeave => 'Leave team';

  @override
  String teamsLeaveConfirm(String name) {
    return 'You lose access to \"$name\" on all your devices.';
  }

  @override
  String teamsKeyEpoch(int epoch) {
    return 'Key generation $epoch';
  }

  @override
  String teamsLogGenesis(String actor) {
    return '$actor created the team';
  }

  @override
  String teamsLogAdd(String actor, String member, String role) {
    return '$actor added $member as $role';
  }

  @override
  String teamsLogRole(String actor, String member, String role) {
    return '$actor changed the role of $member to $role';
  }

  @override
  String teamsLogRemove(String actor, String member) {
    return '$actor removed $member';
  }

  @override
  String teamsLogLeave(String actor) {
    return '$actor left the team';
  }

  @override
  String teamsLogTransfer(String actor, String member) {
    return '$actor made $member the owner';
  }

  @override
  String teamsLogRotate(String actor, int epoch) {
    return '$actor rotated the key (generation $epoch)';
  }

  @override
  String get teamsRoleOwner => 'Owner';

  @override
  String get teamsRoleAdmin => 'Admin';

  @override
  String get teamsRoleEditor => 'Editor';

  @override
  String get teamsRoleViewer => 'Viewer';

  @override
  String get teamsErrorReadOnly => 'You can only view this team\'s resources.';

  @override
  String get teamsErrorUntrusted => 'The team is paused: its membership could not be verified.';

  @override
  String get teamsErrorCrossVault =>
      'A team resource can only use resources of the same team. Copy the credential or jump host into the team first.';

  @override
  String get teamsErrorPersonalOnly => 'This kind of item stays in your personal vault.';

  @override
  String get teamsErrorAiBlocked => 'The team policy does not allow sending this resource to AI assistants.';

  @override
  String get teamsErrorOwnerCannotLeave => 'Make another member the owner before leaving.';

  @override
  String get teamsFilterAll => 'All';

  @override
  String get teamsFilterPersonal => 'Personal';

  @override
  String get teamsCopyTo => 'Copy to team…';

  @override
  String get teamsMoveTo => 'Move to team…';

  @override
  String get teamsMoveToPersonal => 'Move to personal';

  @override
  String get teamsPickTitle => 'Choose a team';

  @override
  String teamsCopied(String name) {
    return 'Copied to $name';
  }

  @override
  String teamsMoved(String name) {
    return 'Moved to $name';
  }

  @override
  String get teamsNewGroup => 'New team group';

  @override
  String get teamsNewGroupHint => 'Hosts created in a team group belong to the team.';

  @override
  String teamsPolicyAck(String host) {
    return 'I checked this command for $host (team policy)';
  }

  @override
  String get notesChooseHost => 'Choose a host';

  @override
  String get notesChooseHostEllipsis => 'Choose a host…';

  @override
  String get notesChosenClear => 'Back to this section\'s object';

  @override
  String get notesNoObject => 'No object selected';

  @override
  String get notesNoObjectTitle => 'Notes about a host';

  @override
  String get notesNoObjectMessage => 'Notes about a host appear here. Open a terminal tab or choose a host.';

  @override
  String get notesSaved => 'Saved';

  @override
  String get notesUnsaved => 'Unsaved changes';

  @override
  String get notesSaveTooltip => 'Save (⌘S / Ctrl+S)';

  @override
  String get notesUnsavedTitle => 'Leave without saving?';

  @override
  String get notesUnsavedMessage => 'The note could not be saved. Your latest changes will be lost.';

  @override
  String get notesDiscardChanges => 'Discard changes';

  @override
  String hostsNotesCount(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '$count notes', one: '1 note');
    return '$_temp0';
  }

  @override
  String get notesNotLinked => 'Not linked';

  @override
  String notesNotLinkedWas(String name) {
    return 'Not linked (was $name)';
  }

  @override
  String get notesAttachTo => 'Attach to…';

  @override
  String get notesFilter => 'Show';

  @override
  String get notesFilterAll => 'All notes';

  @override
  String notesFilterNotLinked(int count) {
    return 'Not linked ($count)';
  }

  @override
  String get dbFolderKeys => 'Keys';

  @override
  String get dbFolderForeignKeys => 'Foreign keys';

  @override
  String get dbFolderReferences => 'References';

  @override
  String get dbFolderDataTypes => 'Data types';

  @override
  String get dbFolderTableTriggers => 'Table triggers';

  @override
  String get dbLocalOnly => 'This device only';

  @override
  String get dbLocalOnlyHelp =>
      'Stored on this device only: not synced, backed up to the vault or shared, because older app versions cannot open this connection type.';

  @override
  String get dbSqlitePath => 'Database file path';

  @override
  String get dbSqliteLocation => 'File location';

  @override
  String get dbSqliteThisComputer => 'This computer';

  @override
  String get dbSqliteSshHost => 'SSH host';

  @override
  String get dbSqliteDocker => 'Docker container';

  @override
  String get dbSqliteKubernetes => 'Kubernetes Pod';

  @override
  String get dbSqliteKubeContext => 'Kubernetes context';

  @override
  String get dbSqliteChooseFile => 'Choose file…';

  @override
  String get dbSqliteCreateFile => 'Create new database…';

  @override
  String get dbSqliteReadOnly => 'Read only';

  @override
  String get dbSqliteReadOnlyHelp => 'The file opens read-only: no query can change it.';

  @override
  String get dbSqliteRemoteHelp =>
      'Queries run with the sqlite3 program (3.16 or newer) on the host, in the container or Pod; the file is not copied.';

  @override
  String get dbSqliteLocalUnavailable =>
      'Files on this device open in the desktop app only. Choose an SSH host, a container or a Pod.';

  @override
  String get dbErrorSqliteNotDatabase => 'The file is not an SQLite database (or it is encrypted).';

  @override
  String get dbErrorSqliteMissing =>
      'The database file was not found. A new file is created only with “Create new database”.';

  @override
  String get dbErrorSqliteCli =>
      'sqlite3 (3.16 or newer) is not installed on the host, in the container or Pod. Install it and try again.';

  @override
  String get dbErrorKubeExecBlocked =>
      'Running commands in this production Kubernetes context is turned off in its settings.';

  @override
  String get clipboardSecureCopyFailed => 'Couldn\'t copy securely. Nothing was placed on the clipboard.';

  @override
  String get copiedSecretPrivateNote => 'It won\'t appear in clipboard history or on other devices.';

  @override
  String get settingsClipboardBackgroundClear =>
      'Clear the clipboard in the background even if the secret can\'t be verified';

  @override
  String get settingsClipboardBackgroundClearHelp =>
      'Android doesn\'t let an app in the background read the clipboard. When this is on, the clipboard is emptied at the deadline without a check, so text you copied in another app during that time may be erased too. When it\'s off, the secret stays on the clipboard until you return to ConsoleCrypt.';

  @override
  String get settingsClipboardBackgroundClearError => 'Couldn\'t read or save this setting.';

  @override
  String get dbNullCell => '[NULL]';

  @override
  String get dbDefaultCell => '[DEFAULT]';

  @override
  String get dbRowAddedHint => 'New row, not saved yet';

  @override
  String get dbRowDeletedHint => 'Will be deleted on save';

  @override
  String get dbRequiredValue => 'A value is required';

  @override
  String get dbKeyColumn => 'Key column';

  @override
  String get dbTypeNumber => 'Number';

  @override
  String get dbTypeText => 'Text';

  @override
  String get dbTypeDate => 'Date and time';

  @override
  String get dbTypeBool => 'True/false';

  @override
  String get dbTypeJson => 'JSON';

  @override
  String get dbTypeBinary => 'Binary';

  @override
  String get dbEditUnsupported => 'Row editing is not available here';

  @override
  String get dbEditEngineUnsupported => 'Rows of this database type cannot be edited here';

  @override
  String get dbEditDesktopOnly => 'Rows are edited on a computer';

  @override
  String get dbEditNoKey => 'Editing needs a query of one table with a primary or unique key';

  @override
  String dbEditPartitions(int count) {
    return 'The rows are in $count partitions. Each partition changes all or nothing, but not together with the others: some partitions can be changed while others are not. Review the statements and enter the vault passphrase.';
  }

  @override
  String get dbEditCqlKey => 'Key columns of a Cassandra / ScyllaDB table cannot be edited';

  @override
  String get dbEditProductionLocked => 'Production is read-only: unlock writes first';

  @override
  String get dbEditReadOnly => 'The session is read-only: allow writes first';

  @override
  String dbRequiredMissing(String columns) {
    return 'Fill in the required columns: $columns';
  }

  @override
  String get dbSaveFailed => 'The changes were not saved';

  @override
  String get dbUnsavedTitle => 'Unsaved changes';

  @override
  String dbUnsavedMessage(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count rows have changes that are not saved.',
      one: '1 row has changes that are not saved.',
    );
    return '$_temp0 Save them first?';
  }

  @override
  String get dbSave => 'Save';

  @override
  String get dbSaveOptions => 'Save options';

  @override
  String get dbShowSql => 'Show SQL';

  @override
  String get dbPendingSql => 'Pending changes as SQL';

  @override
  String get dbRefreshReadOnly => 'only results of read queries refresh';

  @override
  String get dbRefreshUnavailable => 'This result can no longer be refreshed; run the query again';

  @override
  String get dbAutoRefresh => 'Auto-refresh';

  @override
  String get dbAutoRefreshOff => 'Off';

  @override
  String dbEverySeconds(int seconds) {
    return 'Every $seconds s';
  }

  @override
  String dbEveryMinutes(int minutes) {
    return 'Every $minutes min';
  }

  @override
  String get dbEditRow => 'Edit the current row';

  @override
  String get dbAddRow => 'Add a row';

  @override
  String get dbDuplicateRow => 'Duplicate the row';

  @override
  String get dbDeleteRow => 'Delete the row';

  @override
  String get dbFirstRow => 'First row';

  @override
  String get dbPreviousRow => 'Previous row';

  @override
  String get dbNextRow => 'Next row';

  @override
  String get dbLastRow => 'Last row';

  @override
  String get dbExportData => 'Export data…';

  @override
  String get dbImportData => 'Import data…';

  @override
  String get dbFetchSize => 'Rows per fetch';

  @override
  String dbFetchRows(int count) {
    return '$count rows';
  }

  @override
  String get dbFetchSizeInvalid => 'Enter a number from 1 to 1000';

  @override
  String get dbLoadNext => 'Load the next rows';

  @override
  String get dbFetchAll => 'Fetch all rows';

  @override
  String dbRowsFetched(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count rows fetched',
      one: '1 row fetched',
    );
    return '$_temp0';
  }

  @override
  String dbFetchedTiming(String seconds, String date, String time) {
    return '$seconds s, $date at $time';
  }

  @override
  String get dbMoreAvailable => 'more available';

  @override
  String dbPendingChanges(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count changes not saved',
      one: '1 change not saved',
    );
    return '$_temp0';
  }

  @override
  String get dbViewTable => 'Table';

  @override
  String get dbViewText => 'Text';

  @override
  String get dbViewRecord => 'Record';

  @override
  String dbTextMoreRows(int count) {
    return '… $count more rows (use export for all rows)';
  }

  @override
  String get dbNoRows => 'No rows';

  @override
  String get dbNewRow => 'New row';

  @override
  String dbRecordNumber(int row, int count) {
    return 'Row $row of $count';
  }

  @override
  String get dbKeepValue => 'Keep the value';

  @override
  String get dbRecordEditHint => 'Click to change the value';

  @override
  String get dbUseDefault => 'Leave to the server (default)';

  @override
  String dbSizeMb(String size) {
    return '$size MB';
  }

  @override
  String dbSizeKb(int size) {
    return '$size KB';
  }

  @override
  String dbCsvTitle(String table) {
    return 'Import CSV into $table';
  }

  @override
  String dbCsvStep(int step, int count, String title) {
    return 'Step $step of $count: $title';
  }

  @override
  String get dbCsvStepFile => 'File';

  @override
  String get dbCsvStepRead => 'Reading settings';

  @override
  String get dbCsvStepMap => 'Columns';

  @override
  String get dbCsvStepOptions => 'Import options';

  @override
  String get dbCsvStepRun => 'Import';

  @override
  String dbCsvFileHelp(String size) {
    return 'Choose a CSV, TSV or text file up to $size. It is read as a stream on this device.';
  }

  @override
  String get dbCsvChooseFile => 'Choose file…';

  @override
  String dbCsvTooLarge(int size) {
    return 'The file is larger than $size MB and cannot be imported here.';
  }

  @override
  String get dbCsvUnreadable => 'The file could not be read with these settings.';

  @override
  String get dbCsvPrivacy => 'The file\'s content goes only to this database.';

  @override
  String get dbCsvAuto => 'Detect';

  @override
  String get dbCsvComma => 'Comma ,';

  @override
  String get dbCsvSemicolon => 'Semicolon ;';

  @override
  String get dbCsvTab => 'Tab';

  @override
  String get dbCsvPipe => 'Bar |';

  @override
  String get dbCsvEncoding => 'Encoding';

  @override
  String get dbCsvEncodingAuto => 'UTF-8 (detect)';

  @override
  String get dbCsvDelimiter => 'Delimiter';

  @override
  String get dbCsvQuote => 'Quote character';

  @override
  String get dbCsvSkipRows => 'Lines to skip at the start';

  @override
  String get dbCsvNull => 'NULL is';

  @override
  String get dbCsvNullEmpty => 'An empty field';

  @override
  String get dbCsvNullNever => 'Nothing';

  @override
  String get dbCsvDecimal => 'Decimal separator';

  @override
  String get dbCsvDecimalDot => 'Dot 1.5';

  @override
  String get dbCsvDecimalComma => 'Comma 1,5';

  @override
  String get dbCsvDates => 'Dates';

  @override
  String get dbCsvHeader => 'The first row holds column names';

  @override
  String dbCsvDetected(String delimiter, String encoding) {
    return 'Read as: delimiter $delimiter, $encoding';
  }

  @override
  String get dbCsvMapHelp =>
      'Each file column goes to a table column or is skipped. The first 1000 rows are checked against the column types.';

  @override
  String get dbCsvSkipColumn => 'Skip';

  @override
  String get dbCsvDuplicateTarget => 'Two file columns go to the same table column.';

  @override
  String dbCsvMissingRequired(String columns) {
    return 'Required columns without values: $columns';
  }

  @override
  String dbCsvBadValues(int count, String samples) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count values do not fit: $samples',
      one: '1 value does not fit: $samples',
    );
    return '$_temp0';
  }

  @override
  String get dbCsvMode => 'Rows';

  @override
  String get dbCsvModeInsert => 'Insert';

  @override
  String get dbCsvModeSkip => 'Insert, skip existing keys';

  @override
  String get dbCsvModeUpsert => 'Insert or update by key';

  @override
  String get dbCsvNoKeyClickhouse => 'ClickHouse sorting keys are not unique: rows can only be added.';

  @override
  String get dbCsvNoKeyMongo => 'Map a column to _id to skip or replace existing documents.';

  @override
  String get dbCsvTruncateDocuments => 'Delete every document of the collection first';

  @override
  String get dbCsvMapHelpMongo =>
      'Each file column goes to a field or is skipped; a column can also create a new field of its name, typed by each value (number, true/false, date or text). The first 1000 rows are checked.';

  @override
  String get dbCsvMapHelpRedis =>
      'Map one column to key (the folder\'s prefix goes in front). Map one more to value for a string, or several to hash fields of their names (empty ones are left out).';

  @override
  String dbCsvNewField(String name) {
    return '$name · new field';
  }

  @override
  String get dbCsvNoKey => 'The table has no key: rows can only be inserted.';

  @override
  String get dbCsvBatch => 'Rows per batch (1–5000)';

  @override
  String get dbCsvBatchInvalid => 'Enter a batch size from 1 to 5000';

  @override
  String get dbCsvOnError => 'On a bad row';

  @override
  String get dbCsvStop => 'Stop the import';

  @override
  String get dbCsvSkipBad => 'Skip it and write an error report';

  @override
  String get dbCsvTransaction => 'Transaction';

  @override
  String get dbCsvWhole => 'One for the whole file (all or nothing)';

  @override
  String get dbCsvPerBatch => 'One per batch';

  @override
  String get dbCsvTruncate => 'Delete all rows of the table first';

  @override
  String get dbCsvTruncateHelp => 'Runs DELETE FROM in the import transaction; the review shows it.';

  @override
  String get dbCsvNoTransaction =>
      'Every statement commits on its own here (CQL, or sqlite3 on a host): a stopped import keeps the rows already written.';

  @override
  String get dbCsvTruncateHelpNoTransaction =>
      'Runs before the first row and is not undone if the import stops; the review shows it.';

  @override
  String get dbCsvImport => 'Review and import…';

  @override
  String dbCsvCounts(int read, int imported, int skipped, int failed) {
    return 'Read: $read · imported: $imported · skipped: $skipped · failed: $failed';
  }

  @override
  String dbCsvRate(int rows) {
    return '$rows rows/s';
  }

  @override
  String dbCsvRateEta(int rows, String left) {
    return '$rows rows/s · about $left left';
  }

  @override
  String dbCsvDone(int imported, int skipped, int failed) {
    return 'Done: $imported imported, $skipped skipped, $failed with errors.';
  }

  @override
  String dbCsvCancelled(int imported) {
    return 'Cancelled. Rows kept: $imported (committed batches only).';
  }

  @override
  String get dbCsvFailed => 'The import failed; the open transaction was rolled back.';

  @override
  String dbCsvRowType(int line) {
    return 'Line $line: a value does not fit its column. Nothing of the open transaction was kept.';
  }

  @override
  String dbCsvRowRequired(int line) {
    return 'Line $line: a required column is empty. Nothing of the open transaction was kept.';
  }

  @override
  String dbCsvRowUnreadable(int line) {
    return 'Line $line cannot be read (encoding or size). Nothing of the open transaction was kept.';
  }

  @override
  String get dbCsvReport => 'Error report';

  @override
  String get dbCsvShowReport => 'Show in folder';

  @override
  String get dbCsvCreateTable => 'Create table from CSV…';

  @override
  String get dbCsvNewTableName => 'New table name';

  @override
  String get dbImportCsv => 'Import from CSV…';

  @override
  String get dbDesignerCreateTable => 'Create table…';

  @override
  String get dbDesignerCreateCollection => 'Create collection…';

  @override
  String get dbDesignerAlterTable => 'Edit structure…';

  @override
  String dbDesignerTabTitle(String name) {
    return 'Design: $name';
  }

  @override
  String get dbDesignerNewTab => 'New table';

  @override
  String dbDesignerUnsavedTitle(String name) {
    return 'Save changes to $name?';
  }

  @override
  String get dbDesignerUnsavedMessage => 'The table designer has changes that are not applied to the database yet.';

  @override
  String get dbDesignerDontSave => 'Don\'t save';

  @override
  String get dbDesignerSaveButton => 'Save…';

  @override
  String get dbDesignerLoadFailed => 'Could not load the table structure.';

  @override
  String dbDesignerSaved(String name) {
    return '$name: changes applied';
  }

  @override
  String get dbDesignerUnsavedMark => 'Unsaved changes';

  @override
  String get dbDesignerDraftNew => 'New, not saved yet';

  @override
  String get dbDesignerDraftChanged => 'Changed, not saved yet';

  @override
  String dbDesignerSaveTitle(String name) {
    return '$name — save changes';
  }

  @override
  String get dbDesignerSqlPreview => 'SQL preview';

  @override
  String get dbDesignerTransactional => 'Runs in one transaction: all statements apply or none.';

  @override
  String get dbDesignerNotTransactional =>
      'DDL is not transactional here: if a statement fails, the ones before it stay applied.';

  @override
  String get dbDesignerDestructive =>
      'The script drops data or rewrites the table (dropped columns, type or key changes).';

  @override
  String get dbDesignerProductionLocked =>
      'This is a production connection. Unlock production writes in the workspace header to run the script.';

  @override
  String get dbDesignerReadOnly => 'The session is read-only. Allow writes in the workspace header to run the script.';

  @override
  String get dbDesignerConfirm => 'Enter the vault passphrase to run the script.';

  @override
  String get dbDesignerFailedRolledBack => 'A statement failed; nothing was changed.';

  @override
  String get dbDesignerFailedPartly => 'A statement failed; the statements before it were applied.';

  @override
  String dbDesignerStatementResult(int index, String status) {
    return '$index. $status';
  }

  @override
  String get dbDesignerStatusDone => 'applied';

  @override
  String get dbDesignerStatusRolledBack => 'rolled back';

  @override
  String get dbDesignerStatusSkipped => 'not run';

  @override
  String get dbDesignerStatementFailed => 'failed on the server';

  @override
  String get dbDesignFields => 'Fields';

  @override
  String get dbDesignColumns => 'Columns';

  @override
  String get dbDesignKeys => 'Keys';

  @override
  String get dbDesignForeignKeys => 'Foreign keys';

  @override
  String get dbDesignIndexes => 'Indexes';

  @override
  String get dbDesignChecks => 'Checks';

  @override
  String get dbDesignOptions => 'Options';

  @override
  String get dbDesignDdl => 'DDL';

  @override
  String get dbDesignHasErrors => 'Has errors';

  @override
  String get dbDesignNew => 'New';

  @override
  String get dbDesignChanged => 'Changed';

  @override
  String get dbDesignPickColumns => 'Choose columns';

  @override
  String get dbDesignCollectionName => 'Collection name';

  @override
  String get dbDesignTableName => 'Table name';

  @override
  String get dbDesignSchema => 'Schema';

  @override
  String get dbDesignDatabase => 'Database';

  @override
  String get dbDesignComment => 'Comment';

  @override
  String get dbDesignDescription => 'Description';

  @override
  String get dbDesignModeAlter => 'Edit structure';

  @override
  String get dbDesignModeCreate => 'New table';

  @override
  String get dbDesignPending => 'Changed';

  @override
  String get dbDesignUndo => 'Undo';

  @override
  String get dbDesignRevert => 'Revert all changes';

  @override
  String get dbDesignSave => 'Save changes';

  @override
  String get dbDesignSaveShortcut => 'Save changes (⌘S / Ctrl+S)';

  @override
  String get dbDesignColName => 'Name';

  @override
  String get dbDesignColType => 'Type';

  @override
  String get dbDesignBsonType => 'BSON type';

  @override
  String get dbDesignColLength => 'Length';

  @override
  String get dbDesignColScale => 'Scale';

  @override
  String get dbDesignRequired => 'Required';

  @override
  String get dbDesignNullable => 'Nullable';

  @override
  String get dbDesignNotNull => 'Not null';

  @override
  String get dbDesignColDefault => 'Default';

  @override
  String get dbDesignColIdentity => 'Auto increment';

  @override
  String get dbDesignPk => 'PK';

  @override
  String get dbDesignUnique => 'Unique';

  @override
  String get dbDesignUnsigned => 'Unsigned';

  @override
  String get dbDesignArray => 'Array';

  @override
  String get dbDesignLowCardinality => 'LowCard.';

  @override
  String get dbDesignPickType => 'Choose a type';

  @override
  String get dbDesignDefaultPresets => 'Default presets';

  @override
  String get dbDesignNoDefault => 'No default';

  @override
  String get dbDesignDuplicate => 'Duplicate';

  @override
  String get dbDesignMoveUp => 'Move up';

  @override
  String get dbDesignMoveDown => 'Move down';

  @override
  String get dbDesignRemove => 'Remove';

  @override
  String get dbDesignColumnActions => 'Column actions';

  @override
  String get dbDesignWillDrop => 'will be dropped';

  @override
  String get dbDesignRestore => 'Restore';

  @override
  String get dbDesignAddField => 'Add field';

  @override
  String get dbDesignAddColumn => 'Add column';

  @override
  String get dbDesignPrimaryKey => 'Primary key';

  @override
  String get dbDesignUniqueKeys => 'Unique constraints';

  @override
  String get dbDesignAutoName => 'Name (automatic)';

  @override
  String get dbDesignAddUnique => 'Add unique constraint';

  @override
  String get dbDesignRefTable => 'Referenced table';

  @override
  String get dbDesignPickTable => 'Choose a table';

  @override
  String get dbDesignOnDelete => 'On delete';

  @override
  String get dbDesignOnUpdate => 'On update';

  @override
  String get dbDesignDefault => 'Default';

  @override
  String get dbDesignAddForeignKey => 'Add foreign key';

  @override
  String get dbDesignAddIndex => 'Add index';

  @override
  String get dbDesignCheckExpression => 'Condition, e.g. price >= 0';

  @override
  String get dbDesignAddCheck => 'Add check';

  @override
  String get dbDesignCreateOnly => 'Set when the table is created';

  @override
  String get dbDesignClickhouseHint =>
      'Sorting key: column names or expressions, comma-separated; empty uses the primary key. TTL and SETTINGS can be added in the editor later.';

  @override
  String get dbDesignFixErrors => 'Fix the errors to see the DDL.';

  @override
  String get dbDesignNoChanges => 'No changes.';

  @override
  String get dbDesignNone => '—';

  @override
  String get dbDesignIdentityByDefault => 'Identity (by default)';

  @override
  String get dbDesignIdentityAlways => 'Identity (always)';

  @override
  String get dbDesignIdentitySerial => 'serial';

  @override
  String get dbDesignIdentityAutoIncrement => 'AUTO_INCREMENT';

  @override
  String get dbDesignOptEngine => 'Engine';

  @override
  String get dbDesignOptEngineArgs => 'Engine parameters';

  @override
  String get dbDesignOptOrderBy => 'ORDER BY (sorting key)';

  @override
  String get dbDesignOptPartitionBy => 'Partition by';

  @override
  String get dbDesignOptPartitionKey => 'Partition key';

  @override
  String get dbDesignOptCharset => 'Character set';

  @override
  String get dbDesignOptCollation => 'Collation';

  @override
  String get dbDesignOptAutoIncrement => 'AUTO_INCREMENT start';

  @override
  String get dbDesignOptCapped => 'Capped';

  @override
  String get dbDesignOptSize => 'Size, bytes';

  @override
  String get dbDesignOptMax => 'Max documents';

  @override
  String get dbDesignOptTimeField => 'Time field (time series)';

  @override
  String get dbDesignOptMetaField => 'Meta field';

  @override
  String get dbDesignOptGranularity => 'Granularity';

  @override
  String get dbDesignOptExpire => 'Expire after, seconds';

  @override
  String get dbDesignOptValidationLevel => 'Validation level';

  @override
  String get dbDesignOptValidationAction => 'Validation action';

  @override
  String get dbDesignOptCollationLocale => 'Collation locale';

  @override
  String get dbDesignOptCollationStrength => 'Collation strength';

  @override
  String get dbDesignIssueNameRequired => 'A name is required.';

  @override
  String dbDesignIssueNameInvalid(String name) {
    return '“$name” cannot be used as a name here.';
  }

  @override
  String dbDesignIssueNameTooLong(String name) {
    return '“$name” is too long for this database.';
  }

  @override
  String dbDesignIssueNameSpaces(String name) {
    return '“$name” starts or ends with a space.';
  }

  @override
  String dbDesignIssueNameDuplicate(String name) {
    return 'The name “$name” is used twice.';
  }

  @override
  String get dbDesignIssueColumnsRequired => 'Add at least one column.';

  @override
  String dbDesignIssueTypeInvalid(String name) {
    return '“$name” is not a valid type.';
  }

  @override
  String dbDesignIssueLengthRequired(String name) {
    return '$name: this type needs a length.';
  }

  @override
  String dbDesignIssueLengthIgnored(String name) {
    return '$name: this type takes no length; it is ignored.';
  }

  @override
  String dbDesignIssueLengthInvalid(String name) {
    return '$name: check the length and scale.';
  }

  @override
  String dbDesignIssueExpressionInvalid(String name) {
    return '“$name”: not a single expression (no “;”, comments or unbalanced brackets/quotes).';
  }

  @override
  String get dbDesignIssueExpressionRequired => 'Enter the check condition.';

  @override
  String dbDesignIssueIdentityType(String name) {
    return '$name: auto increment needs an integer type.';
  }

  @override
  String dbDesignIssueIdentityDefault(String name) {
    return '$name: an auto-increment column cannot have a default.';
  }

  @override
  String dbDesignIssueIdentityChange(String name) {
    return '$name: this auto-increment change cannot be made here; use the editor.';
  }

  @override
  String get dbDesignIssueKeyColumns => 'Choose at least one column.';

  @override
  String dbDesignIssueColumnUnknown(String name) {
    return 'There is no column “$name”.';
  }

  @override
  String dbDesignIssueColumnRepeated(String name) {
    return 'Column “$name” is listed twice.';
  }

  @override
  String get dbDesignIssueRefTable => 'Choose the referenced table.';

  @override
  String get dbDesignIssueRefColumns => 'Choose as many referenced columns as key columns.';

  @override
  String dbDesignIssueAutoIncrementKey(String name) {
    return '$name: AUTO_INCREMENT needs a key that starts with this column.';
  }

  @override
  String get dbDesignIssueAutoIncrementSingle => 'Only one column can be AUTO_INCREMENT.';

  @override
  String dbDesignIssueUnsigned(String name) {
    return '$name: UNSIGNED applies to numeric types only.';
  }

  @override
  String dbDesignIssueForeignKeysEngine(String name) {
    return 'Foreign keys need the InnoDB engine (now $name).';
  }

  @override
  String get dbDesignIssueOrderBy => 'MergeTree tables need a sorting key: set ORDER BY or a primary key.';

  @override
  String dbDesignIssuePrimaryKeyPrefix(String name) {
    return 'The primary key ($name) must be the start of ORDER BY.';
  }

  @override
  String dbDesignIssueNullableKey(String name) {
    return '$name: a key column cannot be Nullable.';
  }

  @override
  String dbDesignIssueEngineOption(String name) {
    return 'The $name engine has no keys or partitions.';
  }

  @override
  String get dbDesignIssuePartitionKey => 'Enter the partition key.';

  @override
  String get dbDesignIssuePartitionPrimaryKey =>
      'The primary key of a partitioned table must include the partition key.';

  @override
  String get dbDesignIssueCommentBackslash => 'In MySQL and MariaDB a comment cannot contain a backslash (\\) yet.';

  @override
  String dbDesignIssueOptionInvalid(String name) {
    return 'Check the value “$name”.';
  }

  @override
  String dbDesignIssueOptionCreateOnly(String name) {
    return 'Option “$name” is set when the table is created and is not changed here.';
  }

  @override
  String get dbDesignIssueCommentChange => 'The table comment is not changed here; use the editor.';

  @override
  String dbDesignIssueNotNullDefault(String name) {
    return '$name: NOT NULL without a default fails if the table has rows.';
  }

  @override
  String dbDesignIssueColumnOrder(String name) {
    return '$name is added at the end of the table.';
  }

  @override
  String get dbDesignIssueRename => 'Renaming or moving the table is not supported here.';

  @override
  String get dbDesignIssuePrimaryKeyChange => 'The primary key of this table cannot be changed.';

  @override
  String get dbDesignIssueCappedSize => 'A capped collection needs a size.';

  @override
  String get dbDesignIssueCappedTimeSeries => 'A time-series collection cannot be capped.';

  @override
  String get dbDesignIssueTimeField => 'Set the time field first.';

  @override
  String get dbDesignIssueCollationLocale => 'Set the collation locale first.';

  @override
  String dbDesignIssueDefaultIgnored(String name) {
    return '$name: defaults are not part of a collection and are ignored.';
  }

  @override
  String dbDesignIssueNullableTwice(String name) {
    return '$name: use the Nullable checkbox instead of Nullable(…) in the type.';
  }

  @override
  String dbDesignIssueOther(String name) {
    return 'Check the design ($name).';
  }

  @override
  String dbInsertSnippetHelp(String shortcut) {
    return 'Snippets: insert into the editor ($shortcut)';
  }

  @override
  String get stepUpOsReason => 'confirm a protected action';

  @override
  String stepUpOsReasonFor(String action) {
    return 'confirm “$action”';
  }

  @override
  String dbUnlockProductionOsReason(String connection) {
    return 'allow changes on $connection';
  }

  @override
  String get stepUpConfirmTouchId => 'Confirm with Touch ID';

  @override
  String get stepUpConfirmFaceId => 'Confirm with Face ID';

  @override
  String get stepUpConfirmWindowsHello => 'Confirm with Windows Hello';

  @override
  String get stepUpConfirmFingerprint => 'Confirm with fingerprint';

  @override
  String get stepUpConfirmDeviceCredential => 'Confirm with the device password';

  @override
  String get stepUpUsePassphrase => 'Enter the vault passphrase';

  @override
  String stepUpPresenceHint(String method) {
    return 'You will confirm with $method.';
  }

  @override
  String get stepUpPresenceFailed => 'Not confirmed. Enter the vault passphrase instead.';

  @override
  String get errorPassphraseRequired => 'Confirm with the vault passphrase.';

  @override
  String get confirmActionsWithPresence => 'Confirm actions with biometrics';

  @override
  String confirmActionsWithPresenceHelp(String method) {
    return '$method instead of the vault passphrase for risky actions on this device. Changing the passphrase, a new Recovery Kit, turning off device unlock, backups, approving devices and account security always ask for the passphrase.';
  }

  @override
  String get dbKeyspace => 'Keyspace';

  @override
  String get dbCassandraContactPoints => 'More contact points (hosts, comma-separated)';

  @override
  String get dbCassandraLocalDc => 'Local data center';

  @override
  String get dbCassandraReadConsistency => 'Read consistency';

  @override
  String get dbCassandraWriteConsistency => 'Write consistency';

  @override
  String get dbCassandraPageSize => 'Rows per page';

  @override
  String get dbCassandraRouteHelp =>
      'Directly, requests spread over every node in the data center of the first contact point that answers, and move to other nodes when one fails. Through SSH, a jump profile, a proxy, a tunnel, Kubernetes or Docker the session works through one node: the first one that answers. With a local data center set, the contact point must be in it.';

  @override
  String get dbErrorDatacenter =>
      'The node that answered is in another data center than the configured local one. Check the contact points or the local data center.';

  @override
  String get dbErrorEngineChange =>
      'This connection cannot switch to that engine: synced connections and connections kept on this device are stored separately. Create a new connection instead.';

  @override
  String get dbErrorDeviceOnly => 'This connection is kept on this device only and cannot be shared or synced.';

  @override
  String get dbFolderTypes => 'Types';

  @override
  String get dbScanWarningTitle => 'Whole-table read';

  @override
  String get dbScanAllowFiltering =>
      'ALLOW FILTERING makes the cluster read and filter rows of the whole table, which can be slow and load every node. Run it anyway?';

  @override
  String get dbScanFullTable =>
      'This query has no WHERE and no LIMIT: it reads the whole table page by page across the cluster. Run it anyway?';

  @override
  String get snippetPackCassandra => 'Cassandra / ScyllaDB';

  @override
  String get snippetPackCassandraDescription =>
      'Schema, cluster nodes, size estimates, partitions, TTLs and roles in CQL. For cqlsh or the Databases workspace.';

  @override
  String get snippetDbCqlKeyspaces => 'Keyspaces and replication';

  @override
  String get snippetDbCqlKeyspacesNote => 'Every keyspace with its replication strategy and durable writes.';

  @override
  String get snippetDbCqlTables => 'Tables of a keyspace';

  @override
  String get snippetDbCqlTablesNote => 'Tables with their comment, default TTL and gc_grace_seconds.';

  @override
  String get snippetDbCqlColumns => 'Columns and primary key';

  @override
  String get snippetDbCqlColumnsNote =>
      'Columns of a table: partition key, clustering columns with their order, static and regular columns.';

  @override
  String get snippetDbCqlIndexes => 'Secondary indexes';

  @override
  String get snippetDbCqlIndexesNote => 'Secondary indexes of a keyspace with their targets.';

  @override
  String get snippetDbCqlViews => 'Materialized views';

  @override
  String get snippetDbCqlViewsNote => 'Materialized views of a keyspace with their base table and filter.';

  @override
  String get snippetDbCqlTypes => 'User-defined types';

  @override
  String get snippetDbCqlTypesNote => 'User-defined types of a keyspace with their fields.';

  @override
  String get snippetDbCqlLocalNode => 'This node';

  @override
  String get snippetDbCqlLocalNodeNote => 'Cluster name, data center, rack and version of the coordinator.';

  @override
  String get snippetDbCqlPeers => 'Cluster peers';

  @override
  String get snippetDbCqlPeersNote => 'Other nodes as the coordinator sees them: address, data center, rack, version.';

  @override
  String get snippetDbCqlSizeEstimates => 'Table size estimates';

  @override
  String get snippetDbCqlSizeEstimatesNote =>
      'Estimated partitions and mean partition size per token range on the coordinator.';

  @override
  String get snippetDbCqlPartition => 'Rows of one partition';

  @override
  String get snippetDbCqlPartitionNote =>
      'Reads one partition by a text partition key — the efficient way to read in CQL.';

  @override
  String get snippetDbCqlTtl => 'TTL and write time of a column';

  @override
  String get snippetDbCqlTtlNote => 'Remaining TTL and write timestamp of a column within one partition.';

  @override
  String get snippetDbCqlCountPartition => 'Rows in a partition';

  @override
  String get snippetDbCqlCountPartitionNote =>
      'Counts the rows of one partition (counting a whole table scans the cluster).';

  @override
  String get snippetDbCqlTokens => 'Partition tokens';

  @override
  String get snippetDbCqlTokensNote => 'First partition keys with their token, to see how data spreads over the ring.';

  @override
  String get snippetDbCqlRoles => 'Roles';

  @override
  String get snippetDbCqlRolesNote => 'Roles with their login and superuser flags.';

  @override
  String get snippetDbCqlPermissions => 'Permissions of a role';

  @override
  String get snippetDbCqlPermissionsNote => 'Every permission granted to a role, directly or through other roles.';

  @override
  String get snippetDbCqlCreateTable => 'Create a time-series table';

  @override
  String get snippetDbCqlCreateTableNote =>
      'Partition per key, newest rows first (clustering order), rows expire after a default TTL.';

  @override
  String get snippetDbCqlInsertJson => 'Insert a row from JSON with a TTL';

  @override
  String get snippetDbCqlInsertJsonNote =>
      'INSERT … JSON maps JSON fields to columns of any type; the row expires after the TTL.';

  @override
  String syncStateThrottled(String time) {
    return 'The server temporarily limited sync; resuming at $time';
  }

  @override
  String get syncScreenStateThrottled => 'paused by the server';

  @override
  String get syncScreenThrottledBody =>
      'The server received too many requests from this account. Changes stay on this device and are sent automatically — nothing is lost.';

  @override
  String get syncScreenRejectedTitle => 'Changes not accepted by the server';

  @override
  String syncScreenRejectedBody(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other:
          '$count changes were not accepted by the server. They are kept on this device; choose Retry once the problem is fixed.',
      one:
          '1 change was not accepted by the server. It is kept on this device; choose Retry once the problem is fixed.',
    );
    return '$_temp0';
  }

  @override
  String errorObjectLimit(String limit) {
    return 'The vault has too many objects (limit $limit). Delete items you no longer need, then choose Retry.';
  }

  @override
  String errorVaultQuota(String size) {
    return 'The vault has used up its server storage (limit $size). Delete items you no longer need, then choose Retry.';
  }

  @override
  String errorDeviceLimit(String limit) {
    return 'Device limit ($limit) reached — revoke devices you no longer use, then sign in again.';
  }

  @override
  String errorDeviceDailyLimit(String time) {
    return 'Too many new devices today. Try again after $time.';
  }

  @override
  String get errorVaultKeyMismatch =>
      'The server sent a different vault key. Nothing was opened with it and sync is stopped.';

  @override
  String get errorRecipientUnverified =>
      'Verify this device\'s keys first: only verified recipients may get this kind of access.';

  @override
  String get errorRecipientKeysChanged => 'The keys of this device changed. Verify it again before giving access.';

  @override
  String get errorPeerVerificationFailed =>
      'The code does not match. The keys may have been replaced — do not give access and contact your colleague another way.';

  @override
  String get vaultKeyMismatchTitle => 'Different vault key';

  @override
  String get vaultKeyMismatchMessage =>
      'The server sent a different vault key. This may mean the server was tampered with. Data on this device is unchanged and sync is stopped. Do not enter secrets on this device and tell the server administrator.';

  @override
  String get vaultKeyMismatchLock => 'Lock';

  @override
  String get vaultKeyMismatchCopy => 'Copy diagnostics';

  @override
  String get vaultKeyMismatchMore => 'More';

  @override
  String get vaultKeyMismatchDetails =>
      'This device remembers the fingerprint of its vault key and checks every key it receives against it. A different key is never accepted and nothing is re-encrypted with it. The only way out is resetting the profile on this device (remove it and connect again) after the server is checked.';

  @override
  String approvalWaitPairingTitle(String device) {
    return 'Compare the code with “$device”';
  }

  @override
  String approvalWaitPairingHelp(String device) {
    return '“$device” is approving this device. Both screens must show exactly the same code — it is computed from the keys of both devices.';
  }

  @override
  String approvalWaitPairingCheckbox(String device) {
    return 'The code matches the one on “$device”';
  }

  @override
  String get approvalWaitPairingConfirm => 'Codes match';

  @override
  String get approvalWaitPairingConfirmed => 'Confirmed. Waiting for the approval to finish…';

  @override
  String get approveDeviceCodeV2Hint =>
      'This code depends on the keys of both devices; the new device shows it as soon as you open this request.';

  @override
  String get devicesVaultKeyUnconfirmedTitle => 'Vault key not confirmed';

  @override
  String get devicesVaultKeyUnconfirmedMessage =>
      'This device received the vault key without an independent check (older server or approving device). Enter the vault passphrase once to confirm it.';

  @override
  String get devicesVaultKeyConfirm => 'Confirm with passphrase';

  @override
  String get vaultKeyConfirmTitle => 'Confirm the vault key';

  @override
  String get vaultKeyConfirmed => 'Vault key confirmed';

  @override
  String get devicesVaultKeyConflictTitle => 'A device received a different vault key';

  @override
  String devicesVaultKeyConflictMessage(String device) {
    return '“$device” reported a different vault key. Someone who controls the server may have given it their own key. This device keeps its key. Revoke “$device” unless you know why.';
  }

  @override
  String get devicesRevokeConflicting => 'Revoke';

  @override
  String get sharingTrustChosen => 'Chosen (not verified)';

  @override
  String sharingTrustVerified(String date) {
    return 'Verified $date';
  }

  @override
  String get sharingTrustVerifiedNoDate => 'Verified';

  @override
  String get sharingTrustKeysChanged => 'Keys changed';

  @override
  String get sharingTrustNew => 'Not chosen yet';

  @override
  String get sharingVerifyAction => 'Verify';

  @override
  String get sharingGiveAccess => 'Give access to this device';

  @override
  String sharingKeysChangedWarning(String device, String email) {
    return 'The keys of device $device of $email have changed. No new access until it is verified again; existing shares are not re-encrypted for the new keys.';
  }

  @override
  String get sharingVerifiedOnlyHint =>
      'Only verified recipients can receive secrets, Kubernetes contexts and database connections.';

  @override
  String get sharingFingerprint => 'Key fingerprint';

  @override
  String get peerVerifyTitle => 'Verify the colleague\'s device';

  @override
  String peerVerifyDevice(String device) {
    return 'Device $device';
  }

  @override
  String get peerVerifySafetyNumber => 'Safety number';

  @override
  String get peerVerifyQr => 'QR code';

  @override
  String get peerVerifySafetyNumberHelp =>
      'Ask your colleague to open the same check for your device and read out their number — by phone or in person, not through ConsoleCrypt. Type it below: it must match the number on this screen exactly.';

  @override
  String get peerVerifyOwnNumber => 'Number on this device';

  @override
  String get peerVerifyEnterNumber => 'Number read out by the colleague';

  @override
  String get peerVerifyNoPaste => 'Type the number — pasting is disabled on purpose.';

  @override
  String get peerVerifyQrHelp =>
      'In person: show this QR code to your colleague and scan theirs with a phone. On a computer without a camera, enter the text of the colleague\'s QR code.';

  @override
  String get peerVerifyQrInput => 'Text of the colleague\'s QR code';

  @override
  String get peerVerifyConfirm => 'Verify';

  @override
  String get peerVerifySuccess => 'Device verified';

  @override
  String get sharingAcceptOwnerHelp =>
      'Accepting pins the owner\'s keys on this device. Verify them with the owner to mark the item as verified.';

  @override
  String get sharingOwnerUnverified => 'Owner not verified';

  @override
  String get sharingOwnerUnverifiedNote =>
      'Hosts from this item are never connected automatically, snippets show their source.';

  @override
  String get sharingVerifyBannerTitle => 'Verify colleagues who have access to secrets';

  @override
  String get sharingVerifyBannerMessage =>
      'These recipients of secrets, Kubernetes contexts and database connections were added before keys could be verified. Their access is unchanged.';

  @override
  String get sharingVerifyBannerDismiss => 'Hide';

  @override
  String get sharingPolicyTitle => 'Sharing: verified recipients only';

  @override
  String get sharingPolicySensitive => 'Secrets, Kubernetes contexts and database connections';

  @override
  String get sharingPolicyOther => 'Hosts, groups and snippets';

  @override
  String get sharingPolicyHelp =>
      'New access and new devices need verified keys. Existing recipients keep their access.';

  @override
  String get enrollmentTypeCode => 'Code shown on the other device';

  @override
  String get enrollmentTypeCodeHelp =>
      'Type the six groups of digits from the other device\'s screen. The code is never filled in for you.';

  @override
  String get vaultKeyMismatchDiagnostics => 'Diagnostics';

  @override
  String get dbDesignIdentityAutoincrement => 'AUTOINCREMENT';

  @override
  String dbDesignIssueAutoincrementKey(String name) {
    return '$name: AUTOINCREMENT needs this column alone as an INTEGER primary key of a rowid table.';
  }

  @override
  String get dbDesignIssueAutoincrementSingle => 'Only one column can have AUTOINCREMENT.';

  @override
  String dbDesignIssueRebuildBlocked(String name) {
    return 'This change recreates the table, and the table has what the designer does not carry over: $name. Make it in the SQL editor.';
  }

  @override
  String dbDesignIssueTableRebuild(String name) {
    return '$name will be recreated: the rows are copied into a new table and the old one is dropped, all in one transaction. Triggers of the table are not kept, so the change is refused when it has any.';
  }

  @override
  String dbDesignIssueTypeStrict(String name) {
    return 'A STRICT table takes only INT, INTEGER, REAL, TEXT, BLOB and ANY: $name.';
  }

  @override
  String get dbDesignIssuePrimaryKeyRequired => 'Choose the primary key columns.';

  @override
  String dbDesignIssueCommentIgnored(String name) {
    return '$name: this database keeps no comments; the comment is not saved.';
  }

  @override
  String dbDesignIssueColumnRename(String name) {
    return 'Only primary key columns can be renamed: $name.';
  }

  @override
  String dbDesignIssueTypeChange(String name) {
    return 'The column type cannot be changed: $name.';
  }

  @override
  String dbDesignIssueCounterMixed(String name) {
    return '$name: counter columns cannot be in the key, and a table with counters has only counters besides the key.';
  }

  @override
  String get dbDesignIssueIndexSingleColumn => 'A secondary index covers one column.';

  @override
  String get dbDesignUnmodeledTriggers => 'triggers';

  @override
  String get dbDesignUnmodeledGenerated => 'generated columns';

  @override
  String get dbDesignUnmodeledCollations => 'column collations';

  @override
  String get dbDesignUnmodeledIndexes => 'expression, partial or ordered indexes';

  @override
  String get dbDesignUnmodeledOther => 'constraints the designer cannot read';

  @override
  String get dbDesignOptStrict => 'STRICT (strict column types)';

  @override
  String get dbDesignOptUnlogged => 'UNLOGGED (no write-ahead log: faster, emptied after a crash)';

  @override
  String get dbDesignIssueUnloggedPartitioned => 'A partitioned table cannot be UNLOGGED.';

  @override
  String get dbDesignOptWithoutRowid => 'WITHOUT ROWID';

  @override
  String get dbDesignOptPartitionColumns => 'Key columns in the partition key';

  @override
  String get dbDesignOptClusteringOrder => 'Clustering order (for example, at DESC)';

  @override
  String get dbDesignOptDefaultTtl => 'Default TTL, seconds';

  @override
  String get dbDesignOptGcGrace => 'gc_grace_seconds';

  @override
  String get aiHistoryTitle => 'Conversation history';

  @override
  String get aiHistoryEmpty =>
      'No conversations yet. To keep them after the vault locks, turn on “Keep AI conversations in the vault” in Settings.';

  @override
  String get aiHistoryFailed => 'Could not load the conversations.';

  @override
  String get aiHistoryUntitled => 'Untitled conversation';

  @override
  String get aiHistoryKept => 'kept in the vault';

  @override
  String get aiHistoryUntilLock => 'until the vault locks';

  @override
  String get aiHistoryProviderGone => 'its provider was removed';

  @override
  String aiHistoryMessages(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '$count messages', one: '1 message');
    return '$_temp0';
  }

  @override
  String syncVaultDeletionTitle(String date) {
    return 'This vault will be deleted on $date';
  }

  @override
  String get syncVaultDeletionBody =>
      'Another device of your account that is not trusted yet asked to delete it. If it was not you, keep the vault and change your account password.';

  @override
  String get syncVaultDeletionCancel => 'Keep the vault';

  @override
  String get syncVaultDeletionCancelled => 'The deletion is cancelled.';

  @override
  String get dbMoveToVault => 'Sync with the vault';

  @override
  String get dbMoveToVaultHelp =>
      'The connection and its password move into the vault: they sync to all your devices and are backed up with the vault. It does not go back to this device only.';

  @override
  String get dbMoveToVaultConfirm => 'Move into the vault';

  @override
  String get dbMovedToVault => 'The connection now syncs with the vault.';

  @override
  String get dbMoveOutdatedTitle => 'Update your devices first';

  @override
  String get dbMoveOutdatedBody =>
      'Apps before 0.9.2 cannot read this connection. Update these devices of your account (or sign them out), then try again:';

  @override
  String get dbMoveVersionUnknown => 'version unknown';

  @override
  String get aboutServerSource => 'Source code of your sync server:';

  @override
  String get startupFailedTitle => 'ConsoleCrypt could not start its secure core';

  @override
  String get startupFailedBody =>
      'Your data was not touched. Reinstall the app or rebuild it with the Rust toolchain installed, then try again.';

  @override
  String get startupFailedStorageBody =>
      'Existing storage could not be opened safely. Your data was not changed. Resolve the storage issue below and try again.';

  @override
  String get snippetPackSqlite => 'SQLite';

  @override
  String get snippetPackSqliteDescription =>
      'Schema, indexes, foreign keys, integrity, size and query plans of an SQLite file. For sqlite3 or the Databases workspace.';

  @override
  String get snippetDbSqliteTables => 'Tables and views';

  @override
  String get snippetDbSqliteTablesNote =>
      'Every table, view, index and trigger of the file except the internal sqlite_ ones.';

  @override
  String get snippetDbSqliteColumns => 'Columns of a table';

  @override
  String get snippetDbSqliteColumnsNote =>
      'Name, declared type, NOT NULL, default, primary key position and hidden (generated) flag.';

  @override
  String get snippetDbSqliteIndexes => 'Indexes of a table';

  @override
  String get snippetDbSqliteIndexesNote =>
      'Each index with uniqueness, origin (c = CREATE INDEX, u = UNIQUE, pk) and whether it is partial.';

  @override
  String get snippetDbSqliteIndexColumns => 'Columns of an index';

  @override
  String get snippetDbSqliteIndexColumnsNote => 'Key columns of an index in order, with sort direction and collation.';

  @override
  String get snippetDbSqliteForeignKeys => 'Foreign keys of a table';

  @override
  String get snippetDbSqliteForeignKeysNote => 'Referenced table and columns with ON UPDATE / ON DELETE actions.';

  @override
  String get snippetDbSqliteFkCheck => 'Rows breaking foreign keys';

  @override
  String get snippetDbSqliteFkCheckNote =>
      'Rows whose foreign keys point nowhere (checked even when enforcement is off).';

  @override
  String get snippetDbSqliteQuickCheck => 'Quick integrity check';

  @override
  String get snippetDbSqliteQuickCheckNote => 'Structure check of the whole file; returns ok when nothing is wrong.';

  @override
  String get snippetDbSqliteSize => 'File size and free pages';

  @override
  String get snippetDbSqliteSizeNote => 'Pages in use, page size, the size in bytes and pages VACUUM could free.';

  @override
  String get snippetDbSqliteDefinition => 'CREATE statement of an object';

  @override
  String get snippetDbSqliteDefinitionNote =>
      'The SQL that created a table, view, index or trigger, as stored in the file.';

  @override
  String get snippetDbSqliteCount => 'Rows in a table';

  @override
  String get snippetDbSqliteCountNote => 'Exact row count (scans the table).';

  @override
  String get snippetDbSqlitePlan => 'Query plan';

  @override
  String get snippetDbSqlitePlanNote => 'How SQLite will run the query: indexes used, scans and temporary B-trees.';

  @override
  String get snippetDbSqliteVersion => 'SQLite version';

  @override
  String get snippetDbSqliteVersionNote => 'Library version and source id of the program that opened the file.';

  @override
  String get snippetDbSqliteCompileOptions => 'Compile options';

  @override
  String get snippetDbSqliteCompileOptionsNote =>
      'Options the SQLite library was built with (JSON, FTS5, maximum sizes).';

  @override
  String get snippetDbSqliteAnalyze => 'Refresh planner statistics';

  @override
  String get snippetDbSqliteAnalyzeNote =>
      'Collects index statistics into sqlite_stat1 so the planner picks better indexes.';

  @override
  String get snippetDbSqliteVacuum => 'Compact the file';

  @override
  String get snippetDbSqliteVacuumNote =>
      'Rebuilds the file to return free pages to the disk; needs exclusive access and free space about the file\'s size.';
}
