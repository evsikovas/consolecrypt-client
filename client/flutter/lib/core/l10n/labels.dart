/// Localized names for model enums. Models mirror cc-models (wire names
/// only); everything a user reads comes from here. Technical tokens
/// (key algorithms, OS names, snippet languages, AI product names) are
/// the same in every language.
library;

import 'package:consolecrypt/app/platform.dart';

import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/security/passphrase_strength.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';

extension DeviceUnlockInfoL10n on DeviceUnlockInfo {
  String authName(AppLocalizations l) => switch (kind) {
    DeviceAuthKind.touchId => 'Touch ID',
    DeviceAuthKind.faceId => 'Face ID', // l10n-ignore: Apple product name
    DeviceAuthKind.windowsHello => 'Windows Hello',
    DeviceAuthKind.deviceCredential => AppPlatform.isMobile ? l.mobileDeviceAuth : l.deviceUnlockMacPasswordName,
    null => l.platformSystemAuthentication,
  };
}

extension ServerUrlProblemL10n on ServerUrlProblem {
  String localized(AppLocalizations l) => switch (this) {
    ServerUrlProblem.empty => l.serverUrlEmpty,
    ServerUrlProblem.notAUrl => l.serverUrlNotAUrl,
    ServerUrlProblem.insecure => l.serverUrlInsecure,
  };
}

extension BackupFrequencyL10n on BackupFrequency {
  String localized(AppLocalizations l) => switch (this) {
    BackupFrequency.daily => l.backupFrequencyDaily,
    BackupFrequency.weekly => l.backupFrequencyWeekly,
  };
}

extension AiProviderKindL10n on AiProviderKind {
  String localized(AppLocalizations l) => switch (this) {
    AiProviderKind.ollama => 'Ollama',
    AiProviderKind.lmStudio => 'LM Studio',
    AiProviderKind.deepseek => 'DeepSeek',
    AiProviderKind.openaiCompatible => l.aiProviderKindOpenaiCompatible,
  };
}

extension PrivacyProfileL10n on PrivacyProfile {
  String localized(AppLocalizations l) => switch (this) {
    PrivacyProfile.strict => l.privacyProfileStrict,
    PrivacyProfile.standard => l.privacyProfileStandard,
    PrivacyProfile.local => l.privacyProfileLocal,
  };

  String localizedDescription(AppLocalizations l) => switch (this) {
    PrivacyProfile.strict => l.privacyProfileStrictDescription,
    PrivacyProfile.standard => l.privacyProfileStandardDescription,
    PrivacyProfile.local => l.privacyProfileLocalDescription,
  };
}

extension CredentialKindL10n on CredentialKind {
  String localized(AppLocalizations l) => switch (this) {
    CredentialKind.password => l.credentialKindPassword,
    CredentialKind.sshPrivateKey => l.credentialKindSshKey,
    CredentialKind.sshCertificate => l.credentialKindSshCertificate,
    CredentialKind.osSshAgent => l.credentialKindOsSshAgent,
    CredentialKind.fido2 => l.credentialKindFido2,
    CredentialKind.externalAgent => l.credentialKindExternalAgent,
  };
}

extension KeyAlgorithmL10n on KeyAlgorithm {
  /// Algorithm names are technical tokens (not translated).
  String localized(AppLocalizations l) => label;
}

extension DevicePlatformL10n on DevicePlatform {
  /// OS names are not translated.
  String localized(AppLocalizations l) => label;
}

extension HostKeyPolicyL10n on HostKeyPolicy {
  String localized(AppLocalizations l) => switch (this) {
    HostKeyPolicy.ask => l.hostKeyPolicyAsk,
    HostKeyPolicy.strict => l.hostKeyPolicyStrict,
    HostKeyPolicy.acceptNew => l.hostKeyPolicyAcceptNew,
  };

  String localizedDescription(AppLocalizations l) => switch (this) {
    HostKeyPolicy.ask => l.hostKeyPolicyAskDescription,
    HostKeyPolicy.strict => l.hostKeyPolicyStrictDescription,
    HostKeyPolicy.acceptNew => l.hostKeyPolicyAcceptNewDescription,
  };
}

extension SshBackendL10n on SshBackend {
  String localized(AppLocalizations l) => switch (this) {
    SshBackend.native => l.sshBackendNative,
    SshBackend.openSsh => l.sshBackendOpenSsh,
  };
}

