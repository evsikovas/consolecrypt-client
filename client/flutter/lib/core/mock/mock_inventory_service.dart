import 'dart:convert';

import 'package:consolecrypt/core/mock/fake_data.dart';
import 'package:consolecrypt/core/mock/mock_cloud.dart';
import 'package:consolecrypt/core/mock/mock_config.dart';
import 'package:consolecrypt/core/mock/mock_scope.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/core/util/value_stream.dart';

final class MockInventoryService extends VaultScopedMock implements InventoryService {
  MockInventoryService(super.cloud) {
    initScope();
  }

  final ValueStreamController<List<Host>> _hosts = ValueStreamController(const []);
  final ValueStreamController<List<Group>> _groups = ValueStreamController(const []);
  final ValueStreamController<List<JumpProfile>> _jumps = ValueStreamController(const []);
  final ValueStreamController<List<Credential>> _creds = ValueStreamController(const []);
  final ValueStreamController<List<KnownHost>> _known = ValueStreamController(const []);

  MockConfig get _config => cloud.config;

  @override
  void onDataChanged(MockVaultData? data) {
    _hosts.value = List.unmodifiable(data?.hosts ?? const <Host>[]);
    _groups.value = List.unmodifiable(data?.groups ?? const <Group>[]);
    _jumps.value = List.unmodifiable(data?.jumpProfiles ?? const <JumpProfile>[]);
    _creds.value = List.unmodifiable(data?.credentials ?? const <Credential>[]);
    _known.value = List.unmodifiable(data?.knownHosts ?? const <KnownHost>[]);
  }

  void _emit() {
    onDataChanged(dataOrNull);
    cloud.recordMutation();
  }

  static void _upsert<T>(List<T> list, T item, bool Function(T) same) {
    final i = list.indexWhere(same);
    if (i >= 0) {
      list[i] = item;
    } else {
      list.add(item);
    }
  }

  // Hosts ------------------------------------------------------------------

  @override
  Stream<List<Host>> watchHosts() => _hosts.stream;

  /// Synchronous snapshot (tests / developer tooling).
  List<Host> get currentHosts => _hosts.value;

  @override
  Future<Host> saveHost(Host host) async {
    final error = host.validate();
    if (error != null) throw AppException.fromValidation(error);
    await mockDelay(_config.latency);
    _upsert(data.hosts, host, (h) => h.id == host.id);
    _emit();
    return host;
  }

  @override
  Future<Host> saveHostWithAuth(Host host, HostAuth auth) async {
    final error = host.validate();
    if (error != null) throw AppException.fromValidation(error);
    await mockDelay(_config.latency);
    final d = data;
    final stored = d.hosts.where((h) => h.id == host.id).firstOrNull;
    final inlineId = stored?.inlineCredentialId ?? host.inlineCredentialId;
    final inline = d.credentials.where((c) => c.id == inlineId).firstOrNull;
    final meta = Map<String, String>.of(host.metadata)
      ..remove(HostMetadataKeys.authPrompt)
      ..remove(HostMetadataKeys.inlineCredential);
    ObjectId? credentialId;
    var keepInline = false;

    switch (auth) {
      case HostAuthInherit():
        credentialId = null;
      case HostAuthCredential(credentialId: final id):
        if (!d.credentials.any((c) => c.id == id)) {
          throw const AppException(
            AppErrorCode.notFound,
            'The selected credential no longer exists',
            reason: AppErrorReason.credentialNotFound,
          );
        }
        credentialId = id;
        keepInline = id == inline?.id;
      case HostAuthInlinePassword(:final password):
        if (inline != null && inline.kind == CredentialKind.password) {
          if (password != null && password.isNotEmpty) {
            d.secrets.remove(inline.id)?.wipe();
            d.secrets[inline.id] = SecretText.fromBytes(password.exposeBytes());
          }
          final updated = inline.copyWith(name: '${host.name} password', username: host.username);
          d.credentials[d.credentials.indexOf(inline)] = updated;
          credentialId = inline.id;
        } else {
          if (password == null || password.isEmpty) {
            throw const AppException(
              AppErrorCode.validation,
              'Enter the password to save',
              reason: AppErrorReason.passwordRequired,
            );
          }
          final created = _newCredential('${host.name} password', CredentialKind.password, username: host.username);
          d.credentials.add(created);
          d.secrets[created.id] = SecretText.fromBytes(password.exposeBytes());
          credentialId = created.id;
        }
        keepInline = true;
        meta[HostMetadataKeys.inlineCredential] = credentialId.value;
      case HostAuthPasswordPrompt():
        credentialId = null;
        meta[HostMetadataKeys.authPrompt] = 'password';
      case HostAuthAgent(:final kind, :final agentPath):
        final path = (agentPath ?? '').trim();
        final existing = d.credentials
            .where((c) => c.kind == kind && (kind != CredentialKind.externalAgent || c.agentPath == path))
            .firstOrNull;
        if (existing != null) {
          credentialId = existing.id;
        } else {
          if (kind == CredentialKind.externalAgent && path.isEmpty) {
            throw const AppException(
              AppErrorCode.validation,
              'Enter the agent socket path or pipe name',
              reason: AppErrorReason.agentPathRequired,
            );
          }
          final created = _newCredential(
            kind == CredentialKind.osSshAgent ? 'System SSH agent' : 'Agent $path',
            kind,
            hasSecret: false,
            agentPath: kind == CredentialKind.externalAgent ? path : null,
          );
          d.credentials.add(created);
          credentialId = created.id;
        }
    }

    final saved = _copyHost(host, credentialId: credentialId, setCredential: true, metadata: meta);
    final i = d.hosts.indexWhere((h) => h.id == host.id);
    if (i >= 0) {
      d.hosts[i] = saved;
    } else {
      d.hosts.add(saved);
    }
    if (!keepInline) _deleteInlineIfUnused(d, inline?.id);
    _emit();
    return saved;
  }

