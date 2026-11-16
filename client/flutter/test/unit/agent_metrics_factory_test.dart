import 'package:consolecrypt/core/bridge/rust_agent_association.dart';
import 'package:consolecrypt/core/bridge/rust_agent_metrics_collector.dart';
import 'package:consolecrypt/core/bridge/rust_monitoring.dart';
import 'package:consolecrypt/monitoring/agent_association_port.dart';
import 'package:consolecrypt/monitoring/agent_metrics_collector_port.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('actual monitoring factory exposes separate lazy Metrics collection and enrollment ports without Native initialization', () {
    // No RustLib initialization or mock: a hidden Native call would fail here.
    final service = const RustMonitoringBackend().createService();
    expect(service, isA<AgentMetricsCollectorCapability>());
    final metrics = (service as AgentMetricsCollectorCapability).agentMetricsCollectorPort;
    expect(metrics, isA<RustAgentMetricsCollectorPort>());
    final association = (service as AgentAssociationCapability).agentAssociationPort;
    expect(association, isA<RustAgentAssociationPort>());
    expect(association, isA<AgentMetricsEnrollmentPort>());
  });
}
