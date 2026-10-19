import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:math';

import 'package:consolecrypt/ai/local/local_ai_archive.dart';
import 'package:consolecrypt/ai/local/local_ai_catalog.dart';
import 'package:consolecrypt/ai/local/local_ai_download.dart';

export 'local_ai_catalog.dart';
export 'local_ai_download.dart' show LocalAiException, LocalAiDownloadCancelled;

enum LocalAiPhase { unsupported, notInstalled, installing, stopped, starting, ready, stopping, error }

class LocalAiRuntimeStatus {
  const LocalAiRuntimeStatus({
    required this.phase,
    this.isInstalled = false,
    this.modelId = 'qwen2.5-coder-1.5b-instruct-q4_k_m',
    this.downloadedBytes = 0,
    this.totalBytes = 0,
    this.message,
  });
  final LocalAiPhase phase;
  final bool isInstalled;
  final String modelId;
  final int downloadedBytes, totalBytes;
  final String? message;
  double? get progress => totalBytes > 0 ? (downloadedBytes / totalBytes).clamp(0, 1) : null;
}

class LocalAiEndpoint {
  const LocalAiEndpoint({required this.baseUrl, required this.apiKey, required this.modelId});
  final Uri baseUrl;
  final String apiKey, modelId;
  @override
  String toString() => 'LocalAiEndpoint($baseUrl, $modelId, <redacted>)';
}

/// Owns one llama.cpp process, never a pre-existing local service.
/// Construction/initialize are offline; only install() downloads artifacts.
/// macOS uses a signed, bundled helper; Windows downloads a pinned runtime.
class LocalAiRuntime {
  LocalAiRuntime({
    Directory? supportDirectory,
    Directory? bundledRuntimeDirectory,
    this.allowMacRuntimeDownload = false,
    this.idleTimeout = const Duration(minutes: 15),
    this.startTimeout = const Duration(minutes: 2),
    LocalAiPlatform? platform,
    LocalAiDownloader? downloader,
  }) : supportDirectory = supportDirectory ?? _defaultDirectory(),
       platform = platform ?? LocalAiCatalog.platform,
       bundledRuntimeDirectory =
           bundledRuntimeDirectory ?? _defaultBundledRuntimeDirectory(platform ?? LocalAiCatalog.platform),
       _downloader = downloader ?? const LocalAiDownloader() {
    if (this.platform == LocalAiPlatform.unsupported) {
      _status = const LocalAiRuntimeStatus(phase: LocalAiPhase.unsupported);
    }
  }

  final Directory supportDirectory;
  final Directory? bundledRuntimeDirectory;

  /// Only the explicit command-line smoke enables this outside App Sandbox.
  /// App UI always uses the signed macOS helper bundled at build time.
  final bool allowMacRuntimeDownload;
  final Duration idleTimeout, startTimeout;
  final LocalAiPlatform platform;
  final LocalAiDownloader _downloader;
  final _changes = StreamController<LocalAiRuntimeStatus>.broadcast();
  LocalAiRuntimeStatus _status = const LocalAiRuntimeStatus(phase: LocalAiPhase.notInstalled);
  LocalAiRuntimeStatus get status => _status;
  Stream<LocalAiRuntimeStatus> get statuses => _changes.stream;
  LocalAiModel get selectedModel => LocalAiCatalog.model;
  Future<void>? _initializing, _installing, _stopping;
  Future<LocalAiEndpoint>? _starting;
  LocalAiCancellation? _downloadCancellation;
  LocalAiCancellation? _startCancellation;
  LocalAiEndpoint? _endpoint;
  Process? _process;
  Timer? _idle;
  int _leases = 0, _generation = 0;
  bool _disposed = false, _modelVerified = false, _initialized = false;

