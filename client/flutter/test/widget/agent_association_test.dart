import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:consolecrypt/monitoring/agent_association_dialog.dart';
import 'package:consolecrypt/monitoring/monitoring_service.dart';
import 'package:consolecrypt/monitoring/monitoring_workspace.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/monitoring_fakes.dart';
import '../support/agent_association_fixture.dart' as f;

Widget host(
  f.Port port, {
  bool Function()? original,
  Locale locale = const Locale('en'),
  double scale = 1,
}) => MaterialApp(
  locale: locale,
  supportedLocales: AppLocalizations.supportedLocales,
  localizationsDelegates: const [
    AppLocalizations.delegate,
    ...GlobalMaterialLocalizations.delegates,
  ],
  builder: (context, child) => MediaQuery(
    data: MediaQuery.of(context).copyWith(textScaler: TextScaler.linear(scale)),
    child: child!,
  ),
  home: Scaffold(
    body: AgentAssociationDialog(
      key: const ValueKey('same-owner-route'),
      port: port,
      scope: f.scope,
      original: original ?? () => true,
    ),
  ),
);
Future<void> tap(WidgetTester t, String key) async {
  final finder = find.byKey(ValueKey(key));
  await t.ensureVisible(finder);
  await t.tap(finder);
  await t.pump();
  await t.pump(const Duration(milliseconds: 300));
}

Future<void> choose(WidgetTester t) async {
  await tap(t, 'agent-association-status');
  final picker = t.widget<GlassSelect<String?>>(
    find.byKey(const ValueKey('agent-association-host')),
  );
  picker.onChanged!(f.hostId);
  await t.pump();
}

