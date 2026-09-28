import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

/// Native device preference, deliberately outside vault/profile settings:
/// Android must apply it before the first Flutter frame, even while locked.
class ScreenCaptureSettings extends StatefulWidget {
  const ScreenCaptureSettings({super.key});

  @override
  State<ScreenCaptureSettings> createState() => _ScreenCaptureSettingsState();
}

class _ScreenCaptureSettingsState extends State<ScreenCaptureSettings> with WidgetsBindingObserver {
  static const _channel = MethodChannel('consolecrypt/screen_capture');
  bool? _allowed;
  bool _busy = false;
  bool _error = false;

  bool get _android => !kIsWeb && defaultTargetPlatform == TargetPlatform.android;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    if (_android) _load();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (_android && state == AppLifecycleState.resumed && !_busy) _load();
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    super.dispose();
  }

  Future<void> _load() => _run(() => _channel.invokeMethod<bool>('getAllowed'));

  Future<void> _run(Future<bool?> Function() action) async {
    if (_busy) return;
    setState(() {
      _busy = true;
      _error = false;
    });
    try {
      final allowed = await action();
      if (mounted) {
        setState(() {
          _error = allowed == null;
          if (allowed != null) _allowed = allowed;
        });
      }
    } on PlatformException {
      if (mounted) setState(() => _error = true);
    } on MissingPluginException {
      if (mounted) setState(() => _error = true);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    if (!_android && !AppPlatform.isMacOS) return const SizedBox.shrink();
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    if (!_android) {
      // NSWindow.sharingType = .none is a legacy API ignored by current
      // macOS capture APIs. Do not present a non-working protection switch.
      return ListTile(
        key: const ValueKey('screen-capture-macos-info'),
        contentPadding: EdgeInsets.zero,
        leading: const Icon(Icons.screenshot_rounded),
        title: Text(l.screenCaptureTitle),
        subtitle: Text(l.screenCaptureMacosHelp),
      );
    }
    return Column(
      key: const ValueKey('screen-capture-settings'),
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        SwitchListTile.adaptive(
          key: const ValueKey('screen-capture-switch'),
          contentPadding: EdgeInsets.zero,
          title: Text(l.screenCaptureAllow),
          subtitle: Text(l.screenCaptureHelp),
          value: _allowed ?? false,
          onChanged: !_busy && _allowed != null && !_error
              ? (allowed) => _run(() => _channel.invokeMethod<bool>('setAllowed', {'allowed': allowed}))
              : null,
        ),
        Text(l.screenCaptureLocalOnly, style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel)),
        if (_busy) const LinearProgressIndicator(),
        if (_error) ...[
          const SizedBox(height: GlassSpacing.s8),
          Text(
            l.screenCaptureError,
            key: const ValueKey('screen-capture-error'),
            style: TextStyle(color: tokens.palette.danger),
          ),
          Align(
            alignment: AlignmentDirectional.centerStart,
            child: GlassButton.plain(
              key: const ValueKey('screen-capture-retry'),
              icon: Icons.refresh_rounded,
              label: l.commonRetry,
              onPressed: _busy ? null : _load,
            ),
          ),
        ],
      ],
    );
  }
}