  /// Deletes an inline credential once no host or group references it.
  void _deleteInlineIfUnused(MockVaultData d, ObjectId? id) {
    if (id == null) return;
    final used = d.hosts.any((h) => h.credentialId == id) || d.groups.any((g) => g.inheritedCredentialId == id);
    if (used) return;
    d.credentials.removeWhere((c) => c.id == id);
    d.secrets.remove(id)?.wipe();
  }

  @override
  Future<Credential> rememberKeyPassphrase(ObjectId credentialId, SecretText passphrase) async {
    if (passphrase.isEmpty) {
      throw const AppException(
        AppErrorCode.validation,
        'Enter the key passphrase',
        reason: AppErrorReason.keyPassphraseRequired,
      );
    }
    await mockDelay(_config.latency);
    final d = data;
    final i = d.credentials.indexWhere((c) => c.id == credentialId);
    if (i < 0) {
      throw const AppException(
        AppErrorCode.notFound,
        'Credential not found',
        reason: AppErrorReason.credentialNotFound,
      );
    }
    final c = d.credentials[i];
    if (!c.keyEncrypted) {
      throw const AppException(
        AppErrorCode.validation,
        'This key has no passphrase',
        reason: AppErrorReason.keyHasNoPassphrase,
      );
    }
    d.passphrases.remove(c.id)?.wipe();
    d.passphrases[c.id] = SecretText.fromBytes(passphrase.exposeBytes());
    final updated = _withPassphraseSecret(c, c.passphraseSecretId ?? ObjectId.generate());
    d.credentials[i] = updated;
    _emit();
    return updated;
  }

  @override
  Future<Credential> forgetKeyPassphrase(ObjectId credentialId) async {
    await mockDelay(_config.latency);
    final d = data;
    final i = d.credentials.indexWhere((c) => c.id == credentialId);
    if (i < 0) {
      throw const AppException(
        AppErrorCode.notFound,
        'Credential not found',
        reason: AppErrorReason.credentialNotFound,
      );
    }
    d.passphrases.remove(credentialId)?.wipe();
    final updated = _withPassphraseSecret(d.credentials[i], null);
    d.credentials[i] = updated;
    _emit();
    return updated;
  }

  static Credential _withPassphraseSecret(Credential c, ObjectId? secretId) => Credential(
    id: c.id,
    name: c.name,
    kind: c.kind,
    username: c.username,
    secretId: c.secretId,
    passphraseSecretId: secretId,
    keyEncrypted: c.keyEncrypted,
    keyAlgorithm: c.keyAlgorithm,
    publicKey: c.publicKey,
    certificate: c.certificate,
    fingerprint: c.fingerprint,
    agentPath: c.agentPath,
    createdAt: c.createdAt,
    updatedAt: DateTime.now().toUtc(),
  );

