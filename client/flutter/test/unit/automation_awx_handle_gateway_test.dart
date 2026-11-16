import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/automation/automation_awx_controller.dart';
import 'package:consolecrypt/automation/automation_awx_handle_gateway.dart';
import 'package:consolecrypt/automation/automation_awx_models.dart';
import 'package:consolecrypt/automation/automation_awx_service.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/services/runtime_notice_adapter.dart';
import 'package:flutter_test/flutter_test.dart';

import 'automation_awx_models_test.dart' as f;

String envelope(String kind, Object? data) =>
    jsonEncode({'version': 1, 'kind': kind, 'data': data});
String prepared(AwxLaunchParameters p) => envelope('prepare', {
  'reviewTicket': '77777777-7777-4777-8777-777777777777',
  'template': f.templateJson(),
  'parameters': p.toJson(),
  'expiresUtc': '2026-11-12T11:02:00Z',
  'connectionId': f.connectionId,
  'connectionRevision': 3,
});

class Handle implements AwxIssuedRequestHandle {
  Handle({this.auto});
  final String? auto;
  final response = Completer<String>(), joinedResponse = Completer<void>();
  int starts = 0, cancels = 0;
  @override
  Future<String> start() {
    starts++;
    if (auto != null) {
      response.complete(auto);
      joinedResponse.complete();
    }
    return response.future;
  }

  @override
  void cancel() {
    cancels++;
    if (starts == 0 && !joinedResponse.isCompleted) joinedResponse.complete();
  }

  @override
  Future<void> joined() => joinedResponse.future;
}

class Port implements AwxOpaqueNativePort {
  final requests = <Map<String, dynamic>>[],
      catalogs = <Map<String, dynamic>>[],
      discards = <Map<String, dynamic>>[];
  final handles = <Handle>[];
  Completer<String>? captureWait;
  Completer<AwxIssuedRequestHandle>? registrationWait;
  Handle? nextHandle;
  int captures = 0;
  @override
  Future<String> captureScope() async {
    captures++;
    return captureWait?.future ??
        jsonEncode({
          'version': 1,
          'profileId': f.profile,
          'notificationScopeId': f.nonce,
        });
  }

  @override
  Future<String> catalogRequest(String raw) async {
    catalogs.add(jsonDecode(raw) as Map<String, dynamic>);
    return jsonEncode({
      'version': 1,
      'catalogRevision': 7,
      'readOnly': false,
      'connections': [
        {
          'id': f.connectionId,
          'revision': 3,
          'configuration': f.configuration(),
          'readOnly': false,
        },
      ],
    });
  }

  @override
  Future<AwxIssuedRequestHandle> registerRequest(String raw) async {
    final j = jsonDecode(raw) as Map<String, dynamic>;
    requests.add(j);
    if (registrationWait != null) return registrationWait!.future;
    final a = j['action'] as Map<String, dynamic>;
    final result = switch (a['kind']) {
      'templates' => envelope('templates', {
        'templates': [f.templateJson()],
        'has_next': false,
        'page': a['page'],
      }),
      'prepare' => prepared(AwxLaunchParameters.fromJson(a['parameters'])),
      'launch' => envelope('launch', {
        'status': 'unknown',
        'reviewConsumed': true,
      }),
      'job' => envelope('job', f.jobJson()),
      'events' => envelope('events', {
        'events': <Object?>[],
        'has_next': false,
        'after': a['after'],
      }),
      _ => throw StateError('unexpected fixed action'),
    };
    final handle = nextHandle ?? Handle(auto: result);
    nextHandle = null;
    handles.add(handle);
    return handle;
  }

  @override
  Future<void> discardReview(String raw) async {
    discards.add(jsonDecode(raw) as Map<String, dynamic>);
  }
}

AwxHandleGateway gateway(Port? port, {bool Function()? current}) =>
    AwxHandleGateway(
      profileId: f.profile,
      isOriginalCurrent: current ?? () => true,
      port: port,
      now: () => f.now,
    );
