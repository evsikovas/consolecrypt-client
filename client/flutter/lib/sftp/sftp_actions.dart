import 'dart:async';
import 'dart:io';

import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/sftp.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/sftp/dialogs/get_info_dialog.dart';
import 'package:consolecrypt/sftp/dialogs/quick_look_dialog.dart';
import 'package:consolecrypt/sftp/edit_flows.dart';
import 'package:consolecrypt/sftp/sftp_controller.dart';
import 'package:consolecrypt/sftp/widgets/sftp_activity.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Context-menu / Action-menu commands (SFTP_BROWSER_SPEC §1).
enum SftpAction {
  open,
  openWith,
  quickLook,
  download,
  uploadHere,
  newFolder,
  newFile,
  rename,
  duplicate,
  delete,
  getInfo,
  copyPath,
  copyName,
  refresh,
}

/// Groups of the menu (separated by dividers).
const sftpActionGroups = [
  [SftpAction.open, SftpAction.openWith, SftpAction.quickLook],
  [SftpAction.download, SftpAction.uploadHere],
  [SftpAction.newFolder, SftpAction.newFile],
  [SftpAction.rename, SftpAction.duplicate, SftpAction.delete],
  [SftpAction.getInfo, SftpAction.copyPath, SftpAction.copyName],
  [SftpAction.refresh],
];

extension SftpActionUi on SftpAction {
  String label(AppLocalizations l) => switch (this) {
    SftpAction.open => l.commonOpen,
    SftpAction.openWith => l.sftpActionOpenWith,
    SftpAction.quickLook => l.sftpToolbarQuickLook,
    SftpAction.download => l.sftpActionDownload,
    SftpAction.uploadHere => l.sftpActionUploadHere,
    SftpAction.newFolder => l.sftpNewFolder,
    SftpAction.newFile => l.sftpActionNewFile,
    SftpAction.rename => l.commonRename,
    SftpAction.duplicate => l.sftpActionDuplicate,
    SftpAction.delete => l.commonDelete,
    SftpAction.getInfo => l.sftpActionGetInfo,
    SftpAction.copyPath => l.sftpCopyPath,
    SftpAction.copyName => l.sftpActionCopyName,
    SftpAction.refresh => l.sftpRefresh,
  };

  IconData get icon => switch (this) {
    SftpAction.open => Icons.open_in_new_rounded,
    SftpAction.openWith => Icons.apps_rounded,
    SftpAction.quickLook => Icons.visibility_rounded,
    SftpAction.download => Icons.download_rounded,
    SftpAction.uploadHere => Icons.upload_rounded,
    SftpAction.newFolder => Icons.create_new_folder_rounded,
    SftpAction.newFile => Icons.note_add_rounded,
    SftpAction.rename => Icons.drive_file_rename_outline_rounded,
    SftpAction.duplicate => Icons.copy_all_rounded,
    SftpAction.delete => Icons.delete_rounded,
    SftpAction.getInfo => Icons.info_outline_rounded,
    SftpAction.copyPath => Icons.route_rounded,
    SftpAction.copyName => Icons.content_copy_rounded,
    SftpAction.refresh => Icons.refresh_rounded,
  };

  /// Shortcut hint shown in menus (handled by the file list).
  SingleActivator? get shortcut => switch (this) {
    SftpAction.open => const SingleActivator(LogicalKeyboardKey.enter),
    SftpAction.quickLook => const SingleActivator(LogicalKeyboardKey.space),
    SftpAction.newFolder => AppPlatform.primary(LogicalKeyboardKey.keyN, shift: true),
    SftpAction.rename => const SingleActivator(LogicalKeyboardKey.f2),
    SftpAction.duplicate => AppPlatform.primary(LogicalKeyboardKey.keyD),
    SftpAction.delete => SingleActivator(
      AppPlatform.usesMeta ? LogicalKeyboardKey.backspace : LogicalKeyboardKey.delete,
      meta: AppPlatform.usesMeta,
    ),
    SftpAction.getInfo => AppPlatform.primary(LogicalKeyboardKey.keyI),
    SftpAction.refresh => AppPlatform.primary(LogicalKeyboardKey.keyR),
    _ => null,
  };