  @override
  Future<void> deleteHost(ObjectId id) async {
    await mockDelay(_config.latency);
    final d = data;
    final removed = d.hosts.where((h) => h.id == id).firstOrNull;
    d.hosts.removeWhere((h) => h.id == id);
    if (removed != null) _deleteInlineIfUnused(d, removed.inlineCredentialId);
    for (var i = 0; i < d.hosts.length; i++) {
      final h = d.hosts[i];
      if (h.jumpChain.contains(id)) {
        d.hosts[i] = _withJumpChain(h, [
          for (final j in h.jumpChain)
            if (j != id) j,
        ]);
      }
    }
    _emit();
  }

  // Groups -----------------------------------------------------------------

  @override
  Stream<List<Group>> watchGroups() => _groups.stream;

  @override
  Future<Group> saveGroup(Group group) async {
    if (group.name.trim().isEmpty) {
      throw const AppException(
        AppErrorCode.validation,
        'invalid name: must not be empty',
        reason: AppErrorReason.nameRequired,
      );
    }
    final d = data;
    // Reject cycles: the new parent must not be the group or its descendant.
    var cursor = group.parentId;
    while (cursor != null) {
      if (cursor == group.id) {
        throw const AppException(
          AppErrorCode.validation,
          'A group cannot be inside itself',
          reason: AppErrorReason.groupCycle,
        );
      }
      cursor = d.groups.where((g) => g.id == cursor).firstOrNull?.parentId;
    }
    await mockDelay(_config.latency);
    _upsert(d.groups, group, (g) => g.id == group.id);
    _emit();
    return group;
  }

  @override
  Future<void> deleteGroup(ObjectId id) async {
    await mockDelay(_config.latency);
    final d = data;
    final group = d.groups.where((g) => g.id == id).firstOrNull;
    if (group == null) return;
    d.groups.removeWhere((g) => g.id == id);
    for (var i = 0; i < d.groups.length; i++) {
      final g = d.groups[i];
      if (g.parentId == id) d.groups[i] = _withParent(g, group.parentId);
    }
    for (var i = 0; i < d.hosts.length; i++) {
      final h = d.hosts[i];
      if (h.groupId == id) d.hosts[i] = _withGroup(h, group.parentId);
    }
    _emit();
  }

  // Jump profiles ------------------------------------------------------------

  @override
  Stream<List<JumpProfile>> watchJumpProfiles() => _jumps.stream;

  @override
  Future<JumpProfile> saveJumpProfile(JumpProfile profile) async {
    if (profile.name.trim().isEmpty) {
      throw const AppException(
        AppErrorCode.validation,
        'invalid name: must not be empty',
        reason: AppErrorReason.nameRequired,
      );
    }
    if (profile.chain.isEmpty) {
      throw const AppException(
        AppErrorCode.validation,
        'A jump profile needs at least one hop',
        reason: AppErrorReason.jumpChainEmpty,
      );
    }
    await mockDelay(_config.latency);
    _upsert(data.jumpProfiles, profile, (p) => p.id == profile.id);
    _emit();
    return profile;
  }

  @override
  Future<void> deleteJumpProfile(ObjectId id) async {
    await mockDelay(_config.latency);
    data.jumpProfiles.removeWhere((p) => p.id == id);
    _emit();
  }

  // Credentials --------------------------------------------------------------

  @override
  Stream<List<Credential>> watchCredentials() => _creds.stream;

  Credential _newCredential(
    String name,
    CredentialKind kind, {
    String? username,
    bool hasSecret = true,
    bool remember = false,
    bool encrypted = false,
    KeyAlgorithm? algorithm,
    String? publicKey,
    String? fingerprint,
    String? certificate,
    String? agentPath,
  }) {
    if (name.trim().isEmpty) {
      throw const AppException(AppErrorCode.validation, 'Enter a name', reason: AppErrorReason.nameRequired);
    }
    final now = DateTime.now().toUtc();
    return Credential(
      id: ObjectId.generate(),
      name: name.trim(),
      kind: kind,
      username: (username ?? '').trim().isEmpty ? null : username!.trim(),
      secretId: hasSecret ? ObjectId.generate() : null,
      passphraseSecretId: remember ? ObjectId.generate() : null,
      keyEncrypted: encrypted,
      keyAlgorithm: algorithm,
      publicKey: publicKey,
      fingerprint: fingerprint,
      certificate: certificate,
      agentPath: agentPath,
      createdAt: now,
      updatedAt: now,
    );
  }

