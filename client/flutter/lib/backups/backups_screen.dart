import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/l10n/labels.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:material_ui/material_ui.dart';

/// Encrypted backups (ADR-0106): export, restore, scheduled auto-backup.
class BackupsScreen extends ConsumerWidget {
  const BackupsScreen({super.key});

  Future<void> _export(BuildContext context, WidgetRef ref) async {
    final profile = ref.read(activeProfileProvider);
    final stamp = formatIsoDate(DateTime.now());
    final suggested = '${profile?.name ?? 'ConsoleCrypt'}-$stamp.$backupFileExtension';
    final path = await ref
        .read(fileDialogServiceProvider)
        .chooseSaveFile(suggestedName: suggested, extensions: const [backupFileExtension]);
    if (path == null || !context.mounted) return;
    final info = await runWithFeedback(context, () => ref.read(backupServiceProvider).exportBackup(path: path));
    if (info != null && context.mounted) {
      final saved = await runWithFeedback(context, () => ref.read(fileDialogServiceProvider).finishSaveFile(path));
      if (saved != true || !context.mounted) return;
      final l10n = context.l10n;
      showSnack(
        context,
        l10n.backupsSavedSnack(info.fileName, formatBytes(l10n, info.sizeBytes)),
        tone: GlassTone.success,
      );
    }
  }

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final profile = ref.watch(activeProfileProvider);
    final schedule = ref.watch(backupScheduleProvider).value ?? const BackupSchedule();
    final recent = ref.watch(recentBackupsProvider).value ?? const <BackupInfo>[];
    final local = profile?.isLocal ?? false;
    final l10n = context.l10n;
    return PageScaffold(
      title: l10n.backupsTitle,
      subtitle: l10n.backupsSubtitle,
      scrollable: true,
      maxWidth: 900,
      actions: [
        GlassButton(
          onPressed: () => context.go(AppRoutes.restore),
          icon: Icons.settings_backup_restore_rounded,
          label: l10n.backupsRestoreAction,
        ),
        GlassButton(
          key: const ValueKey('export-backup'),
          onPressed: () => _export(context, ref),
          icon: Icons.save_alt_rounded,
          label: l10n.backupsExportAction,
        ),
      ],
      body: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (local)
            InfoBanner(
              tone: BannerTone.warning,
              title: l10n.backupsNoCloudCopyTitle,
              message: l10n.backupsNoCloudCopyMessage,
            )
          else
            InfoBanner(message: l10n.backupsSyncedMessage),
          const SizedBox(height: GlassSpacing.s16),
          _ScheduleCard(schedule: schedule),
          const SizedBox(height: GlassSpacing.s16),
          SectionCard(
            title: l10n.backupsRecentTitle,
            icon: Icons.history_rounded,
            child: recent.isEmpty
                ? Text(l10n.backupsRecentEmpty)
                : Column(
                    children: [
                      for (final b in recent)
                        ListTile(
                          contentPadding: EdgeInsets.zero,
                          leading: Icon(b.automatic ? Icons.schedule_rounded : Icons.save_alt_rounded),
                          title: Text(b.fileName, style: GlassTokens.of(context).typography.mono),
                          subtitle: Text(
                            [
                              formatDateTime(l10n, b.createdAt),
                              l10n.backupsObjectCount(b.objectCount),
                              formatBytes(l10n, b.sizeBytes),
                              if (b.automatic) l10n.backupsAutomatic,
                            ].join(' · '),
                          ),
                        ),
                    ],
                  ),
          ),
        ],
      ),
    );
  }
}

class _ScheduleCard extends ConsumerStatefulWidget {
  const _ScheduleCard({required this.schedule});

  final BackupSchedule schedule;

  @override
  ConsumerState<_ScheduleCard> createState() => _ScheduleCardState();
}

class _ScheduleCardState extends ConsumerState<_ScheduleCard> {
  Future<void> _update(BackupSchedule next) =>
      runWithFeedback(context, () => ref.read(backupServiceProvider).updateSchedule(next));

