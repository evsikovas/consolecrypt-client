import 'package:consolecrypt/account/enterprise_sign_in.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/shell.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/sync_service.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/sync/enable_sync_dialog.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:material_ui/material_ui.dart';

class SyncScreen extends ConsumerWidget {
  const SyncScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final status = ref.watch(syncStatusProvider).value ?? SyncStatus.paused;
    final profile = ref.watch(activeProfileProvider);
    final dev = ref.watch(developerControlsProvider);
    final tokens = GlassTokens.of(context);
    final l10n = context.l10n;
    final presentation = describeSyncStatus(status, l10n);
    final label = presentation.label;
    final icon = presentation.icon ?? Icons.cloud_done_rounded;
    final color = presentation.tone == GlassTone.neutral
        ? tokens.secondaryLabel
        : tokens.palette.tone(presentation.tone);

    if (status.state == SyncState.localOnly) {
      return PageScaffold(
        title: l10n.syncScreenTitle,
        scrollable: true,
        body: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            SectionCard(
              title: l10n.syncStateLocalOnly,
              icon: Icons.laptop_mac_rounded,
              subtitle: l10n.syncScreenLocalSubtitle,
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Text(l10n.syncScreenLocalBody),
                  const SizedBox(height: 12),
                  Wrap(
                    spacing: GlassSpacing.s8,
                    runSpacing: GlassSpacing.s8,
                    children: [
                      GlassButton.prominent(
                        key: const ValueKey('enable-sync'),
                        onPressed: () => showEnableSyncWizard(context),
                        icon: Icons.cloud_upload_rounded,
                        label: l10n.syncScreenEnableSync,
                      ),
                      GlassButton(
                        onPressed: () => context.go(AppRoutes.backups),
                        icon: Icons.save_alt_rounded,
                        label: l10n.syncScreenBackups,
                      ),
                    ],
                  ),
                ],
              ),
            ),
          ],
        ),
      );
    }

    return PageScaffold(
      title: l10n.syncScreenTitle,
      subtitle: profile?.serverUrl == null ? null : l10n.syncScreenServer('${profile!.serverUrl}'),
      scrollable: true,
      actions: [
        GlassButton.prominent(
          key: const ValueKey('sync-now'),
          onPressed: status.state == SyncState.syncing
              ? null
              : () => runWithFeedback(context, () => ref.read(syncServiceProvider).syncNow()),
          icon: Icons.sync_rounded,
          label: l10n.syncScreenSyncNow,
          busy: status.state == SyncState.syncing,
          busyLabel: l10n.syncStateSyncing,
        ),
      ],
      body: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          SectionCard(
            title: label,
            icon: icon,
            child: Column(
              children: [
                LabeledValue(
                  label: l10n.syncScreenState,
                  value: Row(
                    children: [
                      Icon(icon, color: color, size: 18),
                      const SizedBox(width: GlassSpacing.s6),
                      Text(switch (status.state) {
                        SyncState.idle => l10n.syncScreenStateIdle,
                        SyncState.syncing => l10n.syncScreenStateSyncing,
                        SyncState.offline => l10n.syncScreenStateOffline,
                        SyncState.error => l10n.syncScreenStateError,
                        SyncState.throttled => l10n.syncScreenStateThrottled,
                        SyncState.paused => l10n.syncScreenStatePaused,
                        SyncState.localOnly => l10n.syncStateLocalOnly,
                      }),
                    ],
                  ),
                ),
                if (profile != null)
                  LabeledValue(
                    key: const ValueKey('sync-auth-row'),
                    label: l10n.settingsAuthLabel,
                    value: AuthorizationSummary(profile: profile, showHelp: false),
                  ),
                LabeledValue(
                  label: l10n.syncScreenLastSync,
                  value: Text(
                    status.lastSyncAt == null
                        ? l10n.commonNever
                        : '${formatDateTime(l10n, status.lastSyncAt!)} (${formatRelative(l10n, status.lastSyncAt!)})',
                  ),
                ),
                LabeledValue(
                  label: l10n.syncScreenPendingChanges,
                  value: Text('${status.pendingChanges}', key: const ValueKey('pending-changes')),
                ),
                if (status.lastServerSequence != null)
                  LabeledValue(label: l10n.syncScreenServerSequence, value: Text('${status.lastServerSequence}')),
                if (status.nextRetryAt != null)
                  LabeledValue(
                    label: l10n.syncScreenNextRetry,
                    value: Text(formatRemaining(l10n, status.nextRetryAt!)),
                  ),
              ],
            ),
          ),
          if (status.isOffline) ...[
            const SizedBox(height: GlassSpacing.s16),
            InfoBanner(tone: BannerTone.warning, title: l10n.syncStateOffline, message: l10n.syncScreenOfflineMessage),
          ],
          if (status.state == SyncState.throttled) ...[
            const SizedBox(height: GlassSpacing.s16),
            InfoBanner(
              key: const ValueKey('sync-throttled'),
              tone: BannerTone.info,
              title: label,
              message: l10n.syncScreenThrottledBody,
            ),
          ],
          if (ref.watch(vaultDeletionProvider).value case final DateTime at) ...[
            const SizedBox(height: GlassSpacing.s16),
            _PendingDeletionCard(at: at),
          ],
          if (status.failedChanges > 0) ...[
            const SizedBox(height: GlassSpacing.s16),
            _RejectedChangesCard(status: status),
          ],
          if (status.issues.isNotEmpty) ...[
            const SizedBox(height: GlassSpacing.s16),
            SectionCard(
              title: l10n.syncScreenProblems,
              icon: Icons.report_problem_rounded,
              child: Column(
                children: [
                  for (final issue in status.issues)
                    ListTile(
                      contentPadding: EdgeInsets.zero,
                      leading: Icon(Icons.error_rounded, color: tokens.palette.danger),
                      // The issue text is an English diagnostic: shown as secondary detail only.
                      title: Text(issue.retryable ? l10n.syncScreenIssueRetryable : l10n.syncScreenIssue),
                      subtitle: Text('${formatDateTime(l10n, issue.at)} · ${issue.message}'),
                    ),
                ],
              ),
            ),
          ],
          const SizedBox(height: GlassSpacing.s16),
          SectionCard(
            title: l10n.syncScreenDisconnectTitle,
            icon: Icons.link_off_rounded,
            child: Row(
              children: [
                Expanded(child: Text(l10n.syncScreenDisconnectBody)),
                const SizedBox(width: GlassSpacing.s12),
                GlassButton(
                  key: const ValueKey('disconnect-sync'),
                  style: GlassButtonStyle.destructiveQuiet,
                  onPressed: () => showDisconnectDialog(context, ref),
                  label: l10n.syncScreenDisconnect,
                ),
              ],
            ),
          ),
          if (dev != null) ...[
            const SizedBox(height: GlassSpacing.s16),
            SectionCard(
              title: l10n.syncScreenDeveloper,
              icon: Icons.developer_mode_rounded,
              child: SwitchListTile.adaptive(
                key: const ValueKey('simulate-offline'),
                contentPadding: EdgeInsets.zero,
                title: Text(l10n.syncScreenSimulateOutage),
                value: dev.simulatedOffline,
                onChanged: dev.setSimulatedOffline,
              ),
            ),
          ],
        ],
      ),
    );
  }
}

