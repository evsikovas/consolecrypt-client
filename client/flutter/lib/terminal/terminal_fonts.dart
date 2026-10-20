import 'package:consolecrypt/app/platform.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

/// Monospace families offered for terminals. On macOS this is the list of
/// installed fixed-pitch fonts (a family that is not installed silently falls
/// back to the default); elsewhere a list of common families.
final terminalFontFamiliesProvider = FutureProvider<List<String>>((ref) async {
  if (AppPlatform.isMacOS) {
    try {
      final families = await const MethodChannel('consolecrypt/fonts').invokeListMethod<String>('monospaceFamilies');
      if (families != null && families.isNotEmpty) return families;
    } on PlatformException {
      // Fall through to the static list.
    } on MissingPluginException {
      // Tests and older runners have no channel.
    }
  }
  return AppPlatform.terminalFontChoices;
});
