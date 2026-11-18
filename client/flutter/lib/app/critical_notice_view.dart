import 'package:consolecrypt/app/critical_notice_models.dart';
import 'package:consolecrypt/app/critical_notice_owner.dart';
import 'package:consolecrypt/app/critical_notice_providers.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/automation/automation_event_view.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/services/job_notice_events.dart';

import 'package:consolecrypt/core/services/runtime_notice_events.dart';

import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/sftp/widgets/sftp_activity.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Fixed read-only section destination. IDs/errors never form navigation URLs.
String criticalRuntimeRoute(RuntimeNoticeDomain domain) => switch (domain) {
  RuntimeNoticeDomain.sync || RuntimeNoticeDomain.security => AppRoutes.sync,
  RuntimeNoticeDomain.tunnel => AppRoutes.tunnels,
  RuntimeNoticeDomain.terminal => AppRoutes.terminal,
  RuntimeNoticeDomain.device => AppRoutes.devices,
  RuntimeNoticeDomain.backup => AppRoutes.backups,
  RuntimeNoticeDomain.team => AppRoutes.sharing,
  RuntimeNoticeDomain.edit || RuntimeNoticeDomain.transfer => AppRoutes.sftp,
};

String criticalNoticeTitle(AppLocalizations l, CriticalNoticeRow row) {
  if (row.job case final job?) {
    final state = job.change.state == JobNoticeState.cancelling
        ? l.noticesCancelling
        : automationJobStateLabel(l, job.change.state.name);
    return l.noticesJobState(state);
  }
  if (row.runtime case final runtime?) {
    final domain = switch (runtime.value.domain) {
      RuntimeNoticeDomain.sync => l.navSync,
      RuntimeNoticeDomain.tunnel => l.navTunnels,
      RuntimeNoticeDomain.terminal => l.navTerminal,
      RuntimeNoticeDomain.security => l.settingsSecurityTitle,
      RuntimeNoticeDomain.device => l.navDevices,
      RuntimeNoticeDomain.backup => l.navBackups,
      RuntimeNoticeDomain.team => l.sharingTitle,
      RuntimeNoticeDomain.edit => l.sftpToolbarEditing,
      RuntimeNoticeDomain.transfer => l.sftpToolbarTransfers,
    };
    final state = switch (runtime.value.state) {
      RuntimeNoticeState.offline => l.noticesRuntimeOffline,
      RuntimeNoticeState.error when runtime.value.domain == RuntimeNoticeDomain.edit => l.noticesLegacyEditError,
      RuntimeNoticeState.error => l.noticesRuntimeError,
      RuntimeNoticeState.stopped => l.noticesRuntimeStopped,
      RuntimeNoticeState.deviceRevoked => l.noticesRuntimeDeviceRevoked,
      RuntimeNoticeState.reauthRequired => l.noticesRuntimeReauthRequired,
      RuntimeNoticeState.accountDisabled => l.noticesRuntimeAccountDisabled,
      RuntimeNoticeState.notTrusted => l.noticesRuntimeNotTrusted,
      RuntimeNoticeState.vaultGone => l.noticesRuntimeVaultGone,
      RuntimeNoticeState.upgradeRequired => l.noticesRuntimeUpgradeRequired,
      RuntimeNoticeState.idle => l.noticesRuntimeIdle,
      RuntimeNoticeState.running => l.noticesRuntimeRunning,
      RuntimeNoticeState.failed when runtime.value.domain == RuntimeNoticeDomain.backup => l.noticesLegacyBackupFailed,
      RuntimeNoticeState.failed when runtime.value.domain == RuntimeNoticeDomain.transfer =>
        l.noticesLegacyTransferFailed,
      RuntimeNoticeState.failed => l.noticesRuntimeFailed,
      RuntimeNoticeState.connected => l.noticesRuntimeConnected,
      RuntimeNoticeState.closed => l.noticesRuntimeClosed,
      RuntimeNoticeState.revoked => l.noticesRuntimeDeviceRevoked,
      RuntimeNoticeState.integrityWarning => l.noticesLegacyIntegrityWarning,
      RuntimeNoticeState.serverRollback => l.noticesLegacyServerRollback,
      RuntimeNoticeState.secretConflict => l.noticesLegacySecretConflict,
      RuntimeNoticeState.conflictCopy => l.noticesLegacyConflictCopy,
      RuntimeNoticeState.registered => l.noticesLegacyDeviceRegistered,
      RuntimeNoticeState.approvalRequested => l.noticesLegacyApprovalRequested,
      RuntimeNoticeState.recoveryChanged => l.noticesLegacyRecoveryChanged,
      RuntimeNoticeState.keyConflict => l.noticesLegacyKeyConflict,
      RuntimeNoticeState.invalidLog => l.noticesLegacyInvalidLog,
      RuntimeNoticeState.logRollback => l.noticesLegacyLogRollback,
      RuntimeNoticeState.removed => l.noticesLegacyTeamRemoved,
      RuntimeNoticeState.conflict => l.noticesLegacyEditConflict,
      RuntimeNoticeState.recoveryAvailable => l.noticesLegacyEditRecovery,
      RuntimeNoticeState.synced => l.noticesLegacyEditSynced,
      RuntimeNoticeState.completed => l.noticesLegacyCompleted,
    };
    return l.noticesRuntimeState(domain, state);
  }
  return l.noticesTitle;
}

