import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:consolecrypt/ai/local/local_ai_catalog.dart';

typedef LocalAiMemoryQuery = Future<String?> Function(String executable, List<String> arguments);

class LocalAiHardware {
  const LocalAiHardware({this.totalMemoryBytes, required this.platform});
  final int? totalMemoryBytes;
  final LocalAiPlatform platform;
  bool get usesMetal => platform == LocalAiPlatform.macArm64;

  LocalAiModel? get recommendedModel {
    final memory = totalMemoryBytes;
    if (memory == null) return null;
    return LocalAiCatalog.models.where((model) => memory >= model.recommendedRamGiB * 1024 * 1024 * 1024).lastOrNull;
  }

  /// Read total physical RAM once. No profiles, network, installed model files
  /// or app settings are involved, and a recommendation never selects a model.
  static Future<LocalAiHardware> detect({
    LocalAiPlatform? platform,
    LocalAiMemoryQuery? query,
    Duration timeout = const Duration(seconds: 3),
  }) async {
    final target = platform ?? LocalAiCatalog.platform;
    if (target == LocalAiPlatform.unsupported) return LocalAiHardware(platform: target);
    final windows = target == LocalAiPlatform.windowsX64;
    final executable = windows
        ? '${Platform.environment['SystemRoot'] ?? r'C:\Windows'}\\System32\\WindowsPowerShell\\v1.0\\powershell.exe'
        : '/usr/sbin/sysctl';
    final arguments = windows
        ? [
            '-NoLogo',
            '-NoProfile',
            '-NonInteractive',
            '-Command',
            '[Console]::Write([UInt64](Get-CimInstance -ClassName Win32_ComputerSystem -Property TotalPhysicalMemory).TotalPhysicalMemory)', // l10n-ignore: read-only Windows system query
          ]
        : ['-n', 'hw.memsize'];
    try {
      final output = await (query ?? _query)(executable, arguments).timeout(timeout);
      final memory = output == null ? null : int.tryParse(output.trim());
      return LocalAiHardware(platform: target, totalMemoryBytes: memory != null && memory > 0 ? memory : null);
    } catch (_) {
      return LocalAiHardware(platform: target);
    }
  }

  static Future<String?> _query(String executable, List<String> arguments) async {
    Process? child;
    var expired = false;
    try {
      return await (() async {
        final process = await Process.start(executable, arguments, runInShell: false);
        child = process;
        if (expired) {
          process.kill();
          return null;
        }
        unawaited(process.stderr.drain<void>());
        final output = <int>[];
        await for (final chunk in process.stdout) {
          if (output.length + chunk.length > 256) throw const FormatException('unexpected memory query output');
          output.addAll(chunk);
        }
        return await process.exitCode == 0 ? ascii.decode(output).trim() : null;
      })().timeout(const Duration(seconds: 3));
    } catch (_) {
      expired = true;
      return null;
    } finally {
      child?.kill();
    }
  }
}
