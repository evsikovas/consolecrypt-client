import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:intl/intl.dart' as intl;

import 'app_localizations_en.dart';
import 'app_localizations_ru.dart';

// ignore_for_file: type=lint

/// Callers can lookup localized strings with an instance of AppLocalizations
/// returned by `AppLocalizations.of(context)`.
///
/// Applications need to include `AppLocalizations.delegate()` in their app's
/// `localizationDelegates` list, and the locales they support in the app's
/// `supportedLocales` list. For example:
///
/// ```dart
/// import 'l10n/app_localizations.dart';
///
/// return MaterialApp(
///   localizationsDelegates: AppLocalizations.localizationsDelegates,
///   supportedLocales: AppLocalizations.supportedLocales,
///   home: MyApplicationHome(),
/// );
/// ```
///
/// ## Update pubspec.yaml
///
/// Please make sure to update your pubspec.yaml to include the following
/// packages:
///
/// ```yaml
/// dependencies:
///   # Internationalization support.
///   flutter_localizations:
///     sdk: flutter
///   intl: any # Use the pinned version from flutter_localizations
///
///   # Rest of dependencies
/// ```
///
/// ## iOS Applications
///
/// iOS applications define key application metadata, including supported
/// locales, in an Info.plist file that is built into the application bundle.
/// To configure the locales supported by your app, you’ll need to edit this
/// file.
///
/// First, open your project’s ios/Runner.xcworkspace Xcode workspace file.
/// Then, in the Project Navigator, open the Info.plist file under the Runner
/// project’s Runner folder.
///
/// Next, select the Information Property List item, select Add Item from the
/// Editor menu, then select Localizations from the pop-up menu.
///
/// Select and expand the newly-created Localizations item then, for each
/// locale your application supports, add a new item and select the locale
/// you wish to add from the pop-up menu in the Value field. This list should
/// be consistent with the languages listed in the AppLocalizations.supportedLocales
/// property.
abstract class AppLocalizations {
  AppLocalizations(String locale) : localeName = intl.Intl.canonicalizedLocale(locale.toString());

  final String localeName;

  static AppLocalizations of(BuildContext context) {
    return Localizations.of<AppLocalizations>(context, AppLocalizations)!;
  }

  static const LocalizationsDelegate<AppLocalizations> delegate = _AppLocalizationsDelegate();

  /// A list of this localizations delegate along with the default localizations
  /// delegates.
  ///
  /// Returns a list of localizations delegates containing this delegate along with
  /// GlobalMaterialLocalizations.delegate, GlobalCupertinoLocalizations.delegate,
  /// and GlobalWidgetsLocalizations.delegate.
  ///
  /// Additional delegates can be added by appending to this list in
  /// MaterialApp. This list does not have to be used at all if a custom list
  /// of delegates is preferred or required.
  static const List<LocalizationsDelegate<dynamic>> localizationsDelegates = <LocalizationsDelegate<dynamic>>[
    delegate,
    GlobalMaterialLocalizations.delegate,
    GlobalCupertinoLocalizations.delegate,
    GlobalWidgetsLocalizations.delegate,
  ];

  /// A list of this localizations delegate's supported locales.
  static const List<Locale> supportedLocales = <Locale>[Locale('en'), Locale('ru')];

  /// Button: dismiss a dialog without changes.
  ///
  /// In en, this message translates to:
  /// **'Cancel'**
  String get commonCancel;

  /// Button: save changes.
  ///
  /// In en, this message translates to:
  /// **'Save'**
  String get commonSave;

  /// Button/menu: delete an item.
  ///
  /// In en, this message translates to:
  /// **'Delete'**
  String get commonDelete;

  /// Button/menu: edit an item.
  ///
  /// In en, this message translates to:
  /// **'Edit'**
  String get commonEdit;

  /// Button/menu: rename an item.
  ///
  /// In en, this message translates to:
  /// **'Rename'**
  String get commonRename;

  /// Button: close a dialog or panel.
  ///
  /// In en, this message translates to:
  /// **'Close'**
  String get commonClose;

  /// Button/tooltip: copy to the clipboard.
  ///
  /// In en, this message translates to:
  /// **'Copy'**
  String get commonCopy;

  /// Button: go back to the previous screen.
  ///
  /// In en, this message translates to:
  /// **'Back'**
  String get commonBack;

  /// Button: go to the next step of a wizard.
  ///
  /// In en, this message translates to:
  /// **'Next'**
  String get commonNext;

  /// Button: continue a flow.
  ///
  /// In en, this message translates to:
  /// **'Continue'**
  String get commonContinue;

  /// Button: finish a flow.
  ///
  /// In en, this message translates to:
  /// **'Done'**
  String get commonDone;

  /// Button: retry a failed action.
  ///
  /// In en, this message translates to:
  /// **'Retry'**
  String get commonRetry;

  /// Button: add an item.
  ///
  /// In en, this message translates to:
  /// **'Add'**
  String get commonAdd;

  /// Button: create an item.
  ///
  /// In en, this message translates to:
  /// **'Create'**
  String get commonCreate;

  /// Button: open an item.
  ///
  /// In en, this message translates to:
  /// **'Open'**
  String get commonOpen;

  /// Button: remove an item from a list or setting.
  ///
  /// In en, this message translates to:
  /// **'Remove'**
  String get commonRemove;

  /// Button: change a stored value.
  ///
  /// In en, this message translates to:
  /// **'Change'**
  String get commonChange;

  /// Tooltip/button: reveal a hidden value.
  ///
  /// In en, this message translates to:
  /// **'Show'**
  String get commonShow;

  /// Tooltip/button: hide a revealed value.
  ///
  /// In en, this message translates to:
  /// **'Hide'**
  String get commonHide;

  /// Search field label.
  ///
  /// In en, this message translates to:
  /// **'Search'**
  String get commonSearch;

  /// Button: acknowledge.
  ///
  /// In en, this message translates to:
  /// **'OK'**
  String get commonOk;

  /// Button: open the item that needs attention.
  ///
  /// In en, this message translates to:
  /// **'Review'**
  String get commonReview;

  /// Button: open an SSH connection.
  ///
  /// In en, this message translates to:
  /// **'Connect'**
  String get commonConnect;

  /// Value: something has never happened / never happens.
  ///
  /// In en, this message translates to:
  /// **'Never'**
  String get commonNever;

  /// Value: nothing selected.
  ///
  /// In en, this message translates to:
  /// **'None'**
  String get commonNone;

  /// Value: unknown.
  ///
  /// In en, this message translates to:
  /// **'Unknown'**
  String get commonUnknown;

  /// Field label: name of an item.
  ///
  /// In en, this message translates to:
  /// **'Name'**
  String get commonName;

  /// Field label: description.
  ///
  /// In en, this message translates to:
  /// **'Description'**
  String get commonDescription;

  /// Helper text: the field may be left empty.
  ///
  /// In en, this message translates to:
  /// **'Optional'**
  String get commonOptional;

  /// Tag editor field label.
  ///
  /// In en, this message translates to:
  /// **'Tags'**
  String get tagsLabel;

  /// Tag editor hint.
  ///
  /// In en, this message translates to:
  /// **'Type a tag and press Enter'**
  String get tagsHint;

  /// Tooltip of the add-tag button.
  ///
  /// In en, this message translates to:
  /// **'Add tag'**
  String get tagsAdd;

  /// Snackbar after copying non-secret text. {what} is a copyWhat* noun.
  ///
  /// In en, this message translates to:
  /// **'{what} copied.'**
  String copiedNotice(String what);

  /// Snackbar after copying a secret; the clipboard is auto-cleared. {what} is a copyWhat* noun.
  ///
  /// In en, this message translates to:
  /// **'{what} copied. The clipboard will be cleared in {seconds} s.'**
  String copiedSecretNotice(String what, int seconds);

  /// Noun inserted into copiedSecretNotice (RU lowercase, after a colon).
  ///
  /// In en, this message translates to:
  /// **'Secret'**
  String get copyWhatSecret;

  /// Noun inserted into copiedNotice (RU lowercase).
  ///
  /// In en, this message translates to:
  /// **'Text'**
  String get copyWhatText;

  /// Noun inserted into copiedNotice (RU lowercase).
  ///
  /// In en, this message translates to:
  /// **'Command'**
  String get copyWhatCommand;

  /// Noun inserted into copiedNotice (RU lowercase).
  ///
  /// In en, this message translates to:
  /// **'Public key'**
  String get copyWhatPublicKey;

  /// Noun inserted into copiedSecretNotice (RU lowercase).
  ///
  /// In en, this message translates to:
  /// **'Password'**
  String get copyWhatPassword;

  /// Noun inserted into copiedSecretNotice (RU lowercase).
  ///
  /// In en, this message translates to:
  /// **'Recovery words'**
  String get copyWhatRecoveryWords;

  /// Noun inserted into copiedNotice (RU lowercase).
  ///
  /// In en, this message translates to:
  /// **'Fingerprint'**
  String get copyWhatFingerprint;

  /// Relative time: less than a minute ago.
  ///
  /// In en, this message translates to:
  /// **'just now'**
  String get timeJustNow;

  /// Relative time in minutes (abbreviated).
  ///
  /// In en, this message translates to:
  /// **'{count, plural, other{{count} min ago}}'**
  String timeMinutesAgo(int count);

  /// Relative time in hours (abbreviated).
  ///
  /// In en, this message translates to:
  /// **'{count, plural, other{{count} h ago}}'**
  String timeHoursAgo(int count);

  /// Relative time in days.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, other{{count} d ago}}'**
  String timeDaysAgo(int count);

  /// Remaining time when the deadline has passed.
  ///
  /// In en, this message translates to:
  /// **'expired'**
  String get timeExpired;

  /// Remaining time below one minute.
  ///
  /// In en, this message translates to:
  /// **'in <1 min'**
  String get timeInLessThanMinute;

  /// Remaining time in minutes.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, other{in {count} min}}'**
  String timeInMinutes(int count);

  /// Remaining time in hours.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, other{in {count} h}}'**
  String timeInHours(int count);

  /// Remaining time in days.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, other{in {count} d}}'**
  String timeInDays(int count);

  /// Unit: bytes.
  ///
  /// In en, this message translates to:
  /// **'B'**
  String get unitBytes;

  /// Unit: kibibytes (1024 B).
  ///
  /// In en, this message translates to:
  /// **'KiB'**
  String get unitKibibytes;

  /// Unit: mebibytes.
  ///
  /// In en, this message translates to:
  /// **'MiB'**
  String get unitMebibytes;

  /// Unit: gibibytes.
  ///
  /// In en, this message translates to:
  /// **'GiB'**
  String get unitGibibytes;

  /// Unit: tebibytes.
  ///
  /// In en, this message translates to:
  /// **'TiB'**
  String get unitTebibytes;

  /// A formatted size: number + unit (e.g. 1.4 KiB).
  ///
  /// In en, this message translates to:
  /// **'{value} {unit}'**
  String formatSize(String value, String unit);

  /// Label of the language selector.
  ///
  /// In en, this message translates to:
  /// **'Language'**
  String get languageLabel;

  /// Language option: follow the OS language.
  ///
  /// In en, this message translates to:
  /// **'System'**
  String get languageSystem;

  /// Language option showing the language currently resolved from the OS.
  ///
  /// In en, this message translates to:
  /// **'System ({language})'**
  String languageSystemResolved(String language);

  /// Tooltip of the language switcher on the welcome screen (bilingual on purpose).
  ///
  /// In en, this message translates to:
  /// **'Language / Язык'**
  String get languageSwitcherTooltip;

  /// Help text under the language selector in Settings.
  ///
  /// In en, this message translates to:
  /// **'Applies immediately. \"System\" follows the OS language and falls back to English.'**
  String get languageSettingsHelp;

  /// Command risk level: does not change anything.
  ///
  /// In en, this message translates to:
  /// **'Read-only'**
  String get riskReadOnly;

  /// Command risk level: changes state (adjective for "команда").
  ///
  /// In en, this message translates to:
  /// **'Modifying'**
  String get riskModifying;

  /// Command risk level: may destroy data (adjective for "команда").
  ///
  /// In en, this message translates to:
  /// **'Destructive'**
  String get riskDestructive;

  /// Command risk level: not recognised by local rules.
  ///
  /// In en, this message translates to:
  /// **'Unknown'**
  String get riskUnknown;

  /// Screen-reader label of the risk badge.
  ///
  /// In en, this message translates to:
  /// **'Risk: {level}'**
  String riskSemantics(String level);

  /// Screen-reader label of the 6-group device verification code.
  ///
  /// In en, this message translates to:
  /// **'Verification code {groups}'**
  String verificationCodeSemantics(String groups);

  /// Passphrase strength (adjective for "парольная фраза").
  ///
  /// In en, this message translates to:
  /// **'Very weak'**
  String get strengthVeryWeak;

  /// Passphrase strength.
  ///
  /// In en, this message translates to:
  /// **'Weak'**
  String get strengthWeak;

  /// Passphrase strength.
  ///
  /// In en, this message translates to:
  /// **'Fair'**
  String get strengthFair;

  /// Passphrase strength.
  ///
  /// In en, this message translates to:
  /// **'Strong'**
  String get strengthStrong;

  /// Passphrase strength.
  ///
  /// In en, this message translates to:
  /// **'Very strong'**
  String get strengthVeryStrong;

  /// Strength meter caption: level + suggestion.
  ///
  /// In en, this message translates to:
  /// **'{label} — {hint}'**
  String strengthLabelWithHint(String label, String hint);

  /// Passphrase hint when empty.
  ///
  /// In en, this message translates to:
  /// **'Use at least 4 random words or {min}+ characters'**
  String strengthHintEmpty(int min);

  /// Passphrase hint.
  ///
  /// In en, this message translates to:
  /// **'Avoid keyboard or alphabet sequences'**
  String get strengthHintSequence;

  /// Passphrase hint.
  ///
  /// In en, this message translates to:
  /// **'Avoid common passwords and well-known phrases'**
  String get strengthHintCommon;

  /// Passphrase hint.
  ///
  /// In en, this message translates to:
  /// **'Use at least {min} characters'**
  String strengthHintTooShort(int min);

  /// Passphrase hint for an acceptable passphrase.
  ///
  /// In en, this message translates to:
  /// **'Good. Store it somewhere safe — nobody can reset it for you.'**
  String get strengthHintGood;

  /// Passphrase hint.
  ///
  /// In en, this message translates to:
  /// **'Add more random words or characters'**
  String get strengthHintAddMore;

  /// Automatic backup frequency.
  ///
  /// In en, this message translates to:
  /// **'Daily'**
  String get backupFrequencyDaily;

  /// Automatic backup frequency.
  ///
  /// In en, this message translates to:
  /// **'Weekly'**
  String get backupFrequencyWeekly;

  /// AI provider type for any OpenAI-compatible API.
  ///
  /// In en, this message translates to:
  /// **'OpenAI-compatible'**
  String get aiProviderKindOpenaiCompatible;

  /// AI privacy profile.
  ///
  /// In en, this message translates to:
  /// **'Strict'**
  String get privacyProfileStrict;

  /// AI privacy profile.
  ///
  /// In en, this message translates to:
  /// **'Standard'**
  String get privacyProfileStandard;

  /// AI privacy profile for local models.
  ///
  /// In en, this message translates to:
  /// **'Local'**
  String get privacyProfileLocal;

  /// AI privacy profile description.
  ///
  /// In en, this message translates to:
  /// **'Redact secrets and also IPs, hostnames, usernames, DB names'**
  String get privacyProfileStrictDescription;

  /// AI privacy profile description.
  ///
  /// In en, this message translates to:
  /// **'Redact secrets, keep host metadata'**
  String get privacyProfileStandardDescription;

  /// AI privacy profile description.
  ///
  /// In en, this message translates to:
  /// **'For local models: more context; secrets still redacted'**
  String get privacyProfileLocalDescription;

  /// Credential type.
  ///
  /// In en, this message translates to:
  /// **'Password'**
  String get credentialKindPassword;

  /// Credential type.
  ///
  /// In en, this message translates to:
  /// **'SSH key'**
  String get credentialKindSshKey;

  /// Credential type: private key + OpenSSH certificate.
  ///
  /// In en, this message translates to:
  /// **'SSH certificate'**
  String get credentialKindSshCertificate;

  /// Credential type: keys from the operating system SSH agent.
  ///
  /// In en, this message translates to:
  /// **'OS SSH agent'**
  String get credentialKindOsSshAgent;

  /// Credential type: hardware security key.
  ///
  /// In en, this message translates to:
  /// **'FIDO2 security key'**
  String get credentialKindFido2;

  /// Credential type: third-party SSH agent socket (1Password, Secretive…).
  ///
  /// In en, this message translates to:
  /// **'External agent'**
  String get credentialKindExternalAgent;

  /// Host key policy.
  ///
  /// In en, this message translates to:
  /// **'Ask'**
  String get hostKeyPolicyAsk;

  /// Host key policy (adjective for "политика").
  ///
  /// In en, this message translates to:
  /// **'Strict'**
  String get hostKeyPolicyStrict;

  /// Host key policy.
  ///
  /// In en, this message translates to:
  /// **'Accept new'**
  String get hostKeyPolicyAcceptNew;

  /// Host key policy description.
  ///
  /// In en, this message translates to:
  /// **'Confirm unknown host keys; changed keys always fail'**
  String get hostKeyPolicyAskDescription;

  /// Host key policy description.
  ///
  /// In en, this message translates to:
  /// **'Only accept keys already in known hosts'**
  String get hostKeyPolicyStrictDescription;

  /// Host key policy description.
  ///
  /// In en, this message translates to:
  /// **'Record unknown keys automatically; changed keys always fail'**
  String get hostKeyPolicyAcceptNewDescription;

  /// SSH implementation choice.
  ///
  /// In en, this message translates to:
  /// **'Built-in (russh)'**
  String get sshBackendNative;

  /// SSH implementation choice.
  ///
  /// In en, this message translates to:
  /// **'System OpenSSH (compatibility)'**
  String get sshBackendOpenSsh;

  /// Origin of a known host key.
  ///
  /// In en, this message translates to:
  /// **'Accepted on first connect'**
  String get knownHostSourceTofu;

  /// Origin of a known host key.
  ///
  /// In en, this message translates to:
  /// **'Added manually'**
  String get knownHostSourceManual;

  /// Origin of a known host key.
  ///
  /// In en, this message translates to:
  /// **'Imported from known_hosts'**
  String get knownHostSourceImported;

  /// Origin of a known host key.
  ///
  /// In en, this message translates to:
  /// **'Certificate authority'**
  String get knownHostSourceCertAuthority;

  /// Terminal history mode.
  ///
  /// In en, this message translates to:
  /// **'Local only'**
  String get historyModeLocalOnly;

  /// Terminal history mode.
  ///
  /// In en, this message translates to:
  /// **'Encrypted sync'**
  String get historyModeEncryptedSync;

  /// Terminal history mode (adjective for "история").
  ///
  /// In en, this message translates to:
  /// **'Disabled'**
  String get historyModeDisabled;

  /// Terminal history mode description.
  ///
  /// In en, this message translates to:
  /// **'History stays on this device (default)'**
  String get historyModeLocalOnlyDescription;

  /// Terminal history mode description.
  ///
  /// In en, this message translates to:
  /// **'History is synced end-to-end encrypted'**
  String get historyModeEncryptedSyncDescription;

  /// Terminal history mode description.
  ///
  /// In en, this message translates to:
  /// **'No command history is recorded'**
  String get historyModeDisabledDescription;

  /// Profile kind / sync state: no server, no account.
  ///
  /// In en, this message translates to:
  /// **'Local only'**
  String get profileKindLocal;

  /// Profile kind: synced through a self-hosted server.
  ///
  /// In en, this message translates to:
  /// **'Synced'**
  String get profileKindSynced;

  /// Profile switcher subtitle of a synced profile: account e-mail and server host.
  ///
  /// In en, this message translates to:
  /// **'{account} · {server}'**
  String profileSyncedSubtitle(String account, String server);

  /// Placeholder when a synced profile has no e-mail yet.
  ///
  /// In en, this message translates to:
  /// **'account'**
  String get profileUnknownAccount;

  /// Placeholder when a synced profile has no server yet.
  ///
  /// In en, this message translates to:
  /// **'server'**
  String get profileUnknownServer;

  /// Enable-sync progress step.
  ///
  /// In en, this message translates to:
  /// **'Signing in and registering this device'**
  String get enableSyncStepAuthenticating;

  /// Enable-sync progress step.
  ///
  /// In en, this message translates to:
  /// **'Creating the vault on the server (same vault ID)'**
  String get enableSyncStepCreatingRemoteVault;

  /// Enable-sync progress step.
  ///
  /// In en, this message translates to:
  /// **'Vault already exists on the server — merging'**
  String get enableSyncStepReconnecting;

  /// Enable-sync progress step.
  ///
  /// In en, this message translates to:
  /// **'Uploading encrypted objects'**
  String get enableSyncStepUploading;

  /// Enable-sync progress step.
  ///
  /// In en, this message translates to:
  /// **'Finishing'**
  String get enableSyncStepFinishing;

  /// Enable-sync progress step.
  ///
  /// In en, this message translates to:
  /// **'Sync enabled'**
  String get enableSyncStepDone;

  /// Enable-sync progress step.
  ///
  /// In en, this message translates to:
  /// **'Failed'**
  String get enableSyncStepFailed;

  /// Tunnel type (adjective for "туннель").
  ///
  /// In en, this message translates to:
  /// **'Local'**
  String get tunnelKindLocal;

  /// Tunnel type.
  ///
  /// In en, this message translates to:
  /// **'Remote'**
  String get tunnelKindRemote;

  /// Tunnel type.
  ///
  /// In en, this message translates to:
  /// **'Dynamic (SOCKS5)'**
  String get tunnelKindDynamic;

  /// Tunnel type description.
  ///
  /// In en, this message translates to:
  /// **'Forward a local port to a host reachable from the server'**
  String get tunnelKindLocalDescription;

  /// Tunnel type description.
  ///
  /// In en, this message translates to:
  /// **'Expose a local service on a port of the server'**
  String get tunnelKindRemoteDescription;

  /// Tunnel type description.
  ///
  /// In en, this message translates to:
  /// **'Local SOCKS5 proxy through the server'**
  String get tunnelKindDynamicDescription;

  /// Where a snippet came from.
  ///
  /// In en, this message translates to:
  /// **'User'**
  String get snippetSourceUser;

  /// Where a snippet came from.
  ///
  /// In en, this message translates to:
  /// **'AI'**
  String get snippetSourceAi;

  /// Where a snippet came from.
  ///
  /// In en, this message translates to:
  /// **'Imported'**
  String get snippetSourceImported;

  /// Where a snippet came from.
  ///
  /// In en, this message translates to:
  /// **'History'**
  String get snippetSourceHistory;

  /// Name of OS authentication on platforms without Touch ID / Windows Hello.
  ///
  /// In en, this message translates to:
  /// **'system authentication'**
  String get platformSystemAuthentication;

  /// Suggested device name on macOS.
  ///
  /// In en, this message translates to:
  /// **'My Mac'**
  String get platformDeviceNameMac;

  /// Suggested device name on Windows.
  ///
  /// In en, this message translates to:
  /// **'My PC'**
  String get platformDeviceNamePc;

  /// Suggested device name on other platforms.
  ///
  /// In en, this message translates to:
  /// **'This device'**
  String get platformDeviceNameOther;

  /// App command (menu + palette).
  ///
  /// In en, this message translates to:
  /// **'Command Palette'**
  String get commandPalette;

  /// App command.
  ///
  /// In en, this message translates to:
  /// **'New Terminal Tab'**
  String get commandNewTerminalTab;

  /// App command.
  ///
  /// In en, this message translates to:
  /// **'Close Terminal Tab'**
  String get commandCloseTerminalTab;

  /// App command.
  ///
  /// In en, this message translates to:
  /// **'New Host'**
  String get commandNewHost;

  /// App command.
  ///
  /// In en, this message translates to:
  /// **'Lock Vault'**
  String get commandLockVault;

  /// App command: open Settings.
  ///
  /// In en, this message translates to:
  /// **'Settings'**
  String get commandOpenSettings;

  /// Title of the host picker opened by the new-tab shortcut.
  ///
  /// In en, this message translates to:
  /// **'New terminal tab'**
  String get commandNewTabPickerTitle;

  /// macOS menu bar menu.
  ///
  /// In en, this message translates to:
  /// **'File'**
  String get menuFile;

  /// macOS menu bar menu.
  ///
  /// In en, this message translates to:
  /// **'View'**
  String get menuView;

  /// macOS menu bar menu.
  ///
  /// In en, this message translates to:
  /// **'Window'**
  String get menuWindow;

  /// Sidebar item.
  ///
  /// In en, this message translates to:
  /// **'Hosts'**
  String get navHosts;

  /// Sidebar item.
  ///
  /// In en, this message translates to:
  /// **'Groups'**
  String get navGroups;

  /// Sidebar item.
  ///
  /// In en, this message translates to:
  /// **'Credentials'**
  String get navCredentials;

  /// Sidebar item.
  ///
  /// In en, this message translates to:
  /// **'Terminal'**
  String get navTerminal;

  /// Sidebar item (protocol name, not translated).
  ///
  /// In en, this message translates to:
  /// **'SFTP'**
  String get navSftp;

  /// Sidebar item.
  ///
  /// In en, this message translates to:
  /// **'Tunnels'**
  String get navTunnels;

  /// Sidebar item.
  ///
  /// In en, this message translates to:
  /// **'Snippets'**
  String get navSnippets;

  /// Sidebar item.
  ///
  /// In en, this message translates to:
  /// **'AI Chat'**
  String get navAiChat;

  /// Sidebar item.
  ///
  /// In en, this message translates to:
  /// **'Devices'**
  String get navDevices;

  /// Sidebar item.
  ///
  /// In en, this message translates to:
  /// **'Sync'**
  String get navSync;

  /// Sidebar item.
  ///
  /// In en, this message translates to:
  /// **'Backups'**
  String get navBackups;

  /// Sidebar item.
  ///
  /// In en, this message translates to:
  /// **'Settings'**
  String get navSettings;

  /// Tooltip of the sidebar lock button.
  ///
  /// In en, this message translates to:
  /// **'Lock vault ({shortcut})'**
  String shellLockVaultTooltip(String shortcut);

  /// Top bar button that opens the command palette.
  ///
  /// In en, this message translates to:
  /// **'Search snippets or ask AI…'**
  String get shellSearchPlaceholder;

  /// Tooltip of the Offline chip.
  ///
  /// In en, this message translates to:
  /// **'The sync server is unreachable. Hosts, keys and SSH keep working; changes are queued.'**
  String get shellOfflineTooltip;

  /// Top bar button tooltip.
  ///
  /// In en, this message translates to:
  /// **'New terminal tab ({shortcut})'**
  String shellNewTabTooltip(String shortcut);

  /// Banner title for pending device approvals.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, =1{Device approval requested} other{{count} device approval requests}}'**
  String shellApprovalBannerTitle(int count);

  /// Banner text; {ago} is a relative time such as "5 min ago".
  ///
  /// In en, this message translates to:
  /// **'\"{device}\" ({platform}) asked to join your vault {ago}. Approve only after comparing verification codes.'**
  String shellApprovalBannerMessage(String device, String platform, String ago);

  /// Sync indicator: local profile.
  ///
  /// In en, this message translates to:
  /// **'Local only'**
  String get syncStateLocalOnly;

  /// Sync indicator.
  ///
  /// In en, this message translates to:
  /// **'Synced'**
  String get syncStateSynced;

  /// Sync indicator; {ago} is a relative time.
  ///
  /// In en, this message translates to:
  /// **'Synced {ago}'**
  String syncStateSyncedAgo(String ago);

  /// Sync indicator.
  ///
  /// In en, this message translates to:
  /// **'Syncing…'**
  String get syncStateSyncing;

  /// Sync indicator / chip.
  ///
  /// In en, this message translates to:
  /// **'Offline'**
  String get syncStateOffline;

  /// Sync indicator with queued local changes.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, other{Offline · {count} pending}}'**
  String syncStateOfflinePending(int count);

  /// Sync indicator.
  ///
  /// In en, this message translates to:
  /// **'Sync error'**
  String get syncStateError;

  /// Sync indicator.
  ///
  /// In en, this message translates to:
  /// **'Sync paused'**
  String get syncStatePaused;

  /// Sync indicator tooltip for local profiles.
  ///
  /// In en, this message translates to:
  /// **'This profile is not synced. Enable sync in Settings.'**
  String get syncIndicatorLocalTooltip;

  /// Sync indicator tooltip with queued changes.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, =1{{state} — 1 local change waiting} other{{state} — {count} local changes waiting}}'**
  String syncIndicatorPendingTooltip(String state, int count);

  /// Fallback error text.
  ///
  /// In en, this message translates to:
  /// **'Something went wrong. Please try again.'**
  String get errorUnknown;

  /// Error: sign-in rejected.
  ///
  /// In en, this message translates to:
  /// **'Wrong e-mail or password.'**
  String get errorInvalidCredentials;

  /// Error.
  ///
  /// In en, this message translates to:
  /// **'Server unreachable. Check the URL and your connection.'**
  String get errorServerUnreachable;

  /// Error.
  ///
  /// In en, this message translates to:
  /// **'This server version is not compatible with this app. Update the app or the server.'**
  String get errorIncompatibleServer;

  /// Error.
  ///
  /// In en, this message translates to:
  /// **'Registration is closed on this server.'**
  String get errorRegistrationClosed;

  /// Error.
  ///
  /// In en, this message translates to:
  /// **'An account with this e-mail already exists.'**
  String get errorEmailTaken;

  /// Error.
  ///
  /// In en, this message translates to:
  /// **'Wrong passphrase.'**
  String get errorWrongPassphrase;

  /// Error.
  ///
  /// In en, this message translates to:
  /// **'This Recovery Key does not open the vault.'**
  String get errorInvalidRecoveryKey;

  /// Error.
  ///
  /// In en, this message translates to:
  /// **'This device is not trusted for the vault yet.'**
  String get errorDeviceNotTrusted;

  /// Error.
  ///
  /// In en, this message translates to:
  /// **'System authentication failed or was cancelled.'**
  String get errorOsAuthFailed;

  /// Error: device approval code mismatch (security-critical).
  ///
  /// In en, this message translates to:
  /// **'Verification codes do not match — approval refused.'**
  String get errorVerificationMismatch;

  /// Error.
  ///
  /// In en, this message translates to:
  /// **'The request has expired.'**
  String get errorRequestExpired;