  Future<void> _chooseFolder() async {
    final folder = await ref.read(fileDialogServiceProvider).chooseDirectory();
    if (folder != null) await _update(widget.schedule.copyWith(folder: folder));
  }

  @override
  Widget build(BuildContext context) {
    if (AppPlatform.isMobile) {
      return SectionCard(
        title: context.l10n.backupsAutoTitle,
        icon: Icons.schedule_rounded,
        child: Text(context.l10n.mobileBackupsHelp),
      );
    }
    final s = widget.schedule;
    final l10n = context.l10n;
    return SectionCard(
      title: l10n.backupsAutoTitle,
      icon: Icons.schedule_rounded,
      subtitle: l10n.backupsAutoSubtitle,
      trailing: Switch.adaptive(
        key: const ValueKey('auto-backup-switch'),
        value: s.enabled,
        onChanged: (v) async {
          if (v && (s.folder ?? '').isEmpty) {
            final folder = await ref.read(fileDialogServiceProvider).chooseDirectory();
            if (folder == null) return;
            await _update(s.copyWith(enabled: true, folder: folder));
          } else {
            await _update(s.copyWith(enabled: v));
          }
        },
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          LabeledValue(
            label: l10n.backupsFolderLabel,
            value: Text(s.folder ?? l10n.backupsFolderNotChosen),
            trailing: GlassButton.plain(
              size: GlassControlSize.sm,
              icon: Icons.folder_open_rounded,
              onPressed: _chooseFolder,
              label: l10n.backupsChooseFolder,
            ),
          ),
          LabeledValue(
            label: l10n.backupsFrequencyLabel,
            value: Align(
              alignment: Alignment.centerLeft,
              child: GlassSegmented<BackupFrequency>(
                key: const ValueKey('backup-frequency'),
                inChrome: false,
                segments: [for (final f in BackupFrequency.values) GlassSegment(value: f, label: f.localized(l10n))],
                selected: s.frequency,
                onChanged: (f) => _update(s.copyWith(frequency: f)),
              ),
            ),
          ),
          LabeledValue(
            label: l10n.backupsKeepLabel,
            value: Align(
              alignment: Alignment.centerLeft,
              child: GlassSelect<int>(
                key: const ValueKey('backup-keep'),
                value: const [3, 7, 14, 30].contains(s.keepLast) ? s.keepLast : 7,
                items: [
                  for (final n in const [3, 7, 14, 30]) GlassSelectItem(value: n, label: l10n.backupsKeepLast(n)),
                ],
                onChanged: (v) => _update(s.copyWith(keepLast: v)),
              ),
            ),
          ),
          LabeledValue(
            label: l10n.backupsLastRunLabel,
            value: Text(s.lastRunAt == null ? l10n.commonNever : formatRelative(l10n, s.lastRunAt!)),
          ),
          if (s.enabled && s.nextRunAt != null)
            LabeledValue(label: l10n.backupsNextRunLabel, value: Text(formatRemaining(l10n, s.nextRunAt!))),
          // The scheduler's diagnostic is English; show it under a localized title.
          if (s.lastError != null)
            InfoBanner(tone: BannerTone.danger, title: l10n.backupsLastRunFailed, message: s.lastError!),
          const SizedBox(height: GlassSpacing.s8),
          Align(
            alignment: AlignmentDirectional.centerStart,
            child: GlassButton(
              onPressed: (s.folder ?? '').isEmpty
                  ? null
                  : () => runWithFeedback(
                      context,
                      () => ref.read(backupServiceProvider).backupNow(),
                      success: l10n.backupsWrittenTo(s.folder ?? ''),
                    ),
              icon: Icons.play_arrow_rounded,
              label: l10n.backupsBackUpNow,
            ),
          ),
          // TODO(client/ui): the scheduler itself runs in app-core (timer + retention) so it
          // works while the UI is closed to tray; next: expose next-run events in M4.
        ],
      ),
    );
  }
}
