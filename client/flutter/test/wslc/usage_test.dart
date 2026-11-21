import 'package:consolecrypt/wslc/shared/usage.dart';
import 'package:consolecrypt/wslc/src/rust/models.dart';
import 'package:flutter_test/flutter_test.dart';

ContainerStats sample(double cpu, String mem, String net) =>
    ContainerStats(id: 'a' * 64, cpuPercent: cpu, memoryUsage: mem, netIo: net);

void main() {
  test('docker size strings become bytes', () {
    expect(parseSize('0B'), 0);
    expect(parseSize('29.09MiB'), closeTo(29.09 * 1024 * 1024, 1));
    expect(parseSize('1.2MB'), 1.2e6);
    expect(parseSize('340kB'), 340000);
    expect(parseSize('11.49GiB'), closeTo(11.49 * 1073741824, 1));
    expect(parseSize('n/a'), isNull);
    expect(parsePair('1.2MB / 340kB'), (1.2e6, 340000.0));
    expect(parsePair('broken'), isNull);
    expect(formatBytes(1536), '1.5 КБ');
    expect(formatBytes(2 * 1024 * 1024, perSecond: true), '2.0 МБ/с');
  });

  test('history keeps a bounded window and turns counters into rates', () {
    final h = UsageHistory(3);
    final t = DateTime(2026);
    h.add(sample(1, '10MiB / 1GiB', '1MB / 0B'), t);
    expect(h.rx, isEmpty, reason: 'a rate needs two samples');
    h.add(sample(2, '20MiB / 1GiB', '3MB / 1MB'), t.add(const Duration(seconds: 2)));
    expect(h.rx.single, 1e6);
    expect(h.tx.single, 5e5);
    // A restart resets counters; that is never negative traffic.
    h.add(sample(3, '5MiB / 1GiB', '0B / 0B'), t.add(const Duration(seconds: 4)));
    expect(h.rx.last, 0);
    h.add(sample(4, '6MiB / 1GiB', '1kB / 0B'), t.add(const Duration(seconds: 6)));
    expect(h.cpu, [2, 3, 4]);
    expect(h.memory.first, 20 * 1024 * 1024);
  });
}
