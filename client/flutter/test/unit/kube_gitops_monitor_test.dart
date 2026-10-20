import 'dart:async';

import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_gitops_monitor.dart';
import 'package:consolecrypt/kubernetes/kube_insights_service.dart';
import 'package:consolecrypt/kubernetes/kube_insights_watch_service.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';
import 'package:consolecrypt/kubernetes/kube_view_preferences_service.dart';
import 'package:flutter_test/flutter_test.dart';

const _session = KubeSession(id: 's', contextId: 'c', serverVersion: 'v1', production: false);
const _kind = KubeResourceKind(
  group: 'kustomize.toolkit.fluxcd.io',
  version: 'v1',
  resource: 'kustomizations',
  kind: 'Kustomization',
  namespaced: true,
  verbs: ['get', 'list', 'watch'],
);
const _reference = KubeResourceRef(api: _kind, name: 'app', namespace: 'team');

KubeInsights _snapshot({bool ready = true, bool stalled = false, String uid = 'u', String? failedAt}) => KubeInsights(
  issues: const [],
  gitops: const KubeGitOpsOverview(),
  resources: [
    KubeInsightResource(
      api: _kind,
      resource: KubeResourceRow(
        name: 'app',
        namespace: 'team',
        uid: uid,
        resourceVersion: '1',
        status: '',
        object: {
          'status': {
            'conditions': [
              {'type': 'Ready', 'status': ready ? 'True' : 'False', 'lastTransitionTime': ?failedAt},
              {'type': 'Stalled', 'status': stalled ? 'True' : 'False'},
            ],
          },
        },
      ),
    ),
  ],
);

class _Service implements KubeInsightsService, KubeInsightsWatchService, KubeViewPreferencesService {
  final streams = <StreamController<KubeInsightsInvalidation>>[];
  final writes = <KubeJson>[];
  KubeInsights snapshot = _snapshot();
  KubeJson prefs = {};
  Completer<KubeJson>? pendingPreferences;
  Completer<KubeInsights>? pendingRead;
  int reads = 0, cancelled = 0;
  @override
  Future<KubeInsights> insights(String sessionId, {String? namespace}) async {
    reads++;
    return pendingRead?.future ?? snapshot;
  }

  @override
  Stream<KubeInsightsInvalidation> watchInsights(String sessionId, {String? namespace}) {
    final stream = StreamController<KubeInsightsInvalidation>(onCancel: () => cancelled++);
    streams.add(stream);
    return stream.stream;
  }

  @override
  Future<KubeJson> readKubeViewPreferences(String contextId) async => pendingPreferences?.future ?? prefs;
  @override
  Future<void> updateKubeViewPreferences(String contextId, {required KubeJson patch}) async => writes.add(patch);
  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

void main() {
  testWidgets('one watch debounces bursts and stops on background and disposal', (tester) async {
    final service = _Service();
    final monitor = KubeGitOpsMonitor(session: _session, service: service, isCurrent: () => true);
    monitor.show();
    await tester.pump();
    expect(service.reads, 1);
    expect(service.streams.length, 1);
    for (var i = 0; i < 20; i++) {
      service.streams.last.add(const KubeInsightsInvalidation());
    }
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 799));
    expect(service.reads, 1);
    await tester.pump(const Duration(milliseconds: 1));
    expect(service.reads, 2);
    monitor.setForeground(false);
    await tester.pump();
    expect(service.cancelled, 1);
    monitor.setForeground(true);
    expect(service.streams.length, 2);
    monitor.dispose();
    await tester.pump();
    expect(service.cancelled, 2);
  });

  testWidgets('opt in establishes baseline and only reports new failures for the same UID', (tester) async {
    final service = _Service();
    final notices = <KubeGitOpsNotice>[];
    final monitor = KubeGitOpsMonitor(
      session: _session,
      service: service,
      preferences: service,
      isCurrent: () => true,
      onNotice: notices.add,
    );
    await tester.pump();
    expect(service.reads, 0);
    await monitor.setNotifications(true);
    await tester.pump();
    expect(notices, isEmpty);
    service.snapshot = _snapshot(ready: false);
    await monitor.refresh();
    expect(notices, hasLength(1));
    expect(monitor.alertCount, 1);
    await monitor.refresh();
    expect(notices, hasLength(1));
    service.snapshot = _snapshot(ready: false, stalled: true, uid: 'recreated');
    await monitor.refresh();
    expect(notices, hasLength(1));
    monitor.acknowledge();
    expect(monitor.alertCount, 0);
    await monitor.setNotifications(false);
    expect(service.cancelled, 1);
    monitor.dispose();
  });

  testWidgets('new failed reconciliation notifies even while resource remains not ready', (tester) async {
    final service = _Service()..snapshot = _snapshot(ready: false, failedAt: '2026-10-20T10:00:00Z');
    final notices = <KubeGitOpsNotice>[];
    final monitor = KubeGitOpsMonitor(
      session: _session,
      service: service,
      isCurrent: () => true,
      onNotice: notices.add,
    );
    await monitor.setNotifications(true);
    await tester.pump();
    expect(notices, isEmpty);
    service.snapshot = _snapshot(ready: false, failedAt: '2026-10-20T10:05:00Z');
    await monitor.refresh();
    expect(notices, hasLength(1));
    await monitor.refresh();
    expect(notices, hasLength(1));
    monitor.dispose();
  });

  testWidgets('reminder persists metadata, replaces same object and waits while backgrounded', (tester) async {
    var now = DateTime.utc(2026, 10, 20);
    final service = _Service();
    final notices = <KubeGitOpsNotice>[];
    final monitor = KubeGitOpsMonitor(
      session: _session,
      service: service,
      preferences: service,
      isCurrent: () => true,
      onNotice: notices.add,
      clock: () => now,
    );
    await tester.pump();
    await monitor.remind(_reference, const Duration(hours: 1));
    await monitor.remind(_reference, const Duration(hours: 2));
    expect(monitor.reminders, hasLength(1));
    expect(service.writes.last['kind'], 'gitops');
    expect((service.writes.last['preferences'] as KubeJson)['reminders'], hasLength(1));
    monitor.setForeground(false);
    now = now.add(const Duration(hours: 3));
    monitor.checkReminders();
    expect(notices, isEmpty);
    monitor.setForeground(true);
    await tester.pump();
    expect(notices.single.reminder, isTrue);
    expect(monitor.reminders, isEmpty);
    expect(service.reads, 0); // A reminder never opens or mutates cluster state.
    monitor.dispose();
  });

  testWidgets('late reads and preference responses cannot cross lock or overwrite a choice', (tester) async {
    var current = true;
    final service = _Service()..pendingPreferences = Completer<KubeJson>();
    final monitor = KubeGitOpsMonitor(
      session: _session,
      service: service,
      preferences: service,
      isCurrent: () => current,
    );
    await monitor.setNotifications(true);
    service.pendingPreferences!.complete({
      'gitops': {'notifications': false},
    });
    await tester.pump();
    expect(monitor.notifications, isTrue);
    service.pendingRead = Completer<KubeInsights>();
    unawaited(monitor.refresh());
    await tester.pump();
    current = false;
    monitor.dispose();
    service.pendingRead!.complete(_snapshot(ready: false));
    await tester.pump();
    expect(monitor.data, isNull);
    expect(service.cancelled, 1);
    expect(tester.takeException(), isNull);
  });
}
