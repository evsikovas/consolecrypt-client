/// Mechanical mapping between the Rust core's DTOs and `core/models`.
///
/// The bridge (`client/rust/bridge`) passes app-core DTOs as JSON — the serde
/// form documented in ADR-0107 (snake_case fields, enum wire names = the
/// `wireName`s of the Dart enums, Unix-millisecond timestamps). Writers start
/// from the last DTO seen for the object (`base`) so fields the Dart models do
/// not carry (e.g. `proxy_command`) survive a round trip.
library;

import 'dart:convert';

import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/models/sftp_browser.dart';
import 'package:consolecrypt/core/models/sftp_edit.dart';
import 'package:consolecrypt/core/models/terminal_colors.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/src/rust/api/error.dart';

typedef Json = Map<String, Object?>;

DeviceUnlockInfo deviceUnlockFromJson(Json json) => DeviceUnlockInfo(
  enabled: json['enabled'] == true,
  kind: switch (json['kind']) {
    'touch_id' => DeviceAuthKind.touchId,
    'face_id' => DeviceAuthKind.faceId,
    'windows_hello' => DeviceAuthKind.windowsHello,
    'device_credential' => DeviceAuthKind.deviceCredential,
    _ => null,
  },
  hasDeviceEnvelope: json['has_device_envelope'] == true,
  notEnrolled: json['not_enrolled'] == true,
);

// ---- JSON helpers ------------------------------------------------------------------

Json decodeObject(String json) => (jsonDecode(json) as Map).cast<String, Object?>();

List<Json> decodeList(String json) => [for (final e in jsonDecode(json) as List) (e as Map).cast<String, Object?>()];

String encodeJson(Object? value) => jsonEncode(value);

/// `["a","b"]` → `['a', 'b']`; [FormatException] for anything else.
List<String> decodeStringList(String json) {
  final value = jsonDecode(json);
  if (value is! List) throw const FormatException('not a list');
  return [for (final e in value) e is String ? e : throw const FormatException('not a string')];
}

DateTime fromMs(Object? v) => DateTime.fromMillisecondsSinceEpoch((v as num? ?? 0).toInt(), isUtc: true);

DateTime? fromMsOrNull(Object? v) => v == null ? null : fromMs(v);

int toMs(DateTime t) => t.toUtc().millisecondsSinceEpoch;

String? _str(Object? v) => v as String?;

int? _int(Object? v) => (v as num?)?.toInt();

bool _bool(Object? v) => v as bool? ?? false;

List<String> _strings(Object? v) => [for (final e in v as List? ?? const []) e as String];

Map<String, String> _stringMap(Object? v) => {
  for (final e in (v as Map? ?? const {}).entries) e.key as String: e.value as String,
};

T _byWire<T extends Enum>(List<T> values, String Function(T) wire, Object? v, T fallback) {
  for (final e in values) {
    if (wire(e) == v) return e;
  }
  return fallback;
}

String? _emptyToNull(String? s) => (s == null || s.trim().isEmpty) ? null : s;

ObjectId? _id(Object? v) {
  final s = _emptyToNull(v as String?);
  return s == null ? null : ObjectId(s);
}

/// Marker for "a secret is stored" (the core reports `has_*` flags, never
/// secret ids or values). Only compared against `null` by the UI.
const storedSecretMarker = ObjectId('stored-in-core');

// ---- errors ------------------------------------------------------------------------

/// Turns anything a bridge call threw into the one exception type services
/// throw. Messages are English diagnostics; the UI renders code/reason/args.
AppException toAppException(Object error) {
  if (error is AppException) return error;
  if (error is BridgeError) return mapBridgeError(error);
  return AppException(AppErrorCode.internal, 'Unexpected core failure (${error.runtimeType})');
}

/// Runs a bridge call and maps its errors.
Future<T> guard<T>(Future<T> Function() call) async {
  try {
    return await call();
  } on AppException {
    rethrow;
  } catch (e) {
    throw toAppException(e);
  }
}

AppErrorReason? _notFoundReason(String? what) => switch ((what ?? '').toLowerCase()) {
  'host' => AppErrorReason.hostNotFound,
  'credential' => AppErrorReason.credentialNotFound,
  'profile' => AppErrorReason.profileNotFound,
  'tunnel' => AppErrorReason.tunnelNotFound,
  'aiprovider' || 'ai_provider' => AppErrorReason.providerNotFound,
  'device' => AppErrorReason.deviceNotFound,
  _ => null,
};