GlassTone _tone(CriticalNoticeSeverity value) => switch (value) {
  CriticalNoticeSeverity.info => GlassTone.neutral,
  CriticalNoticeSeverity.success => GlassTone.success,
  CriticalNoticeSeverity.warning => GlassTone.warning,
  CriticalNoticeSeverity.danger => GlassTone.danger,
};

String? _feedWarning(AppLocalizations l, CriticalNoticeSnapshot value) {
  if (value.runtimeUnsupported) return l.noticesRuntimeUnsupported;
  if (value.runtimeFeedUnavailable) return l.noticesRuntimeUnavailable;
  if (value.runtimeIncomplete) return l.noticesRuntimeIncomplete;
  if (value.feedUnavailable) return l.noticesStreamUnavailable;
  if (value.refreshNeeded) return l.noticesRefreshNeeded;
  return null;
}

class CriticalNoticePanel extends StatefulWidget {
  const CriticalNoticePanel({
    super.key,
    required this.owner,
    required this.onOpen,
    this.onOpenRow,
    required this.onClose,
  });
  final CriticalNoticeOwner owner;
  final ValueChanged<String> onOpen;
  final ValueChanged<CriticalNoticeRow>? onOpenRow;
  final VoidCallback onClose;
  @override
  State<CriticalNoticePanel> createState() => _CriticalNoticePanelState();
}

