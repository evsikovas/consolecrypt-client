import 'dart:async';

import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_service.dart';

import 'monitoring_fakes.dart';

const bindingHost = MonitoringHostRef(id: '11111111-1111-4111-8111-111111111111', name: 'Explicit saved host');
const otherBindingHost = MonitoringHostRef(id: '44444444-4444-4444-8444-444444444444', name: 'Other saved host');
MonitoringBinding bindingFixture({
  String id = '33333333-3333-4333-8333-333333333333',
  String name = 'Node exporter',
  String? sourceId,
  int sourceRevision = 1,
  MonitoringBindingState state = MonitoringBindingState.current,
  Map<String, String> selector = const {'job': 'node', 'instance': 'manual:9100'},
}) => MonitoringBinding(
  id: id,
  name: name,
  hostId: bindingHost.id,
  sourceId: sourceId ?? monitoringFixture.id,
  revision: 7,
  sourceRevision: sourceRevision,
  selector: selector,
  state: state,
);

/// Synthetic UI contract only. Actual reference validation and catalog migration
/// are verified separately in Native fixture tests.
class MonitoringBindingTestStore extends MonitoringTestStore implements MonitoringBindingStore {
  MonitoringBindingTestStore({super.initial, List<MonitoringBinding> rows = const []}) : bindings = List.of(rows);
  @override
  List<MonitoringBinding> bindings;
  final saves = <(MonitoringBinding, bool)>[];
  final deleted = <String>[];
  int loads = 0;
  MonitoringFailure? saveError;
  Completer<MonitoringBinding>? pendingSave;
  @override
  Future<List<MonitoringSource>> load() async {
    loads++;
    return super.load();
  }

  @override
  Future<MonitoringBinding> saveBinding(MonitoringBinding binding, {bool create = false}) async {
    saves.add((binding, create));
    if (saveError != null) throw saveError!;
    final saved =
        await (pendingSave?.future ??
            Future.value(
              MonitoringBinding(
                id: create ? '55555555-5555-4555-8555-555555555555' : binding.id,
                name: binding.name,
                hostId: binding.hostId,
                sourceId: binding.sourceId,
                revision: binding.revision + 1,
                sourceRevision: binding.sourceRevision,
                selector: binding.selector,
                state: MonitoringBindingState.current,
              ),
            ));
    bindings = [
      for (final old in bindings)
        if (old.id != saved.id) old,
      saved,
    ];
    return saved;
  }

  @override
  Future<void> deleteBinding(MonitoringBinding binding) async {
    deleted.add(binding.id);
    bindings = bindings.where((b) => b.id != binding.id).toList();
  }
}
