import 'dart:async';

import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_service.dart';

const monitoringFixture = MonitoringSource(
  id: 'c0a26470-e5af-4dcc-b37f-d71a65fd7889',
  name: 'Fixture metrics',
  endpoint: 'http://127.0.0.1:9090',
  revision: 1,
);

class MonitoringTestStore implements MonitoringSourceStore {
  MonitoringTestStore({List<MonitoringSource>? initial}) : sources = initial ?? [monitoringFixture];
  List<MonitoringSource> sources;
  @override
  bool readOnly = false;
  @override
  Future<List<MonitoringSource>> load() async => List.of(sources);
  @override
  Future<MonitoringSource> upsert(MonitoringSource source, {bool create = false}) async {
    source.validate(allowEmptyId: create);
    final saved = source.withRecord(
      id: create ? 'c0a26470-e5af-4dcc-b37f-d71a65fd7890' : source.id,
      revision: source.revision + 1,
    );
    sources = [
      for (final old in sources)
        if (old.id != saved.id) old,
      saved,
    ];
    return saved;
  }

  @override
  Future<void> delete(MonitoringSource source) async => sources.removeWhere((s) => s.id == source.id);
}

class MonitoringTestService implements MonitoringService {
  Completer<QueryOutcome>? pending;
  int queries = 0, cancellations = 0;
  MonitoringQuery? last;
  SourceCapabilities capabilities = const SourceCapabilities(
    EndpointCapability(supported: true, httpStatus: 200),
    EndpointCapability(
      supported: false,
      httpStatus: 404,
      error: MonitoringFailure('invalid_response', 'Fixture range unavailable'),
    ),
  );
  QueryOutcome result = const QueryOutcome(
    status: 'success',
    httpStatus: 200,
    partial: true,
    warnings: ['fixture partial warning'],
    infos: ['fixture annotation'],
    effectiveStepSeconds: 15,
    data: QueryData(
      'matrix',
      series: [
        MonitoringSeries(
          {'job': 'fixture'},
          [
            MonitoringSample(1000, SampleKind.finite, 2),
            MonitoringSample(1015, SampleKind.nan),
            MonitoringSample(1030, SampleKind.positiveInfinity),
            MonitoringSample(1045, SampleKind.negativeInfinity),
          ],
        ),
      ],
    ),
  );
  @override
  Future<SourceCapabilities> probe(MonitoringSource source) async => capabilities;
  @override
  Future<QueryOutcome> query(MonitoringSource source, MonitoringQuery query) async {
    queries++;
    last = query;
    return pending?.future ?? result;
  }

  @override
  Future<LabelValues> labelValues(MonitoringSource source, String label) async =>
      const LabelValues(values: ['up', 'node_load1'], warnings: ['fixture metadata warning'], partial: true);
  @override
  void cancel() => cancellations++;
}

class MonitoringTestBackend implements MonitoringBackend {
  MonitoringTestBackend({MonitoringTestStore? store, MonitoringTestService? service})
    : store = store ?? MonitoringTestStore(),
      service = service ?? MonitoringTestService();
  final MonitoringTestStore store;
  final MonitoringTestService service;
  @override
  MonitoringService createService() => service;
  @override
  MonitoringSourceStore createSourceStore() => store;
}
