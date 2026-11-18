import 'dart:async';

import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/monitoring/monitoring_binding_models.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_result_view.dart';
import 'package:consolecrypt/monitoring/monitoring_service.dart';
import 'package:consolecrypt/monitoring/monitoring_time_window.dart';
import 'package:material_ui/material_ui.dart';

/// Fast path for an existing Prometheus-compatible source.
class MonitoringMetricBrowser extends StatefulWidget {
  const MonitoringMetricBrowser({super.key, required this.source, required this.service, required this.isCurrent});
  final MonitoringSource source;
  final MonitoringService service;
  final bool Function() isCurrent;

  @override
  State<MonitoringMetricBrowser> createState() => _MonitoringMetricBrowserState();
}

typedef _Metric = ({String name, String expression, MonitoringUnit? unit});
final _metricName = RegExp(r'^[a-zA-Z_:][a-zA-Z0-9_:]*$');

class _MonitoringMetricBrowserState extends State<MonitoringMetricBrowser> {
  List<String>? _names;
  bool _partial = false, _reading = false, _values = false, _showAll = false;
  String _filter = '';
  int _minutes = 60, _generation = 0;
  Object? _error;
  _Metric? _selected;
  QueryOutcome? _result;
  MonitoringTimeWindow? _window;

  bool _current(int token) => mounted && widget.isCurrent() && token == _generation;

  Future<void> _loadNames() async {
    if (_reading || !widget.isCurrent()) return;
    final token = ++_generation;
    setState(() {
      _showAll = true;
      _reading = true;
      _error = null;
    });
    try {
      final names = await widget.service.labelValues(widget.source, '__name__');
      if (!_current(token)) return;
      if (names.error != null) throw names.error!;
      setState(() {
        _names = names.values.where((name) => name.length <= 1024 && _metricName.hasMatch(name)).toSet().toList()
          ..sort();
        _partial = names.partial;
      });
    } catch (error) {
      if (_current(token)) setState(() => _error = error);
    } finally {
      if (_current(token)) setState(() => _reading = false);
    }
  }

  Future<void> _open(_Metric metric) async {
    if (!widget.isCurrent()) return;
    final token = ++_generation;
    widget.service.cancel();
    final end = DateTime.now().millisecondsSinceEpoch / 1000;
    final history = _minutes != 0;
    final step = _minutes == 360 ? 60.0 : 15.0;
    final start = end - _minutes * 60;
    setState(() {
      _selected = metric;
      _result = null;
      _window = null;
      _error = null;
      _reading = true;
    });
    try {
      final outcome = await widget.service.query(
        widget.source,
        history
            ? MonitoringQuery(expression: metric.expression, start: start, end: end, stepSeconds: step)
            : MonitoringQuery(expression: metric.expression),
      );
      if (!_current(token)) return;
      setState(() {
        _result = outcome;
        _window = history ? MonitoringTimeWindow.fromEpoch(start, end, step) : null;
      });
    } catch (error) {
      if (_current(token)) setState(() => _error = error);
    } finally {
      if (_current(token)) setState(() => _reading = false);
    }
  }

  void _back() {
    ++_generation;
    widget.service.cancel();
    setState(() {
      _selected = null;
      _result = null;
      _error = null;
      _reading = false;
      _values = false;
    });
  }

  @override
  void dispose() {
    ++_generation;
    widget.service.cancel();
    super.dispose();
  }

  List<_Metric> _presets(AppLocalizations l) => [
    (
      name: l.monitoringLiveCpu,
      unit: MonitoringUnit.percent,
      expression: '100 * (1 - avg without (cpu, mode) (rate(node_cpu_seconds_total{mode="idle"}[5m])))',
    ),
    (
      name: l.monitoringLiveRam,
      unit: MonitoringUnit.percent,
      expression: '100 * (1 - node_memory_MemAvailable_bytes / node_memory_MemTotal_bytes)',
    ),
    (
      name: l.monitoringLiveDisk,
      unit: MonitoringUnit.percent,
      expression:
          '100 * (1 - node_filesystem_avail_bytes{mountpoint="/"} / node_filesystem_size_bytes{mountpoint="/"})',
    ),
    (name: l.monitoringBrowseUp, unit: null, expression: 'up'),
  ];