void main() {
  testWidgets(
    'existing dialog requires explicit status probe and two approvals; no tunnel access claim',
    (t) async {
      final p = f.Port();
      await t.pumpWidget(host(p));
      expect(p.statuses, 0);
      expect(p.registrations, 0);
      await choose(t);
      await tap(t, 'agent-association-probe');
      expect(p.issued.offers, 0);
      expect(find.text('18446744073709551615'), findsOneWidget);
      await tap(t, 'agent-association-approve');
      expect(p.issued.offers, 1);
      expect(p.issued.certificates, 0);
      await tap(t, 'agent-association-approve');
      expect(
        find.byKey(const ValueKey('agent-association-associated')),
        findsOneWidget,
      );
      expect(find.textContaining('not Tunnel admission'), findsOneWidget);
      await t.pump(const Duration(seconds: 2));
      expect(p.statuses, 1);
      expect(p.registrations, 1);
      p.issued.retired.complete();
      await t.pumpWidget(const SizedBox());
    },
  );
  testWidgets(
    'explicit cancellation during probe hides late review and does not claim channel closure',
    (t) async {
      final p = f.Port()..issued.pendingProbe = Completer();
      await t.pumpWidget(host(p));
      await choose(t);
      await tap(t, 'agent-association-probe');
      await tap(t, 'agent-association-cancel');
      expect(p.issued.cancels, 1);
      expect(
        find.textContaining('does not prove channel closure'),
        findsOneWidget,
      );
      p.issued.pendingProbe!.complete(f.review());
      await t.pump();
      expect(
        find.byKey(const ValueKey('agent-association-approve')),
        findsNothing,
      );
      p.issued.retired.complete();
      await t.pumpWidget(const SizedBox());
    },
  );
  testWidgets(
    'replacement original port retires old handle and cannot publish late association',
    (t) async {
      final p = f.Port()..issued.pendingOffer = Completer();
      await t.pumpWidget(host(p));
      await choose(t);
      await tap(t, 'agent-association-probe');
      await tap(t, 'agent-association-approve');
      final next = f.Port();
      await t.pumpWidget(host(next));
      expect(p.issued.cancels, 1);
      p.issued.pendingOffer!.complete(f.review(claim: true));
      await t.pump();
      expect(
        find.byKey(const ValueKey('agent-association-approve')),
        findsNothing,
      );
      expect(next.statuses, 0);
      p.issued.retired.complete();
      await t.pumpWidget(const SizedBox());
    },
  );
  testWidgets('RU narrow review keeps full bounded metadata without overflow', (
    t,
  ) async {
    t.view.physicalSize = const Size(500, 1100);
    t.view.devicePixelRatio = 1;
    addTearDown(t.view.resetPhysicalSize);
    addTearDown(t.view.resetDevicePixelRatio);
    final p = f.Port();
    await t.pumpWidget(host(p, locale: const Locale('ru'), scale: 1.5));
    await choose(t);
    await tap(t, 'agent-association-probe');
    expect(find.text('18446744073709551615'), findsOneWidget);
    expect(t.takeException(), isNull);
    await t.pumpWidget(const SizedBox());
    p.issued.retired.complete();
  });
  testWidgets(
    'separate identity consent stores only the client identity and never auto-probes',
    (t) async {
      final p = f.Port()..identityState = 'missing';
      await t.pumpWidget(host(p));
      await tap(t, 'agent-association-status');
      expect(p.identityPrepares, 0);
      await tap(t, 'agent-identity-prepare');
      expect(p.identity.confirms, 0);
      expect(p.registrations, 0);
      await tap(t, 'agent-identity-confirm');
      expect(
        find.byKey(const ValueKey('agent-identity-stored')),
        findsOneWidget,
      );
      expect(p.identity.confirms, 1);
      expect(p.statuses, 1);
      expect(p.registrations, 0);
      p.identity.retired.complete();
      await t.pump();
      await t.pumpWidget(const SizedBox());
    },
  );
  testWidgets(
    'actual Native certificate maximum caps the explicit requested lifetime',
    (t) async {
      final p = f.Port()..issued.maximum = 59;
      await t.pumpWidget(host(p));
      await choose(t);
      await tap(t, 'agent-association-probe');
      await tap(t, 'agent-association-approve');
      final picker = t.widget<GlassSelect<int>>(
        find.byKey(const ValueKey('agent-association-lifetime')),
      );
      expect(picker.value, 59);
      await tap(t, 'agent-association-approve');
      expect(p.issued.lastLifetime, 59);
      expect(find.text('2026-11-15T12:00:00.000Z'), findsOneWidget);
      p.issued.retired.complete();
      await t.pumpWidget(const SizedBox());
    },
  );
  testWidgets(
    'actual workspace optional entry is explicit and old service callback retires without querying',
    (t) async {
      final store = MonitoringTestStore(), service = MonitoringTestService();
      bool Function()? original;
      var opened = 0;
      Widget workspace(MonitoringService selected, {bool enabled = false}) =>
          MaterialApp(
            localizationsDelegates: const [
              AppLocalizations.delegate,
              ...GlobalMaterialLocalizations.delegates,
            ],
            supportedLocales: AppLocalizations.supportedLocales,
            home: Scaffold(
              body: MonitoringWorkspace(
                key: const ValueKey('original-workspace'),
                service: selected,
                store: store,
                openAgentAssociation: enabled
                    ? (current) async {
                        opened++;
                        original = current;
                      }
                    : null,
              ),
            ),
          );
      await t.pumpWidget(workspace(service));
      await t.pumpAndSettle();
      expect(
        find.byKey(const ValueKey('monitoring-agent-association-open')),
        findsNothing,
      );
      await t.pumpWidget(workspace(service, enabled: true));
      await t.pumpAndSettle();
      expect(opened, 0);
      await tap(t, 'monitoring-agent-association-open');
      expect(opened, 1);
      expect(original!(), isTrue);
      expect(service.queries, 0);
      await t.pumpWidget(workspace(MonitoringTestService(), enabled: true));
      await t.pumpAndSettle();
      expect(original!(), isFalse);
      await t.pumpWidget(const SizedBox());
    },
  );
}
