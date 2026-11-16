import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/automation/automation_portable_controller.dart';
import 'package:consolecrypt/automation/automation_portable_lifetime.dart';
import 'package:consolecrypt/automation/automation_portable_package.dart';
import 'package:consolecrypt/core/models/models.dart' show generateUuidV4;
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/file_dialog_service.dart';

/// Internal invocation, not JSON/receipt/approval. The separately reviewed
/// opaque gateway supplies actual original Native registration and join.
/// result may fail early; joined MUST resolve only after the ORIGINAL registered
/// operation and every FS worker have retired. Transport failure is not join.
final class PortableNativeInvocation {
  const PortableNativeInvocation({
    required this.registered,
    required this.startBody,
    required this.result,
    required this.joined,
  });

  /// Actual Native registered-idle acknowledgement, never a caller JSON flag.
  final Future<void> registered;

  /// Only enqueue body after registration AND the original UI/cancel recheck.
  final void Function() startBody;
  final Future<Map<String, dynamic>> result;
  final Future<void> joined;
}

abstract interface class PortableNativeGateway {
  /// Actual guarded automation_portable_scope only. No observer-nonce fallback.
  Future<Map<String, dynamic>> captureScope();

  /// These methods enqueue REGISTRATION ONLY for ORIGINAL scope. operationId is
  /// instance-local Dart cancellation correlation; it is NOT a Native-issued ID.
  /// Delayed registration needs a separate idle ACK/start handshake, with cancel
  /// bound to its actual original transfer/issued handle even before late ACK.
  /// They cannot throw after enqueue without returning a joined handle.
  PortableNativeInvocation context({
    required PortableScope scope,
    required String operationId,
    required String projectId,
    required int projectRevision,
  });

  /// Copy password synchronously into a gateway-owned bounded secret buffer
  /// before return. Keep it until actual Native ACK wraps SecretString, then
  /// wipe on ACK/failure/retirement. No JSON password, credential lookup or
  /// latest-scope fallback. Asynchronous registration is NOT synchronous ACK.
  PortableNativeInvocation export({
    required PortableScope scope,
    required String operationId,
    required Map<String, dynamic> request,
    required SecretText password,
    required String destination,
  });
  PortableNativeInvocation inspect({
    required PortableScope scope,
    required String operationId,
    required String source,
    required SecretText password,
  });

  /// Original-scope operation only, even after the Dart lifetime is retired.
  /// This is NOT the existing latest-owner cancel_setup call used unguarded.
  Future<void> cancel({required PortableScope scope, required String operationId});
}

/// Original-scope Dart boundary over an optional separately bound opaque gateway.
/// Only capture() constructs a bound adapter from the ACTUAL initial Native DTO.
final class AutomationPortableNativeAdapter implements AutomationPortablePackageService {
  AutomationPortableNativeAdapter._(this._gateway, this.lifetime, this.scope);
  final PortableNativeGateway _gateway;
  final PortableUiLifetime lifetime;
  final PortableScope scope;
  bool _closed = false;
  bool get current => !_closed && lifetime.current;

  static Future<AutomationPortableNativeAdapter> capture({
    required PortableUiLifetime lifetime,
    PortableNativeGateway? gateway,
  }) async {
    if (gateway == null) throw const PortablePackageFailure('portable_backend_unavailable');
    if (!lifetime.current) throw const PortablePackageFailure('portable_context_changed');
    final originalProfile = lifetime.profileId;
    late final Map<String, dynamic> raw;
    try {
      raw = await gateway.captureScope();
    } on PortablePackageFailure catch (error) {
      if (!lifetime.current) throw const PortablePackageFailure('portable_context_changed');
      if (error.code == 'portable_backend_unavailable') rethrow;
      throw const PortablePackageFailure('portable_operation_failed');
    } catch (_) {
      throw PortablePackageFailure(lifetime.current ? 'portable_operation_failed' : 'portable_context_changed');
    }
    if (!lifetime.current) throw const PortablePackageFailure('portable_context_changed');
    final scope = PortableScope.fromJson(raw);
    if (scope.profileId != originalProfile) throw const PortablePackageFailure('portable_context_changed');
    return AutomationPortableNativeAdapter._(gateway, lifetime, scope);
  }

  void _require(PortableScope expected, PortableCancellation cancellation) {
    if (!current || cancellation.cancelled || !expected.same(scope)) {
      throw const PortablePackageFailure('portable_context_changed');
    }
  }

  void _cancelOriginal(String id) {
    try {
      unawaited(
        _gateway.cancel(scope: scope, operationId: id).then<void>((_) {}, onError: (Object _, StackTrace _) {}),
      );
    } catch (_) {
      // UI retirement/wiping is unconditional. Native join is a separate await.
    }
  }

