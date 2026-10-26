import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/inventory_service.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/credentials/credential_dialogs.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

/// Synthetic test key text (not a real key).
const _keyText = '-----BEGIN OPENSSH PRIVATE KEY-----\nsynthetic\n-----END OPENSSH PRIVATE KEY-----\n';

class _Inventory extends Fake implements InventoryService {
  SecretText? picked;
  bool cancel = false;
  final inspected = <String>[];

  @override
  Future<SecretText?> pickPrivateKeyFile() async {
    if (cancel) return null;
    return picked = SecretText(_keyText);
  }

  @override
  Future<KeyInspection> inspectPrivateKey(SecretText privateKey) async {
    inspected.add(privateKey.expose());
    return const KeyInspection(valid: true, algorithm: KeyAlgorithm.ed25519, encrypted: false);
  }
}

Future<void> _open(WidgetTester tester, _Inventory inventory) async {
  final budget = GlassBackdropBudget();
  addTearDown(budget.dispose);
  tester.view.physicalSize = const Size(1200, 1000);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(
    ProviderScope(
      retry: (_, _) => null,
      overrides: [inventoryServiceProvider.overrideWithValue(inventory)],
      child: MaterialApp(
        locale: const Locale('en'),
        supportedLocales: AppLocalizations.supportedLocales,
        localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
        theme: AppTheme.build(Brightness.dark, platform: TargetPlatform.macOS),
        themeAnimationDuration: Duration.zero,
        builder: (_, child) => GlassScope(
          data: GlassScopeData(appearance: GlassAppearance.fallback, budget: budget),
          child: child!,
        ),
        home: Builder(
          builder: (context) => Scaffold(
            body: Center(
              child: GlassButton(
                key: const ValueKey('open-import'),
                label: 'Open',
                onPressed: () =>
                    showAppDialog<void>(context, secure: true, builder: (_) => const ImportKeyDialog()).ignore(),
              ),
            ),
          ),
        ),
      ),
    ),
  );
  await settle(tester);
  await tester.tap(find.byKey(const ValueKey('open-import')));
  await settle(tester);
}

void main() {
  testWidgets('TODO-04: "Choose file…" fills the key field, inspects it and wipes the read copy', (tester) async {
    final inventory = _Inventory();
    await _open(tester, inventory);
    expect(find.byTooltip('Choose file…'), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('private-key-choose-file')));
    await settle(tester);
    await tester.pump(const Duration(milliseconds: 300));
    await settle(tester);
    final field = tester.widget<TextField>(find.byKey(const ValueKey('private-key-input')));
    expect(field.controller!.text, _keyText);
    expect(inventory.picked!.isWiped, isTrue, reason: 'the copy returned by the service is wiped');
    expect(inventory.inspected, contains(_keyText));
    await tester.pumpWidget(const SizedBox.shrink());
    await settle(tester);
  });

  testWidgets('TODO-04: cancelling the file dialog leaves the key field alone', (tester) async {
    final inventory = _Inventory()..cancel = true;
    await _open(tester, inventory);
    await tester.tap(find.byKey(const ValueKey('private-key-choose-file')));
    await settle(tester);
    final field = tester.widget<TextField>(find.byKey(const ValueKey('private-key-input')));
    expect(field.controller!.text, isEmpty);
    expect(inventory.inspected, isEmpty);
    await tester.pumpWidget(const SizedBox.shrink());
    await settle(tester);
  });
}