  Directory get _downloads => Directory('${supportDirectory.path}/downloads');
  Directory get _downloadedRuntime =>
      Directory('${supportDirectory.path}/runtime-${LocalAiCatalog.runtimeVersion}-${platform.name}');
  Directory get _runtime => bundledRuntimeDirectory ?? _downloadedRuntime;
  bool get _isMac => platform == LocalAiPlatform.macArm64 || platform == LocalAiPlatform.macX64;
  bool get _downloadsRuntime => bundledRuntimeDirectory == null && (!_isMac || allowMacRuntimeDownload);
  File get _modelFile => File('${supportDirectory.path}/models/${selectedModel.filename}');

  static Directory? _defaultBundledRuntimeDirectory(LocalAiPlatform platform) {
    if (platform != LocalAiPlatform.macArm64 && platform != LocalAiPlatform.macX64) return null;
    final executableDirectory = File(Platform.resolvedExecutable).parent;
    final contents = executableDirectory.parent;
    if (!executableDirectory.path.endsWith('/Contents/MacOS') || !contents.parent.path.endsWith('.app')) return null;
    return Directory('${contents.path}/Helpers/LocalAI/${platform.name}');
  }

  Future<String?> _runtimeExecutable({bool verifyFiles = false, LocalAiCancellation? cancellation}) async {
    final artifact = LocalAiCatalog.runtimes[platform]!;
    final bundled = bundledRuntimeDirectory;
    if (bundled != null) {
      // Signing changes Mach-O bytes. Trust the enclosing app's code signature,
      // as for its other native libraries, and check pinned build metadata.
      try {
        cancellation?.check();
        // Non-code resources must live outside Contents/Helpers so codesign
        // treats every entry under Helpers as properly signed nested code.
        final metadataDirectory = bundled.path.endsWith('/Contents/Helpers/LocalAI/${platform.name}')
            ? Directory('${bundled.parent.parent.parent.path}/Resources/LocalAI/${platform.name}')
            : bundled; // Explicit test fixture directory.
        final metadata =
            jsonDecode(await File('${metadataDirectory.path}/runtime.json').readAsString()) as Map<String, dynamic>;
        if (metadata['version'] != LocalAiCatalog.runtimeVersion ||
            metadata['platform'] != platform.name ||
            metadata['archiveSha256'] != artifact.sha256 ||
            FileSystemEntity.typeSync('${bundled.path}/llama-server', followLinks: false) !=
                FileSystemEntityType.file) {
          return null;
        }
        return 'llama-server';
      } on LocalAiDownloadCancelled {
        rethrow;
      } catch (_) {
        return null;
      }
    }
    if (!_downloadsRuntime) return null;
    return installedLocalAiExecutable(_runtime, artifact, verifyFiles: verifyFiles, cancellation: cancellation);
  }

  static Directory _defaultDirectory() {
    if (Platform.isWindows) {
      final local = Platform.environment['LOCALAPPDATA'];
      if (local != null && local.isNotEmpty) return Directory('$local/ConsoleCrypt/local-ai');
    }
    final home = Platform.environment['HOME'];
    if (home != null && home.isNotEmpty) {
      return Directory(
        '$home/Library/Application Support/ConsoleCrypt/local-ai', // l10n-ignore: platform filesystem path
      );
    }
    // Unsupported mobile platforms never create this directory or install.
    return Directory('${Directory.systemTemp.path}/consolecrypt-local-ai-unsupported');
  }

  void _checkUsable() {
    if (_disposed) throw const LocalAiException('runtime_disposed');
    if (platform == LocalAiPlatform.unsupported) throw const LocalAiException('platform_unsupported');
  }

  void _set(LocalAiPhase phase, {bool? installed, int received = 0, int total = 0, String? message}) {
    _status = LocalAiRuntimeStatus(
      phase: phase,
      isInstalled: installed ?? _status.isInstalled,
      modelId: selectedModel.id,
      downloadedBytes: received,
      totalBytes: total,
      message: message,
    );
    if (!_changes.isClosed) _changes.add(_status);
  }

