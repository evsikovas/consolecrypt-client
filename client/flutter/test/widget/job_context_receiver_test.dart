import 'dart:async';

import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/monitoring/job_context_focus.dart';
import 'package:consolecrypt/monitoring/job_context_navigation.dart';
import 'package:consolecrypt/monitoring/job_context_receiver.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_providers.dart';
import 'package:consolecrypt/monitoring/monitoring_service.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/monitoring_fakes.dart';
import '../unit/job_context_navigation_test.dart' as navigation;
import '../unit/job_metric_correlation_test.dart' as fixture;

class _Backend implements MonitoringBackend {
  _Backend(this.make);
  final AppCoreMonitoringService Function() make;
  @override
  MonitoringService createService() => make();
  @override
  MonitoringSourceStore createSourceStore() => MonitoringTestStore();
}

JobContextFocus focus(MonitoringSession session) {
  final context = fixture.context(scope: session.correlationScope);
  final original = navigation.frame(context), owner = JobContextNavigation()..bind(navigation.frame(context));
  owner.bind(original);
  final selected = owner.take(owner.propose(original, JobContextDestination.sshHost).intent!)!;
  return JobContextFocus.fromTaken(selected, original)!;
}

void main() {
  testWidgets('ordinary source-page hide retains original session and only admits read-only chooser focus', (
    tester,
  ) async {
    var hidden = false, reads = 0, metadata = 0;
    late WidgetRef widgetRef;
    final backend = _Backend(
      () => AppCoreMonitoringService(
        read: (_) async {
          reads++;
          return {};
        },
        cancelOperation: (_) async {},
        hostContextRead: (request) async {
          metadata++;
          expect(request['kind'], 'recheck');
          expect(request['notificationScopeId'], navigation.nonce);
          return navigation.wire(fixture.context());
        },
      ),
    );
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          monitoringBackendProvider.overrideWithValue(backend),
          monitoringScopeProvider.overrideWithValue(MonitoringScope(ObjectId(fixture.profile), true)),
        ],
        child: Consumer(
          builder: (context, ref, _) {
            widgetRef = ref;
            return Offstage(offstage: hidden, child: const SizedBox());
          },
        ),
      ),
    );
    final original = widgetRef.read(monitoringSessionProvider)!;
    final packet = focus(original);
    hidden = true;
    await tester.pump();
    final result = await receiveJobContextFocus(
      widgetRef,
      packet,
      destination: JobContextDestination.sshHost,
      receiverCurrent: () => true,
    );
    expect(result, same(packet));
    expect(widgetRef.read(monitoringSessionProvider), same(original));
    expect(metadata, 1);
    expect(reads, 0);
    expect(
      await receiveJobContextFocus(
        widgetRef,
        packet,
        destination: JobContextDestination.sshHost,
        receiverCurrent: () => true,
      ),
      isNull,
    );
    expect(metadata, 1);
  });
  testWidgets('lock and reopening same profile cannot rescue a late original-nonce handoff', (tester) async {
    late WidgetRef widgetRef;
    var scope = MonitoringScope(ObjectId(fixture.profile), true), metadata = 0;
    final pending = Completer<Json>();
    final backend = _Backend(
      () => AppCoreMonitoringService(
        read: (_) async => throw StateError('No metrics read'),
        cancelOperation: (_) async {},
        hostContextRead: (_) {
          metadata++;
          return pending.future;
        },
      ),
    );
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          monitoringBackendProvider.overrideWithValue(backend),
          monitoringScopeProvider.overrideWith((ref) => scope),
        ],
        child: Consumer(
          builder: (context, ref, _) {
            widgetRef = ref;
            return const SizedBox();
          },
        ),
      ),
    );
    final container = ProviderScope.containerOf(tester.element(find.byType(Consumer)));
    final original = widgetRef.read(monitoringSessionProvider)!;
    final packet = focus(original);
    final accepting = receiveJobContextFocus(
      widgetRef,
      packet,
      destination: JobContextDestination.sshHost,
      receiverCurrent: () => true,
    );
    scope = MonitoringScope(ObjectId(fixture.profile), false);
    container.invalidate(monitoringScopeProvider);
    expect(container.read(monitoringSessionProvider), isNull);
    scope = MonitoringScope(ObjectId(fixture.profile), true);
    container.invalidate(monitoringScopeProvider);
    final reopened = container.read(monitoringSessionProvider)!;
    expect(reopened, isNot(same(original)));
    expect(reopened.correlationScope, isNot(same(original.correlationScope)));
    pending.complete(navigation.wire(fixture.context()));
    expect(await accepting, isNull);
    expect(metadata, 1);
    expect(
      await receiveJobContextFocus(
        widgetRef,
        focus(original),
        destination: JobContextDestination.sshHost,
        receiverCurrent: () => true,
      ),
      isNull,
    );
    expect(metadata, 1);
  });
  testWidgets('destination hide during original recheck refuses focus without a second capture', (tester) async {
    late WidgetRef widgetRef;
    var active = true, metadata = 0;
    final pending = Completer<Json>();
    final backend = _Backend(
      () => AppCoreMonitoringService(
        read: (_) async => throw StateError('No metrics read'),
        cancelOperation: (_) async {},
        hostContextRead: (_) {
          metadata++;
          return pending.future;
        },
      ),
    );
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          monitoringBackendProvider.overrideWithValue(backend),
          monitoringScopeProvider.overrideWithValue(MonitoringScope(ObjectId(fixture.profile), true)),
        ],
        child: Consumer(
          builder: (context, ref, _) {
            widgetRef = ref;
            return const SizedBox();
          },
        ),
      ),
    );
    final pendingFocus = receiveJobContextFocus(
      widgetRef,
      focus(widgetRef.read(monitoringSessionProvider)!),
      destination: JobContextDestination.sshHost,
      receiverCurrent: () => active,
    );
    active = false;
    pending.complete(navigation.wire(fixture.context()));
    expect(await pendingFocus, isNull);
    expect(metadata, 1);
  });
}
