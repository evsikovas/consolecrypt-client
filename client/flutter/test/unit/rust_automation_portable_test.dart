import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';

import 'package:consolecrypt/automation/automation_dependency_models.dart';
import 'package:consolecrypt/automation/automation_inventory_export.dart';
import 'package:consolecrypt/automation/automation_portable_handle_gateway.dart';
import 'package:consolecrypt/automation/automation_portable_service.dart';
import 'package:consolecrypt/automation/automation_runner_cleanup.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/bridge/rust_automation.dart';
import 'package:consolecrypt/core/bridge/rust_automation_portable.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/src/rust/api/automation_portable.dart' as native;
import 'package:consolecrypt/src/rust/api/error.dart';
import 'package:flutter_test/flutter_test.dart';

import 'automation_portable_controller_test.dart' as f;
import 'automation_portable_handle_gateway_test.dart' as g;

class Transfer extends Fake implements native.PortablePasswordTransfer {
  int cancels = 0, disposes = 0;
  @override
  void cancel() {
    cancels++;
  }

  @override
  void dispose() {
    disposes++;
  }

  @override
  bool get isDisposed => disposes != 0;
}

class Handle extends Fake implements native.PortableRequestHandle {
  final reply = Completer<String>(), retired = Completer<void>();
  int starts = 0, cancels = 0;
  @override
  Future<String> start() {
    starts++;
    return reply.future;
  }

  @override
  void cancel() {
    cancels++;
  }

  @override
  Future<void> joined() => retired.future;
}

void main() {
  test('production factory preserves existing capabilities and makes zero Native RPC on construction', () {
    // RustLib is deliberately UNINITIALIZED. Any constructor/factory RPC fails.
    const port = RustPortableOpaqueNativePort();
    expect(port, isA<PortableOpaqueNativePort>());
    final service = RustAutomationService();
    expect(service, isA<AutomationPortableGatewayCapability>());
    expect(service, isA<AutomationService>());
    expect(service, isA<AutomationDependencyService>());
    expect(service, isA<AutomationInventoryExportService>());
    expect(service, isA<AutomationRunnerCleanupService>());
    final a = service.createPortableGateway(), b = service.createPortableGateway();
    expect(a, isA<PortableHandleGateway>());
    expect(identical(a, b), isFalse);
  });
  test('sync actual generated transfer factory receives only bytes and normal release never cancels', () {
    final raw = Transfer(), bytes = Uint8List.fromList([65, 66, 67]);
    List<int>? exact;
    final port = RustPortableOpaqueNativePort(
      createTransfer: (value) {
        exact = value;
        return raw;
      },
    );
    final transfer = port.transferPassword(bytes);
    expect(exact, same(bytes));
    expect(raw.cancels, 0);
    expect(raw.disposes, 0);
    transfer.release();
    transfer.release();
    transfer.cancel();
    expect(raw.disposes, 1);
    expect(raw.cancels, 0);
  });
  test('generated context handle keeps synchronous cancel while start and original join are pending', () async {
    final raw = Handle();
    final seen = <Object>[];
    final port = RustPortableOpaqueNativePort(
      context: (scope, project, revision) async {
        seen.addAll([scope, project, revision]);
        return raw;
      },
    );
    final issued = await port.registerContext('original exact scope', f.project, 1);
    final result = issued.start();
    issued.cancel();
    expect(raw.cancels, 1);
    expect(raw.starts, 1);
    var joined = false;
    final done = issued.joined().then((_) => joined = true);
    raw.reply.complete('owned result');
    expect(await result, 'owned result');
    expect(joined, isFalse);
    raw.retired.complete();
    await done;
    expect(seen, ['original exact scope', f.project, 1]);
  });
  test('generated export retains original transfer across real pending ACK then releases without cancel', () async {
    final rawTransfer = Transfer(), rawHandle = Handle(), ack = Completer<native.PortableRequestHandle>();
    final seen = <Object>[];
    final port = RustPortableOpaqueNativePort(
      createTransfer: (_) => rawTransfer,
      export: (json, password, path) {
        seen.addAll([json, password, path]);
        return ack.future;
      },
    );
    final transfer = port.transferPassword(Uint8List.fromList([65]));
    final pending = port.registerExport('original request JSON', transfer, 'owned.ccproject');
    transfer.cancel();
    expect(rawTransfer.cancels, 1);
    expect(rawTransfer.disposes, 0);
    expect(seen, ['original request JSON', rawTransfer, 'owned.ccproject']);
    ack.complete(rawHandle);
    final handle = await pending;
    transfer.release();
    expect(rawTransfer.disposes, 1);
    expect(rawTransfer.cancels, 1);
    expect(rawHandle.cancels, 0);
    handle.cancel();
    expect(rawHandle.cancels, 1);
    rawHandle.retired.complete();
    await handle.joined();
  });
  test('generated inspect forwards exact original scope/source/transfer and safely maps BridgeError', () async {
    final transfer = Transfer(), handle = Handle(), seen = <Object>[];
    final port = RustPortableOpaqueNativePort(
      scope: () async => jsonEncode(f.scope().toJson()),
      createTransfer: (_) => transfer,
      inspect: (scope, source, password) async {
        seen.addAll([scope, source, password]);
        return handle;
      },
    );
    expect(await port.captureScope(), jsonEncode(f.scope().toJson()));
    final secret = port.transferPassword(Uint8List.fromList([65]));
    final issued = await port.registerInspect('original scope', 'owned.ccproject', secret);
    expect(seen, ['original scope', 'owned.ccproject', transfer]);
    secret.release();
    handle.retired.complete();
    await issued.joined();
    final fail = RustPortableOpaqueNativePort(
      scope: () async => throw const BridgeError(
        code: 'validation',
        message: 'SYNTHETIC_PRIVATE',
        reason: 'invalid_field',
        details: {'field': 'portable_project', 'rule': 'portable_operation_unavailable'},
      ),
    );
    await expectLater(fail.captureScope(), throwsA(isA<AppException>()));
  });
  test('real thin port composed with gateway preserves successful ACK disposal and original response', () async {
    final rawTransfer = Transfer(), rawHandle = Handle(), ack = Completer<native.PortableRequestHandle>();
    final port = RustPortableOpaqueNativePort(
      scope: () async => jsonEncode(f.scope().toJson()),
      createTransfer: (_) => rawTransfer,
      export: (_, password, _) {
        expect(password, same(rawTransfer));
        return ack.future;
      },
    );
    final gateway = PortableHandleGateway(port: port);
    await gateway.captureScope();
    final op = g.Observed(
      gateway.export(
        scope: f.scope(),
        operationId: 'LOCAL_ONLY',
        request: {'scope': f.scope().toJson()},
        password: SecretText('SYNTHETIC_PASSWORD'),
        destination: 'owned.ccproject',
      ),
    );
    expect(rawTransfer.disposes, 0);
    ack.complete(rawHandle);
    await op.registered;
    expect(rawTransfer.disposes, 1);
    expect(rawTransfer.cancels, 0);
    expect(rawHandle.starts, 0);
    op.invocation.startBody();
    expect(rawHandle.starts, 1);
    rawHandle.reply.complete(jsonEncode({'version': 1, 'encryptedSha256': 'a' * 64, 'bytes': 1000}));
    expect((await op.result)!['bytes'], 1000);
    rawHandle.retired.complete();
    expect(await op.joined, isTrue);
    expect(rawTransfer.cancels, 0);
    expect(rawHandle.cancels, 0);
  });
}
