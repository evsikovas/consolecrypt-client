import 'dart:convert';
import 'dart:io';
import 'dart:isolate';

import 'package:archive/archive_io.dart';
import 'package:consolecrypt/ai/local/local_ai_catalog.dart';
import 'package:consolecrypt/ai/local/local_ai_download.dart';
import 'package:crypto/crypto.dart';

/// Extract only a digest-pinned runtime into a new, private staging directory.
/// Internal dylib symlinks are materialized as files: no archive-created link
/// can redirect a subsequent write, on either Windows or macOS.
Future<String> extractLocalAiRuntime(File source, Directory destination, LocalAiArtifact artifact, bool windows) =>
    Isolate.run(() => _extract(source.path, destination.path, artifact, windows));

String _safePath(String name) {
  final normalized = name.endsWith('/') ? name.substring(0, name.length - 1) : name;
  if (normalized.isEmpty ||
      !RegExp(r'^[A-Za-z0-9_.\-/]+$').hasMatch(normalized) ||
      normalized.split('/').any((p) => p.isEmpty || p == '.' || p == '..' || p.endsWith('.'))) {
    throw const LocalAiException('archive_unsafe_path');
  }
  return normalized;
}

String _extract(String source, String destination, LocalAiArtifact artifact, bool windows) {
  final bytes = File(source).readAsBytesSync();
  if (bytes.length != artifact.bytes || sha256.convert(bytes).toString() != artifact.sha256) {
    throw const LocalAiException('runtime_checksum_mismatch');
  }
  const maxExpanded = 512 * 1024 * 1024;
  final archive = artifact.filename.endsWith('.zip')
      ? ZipDecoder().decodeBytes(bytes, verify: true)
      : TarDecoder().decodeBytes(
          const GZipDecoder().decodeBytes(bytes, verify: true, maxOutputSize: maxExpanded),
          verify: true,
        );
  final entries = <String, ArchiveFile>{};
  final names = <String>{};
  var expanded = 0;
  for (final entry in archive) {
    final name = _safePath(entry.name);
    if (!names.add(name.toLowerCase())) throw const LocalAiException('archive_duplicate_path');
    if (entry.size < 0 || (expanded += entry.size) > maxExpanded || names.length > 2048) {
      throw const LocalAiException('archive_too_large');
    }
    final kind = entry.mode & 0xf000;
    if (kind != 0 && kind != 0x8000 && kind != 0x4000 && kind != 0xa000) {
      throw const LocalAiException('archive_special_file');
    }
    entries[name] = entry;
  }
  final root = Directory(destination);
  if (root.existsSync()) throw const LocalAiException('archive_destination_exists');
  root.createSync(recursive: true);
  try {
    final manifest = <String, String>{};
    final materialized = <String>{};
    void write(String name, Set<String> chain) {
      if (materialized.contains(name)) return;
      if (!chain.add(name) || chain.length > 16) throw const LocalAiException('archive_link_cycle');
      final entry = entries[name];
      if (entry == null || entry.isDirectory) throw const LocalAiException('archive_invalid_link');
      final file = File('$destination/$name');
      file.parent.createSync(recursive: true);
      if (entry.isSymbolicLink) {
        final target = entry.symbolicLink!;
        // Official dylib aliases refer to a sibling, never a parent or directory.
        if (target.contains('/') || target.contains('\\')) throw const LocalAiException('archive_unsafe_link');
        final slash = name.lastIndexOf('/');
        final relative = _safePath('${slash < 0 ? '' : name.substring(0, slash + 1)}$target');
        write(relative, chain);
        File('$destination/$relative').copySync(file.path);
      } else {
        final output = OutputFileStream(file.path);
        try {
          entry.writeContent(output);
        } finally {
          output.closeSync();
        }
        if (file.lengthSync() != entry.size) throw const LocalAiException('archive_size_mismatch');
      }
      if (!windows &&
          !Platform.isWindows &&
          ((entry.mode & 0x49) != 0 || name.endsWith('/llama-server') || name == 'llama-server')) {
        final result = Process.runSync('/bin/chmod', ['700', file.path]);
        if (result.exitCode != 0) throw const LocalAiException('runtime_permissions');
      }
      manifest[name] = sha256.convert(file.readAsBytesSync()).toString();
      materialized.add(name);
    }

    for (final item in entries.entries) {
      if (item.value.isDirectory) {
        Directory('$destination/${item.key}').createSync(recursive: true);
      } else {
        write(item.key, {});
      }
    }
    final executable = windows ? 'llama-server.exe' : 'llama-server';
    final matches = manifest.keys.where((p) => p.split('/').last == executable).toList();
    if (matches.length != 1) throw const LocalAiException('runtime_executable_missing');
    File('$destination/verified.json').writeAsStringSync(
      jsonEncode({'archive': artifact.sha256, 'executable': matches.single, 'files': manifest}),
      flush: true,
    );
    return matches.single;
  } catch (_) {
    root.deleteSync(recursive: true);
    rethrow;
  }
}

Future<String?> installedLocalAiExecutable(
  Directory runtime,
  LocalAiArtifact artifact, {
  bool verifyFiles = false,
  LocalAiCancellation? cancellation,
}) async {
  try {
    final marker = File('${runtime.path}/verified.json');
    if (!marker.existsSync()) return null;
    final data = jsonDecode(await marker.readAsString()) as Map<String, dynamic>;
    if (data['archive'] != artifact.sha256) return null;
    final executable = _safePath(data['executable'] as String);
    final expectedName = artifact.filename.endsWith('.zip') ? 'llama-server.exe' : 'llama-server';
    if (executable.split('/').last != expectedName) return null;
    if (FileSystemEntity.typeSync('${runtime.path}/$executable', followLinks: false) != FileSystemEntityType.file) {
      return null;
    }
    if (verifyFiles) {
      final files = data['files'] as Map<String, dynamic>;
      if (!files.containsKey(executable) || files.length > 2048) return null;
      for (final entry in files.entries) {
        cancellation?.check();
        final relative = _safePath(entry.key);
        final file = File('${runtime.path}/$relative');
        if (FileSystemEntity.typeSync(file.path, followLinks: false) != FileSystemEntityType.file) return null;
        if ((await sha256
                    .bind(
                      file.openRead().map((chunk) {
                        cancellation?.check();
                        return chunk;
                      }),
                    )
                    .first)
                .toString() !=
            entry.value) {
          return null;
        }
      }
    }
    return executable;
  } on LocalAiDownloadCancelled {
    rethrow;
  } catch (_) {
    return null;
  }
}
