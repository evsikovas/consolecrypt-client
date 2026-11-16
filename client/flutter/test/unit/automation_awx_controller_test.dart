import 'dart:async';

import 'package:consolecrypt/automation/automation_awx_controller.dart';
import 'package:consolecrypt/automation/automation_awx_models.dart';
import 'package:consolecrypt/automation/automation_awx_service.dart';
import 'package:consolecrypt/core/services/runtime_notice_adapter.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart'
    show MonitoringSource;
import 'package:flutter_test/flutter_test.dart';

import 'automation_awx_models_test.dart' as f;

class Op<T> implements AwxRegisteredOperation<T> {
  Op(this.value, {this.auto = true});
  final T value;
  final bool auto;
  final ack = Completer<void>(),
      done = Completer<T>(),
      join = Completer<void>();
  int starts = 0;
  @override
  Future<void> get registered => ack.future;
  @override
  Future<T> get result => done.future;
  @override
  Future<void> get joined => join.future;
  @override
  void startBody() {
    starts++;
    if (auto) {
      done.complete(value);
      join.complete();
    }
  }
}

class Native implements AwxNativeGateway {
  final calls = <String>[], cancelIds = <String>[], discards = <String>[];
  int captures = 0;
  Completer<AwxNativeScope>? captureWait;
  Op<AwxTemplatePage>? templateWait;
  Op<AwxPreparedReview>? prepareWait;
  Op<AwxLaunchOutcome>? launchWait;
  AwxCatalog currentCatalog = f.catalog();
  AwxLaunchOutcome launchResult = const AwxLaunchUnknown();
  AwxLaunchParameters? preparedParameters;
  AwxNativeScope? cancelledScope;
  String? registeredId;
  T ready<T>(T op) => op;
  Op<T> begin<T>(String kind, String id, T value) {
    calls.add(kind);
    registeredId = id;
    final op = Op(value)..ack.complete();
    return op;
  }

  @override
  Future<AwxNativeScope> captureScope() async {
    captures++;
    return captureWait?.future ??
        AwxNativeScope.fromJson({
          'version': 1,
          'profileId': f.profile,
          'notificationScopeId': f.nonce,
        });
  }

  @override
  Future<AwxCatalog> catalog(AwxNativeScope original) async {
    calls.add('catalog');
    return currentCatalog;
  }

  @override
  Future<AwxCatalog> saveConnection(
    AwxNativeScope original, {
    required int expectedCatalogRevision,
    required int? expectedRevision,
    required MonitoringSource configuration,
  }) async {
    calls.add('save');
    return currentCatalog;
  }

  @override
  Future<AwxCatalog> deleteConnection(
    AwxNativeScope original, {
    required int expectedCatalogRevision,
    required AwxConnection connection,
  }) async {
    calls.add('delete');
    return currentCatalog;
  }

  @override
  AwxRegisteredOperation<AwxTemplatePage> registerTemplates(
    AwxNativeScope scope,
    String id,
    AwxConnection c,
    int page,
    String search,
  ) {
    if (templateWait != null) {
      calls.add('templates');
      registeredId = id;
      return templateWait!;
    }
    return begin(
      'templates',
      id,
      AwxTemplatePage.fromJson({
        'templates': [f.templateJson()],
        'has_next': false,
        'page': page,
      }, expectedPage: page),
    );
  }

  @override
  AwxRegisteredOperation<AwxPreparedReview> registerPrepare(
    AwxNativeScope scope,
    String id,
    AwxConnection c,
    int templateId,
    AwxLaunchParameters p,
  ) {
    preparedParameters = p;
    if (prepareWait != null) {
      calls.add('prepare');
      registeredId = id;
      return prepareWait!;
    }
    return begin('prepare', id, f.review(parameters: p));
  }

  @override
  AwxRegisteredOperation<AwxLaunchOutcome> registerLaunch(
    AwxNativeScope scope,
    String id,
    String ticket,
  ) {
    expect(ticket, 'synthetic-one-use');
    if (launchWait != null) {
      calls.add('launch');
      registeredId = id;
      return launchWait!;
    }
    return begin('launch', id, launchResult);
  }

