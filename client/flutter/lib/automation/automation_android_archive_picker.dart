import 'dart:io';

import 'package:consolecrypt/automation/automation_portable_import_session.dart';
import 'package:flutter/services.dart';

final class AutomationArchiveSelection {
  const AutomationArchiveSelection({required this.path, required this.name, required this.discard});

  final String path;
  final String name;
  final Future<void> Function() discard;
}

Future<AutomationArchiveSelection?> pickAndroidAutomationArchive() async {
  final Map<String, Object?>? reply;
  try {
    reply = await const MethodChannel('consolecrypt/android').invokeMapMethod('pickAutomationArchive');
  } on PlatformException catch (error) {
    throw PortableImportFailure(switch (error.code) {
      'archive_format' => PortableImportIssue.archiveFormat,
      'archive_size' => PortableImportIssue.archiveSize,
      'archive_busy' => PortableImportIssue.busy,
      _ => PortableImportIssue.archiveRead,
    });
  }
  if (reply == null) return null;
  final file = File(reply['path']! as String);
  final name = (reply['name'] as String? ?? '').replaceAll(RegExp(r'[\\/\x00-\x1f\x7f]'), '_').trim();
  return AutomationArchiveSelection(
    path: file.path,
    name: name.isEmpty ? 'archive.ccproject' : name,
    discard: () async {
      try {
        await file.delete();
        await file.parent.delete();
      } on FileSystemException {
        // Android can evict app-private cache at any time.
      }
    },
  );
}
