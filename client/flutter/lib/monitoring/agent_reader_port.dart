/// Optional fixed Native capability. No HTTP, key material or authority fallback.
abstract interface class AgentReaderIssuedHandle {
  String details();
  Future<String> confirm();
  void cancel();
  Future<String> consumeRead();
  Future<String> transferExport({required String publicDestination, String? invitationDestination});
}

abstract interface class AgentReaderNativePort {
  Future<AgentReaderIssuedHandle> prepareAction(String requestJson);
  Future<String> status(String requestJson);
}

abstract interface class AgentReaderCapability {
  AgentReaderNativePort? get agentReaderPort;
}

/// Closed errors supplied by the generated adapter; remote prose is discarded.
final class AgentReaderException implements Exception {
  const AgentReaderException._(this.code);
  factory AgentReaderException.fromCode(String code) => AgentReaderException._(
    const {
          'reader_action_rejected',
          'reader_busy',
          'reader_profile_unsupported',
          'reader_unavailable',
          'reader_outcome_unknown',
          'reader_cancelled',
          'reader_expired',
          'reader_response_invalid',
          'reader_revoke_pending',
        }.contains(code)
        ? code
        : 'reader_unavailable',
  );
  final String code;
  @override
  String toString() => 'AgentReaderException($code)';
}
