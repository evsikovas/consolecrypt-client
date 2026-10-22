import 'dart:async';

import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/secret_exposure.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Whether the window must be hidden from screen capture under [mode].
bool excludeFromCapture(ScreenCaptureProtection mode, {required bool secretsVisible}) => switch (mode) {
  ScreenCaptureProtection.off => false,
  ScreenCaptureProtection.whileSecretsVisible => secretsVisible,
  ScreenCaptureProtection.always => true,
};

/// Native window capture exclusion (Windows runner, `consolecrypt/window`:
/// `SetWindowDisplayAffinity` with `WDA_EXCLUDEFROMCAPTURE`, `WDA_MONITOR`
/// before Windows 10 2004). Best-effort: false when nothing changed.
class WindowCaptureAffinity {
  const WindowCaptureAffinity();

  static const _channel = MethodChannel('consolecrypt/window');

  /// Platforms where the window can be hidden from capture.
  static bool get supported => AppPlatform.isWindows;

  Future<bool> setExcluded({required bool excluded}) async {
    if (!supported) return false;
    try {
      return await _channel.invokeMethod<String>('setCaptureExcluded', {'excluded': excluded}) != null;
    } on PlatformException {
      return false;
    } on MissingPluginException {
      return false;
    }
  }
}

final windowCaptureAffinityProvider = Provider<WindowCaptureAffinity>((ref) => const WindowCaptureAffinity());

/// Applies the "Hide window from screen capture" setting: always, never, or
/// while [SecretExposure] reports a revealed secret.
class CaptureProtectionScope extends ConsumerStatefulWidget {
  const CaptureProtectionScope({super.key, required this.child, this.exposure});

  final Widget child;

  /// Revealed-secrets counter to follow; [SecretExposure.instance] by default.
  final SecretExposure? exposure;

  @override
  ConsumerState<CaptureProtectionScope> createState() => _CaptureProtectionScopeState();
}

class _CaptureProtectionScopeState extends ConsumerState<CaptureProtectionScope> {
  late final SecretExposure _exposure;
  ProviderSubscription<ScreenCaptureProtection>? _mode;
  bool? _applied;

  @override
  void initState() {
    super.initState();
    if (!WindowCaptureAffinity.supported) return;
    _exposure = (widget.exposure ?? SecretExposure.instance)..addListener(_apply);
    _mode = ref.listenManual(
      localSettingsProvider.select(
        (s) => s.value?.screenCaptureProtection ?? const LocalSettings().screenCaptureProtection,
      ),
      (_, _) => _apply(),
    );
    _apply();
  }

  void _apply() {
    final mode = _mode?.read();
    if (mode == null) return;
    final excluded = excludeFromCapture(mode, secretsVisible: _exposure.visible);
    if (excluded == _applied) return;
    _applied = excluded;
    unawaited(ref.read(windowCaptureAffinityProvider).setExcluded(excluded: excluded));
  }

  @override
  void dispose() {
    if (_mode != null) {
      _exposure.removeListener(_apply);
      _mode?.close();
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => widget.child;
}
