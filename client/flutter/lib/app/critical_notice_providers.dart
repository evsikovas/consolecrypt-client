import 'dart:async';

import 'package:consolecrypt/app/critical_notice_owner.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/job_notice_events.dart';
import 'package:consolecrypt/core/services/runtime_notice_adapter.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

final class CriticalNoticeScope {
  const CriticalNoticeScope(this.profileId, this.unlocked);
  final String? profileId;
  final bool unlocked;
}

final criticalNoticeScopeProvider = Provider<CriticalNoticeScope>(
  (ref) => CriticalNoticeScope(
    ref.watch(activeProfileProvider.select((profile) => profile?.id.value)),
    ref.watch(appStageProvider) == AppStage.unlocked,
  ),
);
final criticalJobEventsProvider = Provider<JobNoticeEvents?>((ref) => ref.watch(appServicesProvider).jobNotices);
final criticalRuntimeEventsProvider = Provider<RuntimeNoticeEvents?>(
  (ref) => ref.watch(appServicesProvider).runtimeNotices,
);
final criticalNoticeOwnerProvider = Provider<CriticalNoticeOwner?>((ref) {
  final scope = ref.watch(criticalNoticeScopeProvider);
  final events = ref.watch(criticalJobEventsProvider);
  final jobs = ref.watch(automationJobServiceProvider);
  if (!scope.unlocked || scope.profileId == null) return null;
  final owner = CriticalNoticeOwner(
    scope: scope,
    profileId: scope.profileId!,
    jobs: jobs,
    events: events,
    runtimeEvents: ref.watch(criticalRuntimeEventsProvider),
    isCurrent: () => identical(ref.read(criticalNoticeScopeProvider), scope),
  );
  ref.onDispose(() => unawaited(owner.dispose()));
  return owner;
});

/// A one-shot read-only navigation intent. It carries no manifest/review/token,
/// does not grant capability and never calls prepare/confirm/retry/cancel.
final class CriticalNoticeOpenIntent {
  const CriticalNoticeOpenIntent({required this.owner, required this.epoch, required this.rowId, required this.target});
  final CriticalNoticeOwner owner;
  final int? epoch;
  final String rowId;
  final CriticalNoticeTarget target;
  bool get current => owner.current && owner.ring.epoch == epoch && owner.target(rowId) != null;
}

final criticalNoticeOpenIntentProvider = NotifierProvider<CriticalNoticeOpenIntents, CriticalNoticeOpenIntent?>(
  CriticalNoticeOpenIntents.new,
);

final class CriticalNoticeOpenIntents extends Notifier<CriticalNoticeOpenIntent?> {
  @override
  CriticalNoticeOpenIntent? build() {
    ref.watch(criticalNoticeScopeProvider);
    return null;
  }

  CriticalNoticeTarget? choose(CriticalNoticeOwner owner, String rowId) {
    if (!owner.current) return null;
    final target = owner.target(rowId);
    if (target == null || target is CriticalRuntimeTarget) return null;
    state = CriticalNoticeOpenIntent(owner: owner, epoch: owner.ring.epoch, rowId: rowId, target: target);
    owner.markRead(rowId);
    return target;
  }

  CriticalNoticeTarget? chooseRow(CriticalNoticeOwner owner, CriticalNoticeRow row) {
    if (row.runtime == null) return choose(owner, row.id);
    if (!owner.rowCurrent(row)) return null;
    // Runtime notices navigate only to a section; no destination operation or
    // deferred one-shot command is stored. Opening never reconnects/retries.
    state = null;
    owner.markReadRow(row);
    return row.target;
  }

  CriticalNoticeTarget? take(CriticalNoticeOpenIntent intent) {
    if (!identical(state, intent)) return null;
    state = null;
    return intent.current ? intent.target : null;
  }
}
