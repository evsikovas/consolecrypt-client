import 'dart:async';

import 'package:consolecrypt/ai/local/local_ai_catalog.dart';
import 'package:consolecrypt/ai/local/local_ai_hardware.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('RAM recommendations use explicit 4/8/16 GiB boundaries without choosing a model', () {
    for (final (gib, expected) in [
      (3, null),
      (4, LocalAiCatalog.model),
      (7, LocalAiCatalog.model),
      (8, LocalAiCatalog.qwen4B),
      (15, LocalAiCatalog.qwen4B),
      (16, LocalAiCatalog.qwen9B),
      (64, LocalAiCatalog.qwen9B),
    ]) {
      final hardware = LocalAiHardware(totalMemoryBytes: gib * 1024 * 1024 * 1024, platform: LocalAiPlatform.macArm64);
      expect(hardware.recommendedModel, expected);
    }
    expect(const LocalAiHardware(platform: LocalAiPlatform.windowsX64).recommendedModel, isNull);
  });

  test('bounded hardware query parses memory and reports CPU versus Metal', () async {
    for (final platform in [LocalAiPlatform.macArm64, LocalAiPlatform.macX64, LocalAiPlatform.windowsX64]) {
      final hardware = await LocalAiHardware.detect(
        platform: platform,
        query: (executable, arguments) async => '17179869184\n',
      );
      expect(hardware.totalMemoryBytes, 17179869184);
      expect(hardware.recommendedModel, LocalAiCatalog.qwen9B);
      expect(hardware.usesMetal, platform == LocalAiPlatform.macArm64);
    }
  });

  test('missing malformed failed and stalled queries leave recommendation unknown', () async {
    for (final output in [null, 'no memory', '-1', '0', 'RAM: 16']) {
      expect(
        (await LocalAiHardware.detect(
          platform: LocalAiPlatform.macArm64,
          query: (_, _) async => output,
        )).recommendedModel,
        isNull,
      );
    }
    expect(
      (await LocalAiHardware.detect(
        platform: LocalAiPlatform.windowsX64,
        query: (_, _) async => throw StateError('unavailable'),
      )).recommendedModel,
      isNull,
    );
    expect(
      (await LocalAiHardware.detect(
        platform: LocalAiPlatform.macArm64,
        query: (_, _) => Completer<String?>().future,
        timeout: const Duration(milliseconds: 5),
      )).recommendedModel,
      isNull,
    );
    var queried = false;
    await LocalAiHardware.detect(
      platform: LocalAiPlatform.unsupported,
      query: (_, _) async {
        queried = true;
        return '1';
      },
    );
    expect(queried, isFalse);
  });

  test('Linux reads MemTotal in KiB and uses CPU recommendations without Metal', () async {
    final hardware = await LocalAiHardware.detect(
      platform: LocalAiPlatform.linuxX64,
      query: (executable, arguments) async {
        expect(executable, '/bin/cat');
        expect(arguments, ['/proc/meminfo']);
        return 'MemTotal:       16777216 kB\nMemFree: 123 kB\n';
      },
    );
    expect(hardware.totalMemoryBytes, 16 * 1024 * 1024 * 1024);
    expect(hardware.recommendedModel, LocalAiCatalog.qwen9B);
    expect(hardware.usesMetal, isFalse);
    expect(
      (await LocalAiHardware.detect(
        platform: LocalAiPlatform.linuxX64,
        query: (_, _) async => 'MemFree: 17 kB\n',
      )).recommendedModel,
      isNull,
    );
    final artifact = LocalAiCatalog.runtimes[LocalAiPlatform.linuxX64]!;
    expect(artifact.bytes, 17995598);
    expect(artifact.sha256, '0b3b7579835cc3d837c6046bbc970fd4be0281d1a9ee2ea5b7cdb6eeb84b5842');
  });
}
