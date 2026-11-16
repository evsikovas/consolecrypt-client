import 'dart:convert';

import 'package:consolecrypt/monitoring/agent_collector_service_credentials.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_port.dart';
import 'package:consolecrypt/monitoring/agent_metrics_collector_models.dart';
import 'package:consolecrypt/monitoring/agent_metrics_collector_wire.dart';
import 'package:consolecrypt/monitoring/agent_metrics_manifest.dart';
import 'package:flutter_test/flutter_test.dart';

import '../support/agent_metrics_disk_fixture.dart' as disk;
import '../support/agent_metrics_fixture.dart' as f;
import '../support/agent_metrics_runtime_v2_fixture.dart' as v2;

AgentMetricsManifest parse(Map<String, dynamic> m) => parseAgentMetricsManifest(m, f.metricRecord().hostId);
void main() {
  test('distinct disk-v2 preserves its full Native catalog ID/hash and fixed disk budgets without ELF or running inference', () {
    for (final arm in [false, true]) {
      final raw = v2.diskCollectorView(arm: arm), manifest = parse(raw['manifest'] as Map<String, dynamic>);
      expect(manifest.runtime!.kind, 'prometheusHardmode');
      expect(manifest.runtime!.upstreamTag, '3.16.0');
      expect(manifest.runtime!.platformName, arm ? 'linux-aarch64' : 'linux-x86_64');
      // Both metadata labels are valid shapes; neither supplies an actual ARM ELF.
      expect(manifest.persistentDisk, isTrue);
      expect(manifest.schemaVersion, 2);
      expect(manifest.maxBufferBytes, 536870912);
      expect(manifest.maxBufferAgeSeconds, 7200);
      final catalog = parseAgentMetricsCollectorCatalog({
        'collectorId': raw['collectorId'],
        'manifestSha256': raw['manifestSha256'],
        'manifest': raw['manifest'],
      }, f.metricRecord().hostId);
      expect(catalog.collectorId, raw['collectorId']);
      expect(catalog.manifestSha256, raw['manifestSha256']);
      expect(
        AgentCollectorServiceIntent(f.metricRecord(), catalog).credentialUse,
        AgentCollectorServiceCredentialUse.singleStart,
      );
      final review = parseAgentMetricsCollectorView(
        jsonEncode(raw),
        f.scope,
        AgentMetricsCollectorIntent(f.metricRecord(), 300),
      );
      expect(review.actorAdmission, isFalse);
      expect(review.phase, AgentMetricsCollectorPhase.collectorReview);
      expect(review.leaseHash, isNull);
    }
  });
  test('disk-v2 enforces exact runtime fields kind/tag/platform integer bounds and cannot widen RAM-v2', () {
    Map<String, dynamic> raw() => v2.diskCollectorView()['manifest'] as Map<String, dynamic>;
    for (final key in v2.diskRuntime().keys) {
      final m = raw();
      (m['runtime'] as Map).remove(key);
      expect(() => parse(m), throwsFormatException);
    }
    for (final delta in <Map<String, dynamic>>[
      {'kind': 'vmagent'},
      {'kind': 'prometheus'},
      {'upstreamTag': 'v3.16.0'},
      {'platform': 'linux-arm64'},
      {'platform': 'windows-x86_64'},
      {'binarySha256': 'A' * 64},
      {'binarySha256': 'a' * 63},
      {'binaryBytes': 63},
      {'binaryBytes': 201326593},
      {'binaryBytes': 64.0},
      {'binaryBytes': true},
      {'ready': true},
    ]) {
      final m = raw();
      (m['runtime'] as Map).addAll(delta);
      expect(() => parse(m), throwsFormatException, reason: delta.toString());
    }
    for (final bytes in [64, 144399412, 201326592]) {
      final m = raw();
      (m['runtime'] as Map)['binaryBytes'] = bytes;
      expect(parse(m).runtime!.binaryBytes, bytes);
    }
    for (final key in ['profile', 'bufferMode']) {
      expect(() => parse(raw()..remove(key)), throwsFormatException);
    }
    final shortSeries = raw();
    ((shortSeries['targets'] as List).first as Map)['series_limit'] = 255;
    expect(() => parse(shortSeries), throwsFormatException);
    expect(() => parse({...v2.manifest(), 'runtime': v2.diskRuntime()}), throwsFormatException);
    final wrongDisk = raw();
    wrongDisk['runtime'] = v2.runtime();
    expect(() => parse(wrongDisk), throwsFormatException);
  });

  test('v2 displays exact closed Native runtime tuples while retaining its Native-provided full digest and ID', () {
    for (final arm in [true, false]) {
      final manifest = parse(v2.manifest(arm: arm));
      expect(manifest.schemaVersion, 2);
      expect(manifest.runtime!.platformName, arm ? 'linux-aarch64' : 'linux-x86_64');
      expect(manifest.runtime!.binaryBytes, arm ? 21299768 : 23209584);
      expect(manifest.persistentDisk, isFalse);
      expect(manifest.maxBufferBytes, 33554432);
      expect(manifest.maxBufferAgeSeconds, 300);
      final catalog = parseAgentMetricsCollectorCatalog({
        'collectorId': '70000000-0000-8000-8000-000000000007',
        'manifestSha256': 'a' * 64,
        'manifest': v2.manifest(arm: arm),
      }, f.metricRecord().hostId);
      // Shape forwarding only. Flutter does not recompute or attest either value.
      expect(catalog.collectorId, '70000000-0000-8000-8000-000000000007');
      expect(catalog.manifestSha256, 'a' * 64);
    }
  });

  test('v2 rejects incomplete runtime, unknown kinds/tags/platforms and borrowed binary identities', () {
    for (final key in v2.runtime().keys) {
      final m = v2.manifest();
      (m['runtime'] as Map<String, dynamic>).remove(key);
      expect(() => parse(m), throwsFormatException, reason: key);
    }
    for (final mutation in <Map<String, dynamic>>[
      {'kind': 'prometheus'},
      {'upstreamTag': 'v1.157.0'},
      {'platform': 'linux-arm64'},
      {'platform': 'windows-aarch64'},
      {'platform': 'linux-x86_64'}, // ARM hash/bytes cannot borrow x86 name.
      {'binarySha256': '0' * 64},
      {'binaryBytes': 23209584},
      {'binaryBytes': 21299768.0},
      {'binaryBytes': true},
      {'command': 'start'},
    ]) {
      final m = v2.manifest();
      (m['runtime'] as Map<String, dynamic>).addAll(mutation);
      expect(() => parse(m), throwsFormatException, reason: mutation.toString());
    }
    for (final schema in [null, 2.0, true, 3]) {
      expect(() => parse(v2.manifest()..['schema_version'] = schema), throwsFormatException);
    }
  });

  test('vmagent v2 remains volatile-only and cannot become disk, finite service or lifetime credentials', () {
    for (final mutation in <Map<String, dynamic>>[
      {'profile': agentMetricsDiskProfile},
      {'bufferMode': 'persistentNativeWal'},
      {
        'buffer': {'max_bytes': 536870912, 'max_age_seconds': 7200},
      },
      {'grant': true},
    ]) {
      expect(() => parse({...v2.manifest(), ...mutation}), throwsFormatException);
    }
    final catalog = AgentMetricsCollectorCatalog(
      '70000000-0000-8000-8000-000000000007',
      'a' * 64,
      parse(v2.manifest()),
    );
    expect(() => AgentCollectorServiceIntent(f.metricRecord(), catalog), throwsFormatException);
    expect(
      () => AgentCollectorServiceIntent(
        f.metricRecord(),
        catalog,
        credentialUse: AgentCollectorServiceCredentialUse.serviceLifetime,
      ),
      throwsFormatException,
    );
    expect(
      () => AgentMetricsManifest(
        hostId: catalog.manifest.hostId,
        site: catalog.manifest.site,
        environment: catalog.manifest.environment,
        targets: catalog.manifest.targets,
        scrapeIntervalSeconds: 1,
        receiverUrl: catalog.manifest.receiverUrl,
        storageProfile: AgentMetricsStorageProfile.persistentNativeWal,
        runtime: catalog.manifest.runtime,
      ),
      throwsFormatException,
    );
  });

  test('legacy v1 RAM and disk keep exact existing domains and reject injected runtime', () {
    final ram = f.manifest();
    ((ram['targets'] as List).single as Map)['series_limit'] = 1;
    expect(parse(ram).targets.single.seriesLimit, 1);
    expect(parse(ram).runtime, isNull);
    expect(parse(ram).schemaVersion, 1);
    final d = disk.diskView()['manifest'] as Map<String, dynamic>;
    expect(parse(d).persistentDisk, isTrue);
    expect(parse(d).runtime, isNull);
    for (final old in [f.manifest(), disk.diskView()['manifest'] as Map<String, dynamic>]) {
      expect(() => parse({...old, 'runtime': v2.runtime()}), throwsFormatException);
    }
  });

  test('accepted ARM catalogue is review metadata and cannot fabricate running without original Actor evidence', () {
    AgentMetricsCollectorView view(Map<String, dynamic> value) =>
        parseAgentMetricsCollectorView(jsonEncode(value), f.scope, AgentMetricsCollectorIntent(f.metricRecord(), 300));
    final review = view(v2.collectorView());
    expect(review.phase, AgentMetricsCollectorPhase.collectorReview);
    expect(review.actorAdmission, isFalse);
    expect(review.leaseHash, isNull);
    expect(review.deliveryGuaranteed, isFalse);
    expect(review.automaticRenewal, isFalse);
    expect(() => view(v2.collectorView()..['phase'] = 'running'), throwsFormatException);
    expect(() => view(v2.collectorView()..['profile'] = agentMetricsDiskProfile), throwsFormatException);
    final foreign = v2.collectorView();
    ((foreign['manifest'] as Map)['labels'] as Map)['cc_host_id'] = '90000000-0000-4000-8000-000000000009';
    expect(() => view(foreign), throwsFormatException);
  });
}
