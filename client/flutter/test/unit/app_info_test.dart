import 'dart:io';

import 'package:consolecrypt/app/app_info.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('kAppVersion matches pubspec.yaml', () {
    final pubspec = File('pubspec.yaml').readAsLinesSync();
    final line = pubspec.firstWhere((l) => l.startsWith('version:'));
    final version = line.substring('version:'.length).trim();
    expect(kAppFullVersion, version);
    expect(kAppVersion, version.split('+').first);
    expect(kAppBuildNumber, int.parse(version.split('+').last));
  });

  test('author and licences (ADR-0005)', () {
    expect(kAppAuthor, 'Alexander Evsikov');
    expect(kAppAuthorEmail, 'i@evsikov.net');
    expect(kAppLicense, 'MIT OR Apache-2.0');
    expect(kServerLicense, 'AGPL-3.0');
  });
}
