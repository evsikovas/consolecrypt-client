import 'dart:typed_data';

import 'package:consolecrypt/automation/automation_portable_handle_gateway.dart';
import 'package:consolecrypt/automation/automation_portable_import_gateway.dart';
import 'package:consolecrypt/automation/automation_portable_import_session.dart';
import 'package:consolecrypt/automation/automation_portable_package.dart';
import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/security/vault_key_alarm.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/src/rust/api/automation_portable.dart' as native;
import 'package:consolecrypt/src/rust/api/automation_portable_import.dart' as native_import;

T _sync<T>(T Function() body) {
  try {
    return body();
  } catch (error) {
    final mapped = toAppException(error);
    reportVaultKeyMismatch(mapped);
    throw mapped;
  }
}

/// Thin actual generated binding, no Core/profile lookup or caller Native ID.
/// Constructing the const port makes zero RPC. The existing UI lifetime and
/// Native-issued handles retain original ownership independently.
final class RustPortableOpaqueNativePort implements PortableOpaqueNativePort {
  const RustPortableOpaqueNativePort({
    this.scope = native.automationPortableScope,
    this.createTransfer = _createTransfer,
    this.context = _context,
    this.export = _export,
    this.inspect = _inspect,
  });
  final Future<String> Function() scope;
  final native.PortablePasswordTransfer Function(List<int>) createTransfer;
  final Future<native.PortableRequestHandle> Function(String, String, int) context;
  final Future<native.PortableRequestHandle> Function(String, native.PortablePasswordTransfer, String) export;
  final Future<native.PortableRequestHandle> Function(String, String, native.PortablePasswordTransfer) inspect;

  static native.PortablePasswordTransfer _createTransfer(List<int> bytes) =>
      native.PortablePasswordTransfer(passwordBytes: bytes);
  static Future<native.PortableRequestHandle> _context(String scope, String project, int revision) =>
      native.automationPortableRegisterContext(scopeJson: scope, projectId: project, projectRevision: revision);
  static Future<native.PortableRequestHandle> _export(
    String json,
    native.PortablePasswordTransfer password,
    String destination,
  ) => native.automationPortableRegisterExport(requestJson: json, password: password, destination: destination);
  static Future<native.PortableRequestHandle> _inspect(
    String scope,
    String source,
    native.PortablePasswordTransfer password,
  ) => native.automationPortableRegisterInspect(scopeJson: scope, source: source, password: password);

  @override
  Future<String> captureScope() => guard(scope);
  @override
  PortableIssuedPasswordTransfer transferPassword(Uint8List bytes) => _RustTransfer(_sync(() => createTransfer(bytes)));
  @override
  Future<PortableIssuedRequestHandle> registerContext(String scopeJson, String projectId, int projectRevision) async =>
      _RustHandle(await guard(() => context(scopeJson, projectId, projectRevision)));
  @override
  Future<PortableIssuedRequestHandle> registerExport(
    String requestJson,
    PortableIssuedPasswordTransfer password,
    String destination,
  ) async {
    final original = _borrow(password);
    // Keep the exact original transfer borrow for the whole registration Future.
    // Gateway normal release only happens AFTER this actual Future ACK returns.
    return _RustHandle(await guard(() => export(requestJson, original, destination)));
  }

  @override
  Future<PortableIssuedRequestHandle> registerInspect(
    String scopeJson,
    String source,
    PortableIssuedPasswordTransfer password,
  ) async {
    final original = _borrow(password);
    return _RustHandle(await guard(() => inspect(scopeJson, source, original)));
  }
}

native.PortablePasswordTransfer _borrow(PortableIssuedPasswordTransfer value) {
  if (value is! _RustTransfer || value.original == null) {
    throw const PortablePackageFailure('portable_operation_failed');
  }
  return value.original!;
}

final class _RustTransfer implements PortableIssuedPasswordTransfer {
  _RustTransfer(this.original);
  native.PortablePasswordTransfer? original;
  @override
  void cancel() {
    final value = original;
    if (value != null) _sync(value.cancel);
  }

  @override
  void release() {
    final value = original;
    original = null;
    // Normal opaque Drop disposes remaining VALUE ownership only. Do not call
    // cancel(): that would also retire the separately handed-off native request.
    if (value != null) _sync(value.dispose);
  }
}

final class _RustHandle implements PortableIssuedRequestHandle {
  const _RustHandle(this.original);
  final native.PortableRequestHandle original;
  @override
  Future<String> start() => guard(original.start);
  @override
  void cancel() => _sync(original.cancel);
  @override
  Future<void> joined() => guard(original.joined);
}

/// The distinct generated import owner preserves Ready until explicit acceptance.
/// Construction/factories make no RPC and reuse the existing opaque PW transfer.
final class RustPortableImportOpaqueNativePort implements PortableImportOpaqueNativePort {
  const RustPortableImportOpaqueNativePort({
    this.createTransfer = RustPortableOpaqueNativePort._createTransfer,
    this.register = _register,
  });
  final native.PortablePasswordTransfer Function(List<int>) createTransfer;
  final Future<native_import.PortableImportHandle> Function(
    String, String, String, native.PortablePasswordTransfer,
  ) register;

  static Future<native_import.PortableImportHandle> _register(
    String scope, String source, String name, native.PortablePasswordTransfer password,
  ) => native_import.automationPortableRegisterImport(
    scopeJson: scope, source: source, requestedName: name, password: password,
  );

  @override
  PortableIssuedPasswordTransfer transferPassword(Uint8List bytes) => _RustTransfer(_sync(() => createTransfer(bytes)));
  @override
  Future<PortableIssuedImportHandle> registerImport({
    required String scopeJson, required String source, required String requestedName,
    required PortableIssuedPasswordTransfer password,
  }) async {
    final original = _borrow(password);
    return _RustImportHandle(await _importGuard(() => register(scopeJson, source, requestedName, original)));
  }
}

Future<T> _importGuard<T>(Future<T> Function() body) async {
  try {
    return await guard(body);
  } on AppException catch (error) {
    if (error.code == AppErrorCode.validation && error.args['field'] == 'portable_import') {
      final issue = switch (error.args['rule']) {
        'portable_context_changed' => PortableImportIssue.changed,
        'portable_import_source_changed' => PortableImportIssue.sourceChanged,
        'portable_import_unsupported' => PortableImportIssue.unsupported,
        'portable_import_busy' => PortableImportIssue.busy,
        'portable_import_expired' => PortableImportIssue.expired,
        'portable_import_recovery_required' => PortableImportIssue.recoveryRequired,
        _ => PortableImportIssue.unavailable,
      };
      throw PortableImportFailure(issue);
    }
    rethrow; // an unknown accept failure becomes recovery in the gateway
  }
}

final class _RustImportHandle implements PortableIssuedImportHandle {
  const _RustImportHandle(this.original);
  final native_import.PortableImportHandle original;
  @override
  Future<String> start() => _importGuard(original.start);
  @override
  Future<String> acceptImport({required String reviewId, required String reviewDigest}) =>
      _importGuard(() => original.acceptImport(reviewId: reviewId, reviewDigest: reviewDigest));
  @override
  void cancel() => _sync(original.cancel);
  @override
  Future<void> joined() => guard(original.joined);
}