  /// Menu label of [shortcut]: "⇧⌘N" / "↩" / "⌘⌫" on macOS, "Ctrl+Shift+N" /
  /// "Enter" / "Del" elsewhere; key names come from [MaterialLocalizations].
  String? shortcutLabel(MaterialLocalizations m) {
    final activator = shortcut;
    if (activator == null) return null;
    final mac = AppPlatform.usesMeta;
    final key = switch (activator.trigger) {
      LogicalKeyboardKey.enter => mac ? '↩' : 'Enter',
      LogicalKeyboardKey.space => m.keyboardKeySpace,
      LogicalKeyboardKey.backspace => mac ? '⌫' : m.keyboardKeyBackspace,
      LogicalKeyboardKey.delete => mac ? '⌦' : m.keyboardKeyDelete,
      final other => other.keyLabel.toUpperCase(),
    };
    if (mac) return '${activator.shift ? '⇧' : ''}${activator.meta ? '⌘' : ''}$key';
    return [if (activator.control) m.keyboardKeyControl, if (activator.shift) m.keyboardKeyShift, key].join('+');
  }
}

/// Runs [SftpAction]s against the current selection.
final class SftpActions {
  SftpActions(this.context, this.ref);

  final BuildContext context;
  final WidgetRef ref;

  SftpController get _controller => ref.read(sftpControllerProvider.notifier);

  SftpState get _state => ref.read(sftpControllerProvider);

  static bool isEnabled(SftpAction action, List<RemoteFileInfo> sel) {
    final one = sel.length == 1;
    return switch (action) {
      SftpAction.open => sel.isNotEmpty && (sel.every((e) => !e.isDirectory) || one),
      SftpAction.openWith => one && !sel.first.isDirectory,
      SftpAction.quickLook || SftpAction.rename || SftpAction.getInfo => one,
      SftpAction.download => AppPlatform.isMobile ? one && !sel.first.isDirectory : sel.isNotEmpty,
      SftpAction.duplicate || SftpAction.delete || SftpAction.copyName => sel.isNotEmpty,
      SftpAction.uploadHere ||
      SftpAction.newFolder ||
      SftpAction.newFile ||
      SftpAction.copyPath ||
      SftpAction.refresh => true,
    };
  }

  /// Glass menu entries for [sel] (toolbar Action menu and context menu):
  /// the [sftpActionGroups] separated by dividers, with icons and shortcut
  /// hints; unavailable actions are disabled, Delete is destructive.
  List<GlassMenuEntry<SftpAction>> glassMenuEntries(List<RemoteFileInfo> sel) {
    final l10n = context.l10n;
    final keys = MaterialLocalizations.of(context);
    return [
      for (var g = 0; g < sftpActionGroups.length; g++) ...[
        if (g > 0) const GlassMenuDivider<SftpAction>(),
        for (final a in sftpActionGroups[g])
          if (!AppPlatform.isMobile || a != SftpAction.openWith)
            GlassMenuItem<SftpAction>(
              key: ValueKey('sftp-action-${a.name}'),
              value: a,
              label: a.label(l10n),
              icon: a.icon,
              shortcut: AppPlatform.isMobile ? null : a.shortcutLabel(keys),
              destructive: a == SftpAction.delete,
              enabled: isEnabled(a, sel),
            ),
      ],
    ];
  }

  /// Runs a menu choice (ignores `null` = dismissed).
  void runChoice(SftpAction? action, List<RemoteFileInfo> sel) {
    if (action != null && isEnabled(action, sel)) unawaited(run(action, sel));
  }

