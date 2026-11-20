import 'dart:async';

import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/automation/automation_service_delegation.dart';
import 'package:consolecrypt/automation/automation_service_delegation_dialog.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../support/service_delegation_fixture.dart';

final class FakeDelegation implements AutomationServiceDelegationService {
  FakeDelegation(this.intent);
  final ServiceDelegationIntent intent;
  final prepares = <AutomationJson>[],
      confirms = <AutomationJson>[],
      cancels = <AutomationJson>[],
      statuses = <AutomationJson>[],
      revokes = <AutomationJson>[];
  Completer<AutomationJson>? pendingPrepare, pendingConfirm;
  bool loseConfirm = false;
  @override
  Future<AutomationJson> prepareServiceDelegation(
    AutomationJson request,
  ) async {
    prepares.add(request);
    return pendingPrepare == null
        ? servicePreview(intent)
        : await pendingPrepare!.future;
  }

  @override
  Future<AutomationJson> confirmServiceDelegation(
    AutomationJson request,
  ) async {
    confirms.add(request);
    if (loseConfirm) throw StateError('synthetic transport uncertainty');
    return pendingConfirm == null
        ? serviceStatus(intent.scope)
        : await pendingConfirm!.future;
  }

  @override
  Future<AutomationJson> cancelServiceDelegation(AutomationJson request) async {
    cancels.add(request);
    return {
      ...intent.scope.frame,
      'reviewId': serviceReview,
      'state': 'cancelRequested',
      'remoteState': 'unknown',
    };
  }

  @override
  Future<AutomationJson> serviceDelegationStatus(AutomationJson request) async {
    statuses.add(request);
    return serviceStatus(
      intent.scope,
      local: 'deliveryUnknown',
      remote: 'enrolled',
    );
  }

  @override
  Future<AutomationJson> revokeServiceDelegation(AutomationJson request) async {
    revokes.add(request);
    return serviceStatus(
      intent.scope,
      local: 'localBlocked',
      remote: 'unknown',
    );
  }
}

Widget host(
  FakeDelegation service, {
  bool Function()? current,
  ServiceDelegationIntent? intent,
  Locale locale = const Locale('en'),
  double scale = 1,
  double keyboard = 0,
}) {
  final selected = intent ?? service.intent;
  return MaterialApp(
    locale: locale,
    supportedLocales: AppLocalizations.supportedLocales,
    localizationsDelegates: const [
      AppLocalizations.delegate,
      ...GlobalMaterialLocalizations.delegates,
    ],
    builder: (context, child) => MediaQuery(
      data: MediaQuery.of(context).copyWith(
        textScaler: TextScaler.linear(scale),
        viewInsets: EdgeInsets.only(bottom: keyboard),
      ),
      child: child!,
    ),
    home: Scaffold(
      body: AutomationServiceDelegationDialog(
        key: const ValueKey('owned-service-dialog'),
        service: service,
        scope: selected.scope,
        uiScope: const AutomationScope(serviceProfile, true),
        runnerId: serviceRunner,
        runnerRevision: 3,
        runnerName: 'Owned service',
        intent: selected,
        original: current ?? () => true,
      ),
    ),
  );
}

Future<void> tap(WidgetTester tester, String id) async {
  final target = find.byKey(ValueKey(id));
  await tester.ensureVisible(target);
  await tester.pumpAndSettle();
  await tester.tap(target.hitTestable());
  await tester.pumpAndSettle();
}

