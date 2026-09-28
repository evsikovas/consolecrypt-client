import 'dart:async';
import 'dart:typed_data';

import 'package:consolecrypt/core/bridge/effective_config.dart';
import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/bridge/rust_account.dart';
import 'package:consolecrypt/core/bridge/rust_backend.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/src/rust/api/account.dart' as rs_account;
import 'package:consolecrypt/src/rust/api/ai.dart' as rs_ai;
import 'package:consolecrypt/src/rust/api/credentials.dart' as rs_cred;
import 'package:consolecrypt/src/rust/api/inventory.dart' as rs_inv;
import 'package:consolecrypt/src/rust/api/ssh.dart' as rs_ssh;

// ---- inventory -------------------------------------------------------------------------

final class RustInventoryService implements InventoryService {
  RustInventoryService(this._hub);

  final RustBackend _hub;

  EffectiveConfigResolver get _resolver => EffectiveConfigResolver(
    hosts: _hub.hosts.value,
    groups: _hub.groups.value,
    credentials: _hub.credentials.value,
    jumpProfiles: _hub.jumpProfiles.value,
  );

  Host _storedHost(String json) => hostFromJson(decodeObject(json));

  // Hosts ------------------------------------------------------------------------------

  @override
  Stream<List<Host>> watchHosts() => _hub.hosts.stream;

  @override
  Future<Host> saveHost(Host host) async {
    final error = host.validate();
    if (error != null) throw AppException.fromValidation(error);
    final json = await guard(
      () => rs_inv.hostsSave(hostJson: encodeJson(hostToJson(host, base: _hub.rawHosts[host.id.value]))),
    );
    await _hub.reload({'host'});
    return _storedHost(json);
  }

  @override
  Future<Host> saveHostWithAuth(Host host, HostAuth auth) async {
    final error = host.validate();
    if (error != null) throw AppException.fromValidation(error);
    final hostJson = encodeJson(hostToJson(host, base: _hub.rawHosts[host.id.value]));
    final String json;
    switch (auth) {
      case HostAuthInherit():
        json = await guard(
          () => rs_inv.hostsSaveWithAuth(hostJson: hostJson, auth: _auth(rs_inv.HostAuthKind.inherit)),
        );
      case HostAuthCredential(:final credentialId):
        json = await guard(
          () => rs_inv.hostsSaveWithAuth(
            hostJson: hostJson,
            auth: _auth(rs_inv.HostAuthKind.credential, credentialId: credentialId.value),
          ),
        );
      case HostAuthInlinePassword(:final password):
        if (password == null) {
          json = await guard(
            () => rs_inv.hostsSaveWithAuth(hostJson: hostJson, auth: _auth(rs_inv.HostAuthKind.inlinePassword)),
          );
        } else {
          json = await withSecret(
            password,
            (bytes) => guard(
              () => rs_inv.hostsSaveWithAuth(
                hostJson: hostJson,
                auth: _auth(rs_inv.HostAuthKind.inlinePassword, password: bytes),
              ),
            ),
          );
        }
      case HostAuthPasswordPrompt():
        json = await guard(
          () => rs_inv.hostsSaveWithAuth(hostJson: hostJson, auth: _auth(rs_inv.HostAuthKind.passwordPrompt)),
        );
      case HostAuthAgent(:final kind, :final agentPath):
        final external = kind == CredentialKind.externalAgent;
        if (external && (agentPath ?? '').trim().isEmpty) {
          throw const AppException(
            AppErrorCode.validation,
            'Enter the agent socket or pipe', // l10n-ignore: diagnostic
            reason: AppErrorReason.agentPathRequired,
          );
        }
        json = await guard(
          () => rs_inv.hostsSaveWithAuth(
            hostJson: hostJson,
            auth: _auth(
              external ? rs_inv.HostAuthKind.externalAgent : rs_inv.HostAuthKind.osAgent,
              agentPath: agentPath?.trim(),
            ),
          ),
        );
    }
    await _hub.reload({'host', 'credential'});
    return _storedHost(json);
  }

