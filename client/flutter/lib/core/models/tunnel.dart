import 'package:consolecrypt/core/models/ids.dart';
import 'package:consolecrypt/core/models/validation.dart';

/// Mirrors `cc_models::tunnel::TunnelKind`.
enum TunnelKind {
  /// `bind_host:bind_port` (local) → SSH → `target_host:target_port`.
  local('local'),

  /// `bind_host:bind_port` (remote side) → SSH → `target_host:target_port` (local side).
  remote('remote'),

  /// Local SOCKS5 listener → `direct-tcpip` through SSH.
  dynamic('dynamic');

  const TunnelKind(this.wireName);

  final String wireName;

  bool get needsTarget => this != TunnelKind.dynamic;
}

/// Mirrors `cc_models::tunnel::Tunnel`.
final class Tunnel {
  const Tunnel({
    required this.id,
    required this.name,
    required this.kind,
    required this.hostId,
    required this.bindHost,
    required this.bindPort,
    required this.createdAt,
    required this.updatedAt,
    this.targetHost,
    this.targetPort,
    this.autoStart = false,
  });

  final ObjectId id;
  final String name;
  final TunnelKind kind;

  /// Host whose SSH connection (incl. its jump chain) carries the tunnel.
  final ObjectId hostId;
  final String bindHost;
  final int bindPort;

  /// Required for local/remote, ignored for dynamic.
  final String? targetHost;
  final int? targetPort;
  final bool autoStart;
  final DateTime createdAt;
  final DateTime updatedAt;

  /// Mirrors `Tunnel::validate`.
  ValidationError? validate() {
    if (name.trim().isEmpty) {
      return const ValidationError('name', 'must not be empty');
    }
    if (bindHost.trim().isEmpty) {
      return const ValidationError('bind_host', 'must not be empty');
    }
    if (bindPort < 1 || bindPort > 65535) {
      return const ValidationError('bind_port', 'must be 1..=65535');
    }
    if (kind.needsTarget) {
      if (targetHost == null || targetHost!.trim().isEmpty) {
        return const ValidationError('target_host', 'required');
      }
      final tp = targetPort;
      if (tp == null || tp < 1 || tp > 65535) {
        return const ValidationError('target_port', 'must be 1..=65535');
      }
    }
    return null;
  }

  /// Mirrors `Tunnel::binds_publicly`: anything but loopback exposes the
  /// tunnel to the network.
  bool get bindsPublicly => bindsPubliclyHost(bindHost);
}

/// Shared rule so editors can warn before a [Tunnel] exists.
bool bindsPubliclyHost(String bindHost) => !const {'127.0.0.1', '::1', 'localhost'}.contains(bindHost.trim());

/// Runtime state of a tunnel (tunnel-core; local only, never synced).
enum TunnelRunState { stopped, starting, running, failed }

final class TunnelRuntime {
  const TunnelRuntime({
    required this.state,
    this.since,
    this.activeConnections = 0,
    this.bytesTransferred = 0,
    this.error,
  });

  static const stopped = TunnelRuntime(state: TunnelRunState.stopped);

  final TunnelRunState state;
  final DateTime? since;
  final int activeConnections;
  final int bytesTransferred;
  final String? error;
}
