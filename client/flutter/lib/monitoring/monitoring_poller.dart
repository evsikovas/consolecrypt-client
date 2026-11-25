import 'dart:async';
import 'dart:math' as math;

import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/services/host_metrics_service.dart';
import 'package:flutter/foundation.dart';

typedef MonitorReading = ({HostMetricsSample? sample, Object? error});

/// Each refresh reads at most four hosts concurrently. Stale completions cannot
/// replace a sample after leaving the dashboard or editing the host inventory.
class MonitoringPoller extends ChangeNotifier {
  MonitoringPoller(this.reader);
  final DashboardMetricsReader reader;
  final readings = <ObjectId, MonitorReading>{};
  final _active = <String>{};
  List<Host> _hosts = const [];
  Timer? _timer;
  int _seconds = 30, _generation = 0;
  bool busy = false;

  void configure(List<Host> hosts, int seconds) {
    final unchanged =
        hosts.length == _hosts.length &&
        Iterable<int>.generate(hosts.length)
            .every((i) => hosts[i].id == _hosts[i].id && hosts[i].updatedAt == _hosts[i].updatedAt);
    if (unchanged) {
      if (seconds != _seconds) {
        _seconds = seconds;
        if (!busy) _schedule();
      }
      return;
    }
    final previous = {for (final h in _hosts) h.id: h.updatedAt};
    _cancel();
    final retained = {
      for (final h in hosts)
        if (previous[h.id] == h.updatedAt) h.id,
    };
    readings.removeWhere((id, _) => !retained.contains(id));
    _hosts = hosts;
    _seconds = seconds;
    if (hosts.isNotEmpty) unawaited(refresh());
  }

  Future<void> refresh() async {
    if (busy || _hosts.isEmpty) return;
    _timer?.cancel();
    final generation = _generation, hosts = _hosts;
    busy = true;
    notifyListeners();
    var index = 0;
    Future<void> worker() async {
      while (generation == _generation && index < hosts.length) {
        final host = hosts[index++];
        final id = generateUuidV4();
        _active.add(id);
        try {
          final sample = await reader.sampleHost(host, id);
          if (generation == _generation) readings[host.id] = (sample: sample, error: null);
        } catch (e) {
          if (generation == _generation) readings[host.id] = (sample: readings[host.id]?.sample, error: e);
        } finally {
          _active.remove(id);
        }
        if (generation == _generation) notifyListeners();
      }
    }

    await Future.wait(List.generate(math.min(4, hosts.length), (_) => worker()));
    if (generation != _generation) return;
    busy = false;
    notifyListeners();
    _schedule();
  }

  void _schedule() {
    _timer?.cancel();
    if (_seconds > 0 && _hosts.isNotEmpty) _timer = Timer(Duration(seconds: _seconds), refresh);
  }

  void _cancel() {
    _generation++;
    _timer?.cancel();
    busy = false;
    for (final id in _active) {
      unawaited(reader.cancelHostWatch(id).catchError((Object _) {}));
    }
    _active.clear();
  }

  @override
  void dispose() {
    _cancel();
    super.dispose();
  }
}