  /// Generic validation error.
  ///
  /// In en, this message translates to:
  /// **'Check the entered values.'**
  String get errorValidation;

  /// Error.
  ///
  /// In en, this message translates to:
  /// **'Not found. It may have been deleted.'**
  String get errorNotFound;

  /// Error.
  ///
  /// In en, this message translates to:
  /// **'This was changed elsewhere. Reload and try again.'**
  String get errorConflict;

  /// Error.
  ///
  /// In en, this message translates to:
  /// **'You are offline. The change will be retried when the connection is back.'**
  String get errorOffline;

  /// Error: user cancelled.
  ///
  /// In en, this message translates to:
  /// **'Cancelled.'**
  String get errorCancelled;

  /// Error.
  ///
  /// In en, this message translates to:
  /// **'The host key was rejected.'**
  String get errorHostKeyRejected;

  /// Error (security-critical).
  ///
  /// In en, this message translates to:
  /// **'The host key has changed. The connection was blocked.'**
  String get errorHostKeyChanged;

  /// Error.
  ///
  /// In en, this message translates to:
  /// **'SSH authentication failed.'**
  String get errorAuthFailed;

  /// Error.
  ///
  /// In en, this message translates to:
  /// **'This action is not supported here.'**
  String get errorUnsupported;

  /// Error.
  ///
  /// In en, this message translates to:
  /// **'Internal error. Please try again.'**
  String get errorInternal;

  /// Error: server rejected the access/refresh token.
  ///
  /// In en, this message translates to:
  /// **'Your session has expired. Sign in again.'**
  String get errorSessionExpired;

  /// Error.
  ///
  /// In en, this message translates to:
  /// **'This device has been revoked. Sign in again to register it.'**
  String get errorDeviceRevoked;

  /// Error.
  ///
  /// In en, this message translates to:
  /// **'Verify your e-mail address first.'**
  String get errorEmailNotVerified;

  /// Error.
  ///
  /// In en, this message translates to:
  /// **'You do not have access to this.'**
  String get errorForbidden;

  /// Error.
  ///
  /// In en, this message translates to:
  /// **'Too many attempts. Try again later.'**
  String get errorRateLimited;

  /// Error with retry delay.
  ///
  /// In en, this message translates to:
  /// **'{seconds, plural, other{Too many attempts. Try again in {seconds} s.}}'**
  String errorRateLimitedRetry(int seconds);

  /// Error.
  ///
  /// In en, this message translates to:
  /// **'The data is too large for the server.'**
  String get errorPayloadTooLarge;

  /// Error.
  ///
  /// In en, this message translates to:
  /// **'Cryptographic check failed. The request was rejected.'**
  String get errorInvalidProof;

  /// Error.
  ///
  /// In en, this message translates to:
  /// **'The server is temporarily unavailable. Try again later.'**
  String get errorServerUnavailable;

  /// Error reason.
  ///
  /// In en, this message translates to:
  /// **'Enter a name.'**
  String get errorReasonNameRequired;

  /// Error reason.
  ///
  /// In en, this message translates to:
  /// **'Enter a profile name.'**
  String get errorReasonProfileNameRequired;

  /// Error reason.
  ///
  /// In en, this message translates to:
  /// **'Enter the password.'**
  String get errorReasonPasswordRequired;

  /// Error reason.
  ///
  /// In en, this message translates to:
  /// **'Enter the key passphrase.'**
  String get errorReasonKeyPassphraseRequired;

  /// Error reason.
  ///
  /// In en, this message translates to:
  /// **'Enter the agent socket path or pipe name.'**
  String get errorReasonAgentPathRequired;

  /// Error reason.
  ///
  /// In en, this message translates to:
  /// **'Enter a valid e-mail address.'**
  String get errorReasonInvalidEmail;

  /// Error reason.
  ///
  /// In en, this message translates to:
  /// **'{min, plural, other{Account password must be at least {min} characters.}}'**
  String errorReasonAccountPasswordTooShort(int min);

  /// Error reason.
  ///
  /// In en, this message translates to:
  /// **'Choose a stronger passphrase.'**
  String get errorReasonWeakPassphrase;

  /// Error reason.
  ///
  /// In en, this message translates to:
  /// **'Enter a valid base URL.'**
  String get errorReasonInvalidBaseUrl;

  /// Error reason.
  ///
  /// In en, this message translates to:
  /// **'Enter the chat model name.'**
  String get errorReasonChatModelRequired;

  /// Error reason.
  ///
  /// In en, this message translates to:
  /// **'Enter the command template.'**
  String get errorReasonTemplateRequired;

  /// Error reason: snippet variables without values.
  ///
  /// In en, this message translates to:
  /// **'Fill in: {names}'**
  String errorReasonMissingVariables(String names);

  /// Error reason.
  ///
  /// In en, this message translates to:
  /// **'This is not a valid private key.'**
  String get errorReasonInvalidKey;

  /// Error reason.
  ///
  /// In en, this message translates to:
  /// **'Not an OpenSSH certificate line.'**
  String get errorReasonNotACertificate;

  /// Error reason.
  ///
  /// In en, this message translates to:
  /// **'This is not an agent credential.'**
  String get errorReasonNotAgentCredential;

  /// Error reason.
  ///
  /// In en, this message translates to:
  /// **'This key has no passphrase.'**
  String get errorReasonKeyHasNoPassphrase;

  /// Error reason.
  ///
  /// In en, this message translates to:
  /// **'Generate Ed25519 or RSA 3072/4096 keys.'**
  String get errorReasonUnsupportedKeyAlgorithm;

  /// Error reason.
  ///
  /// In en, this message translates to:
  /// **'A group cannot be inside itself.'**
  String get errorReasonGroupCycle;

  /// Error reason.
  ///
  /// In en, this message translates to:
  /// **'A jump profile needs at least one hop.'**
  String get errorReasonJumpChainEmpty;

  /// Error reason.
  ///
  /// In en, this message translates to:
  /// **'Host not found.'**
  String get errorReasonHostNotFound;

  /// Error reason.
  ///
  /// In en, this message translates to:
  /// **'The credential no longer exists.'**
  String get errorReasonCredentialNotFound;

  /// Error reason.
  ///
  /// In en, this message translates to:
  /// **'Profile not found.'**
  String get errorReasonProfileNotFound;

  /// Error reason.
  ///
  /// In en, this message translates to:
  /// **'Device not found.'**
  String get errorReasonDeviceNotFound;

  /// Error reason.
  ///
  /// In en, this message translates to:
  /// **'Tunnel not found.'**
  String get errorReasonTunnelNotFound;

  /// Error reason.
  ///
  /// In en, this message translates to:
  /// **'AI provider not found.'**
  String get errorReasonProviderNotFound;

  /// Error reason.
  ///
  /// In en, this message translates to:
  /// **'The request no longer exists.'**
  String get errorReasonRequestNotFound;

  /// Error reason.
  ///
  /// In en, this message translates to:
  /// **'No secret is stored for this credential.'**
  String get errorReasonSecretNotStored;

  /// Error reason.
  ///
  /// In en, this message translates to:
  /// **'The session is closed.'**
  String get errorReasonSessionClosed;

  /// Error reason.
  ///
  /// In en, this message translates to:
  /// **'No such directory: {path}'**
  String errorReasonDirectoryNotFound(String path);

  /// Error reason.
  ///
  /// In en, this message translates to:
  /// **'The directory is not empty.'**
  String get errorReasonDirectoryNotEmpty;

  /// Error reason.
  ///
  /// In en, this message translates to:
  /// **'\"{name}\" already exists.'**
  String errorReasonAlreadyExists(String name);

  /// Error reason.
  ///
  /// In en, this message translates to:
  /// **'Select a file.'**
  String get errorReasonSelectFile;

  /// Error reason.
  ///
  /// In en, this message translates to:
  /// **'The vault is locked.'**
  String get errorReasonVaultLocked;

  /// Error reason.
  ///
  /// In en, this message translates to:
  /// **'No active profile.'**
  String get errorReasonNoActiveProfile;

  /// Error reason.
  ///
  /// In en, this message translates to:
  /// **'This profile has no vault yet.'**
  String get errorReasonNoVault;

  /// Error reason.
  ///
  /// In en, this message translates to:
  /// **'A vault already exists.'**
  String get errorReasonVaultExists;

  /// Error reason.
  ///
  /// In en, this message translates to:
  /// **'Not a ConsoleCrypt backup (or the file is missing).'**
  String get errorReasonNotABackup;

  /// Error reason.
  ///
  /// In en, this message translates to:
  /// **'Backup files must end with .{extension}'**
  String errorReasonBackupExtension(String extension);

  /// Error reason.
  ///
  /// In en, this message translates to:
  /// **'Choose a backup folder first.'**
  String get errorReasonBackupFolderRequired;

  /// Error reason.
  ///
  /// In en, this message translates to:
  /// **'Keep at least one backup.'**
  String get errorReasonKeepAtLeastOne;

  /// Error reason.
  ///
  /// In en, this message translates to:
  /// **'This needs a synced profile.'**
  String get errorReasonSyncedProfileRequired;

  /// Error reason.
  ///
  /// In en, this message translates to:
  /// **'Local profiles have no account.'**
  String get errorReasonLocalProfileNoAccount;

  /// Error reason.
  ///
  /// In en, this message translates to:
  /// **'No device is registered.'**
  String get errorReasonNoDeviceRegistered;

  /// Error reason.
  ///
  /// In en, this message translates to:
  /// **'Only an unlocked trusted device can do this.'**
  String get errorReasonTrustedDeviceRequired;

  /// Error reason.
  ///
  /// In en, this message translates to:
  /// **'The current passphrase is wrong.'**
  String get errorReasonCurrentPassphraseWrong;

  /// Error reason: changing the account password.
  ///
  /// In en, this message translates to:
  /// **'The current password is wrong.'**
  String get errorReasonCurrentPasswordWrong;

  /// Error reason: signing in to a synced profile with another account.
  ///
  /// In en, this message translates to:
  /// **'This profile belongs to a different account.'**
  String get errorReasonAccountMismatch;

  /// Field name used in validation messages.
  ///
  /// In en, this message translates to:
  /// **'Name'**
  String get validationFieldName;

  /// Field name used in validation messages.
  ///
  /// In en, this message translates to:
  /// **'Address'**
  String get validationFieldAddress;

  /// Field name used in validation messages.
  ///
  /// In en, this message translates to:
  /// **'Port'**
  String get validationFieldPort;

  /// Field name used in validation messages (tunnel listen address).
  ///
  /// In en, this message translates to:
  /// **'Bind address'**
  String get validationFieldBindHost;

  /// Field name used in validation messages.
  ///
  /// In en, this message translates to:
  /// **'Bind port'**
  String get validationFieldBindPort;

  /// Field name used in validation messages.
  ///
  /// In en, this message translates to:
  /// **'Target host'**
  String get validationFieldTargetHost;

  /// Field name used in validation messages.
  ///
  /// In en, this message translates to:
  /// **'Target port'**
  String get validationFieldTargetPort;

  /// Validation message.
  ///
  /// In en, this message translates to:
  /// **'{field}: required'**
  String validationRequired(String field);

  /// Validation message.
  ///
  /// In en, this message translates to:
  /// **'{field}: must not contain spaces'**
  String validationNoWhitespace(String field);

  /// Validation message.
  ///
  /// In en, this message translates to:
  /// **'{field}: must be 1–65535'**
  String validationPortRange(String field);

  /// Validation message.
  ///
  /// In en, this message translates to:
  /// **'{field}: invalid value'**
  String validationInvalid(String field);

  /// Validation message.
  ///
  /// In en, this message translates to:
  /// **'A host cannot jump through itself.'**
  String get validationJumpChainSelf;

  /// Validation error: the server URL field is empty (login, enable sync).
  ///
  /// In en, this message translates to:
  /// **'Enter the URL of your ConsoleCrypt server'**
  String get serverUrlEmpty;

  /// Validation error: the server URL is not a full URL with scheme and host.
  ///
  /// In en, this message translates to:
  /// **'Enter a full URL, e.g. https://sync.example.org'**
  String get serverUrlNotAUrl;

  /// Validation error: the server URL is not https and not a loopback http URL.
  ///
  /// In en, this message translates to:
  /// **'Use https:// (plain http is only allowed for localhost)'**
  String get serverUrlInsecure;

  /// Login screen title when connecting a new synced profile.
  ///
  /// In en, this message translates to:
  /// **'Connect to your server'**
  String get loginTitle;

  /// Login screen title when the session of the active synced profile ended.
  ///
  /// In en, this message translates to:
  /// **'Sign in again'**
  String get loginTitleReauth;

  /// Login screen subtitle when connecting a new synced profile.
  ///
  /// In en, this message translates to:
  /// **'Use the URL of your self-hosted ConsoleCrypt server.'**
  String get loginSubtitle;

  /// Login screen subtitle when re-authenticating the active profile.
  ///
  /// In en, this message translates to:
  /// **'Your session for this profile ended. Local data stays on this device.'**
  String get loginSubtitleReauth;

  /// Login screen: label of the server URL field.
  ///
  /// In en, this message translates to:
  /// **'Server URL'**
  String get loginServerUrlLabel;

  /// Login screen: button inside the server URL field that probes the server.
  ///
  /// In en, this message translates to:
  /// **'Check'**
  String get loginCheckServer;

  /// Login screen: result of a successful server check when registration is open.
  ///
  /// In en, this message translates to:
  /// **'ConsoleCrypt server {serverVersion} · protocol {protocolVersion} · registration open'**
  String loginServerInfoRegistrationOpen(String serverVersion, String protocolVersion);

  /// Login screen: result of a successful server check when registration is invite-only.
  ///
  /// In en, this message translates to:
  /// **'ConsoleCrypt server {serverVersion} · protocol {protocolVersion} · invite only'**
  String loginServerInfoInviteOnly(String serverVersion, String protocolVersion);

  /// Login screen: segmented button option to sign in to an existing account.
  ///
  /// In en, this message translates to:
  /// **'Sign in'**
  String get loginModeSignIn;

  /// Login screen: segmented button option to create a new account.
  ///
  /// In en, this message translates to:
  /// **'Create account'**
  String get loginModeRegister;

  /// Login screen: label of the e-mail field.
  ///
  /// In en, this message translates to:
  /// **'E-mail'**
  String get loginEmailLabel;

  /// Login screen: validation error when the e-mail is missing or invalid.
  ///
  /// In en, this message translates to:
  /// **'Enter your e-mail address'**
  String get loginEmailRequired;

  /// Login screen: label of the account password field.
  ///
  /// In en, this message translates to:
  /// **'Account password'**
  String get loginPasswordLabel;

  /// Login screen: helper under the password field when creating an account.
  ///
  /// In en, this message translates to:
  /// **'At least 10 characters. Different from your vault passphrase.'**
  String get loginPasswordHelperRegister;

  /// Login screen: validation error when the password is empty.
  ///
  /// In en, this message translates to:
  /// **'Enter your password'**
  String get loginPasswordRequired;

  /// Login screen: validation error when a new account password is shorter than 10 characters.
  ///
  /// In en, this message translates to:
  /// **'At least 10 characters'**
  String get loginPasswordTooShort;

  /// Login screen: label of the password confirmation field.
  ///
  /// In en, this message translates to:
  /// **'Repeat password'**
  String get loginPasswordRepeatLabel;

  /// Login screen: validation error when the confirmation differs from the password.
  ///
  /// In en, this message translates to:
  /// **'Passwords do not match'**
  String get loginPasswordsDoNotMatch;

  /// Login screen: label of the device name field.
  ///
  /// In en, this message translates to:
  /// **'Name of this device'**
  String get loginDeviceNameLabel;

  /// Login screen: helper under the device name field.
  ///
  /// In en, this message translates to:
  /// **'Shown in your device list, e.g. \"Work MacBook\".'**
  String get loginDeviceNameHelper;

  /// Login screen: validation error when the device name is empty.
  ///
  /// In en, this message translates to:
  /// **'Enter a device name'**
  String get loginDeviceNameRequired;

  /// Login screen: primary button to sign in.
  ///
  /// In en, this message translates to:
  /// **'Sign in'**
  String get loginSubmitSignIn;

  /// Login screen: primary button to create an account.
  ///
  /// In en, this message translates to:
  /// **'Create account'**
  String get loginSubmitRegister;

  /// Login screen: button that requests a password reset e-mail.
  ///
  /// In en, this message translates to:
  /// **'Forgot account password?'**
  String get loginForgotPassword;

  /// Login screen: error when requesting a password reset without a valid server URL and e-mail.
  ///
  /// In en, this message translates to:
  /// **'Enter the server URL and your e-mail first'**
  String get loginForgotNeedsServerAndEmail;

  /// Login screen: snackbar after requesting a password reset.
  ///
  /// In en, this message translates to:
  /// **'If the account exists, a reset e-mail is on its way. A reset restores account access only — your vault still needs your passphrase or Recovery Key.'**
  String get loginResetEmailSent;

  /// Login screen: footnote under the form.
  ///
  /// In en, this message translates to:
  /// **'Your vault passphrase never leaves this device; the server only stores encrypted data.'**
  String get loginPassphraseNeverLeaves;

  /// Sidebar profile switcher: tooltip.
  ///
  /// In en, this message translates to:
  /// **'Switch profile'**
  String get profileSwitcherTooltip;

  /// Sidebar profile switcher menu: add a profile.
  ///
  /// In en, this message translates to:
  /// **'Add profile…'**
  String get profileSwitcherAddProfileMenu;

  /// Sidebar profile switcher menu: open profile settings.
  ///
  /// In en, this message translates to:
  /// **'Manage profiles'**
  String get profileSwitcherManageProfiles;

  /// Gate screens: chip that switches to another profile. {name} is the profile name.
  ///
  /// In en, this message translates to:
  /// **'Switch to {name}'**
  String profileSwitcherSwitchTo(String name);

  /// Gate screens: chip that adds a profile.
  ///
  /// In en, this message translates to:
  /// **'Add profile'**
  String get profileSwitcherAddProfile;

  /// Backups screen title.
  ///
  /// In en, this message translates to:
  /// **'Backups'**
  String get backupsTitle;

  /// Backups screen subtitle.
  ///
  /// In en, this message translates to:
  /// **'Encrypted .ccbackup files contain only ciphertext and envelopes — restorable with your passphrase or Recovery Key on any machine.'**
  String get backupsSubtitle;

  /// Backups screen: button that opens the restore screen.
  ///
  /// In en, this message translates to:
  /// **'Restore…'**
  String get backupsRestoreAction;

  /// Backups screen: button that saves a backup file.
  ///
  /// In en, this message translates to:
  /// **'Export backup…'**
  String get backupsExportAction;

  /// Snackbar after exporting a backup. {fileName} is the file name, {size} a formatted size.
  ///
  /// In en, this message translates to:
  /// **'Backup saved: {fileName} ({size})'**
  String backupsSavedSnack(String fileName, String size);

  /// Backups screen: warning title for a local profile.
  ///
  /// In en, this message translates to:
  /// **'This profile has no cloud copy'**
  String get backupsNoCloudCopyTitle;

  /// Backups screen: warning text for a local profile.
  ///
  /// In en, this message translates to:
  /// **'Backups are the only way to get your data back if this device is lost.'**
  String get backupsNoCloudCopyMessage;

  /// Backups screen: info banner for a synced profile.
  ///
  /// In en, this message translates to:
  /// **'This profile is synced, so your server holds an encrypted copy. Offline backups still protect against server loss or accidental deletion.'**
  String get backupsSyncedMessage;

  /// Backups screen: section title with the list of recent backups.
  ///
  /// In en, this message translates to:
  /// **'Recent backups'**
  String get backupsRecentTitle;

  /// Backups screen: empty state of the recent backups list.
  ///
  /// In en, this message translates to:
  /// **'No backups made from this profile yet.'**
  String get backupsRecentEmpty;

  /// Backups screen: number of objects in a backup (part of a list row separated by ·).
  ///
  /// In en, this message translates to:
  /// **'{count, plural, =1{{count} object} other{{count} objects}}'**
  String backupsObjectCount(int count);

  /// Backups screen: marker in a list row for a backup made by the schedule (agrees with «резервная копия»).
  ///
  /// In en, this message translates to:
  /// **'automatic'**
  String get backupsAutomatic;

  /// Backups screen: section title of the backup schedule.
  ///
  /// In en, this message translates to:
  /// **'Automatic backups'**
  String get backupsAutoTitle;

  /// Backups screen: subtitle of the backup schedule section.
  ///
  /// In en, this message translates to:
  /// **'Writes an encrypted backup to a folder you choose (e.g. an external drive or a synced folder).'**
  String get backupsAutoSubtitle;

  /// Backups screen: label of the target folder row.
  ///
  /// In en, this message translates to:
  /// **'Folder'**
  String get backupsFolderLabel;

  /// Backups screen: value when no target folder is chosen (agrees with «папка»).
  ///
  /// In en, this message translates to:
  /// **'Not chosen'**
  String get backupsFolderNotChosen;

  /// Backups screen: button that picks the target folder.
  ///
  /// In en, this message translates to:
  /// **'Choose…'**
  String get backupsChooseFolder;

  /// Backups screen: label of the frequency selector.
  ///
  /// In en, this message translates to:
  /// **'Frequency'**
  String get backupsFrequencyLabel;

  /// Backups screen: label of the retention selector.
  ///
  /// In en, this message translates to:
  /// **'Keep'**
  String get backupsKeepLabel;

  /// Backups screen: retention option (how many most recent backups are kept).
  ///
  /// In en, this message translates to:
  /// **'{count, plural, =1{last {count} backup} other{last {count} backups}}'**
  String backupsKeepLast(int count);

  /// Backups screen: label of the last scheduled run time.
  ///
  /// In en, this message translates to:
  /// **'Last run'**
  String get backupsLastRunLabel;

  /// Backups screen: label of the next scheduled run time.
  ///
  /// In en, this message translates to:
  /// **'Next run'**
  String get backupsNextRunLabel;

  /// Backups screen: title of the error banner; the raw diagnostic from the scheduler is shown below it.
  ///
  /// In en, this message translates to:
  /// **'The last automatic backup failed'**
  String get backupsLastRunFailed;

  /// Snackbar after a manual scheduled backup. {folder} is a folder path.
  ///
  /// In en, this message translates to:
  /// **'Backup written to {folder}'**
  String backupsWrittenTo(String folder);

  /// Backups screen: button that runs the scheduled backup immediately.
  ///
  /// In en, this message translates to:
  /// **'Back up now'**
  String get backupsBackUpNow;

  /// Restore screen title.
  ///
  /// In en, this message translates to:
  /// **'Restore from backup'**
  String get restoreTitle;

  /// Restore screen subtitle.
  ///
  /// In en, this message translates to:
  /// **'Restores an encrypted .ccbackup file into a new local profile on this device. Existing profiles are not touched.'**
  String get restoreSubtitle;

  /// Restore screen: label of the backup file path field.
  ///
  /// In en, this message translates to:
  /// **'Backup file'**
  String get restoreFileLabel;

  /// Restore screen: button that opens a file picker.
  ///
  /// In en, this message translates to:
  /// **'Browse…'**
  String get restoreBrowse;

  /// Restore screen: label of the vault ID of the opened backup.
  ///
  /// In en, this message translates to:
  /// **'Vault ID'**
  String get restoreVaultIdLabel;

  /// Restore screen: label of the creation time of the opened backup.
  ///
  /// In en, this message translates to:
  /// **'Created'**
  String get restoreCreatedLabel;

  /// Restore screen: label of the object count of the opened backup.
  ///
  /// In en, this message translates to:
  /// **'Objects'**
  String get restoreObjectsLabel;

  /// Restore screen: object count and file size. {count} is a formatted number, {size} a formatted size.
  ///
  /// In en, this message translates to:
  /// **'{count} ({size})'**
  String restoreObjectsValue(String count, String size);

  /// Restore screen: label of the app version that wrote the backup.
  ///
  /// In en, this message translates to:
  /// **'Written by'**
  String get restoreWrittenByLabel;

  /// Restore screen: app version and backup format version that wrote the backup.
  ///
  /// In en, this message translates to:
  /// **'ConsoleCrypt {appVersion} · format v{formatVersion}'**
  String restoreWrittenByValue(String appVersion, int formatVersion);

  /// Restore screen: option to unlock the backup with the vault passphrase.
  ///
  /// In en, this message translates to:
  /// **'Vault passphrase'**
  String get restoreUnlockPassphrase;

  /// Restore screen: option to unlock the backup with the Recovery Key.
  ///
  /// In en, this message translates to:
  /// **'Recovery Key'**
  String get restoreUnlockRecoveryKey;

  /// Restore screen: label of the passphrase field.
  ///
  /// In en, this message translates to:
  /// **'Vault passphrase of the backup'**
  String get restorePassphraseLabel;

  /// Restore screen: label of the Recovery Key input (BIP-39 words or QR payload).
  ///
  /// In en, this message translates to:
  /// **'24 words or QR text'**
  String get restoreRecoveryInputLabel;

  /// Restore screen: label of the new passphrase field when restoring with the Recovery Key.
  ///
  /// In en, this message translates to:
  /// **'New vault passphrase'**
  String get restoreNewPassphraseLabel;

  /// Restore screen: label of the new profile name field.
  ///
  /// In en, this message translates to:
  /// **'Profile name'**
  String get restoreProfileNameLabel;

  /// Restore screen: hint of the new profile name field.
  ///
  /// In en, this message translates to:
  /// **'e.g. Personal (restored)'**
  String get restoreProfileNameHint;

  /// Restore screen: primary button before a backup file is opened.
  ///
  /// In en, this message translates to:
  /// **'Open backup'**
  String get restoreOpenBackup;

  /// Restore screen: primary button that restores the backup.
  ///
  /// In en, this message translates to:
  /// **'Restore into new profile'**
  String get restoreIntoNewProfile;

  /// Default name of the first local profile (editable).
  ///
  /// In en, this message translates to:
  /// **'Personal'**
  String get welcomeDefaultProfileName;

  /// First-launch screen title.
  ///
  /// In en, this message translates to:
  /// **'Welcome to ConsoleCrypt'**
  String get welcomeTitle;

  /// Welcome screen title when profiles already exist.
  ///
  /// In en, this message translates to:
  /// **'Add a profile'**
  String get welcomeTitleAddProfile;

  /// First-launch screen subtitle.
  ///
  /// In en, this message translates to:
  /// **'An end-to-end encrypted SSH client. Hosts, keys, passwords and snippets are encrypted on this device with a key only you control.'**
  String get welcomeSubtitle;

  /// Option card: local profile without a server.
  ///
  /// In en, this message translates to:
  /// **'Use locally (no account)'**
  String get welcomeLocalTitle;

  /// Option card body: local profile.
  ///
  /// In en, this message translates to:
  /// **'Everything stays encrypted on this machine. No server, no account, no network traffic for sync. You can enable sync later.'**
  String get welcomeLocalBody;

  /// Field label: name of the new local profile.
  ///
  /// In en, this message translates to:
  /// **'Profile name'**
  String get welcomeProfileNameLabel;

  /// Button: create a local profile.
  ///
  /// In en, this message translates to:
  /// **'Use locally'**
  String get welcomeLocalAction;

  /// Option card: synced profile via a self-hosted server.
  ///
  /// In en, this message translates to:
  /// **'Connect to a server'**
  String get welcomeServerTitle;

  /// Option card body: synced profile.
  ///
  /// In en, this message translates to:
  /// **'Sync end-to-end encrypted between your machines through your own self-hosted ConsoleCrypt server. The server never sees your data or keys.'**
  String get welcomeServerBody;

  /// Button: go to sign-in / registration.
  ///
  /// In en, this message translates to:
  /// **'Sign in or create account'**
  String get welcomeServerAction;

  /// Link: restore a profile from a .ccbackup file.
  ///
  /// In en, this message translates to:
  /// **'Restore from a backup file (.ccbackup)'**
  String get welcomeRestoreFromBackup;

  /// Hint under the options on the welcome screen.
  ///
  /// In en, this message translates to:
  /// **'Several profiles can coexist (e.g. a local personal vault and a synced work vault); switch between them from the sidebar.'**
  String get welcomeProfilesHint;

  /// Credentials screen title.
  ///
  /// In en, this message translates to:
  /// **'Credentials'**
  String get credentialsTitle;

  /// Credentials screen subtitle.
  ///
  /// In en, this message translates to:
  /// **'Passwords and private keys are stored as separate encrypted secrets and never shown unless you reveal them.'**
  String get credentialsSubtitle;

  /// Credentials screen: button that opens the creation menu; also the title of the 'choose a credential kind' dialog.
  ///
  /// In en, this message translates to:
  /// **'New credential'**
  String get credentialsNew;

  /// New credential menu / chooser item: store a password.
  ///
  /// In en, this message translates to:
  /// **'Password'**
  String get credentialsNewPassword;

  /// New credential menu / chooser item: generate a new SSH key (opens a dialog).
  ///
  /// In en, this message translates to:
  /// **'Generate SSH key…'**
  String get credentialsNewGenerateKey;

  /// New credential menu / chooser item: import an existing OpenSSH/PEM private key (opens a dialog).
  ///
  /// In en, this message translates to:
  /// **'Import OpenSSH key…'**
  String get credentialsNewImportKey;

  /// New credential menu / chooser item: import a private key together with an SSH certificate (opens a dialog).
  ///
  /// In en, this message translates to:
  /// **'SSH certificate…'**
  String get credentialsNewCertificate;

  /// New credential menu / chooser item: use the OS SSH agent or an external agent (opens a dialog).
  ///
  /// In en, this message translates to:
  /// **'SSH agent…'**
  String get credentialsNewAgent;

  /// Credentials screen empty state title.
  ///
  /// In en, this message translates to:
  /// **'No credentials yet'**
  String get credentialsEmptyTitle;

  /// Credentials screen empty state message.
  ///
  /// In en, this message translates to:
  /// **'Generate an Ed25519 key, import an existing OpenSSH key, or store a password.'**
  String get credentialsEmptyMessage;

  /// Credentials list row subtitle part (joined with ' · '): the SSH user name stored with the credential.
  ///
  /// In en, this message translates to:
  /// **'user {username}'**
  String credentialsListUser(String username);

  /// Credentials list row subtitle part (joined with ' · '): the key passphrase is stored in the vault.
  ///
  /// In en, this message translates to:
  /// **'passphrase remembered'**
  String get credentialsListPassphraseRemembered;

