import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_metrics_collector_models.dart';

abstract interface class AgentMetricsCollectorCapability {AgentMetricsCollectorPort? get agentMetricsCollectorPort;}
abstract interface class AgentMetricsCollectorPort {
  Future<AgentMetricsCollectorHandle> prepare(RuntimeNoticeScope scope,AgentMetricsCollectorIntent intent);
}
abstract interface class AgentMetricsCollectorHandle {
  Future<AgentMetricsCollectorView> details();
  Future<AgentMetricsCollectorView> approveOnce();
  Future<AgentMetricsCollectorView> stop();
  void cancel();
  Future<AgentMetricsCollectorCleanup> joined();
  void dispose();
}
