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
  final release = ref.read(updateControllerProvider).release;
  if (release == null) return;
  final l = context.l10n;
  final approved = await showDialog<bool>(
    context: context,
    builder: (context) => AlertDialog(
      title: Text(l.updateAvailable(release.version)),
      content: Text(AppPlatform.isMacOS ? l.updateMacInstallHelp : l.updateInstallHelp),
      actions: [
        TextButton(onPressed: () => Navigator.pop(context, false), child: Text(l.commonCancel)),
        FilledButton(onPressed: () => Navigator.pop(context, true), child: Text(l.updateDownloadInstall)),
      ],
    ),
  );
  if (approved == true && context.mounted) {
    await ref
        .read(updateControllerProvider.notifier)
        .downloadAndInstall(macosSaveTitle: l.updateMacSaveTitle, macosSavePrompt: l.updateMacSavePrompt);
  }
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
  UpdatePhase.failed => l.updateFailed,
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
    if (AppPlatform.isLinux) {
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
              child: Text(state.error == 'destination_exists' ? l.updateMacDestinationExists : l.updateFailedHelp),
            ),
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
              if (release != null && state.phase != UpdatePhase.opened)
                GlassButton(
                  key: const ValueKey('updates-install'),
                  label: l.updateDownloadInstall,
                  icon: Icons.download_rounded,
                  busy: state.phase == UpdatePhase.downloading || state.phase == UpdatePhase.installing,
                  onPressed: state.busy ? null : () => confirmInstallUpdate(context, ref),
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
  const UpdateNoticeScope({required this.child, super.key});
  final Widget child;
  @override
  ConsumerState<UpdateNoticeScope> createState() => _UpdateNoticeScopeState();
}

class _UpdateNoticeScopeState extends ConsumerState<UpdateNoticeScope> {
  bool _started = false;
  String? _dismissed;
  @override
  Widget build(BuildContext context) {
    if (!AppPlatform.supportsDirectUpdates) return widget.child;
    final settings = ref.watch(localSettingsProvider).value;
    if (!_started && settings != null) {
      _started = true;
      if (settings.checkUpdatesAutomatically && !ref.read(appServicesProvider).isMock) {
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (mounted) unawaited(ref.read(updateControllerProvider.notifier).check());
        });
      }
    }
    final state = ref.watch(updateControllerProvider);
    final release = state.release;
    final visible = release != null && release.version != _dismissed && state.phase != UpdatePhase.opened;
    return Stack(
      children: [
        widget.child,
        if (visible)
          Positioned(
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
                        Expanded(child: Text(updateStatus(context.l10n, state))),
                        const SizedBox(width: 8),
                        GlassIconButton(
                          tooltip: context.l10n.updateDownloadInstall,
                          icon: Icons.download_rounded,
                          onPressed: state.busy ? null : () => confirmInstallUpdate(context, ref),
                        ),
                        GlassIconButton(
                          tooltip: context.l10n.commonClose,
                          icon: Icons.close_rounded,
                          onPressed: () => setState(() => _dismissed = release.version),
                        ),
                      ],
                    ),
                  ),
                ),
              ),
            ),
          ),
      ],
    );
  }
}
