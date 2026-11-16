import 'package:consolecrypt/core/bridge/rust_agent_collector_service.dart';
import 'package:consolecrypt/core/bridge/rust_agent_collector_service_credentials.dart';
import 'package:consolecrypt/core/bridge/rust_agent_collector_service_lifetime.dart';
import 'package:consolecrypt/core/bridge/rust_agent_collector_service_revocation.dart';
import 'package:consolecrypt/core/bridge/rust_agent_collector_service_status.dart';

/// Exact six free operations from the selected Rust Bridge leaf. This interface
/// does not imply that generated bindings or a production capability are active.
abstract interface class AgentCollectorServiceNativeCalls {
  Future<AgentCollectorServiceNativeHandle> prepare(String requestJson);
  Future<String> catalog(String requestJson);
  Future<String> status(String requestJson);
  Future<AgentCollectorServiceRevocationNativeHandle> revokePrepare(
    String requestJson,
  );
  Future<String> credentialChoices(String requestJson);
  Future<AgentCollectorServiceNativeHandle> lifetimePrepare(String requestJson);
}

/// Lazy composition: every operation stays explicit and each issued proxy keeps
/// its original opaque owner. Lifetime delegation has its own prepare operation.
/// Main's optional capability remains absent until the actual binding is accepted.
RustAgentCollectorServiceWithLifetime createRustAgentCollectorServicePort({
  required AgentCollectorServiceNativeCalls calls,
}) {
  final base = RustAgentCollectorServicePort(
    prepareOriginal: calls.prepare,
    readCatalogJson: calls.catalog,
  );
  final status = RustAgentCollectorServiceWithStatus(
    original: base,
    readJson: calls.status,
  );
  final revocation = RustAgentCollectorServiceWithRevocation(
    original: base,
    status: status,
    prepareOriginal: calls.revokePrepare,
  );
  final credentials = RustAgentCollectorServiceWithCredentials(
    original: base,
    status: status,
    revocation: revocation,
    readJson: calls.credentialChoices,
  );
  return RustAgentCollectorServiceWithLifetime(
    original: base,
    credentials: credentials,
    status: status,
    revocation: revocation,
    prepareLifetimeOriginal: calls.lifetimePrepare,
  );
}
