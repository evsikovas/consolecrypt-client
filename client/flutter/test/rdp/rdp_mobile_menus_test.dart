import 'package:consolecrypt/app/app.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/models/rdp_mobile.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/rdp/rdp_fullscreen.dart';
import 'package:consolecrypt/rdp/rdp_providers.dart';
import 'package:consolecrypt/rdp/rdp_screen.dart';
import 'package:consolecrypt/rdp/rdp_service.dart';
import 'package:consolecrypt/rdp/rdp_window.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';
import 'fake_rdp_service.dart';

class _Window extends RdpWindow {
  @override
  bool get desktop => false;
  @override
  Future<void> enter() async {}
  @override
  Future<void> leave() async {}
}

void main() {
  for (final overlay in ['rdp-mobile-keys', 'rdp-mobile-help']) {
    testWidgets('mobile $overlay opens and disappears on lock', (tester) async {
      final backend = testBackend();
      addTearDown(backend.dispose);
      await backend.debugSignInDemoAndUnlock();
      setTestLocale(backend, AppLocale.en);
      await backend.settings.updateLocal(
        backend.settings.currentLocal.copyWith(rdpMobile: const RdpMobileSettings(hideIntroduction: true)),
      );
      final service = FakeRdpService();
      tester.view.physicalSize = const Size(390, 844);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.reset);
      await tester.pumpWidget(
        ProviderScope(
          overrides: [
            appServicesProvider.overrideWithValue(backend.services),
            rdpServiceProvider.overrideWithValue(service),
            rdpWindowProvider.overrideWithValue(_Window()),
          ],
          child: const ConsoleCryptApp(),
        ),
      );
      await settle(tester);
      final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
      container.read(routerProvider).go(AppRoutes.rdp);
      await settle(tester);
      showAppDialog<void>(
        tester.element(find.byType(RdpScreen)),
        secure: true,
        builder: (_) => RdpConnectionDialog(scope: container.read(rdpScopeProvider)),
      ).ignore();
      await settle(tester);
      await enterKey(tester, 'rdp-address', 'example.test');
      await enterKey(tester, 'rdp-username', 'demo');
      await enterKey(tester, 'rdp-password', 'synthetic-fixture-only');
      await tapKey(tester, 'rdp-connect-submit');
      await tapKey(tester, 'rdp-certificate-checked');
      await tapKey(tester, 'rdp-certificate-accept');
      expect(find.byType(RdpFullscreen), findsOneWidget);
      await tapKey(tester, 'rdp-mobile-menu');
      await tapKey(tester, overlay);
      expect(tester.takeException(), isNull);
      expect(find.text(overlay == 'rdp-mobile-keys' ? 'F12' : 'Phone controls'), findsOneWidget);
      expect(
        service.inputs.expand((e) => e).whereType<RdpPointerInput>(),
        isEmpty,
        reason: 'menu presses never click the remote desktop',
      );
      await backend.vault.lock();
      await settle(tester);
      expect(find.byType(RdpFullscreen), findsNothing);
      expect(find.text('F12'), findsNothing);
      expect(find.text('Phone controls'), findsNothing);
      // The lock screen replaces the desktop; the session survives (BUG-01).
      expect(service.disconnected, isEmpty);
      expect(tester.takeException(), isNull);
    }, variant: const TargetPlatformVariant({TargetPlatform.android, TargetPlatform.iOS}));
  }
}
