import 'dart:async';

import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:consolecrypt/monitoring/agent_association_dialog.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_association_port.dart';
import 'package:consolecrypt/monitoring/agent_rotation.dart';
import 'package:consolecrypt/monitoring/agent_rotation_dialog.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../support/agent_rotation_fake.dart';
import '../support/agent_rotation_fixture.dart' as f;

Widget app(Widget child, {Locale locale = const Locale('en'), double scale = 1}) => MaterialApp(
  locale: locale,
  supportedLocales: AppLocalizations.supportedLocales,
  localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
  builder: (context, child) => MediaQuery(
    data: MediaQuery.of(context).copyWith(textScaler: TextScaler.linear(scale)),
    child: child!,
  ),
  home: Scaffold(body: child),
);
Widget direct(
  RotationPort port,
  ValueNotifier<bool> live, {
  AgentRotationMode mode = AgentRotationMode.rotate,
  Locale locale = const Locale('en'),
  double scale = 1,
  Duration Function()? elapsedForReview,
}) => app(
  AgentRotationDialog(
    port: port,
    scope: port.handle.scope,
    record: port.handle.record,
    mode: mode,
    original: () => live.value,
    originalChanges: live,
    elapsedForReview:elapsedForReview,
  ),
  locale: locale,
  scale: scale,
);
Future<void> tap(WidgetTester t, String key) async {
  final f = find.byKey(ValueKey(key));
  await t.ensureVisible(f);
  await t.tap(f);
  await t.pump();
  await t.pump(const Duration(milliseconds: 300));
}

class _ParentPort extends RotationPort implements AgentAssociationPort {
  _ParentPort(super.handle);
  int statuses = 0;
  @override
  Future<AgentAssociationStatus> status(RuntimeNoticeScope scope) async {
    statuses++;
    return AgentAssociationStatus(
      scope: scope,
      clientIdentityState: AgentClientIdentityState.storedMetadata,
      clientIdentityId: f.deviceId,
      clientCertificateSha256: 'a' * 64,
      hosts: [],
      records: [handle.record],
    );
  }

  @override
  Future<AgentAssociationHandle> registerProbe(RuntimeNoticeScope scope, AgentAssociationHost host) async =>
      throw StateError('No implicit enrollment');
  @override
  Future<AgentClientIdentityHandle> prepareClientIdentity(RuntimeNoticeScope scope) async =>
      throw StateError('No implicit TLS');
}