  static rs_inv.HostAuthInput _auth(
    rs_inv.HostAuthKind kind, {
    String? credentialId,
    List<int>? password,
    String? agentPath,
  }) => rs_inv.HostAuthInput(
    kind: kind,
    credentialId: credentialId,
    password: password == null ? null : _bytes(password),
    agentPath: agentPath,
  );

  @override
  Future<void> deleteHost(ObjectId id) async {
    await guard(() => rs_inv.hostsDelete(id: id.value));
    await _hub.reload({'host', 'credential', 'tunnel'});
  }

  // Groups ----------------------------------------------------------------------------

  @override
  Stream<List<Group>> watchGroups() => _hub.groups.stream;

  @override
  Future<Group> saveGroup(Group group) async {
    if (group.name.trim().isEmpty) {
      throw const AppException(AppErrorCode.validation, 'Enter a name', reason: AppErrorReason.nameRequired);
    }
    final json = await guard(
      () => rs_inv.groupsSave(groupJson: encodeJson(groupToJson(group, base: _hub.rawGroups[group.id.value]))),
    );
    await _hub.reload({'group'});
    return groupFromJson(decodeObject(json));
  }

  @override
  Future<void> deleteGroup(ObjectId id) async {
    await guard(() => rs_inv.groupsDelete(id: id.value));
    await _hub.reload({'group', 'host'});
  }

  // Jump profiles ------------------------------------------------------------------------

  @override
  Stream<List<JumpProfile>> watchJumpProfiles() => _hub.jumpProfiles.stream;

  @override
  Future<JumpProfile> saveJumpProfile(JumpProfile profile) async {
    if (profile.chain.isEmpty) {
      throw const AppException(
        AppErrorCode.validation,
        'A jump profile needs at least one host', // l10n-ignore: diagnostic
        reason: AppErrorReason.jumpChainEmpty,
      );
    }
    final json = await guard(
      () => rs_inv.jumpProfilesSave(
        profileJson: encodeJson(jumpProfileToJson(profile, base: _hub.rawJumpProfiles[profile.id.value])),
      ),
    );
    await _hub.reload({'jump_profile'});
    return jumpProfileFromJson(decodeObject(json));
  }

  @override
  Future<void> deleteJumpProfile(ObjectId id) async {
    await guard(() => rs_inv.jumpProfilesDelete(id: id.value));
    await _hub.reload({'jump_profile'});
  }

  // Credentials ----------------------------------------------------------------------------

  @override
  Stream<List<Credential>> watchCredentials() => _hub.credentials.stream;

  Future<Credential> _storedCredential(String json) async {
    await _hub.reload({'credential'});
    return credentialFromJson(decodeObject(json));
  }

  static String? _clean(String? s) => (s == null || s.trim().isEmpty) ? null : s.trim();

  @override
  Future<Credential> createPasswordCredential({
    required String name,
    required SecretText password,
    String? username,
  }) async {
    final json = await withSecret(
      password,
      (bytes) => guard(() => rs_cred.credentialsAddPassword(name: name, username: _clean(username), password: bytes)),
    );
    return _storedCredential(json);
  }

  @override
  Future<Credential> generateKeyCredential({
    required String name,
    required KeyAlgorithm algorithm,
    String? comment,
    SecretText? passphrase,
    bool rememberPassphrase = false,
  }) async {
    final choice = switch (algorithm) {
      KeyAlgorithm.ed25519 => rs_cred.KeyGenChoice.ed25519,
      KeyAlgorithm.rsa3072 => rs_cred.KeyGenChoice.rsa3072,
      KeyAlgorithm.rsa4096 => rs_cred.KeyGenChoice.rsa4096,
      _ => throw const AppException(
        AppErrorCode.validation,
        'Unsupported key algorithm', // l10n-ignore: diagnostic
        reason: AppErrorReason.unsupportedKeyAlgorithm,
      ),
    };
    Future<String> call(List<int>? p) => guard(
      () => rs_cred.credentialsGenerateKey(
        name: name,
        algorithm: choice,
        passphrase: p == null ? null : _bytes(p),
        rememberPassphrase: rememberPassphrase,
      ),
    );
    final json = passphrase == null || passphrase.isEmpty ? await call(null) : await withSecret(passphrase, call);
    return _storedCredential(json);
  }

