import 'dart:convert';

Map<String, Object?> grafanaPanel({String title = 'Imported panel', String query = 'up', String type = 'timeseries'}) =>
    {
      'title': title,
      'type': type,
      'datasource': {'type': 'prometheus', 'uid': 'never-map-this-to-a-source'},
      'targets': [
        {'expr': query, 'refId': 'A'},
      ],
      'fieldConfig': {
        'defaults': {'unit': 'percentunit'},
      },
    };

String grafanaDashboard({bool unsupported = false, String query = 'up'}) => jsonEncode({
  'schemaVersion': 42,
  'title': 'Owned import fixture',
  'panels': [grafanaPanel(query: query), if (unsupported) grafanaPanel(title: 'Unsupported fixture', type: 'heatmap')],
});