/// app-core `AppError::code` (ADR-0107) → [AppErrorCode] (+ reason/args).
/// A `reason` sent by the core (wire name) always wins over the derived one.
AppException mapBridgeError(BridgeError e) {
  final d = e.details;
  AppErrorReason? reason;
  var args = const <String, String>{};
  final AppErrorCode code;
  switch (e.code) {
    case 'invalid_input':
      code = AppErrorCode.validation;
      reason = AppErrorReason.invalidField;
      args = {'field': d['field'] ?? '', 'rule': d['rule'] ?? ''};
    case 'not_found':
      code = AppErrorCode.notFound;
      reason = _notFoundReason(d['what']);
    case 'in_use':
      code = AppErrorCode.conflict;
    case 'no_active_profile':
      code = AppErrorCode.notFound;
      reason = AppErrorReason.noActiveProfile;
    case 'vault_locked':
      code = AppErrorCode.notFound;
      reason = AppErrorReason.vaultLocked;
    case 'no_vault':
      code = AppErrorCode.notFound;
      reason = AppErrorReason.noVault;
    case 'wrong_passphrase':
      code = AppErrorCode.wrongPassphrase;
    case 'wrong_recovery_key':
      code = AppErrorCode.invalidRecoveryKey;
    case 'weak_passphrase':
      code = AppErrorCode.validation;
      reason = AppErrorReason.weakPassphrase;
    case 'device_not_authorized' || 'device_not_trusted':
      code = AppErrorCode.deviceNotTrusted;
    case 'secure_store':
      code = AppErrorCode.secureStore;
    case 'os_auth_failed':
      code = AppErrorCode.osAuthFailed;
    case 'local_profile':
      code = AppErrorCode.unsupported;
      reason = AppErrorReason.syncedProfileRequired;
    case 'already_synced':
      code = AppErrorCode.conflict;
    case 'offline' || 'ssh_connect' || 'ai_unavailable':
      code = AppErrorCode.serverUnreachable;
    case 'invalid_credentials':
      code = AppErrorCode.invalidCredentials;
    case 'already_exists':
      final what = (d['what'] ?? '').toLowerCase();
      if (e.reason == null && what.contains('email')) {
        code = AppErrorCode.emailTaken;
      } else {
        code = AppErrorCode.conflict;
        reason = AppErrorReason.alreadyExists;
        args = {'name': d['what'] ?? ''};
      }
    case 'reauth_required':
      code = AppErrorCode.sessionExpired;
    case 'new_device_identity_required' || 'device_revoked':
      code = AppErrorCode.deviceRevoked;
    case 'device_proof_rejected':
      code = AppErrorCode.invalidProof;
    case 'upgrade_required':
      code = AppErrorCode.incompatibleServer;
    case 'rate_limited':
      code = AppErrorCode.rateLimited;
      final retry = d['retry_after_seconds'];
      if (retry != null) args = {'retry_after_seconds': retry};
    case 'server':
      code = _serverCode(d['protocol_code']);
    case 'approval':
      code = AppErrorCode.verificationMismatch;
    case 'connection_plan' || 'confirmation_required':
      code = AppErrorCode.validation;
    case 'unresolved_placeholders':
      code = AppErrorCode.validation;
      reason = AppErrorReason.missingVariables;
      args = {'names': d['names'] ?? ''};
    case 'ssh_host_key_changed':
      code = AppErrorCode.hostKeyChanged;
    case 'ssh_host_key_rejected':
      code = AppErrorCode.hostKeyRejected;
    case 'ssh_auth_failed' || 'ai_auth_failed':
      code = AppErrorCode.authFailed;
    case 'ssh_passphrase_required':
      code = AppErrorCode.authFailed;
      reason = AppErrorReason.keyPassphraseRequired;
    case 'backup':
      code = AppErrorCode.validation;
      reason = AppErrorReason.notABackup;
    case 'ai_not_configured':
      code = AppErrorCode.notFound;
      reason = AppErrorReason.providerNotFound;
    case 'approval_required':
      code = AppErrorCode.forbidden;
    case 'unsupported':
      code = AppErrorCode.unsupported;
    case 'cancelled':
      code = AppErrorCode.cancelled;
    case 'permission_denied':
      code = AppErrorCode.forbidden;
    case 'payload_too_large':
      code = AppErrorCode.payloadTooLarge;
      args = {'size': d['size'] ?? '', 'limit': d['limit'] ?? ''};
    default:
      // ssh, tunnel, sftp, terminal, storage, secure_store, crypto, io,
      // internal, ai_provider, not_initialized, …
      code = AppErrorCode.internal;
  }
  final coreReason = AppErrorReason.fromWire(e.reason);
  // The core's reason wins; its details are the (non-secret) args.
  if (coreReason != null) args = {...args, ...d};
  return AppException(code, e.message, reason: coreReason ?? reason, args: args);
}

/// Server `cc_protocol::ErrorCode` (snake_case) → [AppErrorCode]; see the
/// table in `lib/core/l10n/error_messages.dart`.
AppErrorCode _serverCode(String? protocolCode) => switch (protocolCode) {
  'bad_request' => AppErrorCode.validation,
  'unauthorized' || 'refresh_token_reused' => AppErrorCode.sessionExpired,
  'invalid_credentials' => AppErrorCode.invalidCredentials,
  'forbidden' => AppErrorCode.forbidden,
  'device_revoked' => AppErrorCode.deviceRevoked,
  'device_not_trusted' => AppErrorCode.deviceNotTrusted,
  'email_not_verified' => AppErrorCode.emailNotVerified,
  'not_found' => AppErrorCode.notFound,
  'conflict' || 'already_exists' => AppErrorCode.conflict,
  'gone' => AppErrorCode.requestExpired,
  'payload_too_large' => AppErrorCode.payloadTooLarge,
  'invalid_proof' => AppErrorCode.invalidProof,
  'upgrade_required' => AppErrorCode.incompatibleServer,
  'rate_limited' => AppErrorCode.rateLimited,
  'unavailable' => AppErrorCode.serverUnavailable,
  _ => AppErrorCode.internal,
};

// ---- profiles & vault --------------------------------------------------------------

Profile profileFromJson(Json j) => Profile(
  id: ProfileId(j['id']! as String),
  name: j['display_name']! as String,
  kind: j['kind'] == 'synced' ? ProfileKind.synced : ProfileKind.local,
  createdAt: fromMs(j['created_at_ms']),
  lastUsedAt: fromMsOrNull(j['last_opened_at_ms']),
  serverUrl: _emptyToNull(_str(j['server_url'])) == null ? null : Uri.tryParse(j['server_url']! as String),
  accountEmail: _str(j['email']),
  deviceId: _emptyToNull(_str(j['device_id'])) == null ? null : DeviceId(j['device_id']! as String),
  vaultId: _emptyToNull(_str(j['vault_id'])) == null ? null : VaultId(j['vault_id']! as String),
);