  /// Credentials list row subtitle part (joined with ' · '): the private key is encrypted with its own passphrase, which is asked when connecting.
  ///
  /// In en, this message translates to:
  /// **'passphrase-protected'**
  String get credentialsListPassphraseProtected;

  /// Credentials list row subtitle part (joined with ' · '): number of hosts and groups that use this credential.
  ///
  /// In en, this message translates to:
  /// **'used by {count}'**
  String credentialsListUsedBy(int count);

  /// Confirmation dialog title when deleting a credential.
  ///
  /// In en, this message translates to:
  /// **'Delete {name}?'**
  String credentialsDeleteTitle(String name);

  /// Confirmation dialog message when deleting a credential.
  ///
  /// In en, this message translates to:
  /// **'The credential and its secret are deleted from the vault. Hosts that use it will need another credential.'**
  String get credentialsDeleteMessage;

  /// Credential details dialog: label of the credential kind.
  ///
  /// In en, this message translates to:
  /// **'Type'**
  String get credentialsDetailsType;

  /// Credential details dialog: label of the SSH user name.
  ///
  /// In en, this message translates to:
  /// **'Username'**
  String get credentialsDetailsUsername;

  /// Credential details dialog: label of the key algorithm (Ed25519, RSA…).
  ///
  /// In en, this message translates to:
  /// **'Algorithm'**
  String get credentialsDetailsAlgorithm;

  /// Credential details dialog: label of the key fingerprint.
  ///
  /// In en, this message translates to:
  /// **'Fingerprint'**
  String get credentialsDetailsFingerprint;

  /// Credential details dialog, 'Key passphrase' value: the passphrase is stored in the vault.
  ///
  /// In en, this message translates to:
  /// **'Remembered (stored as a separate encrypted secret)'**
  String get credentialsDetailsPassphraseRemembered;

  /// Credential details dialog, 'Key passphrase' value: the passphrase is not stored and is asked on each connection.
  ///
  /// In en, this message translates to:
  /// **'Asked when connecting — the key keeps its own protection'**
  String get credentialsDetailsPassphraseAsked;

  /// Credential details dialog: label of the external agent socket path / pipe name.
  ///
  /// In en, this message translates to:
  /// **'Agent socket'**
  String get credentialsDetailsAgentSocket;

  /// Credential details dialog: label of the creation date.
  ///
  /// In en, this message translates to:
  /// **'Created'**
  String get credentialsDetailsCreated;

  /// Credential details dialog: heading above the public key.
  ///
  /// In en, this message translates to:
  /// **'Public key'**
  String get credentialsDetailsPublicKey;

  /// Button in the credential details dialog and the 'Key generated' dialog: copy the public key.
  ///
  /// In en, this message translates to:
  /// **'Copy public key'**
  String get credentialsCopyPublicKey;

  /// Credential details dialog: heading above the SSH certificate.
  ///
  /// In en, this message translates to:
  /// **'Certificate'**
  String get credentialsDetailsCertificate;

  /// Credential details dialog: button that shows the stored password for 20 seconds.
  ///
  /// In en, this message translates to:
  /// **'Reveal'**
  String get credentialsReveal;

  /// Credential details dialog: note under the password reveal/copy buttons.
  ///
  /// In en, this message translates to:
  /// **'Revealed values hide again after 20 s; copied values are cleared from the clipboard automatically.'**
  String get credentialsRevealHint;

  /// Title of the dialog that stores a password credential.
  ///
  /// In en, this message translates to:
  /// **'New password'**
  String get credentialDialogNewPasswordTitle;

  /// Credential dialogs: field label for the optional SSH user name.
  ///
  /// In en, this message translates to:
  /// **'Username (optional)'**
  String get credentialDialogUsernameOptional;

  /// New password dialog: password field label.
  ///
  /// In en, this message translates to:
  /// **'Password'**
  String get credentialDialogPasswordLabel;

  /// Label of the private key passphrase (import dialog field and credential details row).
  ///
  /// In en, this message translates to:
  /// **'Key passphrase'**
  String get credentialDialogKeyPassphrase;

  /// Checkbox in the generate/import key dialogs: store the key passphrase in the vault.
  ///
  /// In en, this message translates to:
  /// **'Remember SSH key passphrase'**
  String get credentialDialogRememberPassphrase;

  /// Title of the dialog that adds an SSH agent credential.
  ///
  /// In en, this message translates to:
  /// **'SSH agent'**
  String get credentialDialogAgentTitle;

  /// Default (editable) name of a new SSH agent credential.
  ///
  /// In en, this message translates to:
  /// **'SSH agent'**
  String get credentialDialogAgentDefaultName;

  /// SSH agent dialog: field label for the external agent's Unix socket path or Windows named pipe.
  ///
  /// In en, this message translates to:
  /// **'Socket path or pipe name'**
  String get credentialDialogAgentPathLabel;

  /// SSH agent dialog: hint with two example agent locations (paths are not translated).
  ///
  /// In en, this message translates to:
  /// **'{unixPath}  or  {windowsPath}'**
  String credentialDialogAgentPathHint(String unixPath, String windowsPath);

  /// SSH agent dialog: note at the bottom.
  ///
  /// In en, this message translates to:
  /// **'Keys stay in the agent; ConsoleCrypt never sees them.'**
  String get credentialDialogAgentNotice;

  /// Title of the generate SSH key dialog.
  ///
  /// In en, this message translates to:
  /// **'Generate SSH key'**
  String get credentialGenerateTitle;

  /// Generate SSH key dialog: confirm button.
  ///
  /// In en, this message translates to:
  /// **'Generate'**
  String get credentialGenerateAction;

  /// Generate SSH key dialog: algorithm segment label for the recommended algorithm.
  ///
  /// In en, this message translates to:
  /// **'{algorithm} (recommended)'**
  String credentialGenerateRecommended(String algorithm);

  /// Generate SSH key dialog: note shown when an RSA algorithm is selected.
  ///
  /// In en, this message translates to:
  /// **'RSA is for compatibility with older servers; generation takes a few seconds.'**
  String get credentialGenerateRsaNotice;

  /// Generate SSH key dialog: field label for the public key comment.
  ///
  /// In en, this message translates to:
  /// **'Comment (optional)'**
  String get credentialGenerateCommentLabel;

  /// Generate SSH key dialog: field label for the optional key passphrase.
  ///
  /// In en, this message translates to:
  /// **'Key passphrase (optional)'**
  String get credentialGeneratePassphraseLabel;

  /// Generate SSH key dialog: helper text under the key passphrase field.
  ///
  /// In en, this message translates to:
  /// **'Protects the key itself, in addition to vault encryption.'**
  String get credentialGeneratePassphraseHelper;

  /// Generate SSH key dialog: field label for repeating the key passphrase.
  ///
  /// In en, this message translates to:
  /// **'Repeat key passphrase'**
  String get credentialGenerateRepeatPassphrase;

  /// Generate SSH key dialog: error when the two key passphrases differ.
  ///
  /// In en, this message translates to:
  /// **'Passphrases do not match'**
  String get credentialGeneratePassphraseMismatch;

  /// Generate SSH key dialog: explanation under 'Remember SSH key passphrase'.
  ///
  /// In en, this message translates to:
  /// **'Stored as a separate encrypted secret in the vault, so you are not asked when connecting.'**
  String get credentialGenerateRememberSubtitle;

  /// Title of the dialog shown after a key was generated.
  ///
  /// In en, this message translates to:
  /// **'Key generated'**
  String get credentialGenerateDoneTitle;

  /// Key generated dialog: instruction above the public key (the path is not translated).
  ///
  /// In en, this message translates to:
  /// **'Add this public key to ~/.ssh/authorized_keys on your servers:'**
  String get credentialGenerateDoneMessage;

  /// Title of the import private key dialog.
  ///
  /// In en, this message translates to:
  /// **'Import OpenSSH key'**
  String get credentialImportTitle;

  /// Title of the import private key dialog when an SSH certificate is imported with the key.
  ///
  /// In en, this message translates to:
  /// **'Import key + certificate'**
  String get credentialImportTitleWithCertificate;

  /// Import key dialog: confirm button.
  ///
  /// In en, this message translates to:
  /// **'Import'**
  String get credentialImportAction;

  /// Import key dialog: label above the private key text area.
  ///
  /// In en, this message translates to:
  /// **'Private key'**
  String get credentialImportPrivateKey;

  /// Import key dialog: shown instead of the algorithm name in the 'valid key' banner when the algorithm is unknown.
  ///
  /// In en, this message translates to:
  /// **'Key'**
  String get credentialImportUnknownAlgorithm;

  /// Import key dialog: second line of the 'valid key' banner when the pasted key is encrypted with a passphrase.
  ///
  /// In en, this message translates to:
  /// **'Passphrase-protected: it stays encrypted in your vault.'**
  String get credentialImportEncryptedNotice;

  /// Import key dialog: banner title when the pasted text is not a usable private key (the service diagnostic, if any, is shown below).
  ///
  /// In en, this message translates to:
  /// **'Invalid key'**
  String get credentialImportInvalidKey;

  /// Import key dialog: explanation under 'Remember SSH key passphrase'.
  ///
  /// In en, this message translates to:
  /// **'Off: you are asked for it on each connection. On: stored as a separate encrypted secret.'**
  String get credentialImportRememberSubtitle;

  /// Import key + certificate dialog: field label for the OpenSSH certificate.
  ///
  /// In en, this message translates to:
  /// **'OpenSSH certificate'**
  String get credentialImportCertificateLabel;

  /// Approve-device dialog title. {name} is the new device's name.
  ///
  /// In en, this message translates to:
  /// **'Approve \"{name}\"?'**
  String approveDeviceTitle(String name);

  /// Approve-device dialog, step 1 (security-critical). {name} is the new device's name.
  ///
  /// In en, this message translates to:
  /// **'1. On \"{name}\", ConsoleCrypt shows a verification code.'**
  String approveDeviceStepShowsCode(String name);

  /// Approve-device dialog, step 2, shown above the locally computed verification code (6 groups of 5 digits). Security-critical.
  ///
  /// In en, this message translates to:
  /// **'2. Compare it with the code computed here, group by group:'**
  String get approveDeviceStepCompare;

  /// Checkbox the user must tick before Approve is enabled: every one of the 6 code groups is identical on both screens. Security-critical. {name} is the new device's name.
  ///
  /// In en, this message translates to:
  /// **'All 6 groups match the code shown on \"{name}\"'**
  String approveDeviceCodesMatchCheckbox(String name);

  /// Warning banner in the approve-device dialog. Security-critical.
  ///
  /// In en, this message translates to:
  /// **'Approving gives this device the key to your vault. Never approve a device you are not setting up yourself right now.'**
  String get approveDeviceWarning;

  /// Button in the approve-device dialog: the codes differ; pressing it rejects the request.
  ///
  /// In en, this message translates to:
  /// **'Codes don\'t match'**
  String get approveDeviceCodesDontMatch;

  /// Button: approve the new device (enabled only after the codes-match checkbox is ticked).
  ///
  /// In en, this message translates to:
  /// **'Approve'**
  String get approveDeviceApprove;

  /// Snackbar after a device was approved. {name} is the device name.
  ///
  /// In en, this message translates to:
  /// **'{name} can now unlock your vault'**
  String approveDeviceApproved(String name);

  /// Title of the dialog shown after the user pressed "Codes don't match".
  ///
  /// In en, this message translates to:
  /// **'Request rejected'**
  String get approveDeviceRejectedTitle;

  /// Body of the dialog shown after the user pressed "Codes don't match". Security-critical.
  ///
  /// In en, this message translates to:
  /// **'The codes did not match, so the device was NOT approved. Someone — possibly a compromised server — may have tried to add their own device to your account.\n\nIf you did not start this, change your account password and review your devices.'**
  String get approveDeviceRejectedMessage;

  /// Devices page title.
  ///
  /// In en, this message translates to:
  /// **'Devices'**
  String get devicesTitle;

  /// Confirm dialog title: revoke a device. {name} is the device name.
  ///
  /// In en, this message translates to:
  /// **'Revoke {name}?'**
  String devicesRevokeTitle(String name);

  /// Confirm dialog body: consequences of revoking a device.
  ///
  /// In en, this message translates to:
  /// **'The device is signed out immediately, loses access to all your vaults and can no longer sync. This cannot be undone — to use it again it must be approved as a new device.\n\nRevocation protects the future, not the past: data the device already decrypted cannot be erased remotely.'**
  String get devicesRevokeMessage;

  /// Destructive confirm button in the revoke-device dialog.
  ///
  /// In en, this message translates to:
  /// **'Revoke device'**
  String get devicesRevokeConfirm;

  /// Snackbar after a device was revoked. {name} is the device name.
  ///
  /// In en, this message translates to:
  /// **'{name} revoked'**
  String devicesRevoked(String name);

  /// Text input dialog title: rename a device.
  ///
  /// In en, this message translates to:
  /// **'Rename device'**
  String get devicesRenameTitle;

  /// Empty state title on the Devices page for a local profile.
  ///
  /// In en, this message translates to:
  /// **'Devices apply to synced profiles'**
  String get devicesLocalTitle;

  /// Empty state message on the Devices page for a local profile.
  ///
  /// In en, this message translates to:
  /// **'This profile is local-only: it exists on this device and nowhere else. Enable sync to use the vault on several machines with device approval.'**
  String get devicesLocalMessage;

  /// Button on the Devices empty state: go to Settings to enable sync.
  ///
  /// In en, this message translates to:
  /// **'Enable sync…'**
  String get devicesEnableSync;

  /// Devices page subtitle. {email} is the account e-mail.
  ///
  /// In en, this message translates to:
  /// **'Devices signed in to {email}. Only trusted devices can decrypt your vault.'**
  String devicesSubtitle(String email);

  /// Devices page subtitle when the account e-mail is unknown.
  ///
  /// In en, this message translates to:
  /// **'Devices signed in to your account. Only trusted devices can decrypt your vault.'**
  String get devicesSubtitleNoEmail;

  /// Tooltip of the refresh button on the Devices page.
  ///
  /// In en, this message translates to:
  /// **'Refresh'**
  String get devicesRefresh;

  /// Pending device-approval card title. {name} is the new device's name.
  ///
  /// In en, this message translates to:
  /// **'\"{name}\" wants to access your vault'**
  String devicesPendingTitle(String name);

  /// Pending device-approval card details. {platform} is the OS name, {ago} a relative time like "5 min ago", {remaining} a remaining time like "in 9 min".
  ///
  /// In en, this message translates to:
  /// **'{platform} · requested {ago} · expires {remaining}'**
  String devicesPendingMeta(String platform, String ago, String remaining);

  /// Pending device-approval card details when the request has already expired. {platform} is the OS name, {ago} a relative time like "5 min ago".
  ///
  /// In en, this message translates to:
  /// **'{platform} · requested {ago} · expires expired'**
  String devicesPendingMetaExpired(String platform, String ago);

  /// Pending device-approval card hint. Security-critical.
  ///
  /// In en, this message translates to:
  /// **'Approve only if you are setting up this device right now and the verification codes match.'**
  String get devicesPendingHint;

  /// Snackbar after the user rejected a device-approval request.
  ///
  /// In en, this message translates to:
  /// **'Request rejected'**
  String get devicesRequestRejected;

  /// Button on the pending device-approval card: reject the request.
  ///
  /// In en, this message translates to:
  /// **'Reject'**
  String get devicesReject;

  /// Button on the pending device-approval card: open the verification-code dialog.
  ///
  /// In en, this message translates to:
  /// **'Review & approve'**
  String get devicesReviewApprove;

  /// Device status label in the devices list.
  ///
  /// In en, this message translates to:
  /// **'Revoked'**
  String get devicesStatusRevoked;

  /// Device status label: trusted for the current vault.
  ///
  /// In en, this message translates to:
  /// **'Trusted'**
  String get devicesStatusTrusted;

  /// Device status label: an approval request is pending.
  ///
  /// In en, this message translates to:
  /// **'Awaiting approval'**
  String get devicesStatusAwaitingApproval;

  /// Device status label: not trusted for the current vault.
  ///
  /// In en, this message translates to:
  /// **'Not trusted for this vault'**
  String get devicesStatusNotTrusted;

  /// Chip next to the name of the current device in the devices list.
  ///
  /// In en, this message translates to:
  /// **'This device'**
  String get devicesThisDevice;

  /// Device list item detail (joined with " · "). {ago} is a relative time like "5 min ago".
  ///
  /// In en, this message translates to:
  /// **'last seen {ago}'**
  String devicesLastSeen(String ago);

  /// Device list item detail (joined with " · "). {date} is a formatted date.
  ///
  /// In en, this message translates to:
  /// **'added {date}'**
  String devicesAdded(String date);

  /// Device list item detail (joined with " · "). {date} is a formatted date.
  ///
  /// In en, this message translates to:
  /// **'revoked {date}'**
  String devicesRevokedOn(String date);

  /// Device menu item: revoke the device (opens a confirmation).
  ///
  /// In en, this message translates to:
  /// **'Revoke…'**
  String get devicesRevokeMenu;

  /// Sync page title.
  ///
  /// In en, this message translates to:
  /// **'Sync'**
  String get syncScreenTitle;

  /// Sync page, local profile: section subtitle.
  ///
  /// In en, this message translates to:
  /// **'This profile has no server and no account. Nothing leaves this device.'**
  String get syncScreenLocalSubtitle;

  /// Sync page, local profile: explanation of enabling sync.
  ///
  /// In en, this message translates to:
  /// **'Enable sync to use this vault on other machines. The vault is uploaded end-to-end encrypted to your self-hosted server; its ID, passphrase and Recovery Kit stay the same.'**
  String get syncScreenLocalBody;

  /// Button on the Sync page: open the enable-sync wizard.
  ///
  /// In en, this message translates to:
  /// **'Enable sync…'**
  String get syncScreenEnableSync;

  /// Button on the Sync page: go to the Backups page.
  ///
  /// In en, this message translates to:
  /// **'Backups'**
  String get syncScreenBackups;

  /// Sync page subtitle. {url} is the server URL.
  ///
  /// In en, this message translates to:
  /// **'Server: {url}'**
  String syncScreenServer(String url);

  /// Button on the Sync page: sync immediately.
  ///
  /// In en, this message translates to:
  /// **'Sync now'**
  String get syncScreenSyncNow;

  /// Sync page field label: current sync state.
  ///
  /// In en, this message translates to:
  /// **'State'**
  String get syncScreenState;

  /// Sync page, value of the State field: up to date with the server (lower-case).
  ///
  /// In en, this message translates to:
  /// **'idle'**
  String get syncScreenStateIdle;

  /// Sync page, value of the State field: push/pull in progress (lower-case).
  ///
  /// In en, this message translates to:
  /// **'syncing'**
  String get syncScreenStateSyncing;

  /// Sync page, value of the State field: server unreachable (lower-case).
  ///
  /// In en, this message translates to:
  /// **'offline'**
  String get syncScreenStateOffline;

  /// Sync page, value of the State field: sync failed (lower-case).
  ///
  /// In en, this message translates to:
  /// **'error'**
  String get syncScreenStateError;

  /// Sync page, value of the State field: sync paused (lower-case).
  ///
  /// In en, this message translates to:
  /// **'paused'**
  String get syncScreenStatePaused;

  /// Sync page field label.
  ///
  /// In en, this message translates to:
  /// **'Last successful sync'**
  String get syncScreenLastSync;

  /// Sync page field label: number of local changes not yet pushed.
  ///
  /// In en, this message translates to:
  /// **'Pending changes'**
  String get syncScreenPendingChanges;

  /// Sync page field label: last server change sequence number.
  ///
  /// In en, this message translates to:
  /// **'Server sequence'**
  String get syncScreenServerSequence;

  /// Sync page field label: when the next sync attempt happens.
  ///
  /// In en, this message translates to:
  /// **'Next retry'**
  String get syncScreenNextRetry;

  /// Sync page offline banner message.
  ///
  /// In en, this message translates to:
  /// **'The server is unreachable. Your hosts, keys, SSH, SFTP and tunnels keep working; changes are stored locally and pushed when the server is back.'**
  String get syncScreenOfflineMessage;

  /// Sync page section title: list of sync problems.
  ///
  /// In en, this message translates to:
  /// **'Problems'**
  String get syncScreenProblems;

  /// Sync page problem list item title for a temporary problem; the technical details follow below it.
  ///
  /// In en, this message translates to:
  /// **'Sync failed — will retry automatically'**
  String get syncScreenIssueRetryable;

  /// Sync page problem list item title; the technical details follow below it.
  ///
  /// In en, this message translates to:
  /// **'Sync failed'**
  String get syncScreenIssue;

  /// Sync page section title: stop syncing this profile.
  ///
  /// In en, this message translates to:
  /// **'Disconnect'**
  String get syncScreenDisconnectTitle;

  /// Sync page, Disconnect section text.
  ///
  /// In en, this message translates to:
  /// **'Stop syncing this profile and keep its data locally.'**
  String get syncScreenDisconnectBody;

  /// Button on the Sync page: open the disconnect dialog.
  ///
  /// In en, this message translates to:
  /// **'Disconnect…'**
  String get syncScreenDisconnect;

  /// Sync page section title, shown only with the mock backend.
  ///
  /// In en, this message translates to:
  /// **'Developer (mock backend)'**
  String get syncScreenDeveloper;

  /// Developer switch on the Sync page (mock backend only).
  ///
  /// In en, this message translates to:
  /// **'Simulate server outage'**
  String get syncScreenSimulateOutage;

  /// Disconnect-sync dialog title.
  ///
  /// In en, this message translates to:
  /// **'Disconnect sync?'**
  String get syncDisconnectTitle;

  /// Disconnect-sync dialog body.
  ///
  /// In en, this message translates to:
  /// **'Sync stops and this profile becomes local-only. All data stays on this device. The encrypted copy on the server is left untouched.'**
  String get syncDisconnectMessage;

  /// Checkbox in the disconnect-sync dialog.
  ///
  /// In en, this message translates to:
  /// **'Also revoke this device on the server'**
  String get syncDisconnectRevoke;

  /// Confirm button in the disconnect-sync dialog.
  ///
  /// In en, this message translates to:
  /// **'Disconnect (keep data locally)'**
  String get syncDisconnectConfirm;

  /// Enable-sync wizard title.
  ///
  /// In en, this message translates to:
  /// **'Enable sync'**
  String get enableSyncDialogTitle;

  /// Enable-sync wizard step header.
  ///
  /// In en, this message translates to:
  /// **'Step 1 of 3 · Your server'**
  String get enableSyncDialogStepServer;

  /// Enable-sync wizard step header.
  ///
  /// In en, this message translates to:
  /// **'Step 2 of 3 · Account'**
  String get enableSyncDialogStepAccount;

  /// Enable-sync wizard step header (uploading the encrypted vault).
  ///
  /// In en, this message translates to:
  /// **'Step 3 of 3 · Upload'**
  String get enableSyncDialogStepUpload;

  /// Enable-sync wizard, step 1 explanation.
  ///
  /// In en, this message translates to:
  /// **'Your vault is uploaded end-to-end encrypted: the server stores only ciphertext and never sees your passphrase or keys. The vault keeps its ID and your Recovery Kit stays valid.'**
  String get enableSyncDialogIntro;

  /// Enable-sync wizard field label.
  ///
  /// In en, this message translates to:
  /// **'Server URL'**
  String get enableSyncDialogServerUrl;

  /// Enable-sync wizard banner after the server was reached. Placeholders are version strings.
  ///
  /// In en, this message translates to:
  /// **'ConsoleCrypt server {serverVersion} · protocol {protocolVersion}'**
  String enableSyncDialogServerInfo(String serverVersion, String protocolVersion);

  /// Enable-sync wizard segment: register a new account.
  ///
  /// In en, this message translates to:
  /// **'Create account'**
  String get enableSyncDialogCreateAccount;

  /// Enable-sync wizard segment: sign in to an existing account.
  ///
  /// In en, this message translates to:
  /// **'Sign in'**
  String get enableSyncDialogSignIn;

  /// Enable-sync wizard field label.
  ///
  /// In en, this message translates to:
  /// **'E-mail'**
  String get enableSyncDialogEmail;

  /// Enable-sync wizard field label.
  ///
  /// In en, this message translates to:
  /// **'Account password'**
  String get enableSyncDialogPassword;

  /// Enable-sync wizard helper under the account password when creating an account.
  ///
  /// In en, this message translates to:
  /// **'At least 10 characters. Not your vault passphrase.'**
  String get enableSyncDialogPasswordHelper;

  /// Enable-sync wizard field label.
  ///
  /// In en, this message translates to:
  /// **'Name of this device'**
  String get enableSyncDialogDeviceName;

  /// Enable-sync wizard error: e-mail or password missing.
  ///
  /// In en, this message translates to:
  /// **'Enter your e-mail and password'**
  String get enableSyncDialogCredentialsRequired;

  /// Enable-sync wizard upload progress. {uploaded} objects uploaded out of {total}.
  ///
  /// In en, this message translates to:
  /// **'{total, plural, other{{uploaded} of {total} encrypted objects}}'**
  String enableSyncDialogUploadedObjects(int uploaded, int total);

  /// Enable-sync wizard success banner.
  ///
  /// In en, this message translates to:
  /// **'This profile is now synced. Add other devices by signing in there and approving them here with the verification code.'**
  String get enableSyncDialogDone;

  /// Enable-sync wizard failure banner message when no details are available.
  ///
  /// In en, this message translates to:
  /// **'Enabling sync failed.'**
  String get enableSyncDialogFailed;

  /// Enable-sync wizard failure banner title; the technical details follow below it.
  ///
  /// In en, this message translates to:
  /// **'Enabling sync failed'**
  String get enableSyncDialogFailedTitle;

  /// Enable-sync wizard button on the account step: start enabling sync.
  ///
  /// In en, this message translates to:
  /// **'Enable sync'**
  String get enableSyncDialogStart;

  /// Enable-sync wizard button after a failure: go back to the account step.
  ///
  /// In en, this message translates to:
  /// **'Try again'**
  String get enableSyncDialogTryAgain;

  /// Settings → Appearance: Liquid Glass intensity selector label (GlassStrings.glassSetting).
  ///
  /// In en, this message translates to:
  /// **'Glass'**
  String get glassSetting;

  /// Glass mode (adjective for «стекло»).
  ///
  /// In en, this message translates to:
  /// **'Clear'**
  String get glassModeClear;

  /// Glass mode: default intensity.
  ///
  /// In en, this message translates to:
  /// **'Default'**
  String get glassModeStandard;

  /// Glass mode.
  ///
  /// In en, this message translates to:
  /// **'Tinted'**
  String get glassModeTinted;

  /// Glass mode: no transparency.
  ///
  /// In en, this message translates to:
  /// **'Solid'**
  String get glassModeSolid;

  /// Warning under a secret field.
  ///
  /// In en, this message translates to:
  /// **'Caps Lock is on'**
  String get glassCapsLockOn;

  /// Tooltip: dismiss a banner/toast.
  ///
  /// In en, this message translates to:
  /// **'Dismiss'**
  String get glassDismiss;

  /// Tooltip on a terminal tab.
  ///
  /// In en, this message translates to:
  /// **'Close tab'**
  String get glassCloseTab;

  /// Tooltip of the new-tab button in the tab strip.
  ///
  /// In en, this message translates to:
  /// **'New tab'**
  String get glassNewTab;

  /// Terminal tab state (semantics).
  ///
  /// In en, this message translates to:
  /// **'Connected'**
  String get glassTabConnected;

  /// Terminal tab state (semantics).
  ///
  /// In en, this message translates to:
  /// **'Reconnecting…'**
  String get glassTabReconnecting;

  /// Terminal tab state (semantics).
  ///
  /// In en, this message translates to:
  /// **'Disconnected'**
  String get glassTabDisconnected;

  /// Semantics label of the verification code block.
  ///
  /// In en, this message translates to:
  /// **'Verification code'**
  String get glassVerificationCode;

  /// Semantics label of one verification-code group; digits are spelled out.
  ///
  /// In en, this message translates to:
  /// **'Group {number}: {digits}'**
  String glassCodeGroup(int number, String digits);

  /// Countdown until the clipboard is cleared.
  ///
  /// In en, this message translates to:
  /// **'Clears in {seconds} s'**
  String glassClearsIn(int seconds);

  /// Tooltip of an overflow ("more actions") button.
  ///
  /// In en, this message translates to:
  /// **'More'**
  String get glassMoreActions;

  /// Hosts list screen title.
  ///
  /// In en, this message translates to:
  /// **'Hosts'**
  String get hostsTitle;

  /// Hosts list screen subtitle. {count} = number of hosts in the vault.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, =1{{count} host in this vault} other{{count} hosts in this vault}}'**
  String hostsSubtitle(int count);

  /// Hosts list header button: create a host.
  ///
  /// In en, this message translates to:
  /// **'New host'**
  String get hostsNewHost;

  /// Hint of the search field on the hosts list.
  ///
  /// In en, this message translates to:
  /// **'Search by name, address, tag or group'**
  String get hostsSearchHint;

  /// Empty state title: the vault has no hosts.
  ///
  /// In en, this message translates to:
  /// **'No hosts yet'**
  String get hostsEmptyTitle;

  /// Empty state title: search/tag filter matched no hosts.
  ///
  /// In en, this message translates to:
  /// **'No matching hosts'**
  String get hostsNoMatches;

  /// Empty state message: the vault has no hosts.
  ///
  /// In en, this message translates to:
  /// **'Add your first SSH host to connect.'**
  String get hostsEmptyMessage;

  /// Empty state button: create the first host.
  ///
  /// In en, this message translates to:
  /// **'Add host'**
  String get hostsAddHost;

  /// Tooltip of the route icon next to a host that uses jump hosts.
  ///
  /// In en, this message translates to:
  /// **'Connects through jump hosts'**
  String get hostsViaJumpHostsTooltip;

  /// Tooltip of the icon next to a host that uses the system OpenSSH backend.
  ///
  /// In en, this message translates to:
  /// **'Uses the system OpenSSH backend'**
  String get hostsOpenSshTooltip;

  /// Host action and group picker title.
  ///
  /// In en, this message translates to:
  /// **'Add to group…'**
  String get hostsAssignGroup;

  /// Help above host group picker; explains single membership and inheritance.
  ///
  /// In en, this message translates to:
  /// **'Choose one group for this host. Connection settings left unset on the host are inherited from the group.'**
  String get hostsGroupHelp;

  /// Search groups by their full hierarchical path.
  ///
  /// In en, this message translates to:
  /// **'Find a group…'**
  String get hostsGroupSearchHint;

  /// Empty group search results.
  ///
  /// In en, this message translates to:
  /// **'No matching groups'**
  String get hostsGroupNoMatches;

