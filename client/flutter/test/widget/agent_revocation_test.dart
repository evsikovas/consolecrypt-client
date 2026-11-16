import 'dart:async';

import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:consolecrypt/monitoring/agent_association_dialog.dart';
import 'package:consolecrypt/monitoring/agent_association_port.dart';
import 'package:consolecrypt/monitoring/agent_revocation_dialog.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../support/agent_revocation_fixture.dart' as f;

Widget app(
  Widget child, {
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
  home: Scaffold(body: child),
);
Widget parent(AgentAssociationPort p) =>
    app(AgentAssociationDialog(port: p, scope: f.scope, original: () => true));
Widget direct(
  f.Port p,
  ValueNotifier<bool> original, {
  Locale locale = const Locale('en'),
  double scale = 1,
  Duration Function()? elapsedForReview,
}) => app(
  AgentRevocationDialog(
    port: p,
    scope: f.scope,
    record: f.record(),
    original: () => original.value,
    originalChanges: original,
    elapsedForReview: elapsedForReview,
  ),
  locale: locale,
  scale: scale,
);
Future<void> tap(WidgetTester t, String key) async {
  final finder = find.byKey(ValueKey(key));
  await t.ensureVisible(finder);
  await t.tap(finder);
  await t.pump();
  await t.pump(const Duration(milliseconds: 300));
}

void main() {
  testWidgets(
    'existing association row exposes only supported revocation and requires separate explicit review and confirm',
    (t) async {
      final legacy = f.Legacy();
      await t.pumpWidget(parent(legacy));
      await tap(t, 'agent-association-status');
      expect(find.text('Revoke access'), findsNothing);
      expect(legacy.port.prepares, 0);
      await t.pumpWidget(const SizedBox());
      final p = f.Port();
      await t.pumpWidget(parent(p));
      expect(p.statuses, 0);
      await tap(t, 'agent-association-status');
      await tap(t, 'agent-revoke-row-${p.statusValue.records.single.id}');
      expect(p.prepares, 0);
      await tap(t, 'agent-revoke-prepare');
      expect(p.prepares, 1);
      expect(p.issued.confirms, 0);
      expect(find.text('7'), findsOneWidget);
      await tap(t, 'agent-revoke-confirm');
      expect(p.issued.confirms, 1);
      expect(
        find.byKey(const ValueKey('agent-revoke-observed')),
        findsOneWidget,
      );
      expect(find.byKey(const ValueKey('agent-revoke-confirm')), findsNothing);
      await t.pump(const Duration(seconds: 2));
      expect(p.statuses, 1);
      expect(p.prepares, 1);
      p.issued.finishCleanup(observed: true);
      await t.pump();
      await t.pumpWidget(const SizedBox());
    },
  );
  testWidgets(
    'lost confirmation stays unknown despite cleanup observation and offers no retry',
    (t) async {
      final p = f.Port()..issued.failConfirm = true;
      final original = ValueNotifier(true);
      await t.pumpWidget(direct(p, original));
      await tap(t, 'agent-revoke-prepare');
      await tap(t, 'agent-revoke-confirm');
      p.issued.finishCleanup(observed: true);
      await t.pump();
      expect(
        find.byKey(const ValueKey('agent-revoke-unknown')),
        findsOneWidget,
      );
      expect(find.byKey(const ValueKey('agent-revoke-observed')), findsNothing);
      expect(
        t
            .widget<FilledButton>(
              find.byKey(const ValueKey('agent-revoke-confirm')),
            )
            .onPressed,
        isNull,
      );
      await t.pump(const Duration(seconds: 3));
      expect(p.issued.confirms, 1);
      expect(p.prepares, 1);
      expect(p.statuses, 0);
      await t.pumpWidget(const SizedBox());
      original.dispose();
    },
  );
  testWidgets(
    'cancel during async details immediately cancels original and suppresses late review',
    (t) async {
      final p = f.Port()..issued.pendingDetails = Completer();
      final original = ValueNotifier(true);
      await t.pumpWidget(direct(p, original));
      await tap(t, 'agent-revoke-prepare');
      await tap(t, 'agent-revoke-cancel');
      expect(p.issued.cancels, 1);
      expect(p.issued.joins, 1);
      p.issued.pendingDetails!.complete(p.issued.value());
      await t.pump();
      expect(find.byKey(const ValueKey('agent-revoke-confirm')), findsNothing);
      expect(find.byKey(const ValueKey('agent-revoke-observed')), findsNothing);
      p.issued.finishCleanup();
      await t.pumpWidget(const SizedBox());
      original.dispose();
    },
  );
  testWidgets(
    'original owner retirement cancels pending confirm and rejects late observed DTO',
    (t) async {
      final p = f.Port()..issued.pendingConfirm = Completer();
      final original = ValueNotifier(true);
      await t.pumpWidget(direct(p, original));
      await tap(t, 'agent-revoke-prepare');
      await tap(t, 'agent-revoke-confirm');
      original.value = false;
      expect(p.issued.cancels, 1);
      p.issued.pendingConfirm!.complete(p.issued.value(phase: 'revoked'));
      await t.pump();
      expect(find.byKey(const ValueKey('agent-revoke-observed')), findsNothing);
      expect(p.prepares, 1);
      expect(p.issued.confirms, 1);
      p.issued.finishCleanup();
      await t.pumpWidget(const SizedBox());
      original.dispose();
    },
  );
  testWidgets(
    'original display TTL expires without RPC and never cancels an already dispatched confirmation',
    (t) async {
      var elapsed = Duration.zero;
      var p = f.Port()..issued.remaining = 400;
      var original = ValueNotifier(true);
      await t.pumpWidget(direct(p, original, elapsedForReview: () => elapsed));
      await tap(t, 'agent-revoke-prepare');
      elapsed = const Duration(milliseconds: 401);
      await t.pump(const Duration(milliseconds: 401));
      expect(p.issued.cancels, 1);
      expect(
        t
            .widget<FilledButton>(
              find.byKey(const ValueKey('agent-revoke-confirm')),
            )
            .onPressed,
        isNull,
      );
      expect(p.issued.confirms, 0);
      expect(p.issued.detailsCalls, 1);
      p.issued.finishCleanup();
      await t.pumpWidget(const SizedBox());
      original.dispose();
      elapsed = Duration.zero;
      p = f.Port();
      p.issued.remaining = 1000;
      p.issued.pendingConfirm = Completer();
      original = ValueNotifier(true);
      await t.pumpWidget(direct(p, original, elapsedForReview: () => elapsed));
      await tap(t, 'agent-revoke-prepare');
      await tap(t, 'agent-revoke-confirm');
      elapsed = const Duration(seconds: 2);
      await t.pump(const Duration(seconds: 2));
      expect(p.issued.cancels, 0);
      p.issued.pendingConfirm!.complete(p.issued.value(phase: 'revoked'));
      await t.pump();
      expect(
        find.byKey(const ValueKey('agent-revoke-observed')),
        findsOneWidget,
      );
      expect(p.issued.confirms, 1);
      p.issued.finishCleanup(observed: true);
      await t.pumpWidget(const SizedBox());
      original.dispose();
    },
  );
  testWidgets(
    'vault key mismatch remains visible alongside unknown without raw diagnostic text',
    (t) async {
      final p = f.Port()
        ..issued.confirmFailure = const AppException(
          AppErrorCode.vaultKeyMismatch,
          'do-not-render-owned-diagnostic',
        );
      final original = ValueNotifier(true);
      await t.pumpWidget(direct(p, original));
      await tap(t, 'agent-revoke-prepare');
      await tap(t, 'agent-revoke-confirm');
      final l = AppLocalizations.of(
        t.element(find.byType(AgentRevocationDialog)),
      );
      expect(find.text(l.errorVaultKeyMismatch), findsOneWidget);
      expect(
        find.textContaining('do-not-render-owned-diagnostic'),
        findsNothing,
      );
      expect(
        find.byKey(const ValueKey('agent-revoke-unknown')),
        findsOneWidget,
      );
      expect(find.byKey(const ValueKey('agent-revoke-observed')), findsNothing);
      p.issued.finishCleanup();
      await t.pumpWidget(const SizedBox());
      original.dispose();
    },
  );
  testWidgets(
    'parent port replacement retires and removes only its exact child review',
    (t) async {
      final p = f.Port()..issued.pendingDetails = Completer();
      await t.pumpWidget(parent(p));
      await tap(t, 'agent-association-status');
      await tap(t, 'agent-revoke-row-${p.statusValue.records.single.id}');
      await tap(t, 'agent-revoke-prepare');
      final replacement = f.Port();
      await t.pumpWidget(parent(replacement));
      await t.pump();
      expect(p.issued.cancels, 1);
      expect(p.issued.joins, 1);
      expect(find.byType(AgentRevocationDialog), findsNothing);
      p.issued.pendingDetails!.complete(p.issued.value());
      p.issued.finishCleanup();
      await t.pump();
      expect(find.byKey(const ValueKey('agent-revoke-observed')), findsNothing);
      expect(replacement.prepares, 0);
      expect(replacement.statuses, 0);
      expect(t.takeException(), isNull);
      await t.pumpWidget(const SizedBox());
    },
  );
  testWidgets('Russian original metadata review fits narrow enlarged layout', (
    t,
  ) async {
    await t.binding.setSurfaceSize(const Size(500, 900));
    addTearDown(() => t.binding.setSurfaceSize(null));
    final p = f.Port();
    final original = ValueNotifier(true);
    await t.pumpWidget(
      direct(p, original, locale: const Locale('ru'), scale: 1.5),
    );
    await tap(t, 'agent-revoke-prepare');
    expect(find.text('Подтвердить отзыв'), findsOneWidget);
    expect(t.takeException(), isNull);
    await tap(t, 'agent-revoke-cancel');
    p.issued.finishCleanup();
    await t.pumpWidget(const SizedBox());
    original.dispose();
  });
}