Future<T> run<T>(AwxRegisteredOperation<T> op) async {
  final observed = op.result;
  await op.registered;
  op.startBody();
  final result = await observed;
  await op.joined;
  return result;
}

Future<void> flush() async {
  for (var i = 0; i < 30; i++) {
    await Future<void>.value();
  }
}

void main() {
  test(
    'missing Native opaque port remains unavailable without fallback',
    () async {
      final g = gateway(null);
      await expectLater(
        g.captureScope(),
        throwsA(
          isA<AwxFailure>().having(
            (e) => e.reason,
            'reason',
            AwxFailureReason.unavailable,
          ),
        ),
      );
    },
  );
  test(
    'capture is original one-shot; sameprofile late replacement is not adopted',
    () async {
      var current = true;
      final p = Port()..captureWait = Completer(),
          g = gateway(p, current: () => current);
      final pending = g.captureScope();
      current = false;
      p.captureWait!.complete(
        jsonEncode({
          'version': 1,
          'profileId': f.profile,
          'notificationScopeId': f.nonce,
        }),
      );
      await expectLater(pending, throwsA(isA<AwxFailure>()));
      current = true;
      await expectLater(g.captureScope(), throwsA(isA<AwxFailure>()));
      expect(p.captures, 1);
      expect(p.requests, isEmpty);
    },
  );
  test('fixed read wire uses only originalscope/native references, no local NativeID or endpoint', () async {
    final p = Port(), g = gateway(p), s = await g.captureScope();
    final op = g.registerTemplates(
      s,
      'LOCAL-CORRELATION',
      f.connection(),
      1,
      'literal & search',
    );
    await op.registered;
    expect(p.handles.single.starts, 0);
    expect(p.requests.single, {
      'version': 1,
      'profileId': f.profile,
      'notificationScopeId': f.nonce,
      'action': {
        'kind': 'templates',
        'connectionId': f.connectionId,
        'connectionRevision': 3,
        'page': 1,
        'search': 'literal & search',
      },
    });
    op.startBody();
    expect((await op.result).templates.single.id, 9);
    await op.joined;
  });
  test(
    'cancel while registration pending binds latehandle before any body',
    () async {
      final p = Port()..registrationWait = Completer(),
          g = gateway(p),
          s = await g.captureScope();
      final op = g.registerTemplates(s, 'local', f.connection(), 1, '');
      final failure = expectLater(op.result, throwsA(isA<AwxFailure>()));
      await g.cancel(s, 'local');
      final h = Handle();
      p.registrationWait!.complete(h);
      await op.registered;
      await op.joined;
      await failure;
      expect(h.cancels, 1);
      expect(h.starts, 0);
      expect(() => op.startBody(), returnsNormally);
      expect(h.starts, 0);
    },
  );
  test('closed original epoch during registration cancels latehandle without recapture', () async {
    var current = true;
    final p = Port()..registrationWait = Completer(),
        g = gateway(p, current: () => current),
        s = await g.captureScope();
    final op = g.registerTemplates(s, 'local', f.connection(), 1, '');
    final failed = expectLater(op.result, throwsA(isA<AwxFailure>()));
    current = false;
    final h = Handle();
    p.registrationWait!.complete(h);
    await op.registered;
    await op.joined;
    await failed;
    expect(h.starts, 0);
    expect(h.cancels, 1);
    expect(p.captures, 1);
  });
  test('early result does not signal joined; cancel still reaches that issued handle', () async {
    final h = Handle(),
        p = Port()..nextHandle = h,
        g = gateway(p),
        s = await g.captureScope();
    final op = g.registerTemplates(s, 'local', f.connection(), 1, '');
    await op.registered;
    op.startBody();
    h.response.complete(
      envelope('templates', {
        'templates': <Object?>[],
        'has_next': false,
        'page': 1,
      }),
    );
    await op.result;
    var joined = false;
    unawaited(
      op.joined.then<void>((_) {
        joined = true;
      }),
    );
    await flush();
    expect(joined, false);
    await g.cancel(s, 'local');
    expect(h.cancels, 1);
    expect(joined, false);
    h.joinedResponse.complete();
    await op.joined;
  });
  test('prepare onlyreads then launch sends ONLY issued review ticket exactly once', () async {
    final p = Port(), g = gateway(p), s = await g.captureScope();
    final params = AwxLaunchParameters(jobType: 'check', limit: 'exact');
    final review = await run(
      g.registerPrepare(s, 'prepare', f.connection(), 9, params),
    );
    expect(p.requests.single['action'], {
      'kind': 'prepare',
      'connectionId': f.connectionId,
      'connectionRevision': 3,
      'templateId': 9,
      'parameters': params.toJson(),
    });
    final launched = await run(
      g.registerLaunch(s, 'launch', review.nativeTicket),
    );
    expect(launched, isA<AwxLaunchUnknown>());
    expect(p.requests.last['action'], {
      'kind': 'launch',
      'reviewTicket': review.nativeTicket,
    });
    expect(
      () => g.registerLaunch(s, 'again', review.nativeTicket),
      throwsA(isA<AwxFailure>()),
    );
    await flush();
    expect(p.requests.length, 2);
  });
  test('Unknown is never retried by gateway or close', () async {
    final p = Port(), g = gateway(p), s = await g.captureScope();
    final r = await run(
      g.registerPrepare(s, 'p', f.connection(), 9, AwxLaunchParameters()),
    );
    await run(g.registerLaunch(s, 'l', r.nativeTicket));
    await flush();
    await g.close();
    expect(
      p.requests.where((j) => (j['action'] as Map)['kind'] == 'launch').length,
      1,
    );
  });
  test('late prepared result after close is discarded using exact originalscope only', () async {
    final h = Handle(),
        p = Port()..nextHandle = h,
        g = gateway(p),
        s = await g.captureScope();
    final op = g.registerPrepare(
      s,
      'p',
      f.connection(),
      9,
      AwxLaunchParameters(),
    );
    final failed = expectLater(op.result, throwsA(isA<AwxFailure>()));
    await op.registered;
    op.startBody();
    await g.close();
    h.response.complete(prepared(AwxLaunchParameters()));
    h.joinedResponse.complete();
    await op.joined;
    await failed;
    await flush();
    expect(p.discards.single, {
      'version': 1,
      'profileId': f.profile,
      'notificationScopeId': f.nonce,
      'reviewTicket': '77777777-7777-4777-8777-777777777777',
    });
    expect(p.captures, 1);
  });
  test('discard foreign ticket/scope does not call Native cleanup', () async {
    final p = Port(), g = gateway(p), s = await g.captureScope();
    await g.discardReview(s, 'foreign');
    expect(p.discards, isEmpty);
    await g.cancel(s, 'foreign');
    expect(p.requests, isEmpty);
  });
  test('bound operation quota and duplicate local IDs deny before another Native registration', () async {
    final p = Port()..registrationWait = Completer(),
        g = gateway(p),
        s = await g.captureScope();
    for (var i = 0; i < 32; i++) {
      g.registerTemplates(s, '$i', f.connection(), 1, '');
    }
    expect(
      () => g.registerTemplates(s, '32', f.connection(), 1, ''),
      throwsA(isA<AwxFailure>()),
    );
    expect(
      () => g.registerTemplates(s, '0', f.connection(), 1, ''),
      throwsA(isA<AwxFailure>()),
    );
    expect(p.requests.length, 32);
    await g.close();
    final h = Handle();
    p.registrationWait!.complete(h);
    await flush();
    expect(h.starts, 0);
  });
  test('safe mapping is exactfield/code/reason and allowlist, never server messages', () {
    const error = AppException(
      AppErrorCode.validation,
      'private diagnostics must not render',
      reason: AppErrorReason.invalidField,
      args: {'field': 'automation_awx', 'rule': 'awx_needs_awx_input'},
    );
    expect(awxSafeFailure(error).reason, AwxFailureReason.needsAwxInput);
    expect(
      awxSafeFailure(
        const AppException(
          AppErrorCode.validation,
          'private',
          reason: AppErrorReason.invalidField,
          args: {'field': 'other', 'rule': 'awx_needs_awx_input'},
        ),
      ).reason,
      AwxFailureReason.unknown,
    );
    expect(
      awxSafeFailure(
        const AppException(
          AppErrorCode.validation,
          'private',
          reason: AppErrorReason.invalidField,
          args: {'field': 'automation_awx', 'rule': 'future_native_rule'},
        ),
      ).reason,
      AwxFailureReason.unknown,
    );
  });
  test(
    'catalog save is exact flat CAS refs and never raw credentials',
    () async {
      final p = Port(), g = gateway(p), s = await g.captureScope();
      await g.catalog(s);
      await g.saveConnection(
        s,
        expectedCatalogRevision: 7,
        expectedRevision: 3,
        configuration: f.connection().configuration!,
      );
      expect(p.catalogs.last, {
        'version': 1,
        'profileId': f.profile,
        'notificationScopeId': f.nonce,
        'kind': 'save',
        'expectedCatalogRevision': 7,
        'expectedRevision': 3,
        'configuration': f.configuration(),
      });
      expect(p.requests, isEmpty);
    },
  );
  test('real controller plus typed gateway uses one original capture and exact one explicit launch', () async {
    final hub = RuntimeNoticeEventAdapter(), p = Port();
    final epoch = hub.currentScope.epoch;
    final g = gateway(
      p,
      current: () =>
          hub.currentScope.epoch == epoch &&
          !hub.currentScope.streamUnavailable,
    );
    var id = 0;
    final c = AwxController(
      profileId: f.profile,
      events: hub,
      isOriginalUiCurrent: () => true,
      operationId: () => 'local-${id++}',
      gateway: g,
      now: () => f.now,
    );
    addTearDown(hub.dispose);
    addTearDown(g.close);
    addTearDown(c.dispose);
    await c.loadCatalog();
    c.selectConnection(c.catalog!.connections.single);
    await c.loadTemplates();
    c.selectTemplate(c.templates!.templates.single);
    c.editParameters(AwxLaunchParameters(jobType: 'check', limit: 'explicit'));
    await c.prepareLaunch();
    expect(c.canLaunch, true);
    expect(p.requests.map((r) => (r['action'] as Map)['kind']), [
      'templates',
      'prepare',
    ]);
    await c.launchReviewed();
    expect(c.launchUncertain, true);
    expect(c.review, isNull);
    await c.launchReviewed();
    expect(p.requests.length, 3);
    expect(p.captures, 1);
    hub.retireGlobalStream(closed: true);
    await flush();
    expect(c.current, false);
  });
  test('Native joined failure is propagated and retires gateway before another registration', () async {
    final h = Handle(),
        p = Port()..nextHandle = h,
        g = gateway(p),
        s = await g.captureScope();
    final op = g.registerTemplates(s, 'local', f.connection(), 1, '');
    final resultFailed = expectLater(op.result, throwsA(isA<AwxFailure>()));
    final joinFailed = expectLater(op.joined, throwsA(isA<AwxFailure>()));
    await op.registered;
    op.startBody();
    h.joinedResponse.completeError(StateError('private fixture failure'));
    await resultFailed;
    await joinFailed;
    expect(
      () => g.registerTemplates(s, 'next', f.connection(), 1, ''),
      throwsA(isA<AwxFailure>()),
    );
    expect(p.requests.length, 1);
    h.response.complete(
      envelope('templates', {
        'templates': <Object?>[],
        'has_next': false,
        'page': 1,
      }),
    );
    await flush();
  });
}
