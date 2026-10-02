import 'dart:async';

import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/sftp/dialogs/edit_dialogs.dart';
import 'package:consolecrypt/sftp/sftp_controller.dart';
import 'package:consolecrypt/sftp/sftp_providers.dart';
import 'package:consolecrypt/sftp/widgets/sftp_activity.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// "Open" / "Open With…": first-use hint, size check, then an edit session
/// (SFTP_BROWSER_SPEC §2).
Future<void> openInEditor(
  BuildContext context,
  WidgetRef ref,
  RemoteFileInfo entry, {
  OpenWith openWith = const OpenWithDefault(),
}) async {
  if (entry.isDirectory) return;
  final l10n = context.l10n;
  final state = ref.read(sftpControllerProvider);
  final session = state.session;
  final profileId = ref.read(activeProfileProvider)?.id;
  if (session == null) return;
  const limit = SftpBrowserService.maxEditFileSize;
  if (!entry.isSymlink && entry.size > limit) {
    showSnack(context, l10n.sftpEditTooLarge(entry.name, formatBytes(l10n, entry.size), formatBytes(l10n, limit)));
    return;
  }
  if (!state.prefs.editHintAcknowledged) {
    final hint = await showEditHintDialog(context, entry.name);
    if (hint == null || !context.mounted) return;
    if (hint.dontShowAgain) ref.read(sftpControllerProvider.notifier).acknowledgeEditHint();
  }
  try {
    if (openWith is OpenWithDefault) {
      final editor = ref.read(settingsServiceProvider).currentLocal.sftpDefaultEditor;
      if (editor != null) openWith = OpenWithApp(editor);
    }
    if (openWith is OpenWithChoose && (AppPlatform.isMacOS || AppPlatform.isLinux)) {
      final appPath = await ref
          .read(fileDialogServiceProvider)
          .chooseApplication(label: l10n.sftpApplications, confirmButtonText: l10n.sftpChooseApplication);
      if (appPath == null || !context.mounted) return;
      openWith = OpenWithApp(AppRef(AppRefKind.path, appPath));
    }
    if (!context.mounted ||
        ref.read(activeProfileProvider)?.id != profileId ||
        ref.read(sftpControllerProvider).session != session) {
      return;
    }
    await ref.read(sftpBrowserServiceProvider).openInEditor(session, entry.path, openWith: openWith);
    if (!context.mounted) return;
    ref.read(sftpActivityProvider.notifier).show(SftpActivityTab.editing);
    showSnack(context, l10n.sftpEditOpened(entry.name));
  } on AppException catch (e) {
    if (!context.mounted || e.code == AppErrorCode.cancelled) return;
    final message = switch (e) {
      AppException(code: AppErrorCode.payloadTooLarge) => l10n.sftpEditTooLarge(
        entry.name,
        formatBytes(l10n, int.tryParse(e.args['size'] ?? '') ?? entry.size),
        formatBytes(l10n, int.tryParse(e.args['limit'] ?? '') ?? limit),
      ),
      AppException(reason: AppErrorReason.selectFile) => l10n.sftpEditNotAFile,
      _ => errorMessage(l10n, e),
    };
    showSnack(context, message, error: true);
  }
}

/// Conflict dialog → resolution.
Future<void> resolveEditConflict(BuildContext context, WidgetRef ref, EditSessionInfo session) async {
  final choice = await showEditConflictDialog(context, session);
  if (choice == null || !context.mounted) return;
  await runWithFeedback(context, () => ref.read(sftpBrowserServiceProvider).resolveEditConflict(session.id, choice));
}

/// "Stop editing": final upload; a conflict opens the conflict dialog.
Future<void> stopEditing(BuildContext context, WidgetRef ref, EditSessionInfo session) async {
  final l10n = context.l10n;
  final outcome = await runWithFeedback(context, () => ref.read(sftpBrowserServiceProvider).stopEditing(session.id));
  if (outcome == null || !context.mounted) return;
  switch (outcome) {
    case EditStopClosed() || EditStopKeptFiles():
      showSnack(context, l10n.sftpEditStopped(session.fileName));
    case EditStopConflict(:final remote):
      await resolveEditConflict(context, ref, session.copyWith(status: EditStatusConflict(remote: remote)));
    case EditStopUploadFailed(:final message):
      showSnack(context, l10n.sftpEditStopFailed(session.fileName, message), error: true);
  }
}

