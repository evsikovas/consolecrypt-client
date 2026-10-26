import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:consolecrypt/ai/local/local_ai_catalog.dart';
import 'package:consolecrypt/ai/local/local_ai_download.dart';
import 'package:crypto/crypto.dart';

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
  LocalAiInstallation(this.root, {this.catalog = LocalAiCatalog.models}) : _selected = catalog.first;

  final Directory root;
  final List<LocalAiModel> catalog;
  final List<LocalAiModel> _custom = [];

  /// The catalog, then the user's own GGUF models in the order added.
  List<LocalAiModel> get models => [...catalog, ..._custom];
  LocalAiModel _selected;
  LocalAiModel get selected => _selected;
  final Set<String> _installed = {};
  Set<String> get installedModelIds => Set.unmodifiable(_installed);

  File modelFile(LocalAiModel model) => File('${root.path}/models/${model.filename}');
  Directory get downloads => Directory('${root.path}/downloads');

  LocalAiModel modelById(String id) =>
      models.where((model) => model.id == id).firstOrNull ?? (throw const LocalAiException('unknown_model'));

  File get _customList => File('${root.path}/custom-models.json');

  Future<void> initialize() async {
    _custom.clear();
    if (_customList.existsSync()) {
      final data = jsonDecode(await _customList.readAsString());
      if (data is Map<String, dynamic> && data['version'] == 1 && data['models'] is List) {
        for (final entry in data['models'] as List) {
          if (entry case {'label': final String label, 'sha256': final String digest, 'bytes': final int bytes}
              when RegExp(r'^[0-9a-f]{64}$').hasMatch(digest) && bytes > 0) {
            _custom.add(LocalAiModel.custom(label: label, sha256: digest, bytes: bytes));
          }
        }
      }
    }
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
    await _writeAtomically('selection.json', {'version': 1, 'modelId': model.id});
    _selected = model;
  }

  Future<void> _writeAtomically(String name, Object json) async {
    await root.create(recursive: true);
    final temporaryDirectory = await root.createTemp('selection-');
    final temporary = File('${temporaryDirectory.path}/$name');
    try {
      await temporary.writeAsString(jsonEncode(json), flush: true);
      await temporary.rename('${root.path}/$name');
    } finally {
      await temporaryDirectory.delete(recursive: true);
    }
  }

  Future<void> _saveCustom() => _writeAtomically('custom-models.json', {
    'version': 1,
    'models': [
      for (final model in _custom) {'label': model.label, 'sha256': model.sha256, 'bytes': model.bytes},
    ],
  });

  /// Copies a user's GGUF into the models folder, hashing it on the way (no
  /// second read). Only files with the GGUF header (v2/v3) are accepted; the
  /// original is never touched. Adding the same file again reuses it.
  Future<LocalAiModel> importCustom(
    File source, {
    required LocalAiCancellation cancellation,
    void Function(int copied)? onProgress,
  }) async {
    if (FileSystemEntity.typeSync(source.path) != FileSystemEntityType.file || !await isGguf(source)) {
      throw const LocalAiException('invalid_gguf');
    }
    await downloads.create(recursive: true);
    final partial = File('${downloads.path}/import-${DateTime.now().microsecondsSinceEpoch}.part');
    final digests = StreamController<List<int>>();
    final digest = sha256.bind(digests.stream).first;
    var copied = 0;
    try {
      final output = await partial.open(mode: FileMode.write);
      try {
        await for (final chunk in source.openRead()) {
          cancellation.check();
          await output.writeFrom(chunk);
          digests.add(chunk);
          copied += chunk.length;
          onProgress?.call(copied);
        }
        await output.flush();
      } finally {
        await output.close();
        await digests.close();
      }
      final name = source.uri.pathSegments.last;
      final label = name.toLowerCase().endsWith('.gguf') ? name.substring(0, name.length - 5) : name;
      final model = LocalAiModel.custom(
        label: label.length > 80 ? label.substring(0, 80) : label,
        sha256: (await digest).toString(),
        bytes: copied,
      );
      final known = _custom.where((m) => m.id == model.id).firstOrNull;
      if (known != null && await verify(known, cancellation: cancellation)) return known;
      await modelFile(model).parent.create(recursive: true);
      await partial.rename(modelFile(model).path);
      if (known == null) {
        _custom.add(model);
        await _saveCustom();
      }
      await refresh();
      return known ?? model;
    } finally {
      if (partial.existsSync()) await partial.delete();
    }
  }

  /// GGUF magic and a supported container version (llama.cpp reads v2/v3).
  static Future<bool> isGguf(File file) async {
    final raf = await file.open();
    try {
      final head = await raf.read(8);
      if (head.length < 8 || String.fromCharCodes(head.sublist(0, 4)) != 'GGUF') return false;
      final version = head[4] | head[5] << 8 | head[6] << 16 | head[7] << 24;
      return version == 2 || version == 3;
    } finally {
      await raf.close();
    }
  }

  /// Drops a removed custom model from the list; its selection falls back
  /// to the first catalog model.
  Future<void> forget(LocalAiModel model) async {
    if (!model.isCustom) return;
    _custom.removeWhere((m) => m.id == model.id);
    await _saveCustom();
    if (_selected.id == model.id) await select(catalog.first);
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
