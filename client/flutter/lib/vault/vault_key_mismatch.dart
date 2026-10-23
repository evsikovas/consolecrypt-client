import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/vault_key_alarm.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Shows [VaultKeyMismatchDialog] whenever [vaultKeyAlarm] is raised, on
/// any screen (gate screens included). Mounted in `MaterialApp.builder`,
/// above the router: dialogs go through [navigatorKey].
class VaultKeyAlarmScope extends ConsumerStatefulWidget {
  const VaultKeyAlarmScope({required this.child, this.navigatorKey, super.key});
  final Widget child;
  final GlobalKey<NavigatorState>? navigatorKey;
  @override
  ConsumerState<VaultKeyAlarmScope> createState() => _VaultKeyAlarmScopeState();
}

class _VaultKeyAlarmScopeState extends ConsumerState<VaultKeyAlarmScope> {
  bool _showing = false;

  @override
  void initState() {
    super.initState();
    vaultKeyAlarm.addListener(_raise);
  }

  @override
  void dispose() {
    vaultKeyAlarm.removeListener(_raise);
    super.dispose();
  }

  void _raise() {
    final error = vaultKeyAlarm.value;
    if (error == null || _showing) return;
    WidgetsBinding.instance.addPostFrameCallback((_) async {
      final navigatorContext = widget.navigatorKey?.currentState?.overlay?.context ?? (mounted ? context : null);
      if (navigatorContext == null || !navigatorContext.mounted || _showing) return;
      _showing = true;
      try {
        await showVaultKeyMismatch(navigatorContext, error);
      } finally {
        _showing = false;
        vaultKeyAlarm.value = null;
      }
    });
    // The report may arrive while no frame is pending (core event).
    WidgetsBinding.instance.scheduleFrame();
  }

  @override
  Widget build(BuildContext context) => widget.child;
}

Future<void> showVaultKeyMismatch(BuildContext context, AppException error) => showAppDialog<void>(
  context,
  secure: true,
  barrierDismissible: false,
  builder: (_) => VaultKeyMismatchDialog(error: error),
);

/// TR-9: "The server sent a different vault key…" with Lock, Copy
/// diagnostics and More. There is deliberately no "accept the new key".
class VaultKeyMismatchDialog extends ConsumerStatefulWidget {
  const VaultKeyMismatchDialog({required this.error, super.key});
  final AppException error;
  @override
  ConsumerState<VaultKeyMismatchDialog> createState() => _VaultKeyMismatchDialogState();
}

class _VaultKeyMismatchDialogState extends ConsumerState<VaultKeyMismatchDialog> {
  final DateTime _at = DateTime.now().toUtc();
  bool _more = false;

  String _diagnostics() {
    final a = widget.error.args;
    final server = ref.read(activeProfileProvider)?.serverUrl?.toString() ?? '-';
    return [
      'vault_key_mismatch', // l10n-ignore: diagnostic
      'vault_id: ${a['vault_id'] ?? '-'}', // l10n-ignore: diagnostic
      'pinned: ${a['pinned'] ?? '-'}', // l10n-ignore: diagnostic
      'received: ${a['received'] ?? '-'}', // l10n-ignore: diagnostic
      'reason: ${a['reason'] ?? '-'}', // l10n-ignore: diagnostic
      'time: ${_at.toIso8601String()}', // l10n-ignore: diagnostic
      'server: $server', // l10n-ignore: diagnostic
    ].join('\n');
  }

  Future<void> _lock() async {
    try {
      await ref.read(vaultServiceProvider).lock();
    } on Object {
      // Already locked (gate screens): nothing to do.
    }
    if (mounted) closeDialog<void>(context);
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    return GlassDialog(
      key: const ValueKey('vault-key-mismatch'),
      icon: Icons.gpp_bad_rounded,
      iconTone: GlassTone.danger,
      title: l.vaultKeyMismatchTitle,
      width: 560,
      content: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(l.vaultKeyMismatchMessage),
          if (_more) ...[
            const SizedBox(height: GlassSpacing.s12),
            Text(l.vaultKeyMismatchDetails),
            const SizedBox(height: GlassSpacing.s8),
            SelectableText(
              _diagnostics(),
              key: const ValueKey('vault-key-mismatch-diagnostics'),
              style: tokens.typography.mono.copyWith(fontSize: 12, color: tokens.secondaryLabel),
            ),
          ],
        ],
      ),
      leadingAction: GlassButton.plain(
        key: const ValueKey('vault-key-mismatch-more'),
        label: l.vaultKeyMismatchMore,
        onPressed: () => setState(() => _more = !_more),
      ),
      secondaryActions: [
        GlassButton(
          key: const ValueKey('vault-key-mismatch-copy'),
          icon: Icons.copy_rounded,
          label: l.vaultKeyMismatchCopy,
          onPressed: () => copyPlainWithNotice(context, ref, _diagnostics(), what: l.vaultKeyMismatchDiagnostics),
        ),
      ],
      primaryAction: GlassButton.prominent(
        key: const ValueKey('vault-key-mismatch-lock'),
        autofocus: true,
        icon: Icons.lock_rounded,
        label: l.vaultKeyMismatchLock,
        onPressed: _lock,
      ),
    );
  }
}
