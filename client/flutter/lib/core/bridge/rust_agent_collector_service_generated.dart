import 'package:consolecrypt/core/bridge/rust_agent_collector_service.dart';
import 'package:consolecrypt/core/bridge/rust_agent_collector_service_factory.dart';
import 'package:consolecrypt/core/bridge/rust_agent_collector_service_lifetime.dart';
import 'package:consolecrypt/core/bridge/rust_agent_collector_service_revocation.dart';
import 'package:consolecrypt/src/rust/api/agent_collector_service.dart' as native;

/// Actual FRB 2.13.0 output, private generation ABI -714013538. Construction is
/// lazy. Main does not install this capability before the coherent Native gate.
final class GeneratedAgentCollectorServiceCalls implements AgentCollectorServiceNativeCalls {
  const GeneratedAgentCollectorServiceCalls();
  @override
  Future<AgentCollectorServiceNativeHandle> prepare(String raw) async =>
      _Configuration(await native.agentCollectorServicePrepare(requestJson: raw));
  @override
  Future<String> catalog(String raw) => native.agentCollectorServiceCatalog(requestJson: raw);
  @override
  Future<String> status(String raw) => native.agentCollectorServiceStatus(requestJson: raw);
  @override
  Future<AgentCollectorServiceRevocationNativeHandle> revokePrepare(String raw) async =>
      _Revocation(await native.agentCollectorServiceRevokePrepare(requestJson: raw));
  @override
  Future<String> credentialChoices(String raw) => native.agentCollectorServiceCredentialChoices(requestJson: raw);
  @override
  Future<AgentCollectorServiceNativeHandle> lifetimePrepare(String raw) async =>
      _Configuration(await native.agentCollectorServiceLifetimePrepare(requestJson: raw));
}

RustAgentCollectorServiceWithLifetime createGeneratedAgentCollectorServicePort({
  AgentCollectorServiceNativeCalls calls = const GeneratedAgentCollectorServiceCalls(),
}) => createRustAgentCollectorServicePort(calls: calls);

/// Same issued generated object for all calls, including cancellation and final
/// cleanup. Existing typed proxies apply guards and validate the original DTO.
final class _Configuration implements AgentCollectorServiceNativeHandle {
  _Configuration(this.original);
  final native.AgentCollectorServiceHandle original;
  @override
  Future<String> details() => original.details();
  @override
  Future<String> confirmConfiguration() => original.confirmConfiguration();
  @override
  void cancel() => original.cancel();
  @override
  Future<String> joined() => original.joined();
  @override
  void dispose() => original.dispose();
}

final class _Revocation implements AgentCollectorServiceRevocationNativeHandle {
  _Revocation(this.original);
  final native.AgentCollectorServiceRevocationHandle original;
  @override
  Future<String> details() => original.details();
  @override
  Future<String> confirmRevocation() => original.confirmRevocation();
  @override
  void cancel() => original.cancel();
  @override
  Future<String> joined() => original.joined();
  @override
  void dispose() => original.dispose();
}
