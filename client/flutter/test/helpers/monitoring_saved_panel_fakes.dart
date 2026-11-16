import 'dart:async';

import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_saved_panel_models.dart';
import 'package:consolecrypt/monitoring/monitoring_service.dart';

import 'monitoring_fakes.dart';

const savedPanelId = '33333333-3333-4333-8333-333333333333';
const savedDefinition = SavedPanelDefinition(
  title: 'Owned saved panel',
  sourceId: 'c0a26470-e5af-4dcc-b37f-d71a65fd7889',
  sourceRevision: 1,
  query: 'rate(owned_fixture_total{job="explicit"}[5m])',
  unitHint: SavedPanelUnitHint.seconds,
);
const savedPanel = SavedMonitoringPanel(
  id: savedPanelId,
  revision: 1,
  definition: savedDefinition,
  state: SavedPanelState.current,
);
Json savedPanelRow({int revision = 1, String state = 'current'}) => {
  'id': savedPanelId,
  'revision': revision,
  'definition': savedDefinition.toJson(),
  'state': state,
};
Json savedPanelCatalog({int version = 3, int revision = 2, List<Json>? panels}) => {
  'schemaVersion': version,
  'revision': revision,
  'readOnly': false,
  'sources': [
    {
      'id': monitoringFixture.id,
      'revision': monitoringFixture.revision,
      'readOnly': false,
      'configuration': monitoringFixture.toJson(),
    },
  ],
  'bindings': <Json>[],
  if (version == 3) 'panels': panels ?? [savedPanelRow()],
};
Json savedQueryOutcome() => {
  'status': 'success',
  'httpStatus': 200,
  'partial': true,
  'warnings': ['owned partial warning'],
  'infos': <String>[],
  'error': null,
  'effectiveStepSeconds': 15,
  'data': {
    'kind': 'matrix',
    'series': [
      {
        'labels': {'job': 'owned'},
        'samples': [
          {
            'timestamp': 1000,
            'value': {'kind': 'nan'},
          },
        ],
      },
    ],
  },
};
Json savedReadEnvelope() => {
  'kind': 'savedPanel',
  'panelId': savedPanelId,
  'panelRevision': 1,
  'unitHint': 'seconds',
  'outcome': savedQueryOutcome(),
};

class SavedPanelTestStore extends MonitoringTestStore implements MonitoringSavedPanelStore {
  @override
  List<SavedMonitoringPanel> panels = [savedPanel];
  final saves = <SavedPanelDefinition>[];
  int deletions = 0;
  @override
  Future<SavedMonitoringPanel> savePanel(SavedPanelDefinition definition, {SavedMonitoringPanel? previous}) async {
    definition.validate();
    saves.add(definition);
    final panel = SavedMonitoringPanel(
      id: previous?.id ?? savedPanelId,
      revision: (previous?.revision ?? 0) + 1,
      definition: definition,
      state: SavedPanelState.current,
    );
    panels = [panel];
    return panel;
  }

  @override
  Future<void> deletePanel(SavedMonitoringPanel panel) async {
    deletions++;
    panels = [];
  }
}

class SavedPanelTestService extends MonitoringTestService implements MonitoringSavedPanelReader {
  final reads = <(MonitoringSource, SavedMonitoringPanel, SavedPanelRange?)>[];
  Completer<SavedPanelRead>? pendingSaved;
  @override
  Future<SavedPanelRead> readSavedPanel(
    MonitoringSource source,
    SavedMonitoringPanel panel, {
    SavedPanelRange? window,
  }) async {
    reads.add((source, panel, window));
    return pendingSaved?.future ?? SavedPanelRead(result, panel.definition.unitHint);
  }
}