  /// Tooltip of the per-host overflow menu button.
  ///
  /// In en, this message translates to:
  /// **'More'**
  String get hostsMoreTooltip;

  /// Host overflow menu item: open an SFTP session to the host.
  ///
  /// In en, this message translates to:
  /// **'Open SFTP'**
  String get hostsOpenSftp;

  /// Confirmation dialog title for deleting a host. {name} = host name.
  ///
  /// In en, this message translates to:
  /// **'Delete {name}?'**
  String hostsDeleteTitle(String name);

  /// Confirmation dialog message for deleting a host.
  ///
  /// In en, this message translates to:
  /// **'The host is removed from this vault (and from your other devices after sync). Jump chains that use it are updated.'**
  String get hostsDeleteMessage;

  /// Snackbar after a host was deleted.
  ///
  /// In en, this message translates to:
  /// **'Host deleted'**
  String get hostsDeleted;

  /// Last item of credential dropdowns (host key picker, group credential): create a new credential.
  ///
  /// In en, this message translates to:
  /// **'+ New credential…'**
  String get hostsNewCredentialEntry;

  /// Host editor title while the host is still loading.
  ///
  /// In en, this message translates to:
  /// **'Host'**
  String get hostEditorLoadingTitle;

  /// Host editor title when creating a host.
  ///
  /// In en, this message translates to:
  /// **'New host'**
  String get hostEditorNewTitle;

  /// Host editor title when editing a host. {name} = host name.
  ///
  /// In en, this message translates to:
  /// **'Edit {name}'**
  String hostEditorEditTitle(String name);

  /// Host editor title when editing a host whose name is unknown.
  ///
  /// In en, this message translates to:
  /// **'Edit host'**
  String get hostEditorEditTitleFallback;

  /// Host editor section title: name, address, port, username, group.
  ///
  /// In en, this message translates to:
  /// **'Connection'**
  String get hostEditorConnectionSection;

  /// Validator message: the host name is empty.
  ///
  /// In en, this message translates to:
  /// **'Enter a name'**
  String get hostEditorNameRequired;

  /// Host editor field label: host name or IP address.
  ///
  /// In en, this message translates to:
  /// **'Address'**
  String get hostEditorAddressLabel;

  /// Hint of the host address field.
  ///
  /// In en, this message translates to:
  /// **'hostname or IP'**
  String get hostEditorAddressHint;

  /// Validator message: the host address is empty.
  ///
  /// In en, this message translates to:
  /// **'Enter an address'**
  String get hostEditorAddressRequired;

  /// Validator message: the host address contains whitespace.
  ///
  /// In en, this message translates to:
  /// **'No spaces allowed'**
  String get hostEditorAddressNoSpaces;

  /// Host editor field label: SSH port.
  ///
  /// In en, this message translates to:
  /// **'Port'**
  String get hostEditorPortLabel;

  /// Hint of the port field: empty means the value is inherited from the group.
  ///
  /// In en, this message translates to:
  /// **'inherit'**
  String get hostEditorPortHint;

  /// Host editor field label: SSH user name.
  ///
  /// In en, this message translates to:
  /// **'Username'**
  String get hostEditorUsernameLabel;

  /// Hint of the username field: empty means the value is inherited from the group.
  ///
  /// In en, this message translates to:
  /// **'inherit from group'**
  String get hostEditorUsernameHint;

  /// Host editor dropdown label: the host's group.
  ///
  /// In en, this message translates to:
  /// **'Group'**
  String get hostEditorGroupLabel;

  /// Group dropdown item: the host is not in a group.
  ///
  /// In en, this message translates to:
  /// **'No group'**
  String get hostEditorNoGroup;

  /// Host editor section title: jump hosts / route.
  ///
  /// In en, this message translates to:
  /// **'Jump hosts'**
  String get hostEditorJumpSection;

  /// Subtitle of the jump hosts section. direct-tcpip is an SSH channel type (not translated).
  ///
  /// In en, this message translates to:
  /// **'Multi-hop: each hop opens a direct-tcpip channel to the next. No limit on the number of hops.'**
  String get hostEditorJumpSubtitle;

  /// Jump mode segment: use the group's jump profile or connect directly.
  ///
  /// In en, this message translates to:
  /// **'Inherit / direct'**
  String get hostEditorJumpInherit;

  /// Jump mode segment and dropdown label: use a named jump profile.
  ///
  /// In en, this message translates to:
  /// **'Jump profile'**
  String get hostEditorJumpProfile;

  /// Jump mode segment: a chain of jump hosts defined on this host.
  ///
  /// In en, this message translates to:
  /// **'Custom chain'**
  String get hostEditorJumpCustom;

  /// Explanation shown for the 'Inherit / direct' jump mode.
  ///
  /// In en, this message translates to:
  /// **'Uses the jump profile of the group (if any), otherwise connects directly.'**
  String get hostEditorJumpInheritHelp;

  /// Jump profile dropdown item. {name} = profile name, {count} = number of hops (jump hosts) in it.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, other{{name} ({count} hops)}}'**
  String hostEditorJumpProfileItem(int count, String name);

  /// Route preview end when the edited host has no name yet ('This device → jump → this host').
  ///
  /// In en, this message translates to:
  /// **'this host'**
  String get hostEditorThisHost;

  /// Host editor section title: host key policy, SSH backend, keepalive, agent forwarding.
  ///
  /// In en, this message translates to:
  /// **'Security'**
  String get hostEditorSecuritySection;

  /// Label above the host key policy segmented button.
  ///
  /// In en, this message translates to:
  /// **'Host key policy'**
  String get hostEditorHostKeyPolicy;

  /// Dropdown label: built-in or system OpenSSH backend.
  ///
  /// In en, this message translates to:
  /// **'SSH backend'**
  String get hostEditorSshBackend;

  /// Field label: SSH keepalive interval in seconds.
  ///
  /// In en, this message translates to:
  /// **'Keepalive interval (s)'**
  String get hostEditorKeepalive;

  /// Hint of the keepalive field: empty uses the app-wide default.
  ///
  /// In en, this message translates to:
  /// **'app default'**
  String get hostEditorKeepaliveHint;

  /// Switch title: SSH agent forwarding for this host.
  ///
  /// In en, this message translates to:
  /// **'Agent forwarding'**
  String get hostEditorAgentForwarding;

  /// Switch subtitle: security note about SSH agent forwarding.
  ///
  /// In en, this message translates to:
  /// **'Lets this server use your keys for onward connections. Enable only for hosts you trust.'**
  String get hostEditorAgentForwardingHelp;

  /// Host editor section title: tags and notes.
  ///
  /// In en, this message translates to:
  /// **'Organisation'**
  String get hostEditorOrganisationSection;

  /// Field label: free-text notes about the host.
  ///
  /// In en, this message translates to:
  /// **'Notes'**
  String get hostEditorNotes;

  /// Title of the panel showing the settings a connection will actually use after group inheritance.
  ///
  /// In en, this message translates to:
  /// **'Effective settings'**
  String get hostEditorEffectiveTitle;

  /// Subtitle of the effective settings panel.
  ///
  /// In en, this message translates to:
  /// **'Resolved from this host and its group chain — what a connection will use.'**
  String get hostEditorEffectiveSubtitle;

  /// Effective settings row label: group path (narrow 110 px column).
  ///
  /// In en, this message translates to:
  /// **'Group'**
  String get hostEditorEffectiveGroup;

  /// Effective settings row label: SSH user name (narrow 110 px column).
  ///
  /// In en, this message translates to:
  /// **'Username'**
  String get hostEditorEffectiveUsername;

  /// Effective settings row label: SSH port (narrow 110 px column).
  ///
  /// In en, this message translates to:
  /// **'Port'**
  String get hostEditorEffectivePort;

  /// Effective settings row label: credential used (narrow 110 px column).
  ///
  /// In en, this message translates to:
  /// **'Credential'**
  String get hostEditorEffectiveCredential;

  /// Effective settings row label: jump route (narrow 110 px column).
  ///
  /// In en, this message translates to:
  /// **'Route'**
  String get hostEditorEffectiveRoute;

  /// Effective route value: no jump hosts, direct connection.
  ///
  /// In en, this message translates to:
  /// **'Direct'**
  String get hostEditorRouteDirect;

  /// Provenance under an effective value: set directly on the host.
  ///
  /// In en, this message translates to:
  /// **'set on this host'**
  String get hostEditorSourceHost;

  /// Provenance under an effective value. {name} = where the value comes from.
  ///
  /// In en, this message translates to:
  /// **'from {name}'**
  String hostEditorSourceFrom(String name);

  /// Provenance of the username: taken from the credential. {name} = credential name.
  ///
  /// In en, this message translates to:
  /// **'from credential {name}'**
  String hostEditorSourceCredential(String name);

  /// Provenance under an effective value. {name} = group name.
  ///
  /// In en, this message translates to:
  /// **'inherited from group {name}'**
  String hostEditorSourceGroup(String name);

  /// Provenance of the route. {name} = jump profile name.
  ///
  /// In en, this message translates to:
  /// **'jump profile {name}'**
  String hostEditorSourceJumpProfile(String name);

  /// Provenance under an effective value: app default.
  ///
  /// In en, this message translates to:
  /// **'default'**
  String get hostEditorSourceDefault;

  /// Provenance under an effective value: nobody sets it.
  ///
  /// In en, this message translates to:
  /// **'not set'**
  String get hostEditorSourceUnset;

  /// Effective credential value: no stored password, the terminal asks for it.
  ///
  /// In en, this message translates to:
  /// **'Password — asked when connecting'**
  String get hostEditorCredentialPrompt;

  /// Warning in the effective settings panel.
  ///
  /// In en, this message translates to:
  /// **'A jump host in the chain was deleted'**
  String get hostEditorProblemJumpHostDeleted;

  /// Warning in the effective settings panel: the host is in its own jump chain.
  ///
  /// In en, this message translates to:
  /// **'The host cannot jump through itself'**
  String get hostEditorProblemSelfJump;

  /// Warning in the effective settings panel.
  ///
  /// In en, this message translates to:
  /// **'No credential: you will be asked for a password when connecting'**
  String get hostEditorProblemNoCredential;

  /// Warning in the effective settings panel.
  ///
  /// In en, this message translates to:
  /// **'The selected credential no longer exists'**
  String get hostEditorProblemCredentialMissing;

  /// Warning in the effective settings panel.
  ///
  /// In en, this message translates to:
  /// **'No username: set one on the host or on a group'**
  String get hostEditorProblemNoUsername;

  /// Authentication mode segment: password.
  ///
  /// In en, this message translates to:
  /// **'Password'**
  String get hostAuthModePassword;

  /// Authentication mode segment: SSH key.
  ///
  /// In en, this message translates to:
  /// **'SSH key'**
  String get hostAuthModeSshKey;

  /// Authentication mode segment: SSH agent.
  ///
  /// In en, this message translates to:
  /// **'Agent'**
  String get hostAuthModeAgent;

  /// Authentication mode segment: use the group's credential (keep short, one of four segments).
  ///
  /// In en, this message translates to:
  /// **'Inherit from group'**
  String get hostAuthModeInherit;

  /// Host editor section title: how the host authenticates.
  ///
  /// In en, this message translates to:
  /// **'Authentication'**
  String get hostAuthTitle;

  /// Button in the authentication section header: link a shared credential.
  ///
  /// In en, this message translates to:
  /// **'Use existing credential…'**
  String get hostAuthUseExisting;

  /// Shown when the linked shared credential no longer exists.
  ///
  /// In en, this message translates to:
  /// **'Linked credential was deleted'**
  String get hostAuthLinkedDeleted;

  /// Linked shared credential. {name} = credential name, {kind} = lower-cased credential kind (e.g. 'password').
  ///
  /// In en, this message translates to:
  /// **'{name} · shared {kind}'**
  String hostAuthLinkedShared(String name, String kind);

  /// Button: unlink the shared credential and type a password.
  ///
  /// In en, this message translates to:
  /// **'Enter a password instead'**
  String get hostAuthUnlink;

  /// Button: pick another shared credential.
  ///
  /// In en, this message translates to:
  /// **'Change…'**
  String get hostAuthChangeLinked;

  /// Followed by a masked '••••••'; the trailing space separates them.
  ///
  /// In en, this message translates to:
  /// **'Password saved '**
  String get hostAuthPasswordSaved;

  /// Field label: host password.
  ///
  /// In en, this message translates to:
  /// **'Password'**
  String get hostAuthPasswordLabel;

  /// Field label: replacement for the saved host password.
  ///
  /// In en, this message translates to:
  /// **'New password'**
  String get hostAuthNewPasswordLabel;

  /// Helper text under the host password field.
  ///
  /// In en, this message translates to:
  /// **'Leave empty to be asked when connecting.'**
  String get hostAuthPasswordHelper;

  /// Checkbox: store the host password in the vault.
  ///
  /// In en, this message translates to:
  /// **'Save password in vault'**
  String get hostAuthSavePassword;

  /// Checkbox subtitle when saving the password is on.
  ///
  /// In en, this message translates to:
  /// **'Stored end-to-end encrypted as a separate secret; never shown again.'**
  String get hostAuthSavePasswordOn;

  /// Checkbox subtitle when saving the password is off.
  ///
  /// In en, this message translates to:
  /// **'Nothing is stored — the terminal asks for the password each time you connect.'**
  String get hostAuthSavePasswordOff;

  /// Button: cancel changing/removing the saved password.
  ///
  /// In en, this message translates to:
  /// **'Keep the saved password'**
  String get hostAuthKeepSaved;

  /// Dropdown label: SSH key credential for the host.
  ///
  /// In en, this message translates to:
  /// **'Key'**
  String get hostAuthKeyLabel;

  /// Button: generate a new Ed25519 SSH key.
  ///
  /// In en, this message translates to:
  /// **'Generate Ed25519…'**
  String get hostAuthGenerateKey;

  /// Button: import an existing SSH private key.
  ///
  /// In en, this message translates to:
  /// **'Import key…'**
  String get hostAuthImportKey;

  /// Status: the key passphrase is stored in the vault.
  ///
  /// In en, this message translates to:
  /// **'Key passphrase remembered'**
  String get hostAuthPassphraseRemembered;

  /// Button: delete the remembered key passphrase.
  ///
  /// In en, this message translates to:
  /// **'Forget'**
  String get hostAuthForget;

  /// Snackbar after the remembered key passphrase was deleted.
  ///
  /// In en, this message translates to:
  /// **'Passphrase forgotten'**
  String get hostAuthPassphraseForgotten;

  /// Field label: passphrase of the encrypted SSH key.
  ///
  /// In en, this message translates to:
  /// **'Key passphrase'**
  String get hostAuthKeyPassphraseLabel;

  /// Helper under the key passphrase field; "Remember" refers to the 'Remember passphrase' checkbox.
  ///
  /// In en, this message translates to:
  /// **'The key stays passphrase-protected. Without \"Remember\" you are asked when connecting.'**
  String get hostAuthKeyPassphraseHelper;

  /// Checkbox: store the key passphrase in the vault.
  ///
  /// In en, this message translates to:
  /// **'Remember passphrase'**
  String get hostAuthRememberPassphrase;

  /// Subtitle of the 'Remember passphrase' checkbox.
  ///
  /// In en, this message translates to:
  /// **'Stored as a separate encrypted secret for this key (shared by all its hosts).'**
  String get hostAuthRememberPassphraseHelper;

  /// Field label: external SSH agent socket path (Unix) or named pipe (Windows).
  ///
  /// In en, this message translates to:
  /// **'Socket path or pipe name'**
  String get hostAuthAgentPathLabel;

  /// Hint of the agent path field with two example paths (not translated).
  ///
  /// In en, this message translates to:
  /// **'{unixPath}  or  {windowsPath}'**
  String hostAuthAgentPathHint(String unixPath, String windowsPath);

  /// Note under the agent mode of the authentication section.
  ///
  /// In en, this message translates to:
  /// **'Keys stay in the agent; ConsoleCrypt never sees them.'**
  String get hostAuthAgentNote;

  /// Warning in 'Inherit from group' mode when the host has no group.
  ///
  /// In en, this message translates to:
  /// **'This host is not in a group, so there is nothing to inherit: the terminal will ask for a password. Pick a group or another mode.'**
  String get hostAuthInheritNoGroup;

  /// Explanation in 'Inherit from group' mode. {group} = group path; 'Effective settings' is the panel title.
  ///
  /// In en, this message translates to:
  /// **'Username and credential come from group \"{group}\" and its parents — see Effective settings.'**
  String hostAuthInheritFromGroup(String group);

  /// Effective credential preview: shared password credential. {name} = credential name.
  ///
  /// In en, this message translates to:
  /// **'Password · {name}'**
  String hostAuthPreviewPasswordShared(String name);

  /// Effective credential preview: the linked password credential was deleted.
  ///
  /// In en, this message translates to:
  /// **'Password · deleted credential'**
  String get hostAuthPreviewPasswordDeleted;

  /// Effective credential preview: password stored in the vault.
  ///
  /// In en, this message translates to:
  /// **'Password · saved in vault'**
  String get hostAuthPreviewPasswordSaved;

  /// Effective credential preview: the terminal asks for the password.
  ///
  /// In en, this message translates to:
  /// **'Password · asked when connecting'**
  String get hostAuthPreviewPasswordPrompt;

  /// Effective credential preview: SSH key. {name} = key credential name.
  ///
  /// In en, this message translates to:
  /// **'SSH key · {name}'**
  String hostAuthPreviewSshKey(String name);

  /// Effective credential preview: external agent. {path} = socket path or pipe name.
  ///
  /// In en, this message translates to:
  /// **'Agent at {path}'**
  String hostAuthPreviewAgentAt(String path);

  /// Error on save: SSH key mode without a key.
  ///
  /// In en, this message translates to:
  /// **'Choose, generate or import an SSH key'**
  String get hostAuthErrorKeyRequired;

  /// Error on save: external agent mode without a socket path / pipe name.
  ///
  /// In en, this message translates to:
  /// **'Enter the agent socket path or pipe name'**
  String get hostAuthErrorAgentPathRequired;

  /// Dialog title: pick a shared credential for the host.
  ///
  /// In en, this message translates to:
  /// **'Use existing credential'**
  String get hostAuthPickerTitle;

  /// Empty state of the credential picker dialog.
  ///
  /// In en, this message translates to:
  /// **'No credentials yet'**
  String get hostAuthPickerEmpty;

  /// Credential picker subtitle part. {username} = user name stored in the credential.
  ///
  /// In en, this message translates to:
  /// **'user {username}'**
  String hostAuthPickerUser(String username);

  /// Credential picker subtitle part. {count} = number of hosts that use the credential.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, other{used by {count} host(s)}}'**
  String hostAuthPickerUsedBy(int count);

  /// Credential picker dialog button: create a new credential.
  ///
  /// In en, this message translates to:
  /// **'New credential…'**
  String get hostAuthPickerNew;

  /// Default title of the host picker dialog.
  ///
  /// In en, this message translates to:
  /// **'Connect to host'**
  String get hostPickerDefaultTitle;

  /// Hint of the search field in the host picker dialog.
  ///
  /// In en, this message translates to:
  /// **'Search hosts'**
  String get hostPickerSearchHint;

  /// Empty state of the host picker dialog.
  ///
  /// In en, this message translates to:
  /// **'No hosts'**
  String get hostPickerEmpty;

  /// Name shown for a hop whose host was deleted.
  ///
  /// In en, this message translates to:
  /// **'(deleted host)'**
  String get jumpChainDeletedHost;

  /// Start of the route preview ('This device → jump → target').
  ///
  /// In en, this message translates to:
  /// **'This device'**
  String get jumpChainThisDevice;

  /// End of the route preview when the target is unknown (jump profile editor).
  ///
  /// In en, this message translates to:
  /// **'target'**
  String get jumpChainTarget;

  /// Empty jump chain editor.
  ///
  /// In en, this message translates to:
  /// **'No hops — add the first jump host below.'**
  String get jumpChainEmpty;

  /// Tooltip: move the hop one position closer to the client.
  ///
  /// In en, this message translates to:
  /// **'Move up'**
  String get jumpChainMoveUp;

  /// Tooltip: remove the hop from the chain.
  ///
  /// In en, this message translates to:
  /// **'Remove hop'**
  String get jumpChainRemoveHop;

  /// Tooltip of the 'Add hop' menu button.
  ///
  /// In en, this message translates to:
  /// **'Add jump host'**
  String get jumpChainAddTooltip;

  /// Menu button: append a jump host to the chain.
  ///
  /// In en, this message translates to:
  /// **'Add hop'**
  String get jumpChainAddHop;

  /// Groups screen title.
  ///
  /// In en, this message translates to:
  /// **'Groups'**
  String get groupsTitle;

  /// Groups screen subtitle.
  ///
  /// In en, this message translates to:
  /// **'Hosts inherit username, port, credential and jump profile from their group chain.'**
  String get groupsSubtitle;

  /// Groups header button and dialog title: create a group.
  ///
  /// In en, this message translates to:
  /// **'New group'**
  String get groupsNewGroup;

  /// Group dialog title when editing.
  ///
  /// In en, this message translates to:
  /// **'Edit group'**
  String get groupsEditGroup;

  /// Empty state of the group tree.
  ///
  /// In en, this message translates to:
  /// **'No groups yet'**
  String get groupsEmpty;

  /// Group tree item subtitle. {count} = hosts directly in the group.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, other{{count} hosts}}'**
  String groupsHostCount(int count);

  /// Group tree item subtitle suffix: the group defines inherited defaults.
  ///
  /// In en, this message translates to:
  /// **'has defaults'**
  String get groupsHasDefaults;

  /// Confirmation dialog title for deleting a group. {name} = group name.
  ///
  /// In en, this message translates to:
  /// **'Delete group {name}?'**
  String groupsDeleteTitle(String name);

  /// Confirmation dialog message for deleting a group.
  ///
  /// In en, this message translates to:
  /// **'Subgroups and hosts move to the parent group. Hosts that inherited settings from this group will inherit from the parent instead.'**
  String get groupsDeleteMessage;

  /// Tooltip: create a subgroup of the selected group.
  ///
  /// In en, this message translates to:
  /// **'Add subgroup'**
  String get groupsAddSubgroup;

  /// Provenance under a group default: set on the group itself.
  ///
  /// In en, this message translates to:
  /// **'set on this group'**
  String get groupsSourceOwn;

  /// Provenance under a group default. {name} = ancestor group name.
  ///
  /// In en, this message translates to:
  /// **'inherited from {name}'**
  String groupsSourceInherited(String name);

  /// Provenance under a group default: nobody sets it.
  ///
  /// In en, this message translates to:
  /// **'not set'**
  String get groupsSourceUnset;

  /// Group default / field label: SSH user name.
  ///
  /// In en, this message translates to:
  /// **'Username'**
  String get groupsUsername;

  /// Group default / field label: SSH port.
  ///
  /// In en, this message translates to:
  /// **'Port'**
  String get groupsPort;

  /// Group default / field label: credential inherited by hosts.
  ///
  /// In en, this message translates to:
  /// **'Credential'**
  String get groupsCredential;

  /// Group default / field label: jump profile inherited by hosts.
  ///
  /// In en, this message translates to:
  /// **'Jump profile'**
  String get groupsJumpProfile;

  /// Group details section title.
  ///
  /// In en, this message translates to:
  /// **'Hosts in this group'**
  String get groupsHostsSection;

  /// Group details: no hosts directly in the group (subgroups not counted).
  ///
  /// In en, this message translates to:
  /// **'No hosts directly in this group.'**
  String get groupsNoHosts;

  /// Group dialog dropdown label.
  ///
  /// In en, this message translates to:
  /// **'Parent group'**
  String get groupsParentGroup;

  /// Parent group dropdown item: top-level group.
  ///
  /// In en, this message translates to:
  /// **'None (top level)'**
  String get groupsNoParent;

  /// Group dialog heading above username/port/credential/jump profile.
  ///
  /// In en, this message translates to:
  /// **'Defaults inherited by hosts (leave empty to inherit from the parent)'**
  String get groupsDefaultsHeading;

  /// Credential dropdown item: inherit from the parent group.
  ///
  /// In en, this message translates to:
  /// **'Inherit'**
  String get groupsInherit;

  /// Jump profile dropdown item: inherit from the parent group or connect directly.
  ///
  /// In en, this message translates to:
  /// **'Inherit / direct'**
  String get groupsJumpInherit;

  /// Card title on the groups screen: named jump chains.
  ///
  /// In en, this message translates to:
  /// **'Jump profiles'**
  String get groupsJumpProfilesSection;

  /// Tooltip and dialog title: create a jump profile.
  ///
  /// In en, this message translates to:
  /// **'New jump profile'**
  String get groupsNewJumpProfile;

  /// Jump profile dialog title when editing.
  ///
  /// In en, this message translates to:
  /// **'Edit jump profile'**
  String get groupsEditJumpProfile;

  /// Jump profiles card text when no profiles exist.
  ///
  /// In en, this message translates to:
  /// **'Reusable ordered chains of jump hosts.'**
  String get groupsJumpProfilesEmpty;

  /// Settings screen title.
  ///
  /// In en, this message translates to:
  /// **'Settings'**
  String get settingsTitle;

  /// Settings > Profiles: title of the rename dialog.
  ///
  /// In en, this message translates to:
  /// **'Rename profile'**
  String get settingsRenameProfileTitle;

  /// Settings > Profiles: confirmation title for removing a profile.
  ///
  /// In en, this message translates to:
  /// **'Remove profile {name} from this device?'**
  String settingsRemoveProfileTitle(String name);

  /// Settings > Profiles: remove confirmation message for a local profile.
  ///
  /// In en, this message translates to:
  /// **'The local database and keys of this profile are deleted from this device. Without a backup the data is gone for good.'**
  String get settingsRemoveProfileLocalMessage;

  /// Settings > Profiles: remove confirmation message for a synced profile.
  ///
  /// In en, this message translates to:
  /// **'Local data of this profile is deleted from this device. The encrypted vault on your server is not touched.'**
  String get settingsRemoveProfileSyncedMessage;

  /// Settings > Profiles: destructive confirm button of the remove dialog.
  ///
  /// In en, this message translates to:
  /// **'Remove profile'**
  String get settingsRemoveProfileConfirm;

  /// Settings: Profiles section title.
  ///
  /// In en, this message translates to:
  /// **'Profiles'**
  String get settingsProfilesTitle;

  /// Settings: Profiles section subtitle.
  ///
  /// In en, this message translates to:
  /// **'Each profile is a separate encrypted vault (e.g. a local personal vault and a synced work vault).'**
  String get settingsProfilesSubtitle;

  /// Settings > Profiles: button to add a profile.
  ///
  /// In en, this message translates to:
  /// **'Add profile'**
  String get settingsAddProfile;

  /// Settings > Profiles: list title of the active profile.
  ///
  /// In en, this message translates to:
  /// **'{name}  (active)'**
  String settingsProfileActive(String name);

  /// Settings > Profiles: button to switch to this profile.
  ///
  /// In en, this message translates to:
  /// **'Switch'**
  String get settingsProfileSwitch;

  /// Settings: Sync section title (local profile).
  ///
  /// In en, this message translates to:
  /// **'Sync'**
  String get settingsSyncTitle;

  /// Settings > Sync: subtitle for a local profile.
  ///
  /// In en, this message translates to:
  /// **'Local only: no server, no account.'**
  String get settingsSyncLocalSubtitle;

  /// Settings > Sync: explanation next to the Enable sync button (local profile).
  ///
  /// In en, this message translates to:
  /// **'Upload this vault end-to-end encrypted to your self-hosted server to use it on other devices.'**
  String get settingsSyncLocalBody;

  /// Settings > Sync: button that opens the enable-sync wizard.
  ///
  /// In en, this message translates to:
  /// **'Enable sync…'**
  String get settingsEnableSync;

  /// Settings: Account & sync section title (synced profile).
  ///
  /// In en, this message translates to:
  /// **'Account & sync'**
  String get settingsAccountSyncTitle;

  /// Settings > Account & sync: label of the server URL.
  ///
  /// In en, this message translates to:
  /// **'Server'**
  String get settingsServerLabel;

  /// Settings > Account & sync: label of the account e-mail.
  ///
  /// In en, this message translates to:
  /// **'Account'**
  String get settingsAccountLabel;

  /// Settings > Account & sync: label of this device's name.
  ///
  /// In en, this message translates to:
  /// **'This device'**
  String get settingsThisDeviceLabel;

  /// Settings > Account & sync: button opening the account password dialog.
  ///
  /// In en, this message translates to:
  /// **'Change account password'**
  String get settingsChangeAccountPassword;

  /// Settings > Account & sync: button that turns a synced profile into a local one.
  ///
  /// In en, this message translates to:
  /// **'Disconnect (keep data locally)…'**
  String get settingsDisconnect;

  /// Settings > Account & sync: sign-out button.
  ///
  /// In en, this message translates to:
  /// **'Sign out'**
  String get settingsSignOut;

  /// Settings > Account & sync: hint below the buttons.
  ///
  /// In en, this message translates to:
  /// **'To use a different server, add another profile; each profile has its own server and vault.'**
  String get settingsDifferentServerHint;

  /// Settings: Vault section title.
  ///
  /// In en, this message translates to:
  /// **'Vault'**
  String get settingsVaultTitle;

  /// Settings > Vault: label of the vault name.
  ///
  /// In en, this message translates to:
  /// **'Name'**
  String get settingsVaultNameLabel;

  /// Label of the vault ID (Settings > Vault and the New Recovery Kit dialog).
  ///
  /// In en, this message translates to:
  /// **'Vault ID'**
  String get settingsVaultIdLabel;

  /// Settings > Vault: label of the auto-lock dropdown.
  ///
  /// In en, this message translates to:
  /// **'Auto-lock'**
  String get settingsAutoLockLabel;

  /// Settings > Vault: auto-lock dropdown item (lower-case).
  ///
  /// In en, this message translates to:
  /// **'{count, plural, =1{after 1 minute} other{after {count} minutes}}'**
  String settingsAutoLockAfterMinutes(int count);

  /// Settings > Vault: auto-lock dropdown item (lower-case).
  ///
  /// In en, this message translates to:
  /// **'{count, plural, =1{after 1 hour} other{after {count} hours}}'**
  String settingsAutoLockAfterHours(int count);

  /// Settings > Vault: auto-lock dropdown item (lower-case).
  ///
  /// In en, this message translates to:
  /// **'never'**
  String get settingsAutoLockNever;

