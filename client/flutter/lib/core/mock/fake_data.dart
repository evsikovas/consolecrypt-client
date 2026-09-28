import 'dart:convert';
import 'dart:math';

import 'package:consolecrypt/core/models/models.dart';

/// Realistic demo inventory. Hostnames use reserved example domains and
/// private address ranges only (no real infrastructure, ADR-0005).
final class FakeInventory {
  FakeInventory._({
    required this.groups,
    required this.hosts,
    required this.credentials,
    required this.jumpProfiles,
    required this.knownHosts,
    required this.changedKeyPatterns,
    required this.tunnels,
  });

  factory FakeInventory.build() {
    final now = DateTime.now().toUtc();
    DateTime ago(Duration d) => now.subtract(d);

    Credential cred(
      String name,
      CredentialKind kind, {
      KeyAlgorithm? algorithm,
      String? username,
      bool encrypted = false,
      bool remember = false,
      String? certificate,
      String? agentPath,
    }) {
      final isKey = kind == CredentialKind.sshPrivateKey || kind == CredentialKind.sshCertificate;
      return Credential(
        id: ObjectId.generate(),
        name: name,
        kind: kind,
        username: username,
        secretId: kind == CredentialKind.password || isKey ? ObjectId.generate() : null,
        passphraseSecretId: remember ? ObjectId.generate() : null,
        keyEncrypted: encrypted,
        keyAlgorithm: algorithm,
        publicKey: isKey ? fakePublicKey(algorithm ?? KeyAlgorithm.ed25519, name) : null,
        fingerprint: isKey ? fakeFingerprint() : null,
        certificate: certificate,
        agentPath: agentPath,
        createdAt: ago(const Duration(days: 120)),
        updatedAt: ago(const Duration(days: 12)),
      );
    }

    final prodKey = cred(
      'prod-deploy',
      CredentialKind.sshPrivateKey,
      algorithm: KeyAlgorithm.ed25519,
      encrypted: true,
      remember: true,
    );
    final stagingKey = cred('staging', CredentialKind.sshPrivateKey, algorithm: KeyAlgorithm.rsa4096);
    final homePassword = cred('home-lab password', CredentialKind.password, username: 'pi');
    final agent = cred('System ssh-agent', CredentialKind.osSshAgent);
    final corpCert = cred(
      'corp CA certificate',
      CredentialKind.sshCertificate,
      algorithm: KeyAlgorithm.ed25519,
      certificate: 'ssh-ed25519-cert-v01@openssh.com AAAAIHNzaC1lZDI1NTE5LWNlcnQtdjAxQG9wZW5zc2guY29t corp-ca',
    );

    Host host(
      String name,
      String address, {
      Group? group,
      int? port,
      String? username,
      Credential? credential,
      List<Host> jumps = const [],
      List<String> tags = const [],
      HostKeyPolicy policy = HostKeyPolicy.ask,
      SshBackend backend = SshBackend.native,
      String notes = '',
    }) => Host(
      id: ObjectId.generate(),
      name: name,
      address: address,
      port: port,
      username: username,
      credentialId: credential?.id,
      groupId: group?.id,
      jumpChain: [for (final j in jumps) j.id],
      tags: tags,
      hostKeyPolicy: policy,
      backend: backend,
      notes: notes,
      createdAt: ago(const Duration(days: 90)),
      updatedAt: ago(const Duration(days: 3)),
    );

    final bastionA = host(
      'bastion-a',
      'bastion-a.example.net',
      username: 'jump',
      credential: prodKey,
      tags: ['bastion'],
    );
    final bastionB = host(
      'bastion-b',
      '10.0.0.5',
      username: 'jump',
      credential: prodKey,
      jumps: [bastionA],
      tags: ['bastion'],
    );
    final bastions = JumpProfile(
      id: ObjectId.generate(),
      name: 'Prod bastions',
      chain: [bastionA.id, bastionB.id],
      createdAt: ago(const Duration(days: 90)),
      updatedAt: ago(const Duration(days: 30)),
    );

    Group group(
      String name, {
      Group? parent,
      String? username,
      int? port,
      Credential? credential,
      JumpProfile? jump,
      List<String> tags = const [],
    }) => Group(
      id: ObjectId.generate(),
      name: name,
      parentId: parent?.id,
      inheritedUsername: username,
      inheritedPort: port,
      inheritedCredentialId: credential?.id,
      inheritedJumpProfileId: jump?.id,
      tags: tags,
      createdAt: ago(const Duration(days: 100)),
      updatedAt: ago(const Duration(days: 20)),
    );

    final production = group('Production', username: 'deploy', credential: prodKey, jump: bastions, tags: ['prod']);
    final databases = group('Databases', parent: production, username: 'dba');
    final web = group('Web', parent: production, port: 2222);
    final staging = group('Staging', username: 'ubuntu', credential: stagingKey, tags: ['staging']);
    final homeLab = group('Home lab', username: 'pi', credential: homePassword);

    final prodDb1 = host('prod-db-1', '10.10.10.20', group: databases, tags: ['postgres', 'primary']);
    final prodDb2 = host('prod-db-2', '10.10.10.21', group: databases, tags: ['postgres', 'replica']);
    final prodWeb1 = host('prod-web-1', '10.10.20.11', group: web, tags: ['nginx']);
    final stagingWeb = host('staging-web', 'staging.example.net', group: staging, tags: ['nginx']);
    final k8sAdmin = host(
      'k8s-admin',
      'k8s-admin.example.net',
      group: staging,
      tags: ['kubernetes'],
      notes: 'kubectl + helm configured for the staging cluster.',
    );
    final pi = host('raspberry-pi', '192.168.1.50', group: homeLab, tags: ['home']);
    final legacy = host(
      'legacy-ftp',
      'legacy.example.org',
      username: 'admin',
      credential: agent,
      backend: SshBackend.openSsh,
      tags: ['legacy'],
      notes: 'Old OpenSSH 5.x — uses the OpenSSH fallback backend.',
    );
    final certHost = host(
      'build-runner',
      'ci.example.net',
      username: 'runner',
      credential: corpCert,
      policy: HostKeyPolicy.strict,
      tags: ['ci'],
    );

    KnownHost known(Host h, {int port = 22}) => KnownHost(
      id: ObjectId.generate(),
      hostPattern: hostPattern(h.address, port),
      keyType: 'ssh-ed25519',
      publicKey: fakeBase64(32),
      fingerprintSha256: fakeFingerprint(),
      source: KnownHostSource.tofu,
      addedAt: ago(const Duration(days: 80)),
      updatedAt: ago(const Duration(days: 80)),
    );

    Tunnel tunnel(
      String name,
      TunnelKind kind,
      Host via,
      String bindHost,
      int bindPort, {
      String? targetHost,
      int? targetPort,
      bool autoStart = false,
    }) => Tunnel(
      id: ObjectId.generate(),
      name: name,
      kind: kind,
      hostId: via.id,
      bindHost: bindHost,
      bindPort: bindPort,
      targetHost: targetHost,
      targetPort: targetPort,
      autoStart: autoStart,
      createdAt: ago(const Duration(days: 40)),
      updatedAt: ago(const Duration(days: 5)),
    );

    return FakeInventory._(
      groups: [production, databases, web, staging, homeLab],
      hosts: [bastionA, bastionB, prodDb1, prodDb2, prodWeb1, stagingWeb, k8sAdmin, pi, legacy, certHost],
      credentials: [prodKey, stagingKey, homePassword, agent, corpCert],
      jumpProfiles: [bastions],
      knownHosts: [
        known(bastionA),
        known(bastionB),
        known(prodDb1),
        known(prodDb2),
        known(prodWeb1, port: 2222),
        known(pi),
        known(legacy),
        known(certHost),
      ],
      changedKeyPatterns: {hostPattern(legacy.address, 22)},
      tunnels: [
        tunnel(
          'Prod Postgres',
          TunnelKind.local,
          bastionB,
          '127.0.0.1',
          15432,
          targetHost: '10.10.10.20',
          targetPort: 5432,
        ),
        tunnel(
          'Webhook dev server',
          TunnelKind.remote,
          stagingWeb,
          '127.0.0.1',
          9000,
          targetHost: 'localhost',
          targetPort: 3000,
        ),
        tunnel('SOCKS via bastion', TunnelKind.dynamic, bastionA, '127.0.0.1', 1080),
        tunnel(
          'Shared Grafana (LAN)',
          TunnelKind.local,
          bastionB,
          '0.0.0.0',
          3001,
          targetHost: '10.10.30.5',
          targetPort: 3000,
        ),
      ],
    );
  }