class _CriticalNoticePanelState extends State<CriticalNoticePanel> {
  final details = <String>{};
  final scroll = ScrollController();
  @override
  void didUpdateWidget(CriticalNoticePanel oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.owner != widget.owner) details.clear();
  }

  @override
  void dispose() {
    scroll.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    return StreamBuilder<CriticalNoticeSnapshot>(
      key: ObjectKey(widget.owner),
      stream: widget.owner.changes,
      initialData: widget.owner.state,
      builder: (context, snapshot) {
        final value = snapshot.data ?? widget.owner.state;
        details.retainAll(value.rows.map((row) => row.id));
        return GlassDialog(
          title: l.noticesTitle,
          icon: Icons.notifications_none_rounded,
          width: 760,
          content: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(l.noticesSessionHelp),
              const SizedBox(height: 12),
              Wrap(
                spacing: 8,
                runSpacing: 8,
                children: [
                  GlassButton(
                    key: const ValueKey('critical-notices-refresh'),
                    label: l.noticesRefreshJobs,
                    icon: Icons.refresh,
                    onPressed: value.refreshing || !value.canRefresh ? null : widget.owner.refreshJobs,
                  ),
                  GlassButton(
                    key: const ValueKey('critical-notices-clear'),
                    label: l.noticesClear,
                    onPressed: value.rows.isEmpty ? null : widget.owner.clear,
                  ),
                ],
              ),
              if (value.refreshing) const LinearProgressIndicator(),
              if (value.feedUnavailable || value.refreshNeeded)
                Padding(
                  padding: const EdgeInsets.only(top: 8),
                  child: InfoBanner(
                    key: const ValueKey('critical-notices-gap'),
                    tone: BannerTone.warning,
                    message: value.feedUnavailable ? l.noticesStreamUnavailable : l.noticesRefreshNeeded,
                  ),
                ),
              if (value.runtimeUnsupported || value.runtimeFeedUnavailable || value.runtimeIncomplete)
                Padding(
                  padding: const EdgeInsets.only(top: 8),
                  child: InfoBanner(
                    key: const ValueKey('critical-notices-runtime-gap'),
                    tone: BannerTone.warning,
                    message: value.runtimeUnsupported
                        ? l.noticesRuntimeUnsupported
                        : value.runtimeFeedUnavailable
                        ? l.noticesRuntimeUnavailable
                        : l.noticesRuntimeIncomplete,
                  ),
                ),
              if (value.error case final error?)
                Padding(
                  padding: const EdgeInsets.only(top: 8),
                  child: InfoBanner(
                    key: const ValueKey('critical-notices-error'),
                    tone: BannerTone.danger,
                    message: switch (error) {
                      CriticalRefreshError.failed => l.noticesRefreshFailed,
                      CriticalRefreshError.invalidResponse => l.noticesInvalidResponse,
                      CriticalRefreshError.contextChanged => l.noticesContextChanged,
                    },
                  ),
                ),
              const SizedBox(height: 12),
              if (value.rows.isEmpty) Text(l.noticesEmpty),
              Flexible(
                child: ClipRect(
                  child: ListView.builder(
                    key: const ValueKey('critical-notices-list'),
                    controller: scroll,
                    primary: false,
                    shrinkWrap: true,
                    itemCount: value.rows.length,
                    findChildIndexCallback: (key) {
                      final index = value.rows.indexWhere((row) => ValueKey(row.id) == key);
                      return index < 0 ? null : index;
                    },
                    itemBuilder: (context, index) {
                      final row = value.rows[index];
                      return Padding(
                        key: ValueKey(row.id),
                        padding: const EdgeInsets.only(bottom: 8),
                        child: ContentSurface(
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.stretch,
                            children: [
                              Row(
                                children: [
                                  Icon(
                                    row.runtime != null
                                        ? switch (row.runtime!.value.domain) {
                                            RuntimeNoticeDomain.sync => Icons.sync,
                                            RuntimeNoticeDomain.tunnel => Icons.swap_horiz,
                                            RuntimeNoticeDomain.terminal => Icons.terminal,
                                            RuntimeNoticeDomain.security => Icons.verified_user_outlined,
                                            RuntimeNoticeDomain.device => Icons.devices_outlined,
                                            RuntimeNoticeDomain.backup => Icons.save_outlined,
                                            RuntimeNoticeDomain.team => Icons.groups_outlined,
                                            RuntimeNoticeDomain.edit => Icons.edit_outlined,
                                            RuntimeNoticeDomain.transfer => Icons.swap_vert,
                                          }
                                        : Icons.account_tree_outlined,
                                    size: 20,
                                  ),
                                  const SizedBox(width: 8),
                                  Expanded(
                                    child: Text(
                                      criticalNoticeTitle(l, row),
                                      style: Theme.of(context).textTheme.titleSmall,
                                    ),
                                  ),
                                  if (row.unread)
                                    GlassBadge(label: l.noticesNew, tone: _tone(row.severity), dense: true),
                                ],
                              ),
                              Text(l.noticesObservedUtc(row.observedUtc.toIso8601String())),
                              const SizedBox(height: 8),
                              Wrap(
                                spacing: 8,
                                runSpacing: 4,
                                children: [
                                  GlassButton(
                                    key: ValueKey('critical-notice-open-${row.id}'),
                                    size: GlassControlSize.sm,
                                    icon: Icons.open_in_new,
                                    label: row.runtime != null ? l.commonOpen : l.automationOpenOutput,
                                    onPressed: row.runtime != null && widget.onOpenRow == null
                                        ? null
                                        : () =>
                                              widget.onOpenRow != null ? widget.onOpenRow!(row) : widget.onOpen(row.id),
                                  ),
                                  if (row.unread)
                                    GlassButton(
                                      key: ValueKey('critical-notice-read-${row.id}'),
                                      size: GlassControlSize.sm,
                                      label: l.noticesMarkRead,
                                      onPressed: () => widget.owner.markReadRow(row),
                                    ),
                                  GlassIconButton(
                                    key: ValueKey('critical-notice-dismiss-${row.id}'),
                                    icon: Icons.close,
                                    tooltip: l.noticesDismiss,
                                    onPressed: () => widget.owner.dismissRow(row),
                                  ),
                                ],
                              ),
                              ExpansionTile(
                                key: ValueKey('critical-notice-details-${row.id}'),
                                initiallyExpanded: details.contains(row.id),
                                onExpansionChanged: (open) => setState(() {
                                  if (open) {
                                    details.add(row.id);
                                  } else {
                                    details.remove(row.id);
                                  }
                                }),
                                title: Text(l.noticesDetails),
                                children: [
                                  if (row.job case final job?) ...[
                                    SelectableText('${l.noticesJobId}: ${job.change.jobId}'),
                                    Text('${l.noticesRevision}: ${job.change.revision}'),
                                  ],
                                  if (row.runtime case final runtime?) ...[
                                    SelectableText('${l.noticesRuntimeRecordId}: ${runtime.value.recordId}'),
                                    Text('${l.noticesRevision}: ${runtime.value.revision}'),
                                    Text(l.noticesRuntimeHelp),
                                  ],
                                ],
                              ),
                            ],
                          ),
                        ),
                      );
                    },
                  ),
                ),
              ),
            ],
          ),
          secondaryActions: [
            GlassButton(key: const ValueKey('critical-notices-close'), label: l.commonClose, onPressed: widget.onClose),
          ],
        );
      },
    );
  }
}