  Future<void> run(SftpAction action, List<RemoteFileInfo> sel) async {
    final l10n = context.l10n;
    final activity = ref.read(sftpActivityProvider.notifier);
    switch (action) {
      case SftpAction.open:
        if (sel.length == 1 && sel.first.isDirectory) {
          await _controller.navigateTo(sel.first.path);
        } else {
          for (final e in sel.where((e) => !e.isDirectory).take(10)) {
            if (!context.mounted) return;
            if (AppPlatform.isMobile) {
              await showQuickLook(context, e);
            } else {
              await openInEditor(context, ref, e);
            }
          }
        }
      case SftpAction.openWith:
        await openInEditor(context, ref, sel.first, openWith: const OpenWithChoose());
      case SftpAction.quickLook:
        await showQuickLook(
          context,
          sel.first,
          onOpenInEditor: AppPlatform.isMobile ? null : (e) => unawaited(openInEditor(context, ref, e)),
        );
      case SftpAction.download:
        if (AppPlatform.isMobile) {
          await _downloadToDocument(sel.single);
          return;
        }
        final dir = await ref.read(fileDialogServiceProvider).chooseDirectory();
        if (dir == null || !context.mounted) return;
        final ids = await runWithFeedback(context, () => _controller.download([for (final e in sel) e.path], dir));
        if (ids != null && context.mounted) {
          activity.show(SftpActivityTab.transfers);
          showSnack(context, l10n.sftpDownloadStarted(ids.length, dir));
        }
      case SftpAction.uploadHere:
        final target = sel.length == 1 && sel.first.isDirectory ? sel.first.path : _state.remotePath;
        final file = await ref.read(fileDialogServiceProvider).chooseOpenFile();
        if (file == null || !context.mounted) return;
        await uploadPaths(context, ref, [file], target);
      case SftpAction.newFolder:
        await runWithFeedback(context, () => _controller.createFolder(l10n.sftpUntitledFolder));
      case SftpAction.newFile:
        await runWithFeedback(context, () => _controller.createFile(l10n.sftpUntitledFile));
      case SftpAction.rename:
        if (AppPlatform.isMobile) {
          final name = await showTextInputDialog(context, title: l10n.commonRename, initial: sel.first.name);
          if (name != null && context.mounted) {
            await runWithFeedback(context, () => _controller.commitRename(sel.first.path, name));
          }
        } else {
          _controller.startRename(sel.first.path);
        }
      case SftpAction.duplicate:
        await runWithFeedback(
          context,
          () => _controller.duplicate([for (final e in sel) e.path], l10n.sftpDuplicateSuffix),
        );
      case SftpAction.delete:
        await confirmAndDelete(context, ref, sel);
      case SftpAction.getInfo:
        await showGetInfoDialog(context, sel.first);
      case SftpAction.copyPath:
        final text = sel.isEmpty ? _state.remotePath : sel.map((e) => e.path).join('\n');
        await copyPlainWithNotice(context, ref, text, what: l10n.sftpCopyWhatPath);
      case SftpAction.copyName:
        await copyPlainWithNotice(context, ref, sel.map((e) => e.name).join('\n'), what: l10n.sftpCopyWhatName);
      case SftpAction.refresh:
        await _controller.refresh();
    }
  }

  Future<void> _downloadToDocument(RemoteFileInfo entry) async {
    final dialogs = ref.read(fileDialogServiceProvider);
    final service = ref.read(sftpServiceProvider);
    final session = _state.session;
    final path = await dialogs.chooseSaveFile(suggestedName: entry.name);
    if (path == null || !context.mounted || _state.session != session) return;
    await runWithFeedback(context, () async {
      final ids = await _controller.download([entry.path], File(path).parent.path);
      if (!context.mounted || ids.isEmpty) return;
      ref.read(sftpActivityProvider.notifier).show(SftpActivityTab.transfers);
      final jobs = await service.watchTransfers().firstWhere(
        (jobs) => jobs.any((job) => job.id == ids.single && !job.isActive),
      );
      final job = jobs.firstWhere((job) => job.id == ids.single);
      // Transfer errors are already shown in the activity panel. Only export
      // a completed file, and never reopen a picker after changing sessions.
      if (job.state != TransferState.completed || !context.mounted || _state.session != session) return;
      try {
        await dialogs.finishSaveFile(job.destinationPath);
      } finally {
        // SFTP staging is temporary; encrypted backups keep their own history.
        if (File(job.destinationPath).existsSync()) await File(job.destinationPath).delete();
      }
    });
  }
}

/// Delete with confirmation (names the item, or the count).
Future<void> confirmAndDelete(BuildContext context, WidgetRef ref, List<RemoteFileInfo> sel) async {
  if (sel.isEmpty) return;
  final l10n = context.l10n;
  final single = sel.length == 1 ? sel.first : null;
  final ok = await showConfirmDialog(
    context,
    title: single == null ? l10n.sftpDeleteManyTitle(sel.length) : l10n.sftpDeleteTitle(single.name),
    message: single == null
        ? l10n.sftpDeleteManyMessage
        : (single.kind == RemoteEntryKind.directory ? l10n.sftpDeleteFolderMessage : l10n.sftpDeleteFileMessage),
    confirmLabel: l10n.commonDelete,
    destructive: true,
  );
  if (!ok || !context.mounted) return;
  await runWithFeedback(context, () => ref.read(sftpControllerProvider.notifier).delete(sel));
}

/// Uploads local files/folders (dialog, drop from Finder/Explorer, local pane).
Future<void> uploadPaths(BuildContext context, WidgetRef ref, List<String> paths, String remoteDirectory) async {
  if (paths.isEmpty) return;
  final l10n = context.l10n;
  try {
    final ids = await ref.read(sftpControllerProvider.notifier).upload(paths, remoteDirectory);
    if (!context.mounted) return;
    ref.read(sftpActivityProvider.notifier).show(SftpActivityTab.transfers);
    showSnack(context, l10n.sftpUploadStarted(ids.length, remoteDirectory));
  } on AppException catch (e) {
    if (context.mounted) showSnack(context, errorMessage(l10n, e), error: true);
  }
}
