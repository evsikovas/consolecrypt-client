import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:archive/archive.dart';
import 'package:consolecrypt/ai/local/local_ai_archive.dart';
import 'package:consolecrypt/ai/local/local_ai_download.dart';
import 'package:consolecrypt/ai/local/local_ai_runtime.dart';
import 'package:crypto/crypto.dart';
import 'package:flutter_test/flutter_test.dart';

class BlockingDownloader extends LocalAiDownloader {
  int calls = 0;
  final entered = Completer<void>();
  @override
  Future<File> download(
    LocalAiArtifact artifact,
    Directory cache, {
    required LocalAiCancellation cancellation,
    void Function(int, int)? onProgress,
  }) async {
    calls++;
    if (!entered.isCompleted) entered.complete();
    while (!cancellation.isCancelled) {
      await Future<void>.delayed(const Duration(milliseconds: 5));
    }
    cancellation.check();
    throw StateError('unreachable');
  }
}

class ModelFixtureDownloader extends LocalAiDownloader {
  final artifacts = <LocalAiArtifact>[];
  @override
  Future<File> download(
    LocalAiArtifact artifact,
    Directory cache, {
    required LocalAiCancellation cancellation,
    void Function(int, int)? onProgress,
  }) async {
    cancellation.check();
    artifacts.add(artifact);
    await cache.create(recursive: true);
    final model = File('${cache.path}/fixture-model');
    await model.writeAsString('fixture returned by verified downloader');
    onProgress?.call(artifact.bytes, artifact.bytes);
    return model;
  }
}