  @override
  AwxRegisteredOperation<AwxJob> registerJob(
    AwxNativeScope scope,
    String id,
    AwxConnection c,
    int jobId,
  ) => begin('job', id, AwxJob.fromJson(f.jobJson(), expectedId: jobId));
  @override
  AwxRegisteredOperation<AwxEventPage> registerEvents(
    AwxNativeScope scope,
    String id,
    AwxConnection c,
    int jobId,
    int after,
  ) => begin(
    'events',
    id,
    AwxEventPage.fromJson({
      'events': <Object?>[],
      'has_next': false,
      'after': after,
    }, requestedAfter: after),
  );
  @override
  Future<void> cancel(AwxNativeScope original, String id) async {
    cancelledScope = original;
    cancelIds.add(id);
  }

  @override
  Future<void> discardReview(AwxNativeScope original, String ticket) async {
    discards.add(ticket);
  }
}

Future<void> flush() async {
  for (var i = 0; i < 30; i++) {
    await Future<void>.value();
  }
}

AwxController controller(
  RuntimeNoticeEventAdapter hub,
  Native? native, {
  bool Function()? current,
}) {
  var id = 0;
  return AwxController(
    profileId: f.profile,
    events: hub,
    isOriginalUiCurrent: current ?? () => true,
    operationId: () =>
        '55555555-5555-4555-8555-${(++id).toString().padLeft(12, '0')}',
    gateway: native,
    now: () => f.now,
  );
}

Future<void> selectTemplate(AwxController c) async {
  await c.loadCatalog();
  c.selectConnection(c.catalog!.connections.single);
  await c.loadTemplates();
  c.selectTemplate(c.templates!.templates.single);
}

