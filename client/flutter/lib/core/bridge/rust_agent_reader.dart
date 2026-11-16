import 'package:consolecrypt/core/bridge/mapping.dart' show toAppException;
import 'package:consolecrypt/core/security/vault_key_alarm.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/monitoring/agent_reader_port.dart';
import 'package:consolecrypt/src/rust/api/agent_reader.dart' as native;

const _readerRules = <String>{
  'reader_action_rejected',
  'reader_busy',
  'reader_profile_unsupported',
  'reader_unavailable',
  'reader_outcome_unknown',
  'reader_cancelled',
  'reader_expired',
  'reader_response_invalid',
  'reader_revoke_pending',
};

/// Exact Native invalid-input domain/rule only. Diagnostics/private prose and
/// unrelated errors never become feature permission or a caller-owned grant.
Exception readerBridgeFailure(Object error) {
  final mapped = toAppException(error);
  // The existing blocking vault-key alarm has priority over ANY Reader code.
  if (reportVaultKeyMismatch(mapped)) return mapped;
  if (mapped.code == AppErrorCode.validation &&
      mapped.reason == AppErrorReason.invalidField &&
      mapped.args['field'] == 'agent_reader' &&
      _readerRules.contains(mapped.args['rule'])) {
    return AgentReaderException.fromCode(mapped.args['rule']!);
  }
  return AgentReaderException.fromCode('reader_unavailable');
}

Future<T> readerBridgeCall<T>(Future<T> Function() original) async {
  try {
    return await original();
  } catch (error) {
    throw readerBridgeFailure(error);
  }
}

T readerBridgeSync<T>(T Function() original) {
  try {
    return original();
  } catch (error) {
    throw readerBridgeFailure(error);
  }
}

/// Actual generated functions are lazy: factory/menu construction makes no RPC.
final class RustAgentReaderPort implements AgentReaderNativePort {
  const RustAgentReaderPort({this.prepare = _prepare, this.readStatus = _status});
  final Future<native.ReaderActionHandle> Function(String) prepare;
  final Future<String> Function(String) readStatus;
  static Future<native.ReaderActionHandle> _prepare(String raw) => native.agentReaderPrepareAction(requestJson: raw);
  static Future<String> _status(String raw) => native.agentReaderStatus(requestJson: raw);
  @override
  Future<AgentReaderIssuedHandle> prepareAction(String requestJson) async =>
      _OriginalReaderHandle(await readerBridgeCall(() => prepare(requestJson)));
  @override
  Future<String> status(String requestJson) => readerBridgeCall(() => readStatus(requestJson));
}

/// All calls retain the SAME issued Native owner, including cancellation.
final class _OriginalReaderHandle implements AgentReaderIssuedHandle {
  const _OriginalReaderHandle(this.original);
  final native.ReaderActionHandle original;
  @override
  String details() => readerBridgeSync(original.details);
  @override
  Future<String> confirm() => readerBridgeCall(original.confirm);
  @override
  void cancel() => readerBridgeSync(original.cancel);
  @override
  Future<String> consumeRead() => readerBridgeCall(original.consumeRead);
  @override
  Future<String> transferExport({required String publicDestination, String? invitationDestination}) => readerBridgeCall(
    () => original.transferExport(publicDestination: publicDestination, invitationDestination: invitationDestination),
  );
}
