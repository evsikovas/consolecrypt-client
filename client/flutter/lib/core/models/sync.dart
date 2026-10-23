/// Coarse sync engine state (sync-core outbox worker).
enum SyncState {
  /// Up to date with the server.
  idle,

  /// Push/pull in progress.
  syncing,

  /// Server unreachable. Local hosts, keys and SSH keep working (DoD 20);
  /// changes queue in the outbox.
  offline,

  /// Last attempt failed (auth, conflict, server error). Retries with backoff.
  error,

  /// The server temporarily limited this account's requests (`429` with a
  /// Retry-After over [syncThrottleNoticeAfter]); sync resumes by itself at
  /// [SyncStatus.nextRetryAt]. Nothing is lost.
  throttled,

  /// Vault locked or signed out: nothing to sync.
  paused,

  /// Local-only profile (ADR-0106): no server, nothing to sync.
  localOnly,
}

/// A rate-limit pause longer than this is shown as "resuming at HH:MM"
/// instead of a sync error.
const syncThrottleNoticeAfter = Duration(seconds: 60);

/// A vault quota the server refused pushes for (protocol 1.11).
final class SyncQuota {
  const SyncQuota({required this.reason, this.limit});

  static const objectLimitReason = 'object_limit';
  static const vaultQuotaReason = 'vault_quota';

  /// `object_limit` | `vault_quota` (unknown values pass through).
  final String reason;

  /// Objects (`object_limit`) or bytes (`vault_quota`).
  final int? limit;

  @override
  bool operator ==(Object other) => other is SyncQuota && other.reason == reason && other.limit == limit;

  @override
  int get hashCode => Object.hash(reason, limit);
}

/// One problem reported by the sync engine.
final class SyncIssue {
  const SyncIssue({required this.at, required this.message, this.retryable = true});

  final DateTime at;
  final String message;
  final bool retryable;
}

/// Sync status stream item (`SyncService.watchStatus`).
final class SyncStatus {
  const SyncStatus({
    required this.state,
    this.pendingChanges = 0,
    this.failedChanges = 0,
    this.pushQuota,
    this.lastSyncAt,
    this.lastServerSequence,
    this.nextRetryAt,
    this.issues = const [],
  });

  static const paused = SyncStatus(state: SyncState.paused);

  static const localOnly = SyncStatus(state: SyncState.localOnly);

  final SyncState state;

  /// Mutations in the local outbox not yet acknowledged by the server.
  final int pendingChanges;

  /// Changes the server rejected for good (outbox `failed`); "Retry" puts
  /// them back into the queue.
  final int failedChanges;

  /// Why pushes were rejected, when it was a vault quota.
  final SyncQuota? pushQuota;
  final DateTime? lastSyncAt;

  /// `last_sync_seq` (ADR-0003).
  final int? lastServerSequence;
  final DateTime? nextRetryAt;
  final List<SyncIssue> issues;

  bool get isOffline => state == SyncState.offline;

  SyncStatus copyWith({
    SyncState? state,
    int? pendingChanges,
    int? failedChanges,
    SyncQuota? pushQuota,
    DateTime? lastSyncAt,
    int? lastServerSequence,
    DateTime? nextRetryAt,
    bool clearNextRetry = false,
    List<SyncIssue>? issues,
  }) => SyncStatus(
    state: state ?? this.state,
    pendingChanges: pendingChanges ?? this.pendingChanges,
    failedChanges: failedChanges ?? this.failedChanges,
    pushQuota: pushQuota ?? this.pushQuota,
    lastSyncAt: lastSyncAt ?? this.lastSyncAt,
    lastServerSequence: lastServerSequence ?? this.lastServerSequence,
    nextRetryAt: clearNextRetry ? null : (nextRetryAt ?? this.nextRetryAt),
    issues: issues ?? this.issues,
  );
}

/// Steps of the "Enable sync" wizard (Local → Synced, ADR-0106).
enum EnableSyncStep {
  authenticating,
  creatingRemoteVault,

  /// The vault already exists on the server — merging.
  reconnecting,
  uploading,
  finishing,
  done,
  failed,
}

final class EnableSyncProgress {
  const EnableSyncProgress(this.step, {this.uploaded = 0, this.total = 0, this.message, this.error});

  final EnableSyncStep step;
  final int uploaded;
  final int total;

  /// Error text for `failed` (English diagnostic).
  final String? message;

  /// The service error behind `failed`, for a localized message.
  final Object? error;

  double? get fraction => total == 0 ? null : uploaded / total;
}