void main() {
  late Directory root;
  HttpOverrides? previousHttpOverrides;
  setUp(() {
    previousHttpOverrides = HttpOverrides.current;
    HttpOverrides.global = null; // These tests use only an owned loopback fixture.
    root = Directory.systemTemp.createTempSync('cc-local-ai-test-');
  });
  tearDown(() {
    HttpOverrides.global = previousHttpOverrides;
    if (root.existsSync()) root.deleteSync(recursive: true);
  });

  LocalAiArtifact spec(Uri url, List<int> data, {String filename = 'fixture.zip'}) =>
      LocalAiArtifact(url: '$url', sha256: sha256.convert(data).toString(), bytes: data.length, filename: filename);

  Future<HttpServer> serve(Future<void> Function(HttpRequest) handle) async {
    final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    final subscription = server.listen((request) {
      unawaited(handle(request));
    });
    addTearDown(() async {
      await server.close(force: true);
      await subscription.cancel();
    });
    return server;
  }

  const downloader = LocalAiDownloader(allowHttpLoopbackForTests: true);

  test('download publishes only matching size and SHA; verified cache is offline', () async {
    final data = utf8.encode('signed catalog content');
    var requests = 0;
    final server = await serve((request) async {
      requests++;
      request.response.contentLength = data.length;
      request.response.add(data);
      await request.response.close();
    });
    final artifact = spec(Uri.parse('http://127.0.0.1:${server.port}/asset'), data);
    final file = await downloader.download(artifact, root, cancellation: LocalAiCancellation());
    expect(await file.readAsBytes(), data);
    expect(File('${file.path}.part').existsSync(), isFalse);
    await downloader.download(artifact, root, cancellation: LocalAiCancellation());
    expect(requests, 1);
  });

  test('cancel retains partial; resume validates Content-Range and final full digest', () async {
    final data = List<int>.generate(150000, (i) => i % 251);
    var requests = 0;
    String? range;
    final server = await serve((request) async {
      requests++;
      range = request.headers.value(HttpHeaders.rangeHeader);
      if (requests == 1) {
        request.response.contentLength = data.length;
        request.response.add(data.sublist(0, 32768));
        try {
          await request.response.flush();
          await Future<void>.delayed(const Duration(milliseconds: 300));
          await request.response.close();
        } on IOException {
          /* cancelled client */
        }
      } else {
        final offset = int.parse(RegExp(r'bytes=(\d+)-').firstMatch(range!)![1]!);
        request.response.statusCode = 206;
        request.response.headers.set(HttpHeaders.contentRangeHeader, 'bytes $offset-${data.length - 1}/${data.length}');
        request.response.contentLength = data.length - offset;
        request.response.add(data.sublist(offset));
        await request.response.close();
      }
    });
    final artifact = spec(Uri.parse('http://127.0.0.1:${server.port}/asset'), data);
    final cancellation = LocalAiCancellation();
    final failed = expectLater(
      downloader.download(artifact, root, cancellation: cancellation),
      throwsA(isA<LocalAiDownloadCancelled>()),
    );
    final partial = File('${root.path}/${artifact.sha256}.part');
    final deadline = DateTime.now().add(const Duration(seconds: 3));
    while ((!partial.existsSync() || partial.lengthSync() == 0) && DateTime.now().isBefore(deadline)) {
      await Future<void>.delayed(const Duration(milliseconds: 5));
    }
    expect(partial.lengthSync(), greaterThan(0));
    cancellation.cancel();
    await failed;
    expect(File('${root.path}/${artifact.sha256}').existsSync(), isFalse);
    final completed = await downloader.download(artifact, root, cancellation: LocalAiCancellation());
    expect(range, startsWith('bytes='));
    expect(await completed.readAsBytes(), data);
  });

  test('server ignoring Range replaces the partial instead of appending', () async {
    final data = utf8.encode('complete replacement');
    final server = await serve((request) async {
      expect(request.headers.value(HttpHeaders.rangeHeader), 'bytes=4-');
      request.response.contentLength = data.length;
      request.response.add(data);
      await request.response.close();
    });
    final artifact = spec(Uri.parse('http://127.0.0.1:${server.port}/asset'), data);
    File('${root.path}/${artifact.sha256}.part').writeAsBytesSync(data.sublist(0, 4));
    File('${root.path}/${artifact.sha256}.part.json')
        .writeAsStringSync(jsonEncode({'url': artifact.url, 'sha256': artifact.sha256, 'bytes': artifact.bytes}));
    final result = await downloader.download(artifact, root, cancellation: LocalAiCancellation());
    expect(await result.readAsBytes(), data);
  });

  test('digest mismatch cannot publish an installable file', () async {
    final expected = utf8.encode('good');
    final server = await serve((request) async {
      request.response.contentLength = 4;
      request.response.write('evil');
      await request.response.close();
    });
    final artifact = spec(Uri.parse('http://127.0.0.1:${server.port}/asset'), expected);
    await expectLater(
      downloader.download(artifact, root, cancellation: LocalAiCancellation()),
      throwsA(isA<LocalAiException>().having((e) => e.code, 'code', 'download_checksum_mismatch')),
    );
    expect(File('${root.path}/${artifact.sha256}').existsSync(), isFalse);
    expect(File('${root.path}/${artifact.sha256}.part').existsSync(), isFalse);
  });

  test('incomplete chunked response keeps resumable partial without publishing', () async {
    final data = utf8.encode('the entire expected body');
    final server = await serve((request) async {
      request.response.add(data.sublist(0, 4));
      await request.response.close();
    });
    final artifact = spec(Uri.parse('http://127.0.0.1:${server.port}/asset'), data);
    await expectLater(
      downloader.download(artifact, root, cancellation: LocalAiCancellation()),
      throwsA(isA<LocalAiException>().having((e) => e.code, 'code', 'download_incomplete')),
    );
    expect(File('${root.path}/${artifact.sha256}.part').readAsBytesSync(), data.sublist(0, 4));
    expect(File('${root.path}/${artifact.sha256}').existsSync(), isFalse);
  });

  test('incorrect Content-Range never appends to a trusted partial', () async {
    final data = utf8.encode('expected full response');
    final server = await serve((request) async {
      request.response.statusCode = 206;
      request.response.headers.set(HttpHeaders.contentRangeHeader, 'bytes 0-${data.length - 1}/${data.length}');
      request.response.contentLength = data.length;
      request.response.add(data);
      await request.response.close();
    });
    final artifact = spec(Uri.parse('http://127.0.0.1:${server.port}/asset'), data);
    final part = File('${root.path}/${artifact.sha256}.part')..writeAsBytesSync(data.sublist(0, 4));
    File('${part.path}.json')
        .writeAsStringSync(jsonEncode({'url': artifact.url, 'sha256': artifact.sha256, 'bytes': artifact.bytes}));
    await expectLater(
      downloader.download(artifact, root, cancellation: LocalAiCancellation()),
      throwsA(isA<LocalAiException>().having((e) => e.code, 'code', 'download_invalid_range')),
    );
    expect(part.readAsBytesSync(), data.sublist(0, 4));
  });

  test('redirect cannot switch to untrusted host or non-HTTPS remote', () async {
    final server = await serve((request) async {
      request.response.statusCode = 302;
      request.response.headers.set(HttpHeaders.locationHeader, 'http://example.com/payload');
      await request.response.close();
    });
    final artifact = spec(Uri.parse('http://127.0.0.1:${server.port}/asset'), [1]);
    await expectLater(
      downloader.download(artifact, root, cancellation: LocalAiCancellation()),
      throwsA(isA<LocalAiException>().having((e) => e.code, 'code', 'download_untrusted_url')),
    );
  });

  Future<LocalAiArtifact> archiveFixture(Archive archive, {bool tar = false}) async {
    final data = tar ? const GZipEncoder().encode(TarEncoder().encode(archive)) : ZipEncoder().encode(archive);
    File('${root.path}/fixture.zip').writeAsBytesSync(data);
    return spec(Uri.parse('https://github.com/fixture'), data, filename: tar ? 'fixture.tar.gz' : 'fixture.zip');
  }

  test('archive traversal/drive/ADS/backslash paths never escape staging', () async {
    for (final name in ['../outside', '/outside', r'C:\outside', 'dir/../../outside', 'file:stream']) {
      final artifact = await archiveFixture(Archive()..add(ArchiveFile.string(name, 'payload')));
      await expectLater(
        extractLocalAiRuntime(File('${root.path}/fixture.zip'), Directory('${root.path}/stage'), artifact, true),
        throwsA(isA<LocalAiException>()),
      );
      expect(Directory('${root.path}/stage').existsSync(), isFalse);
    }
  });

  test('archive rejects external links and materializes safe sibling aliases', () async {
    final bad = Archive()
      ..add(ArchiveFile.string('llama-server', 'stub'))
      ..add(ArchiveFile.symlink('bad.dylib', '../outside'));
    var artifact = await archiveFixture(bad, tar: true);
    await expectLater(
      extractLocalAiRuntime(File('${root.path}/fixture.zip'), Directory('${root.path}/stage'), artifact, false),
      throwsA(isA<LocalAiException>()),
    );
    expect(Directory('${root.path}/stage').existsSync(), isFalse);
    final good = Archive()
      ..add(ArchiveFile.string('bin/llama-server', 'stub'))
      ..add(ArchiveFile.string('bin/lib-real.dylib', 'library'))
      ..add(ArchiveFile.symlink('bin/lib-alias.dylib', 'lib-real.dylib'));
    artifact = await archiveFixture(good, tar: true);
    final executable = await extractLocalAiRuntime(
      File('${root.path}/fixture.zip'),
      Directory('${root.path}/stage'),
      artifact,
      false,
    );
    expect(executable, 'bin/llama-server');
    final alias = File('${root.path}/stage/bin/lib-alias.dylib');
    expect(alias.readAsStringSync(), 'library');
    expect(FileSystemEntity.typeSync(alias.path, followLinks: false), FileSystemEntityType.file);
    expect(await installedLocalAiExecutable(Directory('${root.path}/stage'), artifact, verifyFiles: true), executable);
    alias.writeAsStringSync('tampered');
    expect(await installedLocalAiExecutable(Directory('${root.path}/stage'), artifact, verifyFiles: true), isNull);
  });

  test('initialize/ensureReady never auto-download; unsupported platforms stay offline', () async {
    final download = BlockingDownloader();
    final runtime = LocalAiRuntime(
      supportDirectory: root,
      platform: LocalAiPlatform.macArm64,
      allowMacRuntimeDownload: true,
      downloader: download,
    );
    await runtime.initialize();
    expect(runtime.status.phase, LocalAiPhase.notInstalled);
    await expectLater(runtime.ensureReady(), throwsA(isA<LocalAiException>()));
    expect(download.calls, 0);
    await runtime.dispose();
    final unsupported = LocalAiRuntime(
      supportDirectory: root,
      platform: LocalAiPlatform.unsupported,
      downloader: download,
    );
    await unsupported.initialize();
    expect(unsupported.status.phase, LocalAiPhase.unsupported);
    expect(() => unsupported.install(), throwsA(isA<LocalAiException>()));
    expect(download.calls, 0);
    await unsupported.dispose();
  });

  test('install is single-flight and cancel/dispose terminate pending work', () async {
    final download = BlockingDownloader();
    final runtime = LocalAiRuntime(
      supportDirectory: root,
      platform: LocalAiPlatform.macArm64,
      allowMacRuntimeDownload: true,
      downloader: download,
    );
    final first = runtime.install(), second = runtime.install();
    expect(identical(first, second), isTrue);
    final assertion = expectLater(first, throwsA(isA<LocalAiDownloadCancelled>()));
    await download.entered.future;
    expect(download.calls, 1);
    runtime.cancelInstall();
    await assertion;
    expect(runtime.status.phase, LocalAiPhase.notInstalled);
    await runtime.dispose();
    expect(() => runtime.ensureReady(), throwsA(isA<LocalAiException>()));
  });

  test('immediate cancellation before initialization performs no download', () async {
    final download = BlockingDownloader();
    final runtime = LocalAiRuntime(
      supportDirectory: root,
      platform: LocalAiPlatform.macArm64,
      allowMacRuntimeDownload: true,
      downloader: download,
    );
    final future = runtime.install();
    runtime.cancelInstall();
    await expectLater(future, throwsA(isA<LocalAiDownloadCancelled>()));
    expect(download.calls, 0);
    await runtime.dispose();
  });

  test('stop during installation is prompt and never auto-starts after download', () async {
    final download = BlockingDownloader();
    final runtime = LocalAiRuntime(
      supportDirectory: root,
      platform: LocalAiPlatform.macArm64,
      allowMacRuntimeDownload: true,
      downloader: download,
    );
    final installation = expectLater(runtime.install(), throwsA(isA<LocalAiDownloadCancelled>()));
    await download.entered.future;
    await expectLater(
      runtime.ensureReady(),
      throwsA(isA<LocalAiException>().having((e) => e.code, 'code', 'install_in_progress')),
    );
    await runtime.stop().timeout(const Duration(seconds: 1));
    expect(runtime.status.phase, LocalAiPhase.installing);
    runtime.cancelInstall();
    await installation;
    await runtime.dispose();
  });

  test('macOS missing or mismatched bundled helper fails before downloading a model', () async {
    final download = BlockingDownloader();
    final bundle = Directory('${root.path}/bundle')..createSync();
    File('${bundle.path}/llama-server').writeAsStringSync('fixture executable');
    final runtime = LocalAiRuntime(
      supportDirectory: Directory('${root.path}/data'),
      platform: LocalAiPlatform.macArm64,
      bundledRuntimeDirectory: bundle,
      downloader: download,
    );
    await expectLater(
      runtime.install(),
      throwsA(isA<LocalAiException>().having((e) => e.code, 'code', 'bundled_runtime_unavailable')),
    );
    File('${bundle.path}/runtime.json').writeAsStringSync(
      jsonEncode({
        'version': LocalAiCatalog.runtimeVersion,
        'platform': 'macArm64',
        'archiveSha256': LocalAiCatalog.runtimes[LocalAiPlatform.macX64]!.sha256,
      }),
    );
    await expectLater(runtime.install(), throwsA(isA<LocalAiException>()));
    expect(download.calls, 0);
    expect(runtime.status.isInstalled, isFalse);
    await runtime.dispose();
  });

  test('macOS install downloads only the model; uninstall preserves its signed helper', () async {
    final download = ModelFixtureDownloader();
    final contents = Directory('${root.path}/Fixture.app/Contents');
    final bundle = Directory('${contents.path}/Helpers/LocalAI/macArm64')..createSync(recursive: true);
    final resources = Directory('${contents.path}/Resources/LocalAI/macArm64')..createSync(recursive: true);
    final executable = File('${bundle.path}/llama-server')..writeAsStringSync('fixture executable');
    final metadata = File('${resources.path}/runtime.json')
      ..writeAsStringSync(
        jsonEncode({
          'version': LocalAiCatalog.runtimeVersion,
          'platform': 'macArm64',
          'archiveSha256': LocalAiCatalog.runtimes[LocalAiPlatform.macArm64]!.sha256,
        }),
      );
    final support = Directory('${root.path}/data');
    final runtime = LocalAiRuntime(
      supportDirectory: support,
      platform: LocalAiPlatform.macArm64,
      bundledRuntimeDirectory: bundle,
      downloader: download,
    );
    final progress = <LocalAiRuntimeStatus>[];
    final subscription = runtime.statuses.listen(progress.add);
    await runtime.install();
    expect(download.artifacts, [LocalAiCatalog.model]);
    expect(runtime.status.isInstalled, isTrue);
    expect(
      progress
          .where((s) => s.phase == LocalAiPhase.installing)
          .every((s) => s.totalBytes == LocalAiCatalog.model.bytes),
      isTrue,
    );
    await runtime.uninstall();
    expect(runtime.status.isInstalled, isFalse);
    expect(Directory('${support.path}/models').existsSync(), isFalse);
    expect(executable.readAsStringSync(), 'fixture executable');
    expect(metadata.existsSync(), isTrue);
    await runtime.dispose();
    await subscription.cancel();
  });
}
