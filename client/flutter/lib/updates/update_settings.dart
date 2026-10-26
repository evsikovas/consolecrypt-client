import 'dart:async';

import 'package:consolecrypt/app/app_info.dart';
import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/settings.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/updates/update_controller.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

Future<void> confirmInstallUpdate(BuildContext context, WidgetRef ref) async {
  final state = ref.read(updateControllerProvider);
  final release = state.release;
  if (release == null) return;
  final l = context.l10n;
  if (!state.inPlace) {
    // A portable (unpacked ZIP) Windows copy: Setup would install a second
    // copy elsewhere and this one would keep offering the same update.
    final open = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(l.updateAvailable(release.version)),
        content: Text(l.updatePortableHelp),
        actions: [
          TextButton(onPressed: () => Navigator.pop(context, false), child: Text(l.commonCancel)),
          FilledButton(onPressed: () => Navigator.pop(context, true), child: Text(l.updateOpenDownloadPage)),
        ],
      ),
    );
    if (open == true && context.mounted) await openUpdateDownloadPage(context, ref);
    return;
  }
  final approved = await showDialog<bool>(
    context: context,
    builder: (context) => AlertDialog(
      title: Text(l.updateAvailable(release.version)),
      content: Text(
        AppPlatform.isMacOS
            ? l.updateMacInstallHelp
            : AppPlatform.isWindows
            ? l.updateWindowsInstallHelp
            : AppPlatform.isLinux
            ? l.updateLinuxInstallHelp
            : l.updateInstallHelp,
      ),
      actions: [
        TextButton(onPressed: () => Navigator.pop(context, false), child: Text(l.commonCancel)),
        FilledButton(onPressed: () => Navigator.pop(context, true), child: Text(l.updateDownloadInstall)),
      ],
    ),
  );
  if (approved == true &&
      context.mounted &&
      ref.context.mounted &&
      identical(ref.read(updateControllerProvider).release, release)) {
    await ref
        .read(updateControllerProvider.notifier)
        .downloadAndInstall(macosSaveTitle: l.updateMacSaveTitle, macosSavePrompt: l.updateMacSavePrompt);
  }
}

Uri _downloadPage(AppLocalizations l) => updateDownloadPage(l.localeName.split('_').first);

/// Opens the official download page; a failure is reported, never ignored.
Future<void> openUpdateDownloadPage(BuildContext context, WidgetRef ref) async {
  final page = _downloadPage(context.l10n);
  var opened = false;
  try {
    opened = await ref.read(updateOpenUrlProvider)(page);
  } on Object {
    opened = false;
  }
  if (!opened && ref.context.mounted) ref.read(updateControllerProvider.notifier).downloadPageFailed();
}

String updateStatus(AppLocalizations l, UpdateState state) => switch (state.phase) {
  UpdatePhase.idle => l.updateCurrentVersion(kAppFullVersion),
  UpdatePhase.checking => l.updateChecking,
  UpdatePhase.current => l.updateCurrent,
  UpdatePhase.available || UpdatePhase.ready => l.updateAvailable(state.release!.version),
  UpdatePhase.downloading => l.updateDownloading((state.progress * 100).round()),
  UpdatePhase.installing => l.updateInstalling,
  UpdatePhase.permission => l.updateAndroidPermission,
  UpdatePhase.opened => AppPlatform.isMacOS ? l.updateMacInstallerOpened : l.updateInstallerOpened,
  UpdatePhase.installed => l.updateLinuxInstalled,
  UpdatePhase.failed => l.updateFailed,
};

/// Windows, macOS, Android, and Linux copies installed from the DEB or RPM
/// (a build tree or copied folder keeps the download page).
bool directUpdates(WidgetRef ref) =>
    AppPlatform.supportsDirectUpdates ||
    (AppPlatform.isLinux && ref.read(updateServiceProvider).platform.startsWith('linux-'));

/// What went wrong and what to do next, for every failure code.
String updateErrorMessage(AppLocalizations l, UpdateState state) => switch (state.error) {
  'destination_exists' => l.updateMacDestinationExists,
  'network' || 'http' || 'redirect' || 'url' => l.updateErrorNetwork,
  'size' || 'checksum' => l.updateErrorIntegrity,
  'signature' ||
  'manifest_format' ||
  'manifest_size' ||
  'manifest_expired' ||
  'asset' ||
  'version' => l.updateErrorFeed,
  'manifest_rollback' => l.updateErrorRollback,
  'launch' => l.updateErrorLaunch(state.errorDetail ?? '?'),
  'pkexec' => l.updateLinuxNoPkexec,
  'installer' when AppPlatform.isLinux => l.updateLinuxInstallFailed(state.errorDetail ?? '?'),
  'storage' => l.updateErrorStorage,
  'portable' => l.updatePortableHelp,
  'browser' => l.updateErrorBrowser(_downloadPage(l).toString()),
  _ => l.updateFailedHelp,
};