void main() {
  test(
    'missing optional Native capability typed unavailable without RPC fallback',
    () async {
      final hub = RuntimeNoticeEventAdapter(), c = controller(hub, null);
      addTearDown(hub.dispose);
      addTearDown(c.dispose);
      await c.loadCatalog();
      expect(c.failure, AwxFailureReason.unavailable);
      expect(c.catalog, isNull);
    },
  );
  test('opening/catalog/templates/prepare never calls launch; exact review consumed once', () async {
    final hub = RuntimeNoticeEventAdapter(),
        n = Native(),
        c = controller(hub, n);
    addTearDown(hub.dispose);
    addTearDown(c.dispose);
    expect(n.calls, isEmpty);
    await selectTemplate(c);
    final p = AwxLaunchParameters(limit: 'synthetic', extraVars: {'x': 1});
    c.editParameters(p);
    await c.prepareLaunch();
    expect(n.calls, ['catalog', 'templates', 'prepare']);
    expect(c.canLaunch, true);
    expect(n.preparedParameters!.sameAs(p), true);
    final pending = c.launchReviewed();
    expect(c.review, isNull);
    await pending;
    await c.launchReviewed();
    expect(n.calls.where((s) => s == 'launch').length, 1);
    expect(c.outcome, isA<AwxLaunchUnknown>());
  });
  test(
    'parameter edit discards original ticket and requires a new review',
    () async {
      final hub = RuntimeNoticeEventAdapter(),
          n = Native(),
          c = controller(hub, n);
      addTearDown(hub.dispose);
      addTearDown(c.dispose);
      await selectTemplate(c);
      await c.prepareLaunch();
      c.editParameters(AwxLaunchParameters(limit: 'new'));
      await flush();
      expect(c.review, isNull);
      expect(c.canLaunch, false);
      expect(n.discards, ['synthetic-one-use']);
      await c.launchReviewed();
      expect(n.calls.contains('launch'), false);
    },
  );
  test('late initial Native scope cannot adopt a replacement unlock', () async {
    final hub = RuntimeNoticeEventAdapter(),
        n = Native()..captureWait = Completer(),
        c = controller(hub, n);
    addTearDown(hub.dispose);
    addTearDown(c.dispose);
    final pending = c.loadCatalog();
    await flush();
    hub.boundary();
    n.captureWait!.complete(
      AwxNativeScope.fromJson({
        'version': 1,
        'profileId': f.profile,
        'notificationScopeId': f.nonce,
      }),
    );
    await pending;
    expect(c.current, false);
    expect(c.catalog, isNull);
    expect(n.calls, isEmpty);
  });
  test('sameepoch domain gap preserves unrelated AWX review', () async {
    final hub = RuntimeNoticeEventAdapter(),
        n = Native(),
        c = controller(hub, n);
    addTearDown(hub.dispose);
    addTearDown(c.dispose);
    await selectTemplate(c);
    await c.prepareLaunch();
    hub.ingest({'type': 'lagged'});
    await flush();
    expect(c.current, true);
    expect(c.canLaunch, true);
    expect(n.discards, isEmpty);
  });
  test(
    'globaldone fences even synchronous late prepare before fanout callback',
    () async {
      final hub = RuntimeNoticeEventAdapter(),
          n = Native(),
          c = controller(hub, n);
      addTearDown(hub.dispose);
      addTearDown(c.dispose);
      await selectTemplate(c);
      final op = Op(f.review(), auto: false)..ack.complete();
      n.prepareWait = op;
      final pending = c.prepareLaunch();
      await flush();
      expect(op.starts, 1);
      hub.retireGlobalStream(closed: true);
      expect(c.current, false);
      op.done.complete(op.value);
      op.join.complete();
      await pending;
      await flush();
      expect(c.review, isNull);
      expect(n.discards, ['synthetic-one-use']);
      expect(n.cancelledScope!.notificationScopeId, f.nonce);
    },
  );
  test(
    'cancel bound before registration ack; late ack cannot start new body',
    () async {
      final hub = RuntimeNoticeEventAdapter(),
          n = Native(),
          c = controller(hub, n);
      addTearDown(hub.dispose);
      addTearDown(c.dispose);
      await selectTemplate(c);
      final op = Op(f.review(), auto: false);
      n.prepareWait = op;
      final pending = c.prepareLaunch();
      await flush();
      c.cancelRead();
      await flush();
      expect(n.cancelIds, [n.registeredId]);
      expect(c.busy, true);
      op.ack.complete();
      await flush();
      expect(op.starts, 0);
      expect(n.cancelIds.length, 2);
      expect(c.busy, true);
      op.done.complete(op.value);
      op.join.complete();
      await pending;
      await flush();
      expect(c.busy, false);
      expect(c.review, isNull);
      expect(n.discards, ['synthetic-one-use']);
    },
  );
  test('result completion cannot unblock another action before original worker join', () async {
    final hub = RuntimeNoticeEventAdapter(),
        n = Native(),
        c = controller(hub, n);
    addTearDown(hub.dispose);
    addTearDown(c.dispose);
    await selectTemplate(c);
    final op = Op(f.review(), auto: false)..ack.complete();
    n.prepareWait = op;
    final pending = c.prepareLaunch();
    await flush();
    op.done.complete(op.value);
    await flush();
    expect(c.busy, true);
    await c.launchReviewed();
    expect(n.calls.contains('launch'), false);
    op.join.complete();
    await pending;
    expect(c.canLaunch, true);
  });
  test(
    'Unknown never auto retries on elapsed clock/read/edit and remains visible',
    () async {
      final hub = RuntimeNoticeEventAdapter(),
          n = Native(),
          c = controller(hub, n);
      addTearDown(hub.dispose);
      addTearDown(c.dispose);
      await selectTemplate(c);
      await c.prepareLaunch();
      await c.launchReviewed();
      c.editParameters(AwxLaunchParameters(limit: 'next'));
      await c.loadCatalog();
      await flush();
      expect(c.launchUncertain, true);
      expect(c.outcome, isA<AwxLaunchUnknown>());
      expect(n.calls.where((s) => s == 'launch').length, 1);
      expect(c.canLaunch, false);
    },
  );
  test(
    'revision change catalog invalidates review and old job results',
    () async {
      final hub = RuntimeNoticeEventAdapter(),
          n = Native(),
          c = controller(hub, n);
      addTearDown(hub.dispose);
      addTearDown(c.dispose);
      await selectTemplate(c);
      await c.prepareLaunch();
      n.currentCatalog = AwxCatalog.fromJson({
        'version': 1,
        'catalogRevision': 8,
        'readOnly': false,
        'connections': [
          {
            'id': f.connectionId,
            'revision': 4,
            'configuration': f.configuration(),
            'readOnly': false,
          },
        ],
      });
      await c.loadCatalog();
      expect(c.review, isNull);
      expect(c.template, isNull);
      expect(c.selected!.revision, 4);
      expect(n.calls.contains('launch'), false);
    },
  );
  test(
    'readonly unknown catalog cannot save/delete with inferred zero',
    () async {
      final hub = RuntimeNoticeEventAdapter(),
          n = Native()
            ..currentCatalog = AwxCatalog.fromJson({
              'version': 1,
              'catalogRevision': null,
              'readOnly': true,
              'connections': <Object?>[],
            }),
          c = controller(hub, n);
      addTearDown(hub.dispose);
      addTearDown(c.dispose);
      await c.loadCatalog();
      await c.saveConnection(
        f.connection().configuration!,
        expectedRevision: 3,
      );
      expect(n.calls, ['catalog']);
      expect(c.catalog!.revision, isNull);
    },
  );
}