  Future<void> initialize() {
    if (_initialized) return Future.value();
    return _initializing ??= _initialize()
        .then((_) {
          _initialized = true;
        })
        .catchError((Object _) {
          if (!_disposed) _set(LocalAiPhase.error, installed: false, message: 'local_storage_unavailable');
        })
        .whenComplete(() => _initializing = null);
  }

  Future<void> _initialize() async {
    if (platform == LocalAiPlatform.unsupported || _disposed) return;
    final installed =
        await _runtimeExecutable() != null &&
        _modelFile.existsSync() &&
        await _modelFile.length() == selectedModel.bytes;
    if (!_disposed) _set(installed ? LocalAiPhase.stopped : LocalAiPhase.notInstalled, installed: installed);
  }

  Future<void> selectModel(String id) async {
    _checkUsable();
    if (id != selectedModel.id) throw const LocalAiException('unknown_model');
    await initialize();
    await supportDirectory.create(recursive: true);
    await File('${supportDirectory.path}/selection.json').writeAsString(jsonEncode({'modelId': id}), flush: true);
  }

  Future<void> install() {
    _checkUsable();
    if (_installing != null) return _installing!;
    final cancellation = LocalAiCancellation();
    _downloadCancellation = cancellation;
    return _installing = _install(cancellation).whenComplete(() {
      _installing = null;
      _downloadCancellation = null;
    });
  }

  Future<void> _install(LocalAiCancellation cancellation) async {
    await initialize();
    cancellation.check();
    if (_status.isInstalled) return;
    final artifact = LocalAiCatalog.runtimes[platform]!;
    final runtimeBytes = _downloadsRuntime ? artifact.bytes : 0;
    final total = runtimeBytes + selectedModel.bytes;
    _set(LocalAiPhase.installing, total: total);
    Directory? staging;
    try {
      if (_downloadsRuntime) {
        final archive = await _downloader.download(
          artifact,
          _downloads,
          cancellation: cancellation,
          onProgress: (received, _) => _set(LocalAiPhase.installing, received: received, total: total),
        );
        cancellation.check();
        if (await _runtimeExecutable(verifyFiles: true) == null) {
          staging = Directory('${_runtime.path}.staging-${DateTime.now().microsecondsSinceEpoch}');
          await extractLocalAiRuntime(archive, staging, artifact, platform == LocalAiPlatform.windowsX64);
          cancellation.check();
          if (_runtime.existsSync()) await _runtime.delete(recursive: true);
          await staging.rename(_runtime.path);
          staging = null;
        }
      } else if (await _runtimeExecutable() == null) {
        throw const LocalAiException('bundled_runtime_unavailable');
      }
      final model = await _downloader.download(
        selectedModel,
        _downloads,
        cancellation: cancellation,
        onProgress: (received, _) => _set(LocalAiPhase.installing, received: runtimeBytes + received, total: total),
      );
      cancellation.check();
      await _modelFile.parent.create(recursive: true);
      if (_modelFile.existsSync()) await _modelFile.delete();
      await model.rename(_modelFile.path);
      _modelVerified = true;
      _set(LocalAiPhase.stopped, installed: true);
    } on LocalAiDownloadCancelled {
      _set(LocalAiPhase.notInstalled, installed: false);
      rethrow;
    } catch (_) {
      _set(LocalAiPhase.error, installed: false, message: 'install_failed');
      rethrow;
    } finally {
      if (staging != null && staging.existsSync()) await staging.delete(recursive: true);
      _downloadCancellation = null;
    }
  }

  void cancelInstall() => _downloadCancellation?.cancel();

  Future<LocalAiEndpoint> ensureReady() {
    _checkUsable();
    final stopping = _stopping;
    if (stopping != null) return stopping.then((_) => ensureReady());
    if (_endpoint != null && _process != null && _stopping == null) {
      touch();
      return Future.value(_endpoint);
    }
    if (_starting != null) return _starting!;
    final cancellation = LocalAiCancellation();
    _startCancellation = cancellation;
    return _starting = _start(cancellation).whenComplete(() {
      _starting = null;
      _startCancellation = null;
    });
  }