extension KnownHostSourceL10n on KnownHostSource {
  String localized(AppLocalizations l) => switch (this) {
    KnownHostSource.tofu => l.knownHostSourceTofu,
    KnownHostSource.manual => l.knownHostSourceManual,
    KnownHostSource.imported => l.knownHostSourceImported,
    KnownHostSource.certAuthority => l.knownHostSourceCertAuthority,
  };
}

extension TerminalHistoryModeL10n on TerminalHistoryMode {
  String localized(AppLocalizations l) => switch (this) {
    TerminalHistoryMode.localOnly => l.historyModeLocalOnly,
    TerminalHistoryMode.encryptedSync => l.historyModeEncryptedSync,
    TerminalHistoryMode.disabled => l.historyModeDisabled,
  };

  String localizedDescription(AppLocalizations l) => switch (this) {
    TerminalHistoryMode.localOnly => l.historyModeLocalOnlyDescription,
    TerminalHistoryMode.encryptedSync => l.historyModeEncryptedSyncDescription,
    TerminalHistoryMode.disabled => l.historyModeDisabledDescription,
  };
}

extension ProfileKindL10n on ProfileKind {
  String localized(AppLocalizations l) => switch (this) {
    ProfileKind.local => l.profileKindLocal,
    ProfileKind.synced => l.profileKindSynced,
  };
}

extension ProfileL10n on Profile {
  /// Secondary line for switchers: "Local only" / "user@host · sync.example.org".
  String localizedSubtitle(AppLocalizations l) => isLocal
      ? l.profileKindLocal
      : l.profileSyncedSubtitle(accountEmail ?? l.profileUnknownAccount, serverUrl?.host ?? l.profileUnknownServer);
}

extension EnableSyncStepL10n on EnableSyncStep {
  String localized(AppLocalizations l) => switch (this) {
    EnableSyncStep.authenticating => l.enableSyncStepAuthenticating,
    EnableSyncStep.creatingRemoteVault => l.enableSyncStepCreatingRemoteVault,
    EnableSyncStep.reconnecting => l.enableSyncStepReconnecting,
    EnableSyncStep.uploading => l.enableSyncStepUploading,
    EnableSyncStep.finishing => l.enableSyncStepFinishing,
    EnableSyncStep.done => l.enableSyncStepDone,
    EnableSyncStep.failed => l.enableSyncStepFailed,
  };
}

extension TunnelKindL10n on TunnelKind {
  String localized(AppLocalizations l) => switch (this) {
    TunnelKind.local => l.tunnelKindLocal,
    TunnelKind.remote => l.tunnelKindRemote,
    TunnelKind.dynamic => l.tunnelKindDynamic,
  };

  String localizedDescription(AppLocalizations l) => switch (this) {
    TunnelKind.local => l.tunnelKindLocalDescription,
    TunnelKind.remote => l.tunnelKindRemoteDescription,
    TunnelKind.dynamic => l.tunnelKindDynamicDescription,
  };
}

extension SnippetTypeL10n on SnippetType {
  /// Shell / tool names are not translated.
  String localized(AppLocalizations l) => label;
}

extension RiskLevelL10n on RiskLevel {
  String localized(AppLocalizations l) => switch (this) {
    RiskLevel.readOnly => l.riskReadOnly,
    RiskLevel.modifying => l.riskModifying,
    RiskLevel.destructive => l.riskDestructive,
    RiskLevel.unknown => l.riskUnknown,
  };
}

extension SnippetSourceL10n on SnippetSource {
  String localized(AppLocalizations l) => switch (this) {
    SnippetSource.user => l.snippetSourceUser,
    SnippetSource.ai => l.snippetSourceAi,
    SnippetSource.imported => l.snippetSourceImported,
    SnippetSource.history => l.snippetSourceHistory,
  };
}

extension PassphraseStrengthL10n on PassphraseStrength {
  String localizedLabel(AppLocalizations l) => switch (score) {
    0 => l.strengthVeryWeak,
    1 => l.strengthWeak,
    2 => l.strengthFair,
    3 => l.strengthStrong,
    _ => l.strengthVeryStrong,
  };

  String localizedHint(AppLocalizations l) => switch (hint) {
    PassphraseHint.empty => l.strengthHintEmpty(PassphraseStrength.minLength),
    PassphraseHint.sequence => l.strengthHintSequence,
    PassphraseHint.common => l.strengthHintCommon,
    PassphraseHint.tooShort => l.strengthHintTooShort(PassphraseStrength.minLength),
    PassphraseHint.good => l.strengthHintGood,
    PassphraseHint.addMore => l.strengthHintAddMore,
  };
}