void main() {
  testWidgets(
    'opening sends nothing; exact Native review stays secret-free and explicit confirm is one-use',
    (tester) async {
      final service = FakeDelegation(serviceIntent());
      await tester.pumpWidget(host(service));
      await tester.pumpAndSettle();
      expect(service.prepares, isEmpty);
      expect(service.statuses, isEmpty);
      expect(service.revokes, isEmpty);
      await tap(tester, 'service-prepare');
      expect(service.prepares, hasLength(1));
      expect(service.confirms, isEmpty);
      expect(find.text('PRIVATE_EXTRA_VALUE'), findsNothing);
      expect(find.text(serviceCredential), findsNothing);
      service.pendingConfirm = Completer();
      final confirm = find.byKey(const ValueKey('service-confirm'));
      await tester.ensureVisible(confirm);
      await tester.tap(confirm);
      await tester.tap(confirm);
      await tester.pump();
      expect(service.confirms, hasLength(1));
      expect(service.confirms.single.keys.toSet(), {
        'version',
        'profileId',
        'notificationScopeId',
        'reviewId',
        'policyId',
      });
      service.pendingConfirm!.complete(serviceStatus(service.intent.scope));
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('service-confirm')), findsNothing);
    },
  );
  testWidgets(
    'late original prepare is cancelled on original owner after service replacement',
    (tester) async {
      final old = FakeDelegation(serviceIntent())..pendingPrepare = Completer();
      await tester.pumpWidget(host(old));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('service-prepare')));
      await tester.pump();
      final replacement = FakeDelegation(old.intent);
      await tester.pumpWidget(host(replacement));
      await tester.pump();
      old.pendingPrepare!.complete(servicePreview(old.intent));
      await tester.pumpAndSettle();
      expect(old.cancels, hasLength(1));
      expect(
        old.cancels.single['notificationScopeId'],
        old.intent.scope.notificationScopeId,
      );
      expect(replacement.prepares, isEmpty);
      expect(replacement.cancels, isEmpty);
      expect(replacement.confirms, isEmpty);
      expect(find.byKey(const ValueKey('service-confirm')), findsNothing);
    },
  );
  testWidgets('unknown confirmation never re-arms or automatically repeats', (
    tester,
  ) async {
    final service = FakeDelegation(serviceIntent())..loseConfirm = true;
    await tester.pumpWidget(host(service));
    await tester.pumpAndSettle();
    await tap(tester, 'service-prepare');
    await tap(tester, 'service-confirm');
    expect(service.confirms, hasLength(1));
    expect(
      find.byKey(const ValueKey('service-delivery-unknown')),
      findsOneWidget,
    );
    expect(find.byKey(const ValueKey('service-confirm')), findsNothing);
    expect(find.byKey(const ValueKey('service-prepare')), findsNothing);
    await tester.pump(const Duration(seconds: 3));
    expect(service.confirms, hasLength(1));
    expect(service.statuses, isEmpty);
  });
  testWidgets(
    'status and offline revoke are explicit and do not depend on connection or run permission',
    (tester) async {
      final service = FakeDelegation(serviceIntent());
      await tester.pumpWidget(host(service));
      await tester.pumpAndSettle();
      await tap(tester, 'service-status-read');
      expect(service.statuses, hasLength(1));
      expect(service.prepares, isEmpty);
      await tap(tester, 'service-revoke');
      expect(service.revokes, isEmpty);
      await tap(tester, 'service-revoke-confirm');
      expect(service.revokes, hasLength(1));
      expect(service.revokes.single, {
        'version': 1,
        'profileId': serviceProfile,
        'notificationScopeId': service.intent.scope.notificationScopeId,
        'runnerId': serviceRunner,
        'expectedRunnerRevision': 3,
      });
      expect(service.confirms, isEmpty);
      expect(find.byKey(const ValueKey('service-prepare')), findsNothing);
    },
  );
  testWidgets(
    'actual original-lifetime loss denies late review without another read',
    (tester) async {
      var original = true;
      final service = FakeDelegation(serviceIntent())
        ..pendingPrepare = Completer();
      await tester.pumpWidget(host(service, current: () => original));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('service-prepare')));
      await tester.pump();
      original = false;
      await tester.pumpWidget(const SizedBox.shrink());
      service.pendingPrepare!.complete(servicePreview(service.intent));
      await tester.pumpAndSettle();
      expect(service.cancels, hasLength(1));
      expect(service.confirms, isEmpty);
      expect(service.prepares, hasLength(1));
    },
  );
  testWidgets(
    'RU narrow keyboard layout keeps explicit prepare and status accessible',
    (tester) async {
      tester.view.physicalSize = const Size(420, 760);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);
      final service = FakeDelegation(serviceIntent());
      await tester.pumpWidget(
        host(service, locale: const Locale('ru'), scale: 1.5, keyboard: 220),
      );
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull);
      await tap(tester, 'service-prepare');
      expect(service.prepares, hasLength(1));
      expect(find.byKey(const ValueKey('service-confirm')), findsOneWidget);
      expect(tester.takeException(), isNull);
      await tap(tester, 'service-status-read');
      expect(service.statuses, hasLength(1));
      expect(tester.takeException(), isNull);
    },
  );
}