/// The one-time Recovery Kit (`RecoveryKitDto`).
RecoveryKit recoveryKitFromJson(Json j) => RecoveryKit(
  vaultId: VaultId(j['vault_id']! as String),
  words: _strings(j['words']),
  qrPayload: j['qr_payload']! as String,
  createdAt: fromMs(j['created_at_ms']),
  serverUrl: _emptyToNull(_str(j['server_url'])) == null ? null : Uri.tryParse(j['server_url']! as String),
);

ServerInfo serverInfoFromJson(Json j) => ServerInfo(
  serverVersion: j['server_version']?.toString() ?? '',
  protocolVersion: j['protocol_version']?.toString() ?? '',
  upgradeRequired: _bool(j['upgrade_required']),
  registrationOpen: j['registration_open'] as bool? ?? true,
  emailVerificationRequired: _bool(j['email_verification_required']),
);

// ---- inventory ---------------------------------------------------------------------

Host hostFromJson(Json j) => Host(
  id: ObjectId(j['id']! as String),
  name: j['name']! as String,
  address: j['address']! as String,
  port: _int(j['port']),
  username: _str(j['username']),
  credentialId: _id(j['credential_id']),
  groupId: _id(j['group_id']),
  jumpChain: [for (final s in _strings(j['jump_chain'])) ObjectId(s)],
  jumpProfileId: _id(j['jump_profile_id']),
  proxyId: _id(j['proxy_id']),
  hostKeyPolicy: _byWire(HostKeyPolicy.values, (e) => e.wireName, j['host_key_policy'], HostKeyPolicy.ask),
  backend: _byWire(SshBackend.values, (e) => e.wireName, j['backend'], SshBackend.native),
  keepaliveSecs: _int(j['keepalive_secs']),
  agentForwarding: _bool(j['agent_forwarding']),
  tags: _strings(j['tags']),
  notes: _str(j['notes']) ?? '',
  metadata: _stringMap(j['metadata']),
  createdAt: fromMs(j['created_at_ms']),
  updatedAt: fromMs(j['updated_at_ms']),
);

Json hostToJson(Host h, {Json? base}) => {
  'proxy_command': null,
  'auth_mode': 'inherit',
  ...?base,
  'id': h.id.value,
  'name': h.name,
  'address': h.address,
  'port': h.port,
  'username': h.username,
  'credential_id': h.credentialId?.value,
  'group_id': h.groupId?.value,
  'jump_chain': [for (final id in h.jumpChain) id.value],
  'jump_profile_id': h.jumpProfileId?.value,
  'proxy_id': h.proxyId?.value,
  'host_key_policy': h.hostKeyPolicy.wireName,
  'backend': h.backend.wireName,
  'keepalive_secs': h.keepaliveSecs,
  'agent_forwarding': h.agentForwarding,
  'tags': h.tags,
  'notes': h.notes,
  'metadata': h.metadata,
  'created_at_ms': toMs(h.createdAt),
  'updated_at_ms': toMs(h.updatedAt),
};

Group groupFromJson(Json j) => Group(
  id: ObjectId(j['id']! as String),
  name: j['name']! as String,
  parentId: _id(j['parent_id']),
  inheritedUsername: _str(j['inherited_username']),
  inheritedPort: _int(j['inherited_port']),
  inheritedCredentialId: _id(j['inherited_credential_id']),
  inheritedJumpProfileId: _id(j['inherited_jump_profile_id']),
  tags: _strings(j['tags']),
  createdAt: fromMs(j['created_at_ms']),
  updatedAt: fromMs(j['updated_at_ms']),
);

Json groupToJson(Group g, {Json? base}) => {
  ...?base,
  'id': g.id.value,
  'name': g.name,
  'parent_id': g.parentId?.value,
  'inherited_username': g.inheritedUsername,
  'inherited_port': g.inheritedPort,
  'inherited_credential_id': g.inheritedCredentialId?.value,
  'inherited_jump_profile_id': g.inheritedJumpProfileId?.value,
  'tags': g.tags,
  'created_at_ms': toMs(g.createdAt),
  'updated_at_ms': toMs(g.updatedAt),
};

JumpProfile jumpProfileFromJson(Json j) => JumpProfile(
  id: ObjectId(j['id']! as String),
  name: j['name']! as String,
  chain: [for (final s in _strings(j['chain'])) ObjectId(s)],
  createdAt: fromMs(j['created_at_ms']),
  updatedAt: fromMs(j['updated_at_ms']),
);

Json jumpProfileToJson(JumpProfile p, {Json? base}) => {
  ...?base,
  'id': p.id.value,
  'name': p.name,
  'chain': [for (final id in p.chain) id.value],
  'created_at_ms': toMs(p.createdAt),
  'updated_at_ms': toMs(p.updatedAt),
};

