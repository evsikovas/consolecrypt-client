import 'dart:async';

import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/docker/docker_service.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:material_ui/material_ui.dart';

/// Marker the native side resolves to bash, ash or sh inside the container.
const dockerAutoShell = '/__consolecrypt_auto_shell__'; // l10n-ignore: wire marker

const dockerShells = ['/bin/sh', '/bin/bash', '/bin/ash'];

/// Lets the user pick the shell, opens a terminal tab in the container and
/// shows the Terminal section. Returns whether a tab was opened.
Future<bool> openDockerTerminal(
  BuildContext context, {
  required DockerSessionInfo session,
  required String hostName,
  required DockerContainer container,
}) async {
  final opened =
      await showAppDialog<bool>(
        context,
        secure: true,
        builder: (_) => _TerminalDialog(session: session, hostName: hostName, container: container),
      ) ??
      false;
  if (opened && context.mounted) GoRouter.maybeOf(context)?.go(AppRoutes.terminal);
  return opened;
}

class _TerminalDialog extends ConsumerStatefulWidget {
  const _TerminalDialog({required this.session, required this.hostName, required this.container});
  final DockerSessionInfo session;
  final String hostName;
  final DockerContainer container;
  @override
  ConsumerState<_TerminalDialog> createState() => _TerminalDialogState();
}

class _TerminalDialogState extends ConsumerState<_TerminalDialog> {
  String _shell = dockerAutoShell;
  bool _busy = false, _failed = false;

  /// First container terminal of this profile: say once that it gives full
  /// access (the vault passphrase only guards what the Docker views show).
  late final bool _hint = !ref.read(settingsServiceProvider).currentLocal.dockerTerminalHintShown;

  @override
  void initState() {
    super.initState();
    if (_hint) unawaited(_rememberHint());
  }

  Future<void> _rememberHint() async {
    final settings = ref.read(settingsServiceProvider);
    try {
      await settings.updateLocal(settings.currentLocal.copyWith(dockerTerminalHintShown: true));
    } catch (_) {
      // A hint only; it shows again next time.
    }
  }

  Future<void> _open() async {
    if (_busy) return;
    final controller = ref.read(terminalTabsProvider.notifier);
    setState(() {
      _busy = true;
      _failed = false;
    });
    try {
      final tab = await controller.openTarget(
        DockerTerminalTarget(
          sessionId: widget.session.id,
          hostName: widget.hostName,
          containerId: widget.container.id,
          containerName: widget.container.name,
          hostId: widget.session.hostId,
          command: [_shell],
        ),
      );
      if (!mounted) {
        await controller.close(tab);
        return;
      }
      closeDialog(context, true);
    } catch (_) {
      if (mounted) setState(() => _failed = true);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    return GlassDialog(
      key: const ValueKey('docker-terminal-dialog'),
      title: l.dockerTerminalTitle,
      content: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(
            [widget.hostName, widget.container.name].where((s) => s.isNotEmpty).join(' · '),
            style: tokens.typography.bodyEmph.copyWith(color: tokens.palette.label),
          ),
          const SizedBox(height: GlassSpacing.s12),
          if (_hint) ...[
            InfoBanner(
              key: const ValueKey('docker-terminal-hint'),
              tone: BannerTone.info,
              message: l.dockerTerminalHint,
            ),
            const SizedBox(height: GlassSpacing.s12),
          ],
          GlassDropdownField<String>(
            key: const ValueKey('docker-terminal-shell'),
            value: _shell,
            label: l.dockerShell,
            items: [
              GlassSelectItem(value: dockerAutoShell, label: l.dockerAutoShell),
              for (final shell in dockerShells) GlassSelectItem(value: shell, label: shell),
            ],
            onChanged: _busy ? null : (value) => setState(() => _shell = value),
          ),
          if (_busy) const Padding(padding: EdgeInsets.only(top: 8), child: LinearProgressIndicator(minHeight: 2)),
          if (_failed)
            Padding(
              padding: const EdgeInsets.only(top: 8),
              child: Text(
                l.dockerTerminalFailed,
                key: const ValueKey('docker-terminal-error'),
                style: tokens.typography.callout.copyWith(color: tokens.palette.danger),
              ),
            ),
        ],
      ),
      primaryAction: GlassButton.prominent(
        key: const ValueKey('docker-terminal-open'),
        label: l.dockerOpenShell,
        busy: _busy,
        onPressed: _busy ? null : _open,
      ),
      secondaryActions: [GlassButton(label: l.commonCancel, onPressed: () => closeDialog(context, false))],
    );
  }
}
