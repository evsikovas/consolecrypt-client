import 'dart:async';
import 'dart:math';

import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/inventory_service.dart';
import 'package:consolecrypt/core/util/value_stream.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/credentials/credentials_screen.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

class _Inventory extends Fake implements InventoryService {
  final secret = Completer<SecretText>();
  int revealCalls = 0;

  @override
  Future<SecretText> revealCredentialSecret(ObjectId credentialId) {
    revealCalls++;
    return secret.future;
  }
}

String _value() => List.generate(32, (_) => Random.secure().nextInt(256)).join('-');

void main() {
  for (final closeDialogFirst in [true, false]) {
    testWidgets('late credential reveal is wiped after ${closeDialogFirst ? 'dialog close' : 'vault lock'}', (
      tester,
    ) async {
      final now = DateTime.now().toUtc();
      final profile = Profile(
        id: ProfileId.generate(),
        name: 'Synthetic workspace',
        kind: ProfileKind.local,
        vaultId: VaultId.generate(),
        createdAt: now,
      );
      final credential = Credential(
        id: ObjectId.generate(),
        name: 'Synthetic credential',
        kind: CredentialKind.password,
        secretId: ObjectId.generate(),
        createdAt: now,
        updatedAt: now,
      );
      final inventory = _Inventory();
      final status = ValueStreamController(VaultStatus(phase: VaultPhase.unlocked, vaultId: profile.vaultId));
      addTearDown(status.close);
      final budget = GlassBackdropBudget();
      addTearDown(budget.dispose);
      tester.view.physicalSize = const Size(1200, 1000);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.reset);
      await tester.pumpWidget(
        ProviderScope(
          retry: (_, _) => null,
          overrides: [
            activeProfileProvider.overrideWithValue(profile),
            vaultStatusProvider.overrideWith((ref) => status.stream),
            inventoryServiceProvider.overrideWithValue(inventory),
          ],
          child: MaterialApp(
            locale: const Locale('en'),
            supportedLocales: AppLocalizations.supportedLocales,
            localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
            theme: AppTheme.build(Brightness.dark, platform: TargetPlatform.windows),
            themeAnimationDuration: Duration.zero,
            builder: (_, child) => GlassScope(
              data: GlassScopeData(appearance: GlassAppearance.fallback, budget: budget),
              child: child!,
            ),
            home: Consumer(
              builder: (context, ref, _) => Scaffold(
                body: Center(
                  child: GlassButton(
                    key: const ValueKey('open-credential'),
                    label: 'Open credential',
                    onPressed: ref.watch(vaultStatusProvider).value?.isUnlocked == true
                        ? () => showAppDialog<void>(
                            context,
                            secure: true,
                            builder: (_) => CredentialDetailsDialog(credential: credential),
                          ).ignore()
                        : null,
                  ),
                ),
              ),
            ),
          ),
        ),
      );
      await settle(tester);
      final scope = ProviderScope.containerOf(tester.element(find.byKey(const ValueKey('open-credential'))));
      expect(scope.read(vaultStatusProvider).value?.isUnlocked, isTrue);
      await tester.tap(find.byKey(const ValueKey('open-credential')));
      await settle(tester);
      expect(find.byType(CredentialDetailsDialog), findsOneWidget);
      expect(find.byType(SecureSurface), findsOneWidget);
      await tester.tap(find.byKey(const ValueKey('reveal-password')));
      await tester.pump();
      expect(inventory.revealCalls, 1);
      expect(inventory.secret.isCompleted, isFalse);
      if (closeDialogFirst) {
        await tester.tap(find.widgetWithText(GlassButton, 'Close'));
      } else {
        status.value = VaultStatus(phase: VaultPhase.locked, vaultId: profile.vaultId);
      }
      await settle(tester);
      expect(find.byType(CredentialDetailsDialog), closeDialogFirst ? findsNothing : findsOneWidget);
      expect(
        identical(scope, ProviderScope.containerOf(tester.element(find.byKey(const ValueKey('open-credential'))))),
        isTrue,
        reason: 'the provider scope survives widget-only disposal and vault locking',
      );
      if (!closeDialogFirst) expect(scope.read(vaultStatusProvider).value?.isUnlocked, isFalse);
      final secret = SecretText(_value());
      inventory.secret.complete(secret);
      await settle(tester);
      expect(secret.isWiped, isTrue);
      expect(inventory.revealCalls, 1);
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox.shrink());
      await settle(tester);
    });
  }
}