Credential credentialFromJson(Json j) => Credential(
  id: ObjectId(j['id']! as String),
  name: j['name']! as String,
  kind: _byWire(CredentialKind.values, (e) => e.wireName, j['kind'], CredentialKind.password),
  username: _str(j['username']),
  secretId: _bool(j['has_secret']) ? storedSecretMarker : null,
  passphraseSecretId: _bool(j['has_remembered_passphrase']) ? storedSecretMarker : null,
  keyEncrypted: _bool(j['key_encrypted']),
  keyAlgorithm: j['key_algorithm'] == null
      ? null
      : _byWire(KeyAlgorithm.values, (e) => e.wireName, j['key_algorithm'], KeyAlgorithm.ed25519),
  publicKey: _str(j['public_key']),
  certificate: _str(j['certificate']),
  fingerprint: _str(j['fingerprint']),
  agentPath: _str(j['agent_path']),
  createdAt: fromMs(j['created_at_ms']),
  updatedAt: fromMs(j['updated_at_ms']),
);

KnownHost knownHostFromJson(Json j) => KnownHost(
  id: ObjectId(j['id']! as String),
  hostPattern: j['host_pattern']! as String,
  keyType: j['key_type']! as String,
  publicKey: j['public_key']! as String,
  fingerprintSha256: j['fingerprint_sha256']! as String,
  source: _byWire(KnownHostSource.values, (e) => e.wireName, j['source'], KnownHostSource.tofu),
  revoked: _bool(j['revoked']),
  addedAt: fromMs(j['added_at_ms']),
  updatedAt: fromMs(j['updated_at_ms']),
);

Tunnel tunnelFromJson(Json j) => Tunnel(
  id: ObjectId(j['id']! as String),
  name: j['name']! as String,
  kind: _byWire(TunnelKind.values, (e) => e.wireName, j['kind'], TunnelKind.local),
  hostId: ObjectId(j['host_id']! as String),
  bindHost: j['bind_host']! as String,
  bindPort: _int(j['bind_port']) ?? 0,
  targetHost: _str(j['target_host']),
  targetPort: _int(j['target_port']),
  autoStart: _bool(j['auto_start']),
  createdAt: fromMs(j['created_at_ms']),
  updatedAt: fromMs(j['updated_at_ms']),
);

Json tunnelToJson(Tunnel t, {Json? base}) => {
  'binds_publicly': false,
  ...?base,
  'id': t.id.value,
  'name': t.name,
  'kind': t.kind.wireName,
  'host_id': t.hostId.value,
  'bind_host': t.bindHost,
  'bind_port': t.bindPort,
  'target_host': t.targetHost,
  'target_port': t.targetPort,
  'auto_start': t.autoStart,
  'created_at_ms': toMs(t.createdAt),
  'updated_at_ms': toMs(t.updatedAt),
};

/// `TunnelStatusDto` → runtime row.
TunnelRuntime tunnelRuntimeFromJson(Json j) {
  final state = (j['state'] as Map?)?.cast<String, Object?>() ?? const {};
  final since = fromMsOrNull(j['started_at_ms']);
  final bytes = (_int(j['bytes_sent']) ?? 0) + (_int(j['bytes_received']) ?? 0);
  return switch (state['state']) {
    'running' => TunnelRuntime(
      state: TunnelRunState.running,
      since: since,
      activeConnections: _int(j['active_connections']) ?? 0,
      bytesTransferred: bytes,
    ),
    'failed' => TunnelRuntime(state: TunnelRunState.failed, since: since, error: _str(state['reason'])),
    _ => TunnelRuntime.stopped,
  };
}

Snippet snippetFromJson(Json j) => Snippet(
  id: ObjectId(j['id']! as String),
  name: j['name']! as String,
  description: _str(j['description']) ?? '',
  packageName: _emptyToNull(_str(j['package_name'])),
  catalogId: _emptyToNull(_str(j['catalog_id'])),
  snippetType: _byWire(SnippetType.values, (e) => e.wireName, j['snippet_type'], SnippetType.shell),
  shell: _str(j['shell']),
  template: j['template']! as String,
  variables: [
    for (final v in j['variables'] as List? ?? const [])
      SnippetVariable(
        name: (v as Map)['name']! as String,
        description: v['description'] as String? ?? '',
        defaultValue: v['default'] as String?,
        required: v['required'] as bool? ?? true,
      ),
  ],
  tags: _strings(j['tags']),
  riskLevel: _byWire(RiskLevel.values, (e) => e.wireName, j['risk_level'], RiskLevel.unknown),
  source: _byWire(SnippetSource.values, (e) => e.wireName, j['source'], SnippetSource.user),
  createdBy: _emptyToNull(_str(j['created_by_device_id'])) == null
      ? null
      : DeviceId(j['created_by_device_id']! as String),
  lastUsedAt: fromMsOrNull(j['last_used_at_ms']),
  usageCount: _int(j['usage_count']) ?? 0,
  createdAt: fromMs(j['created_at_ms']),
  updatedAt: fromMs(j['updated_at_ms']),
);

Json snippetToJson(Snippet s, {Json? base}) => {
  ...?base,
  'id': s.id.value,
  'name': s.name,
  'description': s.description,
  'package_name': s.packageName,
  'catalog_id': s.catalogId,
  'snippet_type': s.snippetType.wireName,
  'shell': s.shell,
  'template': s.template,
  'variables': [
    for (final v in s.variables)
      {'name': v.name, 'description': v.description, 'default': v.defaultValue, 'required': v.required},
  ],
  'tags': s.tags,
  'risk_level': s.riskLevel.wireName,
  'source': s.source.wireName,
  'created_by_device_id': s.createdBy?.value,
  'last_used_at_ms': s.lastUsedAt == null ? null : toMs(s.lastUsedAt!),
  'usage_count': s.usageCount,
  'created_at_ms': toMs(s.createdAt),
  'updated_at_ms': toMs(s.updatedAt),
};