  /// Keep this lease until a streaming request has ended/cancelled. A long
  /// generation cannot be killed by the idle timer, even if no tokens arrive.
  Future<T> withEndpoint<T>(Future<T> Function(LocalAiEndpoint endpoint) action) async {
    _checkUsable();
    _leases++;
    _idle?.cancel();
    try {
      return await action(await ensureReady());
    } finally {
      _leases--;
      touch();
    }
  }

  void touch() {
    _idle?.cancel();
    if (!_disposed && _leases == 0 && _endpoint != null) {
      _idle = Timer(idleTimeout, () => unawaited(stop()));
    }
  }

  Future<LocalAiEndpoint> _start(LocalAiCancellation cancellation) async {
    final generation = _generation;
    void check() {
      _checkUsable();
      if (generation != _generation) throw const LocalAiException('start_cancelled');
    }

    await initialize();
    check();
    if (_installing != null) throw const LocalAiException('install_in_progress');
    if (!_status.isInstalled) throw const LocalAiException('model_not_installed');
    _set(LocalAiPhase.starting);
    Process? child;
    try {
      final executable = await _runtimeExecutable(verifyFiles: true, cancellation: cancellation);
      if (executable == null ||
          (!_modelVerified && !await LocalAiDownloader.verify(_modelFile, selectedModel, cancellation: cancellation))) {
        _set(LocalAiPhase.error, installed: false, message: 'installed_files_changed');
        throw const LocalAiException('installed_files_changed');
      }
      _modelVerified = true;
      check();
      final listener = await ServerSocket.bind(InternetAddress.loopbackIPv4, 0);
      final port = listener.port;
      await listener.close();
      check();
      final random = Random.secure();
      final token = base64UrlEncode(List<int>.generate(32, (_) => random.nextInt(256))).replaceAll('=', '');
      final env = <String, String>{
        for (final name in ['HOME', 'USERPROFILE', 'LOCALAPPDATA', 'SystemRoot', 'WINDIR', 'TEMP', 'TMP', 'TMPDIR'])
          if (Platform.environment[name] != null) name: Platform.environment[name]!,
        'PATH': Platform.isWindows
            ? '${File(Platform.resolvedExecutable).parent.path};'
                  '${Platform.environment['SystemRoot'] ?? r'C:\Windows'}\\System32'
            : '/usr/bin:/bin',
        'LLAMA_API_KEY': token,
      };
      child = await Process.start(
        '${_runtime.path}/$executable',
        [
          '--model',
          _modelFile.path,
          '--alias',
          selectedModel.id,
          '--host',
          '127.0.0.1',
          '--port',
          '$port',
          '--ctx-size',
          '4096',
          '--parallel',
          '1',
          '--n-gpu-layers',
          platform == LocalAiPlatform.macArm64 ? '99' : '0',
          '--offline',
          '--no-webui',
          '--no-slots',
          '--log-disable',
        ],
        workingDirectory: File('${_runtime.path}/$executable').parent.path,
        environment: env,
        includeParentEnvironment: false,
        runInShell: false,
      );
      _process = child;
      // Never persist model prompts or responses in subprocess logs.
      unawaited(child.stdout.drain<void>());
      unawaited(child.stderr.drain<void>());
      final endpoint = LocalAiEndpoint(
        baseUrl: Uri.parse('http://127.0.0.1:$port/v1'),
        apiKey: token,
        modelId: selectedModel.id,
      );
      var exited = false;
      unawaited(
        child.exitCode.then((_) {
          exited = true;
          if (identical(_process, child)) {
            _process = null;
            _endpoint = null;
            _idle?.cancel();
            if (generation == _generation && !_disposed) _set(LocalAiPhase.error, message: 'runtime_exited');
          }
        }),
      );
      final deadline = DateTime.now().add(startTimeout);
      while (DateTime.now().isBefore(deadline)) {
        check();
        if (exited) throw const LocalAiException('runtime_exited');
        if (await _healthy(endpoint)) {
          check();
          _endpoint = endpoint;
          _set(LocalAiPhase.ready);
          touch();
          return endpoint;
        }
        await Future<void>.delayed(const Duration(milliseconds: 200));
      }
      throw const LocalAiException('runtime_start_timeout');
    } catch (error) {
      if (child != null) await _terminate(child);
      if (identical(_process, child)) _process = null;
      _endpoint = null;
      if (generation == _generation && !_disposed) {
        _set(LocalAiPhase.error, message: error is LocalAiException ? error.code : 'runtime_start_failed');
      }
      rethrow;
    }
  }