  @override
  Future<Credential> createPasswordCredential({
    required String name,
    required SecretText password,
    String? username,
  }) async {
    if (password.isEmpty) {
      throw const AppException(AppErrorCode.validation, 'Enter the password', reason: AppErrorReason.passwordRequired);
    }
    final cred = _newCredential(name, CredentialKind.password, username: username);
    await mockDelay(_config.latency);
    final d = data;
    d.credentials.add(cred);
    d.secrets[cred.id] = SecretText.fromBytes(password.exposeBytes());
    _emit();
    return cred;
  }

  @override
  Future<Credential> generateKeyCredential({
    required String name,
    required KeyAlgorithm algorithm,
    String? comment,
    SecretText? passphrase,
    bool rememberPassphrase = false,
  }) async {
    if (!KeyAlgorithm.generatable.contains(algorithm)) {
      throw const AppException(
        AppErrorCode.unsupported,
        'Generate Ed25519 or RSA 3072/4096 keys',
        reason: AppErrorReason.unsupportedKeyAlgorithm,
      );
    }
    final encrypted = passphrase != null && passphrase.isNotEmpty;
    final label = (comment ?? '').trim().isEmpty ? name.trim() : comment!.trim();
    final cred = _newCredential(
      name,
      CredentialKind.sshPrivateKey,
      encrypted: encrypted,
      remember: encrypted && rememberPassphrase,
      algorithm: algorithm,
      publicKey: fakePublicKey(algorithm, label),
      fingerprint: fakeFingerprint(),
    );
    // RSA generation is noticeably slower.
    await mockDelay(algorithm == KeyAlgorithm.ed25519 ? _config.latency : _config.kdfLatency * 2);
    final d = data;
    d.credentials.add(cred);
    d.secrets[cred.id] = SecretText(fakePrivateKeyPem());
    _emit();
    return cred;
  }

  @override
  Future<KeyInspection> inspectPrivateKey(SecretText privateKey) async {
    await mockDelay(_config.latency);
    return inspectPem(privateKey.expose());
  }

  /// Parses just enough of an OpenSSH / PEM key for the mock.
  static KeyInspection inspectPem(String text) {
    final pem = text.trim();
    if (pem.isEmpty) return const KeyInspection(valid: false, error: 'Paste a private key');
    final openssh = RegExp(r'-----BEGIN OPENSSH PRIVATE KEY-----([\s\S]*?)-----END OPENSSH PRIVATE KEY-----')
        .firstMatch(pem);
    if (openssh != null) {
      String decoded;
      try {
        final bytes = base64.decode(openssh.group(1)!.replaceAll(RegExp(r'\s'), ''));
        decoded = latin1.decode(bytes, allowInvalid: true);
      } on FormatException {
        return const KeyInspection(valid: false, error: 'The key body is not valid base64');
      }
      final algorithm = decoded.contains('ssh-rsa')
          ? KeyAlgorithm.rsa4096
          : decoded.contains('ecdsa-sha2-nistp256')
          ? KeyAlgorithm.ecdsaP256
          : KeyAlgorithm.ed25519;
      final encrypted = decoded.contains('bcrypt') || decoded.contains('aes256-ctr');
      return KeyInspection(
        valid: true,
        algorithm: algorithm,
        encrypted: encrypted,
        fingerprint: fakeFingerprint(),
        publicKey: fakePublicKey(algorithm, 'imported'),
      );
    }
    final legacy = RegExp(r'-----BEGIN (RSA|EC|DSA)? ?PRIVATE KEY-----').firstMatch(pem);
    if (legacy != null) {
      final kind = legacy.group(1);
      if (kind == 'DSA') {
        return const KeyInspection(valid: false, error: 'DSA keys are not supported');
      }
      return KeyInspection(
        valid: true,
        algorithm: kind == 'EC' ? KeyAlgorithm.ecdsaP256 : KeyAlgorithm.rsa3072,
        encrypted: pem.contains('ENCRYPTED'),
        fingerprint: fakeFingerprint(),
      );
    }
    if (pem.startsWith('ssh-') || pem.startsWith('ecdsa-')) {
      return const KeyInspection(valid: false, error: 'This is a public key. Paste the private key instead.');
    }
    return const KeyInspection(valid: false, error: 'Unrecognised key format');
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
    final inspection = inspectPem(privateKey.expose());
    if (!inspection.valid) {
      throw AppException(AppErrorCode.validation, inspection.error ?? 'Invalid key', reason: AppErrorReason.invalidKey);
    }
    final cert = (certificate ?? '').trim();
    if (cert.isNotEmpty && !RegExp(r'^[a-z0-9.@-]+-cert-v01@openssh\.com\s+\S+').hasMatch(cert)) {
      throw const AppException(
        AppErrorCode.validation,
        'Not an OpenSSH certificate line',
        reason: AppErrorReason.notACertificate,
      );
    }
    if (inspection.encrypted && rememberPassphrase && (passphrase == null || passphrase.isEmpty)) {
      throw const AppException(
        AppErrorCode.validation,
        'Enter the key passphrase to remember it',
        reason: AppErrorReason.keyPassphraseRequired,
      );
    }
    final cred = _newCredential(
      name,
      cert.isEmpty ? CredentialKind.sshPrivateKey : CredentialKind.sshCertificate,
      username: username,
      encrypted: inspection.encrypted,
      remember: inspection.encrypted && rememberPassphrase,
      algorithm: inspection.algorithm,
      publicKey: inspection.publicKey,
      fingerprint: inspection.fingerprint,
      certificate: cert.isEmpty ? null : cert,
    );
    await mockDelay(_config.latency);
    final d = data;
    d.credentials.add(cred);
    // The key is stored as-is: its own passphrase protection is kept.
    d.secrets[cred.id] = SecretText.fromBytes(privateKey.exposeBytes());
    _emit();
    return cred;
  }