VaultSettings vaultSettingsFromJson(Json j) => VaultSettings(
  id: ObjectId(j['id']! as String),
  vaultName: j['vault_name']! as String,
  terminalHistoryMode: _byWire(
    TerminalHistoryMode.values,
    (e) => e.wireName,
    j['terminal_history_mode'],
    TerminalHistoryMode.localOnly,
  ),
  defaultPrivacyProfile: _byWire(
    PrivacyProfile.values,
    (e) => e.wireName,
    j['default_privacy_profile'],
    PrivacyProfile.strict,
  ),
  syncAiConversations: _bool(j['sync_ai_conversations']),
  createdAt: fromMs(j['created_at_ms']),
  updatedAt: fromMs(j['updated_at_ms']),
);

Json vaultSettingsToJson(VaultSettings s, {Json? base}) => {
  ...?base,
  'id': s.id.value,
  'vault_name': s.vaultName,
  'terminal_history_mode': s.terminalHistoryMode.wireName,
  'default_privacy_profile': s.defaultPrivacyProfile.wireName,
  'sync_ai_conversations': s.syncAiConversations,
  'created_at_ms': toMs(s.createdAt),
  'updated_at_ms': toMs(s.updatedAt),
};

AiProviderConfig aiProviderFromJson(Json j) => AiProviderConfig(
  id: ObjectId(j['id']! as String),
  name: j['name']! as String,
  provider: _byWire(AiProviderKind.values, (e) => e.wireName, j['provider'], AiProviderKind.openaiCompatible),
  baseUrl: j['base_url']! as String,
  apiKeySecretId: _bool(j['has_api_key']) ? storedSecretMarker : null,
  chatModel: j['chat_model']! as String,
  embeddingModel: _str(j['embedding_model']),
  timeoutSecs: _int(j['timeout_secs']) ?? 60,
  streaming: j['streaming'] as bool? ?? true,
  toolSupport: _bool(j['tool_support']),
  privacyProfile: _byWire(PrivacyProfile.values, (e) => e.wireName, j['privacy_profile'], PrivacyProfile.strict),
  isDefault: _bool(j['is_default']),
  createdAt: fromMs(j['created_at_ms']),
  updatedAt: fromMs(j['updated_at_ms']),
);

Json aiProviderToJson(AiProviderConfig p, {Json? base}) => {
  'has_api_key': false,
  ...?base,
  'id': p.id.value,
  'name': p.name,
  'provider': p.provider.wireName,
  'base_url': p.baseUrl,
  'chat_model': p.chatModel,
  'embedding_model': p.embeddingModel,
  'timeout_secs': p.timeoutSecs,
  'streaming': p.streaming,
  'tool_support': p.toolSupport,
  'privacy_profile': p.privacyProfile.wireName,
  'is_default': p.isDefault,
  'created_at_ms': toMs(p.createdAt),
  'updated_at_ms': toMs(p.updatedAt),
};

// ---- sync & devices ----------------------------------------------------------------

/// `SyncStatusDto` → [SyncStatus].
SyncStatus syncStatusFromJson(Json j, {DateTime? now}) {
  final state = switch (j['phase']) {
    'local_only' => SyncState.localOnly,
    'idle' => SyncState.idle,
    'syncing' => SyncState.syncing,
    'offline' => SyncState.offline,
    'error' || 'stopped' => SyncState.error,
    _ => SyncState.paused,
  };
  final at = now ?? DateTime.now().toUtc();
  final retryMs = _int(j['next_retry_in_ms']);
  final issues = <SyncIssue>[
    if (_str(j['last_error']) case final message?) SyncIssue(at: at, message: message),
    if (_str(j['stop_reason']) case final reason?) SyncIssue(at: at, message: reason, retryable: false),
  ];
  return SyncStatus(
    state: state,
    pendingChanges: _int(j['pending']) ?? 0,
    lastSyncAt: fromMsOrNull(j['last_sync_at_ms']),
    lastServerSequence: _int(j['last_sequence']),
    nextRetryAt: retryMs == null ? null : at.add(Duration(milliseconds: retryMs)),
    issues: issues,
  );
}

DevicePlatform _platform(Object? wire) => _byWire(DevicePlatform.values, (e) => e.wireName, wire, DevicePlatform.cli);

/// `DeviceListDto` → [DevicesSnapshot]; `trusted_for_vault` refers to the
/// open profile's vault ([vaultId]).
DevicesSnapshot devicesFromJson(Json j, {VaultId? vaultId}) {
  final devices = [
    for (final d in (j['devices'] as List? ?? const []).cast<Map<Object?, Object?>>())
      DeviceInfo(
        deviceId: DeviceId(d['device_id']! as String),
        name: d['name']! as String,
        platform: _platform(d['platform']),
        status: d['status'] == 'revoked' ? DeviceStatus.revoked : DeviceStatus.active,
        trustedVaults: [if (d['trusted_for_vault'] == true && vaultId != null) vaultId],
        createdAt: fromMs(d['created_at_ms']),
        lastSeenAt: fromMsOrNull(d['last_seen_at_ms']),
        revokedAt: fromMsOrNull(d['revoked_at_ms']),
        isCurrent: d['is_current'] == true,
      ),
  ];
  final requests = [
    for (final r in (j['pending_requests'] as List? ?? const []).cast<Map<Object?, Object?>>())
      DeviceTrustRequest(
        requestId: DeviceRequestId(r['request_id']! as String),
        device: DeviceInfo(
          deviceId: DeviceId(r['device_id']! as String),
          name: r['device_name']! as String,
          platform: _platform(r['platform']),
          status: DeviceStatus.active,
          trustedVaults: const [],
          createdAt: fromMs(r['created_at_ms']),
        ),
        vaultIds: [for (final v in r['vault_ids'] as List? ?? const []) VaultId(v as String)],
        status: _byWire(DeviceRequestStatus.values, (e) => e.name, r['status'], DeviceRequestStatus.pending),
        createdAt: fromMs(r['created_at_ms']),
        expiresAt: fromMs(r['expires_at_ms']),
      ),
  ];
  return DevicesSnapshot(devices: devices, pendingRequests: requests);
}

