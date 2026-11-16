import 'package:consolecrypt/core/bridge/rust_agent_exporter.dart';
import 'package:consolecrypt/src/rust/api/agent_exporter.dart' as native;
import 'package:consolecrypt/src/rust/api/agent_node_association.dart' as association;

/// Actual private FRB233 output, ABI1141874734. Construction is inert.
/// This source remains staged until the matching Native/runtime acceptance.
final class GeneratedAgentExporterCalls implements AgentExporterNativeCalls {
  const GeneratedAgentExporterCalls();
  @override
  Future<String> catalog(String requestJson) => native.agentExporterCatalog(requestJson: requestJson);
  @override
  Future<AgentExporterOriginal> prepare(String requestJson) async =>
      _Original(await native.agentExporterPrepare(requestJson: requestJson));
  @override
  Future<association.AgentNodeAssociationHandle> registerExporters(String requestJson) =>
      association.agentExporterNodeAssociationRegister(requestJson: requestJson);
  @override
  Future<association.AgentNodeRevocationHandle> revokeExporters(String requestJson) =>
      association.agentExporterNodeAssociationRevokePrepare(requestJson: requestJson);
}

/// Exactly the issued object: no later Core or profile lookup, retry or fallback.
final class _Original implements AgentExporterOriginal {
  const _Original(this.original);
  final native.AgentExporterHandle original;
  @override
  Future<String> details() => original.details();
  @override
  Future<String> approveOnce() => original.approveOnce();
  @override
  Future<String> stop() => original.stop();
  @override
  void cancel() => original.cancel();
  @override
  Future<String> joined() => original.joined();
  @override
  void dispose() => original.dispose();
}

/// Available only as a proposed service capability. It performs no enrollment,
/// catalog query, installation, child start or authority change on construction.
RustAgentExporterPort createGeneratedAgentExporterPort() => const RustAgentExporterPort(GeneratedAgentExporterCalls());
