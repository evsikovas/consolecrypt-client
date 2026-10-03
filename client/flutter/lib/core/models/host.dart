import 'package:consolecrypt/core/models/ids.dart';
import 'package:consolecrypt/core/models/validation.dart';

/// Mirrors `cc_models::host::DEFAULT_SSH_PORT`.
const int defaultSshPort = 22;
const int defaultRdpPort = 3389;

enum HostProtocol { ssh, rdp }

/// Mirrors `cc_models::host::HostKeyPolicy`.
enum HostKeyPolicy {
  /// Unknown key → ask the user (TOFU with confirmation). Changed key → hard fail.
  ask('ask'),

  /// Only keys already in known hosts are accepted.
  strict('strict'),

  /// Unknown keys are accepted and recorded; changed key → hard fail.
  acceptNew('accept_new');

  const HostKeyPolicy(this.wireName);

  /// serde name in `cc-models`.
  final String wireName;
}

/// Mirrors `cc_models::host::SshBackend`.
enum SshBackend {
  /// Built-in (russh).
  native('native'),

  /// System OpenSSH (compatibility).
  openSsh('open_ssh');

  const SshBackend(this.wireName);

  final String wireName;
}

/// Mirrors `cc_models::host::Host`. `null` optional fields inherit from the
/// group chain; the effective values come from app-core's Connection Planner
/// (see `InventoryService.resolveEffective`), never from UI-side assembly.
final class Host {
  const Host({
    required this.id,
    required this.name,
    required this.address,
    required this.createdAt,
    required this.updatedAt,
    this.protocol = HostProtocol.ssh,
    this.rdpDomain,
    this.rdpWidth = 1280,
    this.rdpHeight = 720,
    this.port,
    this.username,
    this.credentialId,
    this.groupId,
    this.jumpChain = const [],
    this.jumpProfileId,
    this.proxyId,
    this.hostKeyPolicy = HostKeyPolicy.ask,
    this.backend = SshBackend.native,
    this.keepaliveSecs,
    this.agentForwarding = false,
    this.tags = const [],
    this.notes = '',
    this.metadata = const {},
  });

  factory Host.create({required String name, required String address}) {
    final now = DateTime.now().toUtc();
    return Host(id: ObjectId.generate(), name: name, address: address, createdAt: now, updatedAt: now);
  }

  final ObjectId id;
  final HostProtocol protocol;
  final String? rdpDomain;
  final int rdpWidth;
  final int rdpHeight;
  bool get isRdp => protocol == HostProtocol.rdp;
  final String name;

  /// Hostname or IP literal.
  final String address;

  /// SSH: inherit from group, else 22. RDP: use 3389; never inherits SSH options.
  final int? port;

  /// SSH may inherit from the group. RDP uses only explicit connection credentials.
  final String? username;

  /// SSH may inherit from the group. RDP uses only explicit connection credentials.
  final ObjectId? credentialId;
  final ObjectId? groupId;

  /// Ordered hops (host ids), first = closest to the client. Empty = inherit
  /// the group jump profile (if any), else direct.
  final List<ObjectId> jumpChain;

  /// Explicit jump profile; overrides the group's.
  final ObjectId? jumpProfileId;
  final ObjectId? proxyId;
  final HostKeyPolicy hostKeyPolicy;
  final SshBackend backend;

  /// Keepalive interval; `null` = app default.
  final int? keepaliveSecs;
  final bool agentForwarding;
  final List<String> tags;
  final String notes;
  final Map<String, String> metadata;
  final DateTime createdAt;
  final DateTime updatedAt;

  /// Change membership without altering explicit connection/authentication fields.
  Host withGroup(ObjectId? value) => Host(
    id: id,
    protocol: protocol,
    rdpDomain: rdpDomain,
    rdpWidth: rdpWidth,
    rdpHeight: rdpHeight,
    name: name,
    address: address,
    createdAt: createdAt,
    updatedAt: DateTime.now().toUtc(),
    port: port,
    username: username,
    credentialId: credentialId,
    groupId: value,
    jumpChain: jumpChain,
    jumpProfileId: jumpProfileId,
    proxyId: proxyId,
    hostKeyPolicy: hostKeyPolicy,
    backend: backend,
    keepaliveSecs: keepaliveSecs,
    agentForwarding: agentForwarding,
    tags: tags,
    notes: notes,
    metadata: metadata,
  );

  /// Mirrors `Host::validate`.
  ValidationError? validate() {
    if (name.trim().isEmpty) {
      return const ValidationError('name', 'must not be empty');
    }
    final addr = address.trim();
    if (addr.isEmpty) {
      return const ValidationError('address', 'must not be empty');
    }
    if (RegExp(r'[\s\x00-\x1f\x7f]').hasMatch(addr)) {
      return const ValidationError('address', 'must not contain whitespace');
    }
    final p = port;
    if (p != null && (p < 1 || p > 65535)) {
      return const ValidationError('port', 'must be 1..=65535');
    }
    if (jumpChain.contains(id)) {
      return const ValidationError('jump_chain', 'host cannot jump through itself');
    }
    return null;
  }
}

/// Mirrors `cc_models::host::JumpProfile` — reusable ordered chain.
final class JumpProfile {
  const JumpProfile({
    required this.id,
    required this.name,
    required this.chain,
    required this.createdAt,
    required this.updatedAt,
  });

  final ObjectId id;
  final String name;

  /// Host ids, first = closest to the client.
  final List<ObjectId> chain;
  final DateTime createdAt;
  final DateTime updatedAt;
}

/// Mirrors `cc_models::host::ProxyKind`.
enum ProxyKind {
  socks5('socks5', 'SOCKS5'),
  httpConnect('http_connect', 'HTTP CONNECT');

  const ProxyKind(this.wireName, this.label);

  final String wireName;
  final String label;
}

/// Mirrors `cc_models::host::Proxy` (network proxy to reach the first hop).
final class Proxy {
  const Proxy({
    required this.id,
    required this.name,
    required this.kind,
    required this.address,
    required this.port,
    required this.createdAt,
    required this.updatedAt,
    this.username,
    this.passwordSecretId,
  });

  final ObjectId id;
  final String name;
  final ProxyKind kind;
  final String address;
  final int port;
  final String? username;

  /// Secret with the proxy password (never loaded into the UI).
  final ObjectId? passwordSecretId;
  final DateTime createdAt;
  final DateTime updatedAt;
}

/// `Host.metadata` keys used for host-level authentication until cc-models
/// gains dedicated additive fields (requested from the model owners).
abstract final class HostMetadataKeys {
  /// `'password'`: ask for the password at connect time, never store it.
  static const authPrompt = 'cc.auth.prompt';

  /// Id of the Password credential created from the host editor (owned by
  /// this host, deleted with it). Shared credentials are not marked.
  static const inlineCredential = 'cc.auth.inline_credential';
}

extension HostAuthMetadata on Host {
  /// Password auth without a stored secret: the terminal asks each time.
  bool get promptsForPassword => metadata[HostMetadataKeys.authPrompt] == 'password';

  /// The host's own (inline) password credential, if any.
  ObjectId? get inlineCredentialId {
    final id = metadata[HostMetadataKeys.inlineCredential];
    return id == null ? null : ObjectId(id);
  }
}
