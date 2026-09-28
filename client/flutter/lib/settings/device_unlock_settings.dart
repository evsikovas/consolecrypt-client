import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/l10n/labels.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// This preference belongs to the active profile on this device, not to
/// synced vault settings. The core enforces it for unlock and recovery.
class DeviceUnlockSettings extends ConsumerStatefulWidget {
  const DeviceUnlockSettings({super.key});

  @override
  ConsumerState<DeviceUnlockSettings> createState() => _DeviceUnlockSettingsState();
}

class _DeviceUnlockSettingsState extends ConsumerState<DeviceUnlockSettings> with WidgetsBindingObserver {
  bool _busy = false;
  AppException? _error;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) _refresh();
    });
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.resumed && !_busy) _refresh();
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    super.dispose();
  }

  Future<void> _run(Future<void> Function() action) async {
    if (_busy) return;
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      await action();
    } on AppException catch (e) {
      if (mounted) setState(() => _error = e);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _refresh() => _run(() => ref.read(vaultServiceProvider).refreshDeviceUnlockAvailability());

  @override
  Widget build(BuildContext context) {
    final status = ref.watch(vaultStatusProvider).value;
    final info = status?.deviceUnlock ?? const DeviceUnlockInfo();
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    final help = !info.hasDeviceEnvelope
        ? l.deviceUnlockNeedsTrust
        : info.notEnrolled
        ? l.deviceUnlockNotEnrolled
        : info.kind == null
        ? l.deviceUnlockUnsupported
        : info.kind == DeviceAuthKind.deviceCredential
        ? (AppPlatform.isMobile ? l.mobileDeviceAuthHelp : l.deviceUnlockMacPassword)
        : l.deviceUnlockHelp;
    return Column(
      key: const ValueKey('device-unlock-settings'),
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        SwitchListTile.adaptive(
          key: const ValueKey('device-unlock-switch'),
          contentPadding: EdgeInsets.zero,
          secondary: const Icon(Icons.fingerprint_rounded),
          title: Text(info.kind == null ? l.deviceUnlockTitle : l.deviceUnlockWithMethod(info.authName(l))),
          subtitle: Text(help),
          value: info.enabled,
          onChanged: !_busy && (status?.isUnlocked ?? false) && (info.canEnable || info.enabled)
              ? (enabled) => _run(
                  () => ref
                      .read(vaultServiceProvider)
                      .setDeviceUnlockEnabled(enabled, reason: l.deviceUnlockEnableReason),
                )
              : null,
        ),
        Text(l.deviceUnlockLocalOnly, style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel)),
        if (_busy) const Padding(padding: EdgeInsets.only(top: 8), child: LinearProgressIndicator()),
        if (_error != null)
          Padding(
            padding: const EdgeInsets.only(top: 8),
            child: Text(
              errorMessage(l, _error!),
              key: const ValueKey('device-unlock-error'),
              style: TextStyle(color: tokens.palette.danger),
            ),
          ),
        if (!info.canEnable || info.kind == DeviceAuthKind.deviceCredential)
          Align(
            alignment: Alignment.centerLeft,
            child: GlassButton.plain(
              key: const ValueKey('device-unlock-refresh'),
              label: l.deviceUnlockRefresh,
              icon: Icons.refresh_rounded,
              onPressed: _busy ? null : _refresh,
            ),
          ),
      ],
    );
  }
}