  Future<Map<String, dynamic>> _invoke(
    PortableCancellation cancellation,
    PortableNativeInvocation Function(String id) start, {
    SecretText? ownedPassword,
  }) async {
    var bound = false;
    try {
      _require(scope, cancellation);
      final id = generateUuidV4();
      // Registration of the Dart cancel forwarding is BEFORE gateway start or
      // the first async suspension, not after an operation's result arrives.
      cancellation.bind(() => _cancelOriginal(id));
      bound = true;
      _require(scope, cancellation);
      late final PortableNativeInvocation invocation;
      try {
        invocation = start(id);
      } finally {
        // Gateway copies this input synchronously into its OWN bounded secret
        // buffer. The gateway retains that separate copy until real Native ACK
        // (Native wraps SecretString before body), then wipes it. This adapter's
        // input copy never survives the first suspension; ACK is not fabricated.
        ownedPassword?.wipe();
      }
      // Observe result errors immediately, even when registration is rejected
      // and no body starts. An abandoned error Future must not leak diagnostics.
      final outcome = invocation.result.then<({Map<String, dynamic>? value, bool failed})>(
        (value) => (value: value, failed: false),
        onError: (Object _, StackTrace _) => (value: null, failed: true),
      );
      var failed = false, started = false;
      try {
        await invocation.registered;
      } catch (_) {
        failed = true;
      }
      if (!failed && current && !cancellation.cancelled) {
        try {
          invocation.startBody();
          started = true;
        } catch (_) {
          failed = true;
        }
      }
      if (!started) _cancelOriginal(id); // exact original scope, also after a late registration
      final reply = started ? await outcome : null;
      try {
        await invocation.joined;
      } catch (_) {
        failed = true;
      }
      _require(scope, cancellation);
      if (failed || !started || reply == null || reply.failed || reply.value == null) {
        throw const PortablePackageFailure('portable_operation_failed');
      }
      return reply.value!;
    } finally {
      ownedPassword?.wipe();
      if (bound) cancellation.unbind();
    }
  }

  AutomationPortableController createController({
    required FileDialogService files,
    String? projectId,
    int? projectRevision,
  }) {
    if (!current) throw const PortablePackageFailure('portable_context_changed');
    return AutomationPortableController(
      scope: scope,
      service: this,
      files: files,
      isOriginalCurrent: () => current,
      originalLifetime: lifetime,
      projectId: projectId,
      projectRevision: projectRevision,
    );
  }

  SecretText _copyPassword(SecretText source) {
    final bytes = source.exposeBytes();
    try {
      return SecretText.fromBytes(bytes);
    } finally {
      bytes.fillRange(0, bytes.length, 0);
    }
  }

  @override
  Future<PortableContext> context(
    PortableScope expected,
    String projectId,
    int revision,
    PortableCancellation cancellation,
  ) async {
    _require(expected, cancellation);
    final raw = await _invoke(
      cancellation,
      (id) => _gateway.context(scope: scope, operationId: id, projectId: projectId, projectRevision: revision),
    );
    final value = PortableContext.fromJson(raw);
    if (!value.scope.same(scope) || value.projectId != projectId || value.projectRevision != revision) {
      throw const PortablePackageFailure('portable_context_changed');
    }
    return value;
  }

  @override
  Future<PortableExportOutcome> export(
    Map<String, dynamic> request,
    SecretText password,
    String destination,
    PortableCancellation cancellation,
  ) async {
    _require(PortableScope.fromJson(request['scope']), cancellation);
    final raw = jsonEncode(request);
    if (utf8.encode(raw).length > portableMaxExportRequestBytes || !portablePackageFilename(destination)) {
      throw const PortablePackageFailure('portable_selection_invalid');
    }
    final snapshot = jsonDecode(raw) as Map<String, dynamic>;
    final owned = _copyPassword(password);
    return PortableExportOutcome.fromJson(
      await _invoke(
        cancellation,
        (id) => _gateway.export(
          scope: scope,
          operationId: id,
          request: snapshot,
          password: owned,
          destination: destination,
        ),
        ownedPassword: owned,
      ),
    );
  }

  @override
  Future<PortableImportPreview> inspect(
    PortableScope expected,
    String source,
    SecretText password,
    PortableCancellation cancellation,
  ) async {
    _require(expected, cancellation);
    if (!portablePackageFilename(source)) throw const PortablePackageFailure('portable_selection_invalid');
    final owned = _copyPassword(password);
    return PortableImportPreview.fromJson(
      await _invoke(
        cancellation,
        (id) => _gateway.inspect(scope: scope, operationId: id, source: source, password: owned),
        ownedPassword: owned,
      ),
    );
  }

  void dispose() {
    if (_closed) return;
    _closed = true;
    lifetime.retire(); // Existing controller listener cancels/wipes immediately.
    lifetime.dispose();
  }
}