  @override
  Future<KeyInspection> inspectPrivateKey(SecretText privateKey) async {
    final r = await withSecret(privateKey, (bytes) => guard(() => rs_cred.credentialsInspectKey(privateKey: bytes)));
    return KeyInspection(
      valid: r.valid,
      algorithm: r.algorithm == null ? null : KeyAlgorithm.values.where((a) => a.wireName == r.algorithm).firstOrNull,
      encrypted: r.encrypted,
      fingerprint: r.fingerprint,
      publicKey: r.publicKey,
      error: r.error,
    );
  }

  @override
  Future<Credential> importKeyCredential({
    required String name,
    required SecretText privateKey,
    String? username,
    SecretText? passphrase,
    bool rememberPassphrase = false,
    String? certificate,
  }) async {
    Future<String> call(List<int> key, List<int>? p) => guard(
      () => rs_cred.credentialsImportKey(
        name: name,
        username: _clean(username),
        privateKey: key,
        passphrase: p == null ? null : _bytes(p),
        rememberPassphrase: rememberPassphrase,
        certificate: _clean(certificate),
      ),
    );
    final json = passphrase == null || passphrase.isEmpty
        ? await withSecret(privateKey, (k) => call(k, null))
        : await withSecrets(privateKey, passphrase, call);
    return _storedCredential(json);
  }

  @override
  Future<Credential> createAgentCredential({
    required String name,
    required CredentialKind kind,
    String? username,
    String? agentPath,
  }) async {
    final String? path;
    switch (kind) {
      case CredentialKind.osSshAgent:
        path = null;
      case CredentialKind.externalAgent:
        path = _clean(agentPath);
        if (path == null) {
          throw const AppException(
            AppErrorCode.validation,
            'Enter the agent socket or pipe', // l10n-ignore: diagnostic
            reason: AppErrorReason.agentPathRequired,
          );
        }
      default:
        throw const AppException(
          AppErrorCode.validation,
          'Not an agent credential', // l10n-ignore: diagnostic
          reason: AppErrorReason.notAgentCredential,
        );
    }
    final json = await guard(
      () => rs_cred.credentialsAddAgent(name: name, username: _clean(username), agentPath: path),
    );
    return _storedCredential(json);
  }

  @override
  Future<Credential> rememberKeyPassphrase(ObjectId credentialId, SecretText passphrase) async {
    final json = await withSecret(
      passphrase,
      (p) => guard(() => rs_cred.credentialsRememberPassphrase(id: credentialId.value, passphrase: p)),
    );
    return _storedCredential(json);
  }

  @override
  Future<Credential> forgetKeyPassphrase(ObjectId credentialId) async =>
      _storedCredential(await guard(() => rs_cred.credentialsForgetPassphrase(id: credentialId.value)));

  @override
  Future<Credential> updateCredential(Credential credential) async {
    if (credential.name.trim().isEmpty) {
      throw const AppException(AppErrorCode.validation, 'Enter a name', reason: AppErrorReason.nameRequired);
    }
    final json = await guard(
      () => rs_cred.credentialsUpdate(
        id: credential.id.value,
        name: credential.name,
        username: _clean(credential.username),
        // `null` keeps the stored certificate.
      ),
    );
    return _storedCredential(json);
  }

  @override
  Future<void> deleteCredential(ObjectId id) async {
    await guard(() => rs_cred.credentialsDelete(id: id.value));
    await _hub.reload({'credential', 'host', 'group'});
  }