  @override
  Future<Credential> createAgentCredential({
    required String name,
    required CredentialKind kind,
    String? username,
    String? agentPath,
  }) async {
    if (kind != CredentialKind.osSshAgent && kind != CredentialKind.externalAgent) {
      throw const AppException(
        AppErrorCode.validation,
        'Not an agent credential',
        reason: AppErrorReason.notAgentCredential,
      );
    }
    if (kind == CredentialKind.externalAgent && (agentPath ?? '').trim().isEmpty) {
      throw const AppException(
        AppErrorCode.validation,
        'Enter the agent socket path or pipe name',
        reason: AppErrorReason.agentPathRequired,
      );
    }
    final cred = _newCredential(
      name,
      kind,
      username: username,
      hasSecret: false,
      agentPath: kind == CredentialKind.externalAgent ? agentPath!.trim() : null,
    );
    await mockDelay(_config.latency);
    data.credentials.add(cred);
    _emit();
    return cred;
  }

  @override
  Future<Credential> updateCredential(Credential credential) async {
    await mockDelay(_config.latency);
    _upsert(data.credentials, credential, (c) => c.id == credential.id);
    _emit();
    return credential;
  }

  @override
  Future<void> deleteCredential(ObjectId id) async {
    await mockDelay(_config.latency);
    final d = data;
    d.credentials.removeWhere((c) => c.id == id);
    d.secrets.remove(id)?.wipe();
    _emit();
  }

  @override
  Future<SecretText> revealCredentialSecret(ObjectId credentialId) async {
    await mockDelay(_config.latency);
    final secret = data.secrets[credentialId];
    if (secret == null) {
      throw const AppException(
        AppErrorCode.notFound,
        'No secret stored for this credential',
        reason: AppErrorReason.secretNotStored,
      );
    }
    return SecretText.fromBytes(secret.exposeBytes());
  }

  // Planning -----------------------------------------------------------------

  List<Group> _groupChain(MockVaultData d, ObjectId? groupId) {
    final chain = <Group>[];
    var cursor = groupId;
    final seen = <ObjectId>{};
    while (cursor != null && seen.add(cursor)) {
      final g = d.groups.where((x) => x.id == cursor).firstOrNull;
      if (g == null) break;
      chain.add(g);
      cursor = g.parentId;
    }
    return chain;
  }

