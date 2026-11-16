import 'dart:convert';

import 'package:consolecrypt/automation/automation_inventory_export.dart';
import 'package:flutter_test/flutter_test.dart';

import '../helpers/inventory_export_fixture.dart';

InventoryExportRequest request({
  List<String> hosts = const [inventoryExportHostId],
  List<String> groups = const ['web'],
}) => InventoryExportRequest(
  projectId: inventoryExportProjectId,
  expectedRevision: 7,
  hostIds: hosts,
  groupNames: groups,
);

Map<String, dynamic> hostVariables(Map<String, dynamic> value) =>
    (((value['inventory'] as Map<String, dynamic>)['_meta'] as Map<String, dynamic>)['hostvars']
                as Map<String, dynamic>)
            .values
            .first
        as Map<String, dynamic>;

void main() {
  test('request is exact bounded explicit selection and cannot carry arbitrary options', () {
    expect(request().toJson(), {
      'version': 1,
      'projectId': inventoryExportProjectId,
      'expectedRevision': 7,
      'hostIds': [inventoryExportHostId],
      'groupNames': ['web'],
    });
    for (final hosts in <List<String>>[
      [],
      [inventoryExportHostId, inventoryExportHostId],
      ['00000000-0000-0000-0000-000000000000'],
      List.generate(33, (i) => '${i.toRadixString(16).padLeft(8, '0')}-2222-4222-8222-222222222222'),
    ]) {
      expect(() => request(hosts: hosts), throwsA(isA<InventoryExportFailure>()));
    }
    for (final groups in <List<String>>[
      ['all'],
      ['web', 'web'],
      ['{{ injected }}'],
      List.generate(65, (i) => 'group_$i'),
    ]) {
      expect(() => request(groups: groups), throwsA(isA<InventoryExportFailure>()));
    }
  });

  test('legacy and selected inventory fences do not expand from group members', () {
    final legacy = InventoryExportContext.fromConfiguration(inventoryExportProjectId, inventoryExportConfiguration());
    expect(legacy.permits(inventoryExportOtherId), isTrue);
    final selected = InventoryExportContext.fromConfiguration(
      inventoryExportProjectId,
      inventoryExportConfiguration(selected: [inventoryExportHostId]),
    );
    expect(selected.groups, ['empty', 'web']);
    expect(selected.permits(inventoryExportOtherId), isFalse);
    expect(selected.allows(request(hosts: [inventoryExportOtherId])), isFalse);
    final mixed = InventoryExportContext.fromConfiguration(
      inventoryExportProjectId,
      inventoryExportConfiguration(selected: [inventoryExportHostId], mode: 'mixed'),
    );
    expect(mixed.permits(inventoryExportOtherId), isTrue);
    final external = InventoryExportContext.fromConfiguration(
      inventoryExportProjectId,
      inventoryExportConfiguration(mode: 'external_only'),
    );
    expect(external.externalOnly, isTrue);
    expect(external.permits(inventoryExportHostId), isFalse);
  });

  test('future inventory and external configuration schemas fail closed', () {
    for (final future in [true, false]) {
      final value = inventoryExportConfiguration(mode: 'mixed');
      (value[future ? 'saved_inventory' : 'configuration'] as Map)['version'] = 999;
      expect(
        () => InventoryExportContext.fromConfiguration(inventoryExportProjectId, value),
        throwsA(isA<InventoryExportFailure>()),
      );
    }
  });

  test('Native snapshot retains exact original UTF8 text and accepts working version zero', () {
    final raw = '\n${const JsonEncoder.withIndent('  ').convert(inventoryExportSnapshot())}\n';
    final issued = IssuedInventorySnapshot.fromNative(raw, request());
    expect(issued.nativeJson, raw);
    expect(issued.bytes, utf8.encode(raw));
    expect(issued.hostCount, 1);
    expect(issued.groupCount, 1);
    issued.bytes.fillRange(0, 2, 0); // Returned mutable view never mutates the saved Native text.
    expect(issued.bytes, utf8.encode(raw));
  });

  test('unknown or secret fields are rejected at every export layer', () {
    for (final layer in ['root', 'origin', 'group', 'hostvars']) {
      final value = inventoryExportSnapshot();
      final Map<String, dynamic> target = switch (layer) {
        'origin' => value['origin'] as Map<String, dynamic>,
        'group' => (value['inventory'] as Map<String, dynamic>)['web'] as Map<String, dynamic>,
        'hostvars' => hostVariables(value),
        _ => value,
      };
      target['credential_id'] = 'must-not-export';
      expect(
        () => IssuedInventorySnapshot.fromNative(jsonEncode(value), request()),
        throwsA(isA<InventoryExportFailure>()),
      );
    }
  });

  test('duplicate fields including escaped names cannot pass response validation', () {
    final raw = jsonEncode(inventoryExportSnapshot());
    expect(
      () => IssuedInventorySnapshot.fromNative(
        raw.replaceFirst('"version":1', '"version":1,"versi\\u006fn":1'),
        request(),
      ),
      throwsA(isA<InventoryExportFailure>()),
    );
    expect(
      () => IssuedInventorySnapshot.fromNative(
        raw.replaceFirst('"ansible_user":"fixture"', '"ansible_user":"hidden-value","ansible_user":"fixture"'),
        request(),
      ),
      throwsA(isA<InventoryExportFailure>()),
    );
  });

  test('selection provenance group expansion and malformed endpoint data are rejected', () {
    for (final mutation in [
      (Map<String, dynamic> v) => (v['selection'] as Map)['hostIds'] = [inventoryExportOtherId],
      (Map<String, dynamic> v) => (v['origin'] as Map)['projectRevision'] = 6,
      (Map<String, dynamic> v) => ((v['inventory'] as Map)['web'] as Map)['hosts'] = ['cc_unselected'],
      (Map<String, dynamic> v) => hostVariables(v)['ansible_port'] = 0,
      (Map<String, dynamic> v) => hostVariables(v)['ansible_user'] = '{{ unsafe }}',
    ]) {
      final value = inventoryExportSnapshot();
      mutation(value);
      expect(
        () => IssuedInventorySnapshot.fromNative(jsonEncode(value), request()),
        throwsA(isA<InventoryExportFailure>()),
      );
    }
    expect(
      () => IssuedInventorySnapshot.fromNative(' ' * (256 * 1024 + 1), request()),
      throwsA(isA<InventoryExportFailure>()),
    );
  });

  test('client issues exactly requested IDs and saves original Native response without regeneration', () async {
    String? sent;
    final raw = ' ${jsonEncode(inventoryExportSnapshot())}\n';
    final client = AutomationInventoryExportClient(
      configuration: (_) async => inventoryExportConfiguration(),
      issue: (value) async {
        sent = value;
        return raw;
      },
    );
    final context = await client.inventoryExportContext(inventoryExportProjectId);
    expect(context.allows(request()), isTrue);
    final result = await client.issueInventoryExport(request());
    expect(jsonDecode(sent!), request().toJson());
    expect(result.nativeJson, raw);
  });

  test('safe Native reason mapping is field-scoped and unknown rules never display raw text', () {
    expect(inventoryExportNativeReason('other', 'inventory_export_size'), isNull);
    expect(
      inventoryExportNativeReason('automation_inventory_export', 'inventory_export_unsupported_route'),
      InventoryExportReason.unsupportedRoute,
    );
    expect(
      inventoryExportNativeReason('automation_inventory_export', 'secret raw unknown body'),
      InventoryExportReason.failed,
    );
  });
}