  @override
  Future<SecretText> revealCredentialSecret(ObjectId credentialId) async {
    // Explicit user action only: the core decrypts the Secret on demand and
    // logs an audit line (never the value). The transfer buffer is wiped.
    final bytes = await guard(() => rs_account.credentialsRevealSecret(credentialId: credentialId.value));
    try {
      return SecretText.fromBytes(bytes);
    } finally {
      bytes.fillRange(0, bytes.length, 0);
    }
  }

  // Planning previews ----------------------------------------------------------------------

  /// app-core's planner with provenance (`hosts_plan_preview`); the Dart
  /// resolver is the fallback while locked / for malformed drafts.
  @override
  Future<EffectiveHostConfig> resolveEffective(Host host) async {
    try {
      final json = await guard(
        () => rs_inv.hostsPlanPreview(hostJson: encodeJson(hostToJson(host, base: _hub.rawHosts[host.id.value]))),
      );
      return effectiveHostFromPlanJson(decodeObject(json));
    } on AppException {
      return _resolver.resolve(host);
    }
  }

  @override
  Future<EffectiveGroupDefaults> resolveGroupDefaults(ObjectId groupId) async => _resolver.resolveGroup(groupId);

  // Known hosts ------------------------------------------------------------------------------

  @override
  Stream<List<KnownHost>> watchKnownHosts() => _hub.knownHosts.stream;

  @override
  Future<void> deleteKnownHost(ObjectId id) async {
    await guard(() => rs_inv.knownHostsRemove(id: id.value));
    await _hub.reload({'known_host'});
  }
}

Uint8List _bytes(List<int> v) => v is Uint8List ? v : Uint8List.fromList(v);

// ---- snippets --------------------------------------------------------------------------

final class RustSnippetService implements SnippetService {
  RustSnippetService(this._hub);

  final RustBackend _hub;

  @override
  Stream<List<Snippet>> watchSnippets() => _hub.snippets.stream;

  @override
  Future<Snippet> saveSnippet(Snippet snippet) async {
    if (snippet.name.trim().isEmpty) {
      throw const AppException(AppErrorCode.validation, 'Enter a name', reason: AppErrorReason.nameRequired);
    }
    if (snippet.template.trim().isEmpty) {
      throw const AppException(
        AppErrorCode.validation,
        'Enter the command template', // l10n-ignore: diagnostic
        reason: AppErrorReason.templateRequired,
      );
    }
    final json = await guard(
      () => rs_inv.snippetsSave(
        snippetJson: encodeJson(snippetToJson(snippet, base: _hub.rawSnippets[snippet.id.value])),
      ),
    );
    await _hub.reload({'snippet'});
    return snippetFromJson(decodeObject(json));
  }

  @override
  Future<void> deleteSnippet(ObjectId id) async {
    await guard(() => rs_inv.snippetsDelete(id: id.value));
    await _hub.reload({'snippet'});
  }

  @override
  Future<List<SnippetSearchHit>> search(String query, {int limit = 20}) async {
    // Local FTS in the core (semantic only with an embedding provider,
    // never an LLM); an empty query lists the most used snippets.
    final hits = decodeList(await guard(() => rs_inv.snippetsSearch(query: query, limit: limit.clamp(1, 100))));
    final byId = {for (final s in _hub.snippets.value) s.id.value: s};
    return [
      for (final h in hits)
        if (byId[h['snippet_id']] case final snippet?)
          SnippetSearchHit(
            snippet: snippet,
            score: (h['score'] as num?)?.toDouble() ?? 0,
            matchKind: h['exact'] == true
                ? SearchMatchKind.exact
                : switch (h['origin']) {
                    'semantic' || 'hybrid' => SearchMatchKind.semantic,
                    _ => SearchMatchKind.text,
                  },
          ),
    ];
  }

