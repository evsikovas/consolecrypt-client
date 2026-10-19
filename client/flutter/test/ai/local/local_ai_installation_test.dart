import 'dart:convert';
import 'dart:io';

import 'package:consolecrypt/ai/local/local_ai_download.dart';
import 'package:consolecrypt/ai/local/local_ai_installation.dart';
import 'package:consolecrypt/ai/local/local_ai_runtime.dart';
import 'package:crypto/crypto.dart';
import 'package:flutter_test/flutter_test.dart';

class _FixtureDownloader extends LocalAiDownloader {
  final artifacts = <LocalAiArtifact>[];
  bool fail = false;
  @override
  Future<File> download(
    LocalAiArtifact artifact,
    Directory cache, {
    required LocalAiCancellation cancellation,
    void Function(int, int)? onProgress,
  }) async {
    artifacts.add(artifact);
    if (fail) throw const LocalAiException('fixture_download_failure');
    await cache.create(recursive: true);
    final file = File('${cache.path}/${artifact.sha256}');
    await file.writeAsString(artifact.filename);
    if (!await LocalAiDownloader.verify(file, artifact)) throw StateError('bad test fixture');
    return file;
  }
}

class _CancelAfterBackup extends LocalAiCancellation {
  int checks = 0;
  @override
  void check() {
    if (++checks == 2) cancel();
    super.check();
  }
}