class UpdateSettingsSection extends ConsumerWidget {
  const UpdateSettingsSection({super.key});
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l = context.l10n;
    if (AppPlatform.isIOS) {
      return SectionCard(
        key: const ValueKey('settings-updates'),
        title: l.updatesTitle,
        icon: Icons.system_update_alt_rounded,
        child: Text(l.updateAppleHelp),
      );
    }
    if (AppPlatform.isLinux && !directUpdates(ref)) {
      final language = l.localeName.split('_').first;
      final downloadPage = Uri.https('consolecrypt.dev', '/download', {'lang': language});
      return SectionCard(
        key: const ValueKey('settings-updates'),
        title: l.updatesTitle,
        icon: Icons.system_update_alt_rounded,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(l.updateCurrentVersion(kAppFullVersion)),
            const SizedBox(height: GlassSpacing.s12),
            Text(l.updateLinuxHelp),
            const SizedBox(height: GlassSpacing.s12),
            Text(l.updateLinuxDownloadPage),
            const SizedBox(height: GlassSpacing.s4),
            SelectableText(downloadPage.toString(), key: const ValueKey('updates-linux-download-page')),
          ],
        ),
      );
    }
    final settings = ref.read(settingsServiceProvider);
    final local = ref.watch(localSettingsProvider).value ?? const LocalSettings();
    final state = ref.watch(updateControllerProvider);
    final release = state.release;
    final notes = release?.notes[l.localeName.split('_').first] ?? release?.notes['en'] ?? '';
    return SectionCard(
      key: const ValueKey('settings-updates'),
      title: l.updatesTitle,
      icon: Icons.system_update_alt_rounded,
      subtitle: l.updatePrivacyHelp,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          SwitchListTile.adaptive(
            key: const ValueKey('updates-automatic'),
            contentPadding: EdgeInsets.zero,
            title: Text(l.updateAutomatic),
            value: local.checkUpdatesAutomatically,
            onChanged: (value) =>
                settings.updateLocal(settings.currentLocal.copyWith(checkUpdatesAutomatically: value)),
          ),
          Text(updateStatus(l, state), key: const ValueKey('updates-status')),
          if (notes.isNotEmpty) Padding(padding: const EdgeInsets.only(top: 8), child: Text(notes)),
          if (state.phase == UpdatePhase.downloading)
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 12),
              child: LinearProgressIndicator(value: state.progress),
            ),
          if (state.phase == UpdatePhase.failed)
            Padding(
              padding: const EdgeInsets.only(top: 8),
              child: SelectableText(updateErrorMessage(l, state), key: const ValueKey('updates-error')),
            ),
          if (release != null && !state.inPlace && state.phase != UpdatePhase.failed)
            Padding(padding: const EdgeInsets.only(top: 8), child: Text(l.updatePortableHelp)),
          const SizedBox(height: GlassSpacing.s12),
          Wrap(
            spacing: GlassSpacing.s8,
            runSpacing: GlassSpacing.s8,
            children: [
              GlassButton(
                key: const ValueKey('updates-check'),
                label: l.updateCheck,
                icon: Icons.refresh_rounded,
                busy: state.phase == UpdatePhase.checking,
                onPressed: state.busy ? null : ref.read(updateControllerProvider.notifier).check,
              ),
              if (state.phase == UpdatePhase.installed)
                GlassButton.prominent(
                  key: const ValueKey('updates-relaunch'),
                  label: l.updateRelaunch,
                  icon: Icons.restart_alt_rounded,
                  onPressed: () => unawaited(ref.read(updateRelaunchProvider)()),
                )
              else if (release != null && state.inPlace && state.phase != UpdatePhase.opened)
                GlassButton(
                  key: const ValueKey('updates-install'),
                  label: state.phase == UpdatePhase.failed ? l.commonRetry : l.updateDownloadInstall,
                  icon: Icons.download_rounded,
                  busy: state.phase == UpdatePhase.downloading || state.phase == UpdatePhase.installing,
                  onPressed: state.busy ? null : () => confirmInstallUpdate(context, ref),
                ),
              if (release != null && (!state.inPlace || state.phase == UpdatePhase.failed))
                GlassButton(
                  key: const ValueKey('updates-download-page'),
                  label: l.updateOpenDownloadPage,
                  icon: Icons.open_in_new_rounded,
                  onPressed: state.busy ? null : () => openUpdateDownloadPage(context, ref),
                ),
            ],
          ),
        ],
      ),
    );
  }
}

/// The startup check runs once, after persisted preferences arrive, even while
/// the vault is locked. Demo/test backends never contact the update service.
class UpdateNoticeScope extends ConsumerStatefulWidget {
  const UpdateNoticeScope({required this.child, this.navigatorKey, super.key});
  final Widget child;

  /// MaterialApp.builder is above its Router/Navigator. Its context cannot
  /// host tooltips or dialogs; mount the notice in this navigator's overlay.
  /// When used inside a route, the nearest Navigator is sufficient.
  final GlobalKey<NavigatorState>? navigatorKey;
  @override
  ConsumerState<UpdateNoticeScope> createState() => _UpdateNoticeScopeState();
}

