import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/automation/automation_service_delegation.dart';
import 'package:consolecrypt/automation/automation_service_delegation_dialog.dart';
import 'package:consolecrypt/core/bridge/rust_automation.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:consolecrypt/src/rust/frb_generated.dart';
import 'package:crypto/crypto.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../support/service_delegation_fixture.dart';
import '../support/service_results_fixture.dart';
import 'automation_service_delegation_test.dart' show FakeDelegation;

final class ResultsService implements AutomationServiceDelegationService, AutomationServiceResultsService {
  final delegate = FakeDelegation(serviceIntent());
  final reads = <AutomationJson>[];
  Completer<AutomationJson>? waiting;
  AutomationJson? reply;
  bool fail = false;
  @override
  Future<AutomationJson> serviceDelegationResults(AutomationJson request) async {
    reads.add(request);
    if (fail) throw StateError('owned unavailable');
    return waiting == null ? reply ?? recordedServiceResults() : await waiting!.future;
  }

  @override
  Future<AutomationJson> prepareServiceDelegation(AutomationJson request) => delegate.prepareServiceDelegation(request);
  @override
  Future<AutomationJson> confirmServiceDelegation(AutomationJson request) => delegate.confirmServiceDelegation(request);
  @override
  Future<AutomationJson> cancelServiceDelegation(AutomationJson request) => delegate.cancelServiceDelegation(request);
  @override
  Future<AutomationJson> serviceDelegationStatus(AutomationJson request) => delegate.serviceDelegationStatus(request);
  @override
  Future<AutomationJson> revokeServiceDelegation(AutomationJson request) => delegate.revokeServiceDelegation(request);
}

