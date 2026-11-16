import 'dart:convert';
import 'dart:io';

import 'package:consolecrypt/automation/automation_portable_package.dart';
import 'package:flutter_test/flutter_test.dart';

// Actual Native owned-fixture serde SHAPE only, no UUID/path/password/source.
// Artifact SHA199cd977d0c69272e619873525ca2a1c245ff257c4506ff277b87ae16f4b3e41.
// Synthetic scalar values below exercise Dart's parser under ACTUAL Native keys.
Map<String, dynamic> witness() =>
    jsonDecode(File('test/fixtures/portable_native_wire_shapes_v1.json').readAsStringSync()) as Map<String, dynamic>;
Object? sample(Object? shape, [String key = '']) {
  if (shape is String) {
    switch (shape) {
      case 'integer':
        return key == 'version' || key == 'projectRevision'
            ? 1
            : key == 'bytes'
            ? 24
            : key == 'files' || key == 'exclusions'
            ? 1
            : 0;
      case 'boolean':
        return key == 'selectable';
      case 'string':
        if (key == 'reason') return 'managed_publication_required';
        if (key == 'path') return 'play.yml';
        if (key == 'profileId') return '11111111-1111-4111-8111-111111111111';
        if (key == 'notificationScopeId') return '22222222-2222-4222-8222-222222222222';
        if (key == 'projectId') return '33333333-3333-4333-8333-333333333333';
        return 'a' * 64;
    }
  }
  if (shape is Map<String, dynamic>) {
    if (shape['type'] == 'array') return [for (final row in shape['itemShapes'] as List) sample(row)];
    return {for (final entry in shape.entries) entry.key: sample(entry.value, entry.key)};
  }
  throw StateError('Unknown actual Native shape descriptor');
}

Map<String, dynamic> view(String name) =>
    sample((witness()['shapes'] as Map<String, dynamic>)[name]) as Map<String, dynamic>;
void main() {
  test('actual Native serde context and nested scope/choice deserialize with no invented context version', () {
    final native = witness();
    expect(native['origin'], 'actualNativeOwnedFixtureSerde');
    expect(native['version'], 1);
    final raw = view('context'), value = PortableContext.fromJson(raw);
    expect(value.projectRevision, 1);
    expect(value.scope.toJson(), raw['scope']);
    expect(value.choices, isNotEmpty);
    expect(raw.containsKey('version'), isFalse); // Native Context version belongs to nested scope.
  });
  test('actual Native export envelope shape uses exact encrypted package byte ceiling', () {
    final maximum = (witness()['byteLimits'] as Map<String, dynamic>)['encryptedPackage'] as int;
    expect(portableMaxEncryptedBytes, maximum);
    final raw = view('exportOutcome')..['bytes'] = maximum;
    expect(PortableExportOutcome.fromJson(raw).bytes, maximum);
    raw['bytes'] = maximum + 1;
    expect(() => PortableExportOutcome.fromJson(raw), throwsA(isA<PortablePackageFailure>()));
  });
  test('actual Native import and nested summary preserve publication block rather than offering commit authority', () {
    final raw = view('importPreview'), supported = witness()['supportedValues'] as Map<String, dynamic>;
    expect(raw['publicationSupported'], supported['publicationSupported']);
    expect(raw['reason'], supported['inspectionReason']);
    final value = PortableImportPreview.fromJson(raw);
    expect(value.files, 1);
    expect(value.bytes, 24);
    expect(value.exclusions, 1);
    raw['publicationSupported'] = true;
    expect(() => PortableImportPreview.fromJson(raw), throwsA(isA<PortablePackageFailure>()));
  });
  test('native integer v1 is not accepted as a floating value or a future schema', () {
    for (final version in [1.0, 2]) {
      final raw = view('exportOutcome')..['version'] = version;
      expect(() => PortableExportOutcome.fromJson(raw), throwsA(isA<PortablePackageFailure>()));
      final imported = view('importPreview')..['version'] = version;
      expect(() => PortableImportPreview.fromJson(imported), throwsA(isA<PortablePackageFailure>()));
    }
  });
  test('additional caller-authority fields are rejected on actual Native-derived context and import envelopes', () {
    final raw = view('context')..['approved'] = true;
    expect(() => PortableContext.fromJson(raw), throwsA(isA<PortablePackageFailure>()));
    final imported = view('importPreview')..['installTarget'] = '/not-a-native-managed-target';
    expect(() => PortableImportPreview.fromJson(imported), throwsA(isA<PortablePackageFailure>()));
  });
  test('selection request obeys actual Native UTF8 packet limit before any picker or Native export', () {
    final maximum = (witness()['byteLimits'] as Map<String, dynamic>)['exportRequestJson'] as int;
    expect(portableMaxExportRequestBytes, maximum);
    final raw = view('context');
    raw['choices'] = [
      for (var i = 0; i < 2000; i++)
        {
          'path': 'f${i.toString().padLeft(4, '0')}${'a' * 140}.yml',
          'bytes': 1,
          'directory': false,
          'selectable': true,
        },
    ];
    final context = PortableContext.fromJson(raw), paths = context.choices.map((e) => e.path).toSet();
    expect(() => context.selection(paths), throwsA(isA<PortablePackageFailure>()));
    expect(context.selection({context.choices.first.path})['selectedPaths'], [context.choices.first.path]);
  });
}
