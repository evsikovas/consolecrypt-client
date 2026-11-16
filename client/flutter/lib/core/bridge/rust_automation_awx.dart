import 'package:consolecrypt/automation/automation_awx_handle_gateway.dart';
import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/security/vault_key_alarm.dart';
import 'package:consolecrypt/src/rust/api/automation_awx.dart' as native;

/// The generated opaque handle retains ORIGINAL Native ownership. No caller ID
/// or action is accepted at start, cancel or join. This adapter performs no HTTP.
final class RustAwxOpaqueNativePort implements AwxOpaqueNativePort {
  const RustAwxOpaqueNativePort({
    this.scope = native.automationAwxScope,
    this.catalog = _catalog,
    this.register = _register,
    this.discard = _discard,
  });
  final Future<String> Function() scope;
  final Future<String> Function(String) catalog;
  final Future<native.AwxRequestHandle> Function(String) register;
  final Future<void> Function(String) discard;
  static Future<String> _catalog(String json) => native.automationAwxCatalogRequest(requestJson: json);
  static Future<native.AwxRequestHandle> _register(String json) =>
      native.automationAwxRegisterRequest(requestJson: json);
  static Future<void> _discard(String json) => native.automationAwxDiscardReview(requestJson: json);
  @override
  Future<String> captureScope() => guard(scope);
  @override
  Future<String> catalogRequest(String requestJson) => guard(() => catalog(requestJson));
  @override
  Future<AwxIssuedRequestHandle> registerRequest(String requestJson) async =>
      _RustAwxHandle(await guard(() => register(requestJson)));
  @override
  Future<void> discardReview(String requestJson) => guard(() => discard(requestJson));
}

final class _RustAwxHandle implements AwxIssuedRequestHandle {
  const _RustAwxHandle(this.original);
  final native.AwxRequestHandle original;
  @override
  Future<String> start() => guard(original.start);
  @override
  Future<void> joined() => guard(original.joined);
  @override
  void cancel() {
    try {
      original.cancel();
    } catch (error) {
      final mapped = toAppException(error);
      reportVaultKeyMismatch(mapped);
      throw mapped;
    }
  }
}