  Future<bool> _healthy(LocalAiEndpoint endpoint) async {
    final client = HttpClient()
      ..connectionTimeout = const Duration(seconds: 1)
      ..findProxy = (_) => 'DIRECT';
    try {
      Future<Map<String, dynamic>?> get(String path) async {
        final request = await client.getUrl(endpoint.baseUrl.replace(path: path)).timeout(const Duration(seconds: 1));
        request.followRedirects = false;
        request.headers.set(HttpHeaders.authorizationHeader, 'Bearer ${endpoint.apiKey}');
        final response = await request.close().timeout(const Duration(seconds: 2));
        if (response.statusCode != 200) return null;
        final bytes = <int>[];
        await for (final chunk in response.timeout(const Duration(seconds: 2))) {
          if (bytes.length + chunk.length > 65536) return null;
          bytes.addAll(chunk);
        }
        return jsonDecode(utf8.decode(bytes)) as Map<String, dynamic>;
      }

      if ((await get('/health'))?['status'] != 'ok') return false;
      final models = (await get('/v1/models'))?['data'];
      return models is List && models.any((item) => item is Map && item['id'] == selectedModel.id);
    } catch (_) {
      return false;
    } finally {
      client.close(force: true);
    }
  }

  Future<void> stop() => _stopping ??= _stop().whenComplete(() => _stopping = null);
  Future<void> _stop() async {
    _generation++;
    _startCancellation?.cancel();
    _idle?.cancel();
    _endpoint = null;
    final child = _process;
    _process = null;
    if (child != null || _starting != null) _set(LocalAiPhase.stopping);
    if (child != null) await _terminate(child);
    try {
      await _starting;
    } catch (_) {
      /* explicit cancellation */
    }
    if (!_disposed && _status.phase != LocalAiPhase.installing) {
      _set(
        _status.isInstalled
            ? LocalAiPhase.stopped
            : platform == LocalAiPlatform.unsupported
            ? LocalAiPhase.unsupported
            : LocalAiPhase.notInstalled,
      );
    }
  }

  Future<void> _terminate(Process child) async {
    child.kill();
    try {
      await child.exitCode.timeout(const Duration(seconds: 3));
    } on TimeoutException {
      child.kill(ProcessSignal.sigkill);
      await child.exitCode.timeout(const Duration(seconds: 3));
    }
  }

  Future<void> uninstall() async {
    _checkUsable();
    cancelInstall();
    try {
      await _installing;
    } catch (_) {
      /* remove cancelled partials as well */
    }
    await stop();
    // Never modify the signed macOS application bundle during uninstall.
    for (final directory in [_downloadedRuntime, _downloads, _modelFile.parent]) {
      if (directory.existsSync()) await directory.delete(recursive: true);
    }
    _modelVerified = false;
    _set(LocalAiPhase.notInstalled, installed: false);
  }

  Future<void> dispose() async {
    if (_disposed) return;
    _disposed = true;
    cancelInstall();
    await stop();
    try {
      await _installing;
    } catch (_) {
      /* cancellation */
    }
    await _changes.close();
  }
}
