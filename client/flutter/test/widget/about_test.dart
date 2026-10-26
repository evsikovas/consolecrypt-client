import 'package:consolecrypt/app/about.dart';
import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/mock/mock_backend.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

Future<void> _pump(WidgetTester tester, MockBackend backend) async {
  tester.view.physicalSize = const Size(1000, 900);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(
    ProviderScope(
      overrides: [appServicesProvider.overrideWithValue(backend.services)],
      child: const MaterialApp(
        locale: Locale('en'),
        localizationsDelegates: [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
        supportedLocales: AppLocalizations.supportedLocales,
        home: Scaffold(body: AboutConsoleCryptContent()),
      ),
    ),
  );
  await settle(tester);
}

void main() {
  testWidgets('a synced profile shows where its server offers its source code (AGPL)', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await _pump(tester, backend);
    expect(find.text('Source code of your sync server:'), findsOneWidget);
    expect(find.text('https://github.com/evsikovas/consolecrypt-server'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('a local profile has no server source row', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugCreateUnlockedLocalProfile();
    await _pump(tester, backend);
    expect(find.byKey(const ValueKey('about-server-source')), findsNothing);
    expect(find.byKey(const ValueKey('about-source')), findsOneWidget);
  });

  test('only an http(s) URL with a host is shown', () {
    ServerInfo info(Object? url) => serverInfoFromJson({'source_code_url': url});
    expect(info('https://example.org/src').sourceCodeUrl, 'https://example.org/src');
    expect(info('javascript:alert(1)').sourceCodeUrl, isNull);
    expect(info('file:///etc/passwd').sourceCodeUrl, isNull);
    expect(info(42).sourceCodeUrl, isNull);
    expect(info(null).sourceCodeUrl, isNull);
  });
}