/// Changes the server rejected for good (outbox `failed`), e.g. over the
/// vault object limit (protocol 1.11): what happened, and «Retry» after the
/// user cleaned up.
class _RejectedChangesCard extends ConsumerWidget {
  const _RejectedChangesCard({required this.status});

  final SyncStatus status;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = context.l10n;
    final quota = status.pushQuota;
    final limit = quota?.limit;
    final message = switch (quota?.reason) {
      SyncQuota.objectLimitReason when limit != null => l10n.errorObjectLimit(formatCount(l10n, limit)),
      SyncQuota.vaultQuotaReason when limit != null => l10n.errorVaultQuota(formatBytes(l10n, limit)),
      _ => l10n.syncScreenRejectedBody(status.failedChanges),
    };
    return SectionCard(
      key: const ValueKey('sync-rejected'),
      title: l10n.syncScreenRejectedTitle,
      icon: Icons.cloud_off_rounded,
      child: Row(
        children: [
          Expanded(child: Text(message, key: const ValueKey('sync-rejected-message'))),
          const SizedBox(width: GlassSpacing.s12),
          GlassButton(
            key: const ValueKey('sync-retry-failed'),
            icon: Icons.refresh_rounded,
            label: l10n.commonRetry,
            onPressed: () => runWithFeedback(context, () => ref.read(syncServiceProvider).retryFailed()),
          ),
        ],
      ),
    );
  }
}

/// Another, not yet trusted device of the account scheduled the deletion of
/// this vault (protocol 1.13): when, and «Keep the vault» from this device.
class _PendingDeletionCard extends ConsumerWidget {
  const _PendingDeletionCard({required this.at});

  final DateTime at;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = context.l10n;
    return InfoBanner(
      key: const ValueKey('sync-pending-deletion'),
      tone: BannerTone.danger,
      title: l10n.syncVaultDeletionTitle(formatDateTime(l10n, at)),
      message: l10n.syncVaultDeletionBody,
      action: GlassButton.prominent(
        key: const ValueKey('sync-cancel-deletion'),
        icon: Icons.restore_rounded,
        label: l10n.syncVaultDeletionCancel,
        onPressed: () => runWithFeedback(context, () async {
          final service = ref.read(syncServiceProvider);
          if (service is VaultDeletionControl) await (service as VaultDeletionControl).cancelVaultDeletion();
          ref.invalidate(vaultDeletionProvider);
        }, success: l10n.syncVaultDeletionCancelled),
      ),
    );
  }
}