class _UpdateNoticeScopeState extends ConsumerState<UpdateNoticeScope> {
  bool _started = false;
  String? _dismissed;
  OverlayEntry? _notice;
  OverlayState? _overlay;
  bool _refreshScheduled = false;
  bool _confirming = false;

  @override
  void dispose() {
    _removeNotice();
    super.dispose();
  }

  void _removeNotice() {
    _notice?.remove();
    _notice?.dispose();
    _notice = null;
    _overlay = null;
  }

  void _scheduleNotice() {
    if (_refreshScheduled) return;
    _refreshScheduled = true;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      _refreshScheduled = false;
      if (!mounted) return;
      final state = ref.read(updateControllerProvider);
      final release = state.release;
      final visible =
          directUpdates(ref) && release != null && release.version != _dismissed && state.phase != UpdatePhase.opened;
      final overlay = widget.navigatorKey?.currentState?.overlay ?? Navigator.maybeOf(context)?.overlay;
      if (!visible || overlay == null || !overlay.mounted) {
        _removeNotice();
        return;
      }
      if (_overlay != overlay) {
        _removeNotice();
        _overlay = overlay;
        _notice = OverlayEntry(builder: _buildNotice);
        overlay.insert(_notice!);
      } else {
        _notice?.markNeedsBuild();
      }
    });
  }

  Future<void> _confirm(BuildContext noticeContext) async {
    if (_confirming || ref.read(updateControllerProvider).busy) return;
    _confirming = true;
    try {
      await confirmInstallUpdate(noticeContext, ref);
    } finally {
      _confirming = false;
    }
  }

  @override
  Widget build(BuildContext context) {
    if (!directUpdates(ref)) {
      if (_notice != null) _scheduleNotice();
      return widget.child;
    }
    final settings = ref.watch(localSettingsProvider).value;
    if (!_started && settings != null) {
      _started = true;
      if (settings.checkUpdatesAutomatically && !ref.read(appServicesProvider).isMock) {
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (mounted) unawaited(ref.read(updateControllerProvider.notifier).check());
        });
      }
    }
    ref.watch(updateControllerProvider);
    _scheduleNotice();
    return widget.child;
  }

  Widget _buildNotice(BuildContext context) {
    final state = ref.read(updateControllerProvider);
    final release = state.release;
    if (release == null || release.version == _dismissed || state.phase == UpdatePhase.opened) {
      return const SizedBox.shrink();
    }
    final installed = state.phase == UpdatePhase.installed;
    final l = context.l10n;
    final failed = state.phase == UpdatePhase.failed;
    final downloading = state.phase == UpdatePhase.downloading;
    return Positioned(
      key: const ValueKey('update-notice'),
      left: 16,
      right: 16,
      bottom: MediaQuery.paddingOf(context).bottom + 16,
      child: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 640),
          child: Material(
            borderRadius: BorderRadius.circular(16),
            color: Theme.of(context).colorScheme.surfaceContainerHigh,
            elevation: 8,
            child: Padding(
              padding: const EdgeInsets.all(12),
              child: Row(
                children: [
                  Expanded(
                    child: Column(
                      mainAxisSize: MainAxisSize.min,
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: [
                        Text(updateStatus(l, state), key: const ValueKey('update-notice-status')),
                        if (downloading || state.phase == UpdatePhase.installing)
                          Padding(
                            padding: const EdgeInsets.only(top: GlassSpacing.s8),
                            child: LinearProgressIndicator(
                              key: const ValueKey('update-notice-progress'),
                              value: downloading ? state.progress : null,
                            ),
                          ),
                        if (failed)
                          Padding(
                            padding: const EdgeInsets.only(top: GlassSpacing.s4),
                            child: Text(updateErrorMessage(l, state), key: const ValueKey('update-notice-error')),
                          ),
                      ],
                    ),
                  ),
                  const SizedBox(width: 8),
                  if (installed)
                    GlassIconButton(
                      key: const ValueKey('update-notice-relaunch'),
                      tooltip: l.updateRelaunch,
                      icon: Icons.restart_alt_rounded,
                      onPressed: () => unawaited(ref.read(updateRelaunchProvider)()),
                    )
                  else if (state.inPlace)
                    GlassIconButton(
                      key: const ValueKey('update-notice-install'),
                      tooltip: failed ? l.commonRetry : l.updateDownloadInstall,
                      icon: failed ? Icons.refresh_rounded : Icons.download_rounded,
                      onPressed: state.busy ? null : () => unawaited(_confirm(context)),
                    ),
                  if (!state.inPlace || failed)
                    GlassIconButton(
                      key: const ValueKey('update-notice-download-page'),
                      tooltip: l.updateOpenDownloadPage,
                      icon: Icons.open_in_new_rounded,
                      onPressed: state.busy
                          ? null
                          : () => unawaited(state.inPlace ? openUpdateDownloadPage(context, ref) : _confirm(context)),
                    ),
                  GlassIconButton(
                    key: const ValueKey('update-notice-dismiss'),
                    tooltip: l.commonClose,
                    icon: Icons.close_rounded,
                    onPressed: () => setState(() => _dismissed = release.version),
                  ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}
