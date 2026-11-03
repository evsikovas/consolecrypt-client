import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/mock/mock_docker.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/docker/docker_forwards.dart';
import 'package:consolecrypt/docker/docker_providers.dart';
import 'package:consolecrypt/tunnels/saved_forwards.dart';
import 'package:consolecrypt/tunnels/tunnels_screen.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

void main() {
  testWidgets('Docker checkbox saves a preset that can be restarted from Tunnels', (tester) async {
    final backend = testBackend(), docker = MockDockerService();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    final session = await docker.connect('local');
    final target = (await docker.containers(session.id)).firstWhere((c) => c.name == 'shop-postgres-1');
    final tunnels = ValueNotifier(false);
    addTearDown(tunnels.dispose);
    tester.view.physicalSize = const Size(1400, 1000);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          appServicesProvider.overrideWithValue(backend.services),
          activeProfileProvider.overrideWithValue(backend.profiles.currentProfiles.active),
          dockerServiceProvider.overrideWithValue(docker),
          dockerScopeProvider.overrideWithValue(const DockerScope('profile', true)),
        ],
        child: MaterialApp(
          locale: const Locale('en'),
          localizationsDelegates: [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
          supportedLocales: AppLocalizations.supportedLocales,
          home: Scaffold(
            body: ValueListenableBuilder(
              valueListenable: tunnels,
              builder: (context, value, _) => value
                  ? const TunnelsScreen()
                  : TextButton(
                      onPressed: () =>
                          showDockerForwardDialog(context, service: docker, session: session, container: target),
                      child: const Text('Forward'),
                    ),
            ),
          ),
        ),
      ),
    );
    await settle(tester);
    await tester.tap(find.text('Forward'));
    await settle(tester);
    final checkbox = find.byKey(const ValueKey('docker-forward-save'));
    expect(tester.widget<CheckboxListTile>(checkbox).value, isFalse);
    await tester.tap(checkbox);
    await tester.tap(find.byKey(const ValueKey('docker-forward-start')));
    await settle(tester);
    expect(backend.services.settings.currentLocal.savedForwards, hasLength(1));
    final plan = backend.services.settings.currentLocal.savedForwards.single;
    expect(plan.localPort, 0);
    expect(plan.remotePort, 5432);
    expect(plan.target, 'shop-postgres-1');
    await tester.tap(find.byKey(const ValueKey('docker-forward-done')));
    await settle(tester);
    // Leaving the Docker browser closes its original session. A saved preset
    // must reconnect rather than depend on that runtime handle.
    await docker.disconnect(session.id);
    final scope = ProviderScope.containerOf(tester.element(find.byType(Scaffold).first));
    await scope.read(savedForwardManagerProvider).refresh();
    tunnels.value = true;
    await settle(tester);
    expect(find.byKey(ValueKey('saved-forward-${plan.id}')), findsOneWidget);
    await tester.tap(find.byKey(ValueKey('saved-forward-toggle-${plan.id}')));
    await settle(tester);
    expect(await docker.forwards(), hasLength(1));
    expect(find.textContaining('127.0.0.1:'), findsWidgets);
    await tester.tap(find.byKey(ValueKey('saved-forward-toggle-${plan.id}')));
    await settle(tester);
    expect(await docker.forwards(), isEmpty);
    expect(backend.services.settings.currentLocal.savedForwards, hasLength(1));
    await tester.tap(find.byKey(ValueKey('saved-forward-remove-${plan.id}')));
    await settle(tester);
    await tester.tap(find.text('Delete').last);
    await settle(tester);
    expect(backend.services.settings.currentLocal.savedForwards, isEmpty);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox.shrink());
  });
}