void main() {
  for (final reconcile in [false, true]) {
    testWidgets('existing Agent row exposes only explicit ${reconcile ? 'reconcile' : 'rotation'} original review', (
      t,
    ) async {
      final s = f.scope(),
          r = f.record(reconcile: reconcile),
          h = RotationHandle(s, r, reconcile: reconcile),
          p = _ParentPort(h);
      await t.pumpWidget(app(AgentAssociationDialog(port: p, scope: s, original: () => true)));
      expect(p.statuses, 0);
      expect(p.normal, 0);
      expect(p.reconciles, 0);
      await tap(t, 'agent-association-status');
      await tap(t, '${reconcile ? 'agent-reconcile-row' : 'agent-rotation-row'}-${r.id}');
      expect(p.normal, 0);
      expect(p.reconciles, 0);
      await tap(t, 'agent-rotation-prepare');
      expect(p.normal, reconcile ? 0 : 1);
      expect(p.reconciles, reconcile ? 1 : 0);
      expect(h.candidates, 0);
      expect(h.confirms, 0);
      await t.pumpWidget(const SizedBox());
      expect(h.cancels, 1);
      expect(t.takeException(), isNull);
    });
  }
  testWidgets('normal UI requires separate candidate review then explicit confirm and never infers a data permit', (
    t,
  ) async {
    final s = f.scope(), r = f.record(), h = RotationHandle(s, r), p = RotationPort(h), live = ValueNotifier(true);
    addTearDown(live.dispose);
    await t.pumpWidget(direct(p, live));
    expect(p.normal, 0);
    await tap(t, 'agent-rotation-prepare');
    expect(h.candidates, 0);
    expect(h.confirms, 0);
    expect(find.byKey(const ValueKey('agent-rotation-confirm')), findsNothing);
    await tap(t, 'agent-rotation-candidate');
    expect(h.candidates, 1);
    expect(h.confirms, 0);
    expect(find.textContaining('Proposed Node epoch'), findsOneWidget);
    expect(find.byKey(const ValueKey('agent-rotation-observed')), findsNothing);
    await tap(t, 'agent-rotation-confirm');
    expect(h.confirms, 1);
    expect(find.byKey(const ValueKey('agent-rotation-observed')), findsOneWidget);
    expect(find.textContaining('not a data permit'), findsOneWidget);
    expect(h.reads, 1);
    expect(p.normal, 1);
    await t.pumpWidget(const SizedBox());
    expect(t.takeException(), isNull);
  });
  testWidgets('candidate outcome uncertainty stays blocked/unknown without automatic retry or false readback', (
    t,
  ) async {
    final s = f.scope(),
        r = f.record(),
        h = RotationHandle(s, r)..candidateError = Exception('PRIVATE RAW failure'),
        p = RotationPort(h),
        live = ValueNotifier(true);
    addTearDown(live.dispose);
    await t.pumpWidget(direct(p, live));
    await tap(t, 'agent-rotation-prepare');
    await tap(t, 'agent-rotation-candidate');
    expect(find.byKey(const ValueKey('agent-rotation-unknown')), findsOneWidget);
    expect(find.byKey(const ValueKey('agent-rotation-observed')), findsNothing);
    expect(find.textContaining('PRIVATE RAW'), findsNothing);
    await t.pump(const Duration(seconds: 10));
    expect(h.candidates, 1);
    expect(h.confirms, 0);
    expect(h.reads, 1);
    expect(p.normal, 1);
    await t.pumpWidget(const SizedBox());
    expect(t.takeException(), isNull);
  });
  testWidgets('original owner retirement cancels pending details immediately and late future stays hidden', (t) async {
    final s = f.scope(),
        r = f.record(),
        h = RotationHandle(s, r)..pendingDetails = Completer<AgentRotationDetails>(),
        p = RotationPort(h),
        live = ValueNotifier(true);
    addTearDown(live.dispose);
    await t.pumpWidget(direct(p, live));
    await tap(t, 'agent-rotation-prepare');
    live.value = false;
    expect(h.cancels, 1);
    await t.pump();
    h.pendingDetails!.complete(f.parsed(s, r));
    await t.pump();
    expect(find.byKey(const ValueKey('agent-rotation-candidate')), findsNothing);
    expect(h.candidates, 0);
    expect(h.confirms, 0);
    await t.pumpWidget(const SizedBox());
    expect(t.takeException(), isNull);
  });
  testWidgets('original countdown disables unsubmitted confirm without a hidden Native read', (t) async {
    var elapsed=Duration.zero;
    final s = f.scope(), r = f.record(), h = RotationHandle(s, r), p = RotationPort(h), live = ValueNotifier(true);
    addTearDown(live.dispose);
    await t.pumpWidget(direct(p, live,elapsedForReview:()=>elapsed));
    await tap(t, 'agent-rotation-prepare');
    await tap(t, 'agent-rotation-candidate');
    elapsed=const Duration(seconds:61);
    await t.pump(const Duration(seconds: 61));
    expect(h.confirms, 0);
    expect(h.reads, 1);
    expect(p.normal, 1);
    expect(t.widget<FilledButton>(find.byKey(const ValueKey('agent-rotation-confirm'))).onPressed, isNull);
    await t.pumpWidget(const SizedBox());
    expect(t.takeException(), isNull);
  });
  testWidgets('display expiry never cancels an already dispatched Native confirmation', (t) async {
    var elapsed=Duration.zero;
    final s = f.scope(),
        r = f.record(),
        h = RotationHandle(s, r)..pendingConfirm = Completer<AgentRotationDetails>(),
        p = RotationPort(h),
        live = ValueNotifier(true);
    addTearDown(live.dispose);
    await t.pumpWidget(direct(p, live,elapsedForReview:()=>elapsed));
    await tap(t, 'agent-rotation-prepare');
    await tap(t, 'agent-rotation-candidate');
    await tap(t, 'agent-rotation-confirm');
    elapsed=const Duration(seconds:61);
    await t.pump(const Duration(seconds: 61));
    expect(h.cancels, 0);
    expect(h.confirms, 1);
    h.pendingConfirm!.complete(f.parsed(s, r, phase: 'rotated'));
    await t.pump();
    await t.pump();
    expect(find.byKey(const ValueKey('agent-rotation-observed')), findsOneWidget);
    await t.pumpWidget(const SizedBox());
    expect(t.takeException(), isNull);
  });
  testWidgets('RU rotation review remains bounded at500px and1.5x with vault alarm preserved', (t) async {
    await t.binding.setSurfaceSize(const Size(500, 740));
    addTearDown(() => t.binding.setSurfaceSize(null));
    final s = f.scope(),
        r = f.record(),
        h = RotationHandle(s, r)..candidateError = const AppException(AppErrorCode.vaultKeyMismatch, 'PRIVATE RAW'),
        p = RotationPort(h),
        live = ValueNotifier(true);
    addTearDown(live.dispose);
    await t.pumpWidget(direct(p, live, locale: const Locale('ru'), scale: 1.5));
    await tap(t, 'agent-rotation-prepare');
    expect(t.takeException(), isNull);
    await tap(t, 'agent-rotation-candidate');
    expect(find.textContaining('PRIVATE RAW'), findsNothing);
    expect(find.byKey(const ValueKey('agent-rotation-unknown')), findsOneWidget);
    expect(t.takeException(), isNull);
    await t.pumpWidget(const SizedBox());
  });
}