Future<void> showCriticalNotices(BuildContext context, WidgetRef ref) async {
  final scope = ref.read(criticalNoticeScopeProvider), owner = ref.read(criticalNoticeOwnerProvider);
  if (owner == null || !owner.current) return;
  await showAppDialog<void>(
    context,
    builder: (dialogContext) => _CriticalNoticeScopeGuard(
      scope: scope,
      child: CriticalNoticePanel(
        owner: owner,
        onClose: () => Navigator.of(dialogContext).pop(),
        onOpen: (id) {
          if (!context.mounted || !owner.current || !identical(ref.read(criticalNoticeScopeProvider), scope)) {
            return;
          }
          final target = ref.read(criticalNoticeOpenIntentProvider.notifier).choose(owner, id);
          if (target == null) return;
          Navigator.of(dialogContext).pop();
          final route = switch (target) {
            CriticalJobTarget() => AppRoutes.automation,
            CriticalRuntimeTarget(:final domain) => criticalRuntimeRoute(domain),
          };
          ref.read(routerProvider).go(route);
        },
        onOpenRow: (row) {
          if (!context.mounted || !owner.current || !identical(ref.read(criticalNoticeScopeProvider), scope)) {
            return;
          }
          final target = ref.read(criticalNoticeOpenIntentProvider.notifier).chooseRow(owner, row);
          if (target == null) return;
          Navigator.of(dialogContext).pop();
          final route = switch (target) {
            CriticalJobTarget() => AppRoutes.automation,
            CriticalRuntimeTarget(:final domain) => criticalRuntimeRoute(domain),
          };
          if (target case CriticalRuntimeTarget(:final domain)) {
            if (domain == RuntimeNoticeDomain.edit) {
              ref.read(sftpActivityProvider.notifier).show(SftpActivityTab.editing);
            } else if (domain == RuntimeNoticeDomain.transfer) {
              ref.read(sftpActivityProvider.notifier).show(SftpActivityTab.transfers);
            }
          }
          ref.read(routerProvider).go(route);
        },
      ),
    ),
  );
}

