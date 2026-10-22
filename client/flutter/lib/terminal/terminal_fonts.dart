import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

/// Asks the OS which font families are installed: the runner's
/// `consolecrypt/fonts` channel on macOS and Windows, `fc-list` on Linux.
/// Every method returns null when the OS cannot tell.
class TerminalFontSource {
  const TerminalFontSource();

  static const _channel = MethodChannel('consolecrypt/fonts');

  /// Installed fixed-pitch families, as offered in the terminal font picker.
  Future<List<String>?> monospaceFamilies() async {
    if (AppPlatform.isMacOS || AppPlatform.isWindows) return _native('monospaceFamilies');
    if (AppPlatform.isLinux) return _fcList(':spacing=mono', allNames: false);
    return null;
  }

  /// Every installed family name, to tell a missing font from one that is
  /// installed but not fixed-pitch.
  Future<List<String>?> installedFamilies() async {
    if (AppPlatform.isMacOS || AppPlatform.isWindows) return _native('installedFamilies');
    if (AppPlatform.isLinux) return _fcList(':', allNames: true);
    return null;
  }

  static Future<List<String>?> _native(String method) async {
    try {
      return await _channel.invokeListMethod<String>(method);
    } on PlatformException {
      return null;
    } on MissingPluginException {
      // Tests and older runners have no channel.
      return null;
    }
  }

  static Future<List<String>?> _fcList(String pattern, {required bool allNames}) async {
    try {
      final result = await Process.run('fc-list', [pattern, 'family']);
      if (result.exitCode != 0) return null;
      return parseFcListFamilies(result.stdout as String, allNames: allNames);
    } on Object {
      // No fontconfig: fall back to the static list.
      return null;
    }
  }
}

/// Parses `fc-list … family` output: one font per line, its localized family
/// names separated by unescaped commas (`\,` `\-` `\\` are escapes). Returns
/// the first name of each line, or every name with [allNames], sorted and
/// deduplicated case-insensitively.
List<String> parseFcListFamilies(String output, {bool allNames = false}) {
  final byKey = <String, String>{};
  for (final line in const LineSplitter().convert(output)) {
    final names = <String>[];
    final current = StringBuffer();
    var escaped = false;
    for (final char in line.split('')) {
      if (escaped) {
        current.write(char);
        escaped = false;
      } else if (char == r'\') {
        escaped = true;
      } else if (char == ',') {
        names.add(current.toString());
        current.clear();
      } else {
        current.write(char);
      }
    }
    names.add(current.toString());
    for (final name in allNames ? names : names.take(1)) {
      final trimmed = name.trim();
      if (trimmed.isNotEmpty) byKey.putIfAbsent(trimmed.toLowerCase(), () => trimmed);
    }
  }
  return byKey.values.toList()..sort((a, b) => a.toLowerCase().compareTo(b.toLowerCase()));
}

/// The fonts the OS reports: [offered] fills the picker; [isInstalled]
/// answers whether a family resolves.
class TerminalFontCatalog {
  TerminalFontCatalog({required this.offered, Iterable<String> installed = const [], this.complete = false})
    : _known = {
        for (final name in [...offered, ...installed]) name.toLowerCase(): name,
      };

  /// Picker entries: installed fixed-pitch families, or a static guess list.
  final List<String> offered;

  /// Whether the OS listed every installed family, so a name it did not list
  /// is really missing (otherwise nothing is reported missing).
  final bool complete;

  final Map<String, String> _known;

  /// The listed spelling of [family] (matching ignores case), or null.
  String? installedName(String family) => _known[family.toLowerCase()];
}

final terminalFontSourceProvider = Provider<TerminalFontSource>((ref) => const TerminalFontSource());

/// Installed fonts, read once per run; the static [AppPlatform.terminalFontChoices]
/// are offered when the OS cannot list them.
final terminalFontCatalogProvider = FutureProvider<TerminalFontCatalog>((ref) async {
  final source = ref.watch(terminalFontSourceProvider);
  final results = await Future.wait([source.monospaceFamilies(), source.installedFamilies()]);
  final monospace = results[0] ?? const <String>[];
  final installed = results[1] ?? const <String>[];
  return TerminalFontCatalog(
    offered: monospace.isNotEmpty ? monospace : AppPlatform.terminalFontChoices,
    installed: [...monospace, ...installed],
    complete: installed.isNotEmpty,
  );
});

const _nerdFontSuffixes = [
  (' Nerd Font Mono', ' NFM'), // l10n-ignore: font family name
  (' Nerd Font Propo', ' NFP'), // l10n-ignore: font family name
  (' Nerd Font', ' NF'), // l10n-ignore: font family name
];

/// Other names of the same font: Nerd Fonts register a long family name
/// ("JetBrainsMono Nerd Font Mono") and a short legacy one ("JetBrainsMono
/// NFM"), and an OS may know only one of them.
List<String> terminalFontAliases(String family) {
  final lower = family.toLowerCase();
  for (final (long, short) in _nerdFontSuffixes) {
    if (lower.endsWith(long.toLowerCase())) return ['${family.substring(0, family.length - long.length)}$short'];
    if (lower.endsWith(short.toLowerCase())) return ['${family.substring(0, family.length - short.length)}$long'];
  }
  return const [];
}

/// The terminal font to render, resolved from the saved family.
class TerminalFont {
  const TerminalFont({required this.family, this.requested, this.missing = false});

  /// Family for [TextStyle.fontFamily].
  final String family;

  /// The saved family, null for the system default.
  final String? requested;

  /// The saved family and its aliases are not installed; [fallback] renders.
  final bool missing;

  /// Families tried when [family] lacks a glyph or is not installed.
  List<String> get fallback => [AppPlatform.monospaceFamily, ...AppPlatform.monospaceFallback];

  /// The family that renders instead of a [missing] one.
  String get fallbackFamily => AppPlatform.monospaceFamily;
}

/// Resolves [requested] against [catalog] (null while loading): the family
/// itself when installed, else an installed alias, else [TerminalFont.missing].
TerminalFont resolveTerminalFont(String? requested, TerminalFontCatalog? catalog) {
  final name = requested?.trim() ?? '';
  if (name.isEmpty) return TerminalFont(family: AppPlatform.monospaceFamily);
  if (catalog == null || catalog.installedName(name) != null) return TerminalFont(family: name, requested: name);
  for (final alias in terminalFontAliases(name)) {
    final installed = catalog.installedName(alias);
    if (installed != null) return TerminalFont(family: installed, requested: name);
  }
  return TerminalFont(family: name, requested: name, missing: catalog.complete);
}

/// The one place terminals, previews and other terminal-font users get
/// their font from.
final terminalFontProvider = Provider<TerminalFont>((ref) {
  final requested = ref.watch(localSettingsProvider.select((s) => s.value?.terminalFontFamily));
  return resolveTerminalFont(requested, ref.watch(terminalFontCatalogProvider).value);
});