// Only the existing generated endpoint is mocked; the production Dart facade
// and strict production parser render untouched bytes from a real Native query.
final class _RecordedResultsApi implements RustLibApi {
  _RecordedResultsApi(this.raw);
  final String raw;
  final requests = <AutomationJson>[];
  @override
  Future<String> crateApiAutomationServiceAutomationServiceResultsJson({required String requestJson}) async {
    requests.add(jsonDecode(requestJson) as AutomationJson);
    return raw;
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => throw UnsupportedError('Unexpected Native endpoint');
}

final scope = serviceIntent().scope;
Widget host(
  AutomationServiceDelegationService service, {
  bool Function()? current,
  Locale locale = const Locale('en'),
  ServiceDelegationScope? ownerScope,
  String runnerId = serviceRunner,
  int runnerRevision = 3,
}) => MaterialApp(
  locale: locale,
  supportedLocales: AppLocalizations.supportedLocales,
  localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
  home: Scaffold(
    body: AutomationServiceDelegationDialog(
      key: const ValueKey('same-original-route'),
      service: service,
      scope: ownerScope ?? scope,
      uiScope: AutomationScope((ownerScope ?? scope).profileId, true),
      runnerId: runnerId,
      runnerRevision: runnerRevision,
      runnerName: 'Owned service',
      original: current ?? () => true,
    ),
  ),
);
Future<void> tap(WidgetTester t, String id) async {
  final f = find.byKey(ValueKey(id));
  await t.ensureVisible(f);
  await t.tap(f);
  await t.pump();
  await t.pump(const Duration(milliseconds: 300));
}

void main() {
  testWidgets(
    'genuine post-exit Results bytes render UNKNOWN partial outcome through the production facade with one explicit read',
    (t) async {
      final bytes = File('test/fixtures/service_results_native.json').readAsBytesSync();
      expect(bytes, hasLength(820));
      expect(sha256.convert(bytes).toString(), '6f8f181196ad77b501b4aa201ec749f5d4b927244d2a9556fce917f22514bffa');
      final raw = utf8.decode(bytes), recorded = jsonDecode(raw) as AutomationJson;
      // Recorded owned fixture boundary only: the response grants no live scope.
      final original = ServiceDelegationScope(
        recorded['profileId'] as String,
        recorded['notificationScopeId'] as String,
      );
      final runner = recorded['runnerId'] as String;
      final api = _RecordedResultsApi(raw);
      RustLib.initMock(api: api);
      addTearDown(RustLib.dispose);
      final service = RustAutomationService();
      expect(api.requests, isEmpty);
      // Actual fixture creates this saved Runner at revision1 and never edits it.
      await t.pumpWidget(host(service, ownerScope: original, runnerId: runner, runnerRevision: 1));
      expect(api.requests, isEmpty);
      await t.pump(const Duration(seconds: 2));
      expect(api.requests, isEmpty);
      await tap(t, 'service-results-read');
      expect(api.requests.single, {...original.frame, 'runnerId': runner, 'expectedRunnerRevision': 1});
      const actualJob = 'fa757d56-18d3-8afc-94fb-d175641a42e2';
      expect(find.byKey(const ValueKey('service-result-$actualJob')), findsOneWidget);
      await tap(t, 'service-result-$actualJob');
      expect(find.text('-15'), findsOneWidget);
      expect(find.text('Unknown outcome'), findsWidgets);
      expect(find.textContaining('changes may be partial'), findsOneWidget);
      expect(find.textContaining('does not prove task success'), findsOneWidget);
      expect(find.textContaining('This does not prove current target state'), findsOneWidget);
      expect(find.text('Succeeded'), findsNothing);
      expect(find.text('Task OK'), findsNothing);
      expect(find.byKey(const ValueKey('service-results-unavailable')), findsNothing);
      await t.pump(const Duration(seconds: 5));
      expect(api.requests, hasLength(1));
      await t.pumpWidget(const SizedBox());
      expect(t.takeException(), isNull);
    },
  );

  testWidgets('explicit one-shot results read shows real job UNKNOWN partial exit without polling or task OK', (
    t,
  ) async {
    final s = ResultsService();
    await t.pumpWidget(host(s));
    expect(s.reads, isEmpty);
    await tap(t, 'service-results-read');
    expect(s.reads.single, {...scope.frame, 'runnerId': serviceRunner, 'expectedRunnerRevision': 3});
    await tap(t, 'service-result-$resultJob');
    expect(find.text('-15'), findsOneWidget);
    expect(find.text('Unknown outcome'), findsWidgets);
    expect(find.textContaining('does not prove task success'), findsOneWidget);
    await t.pump(const Duration(seconds: 5));
    expect(s.reads, hasLength(1));
    expect(s.delegate.confirms, isEmpty);
    expect(s.delegate.prepares, isEmpty);
  });
  testWidgets('unavailable result does not retry and explicit empty reply stays honest', (t) async {
    final s = ResultsService()..fail = true;
    await t.pumpWidget(host(s));
    await tap(t, 'service-results-read');
    expect(find.byKey(const ValueKey('service-results-unavailable')), findsOneWidget);
    await t.pump(const Duration(seconds: 5));
    expect(s.reads, hasLength(1));
    s.fail = false;
    s.reply = recordedServiceResults()..['jobs'] = <Object>[];
    await tap(t, 'service-results-read');
    expect(find.text('No matching terminal jobs were recorded.'), findsOneWidget);
    expect(s.reads, hasLength(2));
  });
  testWidgets('late original-scope loss or service replacement never publishes old results', (t) async {
    var current = true;
    final s = ResultsService()..waiting = Completer<AutomationJson>();
    await t.pumpWidget(host(s, current: () => current));
    await tap(t, 'service-results-read');
    current = false;
    await t.pumpWidget(host(s, current: () => current));
    s.waiting!.complete(recordedServiceResults());
    await t.pumpAndSettle();
    expect(find.byKey(const ValueKey('service-result-$resultJob')), findsNothing);
    await t.pumpWidget(const SizedBox());
    final old = ResultsService()..waiting = Completer<AutomationJson>();
    await t.pumpWidget(host(old));
    await tap(t, 'service-results-read');
    final next = ResultsService();
    await t.pumpWidget(host(next));
    old.waiting!.complete(recordedServiceResults());
    await t.pumpAndSettle();
    expect(find.byKey(const ValueKey('service-result-$resultJob')), findsNothing);
    expect(next.reads, isEmpty);
  });
  testWidgets('old capability has no result button; RU result preserves unknown flags', (t) async {
    await t.pumpWidget(host(FakeDelegation(serviceIntent())));
    expect(find.byKey(const ValueKey('service-results-read')), findsNothing);
    await t.pumpWidget(const SizedBox());
    final s = ResultsService();
    await t.pumpWidget(host(s, locale: const Locale('ru')));
    await tap(t, 'service-results-read');
    await tap(t, 'service-result-$resultJob');
    expect(find.text('Неизвестно'), findsWidgets);
    expect(t.takeException(), isNull);
  });
}