/// "12345 67890 …" → [VerificationCode].
VerificationCode verificationCodeFrom(String text) =>
    VerificationCode.tryParse(text) ??
    (throw const AppException(AppErrorCode.internal, 'Malformed verification code from the core'));

// ---- files -------------------------------------------------------------------------

/// `RemoteEntryDto` → [FileEntry].
FileEntry remoteEntryFromJson(Json j) => FileEntry(
  name: j['name']! as String,
  path: j['path']! as String,
  isDirectory: _bool(j['is_dir']),
  size: _int(j['size']) ?? 0,
  modifiedAt: fromMsOrNull(j['modified_at_ms'])?.toLocal(),
  permissions: _str(j['mode']),
  isSymlink: j['kind'] == 'symlink',
);

/// Directories first, then files, each by name (case-insensitive).
List<FileEntry> sortEntries(Iterable<FileEntry> entries) => entries.toList()
  ..sort((a, b) {
    if (a.isDirectory != b.isDirectory) return a.isDirectory ? -1 : 1;
    return a.name.toLowerCase().compareTo(b.name.toLowerCase());
  });

// ---- device-local UI state (own JSON format, never synced) --------------------------

Json localSettingsToJson(LocalSettings s) => {
  'theme_mode': s.themeMode.name,
  'ui_font_scale': s.uiFontScale,
  'ui_font_scale_version': 2,
  'ui_accent_color': s.uiAccentColor,
  'ui_background_color': s.uiBackgroundColor,
  'terminal_color_scheme': s.terminalColorScheme.name,
  'custom_terminal_colors': s.customTerminalColors?.toJson(),
  'reopen_last_profile': s.reopenLastProfile,
  'check_updates_automatically': s.checkUpdatesAutomatically,
  'terminal_font_size': s.terminalFontSize,
  'terminal_scrollback': s.terminalScrollback,
  'clipboard_clear_seconds': s.clipboardClearSeconds,
  'auto_lock_minutes': s.autoLockMinutes,
  'app_locale': s.appLocale.wireName,
  'glass_mode': s.glassMode.wireName,
  'sidebar_style': s.sidebarStyle.wireName,
  'workspace_panel_style': s.workspacePanelStyle.name,
  'sftp_default_editor': s.sftpDefaultEditor == null
      ? null
      : {'kind': s.sftpDefaultEditor!.kind.wireName, 'value': s.sftpDefaultEditor!.value},
};

LocalSettings localSettingsFromJson(Json j) {
  const d = LocalSettings();
  int? rgb(Object? value) => value is int && value >= 0 && value <= 0xFFFFFF ? value : null;
  final scale = j['ui_font_scale'];
  // Old 90% becomes 100% exactly; migrate once without shrinking twice.
  final normalizedScale = scale is num && scale.isFinite
      ? scale.toDouble() / (j['ui_font_scale_version'] == 2 ? 1 : LocalSettings.uiFontBase)
      : d.uiFontScale;
  return LocalSettings(
    themeMode: _byWire(AppThemeMode.values, (e) => e.name, j['theme_mode'], d.themeMode),
    uiFontScale: normalizedScale.clamp(LocalSettings.uiFontScaleMin, LocalSettings.uiFontScaleMax),
    uiAccentColor: rgb(j['ui_accent_color']),
    uiBackgroundColor: rgb(j['ui_background_color']),
    terminalColorScheme: _byWire(
      TerminalColorScheme.values,
      (e) => e.name,
      j['terminal_color_scheme'],
      d.terminalColorScheme,
    ),
    customTerminalColors: TerminalColors.tryFromJson(j['custom_terminal_colors']),
    reopenLastProfile: j['reopen_last_profile'] == true,
    checkUpdatesAutomatically: j['check_updates_automatically'] != false,
    sftpDefaultEditor: _appRefFromJson(j['sftp_default_editor']),
    terminalFontSize: (j['terminal_font_size'] as num?)?.toDouble() ?? d.terminalFontSize,
    terminalScrollback: _int(j['terminal_scrollback']) ?? d.terminalScrollback,
    clipboardClearSeconds: _int(j['clipboard_clear_seconds']) ?? d.clipboardClearSeconds,
    autoLockMinutes: _int(j['auto_lock_minutes']) ?? d.autoLockMinutes,
    appLocale: AppLocale.fromWire(_str(j['app_locale'])),
    glassMode: GlassMode.fromWire(_str(j['glass_mode'])),
    sidebarStyle: SidebarStyle.fromWire(_str(j['sidebar_style'])),
    workspacePanelStyle: _byWire(
      WorkspacePanelStyle.values,
      (style) => style.name,
      j['workspace_panel_style'],
      d.workspacePanelStyle,
    ),
  );
}

Json backupScheduleToJson(BackupSchedule s) => {
  'enabled': s.enabled,
  'folder': s.folder,
  'frequency': s.frequency.name,
  'keep_last': s.keepLast,
  'last_run_at_ms': s.lastRunAt == null ? null : toMs(s.lastRunAt!),
  'next_run_at_ms': s.nextRunAt == null ? null : toMs(s.nextRunAt!),
  'last_error': s.lastError,
};

