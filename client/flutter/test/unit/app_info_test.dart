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
    expect(kAppLicense, 'AGPL-3.0-only');
    expect(kServerLicense, 'AGPL-3.0-only');
  });

  test('all platform bundles include the complete project licence', () {
    final license = File('../../LICENSE').readAsStringSync();
    expect(license, contains('GNU AFFERO GENERAL PUBLIC LICENSE'));
    expect(license, File('../../server/LICENSE').readAsStringSync());
    expect(File('assets/licenses/AGPL-3.0-only.txt').readAsStringSync(), license);
    expect(File('pubspec.yaml').readAsStringSync(), contains('assets/licenses/'));
  });
}