  /// Settings > Vault: button opening the change vault passphrase dialog.
  ///
  /// In en, this message translates to:
  /// **'Change passphrase…'**
  String get settingsChangePassphrase;

  /// Settings > Vault: button opening the new Recovery Kit dialog.
  ///
  /// In en, this message translates to:
  /// **'New Recovery Kit…'**
  String get settingsNewRecoveryKit;

  /// Settings > Vault: button opening the Backups screen.
  ///
  /// In en, this message translates to:
  /// **'Backups…'**
  String get settingsBackups;

  /// Settings > Vault: button that locks the vault.
  ///
  /// In en, this message translates to:
  /// **'Lock now'**
  String get settingsLockNow;

  /// Settings: AI providers section title.
  ///
  /// In en, this message translates to:
  /// **'AI providers'**
  String get settingsAiProvidersTitle;

  /// Settings: AI providers section subtitle.
  ///
  /// In en, this message translates to:
  /// **'The AI can read snippets and host metadata you allow — never passwords, keys or the vault key.'**
  String get settingsAiProvidersSubtitle;

  /// Settings > AI providers: list title of the default provider.
  ///
  /// In en, this message translates to:
  /// **'{name}  · default'**
  String settingsAiProviderDefault(String name);

  /// Settings > AI providers: last part of a provider's subtitle (parts are joined with ' · ').
  ///
  /// In en, this message translates to:
  /// **'API key stored'**
  String get settingsAiProviderApiKeyStored;

  /// Settings > AI providers: label of the default privacy profile selector.
  ///
  /// In en, this message translates to:
  /// **'Default privacy'**
  String get settingsDefaultPrivacyLabel;

  /// Settings > AI providers: switch title.
  ///
  /// In en, this message translates to:
  /// **'Keep AI conversations in the vault'**
  String get settingsKeepAiConversations;

  /// Settings > AI providers: switch subtitle.
  ///
  /// In en, this message translates to:
  /// **'Stored encrypted (and synced for synced profiles). Off: conversations are forgotten.'**
  String get settingsKeepAiConversationsHelp;

  /// Settings: Terminal section title.
  ///
  /// In en, this message translates to:
  /// **'Terminal'**
  String get settingsTerminalTitle;

  /// Settings > Terminal: label above the history mode selector.
  ///
  /// In en, this message translates to:
  /// **'Command history'**
  String get settingsCommandHistoryLabel;

  /// Settings > Terminal: history mode description when encrypted sync is chosen on a local profile.
  ///
  /// In en, this message translates to:
  /// **'{description} (takes effect once sync is enabled)'**
  String settingsHistoryPendingSync(String description);

  /// Settings > Terminal: label of the font size slider.
  ///
  /// In en, this message translates to:
  /// **'Font size'**
  String get settingsFontSizeLabel;

  /// Settings > Terminal: label of the scrollback dropdown.
  ///
  /// In en, this message translates to:
  /// **'Scrollback'**
  String get settingsScrollbackLabel;

  /// Settings > Terminal: scrollback dropdown item.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, =1{{count} line} other{{count} lines}}'**
  String settingsScrollbackLines(int count);

  /// Settings > Terminal: note at the bottom of the section.
  ///
  /// In en, this message translates to:
  /// **'Terminal buffers stay on this device and are never synced.'**
  String get settingsTerminalBuffersNote;

  /// Settings: Appearance & security section title.
  ///
  /// In en, this message translates to:
  /// **'Appearance & security'**
  String get settingsAppearanceSecurityTitle;

  /// Settings > Appearance: label of the theme selector.
  ///
  /// In en, this message translates to:
  /// **'Theme'**
  String get settingsThemeLabel;

  /// Settings > Appearance: theme segment that follows the OS (keep short).
  ///
  /// In en, this message translates to:
  /// **'System'**
  String get settingsThemeSystem;

  /// Settings > Appearance: light theme segment (keep short).
  ///
  /// In en, this message translates to:
  /// **'Light'**
  String get settingsThemeLight;

  /// Settings > Appearance: dark theme segment (keep short).
  ///
  /// In en, this message translates to:
  /// **'Dark'**
  String get settingsThemeDark;

  /// Settings > Appearance & security: label of the clipboard clearing dropdown.
  ///
  /// In en, this message translates to:
  /// **'Clear clipboard'**
  String get settingsClearClipboardLabel;

  /// Settings > Appearance & security: clipboard clearing dropdown item.
  ///
  /// In en, this message translates to:
  /// **'{seconds} s after copying a secret'**
  String settingsClearClipboardAfter(int seconds);

  /// Known hosts page title and sidebar label.
  ///
  /// In en, this message translates to:
  /// **'Known hosts'**
  String get settingsKnownHostsTitle;

  /// Known hosts page subtitle.
  ///
  /// In en, this message translates to:
  /// **'Host keys you trusted. A changed key always blocks the connection until you remove the old one here.'**
  String get settingsKnownHostsSubtitle;

  /// Known hosts page: empty state.
  ///
  /// In en, this message translates to:
  /// **'No known hosts yet.'**
  String get settingsKnownHostsEmpty;

  /// Known hosts page: remove confirmation title.
  ///
  /// In en, this message translates to:
  /// **'Remove {host}?'**
  String settingsRemoveKnownHostTitle(String host);

  /// Known hosts page: remove confirmation message.
  ///
  /// In en, this message translates to:
  /// **'The next connection will ask you to verify the host key again.'**
  String get settingsRemoveKnownHostMessage;

  /// Snackbar after the vault passphrase was changed.
  ///
  /// In en, this message translates to:
  /// **'Vault passphrase changed'**
  String get settingsDialogPassphraseChanged;

  /// Change vault passphrase dialog title.
  ///
  /// In en, this message translates to:
  /// **'Change vault passphrase'**
  String get settingsDialogChangePassphraseTitle;

  /// Change vault passphrase dialog: field label.
  ///
  /// In en, this message translates to:
  /// **'Current passphrase'**
  String get settingsDialogCurrentPassphrase;

  /// Change vault passphrase dialog: field label.
  ///
  /// In en, this message translates to:
  /// **'New passphrase'**
  String get settingsDialogNewPassphrase;

  /// Change vault passphrase dialog: field label.
  ///
  /// In en, this message translates to:
  /// **'Repeat new passphrase'**
  String get settingsDialogRepeatPassphrase;

  /// Change vault passphrase dialog: confirm button.
  ///
  /// In en, this message translates to:
  /// **'Change passphrase'**
  String get settingsDialogChangePassphraseConfirm;

  /// New Recovery Kit dialog title.
  ///
  /// In en, this message translates to:
  /// **'New Recovery Kit'**
  String get settingsDialogNewKitTitle;

  /// New Recovery Kit dialog: warning before generating (security-critical).
  ///
  /// In en, this message translates to:
  /// **'A new Recovery Key replaces the current one: your old Recovery Kit stops working. Save the new kit before closing this window.'**
  String get settingsDialogNewKitWarning;

  /// New Recovery Kit dialog: button revealing the recovery words and QR code.
  ///
  /// In en, this message translates to:
  /// **'Show Recovery Kit'**
  String get settingsDialogShowKit;

  /// New Recovery Kit dialog: closes the dialog after the user saved the kit.
  ///
  /// In en, this message translates to:
  /// **'I saved it'**
  String get settingsDialogKitSaved;

  /// New Recovery Kit dialog: button that generates the new kit.
  ///
  /// In en, this message translates to:
  /// **'Generate new kit'**
  String get settingsDialogGenerateKit;

  /// Snackbar after the account password was changed.
  ///
  /// In en, this message translates to:
  /// **'Account password changed'**
  String get settingsDialogAccountPasswordChanged;

  /// Change account password dialog title.
  ///
  /// In en, this message translates to:
  /// **'Change account password'**
  String get settingsDialogAccountPasswordTitle;

  /// Change account password dialog: field label.
  ///
  /// In en, this message translates to:
  /// **'Current password'**
  String get settingsDialogCurrentPassword;

  /// Change account password dialog: field label.
  ///
  /// In en, this message translates to:
  /// **'New password'**
  String get settingsDialogNewPassword;

  /// Change account password dialog: helper under the new password field.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, =1{At least 1 character} other{At least {count} characters}}'**
  String settingsDialogPasswordMinLength(int count);

  /// Change account password dialog: note.
  ///
  /// In en, this message translates to:
  /// **'This does not change your vault passphrase.'**
  String get settingsDialogVaultPassphraseUnchanged;

  /// AI provider dialog title when adding a provider.
  ///
  /// In en, this message translates to:
  /// **'Add AI provider'**
  String get aiProviderDialogAddTitle;

  /// AI provider dialog title when editing a provider.
  ///
  /// In en, this message translates to:
  /// **'Edit {name}'**
  String aiProviderDialogEditTitle(String name);

  /// AI provider dialog: provider kind dropdown label.
  ///
  /// In en, this message translates to:
  /// **'Provider'**
  String get aiProviderDialogProviderLabel;

  /// AI provider dialog: provider name field label.
  ///
  /// In en, this message translates to:
  /// **'Name'**
  String get aiProviderDialogNameLabel;

  /// AI provider dialog: base URL field label.
  ///
  /// In en, this message translates to:
  /// **'Base URL'**
  String get aiProviderDialogBaseUrlLabel;

  /// AI provider dialog: danger banner for a plain http URL to a remote host.
  ///
  /// In en, this message translates to:
  /// **'Plain http to a remote host: the API key and prompts would travel unencrypted. Use https.'**
  String get aiProviderDialogInsecureHttp;

  /// AI provider dialog: chat model field label.
  ///
  /// In en, this message translates to:
  /// **'Chat model'**
  String get aiProviderDialogChatModelLabel;

  /// AI provider dialog: embedding model field label.
  ///
  /// In en, this message translates to:
  /// **'Embedding model (optional)'**
  String get aiProviderDialogEmbeddingModelLabel;

  /// AI provider dialog: API key field label.
  ///
  /// In en, this message translates to:
  /// **'API key'**
  String get aiProviderDialogApiKeyLabel;

  /// AI provider dialog: API key field hint when a key is already stored.
  ///
  /// In en, this message translates to:
  /// **'A key is stored — type to replace it'**
  String get aiProviderDialogApiKeyStoredHint;

  /// AI provider dialog: API key field hint when no key is stored.
  ///
  /// In en, this message translates to:
  /// **'Optional for local providers'**
  String get aiProviderDialogApiKeyOptionalHint;

  /// AI provider dialog: helper text under the API key field.
  ///
  /// In en, this message translates to:
  /// **'Stored as an encrypted secret in the vault. The AI subsystem cannot read it; it is only placed into the HTTP header of requests.'**
  String get aiProviderDialogApiKeyHelp;

  /// AI provider dialog: checkbox to delete the stored API key.
  ///
  /// In en, this message translates to:
  /// **'Remove the stored API key'**
  String get aiProviderDialogRemoveApiKey;

  /// AI provider dialog: label above the privacy profile selector.
  ///
  /// In en, this message translates to:
  /// **'Privacy profile'**
  String get aiProviderDialogPrivacyProfileLabel;

  /// AI provider dialog: warning when the Local privacy profile is used with a remote host.
  ///
  /// In en, this message translates to:
  /// **'The {profile} profile sends more context. Use it only with models running on machines you control.'**
  String aiProviderDialogLocalProfileWarning(String profile);

  /// AI provider dialog: request timeout field label (seconds).
  ///
  /// In en, this message translates to:
  /// **'Timeout (s)'**
  String get aiProviderDialogTimeoutLabel;

  /// AI provider dialog: filter chip enabling streamed responses.
  ///
  /// In en, this message translates to:
  /// **'Streaming'**
  String get aiProviderDialogStreaming;

  /// AI provider dialog: filter chip enabling tool calling.
  ///
  /// In en, this message translates to:
  /// **'Tool calling'**
  String get aiProviderDialogToolCalling;

  /// AI provider dialog: filter chip making this the default provider.
  ///
  /// In en, this message translates to:
  /// **'Default'**
  String get aiProviderDialogDefault;

  /// AI provider dialog: title of the result banner after a successful test (the provider's own message is shown below).
  ///
  /// In en, this message translates to:
  /// **'Connection test passed'**
  String get aiProviderDialogHealthOk;

  /// AI provider dialog: title of the result banner after a failed test (the provider's own message is shown below).
  ///
  /// In en, this message translates to:
  /// **'Connection test failed'**
  String get aiProviderDialogHealthFailed;

  /// AI provider dialog: list of models reported by the provider.
  ///
  /// In en, this message translates to:
  /// **'Models: {models}'**
  String aiProviderDialogHealthModels(String models);

  /// AI provider dialog: button that saves the provider and tests the connection.
  ///
  /// In en, this message translates to:
  /// **'Save & test'**
  String get aiProviderDialogSaveAndTest;

  /// Snippet variables dialog: heading above the rendered command preview.
  ///
  /// In en, this message translates to:
  /// **'Preview'**
  String get runFlowVariablesPreview;

  /// Run confirmation dialog title (shows host, command and risk).
  ///
  /// In en, this message translates to:
  /// **'Run command?'**
  String get runFlowConfirmTitle;

  /// Run confirmation: why local risk rules flagged the command. {reasons} is a '; '-separated list of diagnostics from the risk engine (not translated).
  ///
  /// In en, this message translates to:
  /// **'Local rules: {reasons}.'**
  String runFlowLocalRules(String reasons);

  /// Run confirmation: the snippet's/AI's declared risk differs from the local rules. {declared} and {effective} are risk level names (Read-only, Modifying, Destructive, Unknown).
  ///
  /// In en, this message translates to:
  /// **'Declared as {declared}; local rules rate it {effective}.'**
  String runFlowDeclaredVsLocal(String declared, String effective);

  /// SECURITY-CRITICAL. Checkbox the user must tick before running a DESTRUCTIVE command. {host} is 'name (address)' of the target host.
  ///
  /// In en, this message translates to:
  /// **'I understand this command can delete or destroy data on {host}'**
  String runFlowAckDestructive(String host);

  /// SECURITY-CRITICAL. Checkbox the user must tick before running a MODIFYING command. {host} is 'name (address)' of the target host.
  ///
  /// In en, this message translates to:
  /// **'I understand this command changes the state of {host}'**
  String runFlowAckModifying(String host);

  /// SECURITY-CRITICAL. Checkbox the user must tick before running a command whose risk local rules cannot determine.
  ///
  /// In en, this message translates to:
  /// **'I reviewed this command; its effect is not known to local rules'**
  String get runFlowAckUnknown;

  /// Run confirmation dialog: button that executes the command on the host.
  ///
  /// In en, this message translates to:
  /// **'Run'**
  String get runFlowRun;

  /// Host picker title when running a command without an active terminal tab.
  ///
  /// In en, this message translates to:
  /// **'Run on which host?'**
  String get runFlowPickHostTitle;

  /// Snackbar: Insert needs an active terminal tab.
  ///
  /// In en, this message translates to:
  /// **'Open a terminal tab first.'**
  String get runFlowOpenTerminalFirst;

  /// Snippet editor dialog title when creating.
  ///
  /// In en, this message translates to:
  /// **'New snippet'**
  String get snippetEditorTitleNew;

  /// Snippet editor dialog title when editing.
  ///
  /// In en, this message translates to:
  /// **'Edit snippet'**
  String get snippetEditorTitleEdit;

  /// Snippet editor banner when the snippet was pre-filled by the AI.
  ///
  /// In en, this message translates to:
  /// **'Drafted by AI. Review the command and its risk before saving.'**
  String get snippetEditorAiDraftBanner;

  /// Snippet editor field label: command template with {{variables}}.
  ///
  /// In en, this message translates to:
  /// **'Command template'**
  String get snippetEditorTemplateLabel;

  /// Snippet editor field label: shell or dialect (bash, pwsh7, psql).
  ///
  /// In en, this message translates to:
  /// **'Shell / dialect (optional)'**
  String get snippetEditorShellLabel;

  /// Snippet editor section header: template variables.
  ///
  /// In en, this message translates to:
  /// **'Variables'**
  String get snippetEditorVariables;

  /// Snippet editor field label: default value of a template variable.
  ///
  /// In en, this message translates to:
  /// **'Default'**
  String get snippetEditorVariableDefault;

  /// Snippet editor section header: declared risk level selector.
  ///
  /// In en, this message translates to:
  /// **'Risk'**
  String get snippetEditorRisk;

  /// Snippet editor: label before the effective risk badge (keep the trailing space).
  ///
  /// In en, this message translates to:
  /// **'Effective: '**
  String get snippetEditorEffective;

  /// Snippet editor: local rules verdict. {detail} is either a risk level name or a '; '-separated list of risk engine diagnostics (not translated).
  ///
  /// In en, this message translates to:
  /// **'Local rules: {detail}'**
  String snippetEditorLocalRules(String detail);

  /// Snippet picker dialog title (terminal toolbar).
  ///
  /// In en, this message translates to:
  /// **'Run snippet'**
  String get snippetPickerTitle;

  /// Snippet picker search field hint.
  ///
  /// In en, this message translates to:
  /// **'Search snippets'**
  String get snippetPickerSearchHint;

  /// Field label: snippet language/type (Bash, PowerShell, kubectl…); used in the editor and as the list filter.
  ///
  /// In en, this message translates to:
  /// **'Type'**
  String get snippetsTypeLabel;

  /// Snippets screen subtitle. {syntax} is the literal template syntax '{{variables}}' (not translated).
  ///
  /// In en, this message translates to:
  /// **'Reusable commands with {syntax}. Running always shows the command, host and risk first.'**
  String snippetsSubtitle(String syntax);

  /// Snippets screen button: create a snippet.
  ///
  /// In en, this message translates to:
  /// **'New snippet'**
  String get snippetsNewButton;

  /// Snippets screen search hint (full-text + semantic search).
  ///
  /// In en, this message translates to:
  /// **'Search text or intent, e.g. \"disk space\"'**
  String get snippetsSearchHint;

  /// Snippets screen type filter: no filter.
  ///
  /// In en, this message translates to:
  /// **'All types'**
  String get snippetsAllTypes;

  /// Snippets screen empty state title.
  ///
  /// In en, this message translates to:
  /// **'No snippets yet'**
  String get snippetsEmptyTitle;

  /// Snippets screen empty state message.
  ///
  /// In en, this message translates to:
  /// **'Save commands you use often — or let AI draft them.'**
  String get snippetsEmptyMessage;

  /// Snippets screen: search/filter has no results.
  ///
  /// In en, this message translates to:
  /// **'Nothing found'**
  String get snippetsNothingFound;

  /// Tooltip of the icon marking a snippet created by the AI.
  ///
  /// In en, this message translates to:
  /// **'Drafted by AI'**
  String get snippetsDraftedByAi;

  /// Tooltip of the icon marking a search result found by meaning rather than text.
  ///
  /// In en, this message translates to:
  /// **'Semantic match'**
  String get snippetsSemanticMatch;

  /// Snippet list: how many times the snippet was run.
  ///
  /// In en, this message translates to:
  /// **'used {count}×'**
  String snippetsUsedCount(int count);

  /// Tooltip: type the snippet into the active terminal without pressing Enter.
  ///
  /// In en, this message translates to:
  /// **'Insert into terminal'**
  String get snippetsInsertIntoTerminal;

  /// Snippet list button: run the snippet (always asks for confirmation).
  ///
  /// In en, this message translates to:
  /// **'Run'**
  String get snippetsRun;

  /// Delete snippet confirmation title. {name} is the snippet name.
  ///
  /// In en, this message translates to:
  /// **'Delete {name}?'**
  String snippetsDeleteTitle(String name);

  /// Delete snippet confirmation message.
  ///
  /// In en, this message translates to:
  /// **'The snippet is removed from this vault.'**
  String get snippetsDeleteMessage;

  /// AI chat screen subtitle. 'Insert' and 'Run…' are the command card buttons (codeBlocksInsert, codeBlocksRun).
  ///
  /// In en, this message translates to:
  /// **'Answers never run by themselves: Insert or Run… always asks you first.'**
  String get aiChatSubtitle;

  /// AI chat: AI provider selector label.
  ///
  /// In en, this message translates to:
  /// **'Provider'**
  String get aiChatProviderLabel;

  /// AI chat tooltip: clear and start a new conversation.
  ///
  /// In en, this message translates to:
  /// **'New conversation'**
  String get aiChatNewConversation;

  /// AI chat notice for an off-device provider. {profile} is the privacy profile name (Strict/Standard/Local), {description} its description.
  ///
  /// In en, this message translates to:
  /// **'Remote provider · privacy profile {profile}: {description}. Passwords and keys are never sent.'**
  String aiChatRemoteProviderNotice(String profile, String description);

  /// AI chat notice for a local model provider.
  ///
  /// In en, this message translates to:
  /// **'Local model · nothing leaves this machine.'**
  String get aiChatLocalModelNotice;

  /// AI chat warning banner when no AI provider exists.
  ///
  /// In en, this message translates to:
  /// **'No AI provider configured.'**
  String get aiChatNoProviderBanner;

  /// AI chat empty state title.
  ///
  /// In en, this message translates to:
  /// **'Ask anything about your servers'**
  String get aiChatEmptyTitle;

  /// AI chat empty state: example prompts.
  ///
  /// In en, this message translates to:
  /// **'e.g. \"find the biggest files in /var/log\" or \"restart nginx safely\"'**
  String get aiChatEmptyMessage;

  /// AI chat input hint.
  ///
  /// In en, this message translates to:
  /// **'Message (Enter to send, Shift+Enter for a new line)'**
  String get aiChatInputHint;

  /// AI chat button: stop the streaming answer.
  ///
  /// In en, this message translates to:
  /// **'Stop'**
  String get aiChatStop;

  /// AI chat button: send the message.
  ///
  /// In en, this message translates to:
  /// **'Send'**
  String get aiChatSend;

  /// AI chat checkbox: attach the text selected in the active terminal.
  ///
  /// In en, this message translates to:
  /// **'Include selected terminal text'**
  String get aiChatIncludeSelection;

  /// AI chat checkbox label when no terminal text is selected.
  ///
  /// In en, this message translates to:
  /// **'Select text in a terminal to include it'**
  String get aiChatSelectToInclude;

  /// AI chat error when sending without any AI provider.
  ///
  /// In en, this message translates to:
  /// **'No AI provider configured. Add one in Settings → AI providers.'**
  String get aiChatErrorNoProvider;

  /// AI chat / command palette error when the AI request fails (a raw provider diagnostic may follow).
  ///
  /// In en, this message translates to:
  /// **'The AI request failed.'**
  String get aiChatErrorRequestFailed;

  /// Appended to a partial AI answer after the user pressed Stop.
  ///
  /// In en, this message translates to:
  /// **'…(stopped)'**
  String get aiChatStoppedSuffix;

  /// Command palette item: ask the AI to generate a command. {query} is what the user typed.
  ///
  /// In en, this message translates to:
  /// **'Generate command: \"{query}\"'**
  String paletteGenerateCommand(String query);

  /// Command palette: subtitle of the generate-command item.
  ///
  /// In en, this message translates to:
  /// **'Ask the AI (never runs automatically)'**
  String get paletteGenerateSubtitle;

  /// Command palette quick action: create a host.
  ///
  /// In en, this message translates to:
  /// **'New host'**
  String get paletteNewHost;

  /// Command palette quick action: go to AI chat.
  ///
  /// In en, this message translates to:
  /// **'Open AI chat'**
  String get paletteOpenAiChat;

  /// Command palette quick action: lock the vault.
  ///
  /// In en, this message translates to:
  /// **'Lock vault'**
  String get paletteLockVault;

  /// Command palette input hint.
  ///
  /// In en, this message translates to:
  /// **'Search snippets, run an action, or describe a command…'**
  String get paletteSearchHint;

  /// Command palette input hint when terminal text is selected.
  ///
  /// In en, this message translates to:
  /// **'Ask about the selected terminal output…'**
  String get paletteAskSelectionHint;

  /// Command palette: header of the AI-generated command panel.
  ///
  /// In en, this message translates to:
  /// **'AI suggestion'**
  String get paletteAiSuggestion;

  /// Command palette: AI is generating, nothing streamed yet.
  ///
  /// In en, this message translates to:
  /// **'Thinking…'**
  String get paletteThinking;

  /// Command palette error when generating without any AI provider.
  ///
  /// In en, this message translates to:
  /// **'No AI provider configured. Add Ollama, LM Studio or DeepSeek in Settings.'**
  String get paletteErrorNoProvider;

  /// Command palette: the AI's risk guess differs from local rules. {aiRisk} and {localRisk} are risk level names.
  ///
  /// In en, this message translates to:
  /// **'The AI rated this {aiRisk}; local rules say {localRisk}.'**
  String paletteRiskMismatch(String aiRisk, String localRisk);

  /// AI command card button: type the command into the active terminal (no Enter).
  ///
  /// In en, this message translates to:
  /// **'Insert'**
  String get codeBlocksInsert;

  /// AI command card button: run after confirmation.
  ///
  /// In en, this message translates to:
  /// **'Run…'**
  String get codeBlocksRun;

  /// AI command card button: save the command as a snippet.
  ///
  /// In en, this message translates to:
  /// **'Save as snippet'**
  String get codeBlocksSaveAsSnippet;

  /// Snackbar after saving an AI command as a snippet. {name} is the snippet name.
  ///
  /// In en, this message translates to:
  /// **'Snippet \"{name}\" saved'**
  String codeBlocksSnippetSaved(String name);

  /// Privacy line under an AI answer from a local model. {profile} is the privacy profile name.
  ///
  /// In en, this message translates to:
  /// **'Processed by a local model · {profile} profile'**
  String codeBlocksProcessedLocally(String profile);

  /// Privacy line under an AI answer from a remote provider. {profile} is the privacy profile name, {count} the number of redacted items.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, =1{Sanitized ({profile}) before leaving this device · {count} item redacted} other{Sanitized ({profile}) before leaving this device · {count} items redacted}}'**
  String codeBlocksSanitized(int count, String profile);

  /// Terminal screen empty state title when no tab is open.
  ///
  /// In en, this message translates to:
  /// **'No open sessions'**
  String get terminalEmptyTitle;

  /// Terminal screen empty state message. {shortcut} is the keyboard shortcut of the "New terminal tab" command (e.g. ⌘T).
  ///
  /// In en, this message translates to:
  /// **'Connect to a host to open a terminal tab ({shortcut}).'**
  String terminalEmptyMessage(String shortcut);

  /// Terminal empty state button: pick a host and open a terminal tab.
  ///
  /// In en, this message translates to:
  /// **'Connect to host'**
  String get terminalConnectToHost;

  /// Tooltip of the × button on a terminal tab; also the button in the changed-host-key banner.
  ///
  /// In en, this message translates to:
  /// **'Close tab'**
  String get terminalCloseTab;

  /// Tooltip of the + button in the terminal tab strip. {shortcut} is the keyboard shortcut (e.g. ⌘T).
  ///
  /// In en, this message translates to:
  /// **'New tab ({shortcut})'**
  String terminalNewTabTooltip(String shortcut);

  /// Tooltip on the disabled split-view icon in the terminal tab strip.
  ///
  /// In en, this message translates to:
  /// **'Split view arrives in a later release'**
  String get terminalSplitViewLater;

  /// Terminal session toolbar button: open the snippet picker.
  ///
  /// In en, this message translates to:
  /// **'Snippets'**
  String get terminalSnippets;

  /// Terminal session toolbar button: open the AI command palette.
  ///
  /// In en, this message translates to:
  /// **'Ask AI'**
  String get terminalAskAi;

  /// SECURITY-CRITICAL. Title of the danger banner when the server presents a host key different from the recorded one (connection is blocked). Upper case is intentional. {host} is the known_hosts pattern (host or [host]:port).
  ///
  /// In en, this message translates to:
  /// **'HOST KEY CHANGED for {host}'**
  String terminalHostKeyChangedTitle(String host);

  /// SECURITY-CRITICAL. Body of the changed-host-key banner (two lines). Must say that the key changed, that this may be a man-in-the-middle attack and that the connection is blocked. {keyType} is the SSH key type, {fingerprint} the SHA-256 fingerprint of the presented key. "Settings → Known hosts" names the settings section.
  ///
  /// In en, this message translates to:
  /// **'Host key verification failed: the server key CHANGED. This can mean a man-in-the-middle attack. Connection refused.\nPresented: {keyType} {fingerprint}. If the server was really reinstalled, remove the old key in Settings → Known hosts and reconnect.'**
  String terminalHostKeyChangedMessage(String keyType, String fingerprint);

  /// SECURITY-CRITICAL. Title of the unknown-host-key prompt shown on first connect. {host} is the known_hosts pattern.
  ///
  /// In en, this message translates to:
  /// **'Unknown host {host}'**
  String terminalUnknownHostTitle(String host);

  /// SECURITY-CRITICAL. Explanation in the unknown-host-key prompt (the fingerprint is shown below it).
  ///
  /// In en, this message translates to:
  /// **'The authenticity of this host cannot be established. Verify the fingerprint with the server administrator before trusting it.'**
  String get terminalUnknownHostMessage;

  /// Unknown-host-key prompt button: reject the key and do not connect.
  ///
  /// In en, this message translates to:
  /// **'Reject'**
  String get terminalHostKeyReject;

  /// Unknown-host-key prompt button: trust the key for this connection only (not saved).
  ///
  /// In en, this message translates to:
  /// **'Accept once'**
  String get terminalHostKeyAcceptOnce;

  /// Unknown-host-key prompt button: trust the key and save it to known hosts.
  ///
  /// In en, this message translates to:
  /// **'Accept and save'**
  String get terminalHostKeyAcceptAndSave;

  /// Terminal status line while connecting (no route known).
  ///
  /// In en, this message translates to:
  /// **'Connecting'**
  String get terminalConnecting;

  /// Terminal status line while connecting. {route} is a technical description from the SSH core (user@host:port and jump hosts), not translated.
  ///
  /// In en, this message translates to:
  /// **'Connecting {route}'**
  String terminalConnectingRoute(String route);

  /// Terminal status line while reconnecting (no route known).
  ///
  /// In en, this message translates to:
  /// **'Reconnecting'**
  String get terminalReconnecting;

  /// Terminal status line while reconnecting. {route} is a technical description from the SSH core, not translated.
  ///
  /// In en, this message translates to:
  /// **'Reconnecting {route}'**
  String terminalReconnectingRoute(String route);

  /// Title of the banner when the remote shell exited. The raw reason from the SSH core is shown below it.
  ///
  /// In en, this message translates to:
  /// **'Session ended'**
  String get terminalSessionEnded;