BackupSchedule backupScheduleFromJson(Json j) => BackupSchedule(
  enabled: _bool(j['enabled']),
  folder: _str(j['folder']),
  frequency: _byWire(BackupFrequency.values, (e) => e.name, j['frequency'], BackupFrequency.daily),
  keepLast: _int(j['keep_last']) ?? 7,
  lastRunAt: fromMsOrNull(j['last_run_at_ms']),
  nextRunAt: fromMsOrNull(j['next_run_at_ms']),
  lastError: _str(j['last_error']),
);

Json backupInfoToJson(BackupInfo b) => {
  'path': b.path,
  'vault_id': b.vaultId.value,
  'created_at_ms': toMs(b.createdAt),
  'objects': b.objectCount,
  'size_bytes': b.sizeBytes,
  'format_version': b.formatVersion,
  'app_version': b.appVersion,
  'automatic': b.automatic,
};

BackupInfo backupInfoFromJson(Json j) => BackupInfo(
  path: j['path']! as String,
  vaultId: VaultId(j['vault_id']! as String),
  createdAt: fromMs(j['created_at_ms']),
  objectCount: _int(j['objects']) ?? 0,
  sizeBytes: _int(j['size_bytes']) ?? 0,
  formatVersion: _int(j['format_version']) ?? 1,
  appVersion: _str(j['app_version']) ?? '',
  automatic: _bool(j['automatic']),
);

/// `ccbackup-v1` → 1.
int backupFormatVersion(String format) => int.tryParse(RegExp(r'(\d+)$').firstMatch(format)?.group(1) ?? '') ?? 1;

// ---- SFTP browser & edit sessions (SFTP_BROWSER_SPEC, ADR-0108) ------------------------

/// `RemoteFileInfoDto` → [RemoteFileInfo].
RemoteFileInfo remoteFileInfoFromJson(Json j) => RemoteFileInfo(
  name: j['name']! as String,
  path: j['path']! as String,
  kind: RemoteEntryKind.fromWire(j['kind'] as String? ?? 'other'),
  size: _int(j['size']) ?? 0,
  permissions: _int(j['permissions']),
  uid: _int(j['uid']),
  gid: _int(j['gid']),
  owner: _emptyToNull(_str(j['owner'])),
  group: _emptyToNull(_str(j['group'])),
  modifiedAt: fromMsOrNull(j['modified_at_ms'])?.toLocal(),
  linkTarget: _str(j['link_target']),
  linkTargetKind: j['link_target_kind'] == null ? null : RemoteEntryKind.fromWire(j['link_target_kind']! as String),
);

AppRef? _appRefFromJson(Object? v) {
  if (v is! Map || v['value'] is! String) return null;
  final value = v['value'] as String;
  if (value.trim().isEmpty || value.contains('\u0000')) return null;
  final kind = switch (v['kind']) {
    'name' => AppRefKind.name,
    'bundle_id' => AppRefKind.bundleId,
    'path' => AppRefKind.path,
    _ => null,
  };
  return kind == null ? null : AppRef(kind, value);
}

/// [OpenWith] → `OpenWithDto` JSON.
String openWithToJson(OpenWith w) => encodeJson(switch (w) {
  OpenWithDefault() => {'kind': 'default'},
  OpenWithChoose() => {'kind': 'choose'},
  OpenWithApp(:final app) => {
    'kind': 'app',
    'app': {'kind': app.kind.wireName, 'value': app.value},
  },
});

RemoteFileMeta? _remoteMetaFromJson(Object? v) => v is Map
    ? RemoteFileMeta(
        size: _int(v['size']) ?? 0,
        modifiedAt: fromMsOrNull(v['modified_at_ms'])?.toLocal(),
        permissions: _int(v['permissions']),
      )
    : null;

/// `EditStatusDto` (`state` tag) → [EditStatus].
EditStatus editStatusFromJson(Json j) => switch (j['state']) {
  'opening' => const EditStatusOpening(),
  'modified' => const EditStatusModified(),
  'uploading' => EditStatusUploading(transferred: _int(j['transferred']) ?? 0, total: _int(j['total'])),
  'conflict' => EditStatusConflict(remote: _remoteMetaFromJson(j['remote'])),
  'error' => EditStatusError(message: _str(j['message']) ?? '', retryable: j['retryable'] != false),
  'closed' => const EditStatusClosed(),
  _ => const EditStatusSynced(),
};

/// `EditSessionDto` → [EditSessionInfo].
EditSessionInfo editSessionFromJson(Json j) => EditSessionInfo(
  id: EditSessionId(j['id']! as String),
  hostId: ObjectId(j['host_id']! as String),
  sftpSession: _emptyToNull(_str(j['sftp_id'])) == null ? null : SftpSessionId(j['sftp_id']! as String),
  remotePath: j['remote_path']! as String,
  targetPath: _str(j['target_path']),
  localPath: j['local_path']! as String,
  status: editStatusFromJson((j['status']! as Map).cast<String, Object?>()),
  openedAt: fromMs(j['opened_at_ms']).toLocal(),
  lastSyncedAt: fromMsOrNull(j['last_synced_at_ms'])?.toLocal(),
  uploads: _int(j['uploads']) ?? 0,
  remoteCopies: _strings(j['remote_copies']),
  app: _appRefFromJson(j['app']),
);