class _CriticalNoticeScopeGuard extends ConsumerWidget {
  const _CriticalNoticeScopeGuard({required this.scope, required this.child});
  final CriticalNoticeScope scope;
  final Widget child;
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    if (identical(ref.watch(criticalNoticeScopeProvider), scope)) return child;
    final route = ModalRoute.of(context);
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (context.mounted && route != null && route.isActive) {
        Navigator.of(context).removeRoute(route);
      }
    });
    return const SizedBox.shrink();
  }
}

class CriticalNoticeButton extends ConsumerWidget {
  const CriticalNoticeButton({super.key});
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final owner = ref.watch(criticalNoticeOwnerProvider), l = context.l10n;
    if (owner == null) return const SizedBox.shrink();
    return StreamBuilder<CriticalNoticeSnapshot>(
      key: ObjectKey(owner),
      stream: owner.changes,
      initialData: owner.state,
      builder: (context, snapshot) {
        final value = snapshot.data ?? owner.state;
        final severities = value.rows.where((row) => row.unread).map((row) => row.severity.index);
        final severity = severities.fold<int>(0, (current, next) => current > next ? current : next);
        final warning = _feedWarning(l, value);
        return Semantics(
          label: warning ?? l.noticesUnread(value.unread.toString()),
          child: Stack(
            clipBehavior: Clip.none,
            children: [
              GlassIconButton(
                key: const ValueKey('critical-notices-open'),
                icon: Icons.notifications_none_rounded,
                tooltip: warning ?? l.noticesTitle,
                style: GlassIconButtonStyle.plain,
                onPressed: () => showCriticalNotices(context, ref),
              ),
              if (value.unread > 0 || warning != null)
                Positioned(
                  top: -2,
                  right: -2,
                  child: IgnorePointer(
                    child: GlassBadge(
                      label: value.unread > 0 ? value.unread.toString() : '!',
                      tone: value.unread > 0 ? _tone(CriticalNoticeSeverity.values[severity]) : GlassTone.warning,
                      dense: true,
                    ),
                  ),
                ),
            ],
          ),
        );
      },
    );
  }
}

/// Visible only where the normal toolbar is hidden; does not steal focus.
class CriticalNoticeAttentionBar extends ConsumerWidget {
  const CriticalNoticeAttentionBar({super.key});
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final owner = ref.watch(criticalNoticeOwnerProvider), l = context.l10n;
    if (owner == null) return const SizedBox.shrink();
    return StreamBuilder<CriticalNoticeSnapshot>(
      key: ObjectKey(owner),
      stream: owner.changes,
      initialData: owner.state,
      builder: (context, snapshot) {
        final value = snapshot.data ?? owner.state;
        final warning = _feedWarning(l, value);
        if (value.unread == 0 && warning == null) {
          return const SizedBox.shrink();
        }
        return Padding(
          padding: const EdgeInsets.all(4),
          child: InfoBanner(
            key: const ValueKey('critical-notices-attention'),
            tone: BannerTone.warning,
            message: warning ?? l.noticesUnread(value.unread.toString()),
            action: GlassButton(label: l.noticesTitle, onPressed: () => showCriticalNotices(context, ref)),
          ),
        );
      },
    );
  }
}
