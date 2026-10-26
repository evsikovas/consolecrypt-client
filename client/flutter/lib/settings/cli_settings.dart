import 'dart:io';

import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/settings/cli_installer.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Settings → Command line: installs the bundled `consolecrypt-cli` (desktop).
class CliSettingsSection extends ConsumerStatefulWidget {
  const CliSettingsSection({super.key});

  @override
  ConsumerState<CliSettingsSection> createState() => _CliSettingsSectionState();
}

class _CliSettingsSectionState extends ConsumerState<CliSettingsSection> {
  bool _busy = false;

  Future<void> _change({required bool install}) async {
    final l = context.l10n;
    final installer = ref.read(cliInstallerProvider);
    setState(() => _busy = true);
    try {
      install ? await installer.install() : await installer.uninstall();
      if (mounted) showSnack(context, install ? l.cliInstalledHint : l.cliRemoved);
    } on CliInstallException catch (e) {
      if (!mounted || e.code == 'cancelled') return;
      showSnack(context, e.code == 'conflict' ? l.cliConflict(e.detail ?? cliCommand) : l.cliFailed, error: true);
    } finally {
      ref.invalidate(cliCommandStateProvider);
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    if (AppPlatform.isMobile) return const SizedBox.shrink();
    final l = context.l10n;
    final state = ref.watch(cliCommandStateProvider).value;
    final bundled = state?.bundled != null;
    final installed = state?.installed ?? false;
    final status = state == null
        ? null
        : !bundled
        ? l.cliNotBundled
        : installed
        ? l.cliInstalled(state.location ?? cliCommand)
        : l.cliNotInstalled;
    return Padding(
      padding: const EdgeInsets.only(bottom: GlassSpacing.s16),
      child: SectionCard(
        key: const ValueKey('settings-cli'),
        title: l.cliTitle,
        icon: Icons.terminal_rounded,
        subtitle: l.cliHelp,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            if (status != null) Text(status, key: const ValueKey('cli-status')),
            if (installed && !state!.onPath) ...[
              const SizedBox(height: GlassSpacing.s4),
              Text(
                l.cliNotOnPath(state.location == null ? '~/.local/bin' : File(state.location!).parent.path),
                key: const ValueKey('cli-not-on-path'),
                style: Theme.of(context).textTheme.bodySmall,
              ),
            ],
            if (bundled) ...[
              const SizedBox(height: GlassSpacing.s12),
              Align(
                alignment: AlignmentDirectional.centerStart,
                child: installed
                    ? GlassButton(
                        key: const ValueKey('cli-uninstall'),
                        label: l.cliUninstall,
                        icon: Icons.remove_circle_outline_rounded,
                        busy: _busy,
                        onPressed: _busy ? null : () => _change(install: false),
                      )
                    : GlassButton.prominent(
                        key: const ValueKey('cli-install'),
                        label: l.cliInstall,
                        icon: Icons.download_rounded,
                        busy: _busy,
                        onPressed: _busy ? null : () => _change(install: true),
                      ),
              ),
            ],
          ],
        ),
      ),
    );
  }
}