/// Once per profile and app run: offers to recover edit sessions a crash
/// or quit left behind ("Recover unsaved edits?").
final _leftoversCheckedProvider = NotifierProvider<_CheckedProfiles, Set<Object>>(_CheckedProfiles.new);

class _CheckedProfiles extends Notifier<Set<Object>> {
  @override
  Set<Object> build() => const {};

  bool markChecked(Object id) {
    if (state.contains(id)) return false;
    state = {...state, id};
    return true;
  }
}

/// Mount once inside the unlocked shell (or the SFTP screen): checks for
/// leftover edit sessions after unlock and shows the recovery dialog.
class SftpEditRecoveryListener extends ConsumerStatefulWidget {
  const SftpEditRecoveryListener({required this.child, super.key});

  final Widget child;

  @override
  ConsumerState<SftpEditRecoveryListener> createState() => _SftpEditRecoveryListenerState();
}

class _SftpEditRecoveryListenerState extends ConsumerState<SftpEditRecoveryListener> {
  bool _running = false;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) => _maybeCheck());
  }

  Future<void> _maybeCheck() async {
    if (!mounted || _running) return;
    final profile = ref.read(activeProfileProvider);
    final hosts = ref.read(hostsProvider);
    if (profile == null || !hosts.hasValue || ref.read(vaultStatusProvider).value?.isUnlocked != true) return;
    if (!ref.read(_leftoversCheckedProvider.notifier).markChecked(profile.id)) return;
    _running = true;
    try {
      final leftovers = await ref.read(sftpBrowserServiceProvider).listEditLeftovers();
      if (leftovers.isEmpty || !mounted) return;
      await _recover(leftovers);
    } on AppException {
      // Nothing to offer; the core lists them again on the next start.
    } finally {
      _running = false;
    }
  }

  Future<void> _recover(List<EditLeftover> leftovers) async {
    final hosts = ref.read(hostByIdProvider);
    final decision = await showEditLeftoversDialog(context, leftovers: leftovers, hosts: hosts);
    if (decision == null || !mounted) return;
    final l10n = context.l10n;
    final service = ref.read(sftpBrowserServiceProvider);
    for (final id in decision.discard) {
      await service.discardEditLeftover(id);
    }
    var recovered = 0;
    final sessions = <ObjectId, SftpSessionId>{};
    for (final leftover in leftovers.where((l) => decision.recover.contains(l.id))) {
      final host = hosts[leftover.hostId];
      if (host == null) continue;
      try {
        var session = sessions[host.id];
        if (session == null) {
          final browser = ref.read(sftpControllerProvider);
          if (browser.host?.id == host.id && browser.session != null) {
            session = browser.session;
          } else if (browser.session == null) {
            // Show the host in the browser, like opening it by hand.
            await ref.read(sftpControllerProvider.notifier).connect(host);
            session = ref.read(sftpControllerProvider).session;
          } else {
            // TODO(sftp-ui): background connections for other hosts live until app quit — the browser has
            // one session at a time; next: a connection pool in app-core shared by browser and edit sessions.
            session = await ref.read(sftpServiceProvider).connect(host.id);
          }
          if (session == null) continue;
          sessions[host.id] = session;
        }
        await service.resumeEditLeftover(leftover.id, session);
        recovered++;
      } on AppException catch (e) {
        if (mounted) showSnack(context, l10n.sftpLeftoversFailed(leftover.fileName ?? '', errorMessage(l10n, e)));
      }
    }
    if (!mounted || recovered == 0) return;
    ref.read(sftpActivityProvider.notifier).show(SftpActivityTab.editing);
    showSnack(context, l10n.sftpLeftoversRecovered(recovered));
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(hostsProvider, (_, _) => unawaited(_maybeCheck()));
    ref.listen(vaultStatusProvider, (_, _) => unawaited(_maybeCheck()));
    return widget.child;
  }
}
