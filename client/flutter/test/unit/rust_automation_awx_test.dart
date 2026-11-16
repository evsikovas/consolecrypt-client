import 'dart:async';

import 'package:consolecrypt/automation/automation_awx_handle_gateway.dart';
import 'package:consolecrypt/automation/automation_awx_service.dart';
import 'package:consolecrypt/core/bridge/rust_automation_awx.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/src/rust/api/automation_awx.dart' as native;
import 'package:consolecrypt/src/rust/api/error.dart';
import 'package:flutter_test/flutter_test.dart';

class Handle extends Fake implements native.AwxRequestHandle {
  final done = Completer<void>(), response = Completer<String>();
  int starts = 0, cancels = 0;
  @override
  Future<String> start() {
    starts++;
    return response.future;
  }

  @override
  void cancel() {
    cancels++;
  }

  @override
  Future<void> joined() => done.future;
}

void main() {
  test('actual generated handle adapter keeps cancel synchronous while start awaits and join independent', () async {
    final raw = Handle();
    final seen = <String>[];
    final port = RustAwxOpaqueNativePort(
      register: (json) async {
        seen.add(json);
        return raw;
      },
    );
    final issued = await port.registerRequest('owned exact request');
    final response = issued.start();
    issued.cancel();
    expect(raw.cancels, 1);
    expect(raw.starts, 1);
    var joined = false;
    final joining = issued.joined().then((_) => joined = true);
    raw.response.complete('owned response');
    expect(await response, 'owned response');
    expect(joined, isFalse);
    raw.done.complete();
    await joining;
    expect(seen, ['owned exact request']);
  });
  test('generated scope/catalog/discard forward exact JSON and map actual BridgeError', () async {
    final seen = <String>[];
    final port = RustAwxOpaqueNativePort(
      scope: () async => 'native original scope',
      catalog: (json) async {
        seen.add(json);
        return 'catalog';
      },
      discard: (json) async {
        seen.add(json);
        throw const BridgeError(
          code: 'validation',
          message: 'private',
          reason: 'invalid_field',
          details: {'field': 'automation_awx', 'rule': 'awx_changed'},
        );
      },
    );
    expect(await port.captureScope(), 'native original scope');
    expect(await port.catalogRequest('exact catalog'), 'catalog');
    await expectLater(port.discardReview('original review'), throwsA(isA<AppException>()));
    expect(seen, ['exact catalog', 'original review']);
  });
  test('only allowlisted awx_changed in exact AWX field becomes changed', () {
    AppException error(String field, String rule) => AppException(
      AppErrorCode.validation,
      'private',
      reason: AppErrorReason.invalidField,
      args: {'field': field, 'rule': rule},
    );
    expect(awxSafeFailure(error('automation_awx', 'awx_changed')).reason, AwxFailureReason.changed);
    expect(awxSafeFailure(error('other', 'awx_changed')).reason, AwxFailureReason.unknown);
    expect(awxSafeFailure(error('automation_awx', 'future')).reason, AwxFailureReason.unknown);
  });
}