  /// Title of the banner when the SSH connection was lost or refused. The raw reason from the SSH core is shown below it.
  ///
  /// In en, this message translates to:
  /// **'Disconnected'**
  String get terminalDisconnected;

  /// Disconnected banner message when the SSH core gave no reason.
  ///
  /// In en, this message translates to:
  /// **'The connection was closed.'**
  String get terminalConnectionClosed;

  /// Disconnected banner button: reconnect the session.
  ///
  /// In en, this message translates to:
  /// **'Reconnect'**
  String get terminalReconnect;

  /// Title of the connect-time password prompt. {user} is the SSH user name, {host} the host label.
  ///
  /// In en, this message translates to:
  /// **'Password for {user}@{host}'**
  String terminalPasswordPromptTitle(String user, String host);

  /// Connect-time password prompt: the previous password was rejected by the server.
  ///
  /// In en, this message translates to:
  /// **'Permission denied, please try again.'**
  String get terminalPasswordRetry;

  /// Field label in the connect-time password prompt.
  ///
  /// In en, this message translates to:
  /// **'Password'**
  String get terminalPasswordLabel;

  /// Helper under the connect-time password field.
  ///
  /// In en, this message translates to:
  /// **'Used for this connection only — not saved. Save it in the host settings to skip this step.'**
  String get terminalPasswordHelper;

  /// Title of the host picker opened by the SFTP Connect button.
  ///
  /// In en, this message translates to:
  /// **'Open SFTP'**
  String get sftpOpenPickerTitle;

  /// SFTP page subtitle when not connected.
  ///
  /// In en, this message translates to:
  /// **'Browse and transfer files over SSH.'**
  String get sftpSubtitle;

  /// SFTP page button: close the SFTP session.
  ///
  /// In en, this message translates to:
  /// **'Disconnect'**
  String get sftpDisconnect;

  /// SFTP empty state title.
  ///
  /// In en, this message translates to:
  /// **'Not connected'**
  String get sftpNotConnectedTitle;

  /// SFTP empty state message.
  ///
  /// In en, this message translates to:
  /// **'Pick a host to browse, transfer and edit its files.'**
  String get sftpNotConnectedMessage;

  /// Title of the local (left) file pane in SFTP.
  ///
  /// In en, this message translates to:
  /// **'This device'**
  String get sftpLocalPaneTitle;

  /// Tooltip: upload the selected local file into the current remote directory.
  ///
  /// In en, this message translates to:
  /// **'Upload selected file'**
  String get sftpUploadTooltip;

  /// Tooltip: download the selected remote file into the current local folder.
  ///
  /// In en, this message translates to:
  /// **'Download selected file'**
  String get sftpDownloadTooltip;

  /// Tooltip and dialog title: create a directory on the server (remote side → «каталог»).
  ///
  /// In en, this message translates to:
  /// **'New folder'**
  String get sftpNewFolder;

  /// Confirmation dialog title for deleting a remote file or directory. {name} is the file name.
  ///
  /// In en, this message translates to:
  /// **'Delete {name}?'**
  String sftpDeleteTitle(String name);

  /// Confirmation message for deleting a remote directory recursively.
  ///
  /// In en, this message translates to:
  /// **'The folder and everything inside it are deleted on the server.'**
  String get sftpDeleteFolderMessage;

  /// Confirmation message for deleting a remote file.
  ///
  /// In en, this message translates to:
  /// **'The file is deleted on the server.'**
  String get sftpDeleteFileMessage;

  /// Tooltip: reload the file list of a pane.
  ///
  /// In en, this message translates to:
  /// **'Refresh'**
  String get sftpRefresh;

  /// Tooltip: go to the parent directory.
  ///
  /// In en, this message translates to:
  /// **'Up'**
  String get sftpUp;

  /// Transfers list: finished transfer. {size} is the formatted total size (e.g. 1.4 MiB).
  ///
  /// In en, this message translates to:
  /// **'Done · {size}'**
  String sftpTransferDone(String size);

  /// Transfers list: failed transfer. {detail} is the raw (English) diagnostic from the SFTP core, may be empty.
  ///
  /// In en, this message translates to:
  /// **'Failed: {detail}'**
  String sftpTransferFailed(String detail);

  /// Transfers list: cancelled transfer.
  ///
  /// In en, this message translates to:
  /// **'Cancelled'**
  String get sftpTransferCancelled;

  /// Transfers list: progress of a running transfer. {done} and {total} are formatted sizes.
  ///
  /// In en, this message translates to:
  /// **'{done} of {total}'**
  String sftpTransferProgress(String done, String total);

  /// Transfers list: transfer speed per second, shown after the progress. {size} is a formatted size.
  ///
  /// In en, this message translates to:
  /// **'{size}/s'**
  String sftpTransferSpeed(String size);

  /// SFTP toolbar button / action: preview the selected file (text or image) in memory.
  ///
  /// In en, this message translates to:
  /// **'Quick Look'**
  String get sftpToolbarQuickLook;

  /// SFTP toolbar menu button with the file actions (same as the context menu).
  ///
  /// In en, this message translates to:
  /// **'Actions'**
  String get sftpToolbarActions;

  /// SFTP toolbar button and drawer tab: list of uploads/downloads.
  ///
  /// In en, this message translates to:
  /// **'Transfers'**
  String get sftpToolbarTransfers;

  /// SFTP toolbar button and drawer tab: files opened in an external editor (edit sessions).
  ///
  /// In en, this message translates to:
  /// **'Editing'**
  String get sftpToolbarEditing;

  /// SFTP toolbar toggle tooltip: show the local file pane next to the remote list.
  ///
  /// In en, this message translates to:
  /// **'Show local files side by side'**
  String get sftpToolbarShowLocal;

  /// SFTP toolbar toggle tooltip: hide the local file pane.
  ///
  /// In en, this message translates to:
  /// **'Hide local files'**
  String get sftpToolbarHideLocal;

  /// SFTP toolbar tooltip: pick another host for the browser.
  ///
  /// In en, this message translates to:
  /// **'Connect to another host'**
  String get sftpToolbarSwitchHost;

  /// SFTP toolbar search field placeholder: filters the current remote folder by name.
  ///
  /// In en, this message translates to:
  /// **'Search this folder'**
  String get sftpSearchHint;

  /// Tooltip of the clear button in the SFTP search field.
  ///
  /// In en, this message translates to:
  /// **'Clear search'**
  String get sftpSearchClear;

  /// SFTP screen while connecting. {host} is the host name.
  ///
  /// In en, this message translates to:
  /// **'Connecting to {host}…'**
  String sftpConnecting(String host);

  /// Placeholder of the editable SFTP path field (paths stay as is).
  ///
  /// In en, this message translates to:
  /// **'Type a path, e.g. /var/www or ~/logs'**
  String get sftpPathHint;

  /// Tooltip of the SFTP breadcrumb bar. {shortcut} is e.g. ⌘L or Ctrl+L.
  ///
  /// In en, this message translates to:
  /// **'Click or press {shortcut} to type a path'**
  String sftpPathEditTooltip(String shortcut);

  /// Menu item: copy the remote path (breadcrumb segment or selected items) to the clipboard.
  ///
  /// In en, this message translates to:
  /// **'Copy path'**
  String get sftpCopyPath;

  /// Noun for the copied notice ('{what} copied.'); Russian is lower-case like the other copyWhat* nouns.
  ///
  /// In en, this message translates to:
  /// **'Path'**
  String get sftpCopyWhatPath;

  /// Noun for the copied notice ('{what} copied.'); Russian is lower-case like the other copyWhat* nouns.
  ///
  /// In en, this message translates to:
  /// **'Name'**
  String get sftpCopyWhatName;

  /// SFTP file list column header.
  ///
  /// In en, this message translates to:
  /// **'Name'**
  String get sftpColumnName;

  /// SFTP file list column header.
  ///
  /// In en, this message translates to:
  /// **'Size'**
  String get sftpColumnSize;

  /// SFTP file list column header: file kind (Folder, PHP, PNG image…).
  ///
  /// In en, this message translates to:
  /// **'Kind'**
  String get sftpColumnKind;

  /// SFTP file list column header: last modification time.
  ///
  /// In en, this message translates to:
  /// **'Date Modified'**
  String get sftpColumnModified;

  /// SFTP file list column header: Unix permissions (drwxr-xr-x).
  ///
  /// In en, this message translates to:
  /// **'Permissions'**
  String get sftpColumnPermissions;

  /// SFTP file list column header and Get Info row: owning user.
  ///
  /// In en, this message translates to:
  /// **'Owner'**
  String get sftpColumnOwner;

  /// SFTP file list column header and Get Info row: owning group.
  ///
  /// In en, this message translates to:
  /// **'Group'**
  String get sftpColumnGroup;

  /// Column header menu toggle: keep folders above files whatever the sort order.
  ///
  /// In en, this message translates to:
  /// **'Folders first'**
  String get sftpFoldersFirst;

  /// Column header menu toggle: show dot-files.
  ///
  /// In en, this message translates to:
  /// **'Show hidden files'**
  String get sftpShowHidden;

  /// Column header menu item: restore default columns, order and widths.
  ///
  /// In en, this message translates to:
  /// **'Reset columns'**
  String get sftpResetColumns;

  /// Accessibility label of a folder disclosure triangle (collapsed).
  ///
  /// In en, this message translates to:
  /// **'Expand'**
  String get sftpExpand;

  /// Accessibility label of a folder disclosure triangle (expanded).
  ///
  /// In en, this message translates to:
  /// **'Collapse'**
  String get sftpCollapse;

  /// SFTP file list: the remote folder has no (visible) entries.
  ///
  /// In en, this message translates to:
  /// **'This folder is empty'**
  String get sftpEmptyFolder;

  /// SFTP file list: the search filter matched nothing. {query} is the typed text.
  ///
  /// In en, this message translates to:
  /// **'Nothing in this folder matches “{query}”'**
  String sftpNoMatches(String query);

  /// Tooltip on the symlink badge of a file icon. {target} is the link text.
  ///
  /// In en, this message translates to:
  /// **'Symbolic link → {target}'**
  String sftpSymlinkTooltip(String target);

  /// Tooltip / label: symlink whose target does not exist.
  ///
  /// In en, this message translates to:
  /// **'Broken link'**
  String get sftpDanglingLink;

  /// Kind column: a remote directory (remote side → «каталог»).
  ///
  /// In en, this message translates to:
  /// **'Folder'**
  String get sftpKindFolder;

  /// Kind column: plain text file.
  ///
  /// In en, this message translates to:
  /// **'Plain text'**
  String get sftpKindText;

  /// Kind column: file of an unknown type.
  ///
  /// In en, this message translates to:
  /// **'Document'**
  String get sftpKindDocument;

  /// Kind column: office document. {format} is e.g. Word, Excel, RTF (not translated).
  ///
  /// In en, this message translates to:
  /// **'{format} document'**
  String sftpKindDocumentFormat(String format);

  /// Kind column: file with the execute bit and no known extension.
  ///
  /// In en, this message translates to:
  /// **'Executable'**
  String get sftpKindExecutable;

  /// Kind column: .sh/.bash files and shell dot-files.
  ///
  /// In en, this message translates to:
  /// **'Shell script'**
  String get sftpKindShellScript;

  /// Kind column: image. {format} is PNG, JPEG… (not translated).
  ///
  /// In en, this message translates to:
  /// **'{format} image'**
  String sftpKindImage(String format);

  /// Kind column: archive. {format} is ZIP, TAR.GZ… (not translated).
  ///
  /// In en, this message translates to:
  /// **'{format} archive'**
  String sftpKindArchive(String format);

  /// Kind column: .log files.
  ///
  /// In en, this message translates to:
  /// **'Log file'**
  String get sftpKindLog;

  /// Kind column: configuration files (.conf, .ini, .htaccess…).
  ///
  /// In en, this message translates to:
  /// **'Configuration'**
  String get sftpKindConfig;

  /// Kind column: PDF file.
  ///
  /// In en, this message translates to:
  /// **'PDF document'**
  String get sftpKindPdf;

  /// Kind column: audio file.
  ///
  /// In en, this message translates to:
  /// **'Audio'**
  String get sftpKindAudio;

  /// Kind column: video file.
  ///
  /// In en, this message translates to:
  /// **'Video'**
  String get sftpKindVideo;

  /// Kind column: font file.
  ///
  /// In en, this message translates to:
  /// **'Font'**
  String get sftpKindFont;

  /// Kind column: .pem/.key/.crt, authorized_keys, known_hosts.
  ///
  /// In en, this message translates to:
  /// **'Key or certificate'**
  String get sftpKindKey;

  /// Kind column: database file (.sqlite, .db).
  ///
  /// In en, this message translates to:
  /// **'Database'**
  String get sftpKindDatabase;

  /// Kind column: symlink whose target is missing.
  ///
  /// In en, this message translates to:
  /// **'Symbolic link'**
  String get sftpKindSymlink;

  /// Kind column for a symlink: {kind} is the kind of its target (e.g. Folder).
  ///
  /// In en, this message translates to:
  /// **'Link → {kind}'**
  String sftpKindSymlinkTo(String kind);

  /// Kind column: socket, device or FIFO.
  ///
  /// In en, this message translates to:
  /// **'Special file'**
  String get sftpKindSpecial;

  /// Status bar: number of folders in the current view (part of sftpStatusSummary).
  ///
  /// In en, this message translates to:
  /// **'{count, plural, one{{count} folder} other{{count} folders}}'**
  String sftpStatusFolders(int count);

  /// Status bar: number of files in the current view (part of sftpStatusSummary).
  ///
  /// In en, this message translates to:
  /// **'{count, plural, one{{count} file} other{{count} files}}'**
  String sftpStatusFiles(int count);

  /// Status bar: "4 folders, 18 files, 188 KiB". {folders}/{files} are sftpStatusFolders/sftpStatusFiles, {size} a formatted size.
  ///
  /// In en, this message translates to:
  /// **'{folders}, {files}, {size}'**
  String sftpStatusSummary(String folders, String files, String size);

  /// Status bar with a selection. {selected}/{total} are formatted counts, {size} the size of the selected files.
  ///
  /// In en, this message translates to:
  /// **'{selected} of {total} selected, {size}'**
  String sftpStatusSelection(String selected, String total, String size);

  /// Status bar: number of active edit sessions (click opens the Editing panel).
  ///
  /// In en, this message translates to:
  /// **'{count, plural, one{Editing {count} file} other{Editing {count} files}}'**
  String sftpStatusEditing(int count);

  /// Tooltip of the lock + SFTP indicator in the status bar.
  ///
  /// In en, this message translates to:
  /// **'Encrypted SFTP connection over SSH'**
  String get sftpStatusSecure;

  /// Native application bundle picker type label.
  ///
  /// In en, this message translates to:
  /// **'Applications'**
  String get sftpApplications;

  /// Native application bundle picker confirmation button.
  ///
  /// In en, this message translates to:
  /// **'Choose Application'**
  String get sftpChooseApplication;

  /// External editor launch failure with a bounded OS diagnostic; no file contents.
  ///
  /// In en, this message translates to:
  /// **'Could not open the editor. Choose another application using Open With. System message: {detail}'**
  String errorEditorLaunchFailed(String detail);

  /// Native application picker failure with the OS diagnostic.
  ///
  /// In en, this message translates to:
  /// **'Could not show the application picker. System message: {detail}'**
  String errorApplicationPickerFailed(String detail);

  /// Action: open the file for editing in an application chosen in the system dialog.
  ///
  /// In en, this message translates to:
  /// **'Open With…'**
  String get sftpActionOpenWith;

  /// Action: download the selected items to a local folder (asks for the folder).
  ///
  /// In en, this message translates to:
  /// **'Download…'**
  String get sftpActionDownload;

  /// Action: upload a local file into the current (or selected) remote folder.
  ///
  /// In en, this message translates to:
  /// **'Upload here…'**
  String get sftpActionUploadHere;

  /// Action: create an empty file in the current remote folder.
  ///
  /// In en, this message translates to:
  /// **'New file'**
  String get sftpActionNewFile;

  /// Action: copy the selected items next to themselves on the server.
  ///
  /// In en, this message translates to:
  /// **'Duplicate'**
  String get sftpActionDuplicate;

  /// Action: details and permissions of the selected item.
  ///
  /// In en, this message translates to:
  /// **'Get Info'**
  String get sftpActionGetInfo;

  /// Action: copy the names of the selected items.
  ///
  /// In en, this message translates to:
  /// **'Copy name'**
  String get sftpActionCopyName;

  /// Default name of a new remote folder (renamed inline right away).
  ///
  /// In en, this message translates to:
  /// **'untitled folder'**
  String get sftpUntitledFolder;

  /// Default name of a new remote file (renamed inline right away); keep the .txt extension.
  ///
  /// In en, this message translates to:
  /// **'untitled.txt'**
  String get sftpUntitledFile;

  /// Suffix of duplicated files: "index copy.php".
  ///
  /// In en, this message translates to:
  /// **'copy'**
  String get sftpDuplicateSuffix;

  /// Error after inline rename with an invalid name.
  ///
  /// In en, this message translates to:
  /// **'A name can’t be empty or contain “/”.'**
  String get sftpNameInvalid;

  /// Confirmation title for deleting several remote items.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, one{Delete {count} item?} other{Delete {count} items?}}'**
  String sftpDeleteManyTitle(int count);

  /// Confirmation message for deleting several remote items.
  ///
  /// In en, this message translates to:
  /// **'The selected items are deleted on the server, folders with everything inside them. This can’t be undone.'**
  String get sftpDeleteManyMessage;

  /// Snackbar after starting uploads. {folder} is the remote folder path.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, one{Uploading {count} item to {folder}} other{Uploading {count} items to {folder}}}'**
  String sftpUploadStarted(int count, String folder);

  /// Snackbar after starting downloads. {folder} is the local folder path.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, one{Downloading {count} item to {folder}} other{Downloading {count} items to {folder}}}'**
  String sftpDownloadStarted(int count, String folder);

  /// Snackbar after moving items by drag and drop. {folder} is the remote target folder.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, one{Moved {count} item to {folder}} other{Moved {count} items to {folder}}}'**
  String sftpMoved(int count, String folder);

  /// Drag feedback chip: number of dragged items.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, one{{count} item} other{{count} items}}'**
  String sftpDragItems(int count);

  /// Confirmation when disconnecting while files of this host are being edited.
  ///
  /// In en, this message translates to:
  /// **'Stop editing and disconnect?'**
  String get sftpDisconnectEditsTitle;

  /// Confirmation message when disconnecting with active edit sessions.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, one{{count} file from this host is being edited.} other{{count} files from this host are being edited.}} Pending changes are uploaded first; if that fails, the working copies are kept for recovery.'**
  String sftpDisconnectEditsMessage(int count);

  /// Get Info dialog title. {name} is the file name.
  ///
  /// In en, this message translates to:
  /// **'“{name}” info'**
  String sftpInfoTitle(String name);

  /// Get Info row: parent folder path.
  ///
  /// In en, this message translates to:
  /// **'Where'**
  String get sftpInfoWhere;

  /// Get Info size: human size and exact byte count (formatted).
  ///
  /// In en, this message translates to:
  /// **'{size} ({bytes} bytes)'**
  String sftpInfoSizeBytes(String size, String bytes);

  /// Get Info row: target of a symbolic link.
  ///
  /// In en, this message translates to:
  /// **'Points to'**
  String get sftpInfoLinkTarget;

  /// Get Info note for symlinks (chmod follows the link).
  ///
  /// In en, this message translates to:
  /// **'Permissions of a symbolic link apply to the item it points to.'**
  String get sftpInfoSymlinkNote;

  /// Get Info permissions row: everyone except owner and group.
  ///
  /// In en, this message translates to:
  /// **'Others'**
  String get sftpInfoOthers;

  /// Get Info permissions column: read bit.
  ///
  /// In en, this message translates to:
  /// **'Read'**
  String get sftpInfoRead;

  /// Get Info permissions column: write bit.
  ///
  /// In en, this message translates to:
  /// **'Write'**
  String get sftpInfoWrite;

  /// Get Info permissions column: execute bit.
  ///
  /// In en, this message translates to:
  /// **'Execute'**
  String get sftpInfoExecute;

  /// Get Info field label: permissions as octal digits (0755).
  ///
  /// In en, this message translates to:
  /// **'Octal'**
  String get sftpInfoOctal;

  /// Get Info octal field validation error.
  ///
  /// In en, this message translates to:
  /// **'Enter 3 or 4 digits from 0 to 7.'**
  String get sftpInfoOctalInvalid;

  /// Get Info button: change the permissions on the server (chmod).
  ///
  /// In en, this message translates to:
  /// **'Apply'**
  String get sftpInfoApply;

  /// Snackbar after a successful chmod.
  ///
  /// In en, this message translates to:
  /// **'Permissions changed.'**
  String get sftpInfoPermissionsSaved;

  /// Quick Look body for a folder.
  ///
  /// In en, this message translates to:
  /// **'Folders have no preview.'**
  String get sftpQuickLookFolder;

  /// Quick Look body for binary files.
  ///
  /// In en, this message translates to:
  /// **'No preview for this kind of file.'**
  String get sftpQuickLookNoPreview;

  /// Quick Look body for images over the size cap. {size} is the formatted file size.
  ///
  /// In en, this message translates to:
  /// **'This image is too large to preview ({size}).'**
  String sftpQuickLookTooLarge(String size);

  /// Quick Look body when image decoding fails.
  ///
  /// In en, this message translates to:
  /// **'The image couldn’t be displayed.'**
  String get sftpQuickLookImageError;

  /// Quick Look note when only the beginning of a large text file was loaded. Sizes are formatted.
  ///
  /// In en, this message translates to:
  /// **'Showing the first {shown} of {total}.'**
  String sftpQuickLookTruncated(String shown, String total);

  /// Quick Look footer (privacy note: the preview never touches the disk).
  ///
  /// In en, this message translates to:
  /// **'Loaded into memory only — nothing is saved on this device.'**
  String get sftpQuickLookMemoryNote;

  /// Quick Look button: open the file in its default app for editing.
  ///
  /// In en, this message translates to:
  /// **'Open in editor'**
  String get sftpQuickLookOpenInEditor;

  /// Tooltip: collapse the bottom Transfers/Editing drawer.
  ///
  /// In en, this message translates to:
  /// **'Hide panel'**
  String get sftpActivityHide;

  /// Transfers drawer button: remove completed, failed and cancelled transfers.
  ///
  /// In en, this message translates to:
  /// **'Clear finished'**
  String get sftpTransfersClear;

  /// Transfers drawer empty state.
  ///
  /// In en, this message translates to:
  /// **'No transfers yet. Drop files onto the list or use Upload here…'**
  String get sftpTransfersEmpty;

  /// Tooltip of the transfer direction icon (glossary: transfers list «Выгрузка»).
  ///
  /// In en, this message translates to:
  /// **'Upload'**
  String get sftpTransferDirectionUpload;

  /// Tooltip of the transfer direction icon (glossary: transfers list «Загрузка»).
  ///
  /// In en, this message translates to:
  /// **'Download'**
  String get sftpTransferDirectionDownload;

  /// Transfers drawer: queued transfer.
  ///
  /// In en, this message translates to:
  /// **'Waiting…'**
  String get sftpTransferQueued;

  /// Transfers drawer: destination folder after the file name. {path} is a path.
  ///
  /// In en, this message translates to:
  /// **'to {path}'**
  String sftpTransferTo(String path);

  /// Editing panel empty state.
  ///
  /// In en, this message translates to:
  /// **'No files are being edited. Double-click a file to open it in its default app — every save is uploaded automatically.'**
  String get sftpEditingEmpty;

  /// Edit session status: downloading and opening the editor.
  ///
  /// In en, this message translates to:
  /// **'Opening…'**
  String get sftpEditStatusOpening;

  /// Edit session status: the server has the latest save.
  ///
  /// In en, this message translates to:
  /// **'Synced'**
  String get sftpEditStatusSynced;

  /// Edit session status: local changes not uploaded yet.
  ///
  /// In en, this message translates to:
  /// **'Modified'**
  String get sftpEditStatusModified;

  /// Edit session status while uploading a save. {percent} is 0–100.
  ///
  /// In en, this message translates to:
  /// **'Uploading {percent}%'**
  String sftpEditStatusUploading(int percent);

  /// Edit session status while uploading (size unknown).
  ///
  /// In en, this message translates to:
  /// **'Uploading…'**
  String get sftpEditStatusUploadingUnknown;

  /// Edit session status: the file changed on the server; nothing was uploaded.
  ///
  /// In en, this message translates to:
  /// **'Conflict'**
  String get sftpEditStatusConflict;

  /// Edit session status: the last upload failed (Retry available).
  ///
  /// In en, this message translates to:
  /// **'Upload failed'**
  String get sftpEditStatusError;

  /// Edit session status: ended.
  ///
  /// In en, this message translates to:
  /// **'Closed'**
  String get sftpEditStatusClosed;

  /// Editing panel detail: when the last save reached the server. {time} is relative ("5 min ago").
  ///
  /// In en, this message translates to:
  /// **'uploaded {time}'**
  String sftpEditLastSynced(String time);

  /// Editing panel detail: application chosen with Open With… {app} is the app name.
  ///
  /// In en, this message translates to:
  /// **'in {app}'**
  String sftpEditOpenedWith(String app);

  /// Editing panel action on macOS: reveal the local working copy.
  ///
  /// In en, this message translates to:
  /// **'Show in Finder'**
  String get sftpEditRevealFinder;

  /// Editing panel action on Windows: reveal the local working copy.
  ///
  /// In en, this message translates to:
  /// **'Show in Explorer'**
  String get sftpEditRevealExplorer;

  /// Editing panel action on other systems: open the folder of the working copy.
  ///
  /// In en, this message translates to:
  /// **'Show in folder'**
  String get sftpEditRevealOther;

  /// Editing panel action: open the working copy in the editor again.
  ///
  /// In en, this message translates to:
  /// **'Reopen in editor'**
  String get sftpEditReopen;

  /// Editing panel action: check the working copy and upload it if it changed.
  ///
  /// In en, this message translates to:
  /// **'Sync now'**
  String get sftpEditSyncNow;

  /// Editing panel action: upload pending changes and delete the working copy.
  ///
  /// In en, this message translates to:
  /// **'Stop editing'**
  String get sftpEditStop;

  /// Editing panel button for a session in conflict: opens the conflict dialog.
  ///
  /// In en, this message translates to:
  /// **'Resolve…'**
  String get sftpEditResolve;

  /// Snackbar after opening a remote file in its default app.
  ///
  /// In en, this message translates to:
  /// **'Opened “{name}”. Every save is uploaded automatically.'**
  String sftpEditOpened(String name);

  /// Snackbar after Stop editing.
  ///
  /// In en, this message translates to:
  /// **'Stopped editing “{name}”.'**
  String sftpEditStopped(String name);

  /// Snackbar when the final upload of Stop editing failed. {detail} is the core diagnostic (English).
  ///
  /// In en, this message translates to:
  /// **'“{name}” was not uploaded ({detail}). Editing continues.'**
  String sftpEditStopFailed(String name, String detail);

  /// Snackbar: file over the edit size limit. Sizes are formatted.
  ///
  /// In en, this message translates to:
  /// **'“{name}” is too large to edit ({size}, limit {limit}).'**
  String sftpEditTooLarge(String name, String size, String limit);

  /// Snackbar: tried to edit a folder, device or broken link.
  ///
  /// In en, this message translates to:
  /// **'Only regular files can be opened for editing.'**
  String get sftpEditNotAFile;

  /// First-use hint title before a remote file is opened in an external app.
  ///
  /// In en, this message translates to:
  /// **'Editing in another app'**
  String get sftpEditHintTitle;

  /// SECURITY: first-use hint body (local plaintext working copy by design).
  ///
  /// In en, this message translates to:
  /// **'“{name}” opens in its default app, outside ConsoleCrypt. While you edit, a private working copy is kept on this device (readable only by your user) and every save is uploaded to the server automatically.'**
  String sftpEditHintBody(String name);

  /// SECURITY: first-use hint — the external editor is outside our control (SFTP_BROWSER_SPEC §2).
  ///
  /// In en, this message translates to:
  /// **'ConsoleCrypt can’t control what that app does with the file — for example its own autosave, backups or cloud sync. The working copy is deleted when you stop editing.'**
  String get sftpEditHintControl;

  /// First-use hint checkbox.
  ///
  /// In en, this message translates to:
  /// **'Don’t show again'**
  String get sftpEditHintDontShow;

  /// Conflict dialog title. {name} is the file name.
  ///
  /// In en, this message translates to:
  /// **'“{name}” changed on the server'**
  String sftpConflictTitle(String name);

  /// Conflict dialog message.
  ///
  /// In en, this message translates to:
  /// **'Someone changed this file on the server after you opened it. Your latest save has not been uploaded, so nothing was overwritten. Choose what to do:'**
  String get sftpConflictMessage;

  /// Conflict dialog message when the remote file is gone.
  ///
  /// In en, this message translates to:
  /// **'The file was deleted or moved on the server after you opened it. Your latest save has not been uploaded.'**
  String get sftpConflictDeletedMessage;

  /// Conflict dialog detail about the changed remote file.
  ///
  /// In en, this message translates to:
  /// **'Server version: {size}, modified {time}'**
  String sftpConflictRemoteMeta(String size, String time);

  /// Conflict option title (resolution OverwriteRemote).
  ///
  /// In en, this message translates to:
  /// **'Overwrite the server file'**
  String get sftpConflictOverwriteTitle;

  /// Conflict option description (OverwriteRemote).
  ///
  /// In en, this message translates to:
  /// **'Upload your version. The other changes on the server are lost.'**
  String get sftpConflictOverwriteBody;

  /// Conflict option title when the remote file was deleted (OverwriteRemote recreates it).
  ///
  /// In en, this message translates to:
  /// **'Upload my version again'**
  String get sftpConflictRecreateTitle;

  /// Conflict option description when the remote file was deleted.
  ///
  /// In en, this message translates to:
  /// **'Create the file on the server again from your local copy.'**
  String get sftpConflictRecreateBody;

  /// Conflict option title (resolution KeepRemoteCopyLocally).
  ///
  /// In en, this message translates to:
  /// **'Keep both'**
  String get sftpConflictKeepTitle;

  /// Conflict option description (KeepRemoteCopyLocally). {name} is the file name without extension.
  ///
  /// In en, this message translates to:
  /// **'Save the server version next to your copy as “{name}.remote-…” and open it to compare. Your next save uploads your version.'**
  String sftpConflictKeepBody(String name);

