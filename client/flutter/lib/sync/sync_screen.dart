import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/shell.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
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
        maxWidth: 820,
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
      maxWidth: 820,
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
                        SyncState.paused => l10n.syncScreenStatePaused,
                        SyncState.localOnly => l10n.syncStateLocalOnly,
                      }),
                    ],
                  ),
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
