import 'package:consolecrypt/account/profile_switcher.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/services/profile_service.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';

class _Profiles extends Fake implements ProfileService {
  bool stay = true;
  int calls = 0;
  @override
  Future<void> switchTo(ProfileId id) async {
    calls++;
    if (stay) throw const AppException(AppErrorCode.cancelled, 'Synthetic Stay');
  }
}

void main() {
  testWidgets('Stay preserves the current route, while a successful switch opens the target', (tester) async {
    final profiles = _Profiles();
    final target = Profile(
      id: const ProfileId('target'),
      name: 'Target',
      kind: ProfileKind.local,
      createdAt: DateTime.utc(2026),
    );
    final router = GoRouter(
      initialLocation: '/original',
      routes: [
        GoRoute(
          path: '/original',
          builder: (context, state) => Scaffold(
            body: Consumer(
              builder: (context, ref, _) =>
                  TextButton(onPressed: () => switchProfile(context, ref, target), child: const Text('Switch fixture')),
            ),
          ),
        ),
        GoRoute(
          path: AppRoutes.hosts,
          builder: (_, _) => const Scaffold(body: Text('Hosts fixture')),
        ),
      ],
    );
    addTearDown(router.dispose);
    await tester.pumpWidget(
      ProviderScope(
        overrides: [profileServiceProvider.overrideWithValue(profiles)],
        child: MaterialApp.router(
          routerConfig: router,
          localizationsDelegates: AppLocalizations.localizationsDelegates,
          supportedLocales: AppLocalizations.supportedLocales,
          locale: const Locale('en'),
        ),
      ),
    );
    await tester.pumpAndSettle();
    await tester.tap(find.text('Switch fixture'));
    await tester.pumpAndSettle();
    expect(router.routeInformationProvider.value.uri.path, '/original');
    expect(find.byType(SnackBar), findsNothing);
    profiles.stay = false;
    await tester.tap(find.text('Switch fixture'));
    await tester.pumpAndSettle();
    expect(router.routeInformationProvider.value.uri.path, AppRoutes.hosts);
    expect(profiles.calls, 2);
    expect(tester.takeException(), isNull);
  });
}
