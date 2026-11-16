import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';

import 'package:consolecrypt/automation/automation_awx_handle_gateway.dart';
import 'package:consolecrypt/automation/automation_dependency_models.dart';
import 'package:consolecrypt/automation/automation_portable_import_gateway.dart';
import 'package:consolecrypt/automation/automation_portable_import_session.dart';
import 'package:consolecrypt/automation/automation_portable_service.dart';
import 'package:consolecrypt/core/bridge/rust_automation.dart';
import 'package:consolecrypt/core/bridge/rust_automation_portable.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/src/rust/api/automation_portable_import.dart' as native;
import 'package:consolecrypt/src/rust/api/error.dart';
import 'package:flutter_test/flutter_test.dart';

import '../helpers/portable_import_fixture.dart' as f;
import 'rust_automation_portable_test.dart' show Transfer;

class Handle extends Fake implements native.PortableImportHandle {
  final started = Completer<String>(), accepted = Completer<String>(), retired = Completer<void>();
  int starts = 0, accepts = 0, cancels = 0;
  String? id, digest;
  Object? error;
  @override
  Future<String> start() {
    starts++;
    return error == null ? started.future : Future.error(error!);
  }

  @override
  Future<String> acceptImport({required String reviewId, required String reviewDigest}) {
    accepts++;
    id = reviewId;
    digest = reviewDigest;
    return error == null ? accepted.future : Future.error(error!);
  }

  @override
  void cancel() {
    cancels++;
  }

  @override
  Future<void> joined() => retired.future;
}

BridgeError failure(String field, String rule, {String code = 'invalid_input'}) => BridgeError(
  code: code,
  message: 'SYNTHETIC_PRIVATE',
  reason: 'invalid_field',
  details: {'field': field, 'rule': rule},
);

void main() {
  test('production factory is lazy and preserves prior portable/AWX/dependency capabilities', () {
    final service = RustAutomationService();
    expect(service, isA<AutomationPortableImportReviewCapability>());
    expect(service, isA<AutomationPortableGatewayCapability>());
    expect(service, isA<AutomationAwxPortCapability>());
    expect(service, isA<AutomationDependencyService>());
    expect(service.createPortableImportReviewGateway(), isA<PortableImportHandleGateway>());
  });
  test('actual generated registration receives original transfer/scope/rawname through pending ACK', () async {
    final transfer = Transfer(), raw = Handle(), ack = Completer<native.PortableImportHandle>();
    final seen = <Object>[];
    final port = RustPortableImportOpaqueNativePort(
      createTransfer: (_) => transfer,
      register: (scope, source, name, password) {
        seen.addAll([scope, source, name, password]);
        return ack.future;
      },
    );
    final secret = port.transferPassword(Uint8List.fromList([65]));
    final pending = port.registerImport(
      scopeJson: 'ORIGINAL_SCOPE',
      source: 'owned.ccproject',
      requestedName: ' Raw ',
      password: secret,
    );
    expect(seen, ['ORIGINAL_SCOPE', 'owned.ccproject', ' Raw ', transfer]);
    secret.cancel();
    expect(transfer.cancels, 1);
    expect(transfer.disposes, 0);
    ack.complete(raw);
    final issued = await pending;
    secret.release();
    expect(transfer.disposes, 1);
    expect(raw.cancels, 0);
    final read = issued.start();
    issued.cancel();
    expect(raw.cancels, 1);
    raw.started.complete('Ready exact string');
    expect(await read, 'Ready exact string');
    raw.retired.complete();
    await issued.joined();
  });
  test('closed import mapping requires validation+exact field and fixed rule', () async {
    final raw = Handle(), transfer = Transfer();
    final port = RustPortableImportOpaqueNativePort(
      createTransfer: (_) => transfer,
      register: (_, _, _, _) async => raw,
    );
    final secret = port.transferPassword(Uint8List.fromList([65]));
    final issued = await port.registerImport(
      scopeJson: 'scope',
      source: 'file.ccproject',
      requestedName: 'name',
      password: secret,
    );
    secret.release();
    final cases = {
      'portable_context_changed': PortableImportIssue.changed,
      'portable_import_source_changed': PortableImportIssue.sourceChanged,
      'portable_import_unsupported': PortableImportIssue.unsupported,
      'portable_import_busy': PortableImportIssue.busy,
      'portable_import_expired': PortableImportIssue.expired,
      'portable_import_recovery_required': PortableImportIssue.recoveryRequired,
      'portable_operation_unavailable': PortableImportIssue.unavailable,
    };
    for (final entry in cases.entries) {
      raw.error = failure('portable_import', entry.key);
      await expectLater(
        issued.start(),
        throwsA(isA<PortableImportFailure>().having((e) => e.issue, 'issue', entry.value)),
      );
    }
    raw.error = failure('foreign', 'portable_import_expired');
    await expectLater(issued.start(), throwsA(isA<AppException>()));
    raw.error = failure('portable_import', 'portable_import_expired', code: 'internal');
    await expectLater(issued.start(), throwsA(isA<AppException>()));
    raw.retired.complete();
    await issued.joined();
  });
  test('real port plus production gateway returns Ready before joined and same refs accept on same handle', () async {
    final transfer = Transfer(), raw = Handle();
    final gateway = PortableImportHandleGateway(
      port: RustPortableImportOpaqueNativePort(createTransfer: (_) => transfer, register: (_, _, _, _) async => raw),
    );
    final intent = PortableImportIntent.capture(scope: f.scope(), source: 'file.ccproject', requestedName: ' Raw ');
    final password = SecretText('SYNTHETIC_PASSWORD');
    final registration = gateway.register(intent: intent, password: password);
    password.wipe();
    final session = await registration.registered;
    expect(transfer.disposes, 1);
    expect(transfer.cancels, 0);
    final preview = session.preview();
    raw.started.complete(jsonEncode(f.readyJson(intent)));
    await preview;
    bool joined = false;
    unawaited(registration.joined.then((_) => joined = true));
    await f.flush();
    expect(joined, isFalse);
    final ready = PortableImportReady.fromJson(f.readyJson(intent), intent: intent);
    final result = session.accept(reviewId: ready.reviewId, reviewDigest: ready.reviewDigest);
    expect(raw.id, ready.reviewId);
    expect(raw.digest, ready.reviewDigest);
    expect(raw.accepts, 1);
    raw.accepted.complete(jsonEncode(f.installedJson(ready)));
    await result;
    expect(joined, isFalse);
    raw.retired.complete();
    await registration.joined;
    expect(transfer.cancels, 0);
  });
}
