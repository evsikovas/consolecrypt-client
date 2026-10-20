// Run on a device with:
// flutter drive --profile --target integration_test/kubernetes_performance_test.dart
//   --driver test_driver/kubernetes_performance.dart -d <device>
// Synthetic data only: this benchmark opens no vault, cluster, or native session.
import 'dart:convert';
import 'dart:math' as math;

import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_operations_service.dart';
import 'package:consolecrypt/kubernetes/kube_resource_table_view.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/scheduler.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:material_ui/material_ui.dart';

Map<String, Object> _summary(Iterable<Duration> values) {
  final sorted = values.map((duration) => duration.inMicroseconds / 1000).toList()..sort();
  double percentile(double fraction) => sorted[(fraction * sorted.length).ceil().clamp(1, sorted.length) - 1];
  return {
    'p50_ms': percentile(.5),
    'p90_ms': percentile(.9),
    'p99_ms': percentile(.99),
    'max_ms': sorted.last,
    'over_16_7_ms': sorted.where((value) => value > 16.7).length,
    'over_33_3_ms': sorted.where((value) => value > 33.3).length,
  };
}

void main() {
  final binding = IntegrationTestWidgetsFlutterBinding.ensureInitialized()
    ..framePolicy = LiveTestWidgetsFlutterBindingFramePolicy.fullyLive;

  testWidgets('10,000 Kubernetes resources render and scroll with measured frame timings', (tester) async {
    expect(kProfileMode, isTrue, reason: 'Use flutter drive --profile; debug timings are not performance evidence.');
    const rowCount = 10000;
    final rows = List.generate(
      rowCount,
      (index) => KubeResourceRow(
        name: 'application-${index.toString().padLeft(5, '0')}-replica',
        uid: 'benchmark-$index',
        resourceVersion: '1',
        namespace: 'synthetic-workloads',
        status: index % 20 == 0 ? 'Pending' : 'Running',
        cells: ['application-$index', '${index % 4}'],
      ),
      growable: false,
    );
    final mount = Stopwatch()..start();
    await tester.pumpWidget(
      ProviderScope(
        child: MaterialApp(
          theme: AppTheme.light(),
          locale: const Locale('en'),
          supportedLocales: AppLocalizations.supportedLocales,
          localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
          home: Scaffold(
            body: KubeResourceTableView(
              rows: rows,
              kind: const KubeResourceKind(
                group: '',
                version: 'v1',
                resource: 'pods',
                kind: 'Pod',
                namespaced: true,
                verbs: ['get'],
              ),
              columns: const [
                KubeTableColumn(name: 'Name', type: 'string', format: 'name', description: '', priority: 0),
                KubeTableColumn(name: 'Restarts', type: 'integer', format: '', description: '', priority: 0),
              ],
              selected: const {},
              canSelect: true,
              onSelection: (_) {},
              onOpen: (_) {},
              hasMore: false,
              loadingMore: false,
              onMore: () {},
            ),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    mount.stop();
    final list = find.byKey(const ValueKey('kube-resource-list'));
    final scroll = tester.state<ScrollableState>(find.descendant(of: list, matching: find.byType(Scrollable)).first);
    final position = scroll.position;
    final distance = math.min(6000.0, position.maxScrollExtent);
    expect(distance, greaterThan(3000));
    final materialized = find.byWidgetPredicate(
      (widget) =>
          widget.key is ValueKey<String> &&
          (widget.key! as ValueKey<String>).value.startsWith('kube-resource-benchmark-'),
    );
    final builtBefore = materialized.evaluate().length;
    expect(builtBefore, lessThan(80));

    // Warm up text/raster caches, then let batched old timings flush. Real
    // animation time is used, rather than pumping a fake 60 Hz test clock.
    await position.animateTo(800, duration: const Duration(seconds: 1), curve: Curves.linear);
    await position.animateTo(0, duration: const Duration(seconds: 1), curve: Curves.linear);
    await Future<void>.delayed(const Duration(seconds: 2));
    final frames = <FrameTiming>[];
    void collect(List<FrameTiming> values) => frames.addAll(values);
    binding.addTimingsCallback(collect);
    final elapsed = Stopwatch()..start();
    try {
      for (var pass = 0; pass < 4; pass++) {
        await position.animateTo(
          pass.isEven ? distance : 0,
          duration: const Duration(seconds: 4),
          curve: Curves.linear,
        );
      }
      elapsed.stop();
      await Future<void>.delayed(const Duration(seconds: 2));
    } finally {
      binding.removeTimingsCallback(collect);
    }
    expect(
      frames.length,
      greaterThan(120),
      reason: 'Enough real frames must be rendered to make this measurement useful.',
    );
    expect(materialized.evaluate().length, lessThan(80));
    expect(tester.takeException(), isNull);
    final view = tester.view;
    final report = <String, Object>{
      'mode': 'profile',
      'platform': defaultTargetPlatform.name,
      'rows': rowCount,
      'frames': frames.length,
      'mounted_rows_before': builtBefore,
      'mounted_rows_after': materialized.evaluate().length,
      'mount_wall_ms': mount.elapsedMicroseconds / 1000,
      'scroll_wall_ms': elapsed.elapsedMicroseconds / 1000,
      'scroll_distance_logical_px': distance * 4,
      'viewport_physical_px': {'width': view.physicalSize.width, 'height': view.physicalSize.height},
      'device_pixel_ratio': view.devicePixelRatio,
      'build': _summary(frames.map((frame) => frame.buildDuration)),
      'raster': _summary(frames.map((frame) => frame.rasterDuration)),
      'total_span': _summary(frames.map((frame) => frame.totalSpan)),
    };
    binding.reportData = {'kubernetes_10000_rows': report};
    debugPrint('KUBE_PERFORMANCE: ${jsonEncode(report)}');
    await tester.pumpWidget(const SizedBox.shrink());
  });
}
