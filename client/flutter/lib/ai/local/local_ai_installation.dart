import 'dart:convert';
import 'dart:io';

import 'package:consolecrypt/ai/local/local_ai_catalog.dart';
import 'package:consolecrypt/ai/local/local_ai_download.dart';

/// Keep the previous engine until the new verified directory is published.
/// A failed/cancelled second rename restores the original engine in place.
Future<void> replaceLocalAiRuntime(Directory staging, Directory destination, LocalAiCancellation cancellation) async {
  cancellation.check();
  final rollback = await destination.parent.createTemp('.local-ai-engine-');
  final previous = Directory('${rollback.path}/previous');
  var moved = false;
  var published = false;
  try {
    if (destination.existsSync()) {
      await destination.rename(previous.path);
      moved = true;
    }
    cancellation.check();
    await staging.rename(destination.path);
    published = true;
  } catch (_) {
    if (moved) {
      // If restoration itself fails, leave the backup intact for recovery.
      await previous.rename(destination.path);
      moved = false;
    }
    rethrow;
  } finally {
    if (published || !moved) await rollback.delete(recursive: true);
  }
}

/// Device-local model inventory. The catalog and actual files are authoritative;
/// no app/engine version is part of the model path or selection identity.
class LocalAiInstallation {
  LocalAiInstallation(this.root, {this.models = LocalAiCatalog.models}) : _selected = models.first;

  final Directory root;
  final List<LocalAiModel> models;
  LocalAiModel _selected;
  LocalAiModel get selected => _selected;
  final Set<String> _installed = {};
  Set<String> get installedModelIds => Set.unmodifiable(_installed);

  File modelFile(LocalAiModel model) => File('${root.path}/models/${model.filename}');
  Directory get downloads => Directory('${root.path}/downloads');

  LocalAiModel modelById(String id) =>
      models.where((model) => model.id == id).firstOrNull ?? (throw const LocalAiException('unknown_model'));

  Future<void> initialize() async {
    final selection = File('${root.path}/selection.json');
    if (selection.existsSync()) {
      final data = jsonDecode(await selection.readAsString());
      if (data is! Map<String, dynamic> || (data['version'] != null && data['version'] != 1)) {
        throw const LocalAiException('invalid_model_selection');
      }
      _selected = modelById(data['modelId'] as String? ?? '');
    }
    await refresh();
  }

  Future<void> refresh() async {
    final installed = <String>{};
    for (final model in models) {
      final file = modelFile(model);
      if (FileSystemEntity.typeSync(file.path, followLinks: false) == FileSystemEntityType.file &&
          await file.length() == model.bytes) {
        installed.add(model.id);
      }
    }
    _installed
      ..clear()
      ..addAll(installed);
  }

  Future<bool> verify(LocalAiModel model, {LocalAiCancellation? cancellation}) async {
    final file = modelFile(model);
    if (FileSystemEntity.typeSync(file.path, followLinks: false) != FileSystemEntityType.file) return false;
    return LocalAiDownloader.verify(file, model, cancellation: cancellation);
  }

  Future<void> select(LocalAiModel model) async {
    await root.create(recursive: true);
    final temporaryDirectory = await root.createTemp('selection-');
    final temporary = File('${temporaryDirectory.path}/selection.json');
    try {
      await temporary.writeAsString(jsonEncode({'version': 1, 'modelId': model.id}), flush: true);
      await temporary.rename('${root.path}/selection.json');
      _selected = model;
    } finally {
      await temporaryDirectory.delete(recursive: true);
    }
  }

  /// Publish verified downloader output; never delete the destination first.
  Future<void> publish(LocalAiModel model, File download) async {
    final destination = modelFile(model);
    await destination.parent.create(recursive: true);
    await download.rename(destination.path);
    await refresh();
  }

  Future<void> remove(LocalAiModel model) async {
    // Each model owns only its GGUF and digest-named resumable download files.
    // Shared engines, other models, the selected id and app settings survive.
    for (final file in [
      modelFile(model),
      File('${downloads.path}/${model.sha256}'),
      File('${downloads.path}/${model.sha256}.part'),
      File('${downloads.path}/${model.sha256}.part.json'),
    ]) {
      if (FileSystemEntity.typeSync(file.path, followLinks: false) != FileSystemEntityType.notFound) {
        await file.delete();
      }
    }
    await refresh();
  }
}
