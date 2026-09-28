import 'package:consolecrypt/core/models/ids.dart';

/// Local handle of a profile (one local database + one vault, ADR-0106).
extension type const ProfileId(String value) implements Object {
  factory ProfileId.generate() => ProfileId(generateUuidV4());
}

/// Mirrors `ProfileKind` (ADR-0106).
enum ProfileKind {
  /// No server, no account, no network I/O for sync. First-class mode.
  local,

  /// Synced E2EE with a self-hosted server.
  synced,
}

final class Profile {
  const Profile({
    required this.id,
    required this.name,
    required this.kind,
    required this.createdAt,
    this.serverUrl,
    this.accountEmail,
    this.deviceId,
    this.vaultId,
    this.lastUsedAt,
  });

  final ProfileId id;

  /// User-visible, e.g. "Personal" or "Work".
  final String name;
  final ProfileKind kind;

  /// Synced only.
  final Uri? serverUrl;
  final String? accountEmail;
  final DeviceId? deviceId;

  /// Client-generated; stays the same when a local vault is later synced.
  final VaultId? vaultId;
  final DateTime createdAt;
  final DateTime? lastUsedAt;

  bool get isLocal => kind == ProfileKind.local;

  bool get isSynced => kind == ProfileKind.synced;
}

/// Profiles on this installation; at most one active.
final class ProfilesState {
  const ProfilesState({required this.profiles, this.activeId});

  static const empty = ProfilesState(profiles: []);

  final List<Profile> profiles;
  final ProfileId? activeId;

  Profile? get active {
    final id = activeId;
    if (id == null) return null;
    for (final p in profiles) {
      if (p.id == id) return p;
    }
    return null;
  }
}
