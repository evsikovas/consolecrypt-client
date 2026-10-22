import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/terminal/terminal_fonts.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

class _FakeSource extends TerminalFontSource {
  const _FakeSource({this.monospace, this.installed});
  final List<String>? monospace;
  final List<String>? installed;

  @override
  Future<List<String>?> monospaceFamilies() async => monospace;

  @override
  Future<List<String>?> installedFamilies() async => installed;
}

Future<TerminalFontCatalog> _catalog(TerminalFontSource source) {
  final container = ProviderContainer(overrides: [terminalFontSourceProvider.overrideWithValue(source)]);
  addTearDown(container.dispose);
  return container.read(terminalFontCatalogProvider.future);
}

void main() {
  tearDown(() => debugDefaultTargetPlatformOverride = null);

  group('terminalFontAliases', () {
    test('maps long Nerd Font names to the short legacy ones and back', () {
      expect(terminalFontAliases('JetBrainsMono Nerd Font Mono'), ['JetBrainsMono NFM']);
      expect(terminalFontAliases('JetBrainsMono NFM'), ['JetBrainsMono Nerd Font Mono']);
      expect(terminalFontAliases('JetBrainsMono Nerd Font Propo'), ['JetBrainsMono NFP']);
      expect(terminalFontAliases('JetBrainsMonoNL NFP'), ['JetBrainsMonoNL Nerd Font Propo']);
      expect(terminalFontAliases('Hack Nerd Font'), ['Hack NF']);
      expect(terminalFontAliases('Hack NF'), ['Hack Nerd Font']);
      expect(terminalFontAliases('hack nerd font mono'), ['hack NFM']);
    });

    test('has no aliases for other fonts', () {
      expect(terminalFontAliases('Consolas'), isEmpty);
      expect(terminalFontAliases('JetBrains Mono'), isEmpty);
    });
  });

  group('resolveTerminalFont', () {
    final catalog = TerminalFontCatalog(
      offered: const ['Consolas', 'Cascadia Mono', 'JetBrainsMono Nerd Font Mono'],
      installed: const ['Segoe UI', 'JetBrainsMono NFM', 'Fira Code NF'],
      complete: true,
    );

    setUp(() => debugDefaultTargetPlatformOverride = TargetPlatform.windows);

    test('no saved family uses the platform default', () {
      final font = resolveTerminalFont(null, catalog);
      expect(font.family, 'Consolas');
      expect(font.missing, isFalse);
      expect(font.fallback.first, 'Consolas');
    });

    test('an installed family is used as saved, ignoring case', () {
      expect(resolveTerminalFont('Cascadia Mono', catalog).family, 'Cascadia Mono');
      expect(resolveTerminalFont('cascadia mono', catalog).missing, isFalse);
      // Installed but not fixed-pitch: still not reported missing.
      expect(resolveTerminalFont('Segoe UI', catalog).missing, isFalse);
    });

    test('a missing Nerd Font name falls back to its installed alias', () {
      final font = resolveTerminalFont('Fira Code Nerd Font', catalog);
      expect(font.family, 'Fira Code NF');
      expect(font.requested, 'Fira Code Nerd Font');
      expect(font.missing, isFalse);
      expect(resolveTerminalFont('JetBrainsMono NFM', catalog).family, 'JetBrainsMono NFM');
    });

    test('a missing font without an installed alias is reported', () {
      final font = resolveTerminalFont('JetBrains Mono', catalog);
      expect(font.missing, isTrue);
      expect(font.requested, 'JetBrains Mono');
      expect(font.fallbackFamily, 'Consolas');
      expect(resolveTerminalFont('Hack Nerd Font Mono', catalog).missing, isTrue);
    });

    test('nothing is reported missing while loading or when the OS cannot list every family', () {
      expect(resolveTerminalFont('JetBrains Mono', null).missing, isFalse);
      expect(resolveTerminalFont('JetBrains Mono', null).family, 'JetBrains Mono');
      final partial = TerminalFontCatalog(offered: const ['Consolas']);
      expect(resolveTerminalFont('JetBrains Mono', partial).missing, isFalse);
    });
  });

  group('terminalFontCatalogProvider', () {
    test('offers the installed fixed-pitch families', () async {
      final catalog = await _catalog(
        const _FakeSource(monospace: ['Consolas', 'Ubuntu Mono'], installed: ['Consolas', 'Ubuntu Mono', 'Arial']),
      );
      expect(catalog.offered, ['Consolas', 'Ubuntu Mono']);
      expect(catalog.complete, isTrue);
      expect(catalog.installedName('arial'), 'Arial');
      expect(catalog.installedName('JetBrains Mono'), isNull);
    });

    test('falls back to the static list when the OS cannot list fonts', () async {
      debugDefaultTargetPlatformOverride = TargetPlatform.windows;
      final catalog = await _catalog(const _FakeSource());
      expect(catalog.offered, AppPlatform.terminalFontChoices);
      expect(catalog.complete, isFalse);
      expect(resolveTerminalFont('JetBrains Mono', catalog).missing, isFalse);

      final empty = await _catalog(const _FakeSource(monospace: [], installed: []));
      expect(empty.offered, AppPlatform.terminalFontChoices);
    });

    test('the real source has no channel in tests and falls back', () async {
      debugDefaultTargetPlatformOverride = TargetPlatform.windows;
      TestWidgetsFlutterBinding.ensureInitialized();
      final catalog = await _catalog(const TerminalFontSource());
      expect(catalog.offered, AppPlatform.terminalFontChoices);
      expect(catalog.complete, isFalse);
    });
  });

  test('parses fc-list output with localized names and escapes', () {
    const output =
        'DejaVu Sans Mono\n'
        'Noto Sans Mono,Noto Sans Mono Light\n'
        r'Font\-Name\, Inc,Other'
        '\n'
        'dejavu sans mono\n'
        '\n';
    expect(parseFcListFamilies(output), ['DejaVu Sans Mono', 'Font-Name, Inc', 'Noto Sans Mono']);
    expect(parseFcListFamilies(output, allNames: true), [
      'DejaVu Sans Mono',
      'Font-Name, Inc',
      'Noto Sans Mono',
      'Noto Sans Mono Light',
      'Other',
    ]);
  });
}