void main() {
  late Directory root, bundle;
  late List<LocalAiModel> models;
  setUp(() {
    root = Directory.systemTemp.createTempSync('cc-ai-upgrade-test-');
    bundle = Directory('${root.path}/app-bundle')..createSync();
    File('${bundle.path}/llama-server').writeAsStringSync('never executed fixture');
    File('${bundle.path}/runtime.json').writeAsStringSync(
      jsonEncode({
        'version': LocalAiCatalog.runtimeVersion,
        'platform': 'macArm64',
        'archiveSha256': LocalAiCatalog.runtimes[LocalAiPlatform.macArm64]!.sha256,
      }),
    );
    // Tiny owned fixtures retain the real catalog's stable identities/paths.
    models = [
      for (final actual in LocalAiCatalog.models)
        LocalAiModel(
          id: actual.id,
          label: actual.label,
          filename: actual.filename,
          url: 'https://huggingface.co/test/${actual.filename}',
          bytes: utf8.encode(actual.filename).length,
          sha256: sha256.convert(utf8.encode(actual.filename)).toString(),
        ),
    ];
  });
  tearDown(() => root.deleteSync(recursive: true));
  Directory data() => Directory('${root.path}/data');
  LocalAiRuntime runtime(_FixtureDownloader download) => LocalAiRuntime(
    supportDirectory: data(),
    bundledRuntimeDirectory: bundle,
    platform: LocalAiPlatform.macArm64,
    downloader: download,
    models: models,
  );

  for (final cancelled in [false, true]) {
    test('runtime replacement restores original on ${cancelled ? 'cancellation' : 'rename failure'}', () async {
      final destination = Directory('${root.path}/engine')..createSync();
      final original = File('${destination.path}/llama-server')..writeAsStringSync('original engine');
      final staging = Directory('${root.path}/new-engine');
      if (cancelled) {
        staging.createSync();
        File('${staging.path}/llama-server').writeAsStringSync('replacement engine');
      }
      await expectLater(
        replaceLocalAiRuntime(staging, destination, cancelled ? _CancelAfterBackup() : LocalAiCancellation()),
        throwsA(cancelled ? isA<LocalAiDownloadCancelled>() : isA<FileSystemException>()),
      );
      expect(original.readAsStringSync(), 'original engine');
      expect(root.listSync().whereType<Directory>().where((d) => d.path.contains('.local-ai-engine-')), isEmpty);
      expect(staging.existsSync(), cancelled);
    });
  }

  test('successful runtime replacement publishes new engine then removes owned rollback', () async {
    final destination = Directory('${root.path}/engine')..createSync();
    File('${destination.path}/llama-server').writeAsStringSync('old engine');
    final staging = Directory('${root.path}/new-engine')..createSync();
    File('${staging.path}/llama-server').writeAsStringSync('new engine');
    await replaceLocalAiRuntime(staging, destination, LocalAiCancellation());
    expect(File('${destination.path}/llama-server').readAsStringSync(), 'new engine');
    expect(staging.existsSync(), isFalse);
    expect(root.listSync().whereType<Directory>().where((d) => d.path.contains('.local-ai-engine-')), isEmpty);
  });

  test('legacy selection and GGUF survive upgrade without download or rewrite', () async {
    final installation = LocalAiInstallation(data(), models: models);
    final file = installation.modelFile(models.first);
    file.parent.createSync(recursive: true);
    file.writeAsStringSync(models.first.filename);
    final selection = File('${data().path}/selection.json')
      ..writeAsStringSync(jsonEncode({'modelId': models.first.id}));
    final selectionBytes = selection.readAsBytesSync();
    final modified = file.lastModifiedSync();
    final download = _FixtureDownloader()..fail = true;
    final app = runtime(download);
    await app.initialize();
    expect(app.selectedModel.id, models.first.id);
    expect(app.status.isInstalled, isTrue);
    expect(app.status.runtimeInstalled, isTrue);
    await app.install();
    expect(download.artifacts, isEmpty);
    expect(file.readAsStringSync(), models.first.filename);
    expect(file.lastModifiedSync(), modified);
    expect(selection.readAsBytesSync(), selectionBytes);
    await app.dispose();
  });

  test('engine absent does not erase model inventory and repair reuses GGUF', () async {
    final download = _FixtureDownloader();
    var app = runtime(download);
    await app.install();
    await app.dispose();
    File('${bundle.path}/runtime.json').deleteSync();
    app = runtime(download);
    await app.initialize();
    expect(app.status.isInstalled, isTrue);
    expect(app.status.runtimeInstalled, isFalse);
    await expectLater(
      app.ensureReady(),
      throwsA(isA<LocalAiException>().having((e) => e.code, 'code', 'runtime_not_installed')),
    );
    await expectLater(app.install(), throwsA(isA<LocalAiException>()));
    expect(app.status.isInstalled, isTrue);
    expect(app.status.installedModelIds, {models.first.id});
    // A replacement app supplies its own signed engine; model stays in data.
    File('${bundle.path}/runtime.json').writeAsStringSync(
      jsonEncode({
        'version': LocalAiCatalog.runtimeVersion,
        'platform': 'macArm64',
        'archiveSha256': LocalAiCatalog.runtimes[LocalAiPlatform.macArm64]!.sha256,
      }),
    );
    await app.install();
    expect(app.status.runtimeInstalled, isTrue);
    expect(download.artifacts, [models.first]);
    await app.dispose();
  });

  test('three selections persist across restart; uninstall removes only selected model', () async {
    final download = _FixtureDownloader();
    var app = runtime(download);
    for (final model in models) {
      await app.selectModel(model.id);
      await app.install();
    }
    expect(app.status.installedModelIds, models.map((m) => m.id).toSet());
    await app.dispose();
    app = runtime(download);
    await app.initialize();
    expect(app.selectedModel.id, models.last.id);
    final inventory = LocalAiInstallation(data(), models: models);
    inventory.downloads.createSync(recursive: true);
    final otherPartial = File('${inventory.downloads.path}/${models.first.sha256}.part')..writeAsStringSync('resume');
    final selectedPartial = File('${inventory.downloads.path}/${models.last.sha256}.part')..writeAsStringSync('remove');
    await app.uninstall();
    expect(app.status.isInstalled, isFalse);
    expect(app.status.hasInstalledModels, isTrue);
    expect(app.status.installedModelIds, {models[0].id, models[1].id});
    expect(inventory.modelFile(models[2]).existsSync(), isFalse);
    expect(selectedPartial.existsSync(), isFalse);
    expect(otherPartial.readAsStringSync(), 'resume');
    expect(File('${bundle.path}/llama-server').existsSync(), isTrue);
    expect(app.selectedModel.id, models.last.id);
    await app.selectModel(models.first.id);
    await app.install();
    expect(download.artifacts, models, reason: 'switching back reuses an installed GGUF');
    await app.dispose();
    app = runtime(download);
    await app.initialize();
    expect(app.selectedModel.id, models.first.id);
    expect(app.status.isInstalled, isTrue);
    await app.dispose();
  });

  test('failed new model download preserves old models and explicit selection', () async {
    final download = _FixtureDownloader();
    final app = runtime(download);
    await app.install();
    await app.selectModel(models[1].id);
    download.fail = true;
    await expectLater(app.install(), throwsA(isA<LocalAiException>()));
    expect(app.status.isInstalled, isFalse);
    expect(app.status.installedModelIds, {models.first.id});
    expect(app.selectedModel.id, models[1].id);
    expect(
      LocalAiInstallation(data(), models: models).modelFile(models.first).readAsStringSync(),
      models.first.filename,
    );
    await app.dispose();
  });

  test('switching a model cancels a pending process start without deleting its GGUF', () async {
    final executable = File('${bundle.path}/llama-server');
    executable.writeAsStringSync('#!/bin/sh\nexec /bin/sleep 60\n');
    expect((await Process.run('/bin/chmod', ['+x', executable.path])).exitCode, 0);
    final app = runtime(_FixtureDownloader());
    await app.install();
    final startup = expectLater(app.ensureReady(), throwsA(isA<LocalAiException>()));
    // Let the child enter health polling; it deliberately never serves HTTP.
    await Future<void>.delayed(const Duration(milliseconds: 250));
    expect(app.status.phase, LocalAiPhase.starting);
    final oldRevision = app.status.selectionRevision;
    await app.selectModel(models[1].id).timeout(const Duration(seconds: 5));
    await startup;
    expect(app.selectedModel.id, models[1].id);
    expect(app.status.selectionRevision, greaterThan(oldRevision));
    expect(app.status.phase, LocalAiPhase.notInstalled);
    expect(app.status.installedModelIds, {models.first.id});
    expect(LocalAiInstallation(data(), models: models).modelFile(models.first).existsSync(), isTrue);
    await app.dispose();
  }, skip: Platform.isWindows);

  test('unknown/future selection fails closed without overwriting user data', () async {
    data().createSync();
    final selection = File('${data().path}/selection.json')..writeAsStringSync('{"version":2,"modelId":"future"}');
    final app = runtime(_FixtureDownloader());
    await app.initialize();
    expect(app.status.message, 'local_storage_unavailable');
    await expectLater(app.selectModel(models[1].id), throwsA(isA<LocalAiException>()));
    await expectLater(app.uninstall(), throwsA(isA<LocalAiException>()));
    expect(selection.readAsStringSync(), '{"version":2,"modelId":"future"}');
    await app.dispose();
  });

  test('failed atomic selection retains prior id and installed model', () async {
    final inventory = LocalAiInstallation(data(), models: models);
    await inventory.select(models.first);
    final selection = File('${data().path}/selection.json');
    selection.deleteSync();
    Directory(selection.path).createSync(); // Rename fails on every host, even root.
    await expectLater(inventory.select(models[1]), throwsA(isA<FileSystemException>()));
    expect(inventory.selected.id, models.first.id);
    expect(data().listSync().whereType<Directory>().where((d) => d.path.contains('/selection-')), isEmpty);
  });
}