  String _hostLabel(MockVaultData d, Host h) {
    final eff = _resolveBasic(d, h);
    final user = eff.$2.value;
    final port = eff.$1.value ?? defaultSshPort;
    return '${h.name} (${user == null ? '' : '$user@'}${h.address}${port == 22 ? '' : ':$port'})';
  }

  (Resolved<int>, Resolved<String>, Resolved<ObjectId>) _resolveBasic(MockVaultData d, Host host) {
    final chain = _groupChain(d, host.groupId);
    Resolved<int> port = host.port != null
        ? Resolved(host.port, ValueSource.host)
        : const Resolved(defaultSshPort, ValueSource.appDefault);
    if (host.port == null) {
      for (final g in chain) {
        if (g.inheritedPort != null) {
          port = Resolved(g.inheritedPort, ValueSource.group, sourceName: g.name);
          break;
        }
      }
    }
    Resolved<ObjectId> credential = host.credentialId != null
        ? Resolved(host.credentialId, ValueSource.host)
        : const Resolved.unset();
    if (host.credentialId == null && !host.promptsForPassword) {
      for (final g in chain) {
        if (g.inheritedCredentialId != null) {
          credential = Resolved(g.inheritedCredentialId, ValueSource.group, sourceName: g.name);
          break;
        }
      }
    }
    Resolved<String> username = host.username != null
        ? Resolved(host.username, ValueSource.host)
        : const Resolved.unset();
    if (host.username == null) {
      for (final g in chain) {
        if (g.inheritedUsername != null) {
          username = Resolved(g.inheritedUsername, ValueSource.group, sourceName: g.name);
          break;
        }
      }
    }
    if (username.value == null && credential.value != null) {
      final c = d.credentials.where((c) => c.id == credential.value).firstOrNull;
      if (c?.username != null) {
        username = Resolved(c!.username, ValueSource.host, sourceName: 'credential ${c.name}');
      }
    }
    return (port, username, credential);
  }

  @override
  Future<EffectiveHostConfig> resolveEffective(Host host) async {
    final d = data;
    final (port, username, resolvedCredential) = _resolveBasic(d, host);
    var credential = resolvedCredential;
    final chain = _groupChain(d, host.groupId);
    List<ObjectId> hops = const [];
    Resolved<String> routeSource = const Resolved('Direct', ValueSource.unset);
    if (host.jumpChain.isNotEmpty) {
      hops = host.jumpChain;
      routeSource = const Resolved('Custom chain', ValueSource.host);
    } else if (host.jumpProfileId != null) {
      final profile = d.jumpProfiles.where((p) => p.id == host.jumpProfileId).firstOrNull;
      if (profile != null) {
        hops = profile.chain;
        routeSource = Resolved(profile.name, ValueSource.jumpProfile, sourceName: profile.name);
      }
    } else {
      for (final g in chain) {
        if (g.inheritedJumpProfileId != null) {
          final profile = d.jumpProfiles.where((p) => p.id == g.inheritedJumpProfileId).firstOrNull;
          if (profile != null) {
            hops = profile.chain;
            routeSource = Resolved(profile.name, ValueSource.group, sourceName: g.name);
          }
          break;
        }
      }
    }
    final problems = <String>[];
    final route = <RouteHop>[];
    for (final id in hops) {
      final hop = d.hosts.where((h) => h.id == id).firstOrNull;
      if (hop == null) {
        problems.add('A jump host in the chain was deleted');
      } else if (hop.id == host.id) {
        problems.add('The host cannot jump through itself');
      } else {
        route.add(RouteHop(hostId: hop.id, label: _hostLabel(d, hop)));
      }
    }
    final cred = d.credentials.where((c) => c.id == credential.value).firstOrNull;
    if (host.promptsForPassword) {
      credential = const Resolved(null, ValueSource.host);
    } else if (credential.value == null) {
      problems.add('No credential: you will be asked for a password when connecting');
    } else if (cred == null) {
      problems.add('The selected credential no longer exists');
    }
    if (username.value == null) {
      problems.add('No username: set one on the host or on a group');
    }
    await mockDelay(Duration.zero);
    return EffectiveHostConfig(
      port: port,
      username: username,
      credentialId: credential,
      credentialName: host.promptsForPassword ? 'Password — asked when connecting' : cred?.name,
      route: route,
      routeSource: routeSource,
      groupPath: [for (final g in chain.reversed) g.name],
      problems: problems,
    );
  }

