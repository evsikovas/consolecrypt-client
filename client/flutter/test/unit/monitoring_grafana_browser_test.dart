import 'package:consolecrypt/monitoring/monitoring_grafana_browser.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('default external-launch boundary rejects forbidden destinations before any platform launch', () async {
    final container = ProviderContainer();
    addTearDown(container.dispose);
    final launch = container.read(monitoringGrafanaBrowserLauncherProvider);
    for (final address in ['file:///owned', 'javascript:owned', 'https://user:private@grafana.example/']) {
      // No valid destination is passed to the default launcher by this test.
      expect(await launch(Uri.parse(address)), isFalse);
    }
  });
}