  @override
  Future<String> render(Snippet snippet, Map<String, String> values) async {
    // The core's quoting policy (ai-core) for saved and unsaved snippets.
    final r = decodeObject(
      await guard(
        () => rs_inv.snippetsRender(
          snippetJson: encodeJson(snippetToJson(snippet, base: _hub.rawSnippets[snippet.id.value])),
          valuesJson: encodeJson(values),
        ),
      ),
    );
    final command = r['command'] as String?;
    if (command != null) return command;
    final names = [
      for (final e in (r['field_errors'] as List? ?? const []).cast<Map<Object?, Object?>>())
        if (e['name'] case final String name) name,
    ];
    throw AppException(
      AppErrorCode.validation,
      'Missing or invalid variables', // l10n-ignore: diagnostic (UI renders code/reason)
      reason: AppErrorReason.missingVariables,
      args: {'names': names.join(', ')},
    );
  }

  @override
  Future<RiskAssessment> assessRisk(
    String command, {
    RiskLevel declared = RiskLevel.unknown,
    SnippetSource source = SnippetSource.user,
  }) async {
    final j = decodeObject(await guard(() => rs_ai.aiAssessRisk(command: command)));
    RiskLevel level(Object? wire) => RiskLevel.values.where((r) => r.wireName == wire).firstOrNull ?? RiskLevel.unknown;
    final local = level(j['level']);
    return RiskAssessment(
      effective: combineRisk(declared: declared, local: local, source: source),
      local: local,
      declared: declared,
      reasons: [
        for (final r in (j['reasons'] as List? ?? const []).cast<Map<Object?, Object?>>())
          (r['detail'] as String?)?.isNotEmpty == true ? r['detail']! as String : r['rule'] as String? ?? '',
      ],
    );
  }

  @override
  Future<void> recordUsage(ObjectId id) async {
    final raw = _hub.rawSnippets[id.value];
    if (raw == null) return;
    final updated = {
      ...raw,
      'usage_count': ((raw['usage_count'] as num?)?.toInt() ?? 0) + 1,
      'last_used_at_ms': toMs(DateTime.now()),
    };
    await guard(() => rs_inv.snippetsSave(snippetJson: encodeJson(updated)));
    await _hub.reload({'snippet'});
  }
}

// ---- tunnels ---------------------------------------------------------------------------

final class RustTunnelService implements TunnelService {
  RustTunnelService(this._hub);

  final RustBackend _hub;

  @override
  Stream<List<Tunnel>> watchTunnels() => _hub.tunnels.stream;

  @override
  Future<Tunnel> saveTunnel(Tunnel tunnel) async {
    final error = tunnel.validate();
    if (error != null) throw AppException.fromValidation(error);
    final json = await guard(
      () => rs_inv.tunnelsSave(tunnelJson: encodeJson(tunnelToJson(tunnel, base: _hub.rawTunnels[tunnel.id.value]))),
    );
    await _hub.reload({'tunnel'});
    return tunnelFromJson(decodeObject(json));
  }

  @override
  Future<void> deleteTunnel(ObjectId id) async {
    await guard(() => rs_inv.tunnelsDelete(id: id.value));
    await _hub.reload({'tunnel'});
  }

  @override
  Future<void> start(ObjectId id) async {
    final current = Map.of(_hub.tunnelRuntime.value);
    current[id] = TunnelRuntime(state: TunnelRunState.starting, since: DateTime.now());
    _hub.tunnelRuntime.value = Map.unmodifiable(current);
    try {
      await guard(() => rs_ssh.tunnelStart(tunnelId: id.value));
    } finally {
      await _hub.refreshTunnelRuntime();
    }
  }

  @override
  Future<void> stop(ObjectId id) async {
    await guard(() => rs_ssh.tunnelStop(tunnelId: id.value));
    await _hub.refreshTunnelRuntime();
  }

  @override
  Stream<Map<ObjectId, TunnelRuntime>> watchRuntime() => _hub.tunnelRuntime.stream;
}

// ---- settings --------------------------------------------------------------------------

final class RustSettingsService implements SettingsService {
  RustSettingsService(this._hub);