  @override
  Future<EffectiveGroupDefaults> resolveGroupDefaults(ObjectId groupId) async {
    final d = data;
    final chain = _groupChain(d, groupId);
    Resolved<T> pick<T>(T? Function(Group) get) {
      for (final g in chain) {
        final v = get(g);
        if (v != null) {
          return Resolved(v, g.id == groupId ? ValueSource.host : ValueSource.group, sourceName: g.name);
        }
      }
      return const Resolved.unset();
    }

    final credential = pick((g) => g.inheritedCredentialId);
    final jump = pick((g) => g.inheritedJumpProfileId);
    await mockDelay(Duration.zero);
    return EffectiveGroupDefaults(
      username: pick((g) => g.inheritedUsername),
      port: pick((g) => g.inheritedPort),
      credentialId: credential,
      credentialName: d.credentials.where((c) => c.id == credential.value).firstOrNull?.name,
      jumpProfileId: jump,
      jumpProfileName: d.jumpProfiles.where((p) => p.id == jump.value).firstOrNull?.name,
    );
  }

  // Known hosts ------------------------------------------------------------------

  @override
  Stream<List<KnownHost>> watchKnownHosts() => _known.stream;

  @override
  Future<void> deleteKnownHost(ObjectId id) async {
    await mockDelay(_config.latency);
    final d = data;
    final removed = d.knownHosts.where((k) => k.id == id).firstOrNull;
    d.knownHosts.removeWhere((k) => k.id == id);
    if (removed != null) d.changedKeyPatterns.remove(removed.hostPattern);
    _emit();
  }

  /// Terminal mock support: known-host lookup for a host.
  KnownHost? knownHostFor(String pattern) => dataOrNull?.knownHosts.where((k) => k.hostPattern == pattern).firstOrNull;

  bool keyChanged(String pattern) => dataOrNull?.changedKeyPatterns.contains(pattern) ?? false;

  Host? hostById(ObjectId id) => dataOrNull?.hosts.where((h) => h.id == id).firstOrNull;

  void addKnownHost(KnownHost known) {
    final d = dataOrNull;
    if (d == null) return;
    d.knownHosts.add(known);
    _emit();
  }

  Future<void> dispose() async {
    await disposeScope();
    await _hosts.close();
    await _groups.close();
    await _jumps.close();
    await _creds.close();
    await _known.close();
  }

  // Copy helpers (the models are immutable) ---------------------------------------

  static Host _withJumpChain(Host h, List<ObjectId> chain) => _copyHost(h, jumpChain: chain);

  static Host _withGroup(Host h, ObjectId? groupId) => _copyHost(h, groupId: groupId, setGroup: true);

  static Host _copyHost(
    Host h, {
    List<ObjectId>? jumpChain,
    ObjectId? groupId,
    bool setGroup = false,
    ObjectId? credentialId,
    bool setCredential = false,
    Map<String, String>? metadata,
  }) => Host(
    id: h.id,
    protocol: h.protocol,
    rdpDomain: h.rdpDomain,
    rdpWidth: h.rdpWidth,
    rdpHeight: h.rdpHeight,
    name: h.name,
    address: h.address,
    port: h.port,
    username: h.username,
    credentialId: setCredential ? credentialId : h.credentialId,
    groupId: setGroup ? groupId : h.groupId,
    jumpChain: jumpChain ?? h.jumpChain,
    jumpProfileId: h.jumpProfileId,
    proxyId: h.proxyId,
    hostKeyPolicy: h.hostKeyPolicy,
    backend: h.backend,
    keepaliveSecs: h.keepaliveSecs,
    agentForwarding: h.agentForwarding,
    tags: h.tags,
    notes: h.notes,
    metadata: metadata ?? h.metadata,
    createdAt: h.createdAt,
    updatedAt: DateTime.now().toUtc(),
  );

  static Group _withParent(Group g, ObjectId? parentId) => Group(
    id: g.id,
    name: g.name,
    parentId: parentId,
    inheritedUsername: g.inheritedUsername,
    inheritedPort: g.inheritedPort,
    inheritedCredentialId: g.inheritedCredentialId,
    inheritedJumpProfileId: g.inheritedJumpProfileId,
    tags: g.tags,
    createdAt: g.createdAt,
    updatedAt: DateTime.now().toUtc(),
  );
}