  /// Conflict option title (resolution DiscardLocal).
  ///
  /// In en, this message translates to:
  /// **'Discard my changes'**
  String get sftpConflictDiscardTitle;

  /// Conflict option description (DiscardLocal).
  ///
  /// In en, this message translates to:
  /// **'Replace your local copy with the server version.'**
  String get sftpConflictDiscardBody;

  /// Conflict dialog: close without resolving (nothing is uploaded meanwhile).
  ///
  /// In en, this message translates to:
  /// **'Decide later'**
  String get sftpConflictLater;

  /// Startup dialog title: edit sessions left behind by a crash or quit.
  ///
  /// In en, this message translates to:
  /// **'Recover unsaved edits?'**
  String get sftpLeftoversTitle;

  /// Startup dialog message for leftover edit sessions.
  ///
  /// In en, this message translates to:
  /// **'ConsoleCrypt was closed while these remote files were being edited. Their working copies are still on this device.'**
  String get sftpLeftoversMessage;

  /// Leftover row: the working copy has unsaved (not uploaded) edits.
  ///
  /// In en, this message translates to:
  /// **'Changed locally, not on the server yet'**
  String get sftpLeftoverModified;

  /// Leftover row: the working copy equals the last uploaded version.
  ///
  /// In en, this message translates to:
  /// **'No local changes'**
  String get sftpLeftoverUnchanged;

  /// Leftover row: its host no longer exists in the vault.
  ///
  /// In en, this message translates to:
  /// **'Unknown host — can only be deleted'**
  String get sftpLeftoverUnknownHost;

  /// Leftover row: the session manifest is missing or unreadable.
  ///
  /// In en, this message translates to:
  /// **'Session data is damaged — can only be deleted'**
  String get sftpLeftoverUnreadable;

  /// Leftovers dialog primary button.
  ///
  /// In en, this message translates to:
  /// **'Recover selected'**
  String get sftpLeftoversRecover;

  /// Leftovers dialog note under the list.
  ///
  /// In en, this message translates to:
  /// **'Selected files are uploaded after a conflict check and stay open for editing. The other working copies are deleted.'**
  String get sftpLeftoversRecoverHint;

  /// Leftovers dialog: securely delete every working copy.
  ///
  /// In en, this message translates to:
  /// **'Delete all'**
  String get sftpLeftoversDiscardAll;

  /// Leftovers dialog: keep the working copies and ask again next start.
  ///
  /// In en, this message translates to:
  /// **'Later'**
  String get sftpLeftoversLater;

  /// Snackbar after recovering leftover edit sessions.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, one{Recovered {count} file} other{Recovered {count} files}}'**
  String sftpLeftoversRecovered(int count);

  /// Snackbar when a leftover could not be resumed. {error} is a localized error message.
  ///
  /// In en, this message translates to:
  /// **'Couldn’t recover “{name}”: {error}'**
  String sftpLeftoversFailed(String name, String error);

  /// Developer-only menu of the mock backend in the Editing panel.
  ///
  /// In en, this message translates to:
  /// **'Simulate (demo backend)'**
  String get sftpDebugMenu;

  /// Developer-only (mock backend): pretend the external editor saved the file.
  ///
  /// In en, this message translates to:
  /// **'Simulate a save in the editor'**
  String get sftpDebugSave;

  /// Developer-only (mock backend): pretend someone changed the remote file.
  ///
  /// In en, this message translates to:
  /// **'Simulate a change on the server'**
  String get sftpDebugRemoteChange;

  /// Developer-only (mock backend): the next edit upload fails.
  ///
  /// In en, this message translates to:
  /// **'Make the next upload fail'**
  String get sftpDebugFailUpload;

  /// Developer-only (mock backend): the next transfer fails half-way.
  ///
  /// In en, this message translates to:
  /// **'Make the next transfer fail (demo)'**
  String get sftpDebugFailTransfer;

  /// Tunnels page title.
  ///
  /// In en, this message translates to:
  /// **'Tunnels'**
  String get tunnelsTitle;

  /// Tunnels page subtitle.
  ///
  /// In en, this message translates to:
  /// **'Port forwarding over SSH. Tunnels run on this device; their definitions sync with your vault.'**
  String get tunnelsSubtitle;

  /// Tunnels page button and editor dialog title for a new tunnel.
  ///
  /// In en, this message translates to:
  /// **'New tunnel'**
  String get tunnelsNew;

  /// Tunnels empty state title.
  ///
  /// In en, this message translates to:
  /// **'No tunnels yet'**
  String get tunnelsEmptyTitle;

  /// Tunnels empty state example. Addresses stay as is.
  ///
  /// In en, this message translates to:
  /// **'e.g. 127.0.0.1:15432 → db.internal:5432 through a bastion.'**
  String get tunnelsEmptyMessage;

  /// Shown instead of the carrier host name when that host no longer exists.
  ///
  /// In en, this message translates to:
  /// **'(deleted host)'**
  String get tunnelsDeletedHost;

  /// Tunnel status: running (start time unknown). {count} is the number of active forwarded connections.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, other{Running · {count} conn.}}'**
  String tunnelsStatusRunning(int count);

  /// Tunnel status: running. {ago} is a relative time ("5 min ago", "just now"), {count} the number of active forwarded connections.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, other{Running since {ago} · {count} conn.}}'**
  String tunnelsStatusRunningSince(int count, String ago);

  /// Tunnel status: starting.
  ///
  /// In en, this message translates to:
  /// **'Starting…'**
  String get tunnelsStatusStarting;

  /// Tunnel status: failed (no reason given).
  ///
  /// In en, this message translates to:
  /// **'Failed'**
  String get tunnelsStatusFailed;

  /// Tunnel status: failed. {detail} is the raw (English) diagnostic from the tunnel core.
  ///
  /// In en, this message translates to:
  /// **'Failed: {detail}'**
  String tunnelsStatusFailedDetail(String detail);

  /// Tunnel status: stopped.
  ///
  /// In en, this message translates to:
  /// **'Stopped'**
  String get tunnelsStatusStopped;

  /// Warning icon tooltip on a tunnel bound to a non-loopback address. {address} is the bind address.
  ///
  /// In en, this message translates to:
  /// **'Bound to {address}: reachable from the network'**
  String tunnelsPublicBindTooltip(String address);

  /// Tunnel list summary of a remote tunnel: {bind} is host:port on the server, {target} host:port on this device.
  ///
  /// In en, this message translates to:
  /// **'remote {bind} → {target}'**
  String tunnelsSummaryRemote(String bind, String target);

  /// Tunnel list subtitle. {summary} is the address mapping, {host} the name of the SSH host carrying the tunnel.
  ///
  /// In en, this message translates to:
  /// **'{summary}  ·  via {host}'**
  String tunnelsTileSubtitle(String summary, String host);

  /// Confirmation dialog title for deleting a tunnel. {name} is the tunnel name.
  ///
  /// In en, this message translates to:
  /// **'Delete tunnel {name}?'**
  String tunnelsDeleteTitle(String name);

  /// Confirmation message for deleting a tunnel.
  ///
  /// In en, this message translates to:
  /// **'The tunnel is stopped and removed from the vault.'**
  String get tunnelsDeleteMessage;

  /// Tunnel editor error: no carrier host selected.
  ///
  /// In en, this message translates to:
  /// **'Choose the host that carries the tunnel'**
  String get tunnelsErrorChooseHost;

  /// Tunnel editor dialog title for an existing tunnel.
  ///
  /// In en, this message translates to:
  /// **'Edit tunnel'**
  String get tunnelsEdit;

  /// Tunnel editor dropdown label: the SSH host whose connection carries the tunnel.
  ///
  /// In en, this message translates to:
  /// **'Through host (SSH connection incl. its jump chain)'**
  String get tunnelsHostLabel;

  /// Tunnel editor field label (remote tunnel): address the server listens on.
  ///
  /// In en, this message translates to:
  /// **'Bind address on the server'**
  String get tunnelsBindAddressRemote;

  /// Tunnel editor field label (local/dynamic tunnel): address this device listens on.
  ///
  /// In en, this message translates to:
  /// **'Bind address (this device)'**
  String get tunnelsBindAddressLocal;

  /// Tunnel editor port field label (bind and target port).
  ///
  /// In en, this message translates to:
  /// **'Port'**
  String get tunnelsPortLabel;

  /// Tunnel editor field label (remote tunnel): host on this device the forwarded port points to.
  ///
  /// In en, this message translates to:
  /// **'Target on this device'**
  String get tunnelsTargetRemote;

  /// Tunnel editor field label (local tunnel): target host, resolved on the server.
  ///
  /// In en, this message translates to:
  /// **'Target host (as seen from the server)'**
  String get tunnelsTargetLocal;

  /// Tunnel editor danger banner title: the bind address is not 127.0.0.1 / ::1 / localhost.
  ///
  /// In en, this message translates to:
  /// **'Not bound to loopback'**
  String get tunnelsPublicBindTitle;

  /// Tunnel editor danger banner (remote tunnel). {address} is the bind address. GatewayPorts is an sshd option, not translated.
  ///
  /// In en, this message translates to:
  /// **'\"{address}\" exposes the forwarded port to the server\'s network (requires GatewayPorts). Anyone who can reach it can use the tunnel.'**
  String tunnelsPublicBindWarningRemote(String address);

  /// Tunnel editor danger banner (local/dynamic tunnel). {address} is the bind address.
  ///
  /// In en, this message translates to:
  /// **'\"{address}\" exposes the tunnel to your local network. Anyone who can reach this device can use it. Use 127.0.0.1 unless you really mean it.'**
  String tunnelsPublicBindWarningLocal(String address);

  /// Tunnel editor checkbox required before saving a tunnel bound to a non-loopback address.
  ///
  /// In en, this message translates to:
  /// **'I understand this tunnel is reachable from the network'**
  String get tunnelsPublicBindAck;

  /// Tunnel editor switch: start the tunnel when the vault is unlocked.
  ///
  /// In en, this message translates to:
  /// **'Start automatically after unlock'**
  String get tunnelsAutoStart;

  /// Device approval wait screen (new, untrusted device) title.
  ///
  /// In en, this message translates to:
  /// **'Approve this device'**
  String get approvalWaitTitle;

  /// Device approval wait screen subtitle. SECURITY: the verification code (six groups) must be compared on both devices. 'Devices' is the sidebar section name.
  ///
  /// In en, this message translates to:
  /// **'On a device where ConsoleCrypt is unlocked, open Devices and review the request from \"{device}\". The code shown there must match this one exactly.'**
  String approvalWaitSubtitle(String device);

  /// Device approval wait screen subtitle when the device name is unknown. SECURITY: the verification code (six groups) must be compared on both devices.
  ///
  /// In en, this message translates to:
  /// **'On a device where ConsoleCrypt is unlocked, open Devices and review the request from \"this device\". The code shown there must match this one exactly.'**
  String get approvalWaitSubtitleUnnamed;

  /// Label above this device's verification code on the approval wait screen.
  ///
  /// In en, this message translates to:
  /// **'Verification code of this device'**
  String get approvalWaitCodeLabel;

  /// Progress text next to a spinner on the approval wait screen.
  ///
  /// In en, this message translates to:
  /// **'Waiting for approval… The request expires in 24 hours.'**
  String get approvalWaitWaiting;

  /// SECURITY warning banner on the approval wait screen: what to do when verification codes differ.
  ///
  /// In en, this message translates to:
  /// **'If the codes differ, reject the request on the other device. A mismatch can mean someone (even a compromised server) is trying to add their own device.'**
  String get approvalWaitMismatchWarning;

  /// Developer-only button (mock backend): simulate approval from another device.
  ///
  /// In en, this message translates to:
  /// **'Simulate approval (mock)'**
  String get approvalWaitSimulateApproval;

  /// Default vault name when the profile has no name (editable text field).
  ///
  /// In en, this message translates to:
  /// **'Personal'**
  String get createVaultDefaultName;

  /// Onboarding step 1 title: choose the vault passphrase.
  ///
  /// In en, this message translates to:
  /// **'Create your vault passphrase'**
  String get createVaultTitle;

  /// Onboarding step 1 subtitle for a local profile ('It' = the vault passphrase).
  ///
  /// In en, this message translates to:
  /// **'It encrypts everything in this profile on this device.'**
  String get createVaultSubtitleLocal;

  /// Onboarding step 1 subtitle for a synced profile ('It' = the vault passphrase).
  ///
  /// In en, this message translates to:
  /// **'It encrypts your vault before anything is synced. Your server never sees it, and it is different from your account password.'**
  String get createVaultSubtitleSynced;

  /// Field label: vault name.
  ///
  /// In en, this message translates to:
  /// **'Vault name'**
  String get createVaultNameLabel;

  /// Field label: new vault passphrase.
  ///
  /// In en, this message translates to:
  /// **'Vault passphrase'**
  String get createVaultPassphraseLabel;

  /// Field label: repeat the new vault passphrase.
  ///
  /// In en, this message translates to:
  /// **'Repeat passphrase'**
  String get createVaultRepeatPassphraseLabel;

  /// Inline error under the repeat field when the two passphrases differ.
  ///
  /// In en, this message translates to:
  /// **'Passphrases do not match'**
  String get createVaultPassphraseMismatch;

  /// SECURITY info banner: nobody (including the server operator) can reset the passphrase; the Recovery Kit is the only other way in.
  ///
  /// In en, this message translates to:
  /// **'Nobody can reset this passphrase for you — not even your server administrator. Next you will get a Recovery Kit, the only other way into the vault.'**
  String get createVaultNoResetNotice;

  /// Button: create the vault.
  ///
  /// In en, this message translates to:
  /// **'Create vault'**
  String get createVaultSubmit;

  /// Onboarding step 2 title: the printable Recovery Kit.
  ///
  /// In en, this message translates to:
  /// **'Save your Recovery Kit'**
  String get recoveryKitTitle;

  /// SECURITY: Recovery Kit screen subtitle — the kit is the only way back if the passphrase is lost.
  ///
  /// In en, this message translates to:
  /// **'If you forget your passphrase and lose access to your trusted devices, this kit is the only way back into your vault. Keep it offline: print it or write it down.'**
  String get recoveryKitSubtitle;

  /// Warning banner when the kit is gone after an app restart mid-onboarding.
  ///
  /// In en, this message translates to:
  /// **'The Recovery Kit is no longer in memory (the app was restarted). Generate a new one — the previous kit stops working.'**
  String get recoveryKitLostFromMemory;

  /// Button: regenerate the Recovery Kit (invalidates the previous one).
  ///
  /// In en, this message translates to:
  /// **'Generate a new Recovery Kit'**
  String get recoveryKitGenerateNew;

  /// Snackbar: printing is not implemented yet (M5 = milestone 5).
  ///
  /// In en, this message translates to:
  /// **'Printing arrives with M5. For now, write the words down.'**
  String get recoveryKitPrintUnavailable;

  /// Button: print the Recovery Kit or save it as PDF.
  ///
  /// In en, this message translates to:
  /// **'Print / Save as PDF'**
  String get recoveryKitPrint;

  /// Button: copy the recovery words to the clipboard.
  ///
  /// In en, this message translates to:
  /// **'Copy words'**
  String get recoveryKitCopyWords;

  /// Checkbox the user must tick before continuing.
  ///
  /// In en, this message translates to:
  /// **'I have stored my Recovery Kit somewhere safe'**
  String get recoveryKitSavedConfirmation;

  /// Checkbox subtitle while the kit is still hidden.
  ///
  /// In en, this message translates to:
  /// **'Reveal the kit first'**
  String get recoveryKitRevealFirst;

  /// Footer note on the Recovery Kit screen ('it' = the Recovery Key).
  ///
  /// In en, this message translates to:
  /// **'The Recovery Key is never stored by ConsoleCrypt or your server — only an envelope it can open.'**
  String get recoveryKitKeyNotStored;

  /// Recovery Kit detail label: vault identifier.
  ///
  /// In en, this message translates to:
  /// **'Vault ID'**
  String get recoveryKitVaultId;

  /// Recovery Kit detail label: server URL.
  ///
  /// In en, this message translates to:
  /// **'Server'**
  String get recoveryKitServer;

  /// Recovery Kit server value for a local profile: no copy exists on any server.
  ///
  /// In en, this message translates to:
  /// **'Local profile — no server copy'**
  String get recoveryKitNoServerCopy;

  /// Recovery Kit detail label: creation date and time.
  ///
  /// In en, this message translates to:
  /// **'Created'**
  String get recoveryKitCreated;

  /// Hint above the button that reveals the recovery words.
  ///
  /// In en, this message translates to:
  /// **'Make sure nobody can see your screen.'**
  String get recoveryKitPrivacyHint;

  /// Button: reveal the recovery words and QR code.
  ///
  /// In en, this message translates to:
  /// **'Show Recovery Kit'**
  String get recoveryKitShow;

  /// Onboarding step 3 title: re-enter requested recovery words.
  ///
  /// In en, this message translates to:
  /// **'Confirm your Recovery Kit'**
  String get verifyKitTitle;

  /// Warning when the kit is gone from memory on the verification step.
  ///
  /// In en, this message translates to:
  /// **'No Recovery Kit in memory. Go back to generate one.'**
  String get verifyKitNoKit;

  /// Button: go back to the Recovery Kit screen.
  ///
  /// In en, this message translates to:
  /// **'Back to Recovery Kit'**
  String get verifyKitBackToRecoveryKit;

  /// Verification step subtitle.
  ///
  /// In en, this message translates to:
  /// **'Enter the requested words from your kit to prove it is saved. This step is required.'**
  String get verifyKitSubtitle;

  /// Field label: the recovery word at this position must be entered.
  ///
  /// In en, this message translates to:
  /// **'Word #{index}'**
  String verifyKitWordLabel(int index);

  /// Error when the entered words do not match the kit.
  ///
  /// In en, this message translates to:
  /// **'Those words don\'t match your Recovery Kit. Check your kit and try again.'**
  String get verifyKitMismatch;

  /// Button: go back to the Recovery Kit screen.
  ///
  /// In en, this message translates to:
  /// **'Back to kit'**
  String get verifyKitBackToKit;

  /// Button: check the entered words.
  ///
  /// In en, this message translates to:
  /// **'Verify'**
  String get verifyKitSubmit;

  /// SECURITY: title of the one-time notice at the end of local-profile onboarding.
  ///
  /// In en, this message translates to:
  /// **'No cloud copy'**
  String get onboardingLocalNoticeTitle;

  /// SECURITY: 'no cloud copy' notice subtitle — no copy exists on any server.
  ///
  /// In en, this message translates to:
  /// **'This profile lives only on this device. Keep your Recovery Kit and make backups.'**
  String get onboardingLocalNoticeSubtitle;

  /// 'No cloud copy' notice point title.
  ///
  /// In en, this message translates to:
  /// **'Keep your Recovery Kit safe'**
  String get onboardingLocalNoticeKitTitle;

  /// 'No cloud copy' notice point body ('It' = the Recovery Kit).
  ///
  /// In en, this message translates to:
  /// **'It opens the vault if you forget your passphrase.'**
  String get onboardingLocalNoticeKitBody;

  /// 'No cloud copy' notice point title.
  ///
  /// In en, this message translates to:
  /// **'Make encrypted backups'**
  String get onboardingLocalNoticeBackupsTitle;

  /// 'No cloud copy' notice point body.
  ///
  /// In en, this message translates to:
  /// **'Export a .ccbackup file, or let ConsoleCrypt back up to a folder automatically.'**
  String get onboardingLocalNoticeBackupsBody;

  /// SECURITY: 'No cloud copy' notice point title.
  ///
  /// In en, this message translates to:
  /// **'Lost device without a backup = lost data'**
  String get onboardingLocalNoticeLossTitle;

  /// SECURITY: 'No cloud copy' notice point body — no copy exists on any server.
  ///
  /// In en, this message translates to:
  /// **'There is no server copy to restore from. You can enable sync later in Settings.'**
  String get onboardingLocalNoticeLossBody;

  /// Button: finish onboarding and open backup settings.
  ///
  /// In en, this message translates to:
  /// **'Set up automatic backups'**
  String get onboardingLocalNoticeSetUpBackups;

  /// Button: acknowledge the notice and finish onboarding.
  ///
  /// In en, this message translates to:
  /// **'I understand — continue'**
  String get onboardingLocalNoticeContinue;

  /// Unlock screen title.
  ///
  /// In en, this message translates to:
  /// **'Unlock {name}'**
  String unlockTitle(String name);

  /// Unlock screen title when the vault name is unknown.
  ///
  /// In en, this message translates to:
  /// **'Unlock vault'**
  String get unlockTitleGeneric;

  /// Field label on the unlock screen.
  ///
  /// In en, this message translates to:
  /// **'Vault passphrase'**
  String get unlockPassphraseLabel;

  /// Button: unlock the vault with the passphrase.
  ///
  /// In en, this message translates to:
  /// **'Unlock'**
  String get unlockSubmit;

  /// Reason shown in the OS authentication prompt (Touch ID / Windows Hello).
  ///
  /// In en, this message translates to:
  /// **'Unlock your ConsoleCrypt vault'**
  String get unlockOsAuthReason;

  /// Button: unlock with OS authentication.
  ///
  /// In en, this message translates to:
  /// **'Unlock with {method}'**
  String unlockWithOsAuth(String method);

  /// Banner title on the unlock screen for an untrusted synced device.
  ///
  /// In en, this message translates to:
  /// **'This device is not trusted yet'**
  String get unlockUntrustedTitle;

  /// SECURITY: banner message on the unlock screen for an untrusted synced device.
  ///
  /// In en, this message translates to:
  /// **'Unlock with your passphrase, or approve this device from one you already use. Approval requires comparing a verification code on both screens.'**
  String get unlockUntrustedMessage;

  /// Button: request device approval from a trusted device.
  ///
  /// In en, this message translates to:
  /// **'Approve from another device'**
  String get unlockApproveFromOtherDevice;

  /// Link: open vault recovery.
  ///
  /// In en, this message translates to:
  /// **'Forgot passphrase?'**
  String get unlockForgotPassphrase;

  /// Button on the unlock screen: sign out of the synced account.
  ///
  /// In en, this message translates to:
  /// **'Sign out'**
  String get unlockSignOut;

  /// Vault recovery screen title.
  ///
  /// In en, this message translates to:
  /// **'Recover vault access'**
  String get recoveryTitle;

  /// Vault recovery screen subtitle.
  ///
  /// In en, this message translates to:
  /// **'Forgot your vault passphrase? Open the vault another way and set a new passphrase.'**
  String get recoverySubtitle;

  /// Segmented button: recover with the Recovery Key.
  ///
  /// In en, this message translates to:
  /// **'Recovery Key'**
  String get recoveryMethodRecoveryKey;

  /// Segmented button: recover with this trusted device (OS authentication).
  ///
  /// In en, this message translates to:
  /// **'This trusted device'**
  String get recoveryMethodTrustedDevice;

  /// Instruction above the Recovery Key input.
  ///
  /// In en, this message translates to:
  /// **'Type the 24 words from your Recovery Kit, or paste the text of its QR code.'**
  String get recoveryWordsInstructions;

  /// Hint in the Recovery Key input. Only 'or' is translated; the rest is literal input format.
  ///
  /// In en, this message translates to:
  /// **'word1 word2 … word24  or  consolecrypt-recovery:v1:…'**
  String get recoveryInputHint;

  /// Status under the input: pasted QR text is valid.
  ///
  /// In en, this message translates to:
  /// **'QR text recognised'**
  String get recoveryQrRecognised;

  /// Status under the input: pasted QR text is incomplete.
  ///
  /// In en, this message translates to:
  /// **'QR text is incomplete'**
  String get recoveryQrIncomplete;

  /// Status under the input: how many recovery words were entered.
  ///
  /// In en, this message translates to:
  /// **'{count} / {total} words'**
  String recoveryWordCount(int count, int total);

  /// Info banner for recovery via this trusted device.
  ///
  /// In en, this message translates to:
  /// **'Uses this device\'s key, protected by {method}, to open the vault. Then set a new passphrase; other devices keep working.'**
  String recoveryTrustedDeviceInfo(String method);

  /// Warning when trusted-device recovery is selected on an untrusted device.
  ///
  /// In en, this message translates to:
  /// **'This device is not trusted for the vault. Use your Recovery Key instead.'**
  String get recoveryDeviceNotTrusted;

  /// Field label: new vault passphrase.
  ///
  /// In en, this message translates to:
  /// **'New vault passphrase'**
  String get recoveryNewPassphraseLabel;

  /// Field label: repeat the new vault passphrase.
  ///
  /// In en, this message translates to:
  /// **'Repeat new passphrase'**
  String get recoveryRepeatNewPassphraseLabel;

  /// Button: OS authentication, then set the new passphrase.
  ///
  /// In en, this message translates to:
  /// **'Authenticate and set passphrase'**
  String get recoverySubmitTrustedDevice;

  /// Button: open the vault with the Recovery Key, then set the new passphrase.
  ///
  /// In en, this message translates to:
  /// **'Recover and set passphrase'**
  String get recoverySubmitRecoveryKey;

  /// Expandable section title on the recovery screen.
  ///
  /// In en, this message translates to:
  /// **'Lost the passphrase, the Recovery Key and every trusted device?'**
  String get recoveryLostEverythingTitle;

  /// SECURITY: expandable section body for a local profile. 'Welcome → Restore from a backup' refers to the welcome screen's restore link.
  ///
  /// In en, this message translates to:
  /// **'This local profile can only come back from a .ccbackup file opened with its passphrase or Recovery Key (Welcome → Restore from a backup). Without one, the data cannot be recovered — by design, nobody holds a master key.'**
  String get recoveryLostEverythingLocal;

  /// SECURITY: expandable section body for a synced profile.
  ///
  /// In en, this message translates to:
  /// **'The old vault cannot be decrypted by anyone, including your server — by design. You can recover your account by e-mail and start a new, empty vault.'**
  String get recoveryLostEverythingSynced;

  /// Tooltip: toolbar button that collapses the sidebar to icons or expands it.
  ///
  /// In en, this message translates to:
  /// **'Show or hide the sidebar'**
  String get shellToggleSidebar;

  /// Toolbar primary button: pick a host and open a terminal tab.
  ///
  /// In en, this message translates to:
  /// **'New connection'**
  String get shellNewConnection;

  /// Tooltip of the toolbar primary button; {shortcut} is e.g. ⌘T / Ctrl+T.
  ///
  /// In en, this message translates to:
  /// **'New connection ({shortcut})'**
  String shellNewConnectionTooltip(String shortcut);

  /// Sync status popover: value of 'Last successful sync' when the profile has never synced.
  ///
  /// In en, this message translates to:
  /// **'Never'**
  String get shellSyncNever;

  /// Sync status popover button: opens the Sync page.
  ///
  /// In en, this message translates to:
  /// **'Sync details'**
  String get shellSyncDetails;

  /// Title of the About dialog and label of the macOS app-menu item and the Settings button. {appName} is the product name (not translated).
  ///
  /// In en, this message translates to:
  /// **'About {appName}'**
  String aboutTitle(String appName);

  /// About dialog: one-line product description under the logo.
  ///
  /// In en, this message translates to:
  /// **'End-to-end encrypted SSH client'**
  String get aboutTagline;

  /// About dialog: app version line.
  ///
  /// In en, this message translates to:
  /// **'Version {version}'**
  String aboutVersion(String version);

  /// About dialog: label of the author row.
  ///
  /// In en, this message translates to:
  /// **'Author'**
  String get aboutAuthor;

  /// About dialog: heading of the licence rows.
  ///
  /// In en, this message translates to:
  /// **'Licenses'**
  String get aboutLicenses;

  /// About dialog: licence row label for this desktop app; the value is an SPDX expression (not translated).
  ///
  /// In en, this message translates to:
  /// **'Desktop client'**
  String get aboutLicenseClient;

  /// About dialog: licence row label for the self-hosted sync server; the value is an SPDX expression (not translated).
  ///
  /// In en, this message translates to:
  /// **'Sync server'**
  String get aboutLicenseServer;

  /// About dialog: short open-source and zero-knowledge statement.
  ///
  /// In en, this message translates to:
  /// **'ConsoleCrypt is open source. The server never sees your vault: everything is encrypted on your devices.'**
  String get aboutOpenSource;

  /// Settings section title: product information (version, author, licences).
  ///
  /// In en, this message translates to:
  /// **'About'**
  String get settingsAboutTitle;

  /// Settings section title: theme, glass, sidebar and language.
  ///
  /// In en, this message translates to:
  /// **'Appearance'**
  String get settingsAppearanceTitle;

  /// Settings section title: clipboard clearing and similar device-local security options.
  ///
  /// In en, this message translates to:
  /// **'Security'**
  String get settingsSecurityTitle;

  /// Settings → Appearance: help text under the Glass selector (Clear · Default · Tinted · Solid).
  ///
  /// In en, this message translates to:
  /// **'How see-through the sidebar, toolbar, menus and dialogs are. Security prompts are always opaque.'**
  String get settingsGlassHelp;

  /// Settings → Appearance: why glass is solid although another Glass option is selected (OS accessibility setting).
  ///
  /// In en, this message translates to:
  /// **'Shown solid because Reduce Transparency is on in the system settings.'**
  String get settingsGlassSolidReduceTransparency;

  /// Settings → Appearance: why glass is solid (remote desktop session).
  ///
  /// In en, this message translates to:
  /// **'Shown solid in a remote desktop session.'**
  String get settingsGlassSolidRemote;

  /// Settings → Appearance: why glass is solid (battery saver).
  ///
  /// In en, this message translates to:
  /// **'Shown solid to save battery (battery saver / Low Power Mode).'**
  String get settingsGlassSolidBattery;

  /// Settings → Appearance: glass was stepped down automatically because rendering was too slow.
  ///
  /// In en, this message translates to:
  /// **'Glass reduced for performance.'**
  String get settingsGlassSolidPerformance;

  /// Settings → Appearance: label of the sidebar style selector.
  ///
  /// In en, this message translates to:
  /// **'Sidebar'**
  String get settingsSidebarLabel;

  /// Settings → Appearance: sidebar style — a floating glass panel inset from the window edges.
  ///
  /// In en, this message translates to:
  /// **'Floating'**
  String get settingsSidebarFloating;

  /// Settings → Appearance: sidebar style — attached to the window edges (macOS 27 style).
  ///
  /// In en, this message translates to:
  /// **'Edge to edge'**
  String get settingsSidebarEdgeToEdge;