  final RustBackend _hub;
  Future<void> _pendingLocalWrite = Future<void>.value();

  /// Drain accepted edits before the core is shut down on app exit.
  Future<void> flushLocalWrites() => _pendingLocalWrite;

  @override
  Stream<LocalSettings> watchLocal() => _hub.localSettings.stream;

  @override
  LocalSettings get currentLocal => _hub.localSettings.value;

  /// Device-local (`<data dir>/ui-state.json`), never synced.
  @override
  Future<void> updateLocal(LocalSettings settings) async {
    _hub.localSettings.value = settings;
    final json = encodeJson(localSettingsToJson(settings));
    final write = _pendingLocalWrite.then((_) => _hub.storeSet(RustBackend.localSettingsKey, json));
    // Slider and colour changes may arrive before the previous disk write.
    // Preserve request order, and allow a later retry after a failed write.
    _pendingLocalWrite = write.then<void>((_) {}, onError: (Object _, StackTrace _) {});
    await write;
  }

  @override
  Stream<VaultSettings?> watchVault() => _hub.vaultSettings.stream;

  @override
  Future<void> updateVault(VaultSettings settings) async {
    await guard(
      () => rs_inv.vaultSettingsSave(
        settingsJson: encodeJson(vaultSettingsToJson(settings, base: _hub.rawVaultSettings)),
      ),
    );
    await _hub.reload({'vault_settings'});
  }
}

// ---- AI --------------------------------------------------------------------------------

/// AI over app-core's AI runtime (providers, Ask, command generation,
/// snippet drafts). Provider configs and API keys live in the vault; keys
/// are write-only.
final class RustAiService implements AiService {
  RustAiService(this._hub);

  final RustBackend _hub;

  /// Conversation per provider for multi-turn chats (app-core keeps the
  /// history; the UI sends the whole transcript).
  final Map<String, String> _conversations = {};

  @override
  Stream<List<AiProviderConfig>> watchProviders() => _hub.aiProviders.stream;

  @override
  Future<AiProviderConfig> saveProvider(AiProviderConfig config, {SecretText? apiKey, bool clearApiKey = false}) async {
    final providerJson = encodeJson(aiProviderToJson(config, base: _hub.rawAiProviders[config.id.value]));
    final String json;
    if (apiKey != null && apiKey.isNotEmpty) {
      json = await withSecret(
        apiKey,
        (k) => guard(() => rs_inv.aiProvidersSave(providerJson: providerJson, apiKey: k, clearApiKey: false)),
      );
    } else {
      apiKey?.wipe();
      json = await guard(() => rs_inv.aiProvidersSave(providerJson: providerJson, clearApiKey: clearApiKey));
    }
    await _hub.reload({'ai_provider'});
    return aiProviderFromJson(decodeObject(json));
  }

  @override
  Future<void> deleteProvider(ObjectId id) async {
    await guard(() => rs_inv.aiProvidersDelete(id: id.value));
    _conversations.remove(id.value);
    await _hub.reload({'ai_provider'});
  }

  @override
  Future<ProviderHealth> testProvider(ObjectId id) async {
    try {
      final j = decodeObject(await guard(() => rs_ai.aiTestProvider(providerId: id.value)));
      final models = [for (final m in j['models'] as List? ?? const []) m as String];
      return ProviderHealth(ok: true, message: '${j['latency_ms'] ?? 0} ms', models: models);
    } on AppException catch (e) {
      return ProviderHealth(ok: false, message: e.message);
    }
  }

  static String? _contextJson(AiContextSelection c) {
    if (c.hostId == null && c.selectedTerminalText == null) return null;
    return encodeJson({
      'host_id': c.hostId?.value,
      'terminal_id': null,
      'selected_text': c.selectedTerminalText,
      'include_terminal': c.selectedTerminalText != null,
    });
  }

