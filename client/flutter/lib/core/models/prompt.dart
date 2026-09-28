import 'package:consolecrypt/core/models/ids.dart';

/// A question from the core that needs the user while a connection is being
/// made **outside a terminal tab** — SFTP browser, tunnels, exec / AI runs
/// (app-core `PromptRequest`, ADR-0107). Terminal tabs keep answering their
/// own prompts inline; everything else goes through `PromptService`.
///
/// Prompts time out in the core (5 min by default, then they count as
/// rejected / cancelled).
sealed class CorePrompt {
  const CorePrompt(this.requestId);

  /// Correlates the answer (`PromptService.answer*`).
  final String requestId;

  /// The vault host the connection is for, if known (the first hop of a jump
  /// chain may differ from the host being opened).
  ObjectId? get hostId;
}

/// "Unknown host key" (host-key policy `ask`). Changed keys never ask: they
/// fail hard (`hostKeyChanged`).
final class HostKeyCorePrompt extends CorePrompt {
  const HostKeyCorePrompt({
    required String requestId,
    required this.host,
    required this.port,
    required this.hostPattern,
    required this.keyType,
    required this.fingerprintSha256,
    this.hostId,
    this.hostName,
    this.hopIndex = 0,
    this.hopCount = 1,
    this.otherKnownKeyTypes = const [],
  }) : super(requestId);

  final String host;
  final int port;

  /// `host` or `[host]:port` as written to known hosts.
  final String hostPattern;
  @override
  final ObjectId? hostId;
  final String? hostName;

  /// Position in a jump chain (0-based) and its length.
  final int hopIndex;
  final int hopCount;
  final String keyType;
  final String fingerprintSha256;

  /// Other key types already trusted for this host — be extra careful.
  final List<String> otherKnownKeyTypes;
}

/// "Password for this host" (auth mode ask-at-connect). Used once, never
/// stored.
final class PasswordCorePrompt extends CorePrompt {
  const PasswordCorePrompt({required String requestId, required this.hostName, this.hostId}) : super(requestId);

  @override
  final ObjectId? hostId;
  final String hostName;
}

/// "Passphrase of this SSH key" (no passphrase remembered). `attempt > 0`
/// after a wrong passphrase.
final class PassphraseCorePrompt extends CorePrompt {
  const PassphraseCorePrompt({
    required String requestId,
    required this.credentialId,
    required this.credentialName,
    this.attempt = 0,
  }) : super(requestId);

  final ObjectId credentialId;
  final String credentialName;
  final int attempt;

  bool get isRetry => attempt > 0;

  @override
  ObjectId? get hostId => null;
}