  @override
  Widget build(BuildContext context) {
    final l = context.l10n, metric = _selected, result = _result;
    final names = _names?.where((name) => name.toLowerCase().contains(_filter)).toList();
    return AlertDialog(
      key: const ValueKey('monitoring-metric-browser'),
      title: Text(metric?.name ?? l.monitoringBrowseTitle),
      content: SizedBox(
        width: 820,
        height: (MediaQuery.sizeOf(context).height - 190).clamp(230.0, 610.0),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(widget.source.name, style: Theme.of(context).textTheme.titleSmall),
            if (metric != null)
              Wrap(
                spacing: 6,
                crossAxisAlignment: WrapCrossAlignment.center,
                children: [
                  for (final (duration, label) in [
                    (0, l.monitoringBrowseNow),
                    (15, l.monitoringTime15Minutes),
                    (60, l.monitoringTime1Hour),
                    (360, l.monitoringTime6Hours),
                  ])
                    ChoiceChip(
                      label: Text(label),
                      selected: _minutes == duration,
                      onSelected: (_) {
                        if (_minutes == duration) return;
                        _minutes = duration;
                        unawaited(_open(metric));
                      },
                    ),
                  if (_minutes != 0) ...[
                    ChoiceChip(
                      label: Text(l.monitoringSetupGraphs),
                      selected: !_values,
                      onSelected: (_) => setState(() => _values = false),
                    ),
                    ChoiceChip(
                      label: Text(l.monitoringLiveValues),
                      selected: _values,
                      onSelected: (_) => setState(() => _values = true),
                    ),
                  ],
                ],
              )
            else ...[
              Text(l.monitoringBrowseHint),
              const SizedBox(height: 8),
              Wrap(
                spacing: 6,
                runSpacing: 6,
                children: [
                  for (final item in _presets(l)) OutlinedButton(onPressed: () => _open(item), child: Text(item.name)),
                  if (!_showAll) FilledButton(onPressed: _loadNames, child: Text(l.monitoringBrowseAll)),
                ],
              ),
              if (_showAll) ...[
                const SizedBox(height: 8),
                TextField(
                  key: const ValueKey('monitoring-metric-search'),
                  decoration: InputDecoration(
                    labelText: l.monitoringBrowseSearch,
                    prefixIcon: const Icon(Icons.search),
                  ),
                  onChanged: (value) => setState(() => _filter = value.trim().toLowerCase()),
                ),
                if (_partial) Text(l.monitoringPartialMetadata),
              ],
            ],
            if (_reading) const LinearProgressIndicator(),
            if (_error case final error?) InfoBanner(message: errorMessage(l, error), tone: BannerTone.danger),
            if (_error != null)
              Align(
                alignment: Alignment.centerLeft,
                child: TextButton(
                  onPressed: metric == null ? _loadNames : () => _open(metric),
                  child: Text(l.commonRetry),
                ),
              ),
            if (metric != null && result != null)
              Expanded(
                child: SingleChildScrollView(
                  child: MonitoringResultView(
                    result: result,
                    requestedWindow: _window,
                    unit: metric.unit,
                    showChart: _minutes != 0 && !_values,
                    showValues: _minutes == 0 || _values || result.data?.kind != 'matrix',
                    showDiagnostics: false,
                  ),
                ),
              )
            else if (metric == null && names != null)
              Expanded(
                child: names.isEmpty
                    ? Center(child: Text(l.monitoringBrowseNoMatch))
                    : ListView.builder(
                        itemCount: names.length,
                        itemBuilder: (context, index) {
                          final name = names[index];
                          return ListTile(
                            dense: true,
                            title: Text(name, overflow: TextOverflow.ellipsis),
                            trailing: const Icon(Icons.show_chart),
                            onTap: () => _open((name: name, expression: name, unit: null)),
                          );
                        },
                      ),
              ),
          ],
        ),
      ),
      actions: [
        if (metric != null) TextButton(onPressed: _back, child: Text(l.commonBack)),
        TextButton(onPressed: () => Navigator.of(context).pop(), child: Text(l.commonClose)),
      ],
    );
  }
}