  SanitizationReport _report(Json summary, String? providerId) {
    final redactions = (summary['redactions'] as Map?)?['total'];
    final provider = providerId == null
        ? null
        : _hub.aiProviders.value.where((p) => p.id.value == providerId).firstOrNull;
    return SanitizationReport(
      profile: provider?.privacyProfile ?? PrivacyProfile.strict,
      redactions: (redactions as num?)?.toInt() ?? 0,
      remote: !(provider?.provider.isLocalByDefault ?? false),
    );
  }

  @override
  Stream<AiStreamEvent> chat({
    required ObjectId providerId,
    required List<ChatMessage> history,
    AiContextSelection context = AiContextSelection.none,
  }) async* {
    final question = history.where((m) => m.role == ChatRole.user).lastOrNull?.content ?? '';
    final userTurns = history.where((m) => m.role == ChatRole.user).length;
    final conversation = userTurns > 1 ? _conversations[providerId.value] : null;
    final text = StringBuffer();
    try {
      await for (final json in rs_ai.aiAsk(
        providerId: providerId.value,
        conversationId: conversation,
        question: question,
        contextJson: _contextJson(context),
      )) {
        final chunk = decodeObject(json);
        switch (chunk['type']) {
          case 'started':
            _conversations[providerId.value] = chunk['conversation_id']! as String;
          case 'delta':
            final t = chunk['text'] as String? ?? '';
            text.write(t);
            yield AiDelta(t);
          case 'done':
            yield AiCompleted(fullText: text.toString(), report: _report(chunk, providerId.value));
            return;
          case 'error':
            yield AiFailed(chunk['message'] as String? ?? '');
            return;
        }
      }
    } catch (e) {
      yield AiFailed(toAppException(e).message);
    }
  }

  @override
  Stream<AiStreamEvent> generateCommand({
    required ObjectId providerId,
    required String request,
    SnippetType? dialect,
    AiContextSelection context = AiContextSelection.none,
  }) async* {
    try {
      final j = decodeObject(
        await guard(
          () => rs_ai.aiGenerateCommand(
            providerId: providerId.value,
            request: request,
            snippetType: dialect?.wireName,
            contextJson: _contextJson(context),
          ),
        ),
      );
      final run = (j['run'] as Map?)?.cast<String, Object?>() ?? const {};
      final risk = RiskLevel.values.where((r) => r.wireName == run['risk']).firstOrNull ?? RiskLevel.unknown;
      final explanation = j['explanation'] as String? ?? '';
      final command = j['command'] as String? ?? '';
      if (explanation.isNotEmpty) yield AiDelta(explanation);
      yield AiCompleted(
        fullText: explanation,
        report: SanitizationReport(
          profile:
              PrivacyProfile.values.where((p) => p.wireName == j['privacy_profile']).firstOrNull ??
              PrivacyProfile.strict,
          redactions: ((j['redactions'] as Map?)?['total'] as num?)?.toInt() ?? 0,
          remote: true,
        ),
        command: GeneratedCommand(command: command, explanation: explanation, suggestedRisk: risk, dialect: dialect),
      );
    } on AppException catch (e) {
      yield AiFailed(e.message);
    }
  }

  @override
  Future<SnippetDraft> draftSnippet({required String command, ObjectId? providerId}) async {
    final j = decodeObject(
      await guard(
        () => rs_ai.aiConvertToSnippet(providerId: providerId?.value, command: command, useLlm: providerId != null),
      ),
    );
    final snippet = snippetFromJson({
      'id': '',
      'created_at_ms': 0,
      'updated_at_ms': 0,
      ...(j['snippet']! as Map).cast<String, Object?>(),
    });
    final risk = RiskLevel.values.where((r) => r.wireName == j['local_risk']).firstOrNull ?? snippet.riskLevel;
    return SnippetDraft(
      name: snippet.name,
      description: snippet.description,
      template: snippet.template,
      snippetType: snippet.snippetType,
      variables: snippet.variables,
      tags: snippet.tags,
      suggestedRisk: risk,
    );
  }
}