  final List<Group> groups;
  final List<Host> hosts;
  final List<Credential> credentials;
  final List<JumpProfile> jumpProfiles;
  final List<KnownHost> knownHosts;

  /// Known-host patterns whose server now presents a different key.
  final Set<String> changedKeyPatterns;
  final List<Tunnel> tunnels;
}

final Random _rng = Random.secure();

String fakeBase64(int bytes) => base64.encode(List<int>.generate(bytes, (_) => _rng.nextInt(256))).replaceAll('=', '');

String fakeFingerprint() => 'SHA256:${fakeBase64(32)}';

String fakePublicKey(KeyAlgorithm algorithm, String comment) => switch (algorithm) {
  KeyAlgorithm.rsa2048 ||
  KeyAlgorithm.rsa3072 ||
  KeyAlgorithm.rsa4096 => 'ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQ${fakeBase64(96)} $comment',
  KeyAlgorithm.ecdsaP256 => 'ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTY${fakeBase64(48)} $comment',
  _ => 'ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAI${fakeBase64(32)} $comment',
};

List<Snippet> buildFakeSnippets() {
  final now = DateTime.now().toUtc();
  var n = 0;
  Snippet s(
    String name,
    SnippetType type,
    String template, {
    String description = '',
    RiskLevel risk = RiskLevel.readOnly,
    List<String> tags = const [],
    List<SnippetVariable> variables = const [],
    SnippetSource source = SnippetSource.user,
    int usage = 0,
  }) {
    n++;
    return Snippet(
      id: ObjectId.generate(),
      name: name,
      description: description,
      snippetType: type,
      template: template,
      variables: variables,
      tags: tags,
      riskLevel: risk,
      source: source,
      usageCount: usage,
      lastUsedAt: usage > 0 ? now.subtract(Duration(hours: n * 5)) : null,
      createdAt: now.subtract(Duration(days: 60 - n)),
      updatedAt: now.subtract(Duration(days: 30 - n)),
    );
  }

  return [
    s(
      'Tail pod logs',
      SnippetType.kubectl,
      'kubectl logs -n {{namespace}} {{pod}} --tail={{lines}}',
      description: 'Last lines of a pod log',
      tags: ['k8s', 'logs'],
      usage: 14,
      variables: const [
        SnippetVariable(name: 'namespace', description: 'Kubernetes namespace', defaultValue: 'default'),
        SnippetVariable(name: 'pod', description: 'Pod or deploy/<name>'),
        SnippetVariable(name: 'lines', description: 'Number of lines', defaultValue: '200'),
      ],
    ),
    s('Disk usage', SnippetType.bash, 'df -h', description: 'Free space per filesystem', tags: ['disk'], usage: 22),
    s(
      'Largest files',
      SnippetType.bash,
      'du -ah {{path}} | sort -rh | head -n {{count}}',
      description: 'Biggest files and directories under a path',
      tags: ['disk'],
      variables: const [
        SnippetVariable(name: 'path', defaultValue: '/var/log'),
        SnippetVariable(name: 'count', defaultValue: '20'),
      ],
    ),
    s(
      'Restart service',
      SnippetType.bash,
      'sudo systemctl restart {{service}}',
      description: 'Restart a systemd unit',
      risk: RiskLevel.modifying,
      tags: ['systemd'],
      usage: 6,
    ),
    s(
      'Clean old compressed logs',
      SnippetType.bash,
      "find {{dir}} -name '*.gz' -mtime +{{days}} -delete",
      description: 'Delete rotated logs older than N days',
      risk: RiskLevel.destructive,
      tags: ['cleanup', 'logs'],
      variables: const [
        SnippetVariable(name: 'dir', defaultValue: '/var/log'),
        SnippetVariable(name: 'days', defaultValue: '30'),
      ],
    ),
    s(
      'Active Postgres queries',
      SnippetType.postgresql,
      "SELECT pid, usename, state, query FROM pg_stat_activity WHERE state <> 'idle';",
      description: 'What is running right now',
      tags: ['postgres'],
      usage: 3,
    ),
    s(
      'Helm upgrade release',
      SnippetType.helm,
      'helm upgrade {{release}} {{chart}} -n {{namespace}} --reuse-values',
      risk: RiskLevel.modifying,
      tags: ['k8s', 'helm'],
      source: SnippetSource.ai,
    ),
    s('Containers', SnippetType.docker, 'docker ps -a', tags: ['docker'], usage: 9),
    s('Cluster health', SnippetType.opensearchDsl, 'GET _cluster/health?pretty', tags: ['opensearch']),
    s('Drop table', SnippetType.sql, 'DROP TABLE IF EXISTS {{table}};', risk: RiskLevel.destructive, tags: ['sql']),
    s(
      'Listening ports',
      SnippetType.bash,
      'ss -tulpn',
      description: 'Sockets in LISTEN state',
      tags: ['network'],
      usage: 4,
    ),
    s('Running services', SnippetType.powershell, "Get-Service | Where-Object Status -eq 'Running'", tags: ['windows']),
  ];
}

List<AiProviderConfig> buildFakeProviders() {
  final ollama = AiProviderConfig.draft(AiProviderKind.ollama).copyWith(name: 'Ollama (local)', isDefault: true);
  final lmStudio = AiProviderConfig.draft(AiProviderKind.lmStudio).copyWith(name: 'LM Studio');
  final deepseek = AiProviderConfig.draft(AiProviderKind.deepseek);
  return [ollama, lmStudio, deepseek];
}
