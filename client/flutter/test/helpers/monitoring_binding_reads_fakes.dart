import 'dart:async';

import 'package:consolecrypt/monitoring/monitoring_binding_models.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_service.dart';

import 'monitoring_binding_fakes.dart';
import 'monitoring_fakes.dart';

Json identityOutcome({int candidates = 1, bool partial = false, List<String> warnings = const []}) => {
  'status': 'success',
  'httpStatus': 200,
  'partial': partial,
  'warnings': warnings,
  'infos': <String>[],
  'data': {
    'kind': 'vector',
    'series': [
      for (var i = 0; i < candidates; i++)
        {
          'labels': {'__name__': 'up', 'job': 'node', 'instance': 'manual:9100'},
          'samples': [
            {
              'timestamp': 1000,
              'value': {'kind': 'finite', 'value': i == 0 ? 0 : 1},
            },
          ],
        },
    ],
  },
};
Json bindingDiscoveryJson({String state = 'resolved', int candidates = 1}) => {
  'kind': 'bindingDiscovery',
  'bindingId': bindingFixture().id,
  'bindingRevision': 7,
  'state': state,
  'discovery': identityOutcome(candidates: candidates),
  if (state != 'unavailable')
    'resolution': {
      'state': state,
      if (state == 'resolved') 'candidateIndex': 0,
      if (state == 'ambiguous') 'candidateIndices': [0, 1],
    },
  if (state == 'unavailable') 'reason': 'discoveryInvalid',
};
Json metricOutcome({bool partial = false, bool empty = false}) => {
  'status': 'success',
  'httpStatus': 200,
  'partial': partial,
  'warnings': partial ? ['Synthetic metric warning'] : <String>[],
  'infos': ['Synthetic annotation'],
  'effectiveStepSeconds': 15,
  'data': {
    'kind': 'matrix',
    'series': empty
        ? <Json>[]
        : [
            {
              'labels': {'instance': 'manual:9100', 'mountpoint': '/'},
              'samples': [
                {
                  'timestamp': 970,
                  'value': {'kind': 'finite', 'value': 2},
                },
                {
                  'timestamp': 985,
                  'value': {'kind': 'nan'},
                },
                {
                  'timestamp': 1000,
                  'value': {'kind': 'positiveInfinity'},
                },
              ],
            },
          ],
  },
};
Json bindingPanelJson({
  MonitoringPanel panel = MonitoringPanel.memory,
  String state = 'ready',
  bool limited = false,
  bool partial = false,
}) => {
  'kind': 'bindingPanel',
  'bindingId': bindingFixture().id,
  'bindingRevision': 7,
  'panel': panel.name,
  'state': state,
  'resolution': {'state': 'resolved', 'candidateIndex': 0},
  'discovery': identityOutcome(),
  'preparation': {
    'state': 'ready',
    'probe': identityOutcome(),
    'plan': {
      'panel': panel.name,
      'sourceId': monitoringFixture.id,
      'hostId': bindingHost.id,
      'bindingRevision': 7,
      'start': 0,
      'end': 1000,
      'stepSeconds': 15,
      'maxSeries': 100,
      'maxPointsPerSeries': 1000,
      'maxTotalPoints': 100000,
      'queries': [
        for (final metric in panel.metrics)
          {
            'id': metric.name,
            'unit': metric.unit.name,
            'semantics': metric.semantics,
            'expression': 'Native-owned-expression-not-UI-authority',
          },
      ],
    },
  },
  'queries': [
    for (final metric in panel.metrics)
      {
        'id': metric.name,
        'unit': metric.unit.name,
        'semantics': metric.semantics,
        'outcome': metricOutcome(partial: partial, empty: state == 'noData'),
      },
  ],
  'limited': limited,
};

class MonitoringBindingReadTestService extends MonitoringTestService implements MonitoringBindingReader {
  final discoveries = <(String, int, double)>[];
  final panels = <(String, int, MonitoringPanel, double, double, double)>[];
  int bindingCancellations = 0;
  Completer<BindingReadResult>? pendingBinding;
  BindingReadResult discoveryResult = BindingReadResult.fromJson(bindingDiscoveryJson());
  Json Function(MonitoringPanel) panelResult = (panel) => bindingPanelJson(panel: panel);
  @override
  Future<BindingReadResult> discover(MonitoringSource source, MonitoringBinding binding, double time) async {
    discoveries.add((binding.id, binding.revision, time));
    return pendingBinding?.future ?? discoveryResult;
  }

  @override
  Future<BindingReadResult> readPanel(
    MonitoringSource source,
    MonitoringBinding binding,
    MonitoringPanel panel, {
    required double start,
    required double end,
    required double stepSeconds,
  }) async {
    panels.add((binding.id, binding.revision, panel, start, end, stepSeconds));
    return pendingBinding?.future ?? BindingReadResult.fromJson(panelResult(panel));
  }

  @override
  void cancelBinding() {
    bindingCancellations++;
  }
}
