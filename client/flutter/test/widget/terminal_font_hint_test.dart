import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/terminal/terminal_fonts.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

class _FakeSource extends TerminalFontSource {
  const _FakeSource();

  @override
  Future<List<String>?> monospaceFamilies() async => const [
    'Consolas',
    'Cascadia Mono',
    'JetBrainsMono Nerd Font Mono',
  ];

  @override
  Future<List<String>?> installedFamilies() async => const ['Consolas', 'Segoe UI', 'JetBrainsMono NFM'];
}

void main() {
  testWidgets('terminal font picker hints when the saved font is not installed', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    final settings = backend.services.settings;
    await settings.updateLocal(settings.currentLocal.copyWith(terminalFontFamily: 'JetBrains Mono'));
    await pumpApp(
      tester,
      backend,
      locale: AppLocale.ru,
      overrides: [terminalFontSourceProvider.overrideWithValue(const _FakeSource())],
    );
    await tapKey(tester, 'nav-settings');
    final picker = find.byKey(const ValueKey('terminal-font-family'));
    await tester.scrollUntilVisible(picker, 200, scrollable: find.byType(Scrollable).last);
    await settle(tester);

    final hint = find.byKey(const ValueKey('terminal-font-missing'));
    expect(hint, findsOneWidget);
    expect(tester.widget<Text>(hint).data, 'Шрифт «JetBrains Mono» не установлен — используется Consolas.');
    final container = ProviderScope.containerOf(tester.element(picker));
    expect(container.read(terminalFontProvider).missing, isTrue);
    expect(container.read(terminalFontCatalogProvider).value?.offered, contains('JetBrainsMono Nerd Font Mono'));

    // The macOS spelling of an installed Nerd Font resolves through its alias.
    await settings.updateLocal(settings.currentLocal.copyWith(terminalFontFamily: 'JetBrainsMono Nerd Font Mono'));
    await settle(tester);
    expect(hint, findsNothing);
    await settings.updateLocal(settings.currentLocal.copyWith(terminalFontFamily: 'JetBrainsMono NF'));
    await settle(tester);
    expect(hint, findsOneWidget);
    await settings.updateLocal(settings.currentLocal.copyWith(terminalFontFamily: 'JetBrainsMono NFM'));
    await settle(tester);
    expect(hint, findsNothing);
    expect(container.read(terminalFontProvider).family, 'JetBrainsMono NFM');
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
}