/// `EditStopOutcomeDto` (`outcome` tag) → [EditStopOutcome].
EditStopOutcome editStopOutcomeFromJson(Json j) => switch (j['outcome']) {
  'kept_files' => EditStopKeptFiles(_str(j['directory']) ?? ''),
  'conflict' => EditStopConflict(remote: _remoteMetaFromJson(j['remote'])),
  'upload_failed' => EditStopUploadFailed(_str(j['message']) ?? ''),
  _ => EditStopClosed(uploaded: _bool(j['uploaded'])),
};

/// `EditLeftoverDto` → [EditLeftover].
EditLeftover editLeftoverFromJson(Json j) => EditLeftover(
  id: EditSessionId(j['id']! as String),
  hostId: _id(j['host_id']),
  remotePath: _str(j['remote_path']),
  targetPath: _str(j['target_path']),
  createdAt: fromMsOrNull(j['created_at_ms'])?.toLocal(),
  workingFile: _str(j['working_file']),
  locallyModified: j['locally_modified'] as bool?,
);

// ---- prompts outside terminal tabs -------------------------------------------------------

/// `PromptRequest` JSON (`{"HostKey":{…}}` / `{"Password":{…}}` /
/// `{"Passphrase":{…}}`) → [CorePrompt]; `null` for unknown shapes.
CorePrompt? corePromptFromJson(Json j) {
  if (j['HostKey'] case final Map<Object?, Object?> b) {
    return HostKeyCorePrompt(
      requestId: b['request_id']! as String,
      host: b['host'] as String? ?? '',
      port: _int(b['port']) ?? 22,
      hostPattern: b['host_pattern'] as String? ?? '',
      hostId: _id(b['host_id']),
      hostName: _str(b['host_name']),
      hopIndex: _int(b['hop_index']) ?? 0,
      hopCount: _int(b['hop_count']) ?? 1,
      keyType: b['key_type'] as String? ?? '',
      fingerprintSha256: b['fingerprint_sha256'] as String? ?? '',
      otherKnownKeyTypes: _strings(b['other_known_key_types']),
    );
  }
  if (j['Password'] case final Map<Object?, Object?> b) {
    return PasswordCorePrompt(
      requestId: b['request_id']! as String,
      hostId: _id(b['host_id']),
      hostName: b['host_name'] as String? ?? '',
    );
  }
  if (j['Passphrase'] case final Map<Object?, Object?> b) {
    return PassphraseCorePrompt(
      requestId: b['request_id']! as String,
      credentialId: ObjectId(b['credential_id'] as String? ?? ''),
      credentialName: b['credential_name'] as String? ?? '',
      attempt: _int(b['attempt']) ?? 0,
    );
  }
  return null;
}

// ---- connection planner preview -----------------------------------------------------------

/// Legacy English planner texts the host editor still matches (until it
/// switches to [PlanDiagnostic.code]).
const planDiagnosticLegacyText = {
  'jump_host_deleted': 'A jump host in the chain was deleted', // l10n-ignore: planner diagnostic
  'self_jump': 'The host cannot jump through itself', // l10n-ignore: planner diagnostic
  'no_credential': 'No credential: you will be asked for a password when connecting', // l10n-ignore: planner diagnostic
  'credential_missing': 'The selected credential no longer exists', // l10n-ignore: planner diagnostic
  'no_username': 'No username: set one on the host or on a group', // l10n-ignore: planner diagnostic
};

/// Credential label of password-prompt hosts (legacy text, see above).
const planCredentialPromptText = 'Password — asked when connecting'; // l10n-ignore: planner diagnostic

ValueSource _valueSource(Object? v) => switch (v) {
  'host' || 'credential' => ValueSource.host,
  'group' => ValueSource.group,
  'jump_profile' => ValueSource.jumpProfile,
  'app_default' => ValueSource.appDefault,
  _ => ValueSource.unset,
};

Resolved<T> _resolved<T>(Object? v, T? Function(Object?) value) {
  if (v is! Map) return const Resolved.unset();
  return Resolved(value(v['value']), _valueSource(v['source']), sourceName: v['source_name'] as String?);
}

/// `PlanPreviewDto` → [EffectiveHostConfig] (values + provenance from the
/// core's planner; `diagnostics` carry stable codes).
EffectiveHostConfig effectiveHostFromPlanJson(Json j) {
  final diagnostics = [
    for (final d in (j['diagnostics'] as List? ?? const []).cast<Map<Object?, Object?>>())
      PlanDiagnostic(
        code: d['code'] as String? ?? '',
        severity: PlanDiagnosticSeverity.fromWire(d['severity'] as String?),
        args: _stringMap(d['args']),
        message: d['message'] as String? ?? '',
      ),
  ];
  final prompts = _bool(j['prompts_for_password']);
  return EffectiveHostConfig(
    port: _resolved<int>(j['port'], (v) => (v as num?)?.toInt()),
    username: _resolved<String>(j['username'], (v) => v as String?),
    credentialId: _resolved<ObjectId>(j['credential_id'], _id),
    credentialName: prompts ? planCredentialPromptText : _str(j['credential_name']),
    route: [
      for (final h in (j['route'] as List? ?? const []).cast<Map<Object?, Object?>>())
        RouteHop(hostId: ObjectId(h['host_id']! as String), label: h['label'] as String? ?? ''),
    ],
    routeSource: _resolved<String>(j['route_source'], (v) => v as String?),
    groupPath: _strings(j['group_path']),
    problems: [
      for (final d in diagnostics)
        if (d.severity != PlanDiagnosticSeverity.info) planDiagnosticLegacyText[d.code] ?? d.message,
    ],
    diagnostics: diagnostics,
  );
}
