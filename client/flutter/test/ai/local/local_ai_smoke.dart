// Explicit opt-in smoke; deliberately not named *_test.dart.
// dart run test/ai/local/local_ai_smoke.dart --run --directory=<temporary-cache>
// Add --runtime-only to verify the native executable without downloading a model.
// ignore_for_file: avoid_print
import 'dart:convert';
import 'dart:io';

import 'package:consolecrypt/ai/local/local_ai_archive.dart';
import 'package:consolecrypt/ai/local/local_ai_download.dart';
import 'package:consolecrypt/ai/local/local_ai_runtime.dart';

Future<void> main(List<String> args) async {
  final directoryArg = args.where((a) => a.startsWith('--directory=')).firstOrNull;
  if (!args.contains('--run') || directoryArg == null) {
    stderr.writeln('Explicit opt-in required: --run --directory=<temporary-cache> [--runtime-only]');
    exitCode = 64;
    return;
  }
  final directory = Directory(directoryArg.substring('--directory='.length)).absolute;
  if (LocalAiCatalog.platform == LocalAiPlatform.unsupported) throw StateError('Unsupported smoke platform');
  if (args.contains('--runtime-only')) {
    final artifact = LocalAiCatalog.runtimes[LocalAiCatalog.platform]!;
    final archive = await const LocalAiDownloader().download(
      artifact,
      Directory('${directory.path}/downloads'),
      cancellation: LocalAiCancellation(),
    );
    final destination = Directory('${directory.path}/runtime-check-${LocalAiCatalog.platform.name}');
    var executable = await installedLocalAiExecutable(destination, artifact, verifyFiles: true);
    if (executable == null) {
      if (destination.existsSync()) await destination.delete(recursive: true);
      executable = await extractLocalAiRuntime(archive, destination, artifact, Platform.isWindows);
    }
    final result = await Process.run('${destination.path}/$executable', ['--version'], runInShell: false);
    print(result.stdout);
    print(result.stderr);
    if (result.exitCode != 0) throw StateError('Native runtime --version exited ${result.exitCode}');
    print('RUNTIME_VERSION_PASS');
    return;
  }
  final runtime = LocalAiRuntime(
    supportDirectory: directory,
    allowMacRuntimeDownload: true, // Explicit CLI opt-in; app uses the signed bundled helper.
    idleTimeout: const Duration(seconds: 2),
  );
  var lastProgress = -1;
  final subscription = runtime.statuses.listen((status) {
    final progress = ((status.progress ?? 0) * 100).floor();
    if (progress ~/ 5 != lastProgress || status.phase != LocalAiPhase.installing) {
      lastProgress = progress ~/ 5;
      print('${status.phase.name}: $progress% ${status.message ?? ''}');
    }
  });
  final http = HttpClient()..findProxy = (_) => 'DIRECT';
  try {
    await runtime.initialize();
    await runtime.install();
    final endpoints = await Future.wait([runtime.ensureReady(), runtime.ensureReady()]);
    if (!identical(endpoints[0], endpoints[1])) throw StateError('Start was not single-flight');
    var request = await http.getUrl(endpoints[0].baseUrl.resolve('/v1/models'));
    var response = await request.close();
    final unauthenticatedStatus = response.statusCode;
    await response.drain<void>();
    if (unauthenticatedStatus != 401) throw StateError('Missing bearer token was not rejected');
    await runtime.withEndpoint((endpoint) async {
      await Future<void>.delayed(const Duration(seconds: 3));
      if (runtime.status.phase != LocalAiPhase.ready) throw StateError('Idle timer killed an active lease');
      request = await http.postUrl(endpoint.baseUrl.resolve('/v1/chat/completions'));
      request.headers.set(HttpHeaders.authorizationHeader, 'Bearer ${endpoint.apiKey}');
      request.headers.contentType = ContentType.json;
      request.write(
        jsonEncode({
          'model': endpoint.modelId,
          'messages': [
            {'role': 'user', 'content': 'Reply with exactly OK.'},
          ],
          'temperature': 0,
          'max_tokens': 16,
        }),
      );
      response = await request.close().timeout(const Duration(minutes: 3));
      final body = await utf8.decoder.bind(response).join().timeout(const Duration(minutes: 3));
      if (response.statusCode != 200) throw StateError('Generation failed with HTTP ${response.statusCode}');
      final payload = jsonDecode(body) as Map<String, dynamic>;
      final choices = payload['choices'] as List<dynamic>;
      final choice = choices.first as Map<String, dynamic>;
      final content = (choice['message'] as Map<String, dynamic>)['content'] as String;
      if (content.trim().isEmpty) throw StateError('Empty model response');
      print('CHAT_PASS: ${content.trim()}');
    });
    final deadline = DateTime.now().add(const Duration(seconds: 10));
    while (runtime.status.phase != LocalAiPhase.stopped && DateTime.now().isBefore(deadline)) {
      await Future<void>.delayed(const Duration(milliseconds: 100));
    }
    if (runtime.status.phase != LocalAiPhase.stopped) throw StateError('Idle unload failed');
    print('AUTH_SINGLE_FLIGHT_LEASE_IDLE_PASS');
  } finally {
    http.close(force: true);
    await runtime.dispose();
    await subscription.cancel();
  }
}
