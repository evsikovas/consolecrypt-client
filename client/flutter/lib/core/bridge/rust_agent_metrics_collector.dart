import 'dart:convert';
import 'package:consolecrypt/core/bridge/mapping.dart' show guard,toAppException;
import 'package:consolecrypt/core/security/vault_key_alarm.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_metrics_collector_models.dart';
import 'package:consolecrypt/monitoring/agent_metrics_collector_port.dart';
import 'package:consolecrypt/monitoring/agent_metrics_collector_wire.dart';
import 'package:consolecrypt/src/rust/api/agent_collector.dart' as native;

abstract interface class AgentMetricsCollectorNativeCalls {
  Future<native.AgentCollectorHandle> prepare(String requestJson);
}
final class GeneratedAgentMetricsCollectorCalls implements AgentMetricsCollectorNativeCalls {
  const GeneratedAgentMetricsCollectorCalls();
  @override Future<native.AgentCollectorHandle> prepare(String requestJson)=>native.agentCollectorPrepare(requestJson:requestJson);
}
void _sync(void Function() call){try{call();}catch(error){final mapped=toAppException(error);reportVaultKeyMismatch(mapped);throw mapped;}}
final class RustAgentMetricsCollectorPort implements AgentMetricsCollectorPort {
  const RustAgentMetricsCollectorPort({this.calls=const GeneratedAgentMetricsCollectorCalls()});
  final AgentMetricsCollectorNativeCalls calls;
  @override Future<AgentMetricsCollectorHandle> prepare(RuntimeNoticeScope scope,AgentMetricsCollectorIntent intent)async=>_Original(
    await guard(()=>calls.prepare(jsonEncode(intent.request(scope)))),scope,intent);
}
final class _Original implements AgentMetricsCollectorHandle {
  _Original(this.original,this.scope,this.intent);
  final native.AgentCollectorHandle original;
  final RuntimeNoticeScope scope;
  final AgentMetricsCollectorIntent intent;
  bool disposed=false;
  void live(){if(disposed)throw StateError('Original collection proxy disposed');}
  @override Future<AgentMetricsCollectorView> details()async{live();return parseAgentMetricsCollectorView(await guard(original.details),scope,intent);}
  @override Future<AgentMetricsCollectorView> approveOnce()async{live();return parseAgentMetricsCollectorView(await guard(original.approveOnce),scope,intent);}
  @override Future<AgentMetricsCollectorView> stop()async{live();return parseAgentMetricsCollectorView(await guard(original.stop),scope,intent);}
  @override void cancel(){live();_sync(original.cancel);}
  @override Future<AgentMetricsCollectorCleanup> joined()async{live();return parseAgentMetricsCollectorCleanup(await guard(original.joined));}
  @override void dispose(){if(disposed)return;disposed=true;_sync(original.dispose);}
}