  /// Note under a password/passphrase field when the typed characters include Cyrillic letters (likely wrong keyboard layout). Never shows the text itself.
  ///
  /// In en, this message translates to:
  /// **'Typed text contains Cyrillic letters — check the keyboard layout.'**
  String get secretFieldCyrillicHint;

  /// Note under a password/passphrase field when the typed characters include non-ASCII letters other than Cyrillic (likely wrong keyboard layout).
  ///
  /// In en, this message translates to:
  /// **'Typed text contains letters outside A–Z — check the keyboard layout.'**
  String get secretFieldNonLatinHint;

  /// Notice when a NEW vault passphrase contains non-ASCII letters; the user may still continue.
  ///
  /// In en, this message translates to:
  /// **'This passphrase contains letters outside A–Z. To unlock the vault you will have to type it with the same keyboard layout.'**
  String get passphraseLayoutNotice;

  /// Unlock screen: secondary destructive action that opens a confirmation to delete the locked profile (e.g. a forgotten test vault).
  ///
  /// In en, this message translates to:
  /// **'Delete this profile…'**
  String get unlockDeleteProfile;

  /// SECURITY: delete-profile confirmation for a LOCAL profile: explains permanent data loss unless a backup exists.
  ///
  /// In en, this message translates to:
  /// **'This vault exists only on this device. Once deleted, it can come back only from a backup file (.ccbackup) opened with its passphrase or Recovery Kit. Without one, every host, key and password in it is lost for good.'**
  String get deleteProfileLocalWarning;

  /// Checkbox that must be ticked before a local profile can be deleted.
  ///
  /// In en, this message translates to:
  /// **'I understand that this cannot be undone'**
  String get deleteProfileAcknowledge;

  /// SECURITY: title of the run confirmation for a DESTRUCTIVE command; repeats the target host (name and address).
  ///
  /// In en, this message translates to:
  /// **'Run a destructive command on {host}?'**
  String runFlowConfirmTitleDestructive(String host);

  /// Command palette: caption above the quick actions (new host, settings, lock…).
  ///
  /// In en, this message translates to:
  /// **'Actions'**
  String get paletteGroupActions;

  /// Command palette: caption above the 'generate a command with AI' row.
  ///
  /// In en, this message translates to:
  /// **'AI'**
  String get paletteGroupAi;

  /// Appearance settings: Interface text size
  ///
  /// In en, this message translates to:
  /// **'Interface text size'**
  String get settingsUiFontScale;

  /// Appearance settings: new compact base at 100%, adjustable from 80% to 140%; terminal size is separate.
  ///
  /// In en, this message translates to:
  /// **'100% is the new base size, equal to the previous 90%. Choose from 80% to 140%. Terminal text size is configured separately.'**
  String get settingsUiScaleHelp;

  /// Appearance settings: Accent colour
  ///
  /// In en, this message translates to:
  /// **'Accent colour'**
  String get settingsAccentColor;

  /// Appearance settings: Background tint
  ///
  /// In en, this message translates to:
  /// **'Background tint'**
  String get settingsBackgroundTint;

  /// Appearance settings: Choose a swatch or enter any HEX colour. Backgrounds adapt to the light or dark theme; text stays readable.
  ///
  /// In en, this message translates to:
  /// **'Choose a swatch or enter any HEX colour. Backgrounds adapt to the light or dark theme; text stays readable.'**
  String get settingsColorsHelp;

  /// Appearance settings: Apply
  ///
  /// In en, this message translates to:
  /// **'Apply'**
  String get settingsColorApply;

  /// Appearance settings: HEX colour
  ///
  /// In en, this message translates to:
  /// **'HEX colour'**
  String get settingsColorHex;

  /// Appearance settings: Enter six hexadecimal digits, e.g. #7C5CFC.
  ///
  /// In en, this message translates to:
  /// **'Enter six hexadecimal digits, for example #4C8DFF.'**
  String get settingsColorInvalid;

  /// Appearance settings: Reset text size and colours
  ///
  /// In en, this message translates to:
  /// **'Reset text size and colours'**
  String get settingsResetUi;

  /// Appearance settings: Terminal colour theme
  ///
  /// In en, this message translates to:
  /// **'Terminal colour theme'**
  String get settingsTerminalColorScheme;

  /// Appearance settings: Applies to all terminal tabs immediately, independently of interface colours.
  ///
  /// In en, this message translates to:
  /// **'Applies to all terminal tabs immediately, independently of interface colours.'**
  String get settingsTerminalThemeHelp;

  /// Appearance settings: Follow interface
  ///
  /// In en, this message translates to:
  /// **'Follow interface'**
  String get settingsTerminalThemeSystem;

  /// Appearance settings: Graphite
  ///
  /// In en, this message translates to:
  /// **'Graphite'**
  String get settingsTerminalThemeDark;

  /// Appearance settings: Paper
  ///
  /// In en, this message translates to:
  /// **'Paper'**
  String get settingsTerminalThemeLight;

  /// Appearance settings: Midnight
  ///
  /// In en, this message translates to:
  /// **'Midnight'**
  String get settingsTerminalThemeMidnight;

  /// Appearance settings: Ocean
  ///
  /// In en, this message translates to:
  /// **'Ocean'**
  String get settingsTerminalThemeOcean;

  /// Appearance settings: Forest
  ///
  /// In en, this message translates to:
  /// **'Forest'**
  String get settingsTerminalThemeForest;

  /// Appearance settings: Amber
  ///
  /// In en, this message translates to:
  /// **'Amber'**
  String get settingsTerminalThemeAmber;

  /// Appearance settings: Terminal preview
  ///
  /// In en, this message translates to:
  /// **'Terminal preview'**
  String get settingsTerminalPreview;

  /// Appearance settings: Saved on this device. No restart required.
  ///
  /// In en, this message translates to:
  /// **'Saved on this device. No restart required.'**
  String get settingsAppearanceLocalHint;

  /// Terminal appearance or system keychain access UI.
  ///
  /// In en, this message translates to:
  /// **'System secure storage is unavailable. On macOS, the “login” keychain asks for your Mac login password, not your vault passphrase. If your Mac password changed, the keychain may still use the old one. Open Keychain Access and unlock “login”, then retry opening the profile. Do not delete or reset the keychain: it holds this device’s encryption keys.'**
  String get errorSecureStore;

  /// Terminal appearance or system keychain access UI.
  ///
  /// In en, this message translates to:
  /// **'Open a profile'**
  String get welcomeResumeTitle;

  /// Terminal appearance or system keychain access UI.
  ///
  /// In en, this message translates to:
  /// **'Your profiles are on this device. Choose one to continue.'**
  String get welcomeResumeHelp;

  /// Terminal appearance or system keychain access UI.
  ///
  /// In en, this message translates to:
  /// **'Opening a profile may ask for access to the system keychain. On macOS, enter the password for the “login” keychain (usually your Mac login password). If rejected, unlock “login” in Keychain Access; after a Mac password change it may require the previous password.'**
  String get welcomeKeychainHelp;

  /// Terminal appearance or system keychain access UI.
  ///
  /// In en, this message translates to:
  /// **'Open the last profile at startup'**
  String get reopenLastProfile;

  /// Terminal appearance or system keychain access UI.
  ///
  /// In en, this message translates to:
  /// **'May show the system keychain prompt before the app opens.'**
  String get reopenLastProfileHelp;

  /// Terminal appearance or system keychain access UI.
  ///
  /// In en, this message translates to:
  /// **'Custom'**
  String get settingsTerminalThemeCustom;

  /// Terminal appearance or system keychain access UI.
  ///
  /// In en, this message translates to:
  /// **'Colour spectrum'**
  String get colorSpectrum;

  /// Terminal appearance or system keychain access UI.
  ///
  /// In en, this message translates to:
  /// **'Hue'**
  String get colorHue;

  /// Terminal appearance or system keychain access UI.
  ///
  /// In en, this message translates to:
  /// **'Saturation'**
  String get colorSaturation;

  /// Terminal appearance or system keychain access UI.
  ///
  /// In en, this message translates to:
  /// **'Brightness'**
  String get colorBrightness;

  /// Terminal appearance or system keychain access UI.
  ///
  /// In en, this message translates to:
  /// **'Colour'**
  String get commonThemeColor;

  /// Terminal appearance or system keychain access UI.
  ///
  /// In en, this message translates to:
  /// **'Customize / import…'**
  String get terminalThemeEdit;

  /// Terminal appearance or system keychain access UI.
  ///
  /// In en, this message translates to:
  /// **'Choose a colour to edit. Changes apply to all terminal tabs when you press Apply.'**
  String get terminalThemeEditorHelp;

  /// Terminal appearance or system keychain access UI.
  ///
  /// In en, this message translates to:
  /// **'Import theme'**
  String get terminalThemeImport;

  /// Terminal appearance or system keychain access UI.
  ///
  /// In en, this message translates to:
  /// **'Export JSON'**
  String get terminalThemeExport;

  /// Terminal appearance or system keychain access UI.
  ///
  /// In en, this message translates to:
  /// **'Import: iTerm .itermcolors (XML) or ConsoleCrypt .json, up to 256 KB. Export: ConsoleCrypt .json.'**
  String get terminalThemeFormats;

  /// Terminal appearance or system keychain access UI.
  ///
  /// In en, this message translates to:
  /// **'Could not import this theme. Choose a valid iTerm XML preset or ConsoleCrypt JSON file, up to 256 KB. Your saved theme has not changed.'**
  String get terminalThemeImportError;

  /// Terminal appearance or system keychain access UI.
  ///
  /// In en, this message translates to:
  /// **'Could not save the theme. Choose another location and try again.'**
  String get terminalThemeExportError;

  /// Terminal appearance or system keychain access UI.
  ///
  /// In en, this message translates to:
  /// **'Background'**
  String get terminalColorBackground;

  /// Terminal appearance or system keychain access UI.
  ///
  /// In en, this message translates to:
  /// **'Text'**
  String get terminalColorForeground;

  /// Terminal appearance or system keychain access UI.
  ///
  /// In en, this message translates to:
  /// **'Cursor'**
  String get terminalColorCursor;

  /// Terminal appearance or system keychain access UI.
  ///
  /// In en, this message translates to:
  /// **'Selection'**
  String get terminalColorSelection;

  /// Label for an ANSI palette colour, indexed 0 through 7.
  ///
  /// In en, this message translates to:
  /// **'ANSI {index}'**
  String terminalColorAnsi(int index);

  /// Label for an ANSI palette colour, indexed 0 through 7.
  ///
  /// In en, this message translates to:
  /// **'Bright {index}'**
  String terminalColorAnsiBright(int index);

  /// Terminal colour preset file picker or preview label.
  ///
  /// In en, this message translates to:
  /// **'Terminal colour schemes'**
  String get terminalThemeFileType;

  /// Terminal colour preset file picker or preview label.
  ///
  /// In en, this message translates to:
  /// **'ConsoleCrypt theme'**
  String get terminalThemeJsonType;

  /// Terminal colour preset file picker or preview label.
  ///
  /// In en, this message translates to:
  /// **' selected text '**
  String get terminalSampleSelection;

  /// Retries glass rendering after automatic performance reduction. OS accessibility restrictions remain in effect.
  ///
  /// In en, this message translates to:
  /// **'Retry glass effects'**
  String get settingsGlassRetryEffects;

  /// Vault settings: deviceUnlockTitle
  ///
  /// In en, this message translates to:
  /// **'Fingerprint unlock'**
  String get deviceUnlockTitle;

  /// Vault settings: deviceUnlockHelp
  ///
  /// In en, this message translates to:
  /// **'Confirm access with your fingerprint. macOS may also offer your account password.'**
  String get deviceUnlockHelp;

  /// Vault settings: deviceUnlockLocalOnly
  ///
  /// In en, this message translates to:
  /// **'Only for this vault on this device. Your passphrase and Recovery Key remain available.'**
  String get deviceUnlockLocalOnly;

  /// Vault settings: deviceUnlockEnableReason
  ///
  /// In en, this message translates to:
  /// **'Allow unlocking your ConsoleCrypt vault on this Mac'**
  String get deviceUnlockEnableReason;

  /// Vault settings: deviceUnlockNeedsTrust
  ///
  /// In en, this message translates to:
  /// **'First unlock the vault with its passphrase on this device.'**
  String get deviceUnlockNeedsTrust;

  /// Vault settings: deviceUnlockNotEnrolled
  ///
  /// In en, this message translates to:
  /// **'No fingerprint is enrolled. Add one in macOS System Settings → Touch ID & Password, then check availability again.'**
  String get deviceUnlockNotEnrolled;

  /// Vault settings: deviceUnlockUnsupported
  ///
  /// In en, this message translates to:
  /// **'Biometric unlock is currently unavailable on this device. If your Mac supports Touch ID, configure it in System Settings → Touch ID & Password.'**
  String get deviceUnlockUnsupported;

  /// Vault settings: deviceUnlockMacPassword
  ///
  /// In en, this message translates to:
  /// **'macOS currently offers its account password. Fingerprint unlock needs available Touch ID with an enrolled fingerprint: System Settings → Touch ID & Password.'**
  String get deviceUnlockMacPassword;

  /// Vault settings: deviceUnlockRecoveryUnavailable
  ///
  /// In en, this message translates to:
  /// **'Unlock with this device is disabled or currently unavailable. Use your Recovery Key. You can enable it after signing in: Settings → Vault.'**
  String get deviceUnlockRecoveryUnavailable;

  /// Vault settings: deviceUnlockRefresh
  ///
  /// In en, this message translates to:
  /// **'Check availability again'**
  String get deviceUnlockRefresh;

  /// Vault setting label with the available authentication method
  ///
  /// In en, this message translates to:
  /// **'Unlock with {method}'**
  String deviceUnlockWithMethod(String method);

  /// macOS account password authentication method
  ///
  /// In en, this message translates to:
  /// **'macOS password'**
  String get deviceUnlockMacPasswordName;

  /// Workspace tool panel: workspaceToolsClose
  ///
  /// In en, this message translates to:
  /// **'Close right panel'**
  String get workspaceToolsClose;

  /// Workspace tool panel: workspaceToolsResize
  ///
  /// In en, this message translates to:
  /// **'Right panel width'**
  String get workspaceToolsResize;

  /// Snippet library: snippetPackage
  ///
  /// In en, this message translates to:
  /// **'Package'**
  String get snippetPackage;

  /// Snippet library: snippetPackageHint
  ///
  /// In en, this message translates to:
  /// **'Choose an existing name or enter a new one. Leave empty for no package.'**
  String get snippetPackageHint;

  /// Snippet library: snippetAllPackages
  ///
  /// In en, this message translates to:
  /// **'All snippets'**
  String get snippetAllPackages;

  /// Snippet library: snippetNoPackage
  ///
  /// In en, this message translates to:
  /// **'Without a package'**
  String get snippetNoPackage;

  /// Snippet library: snippetStarterCatalog
  ///
  /// In en, this message translates to:
  /// **'Starter packages'**
  String get snippetStarterCatalog;

  /// Snippet library: snippetStarterIntro
  ///
  /// In en, this message translates to:
  /// **'Add useful commands to your vault. You can edit or delete them afterwards; changes sync with your vault.'**
  String get snippetStarterIntro;

  /// Snippet library: snippetStarterOrigin
  ///
  /// In en, this message translates to:
  /// **'From the {package} starter package.'**
  String snippetStarterOrigin(String package);

  /// Snippet library: snippetAddCount
  ///
  /// In en, this message translates to:
  /// **'Add {count} commands'**
  String snippetAddCount(int count);

  /// Snippet library: snippetPackageAdded
  ///
  /// In en, this message translates to:
  /// **'Added'**
  String get snippetPackageAdded;

  /// Snippet library: snippetPackageRename
  ///
  /// In en, this message translates to:
  /// **'Rename package'**
  String get snippetPackageRename;

  /// Snippet library: snippetPackageDelete
  ///
  /// In en, this message translates to:
  /// **'Delete package'**
  String get snippetPackageDelete;

  /// Snippet library: snippetPackageDeleteMessage
  ///
  /// In en, this message translates to:
  /// **'Delete all {count} snippets in this package? This deletion also syncs to your other devices.'**
  String snippetPackageDeleteMessage(int count);

  /// Snippet library: snippetMovePackage
  ///
  /// In en, this message translates to:
  /// **'Move to package…'**
  String get snippetMovePackage;

  /// Snippet library: snippetPaste
  ///
  /// In en, this message translates to:
  /// **'Paste'**
  String get snippetPaste;

  /// Snippet library: snippetRunMany
  ///
  /// In en, this message translates to:
  /// **'Run in multiple terminals…'**
  String get snippetRunMany;

  /// Snippet library: snippetChooseTerminals
  ///
  /// In en, this message translates to:
  /// **'Choose terminals'**
  String get snippetChooseTerminals;

  /// Snippet library: snippetConnectedOnly
  ///
  /// In en, this message translates to:
  /// **'Only connected terminals are listed. The command and all targets are reviewed before running.'**
  String get snippetConnectedOnly;

  /// Snippet library: snippetNoConnected
  ///
  /// In en, this message translates to:
  /// **'Connect a terminal first.'**
  String get snippetNoConnected;

  /// Snippet library: snippetTargetsChanged
  ///
  /// In en, this message translates to:
  /// **'The profile or selected terminals changed. Choose your targets again.'**
  String get snippetTargetsChanged;

  /// Snippet library: snippetUnsafePaste
  ///
  /// In en, this message translates to:
  /// **'This terminal does not support safe multiline paste. Use Run to review the script before execution.'**
  String get snippetUnsafePaste;

  /// Snippet library: snippetSyncHint
  ///
  /// In en, this message translates to:
  /// **'Saved in the vault · synced when sync is enabled'**
  String get snippetSyncHint;

  /// Snippet library: snippetPackLinux
  ///
  /// In en, this message translates to:
  /// **'Linux diagnostics'**
  String get snippetPackLinux;

  /// Snippet library: snippetPackLinuxDescription
  ///
  /// In en, this message translates to:
  /// **'System load, disks, memory, ports and service logs. For Linux hosts.'**
  String get snippetPackLinuxDescription;

  /// Snippet library: snippetPackDocker
  ///
  /// In en, this message translates to:
  /// **'Docker'**
  String get snippetPackDocker;

  /// Snippet library: snippetPackDockerDescription
  ///
  /// In en, this message translates to:
  /// **'Containers, Compose, resource usage and logs. Requires Docker on the host.'**
  String get snippetPackDockerDescription;

  /// Snippet library: snippetPackKubernetes
  ///
  /// In en, this message translates to:
  /// **'Kubernetes'**
  String get snippetPackKubernetes;

  /// Snippet library: snippetPackKubernetesDescription
  ///
  /// In en, this message translates to:
  /// **'Nodes, pods, services, logs and events. Requires a configured kubectl context.'**
  String get snippetPackKubernetesDescription;

  /// Snippet library: snippetCatalogUptime
  ///
  /// In en, this message translates to:
  /// **'System load and uptime'**
  String get snippetCatalogUptime;

  /// Snippet library: snippetCatalogDisk
  ///
  /// In en, this message translates to:
  /// **'Disk space'**
  String get snippetCatalogDisk;

  /// Snippet library: snippetCatalogMemory
  ///
  /// In en, this message translates to:
  /// **'Memory usage'**
  String get snippetCatalogMemory;

  /// Snippet library: snippetCatalogPorts
  ///
  /// In en, this message translates to:
  /// **'Listening ports'**
  String get snippetCatalogPorts;

  /// Snippet library: snippetCatalogJournal
  ///
  /// In en, this message translates to:
  /// **'Service logs'**
  String get snippetCatalogJournal;

  /// Snippet library: snippetCatalogContainers
  ///
  /// In en, this message translates to:
  /// **'All containers'**
  String get snippetCatalogContainers;

  /// Snippet library: snippetCatalogCompose
  ///
  /// In en, this message translates to:
  /// **'Compose project status'**
  String get snippetCatalogCompose;

  /// Snippet library: snippetCatalogStats
  ///
  /// In en, this message translates to:
  /// **'Container resource usage'**
  String get snippetCatalogStats;

  /// Snippet library: snippetCatalogDockerLogs
  ///
  /// In en, this message translates to:
  /// **'Container logs'**
  String get snippetCatalogDockerLogs;

  /// Snippet library: snippetCatalogDockerDisk
  ///
  /// In en, this message translates to:
  /// **'Docker disk usage'**
  String get snippetCatalogDockerDisk;

  /// Snippet library: snippetCatalogNodes
  ///
  /// In en, this message translates to:
  /// **'Cluster nodes'**
  String get snippetCatalogNodes;

  /// Snippet library: snippetCatalogPods
  ///
  /// In en, this message translates to:
  /// **'Pods in namespace'**
  String get snippetCatalogPods;

  /// Snippet library: snippetCatalogServices
  ///
  /// In en, this message translates to:
  /// **'Services in namespace'**
  String get snippetCatalogServices;

  /// Snippet library: snippetCatalogPodLogs
  ///
  /// In en, this message translates to:
  /// **'Pod logs'**
  String get snippetCatalogPodLogs;

  /// Snippet library: snippetCatalogEvents
  ///
  /// In en, this message translates to:
  /// **'Recent cluster events'**
  String get snippetCatalogEvents;

  /// Accessible tooltip for scrolling the overflowing terminal tab strip.
  ///
  /// In en, this message translates to:
  /// **'Scroll tabs left'**
  String get glassScrollTabsLeft;

  /// Accessible tooltip for scrolling the overflowing terminal tab strip.
  ///
  /// In en, this message translates to:
  /// **'Scroll tabs right'**
  String get glassScrollTabsRight;

  /// Total host and group counts in the current vault.
  ///
  /// In en, this message translates to:
  /// **'{hosts} hosts · {groups} groups'**
  String inventoryOverview(int hosts, int groups);

  /// Host and group inventory browser.
  ///
  /// In en, this message translates to:
  /// **'All hosts'**
  String get inventoryAllHosts;

  /// Host and group inventory browser.
  ///
  /// In en, this message translates to:
  /// **'Ungrouped'**
  String get inventoryUngrouped;

  /// Host and group inventory browser.
  ///
  /// In en, this message translates to:
  /// **'GROUPS'**
  String get inventoryFolders;

  /// Host and group inventory browser.
  ///
  /// In en, this message translates to:
  /// **'Location'**
  String get inventoryLocation;

  /// Host and group inventory browser.
  ///
  /// In en, this message translates to:
  /// **'Expand group'**
  String get inventoryExpand;

  /// Host and group inventory browser.
  ///
  /// In en, this message translates to:
  /// **'Collapse group'**
  String get inventoryCollapse;

  /// Host and group inventory browser.
  ///
  /// In en, this message translates to:
  /// **'Search hosts, groups, addresses or tags'**
  String get inventorySearch;

  /// Host and group inventory browser.
  ///
  /// In en, this message translates to:
  /// **'Cards'**
  String get inventoryCards;

  /// Host and group inventory browser.
  ///
  /// In en, this message translates to:
  /// **'List'**
  String get inventoryList;

  /// Host and group inventory browser.
  ///
  /// In en, this message translates to:
  /// **'Group settings'**
  String get inventoryGroupSettings;

  /// Host and group inventory browser.
  ///
  /// In en, this message translates to:
  /// **'Saved settings and inheritance'**
  String get inventoryEffectiveDefaults;

  /// Host and group inventory browser.
  ///
  /// In en, this message translates to:
  /// **'Hosts in this group and its subgroups'**
  String get inventoryIncludesSubgroups;

  /// Host and group inventory browser.
  ///
  /// In en, this message translates to:
  /// **'Add hosts or groups to organize your servers.'**
  String get inventoryEmptyHelp;

  /// Host and group inventory browser.
  ///
  /// In en, this message translates to:
  /// **'Try another search or clear the filters.'**
  String get inventorySearchHelp;

  /// Host and group inventory browser.
  ///
  /// In en, this message translates to:
  /// **'Clear filters'**
  String get inventoryClearFilters;

  /// Host and group inventory browser.
  ///
  /// In en, this message translates to:
  /// **'Active SSH connection'**
  String get inventoryConnected;

  /// Device-local right workspace panel presentation.
  ///
  /// In en, this message translates to:
  /// **'Right panel'**
  String get settingsWorkspacePanel;

  /// Device-local right workspace panel presentation.
  ///
  /// In en, this message translates to:
  /// **'Bubbles'**
  String get settingsWorkspaceFloating;

  /// Device-local right workspace panel presentation.
  ///
  /// In en, this message translates to:
  /// **'Unified panel'**
  String get settingsWorkspaceExpanded;

  /// Device-local right workspace panel presentation.
  ///
  /// In en, this message translates to:
  /// **'Separate floating panels beside the tool icons. This preference is saved on this device.'**
  String get settingsWorkspaceFloatingHelp;

  /// Device-local right workspace panel presentation.
  ///
  /// In en, this message translates to:
  /// **'The right rail expands into one panel with Snippets and AI chat tabs. It shares the workspace in wide windows and overlays it in narrow windows. Saved on this device.'**
  String get settingsWorkspaceExpandedHelp;

  /// Device-local right workspace panel presentation.
  ///
  /// In en, this message translates to:
  /// **'Collapse right panel'**
  String get workspaceToolsCollapse;

  /// Mobile navigation and terminal controls.
  ///
  /// In en, this message translates to:
  /// **'More'**
  String get mobileMore;

  /// Mobile navigation and terminal controls.
  ///
  /// In en, this message translates to:
  /// **'Keyboard'**
  String get mobileKeyboard;

  /// Mobile navigation and terminal controls.
  ///
  /// In en, this message translates to:
  /// **'Tools'**
  String get mobileTools;

  /// Android mobile interface.
  ///
  /// In en, this message translates to:
  /// **'Message…'**
  String get mobileMessageHint;

  /// Android mobile interface.
  ///
  /// In en, this message translates to:
  /// **'Fingerprint or PIN'**
  String get mobileDeviceAuth;

  /// Android mobile interface.
  ///
  /// In en, this message translates to:
  /// **'Confirm your identity using the fingerprint sensor or your phone’s screen lock.'**
  String get mobileDeviceAuthHelp;

  /// Availability of backup scheduling on Android preview.
  ///
  /// In en, this message translates to:
  /// **'Manual encrypted backups can be saved to Files. Automatic scheduled backups are not yet available on Android.'**
  String get mobileBackupsHelp;

  /// SFTP default editor preference in device-local settings.
  ///
  /// In en, this message translates to:
  /// **'Default editor'**
  String get sftpDefaultEditor;

  /// SFTP default editor preference in device-local settings.
  ///
  /// In en, this message translates to:
  /// **'System default'**
  String get sftpSystemEditor;

  /// SFTP default editor preference in device-local settings.
  ///
  /// In en, this message translates to:
  /// **'Open and Open in editor use this application. Open With lets you choose another application for one file. Saved only on this device.'**
  String get sftpDefaultEditorHelp;

  /// Terminal context menu or its selected-text attachment.
  ///
  /// In en, this message translates to:
  /// **'Terminal actions'**
  String get terminalContextMenu;

  /// Terminal context menu or its selected-text attachment.
  ///
  /// In en, this message translates to:
  /// **'Paste'**
  String get terminalPaste;

  /// Terminal context menu or its selected-text attachment.
  ///
  /// In en, this message translates to:
  /// **'Save as snippet…'**
  String get terminalSaveSnippet;

  /// Terminal context menu or its selected-text attachment.
  ///
  /// In en, this message translates to:
  /// **'Select entire buffer'**
  String get terminalSelectAll;

  /// Terminal context menu or its selected-text attachment.
  ///
  /// In en, this message translates to:
  /// **'Clear selection'**
  String get terminalClearSelection;

  /// Terminal context menu or its selected-text attachment.
  ///
  /// In en, this message translates to:
  /// **'Paste multiple lines or control characters?'**
  String get terminalPasteConfirmTitle;

  /// Terminal context menu or its selected-text attachment.
  ///
  /// In en, this message translates to:
  /// **'This text may execute commands or interrupt a running process. Review it before pasting.'**
  String get terminalPasteConfirmMessage;

  /// Terminal context menu or its selected-text attachment.
  ///
  /// In en, this message translates to:
  /// **'Selected text · {host}'**
  String aiTerminalAttachment(String host);

  /// Terminal context menu or its selected-text attachment.
  ///
  /// In en, this message translates to:
  /// **'Remove selected text'**
  String get aiRemoveAttachment;

  /// Screen capture privacy preference: screenCaptureTitle
  ///
  /// In en, this message translates to:
  /// **'Screenshots and screen recording'**
  String get screenCaptureTitle;

  /// Screen capture privacy preference: screenCaptureAllow
  ///
  /// In en, this message translates to:
  /// **'Allow screenshots'**
  String get screenCaptureAllow;

  /// Screen capture privacy preference: screenCaptureHelp
  ///
  /// In en, this message translates to:
  /// **'Also allows screen recording and app previews in Recent apps. Turn off to hide the app content from capture.'**
  String get screenCaptureHelp;

  /// Screen capture privacy preference: screenCaptureLocalOnly
  ///
  /// In en, this message translates to:
  /// **'Applies immediately to this entire app on this device, including the lock screen. Saved after restart; never synced.'**
  String get screenCaptureLocalOnly;

  /// Screen capture privacy preference: screenCaptureMacosHelp
  ///
  /// In en, this message translates to:
  /// **'Screenshots are allowed. Current macOS versions do not provide a reliable way for this app to block screenshots or screen recording.'**
  String get screenCaptureMacosHelp;

  /// Screen capture privacy preference: screenCaptureError
  ///
  /// In en, this message translates to:
  /// **'Could not read or apply the screenshot setting. Refresh its status and try again.'**
  String get screenCaptureError;
}

class _AppLocalizationsDelegate extends LocalizationsDelegate<AppLocalizations> {
  const _AppLocalizationsDelegate();

  @override
  Future<AppLocalizations> load(Locale locale) {
    return SynchronousFuture<AppLocalizations>(lookupAppLocalizations(locale));
  }

  @override
  bool isSupported(Locale locale) => <String>['en', 'ru'].contains(locale.languageCode);

  @override
  bool shouldReload(_AppLocalizationsDelegate old) => false;
}

AppLocalizations lookupAppLocalizations(Locale locale) {
  // Lookup logic when only language code is specified.
  switch (locale.languageCode) {
    case 'en':
      return AppLocalizationsEn();
    case 'ru':
      return AppLocalizationsRu();
  }

  throw FlutterError(
    'AppLocalizations.delegate failed to load unsupported locale "$locale". This is likely '
    'an issue with the localizations generation tool. Please file an issue '
    'on GitHub with a reproducible sample app and the gen-l10n configuration '
    'that was used.',
  );
}
